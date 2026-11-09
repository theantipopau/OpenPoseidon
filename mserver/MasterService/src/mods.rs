use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use futures::StreamExt;
use object_store::aws::{AmazonS3, AmazonS3Builder, S3ConditionalPut};
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::signer::Signer;
use object_store::{ObjectStore, PutMode, PutOptions, UpdateVersion, WriteMultipart};
use papa_bear_client::catalog::{is_single_path_component, normalize_folder_name};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{FolderClaimReservation, InstallFolderClaim, ListModsQuery, ModCatalogEntry};

/// A streamed artifact download body: `(size_bytes, byte-stream)`. Lets the HTTP layer
/// stream a mod download straight from the store without buffering the whole artifact.
pub type ArtifactStream =
    futures::stream::BoxStream<'static, std::result::Result<Bytes, object_store::Error>>;

async fn sha256_stream(mut stream: ArtifactStream) -> Result<String> {
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        hasher.update(chunk?);
    }
    Ok(hex(&hasher.finalize()))
}

/// An in-flight streamed upload to a temporary object. The publisher compresses the artifact
/// (`.pbo.zst`) before upload, so the service stores the chunks verbatim — written as they
/// arrive so the process never holds the whole artifact (mods reach tens of GB).
/// `ModStore::finalize_mod` then derives the mod id and moves the temp object into place.
pub struct ArtifactUpload {
    temp: ObjectPath,
    writer: WriteMultipart,
    hasher: Sha256,
    size: u64,
}

impl ArtifactUpload {
    /// Append a chunk verbatim. `WriteMultipart` uploads completed parts in the background, so
    /// memory stays bounded regardless of total size.
    pub fn write(&mut self, chunk: &[u8]) {
        self.writer.write(chunk);
        self.hasher.update(chunk);
        self.size += chunk.len() as u64;
    }
}

const MOD_METADATA_FILE: &str = "mod.json";
const MOD_ARTIFACT_EXT: &str = "pbo.zst";
const CURRENT_FILE: &str = "current";

/// Object prefix holding one claim per install folder: `<CLAIM_PREFIX>/<hex>.json`.
///
/// A catalog entry's install folder is not a store key, so nothing in the object layout keeps
/// two entries apart — this claim is that key. Its name is the folded folder, hex-encoded so any
/// name the component rule accepts (including non-ASCII, `@` and C1 characters) is a valid
/// object key on both backends; the body carries the folder as written and the entry holding it.
const CLAIM_PREFIX: &str = "_claims";
/// Attempts made to replace an object the filesystem briefly refuses to rename over. Eight
/// attempts with a linear back-off hold a publish for at most ~1.4 s, well inside the seconds an
/// artifact upload already takes, while covering a scanner that holds a file for longer than a
/// quarter second.
const REPLACE_ATTEMPTS: u32 = 8;
/// Attempts made to take a claim's write window before answering a conflict. Ten attempts with a
/// short pause hold a reader for at most a few hundred milliseconds, which is the length of the
/// local write the window protects.
const CLAIM_WRITE_ATTEMPTS: u32 = 10;
/// How long a claim writer waits before trying its write window again.
const CLAIM_WRITE_RETRY_MS: u64 = 20;
/// How old a claim's write window must be before another writer may clear it. The window covers a
/// read and one object write, so a marker this old belongs to a master that died holding it.
const CLAIM_WRITE_STALE_MS: u64 = 5_000;
/// How long a reservation holds its folder without being renewed.
///
/// A reservation is taken before the source folder is packed and stays held while the artifact is
/// compressed and streamed, which for a large mod is minutes. The lease is the deadline the sweep
/// judges a reservation by, so the publisher states how long it will keep working instead of the
/// claim's write time implying it has stopped; a publisher renews it while it packs and streams.
const RESERVATION_LEASE_MS: i64 = 10 * 60 * 1000;
/// How often a publisher renews its lease: a third of it, so two renewals may be lost before the
/// sweep could read the reservation as abandoned.
const RESERVATION_RENEW_AFTER_MS: u64 = RESERVATION_LEASE_MS as u64 / 3;
/// The least a lease may have left before a renewal is worth a write. A publisher renews once
/// before it starts packing, moments after the reservation wrote the claim; rewriting an object
/// that was just created is the rename a Windows scanner can refuse, and the deadline would not
/// move far anyway. Renewing a lease this fresh is a no-op success.
const RESERVATION_RENEW_MARGIN_MS: i64 = RESERVATION_LEASE_MS - RESERVATION_RENEW_AFTER_MS as i64;

#[derive(Deserialize, Serialize)]
struct CurrentRevision {
    #[serde(rename = "packageRevision")]
    package_revision: u64,
}

/// The body of an install-folder claim: the catalog directory that holds the folder, the folder
/// as clients will join it (`folderName`, or the `@<modId>` fallback), and — when a revision
/// moves the entry — the folder that revision leaves behind.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct StoredClaim {
    #[serde(rename = "modId")]
    mod_id: String,
    folder: String,
    /// The folder the entry installs into today, written by the move that takes this one before
    /// the new revision is current. A sweep compares it with the catalog's current folder: naming
    /// that folder means this is a move that has not landed yet, and naming any other folder means
    /// no move in flight can explain the claim. Absent on a first publish, and on claims written
    /// before moves recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replaces: Option<String>,
    /// SHA-256 (hex) of the token the publisher reserved this folder with, while the publish that
    /// took the claim has not finished. The token itself is only ever in that publisher's hands:
    /// the claim carries its hash, so only the reservation's owner can commit or abort it. Absent
    /// on a claim a publish took without reserving first (every upload before this ticket), and
    /// removed once the upload that carried the token lands — a claim with no token cannot be
    /// aborted out from under the entry it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    /// Unix milliseconds after which the reservation's lease has run out. Written when a
    /// reservation takes the claim and pushed forward by every renewal, so a publish that packs
    /// and streams for longer than the lease is still visibly alive. Absent on a claim taken
    /// without a reservation, and cleared with the token when the upload consumes it: a claim the
    /// catalog explains is decided by the catalog, not by a deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_until: Option<i64>,
    /// How many times this claim object has been rewritten. Every take, renewal and consume reads
    /// the body and writes it back with this one higher, so a write carries the generation it was
    /// decided against: a body that changed in between (a consume clearing the token, an abort, a
    /// renewal moving the lease) no longer matches, and the write is refused instead of landing on
    /// the newer body. Absent on claims written before generations were recorded, and read as 0.
    #[serde(default)]
    generation: u64,
}

impl StoredClaim {
    /// Whether the reservation this claim records is still being renewed. A claim that never had
    /// a deadline (taken without a reservation) is judged by the sweep's age floor instead.
    fn lease_held(&self, now_unix_ms: i64) -> bool {
        self.lease_until.is_some_and(|until| now_unix_ms < until)
    }

    /// This body as the next generation, which is what a conditional rewrite stores.
    fn next_generation(&self) -> Self {
        Self {
            generation: self.generation.saturating_add(1),
            ..self.clone()
        }
    }
}

/// How a publish takes the claim object for its install folder.
///
/// A publisher that reserves before it streams writes [`ClaimTake::Reserve`]'s token into the
/// claim and sends it back with the upload, which consumes it ([`ClaimTake::Consume`]). A publish
/// that never reserved — an older client, or `publish_mod` — takes the claim with [`ClaimTake::Plain`]
/// and can neither claim a reserved folder nor release one.
enum ClaimTake<'a> {
    Plain,
    Reserve(&'a str),
    Consume(&'a str),
}

impl ClaimTake<'_> {
    /// The hash a fresh claim body records, or `None` when this take leaves no token behind.
    fn token_hash(&self) -> Option<String> {
        if let Self::Reserve(token) = self {
            Some(claim_token_hash(token))
        } else {
            None
        }
    }
}

/// A claim this publish holds on an install folder.
///
/// `owned` records whether this call may delete the object if the publish fails afterwards: it
/// made the object with a conditional put, or it consumed the token a reservation left, which
/// makes the claim this publish's rather than an earlier one's.
struct InstallFolderReservation {
    path: ObjectPath,
    owned: bool,
}

struct CurrentState {
    revision: u64,
    update: Option<UpdateVersion>,
}

/// Inputs for publishing a packed mod to the workshop.
pub struct PublishModInput {
    pub name: String,
    pub app_name: Option<String>,
    pub actver: Option<i32>,
    pub compatible_actvers: Vec<i32>,
    pub version_tag: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub homepage_url: Option<String>,
    pub file: Vec<u8>,
}

/// Catalog metadata for a streamed upload, paired with an `ArtifactUpload` at finalize time.
/// `version` is optional (defaults to the raw content hash); the rest map straight onto the
/// `ModCatalogEntry`.
#[derive(Default)]
pub struct ModUploadMeta {
    pub mod_id: Option<String>,
    pub name: String,
    pub app_name: Option<String>,
    pub actver: Option<i32>,
    pub compatible_actvers: Vec<i32>,
    pub version_tag: Option<String>,
    pub version: Option<String>,
    pub folder_name: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub homepage_url: Option<String>,
    /// The token `POST /v1/catalog/folder-claim` returned, when this upload is finishing a
    /// reservation taken before the artifact was streamed. Consuming it commits the folder.
    pub claim_token: Option<String>,
}

/// One stored value the catalog-name rule rejects, as the `check-names` sweep reports it. Rows
/// are per field, so an entry with both a bad id and a bad folder name lists twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidCatalogName {
    /// The catalog directory (the store key).
    pub directory: String,
    /// The manifest key the value belongs to: `mod_id` or `folderName`.
    pub field: &'static str,
    /// The stored value, verbatim (not trimmed, so the report shows what is actually stored).
    pub value: String,
}

/// Why a catalog entry's current artifact does not match its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactFault {
    /// Nothing at the object a download would read.
    Missing,
    /// The stored `sha256` is not the hash of the object. Hex case is not a difference: the
    /// installer compares with `eq_ignore_ascii_case`, and both values are reported verbatim.
    Sha256Mismatch { expected: String, actual: String },
}

/// One catalog entry whose current artifact is not what its metadata promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamagedArtifact {
    /// The catalog directory (the store key), as a download addresses it.
    pub directory: String,
    /// The entry's current revision: the object this check resolved.
    pub revision: u64,
    pub fault: ArtifactFault,
}

/// One catalog entry's claim on an install folder: the catalog directory, and the folder it
/// resolves to as written (`folderName`, or the `@<modId>` fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallClaim {
    pub directory: String,
    pub folder: String,
}

/// Two or more catalog entries that resolve to one install folder.
///
/// Each entry is unpacked into `<modsRoot>/<folderName>` (`@<modId>` when it carries no folder
/// name), and an install clears that directory before unpacking — so the second entry to install
/// replaces the first one's files instead of sitting beside them. Individually every entry here
/// is valid; only the combination is wrong, which is why a per-entry check cannot see it.
///
/// Folders are grouped case-insensitively, because the installer runs on Windows, where `CSLA`
/// and `csla` are one directory. Each claim keeps its own spelling, so the report can say which
/// entry writes which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCollision {
    /// The entries that share a folder, at least two, in catalog order.
    pub claims: Vec<InstallClaim>,
}

impl InstallClaim {
    /// Resolve an entry's install folder the way the client does: `folderName` verbatim when it
    /// is not blank, otherwise `@<modId>`.
    ///
    /// `None` when the installer would refuse either name, because `ModInstallDir` never joins
    /// it. `directory` is the fallback id for a legacy entry whose stored `modId` is empty, which
    /// is what `parse_metadata` serves.
    fn resolve(directory: &str, mod_id: &str, folder_name: Option<&str>) -> Option<Self> {
        let install_id = if mod_id.is_empty() { directory } else { mod_id };
        let folder = folder_name
            .filter(|folder| !folder.trim().is_empty())
            .map_or_else(|| format!("@{install_id}"), str::to_string);
        if !is_single_path_component(install_id) || !is_single_path_component(&folder) {
            return None;
        }
        Some(Self {
            directory: directory.to_string(),
            folder,
        })
    }
}

/// The key two folders share when a client would install them into one directory: the shipped
/// installer is a Windows client, where `CSLA` and `csla` are one folder.
fn install_folder_key(folder: &str) -> String {
    folder.to_lowercase()
}

/// A fresh reservation token: 32 random bytes, hex, so it names no folder and no entry and cannot
/// be guessed from either.
fn new_claim_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| anyhow::anyhow!("rng failed: {error}"))?;
    Ok(hex(&bytes))
}

/// The hash a claim body records in place of the token the publisher kept.
fn claim_token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

impl InstallCollision {
    /// True when every claim spells the folder the same way: a collision on any filesystem.
    pub fn spells_one_folder(&self) -> bool {
        self.claims
            .first()
            .is_some_and(|first| self.claims.iter().all(|claim| claim.folder == first.folder))
    }

    /// The folder as the first claim writes it (`""` only for an empty, malformed collision).
    pub fn folder(&self) -> &str {
        self.claims
            .first()
            .map_or("", |claim| claim.folder.as_str())
    }
}

/// A claim object no publish needs.
///
/// What a publish leaves behind when it stops between taking its folder and making the entry
/// current, when the release after a move fails, or when a reservation's lease runs out without
/// the upload that would consume it: the folder is refused to every other publish and nothing
/// says a publisher is still working. `check-names` reports it and `--fix` removes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusedClaim {
    /// The `_claims/…` object, for `remove_unused_claims`.
    pub path: ObjectPath,
    /// The entry the claim names, and the folder it holds.
    pub claim: InstallClaim,
    /// Why no entry installs into the folder.
    pub reason: UnusedClaimReason,
    /// When the claim object was written, so a sweep can leave a publish still finishing alone,
    /// and so a re-read can tell a claim written again from the one this scan decided on.
    pub claimed_at: SystemTime,
    /// Unix milliseconds the reservation's publisher set as its deadline, or `None` for a claim
    /// taken without one. The lease — not the write time — is what says a publisher is still
    /// working, however long its pack and upload take.
    pub lease_until: Option<i64>,
    /// The body the scan read. A sweep compares it, and the write time, again immediately before
    /// deleting: the folder may have been released and taken by another publish since.
    body: StoredClaim,
}

impl UnusedClaim {
    /// Whether a publisher is still renewing this claim's reservation.
    pub fn lease_held(&self, now_unix_ms: i64) -> bool {
        self.lease_until.is_some_and(|until| now_unix_ms < until)
    }
}

/// Why a claim object no publish needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnusedClaimReason {
    /// No catalog directory has the claim's `modId`: the publish never wrote its entry.
    NoEntry,
    /// The entry installs into this folder, but the claim carries a reservation token whose lease
    /// ran out: a revision reserved the folder, then stopped renewing (a crash, or a release that
    /// failed). The catalog explains the folder without the claim; the deadline explains the token.
    LapsedReservation,
    /// The claim records that it replaces `from`, which is still the folder the entry installs
    /// into: a move that has taken its destination and has not made the new revision current.
    MovePending { from: String },
    /// The entry exists, but its current revision installs into this folder instead, and the
    /// claim records no move off that folder: a release that failed, or a claim left by a move the
    /// catalog has long since moved past.
    MovedAway { folder: String },
}

impl UnusedClaimReason {
    /// Whether a publish that has not finished can still be the reason the folder is claimed.
    ///
    /// These two can: a first publish takes its folder before writing the entry, and a move takes
    /// its destination before the new revision is current. `MovedAway` cannot — the catalog is
    /// already past the folder, and the only step a move has left there is the delete — so its age
    /// decides nothing a sweep needs. `LapsedReservation` cannot either: its lease is the
    /// publisher's own statement that it is alive, and it has run out.
    pub const fn may_still_be_finishing(&self) -> bool {
        matches!(self, Self::NoEntry | Self::MovePending { .. })
    }
}

/// What a sweep did with the unused claims it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnusedClaimSweep {
    /// The claims deleted from the store.
    pub removed: Vec<ObjectPath>,
    /// The claims left alone because the object was written again after the scan read it: a
    /// publish had taken the folder since, so the folder was not unused by the time the sweep ran.
    pub retaken: Vec<ObjectPath>,
}

/// Result of a `check-names` sweep over the catalog.
///
/// It carries the stored names the rule rejects, the install folders claimed by more than one
/// entry, the current artifacts that are missing or no longer hash to the `sha256` recorded
/// beside them, and the claim objects that no catalog entry needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogScan {
    /// Catalog directories seen. Each one is a mod, whatever its revision count.
    pub entries_checked: usize,
    /// Entries whose current revision metadata was readable, so an artifact could be resolved.
    pub artifacts_checked: usize,
    pub rejected_names: Vec<InvalidCatalogName>,
    pub install_collisions: Vec<InstallCollision>,
    pub damaged_artifacts: Vec<DamagedArtifact>,
    /// Claim objects no catalog entry needs, in listing order.
    pub unused_claims: Vec<UnusedClaim>,
}

impl CatalogScan {
    pub const fn is_clean(&self) -> bool {
        self.rejected_names.is_empty()
            && self.install_collisions.is_empty()
            && self.damaged_artifacts.is_empty()
            && self.unused_claims.is_empty()
    }
}

/// Object-store-backed mod artifact store. Immutable artifacts and revision metadata live under
/// `<modId>/revisions/<revision>/`, with a conditional `current` object selecting the catalog
/// entry. The local filesystem backend serves development and S3 lets stateless replicas share
/// one store.
#[derive(Clone)]
pub struct ModStore {
    store: Arc<dyn ObjectStore>,
    signer: Option<Arc<dyn Signer>>,
    prefix: Option<ObjectPath>,
    signed_url_ttl: Duration,
    current_lock: Arc<tokio::sync::Mutex<()>>,
    artifact_placement: ArtifactPlacement,
}

#[derive(Clone, Copy)]
enum ArtifactPlacement {
    ConditionalRename,
    ReservedRename,
}

pub struct S3StoreConfig<'a> {
    pub bucket: &'a str,
    pub endpoint: &'a str,
    pub region: &'a str,
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub allow_http: bool,
    pub virtual_hosted_style: bool,
    pub prefix: Option<&'a str>,
    pub signed_downloads: bool,
    pub signed_url_ttl: Duration,
}

impl ModStore {
    /// Local-filesystem store rooted at `root` (created if missing). For `cargo`-local dev.
    pub fn local(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)
            .with_context(|| format!("creating mods root {}", root.display()))?;
        let store = LocalFileSystem::new_with_prefix(root)
            .with_context(|| format!("opening local mods store at {}", root.display()))?;
        Ok(Self {
            store: Arc::new(store),
            signer: None,
            prefix: None,
            signed_url_ttl: Duration::from_secs(0),
            current_lock: Arc::new(tokio::sync::Mutex::new(())),
            artifact_placement: ArtifactPlacement::ConditionalRename,
        })
    }

    /// S3 (MinIO-compatible) store. `endpoint` is the MinIO URL (e.g. `http://minio:9000`);
    /// `allow_http` permits the in-cluster plaintext endpoint.
    pub fn s3(config: &S3StoreConfig<'_>) -> Result<Self> {
        let store: AmazonS3 = AmazonS3Builder::new()
            .with_bucket_name(config.bucket)
            .with_endpoint(config.endpoint)
            .with_region(config.region)
            .with_access_key_id(config.access_key)
            .with_secret_access_key(config.secret_key)
            .with_virtual_hosted_style_request(config.virtual_hosted_style)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            // ClientOptions replaces the builder's defaults wholesale, so allow_http must be
            // set here too. Disable the 30s request timeout: a streamed download/upload of a
            // large mod runs longer (bounded by the client's transfer speed), and the default
            // would abort the GET mid-stream — clients then see a truncated "partial file".
            .with_client_options(
                object_store::ClientOptions::new()
                    .with_allow_http(config.allow_http)
                    .with_timeout_disabled(),
            )
            .build()
            .with_context(|| {
                format!(
                    "opening S3 mods store at {}/{}",
                    config.endpoint, config.bucket
                )
            })?;
        let signer = config
            .signed_downloads
            .then(|| Arc::new(store.clone()) as Arc<dyn Signer>);
        Ok(Self {
            store: Arc::new(store),
            signer,
            prefix: normalize_prefix(config.prefix),
            signed_url_ttl: config.signed_url_ttl,
            current_lock: Arc::new(tokio::sync::Mutex::new(())),
            artifact_placement: ArtifactPlacement::ReservedRename,
        })
    }

    async fn place_temp_if_absent(
        &self,
        temp: &ObjectPath,
        destination: &ObjectPath,
    ) -> object_store::Result<()> {
        match self.artifact_placement {
            ArtifactPlacement::ConditionalRename => {
                self.store.rename_if_not_exists(temp, destination).await?;
            }
            ArtifactPlacement::ReservedRename => {
                let reservation = ObjectPath::from(format!("{}.reserved", destination.as_ref()));
                self.store
                    .put_opts(
                        &reservation,
                        Bytes::from(temp.as_ref().as_bytes().to_vec()).into(),
                        PutMode::Create.into(),
                    )
                    .await?;
                self.store.rename(temp, destination).await?;
            }
        }
        Ok(())
    }

    fn artifact_path(mod_id: &str) -> ObjectPath {
        ObjectPath::from(format!("{mod_id}/{mod_id}.{MOD_ARTIFACT_EXT}"))
    }

    fn revision_artifact_path(mod_id: &str, revision: u64) -> ObjectPath {
        ObjectPath::from(format!(
            "{mod_id}/revisions/{revision}/{mod_id}.{MOD_ARTIFACT_EXT}"
        ))
    }

    /// The object a download of `mod_id` revision `revision` would read: the revision artifact,
    /// or the pre-revision `<mod_id>/<mod_id>.pbo.zst` a legacy revision 1 keeps. `None` when
    /// neither exists, so a sweep and the download route cannot disagree about what is served.
    async fn artifact_object(&self, mod_id: &str, revision: u64) -> Result<Option<ObjectPath>> {
        let revision_path = self.store_path(Self::revision_artifact_path(mod_id, revision));
        match self.store.head(&revision_path).await {
            Ok(_) => Ok(Some(revision_path)),
            Err(object_store::Error::NotFound { .. }) if revision == 1 => {
                let legacy_path = self.store_path(Self::artifact_path(mod_id));
                match self.store.head(&legacy_path).await {
                    Ok(_) => Ok(Some(legacy_path)),
                    Err(object_store::Error::NotFound { .. }) => Ok(None),
                    Err(error) => Err(error.into()),
                }
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn metadata_path(mod_id: &str) -> ObjectPath {
        ObjectPath::from(format!("{mod_id}/{MOD_METADATA_FILE}"))
    }

    fn revision_metadata_path(mod_id: &str, revision: u64) -> ObjectPath {
        ObjectPath::from(format!("{mod_id}/revisions/{revision}/{MOD_METADATA_FILE}"))
    }

    fn current_path(mod_id: &str) -> ObjectPath {
        ObjectPath::from(format!("{mod_id}/{CURRENT_FILE}"))
    }

    fn store_path(&self, path: ObjectPath) -> ObjectPath {
        if let Some(prefix) = &self.prefix {
            ObjectPath::from(format!("{}/{}", prefix.as_ref(), path.as_ref()))
        } else {
            path
        }
    }

    fn list_prefix(&self) -> Option<&ObjectPath> {
        self.prefix.as_ref()
    }

    fn strip_prefix<'a>(&self, path: &'a ObjectPath) -> &'a str {
        let path = path.as_ref();
        if let Some(prefix) = &self.prefix {
            path.strip_prefix(prefix.as_ref())
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(path)
        } else {
            path
        }
    }

    /// Store the first immutable package revision under a stable slug identity.
    pub async fn publish_mod(&self, input: &PublishModInput) -> Result<ModCatalogEntry> {
        let slug = slugify(&input.name);
        if slug.is_empty() {
            bail!(
                "mod name '{}' has no usable characters for an id",
                input.name
            );
        }
        if input.file.is_empty() {
            bail!("mod artifact is empty");
        }
        if self.get_mod(&slug).await?.is_some() {
            bail!("mod id '{slug}' already exists");
        }
        // This path carries no `folderName`, so the entry installs into `@<slug>`. Reserve the
        // folder before anything is written under the entry: a concurrent publish of another
        // entry into the same folder is refused by the reservation, not by a read that both
        // publishers can pass.
        let reservation = match InstallClaim::resolve(&slug, &slug, None) {
            Some(claim) => Some(
                self.reserve_install_folder(&claim, None, ClaimTake::Plain)
                    .await?,
            ),
            None => None,
        };
        let result = async {
            let digest = Sha256::digest(&input.file);
            let sha256 = hex(&digest);
            let version = match input.version.as_deref() {
                Some(value) => sanitize_version(value)
                    .ok_or_else(|| anyhow::anyhow!("invalid version '{value}'"))?,
                None => content_hash8(&input.file),
            };
            let size_bytes = input.file.len() as u64;
            let entry = ModCatalogEntry {
                mod_id: slug.clone(),
                app_name: input.app_name.clone(),
                actver: input.actver,
                compatible_actvers: normalize_actvers(&input.compatible_actvers),
                version_tag: input.version_tag.clone(),
                compatible: false,
                name: input.name.clone(),
                version,
                package_revision: 1,
                sha256: Some(sha256),
                published_unix_ms: Some(now_unix_millis()),
                folder_name: None,
                description: input.description.clone().unwrap_or_default(),
                authors: input.authors.clone(),
                homepage_url: input.homepage_url.clone(),
                download_url: Some(format!("/v1/mods/{slug}/revisions/1/download")),
                size_bytes: Some(size_bytes),
            };
            self.write_new_revision(&entry, Bytes::from(input.file.clone()))
                .await?;
            self.create_current(&slug, 1).await?;
            Ok::<ModCatalogEntry, anyhow::Error>(entry)
        }
        .await;
        if result.is_err() {
            self.release_reservation(reservation).await;
        }
        result
    }

    async fn write_new_revision(&self, entry: &ModCatalogEntry, artifact: Bytes) -> Result<()> {
        let artifact_path = self.store_path(Self::revision_artifact_path(
            &entry.mod_id,
            entry.package_revision,
        ));
        self.store
            .put_opts(&artifact_path, artifact.into(), PutMode::Create.into())
            .await
            .with_context(|| {
                format!(
                    "reserving revision {} for {}",
                    entry.package_revision, entry.mod_id
                )
            })?;
        let metadata_path = self.store_path(Self::revision_metadata_path(
            &entry.mod_id,
            entry.package_revision,
        ));
        let json = serde_json::to_vec_pretty(entry)?;
        if let Err(error) = self
            .store
            .put_opts(
                &metadata_path,
                Bytes::from(json).into(),
                PutMode::Create.into(),
            )
            .await
        {
            let _ = self.store.delete(&artifact_path).await;
            return Err(error).with_context(|| {
                format!(
                    "writing revision {} metadata for {}",
                    entry.package_revision, entry.mod_id
                )
            });
        }
        Ok(())
    }

    async fn create_current(&self, mod_id: &str, revision: u64) -> Result<()> {
        let bytes = serde_json::to_vec(&CurrentRevision {
            package_revision: revision,
        })?;
        self.store
            .put_opts(
                &self.store_path(Self::current_path(mod_id)),
                Bytes::from(bytes).into(),
                PutMode::Create.into(),
            )
            .await
            .with_context(|| format!("creating current revision for {mod_id}"))?;
        Ok(())
    }

    async fn set_current(
        &self,
        mod_id: &str,
        revision: u64,
        update: Option<UpdateVersion>,
    ) -> Result<()> {
        let mode = update.map_or(PutMode::Create, PutMode::Update);
        let bytes = serde_json::to_vec(&CurrentRevision {
            package_revision: revision,
        })?;
        let result = self
            .store
            .put_opts(
                &self.store_path(Self::current_path(mod_id)),
                Bytes::from(bytes).into(),
                PutOptions {
                    mode,
                    ..Default::default()
                },
            )
            .await;
        if matches!(&result, Err(object_store::Error::NotImplemented)) {
            let _guard = self.current_lock.lock().await;
            if self
                .current_state(mod_id)
                .await?
                .is_some_and(|state| state.revision >= revision)
            {
                return Ok(());
            }
            let bytes = serde_json::to_vec(&CurrentRevision {
                package_revision: revision,
            })?;
            self.put_replacing(
                &self.store_path(Self::current_path(mod_id)),
                Bytes::from(bytes),
                &format!("advancing {mod_id} to revision {revision}"),
            )
            .await?;
        } else {
            result.with_context(|| format!("advancing {mod_id} to revision {revision}"))?;
        }
        Ok(())
    }

    async fn current_state(&self, mod_id: &str) -> Result<Option<CurrentState>> {
        let path = self.store_path(Self::current_path(mod_id));
        match self.store.get(&path).await {
            Ok(result) => {
                let update = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                let current: CurrentRevision = serde_json::from_slice(&result.bytes().await?)
                    .with_context(|| format!("parsing current revision for {mod_id}"))?;
                Ok(Some(CurrentState {
                    revision: current.package_revision,
                    update: Some(update),
                }))
            }
            Err(object_store::Error::NotFound { .. }) => {
                match self
                    .store
                    .head(&self.store_path(Self::metadata_path(mod_id)))
                    .await
                {
                    Ok(_) => Ok(Some(CurrentState {
                        revision: 1,
                        update: None,
                    })),
                    Err(object_store::Error::NotFound { .. }) => Ok(None),
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Write a catalog entry's `mod.json` (used by dev seeding; writes no artifact, so the
    /// seeded entry carries a synthetic `size_bytes` and isn't downloadable).
    pub async fn put_metadata(&self, entry: &ModCatalogEntry) -> Result<()> {
        let json = serde_json::to_vec_pretty(entry)?;
        self.put_replacing(
            &self.store_path(Self::metadata_path(&entry.mod_id)),
            Bytes::from(json),
            &format!("writing mod.json for {}", entry.mod_id),
        )
        .await?;
        Ok(())
    }

    /// Begin a streamed artifact upload to a unique temporary object.
    pub async fn begin_upload(&self) -> Result<ArtifactUpload> {
        let mut suffix = [0u8; 16];
        getrandom::getrandom(&mut suffix)
            .map_err(|error| anyhow::anyhow!("rng failed: {error}"))?;
        let temp = self.store_path(ObjectPath::from(format!("_incoming/{}.tmp", hex(&suffix))));
        let upload = self
            .store
            .put_multipart(&temp)
            .await
            .with_context(|| format!("starting upload {temp}"))?;
        Ok(ArtifactUpload {
            temp,
            writer: WriteMultipart::new(upload),
            hasher: Sha256::new(),
            size: 0,
        })
    }

    /// Finalize a streamed upload as revision 1 of a stable slug identity.
    pub async fn finalize_mod(
        &self,
        upload: ArtifactUpload,
        mut meta: ModUploadMeta,
    ) -> Result<ModCatalogEntry> {
        let ArtifactUpload {
            temp,
            writer,
            hasher,
            size,
        } = upload;
        // Complete the temp object first so we never leave a dangling multipart, then
        // validate — deleting the temp object on any rejection.
        writer
            .finish()
            .await
            .with_context(|| format!("finishing upload {temp}"))?;

        let slug = meta.mod_id.take().unwrap_or_else(|| slugify(&meta.name));
        let folder_name = normalize_folder_name(meta.folder_name.as_deref());
        if let Err(error) = folder_name {
            let _ = self.store.delete(&temp).await;
            return Err(error.into());
        }
        let folder_name = folder_name.unwrap_or_default();
        if !is_single_path_component(&slug) || size == 0 {
            let _ = self.store.delete(&temp).await;
            if !is_single_path_component(&slug) {
                bail!("invalid mod id '{slug}'");
            }
            bail!("mod artifact is empty");
        }

        if self.get_mod(&slug).await?.is_some() {
            let _ = self.store.delete(&temp).await;
            bail!("mod id '{slug}' already exists");
        }
        // A publisher that reserved its folder before streaming sends the token back with the
        // upload; taking the claim with it proves this upload is that reservation and clears the
        // token, so a late abort cannot release a folder the new entry is about to hold.
        let claim_token = meta.claim_token.take();
        let claim_take = claim_token
            .as_deref()
            .map_or(ClaimTake::Plain, ClaimTake::Consume);
        let reservation = match InstallClaim::resolve(&slug, &slug, folder_name.as_deref()) {
            Some(claim) => match self.reserve_install_folder(&claim, None, claim_take).await {
                Ok(reservation) => Some(reservation),
                Err(error) => {
                    let _ = self.store.delete(&temp).await;
                    return Err(error);
                }
            },
            None => None,
        };
        let result = async {
            let digest = hasher.finalize();
            let version = match meta.version.as_deref() {
                Some(value) => match sanitize_version(value) {
                    Some(version) => version,
                    None => {
                        let _ = self.store.delete(&temp).await;
                        bail!("invalid version '{value}'");
                    }
                },
                None => digest.iter().take(4).map(|b| format!("{b:02x}")).collect(),
            };

            let artifact_path = self.store_path(Self::revision_artifact_path(&slug, 1));
            self.place_temp_if_absent(&temp, &artifact_path)
                .await
                .with_context(|| format!("placing revision 1 artifact for {slug}"))?;

            let entry = ModCatalogEntry {
                mod_id: slug.clone(),
                app_name: meta.app_name,
                actver: meta.actver,
                compatible_actvers: normalize_actvers(&meta.compatible_actvers),
                version_tag: meta.version_tag,
                compatible: false,
                name: meta.name,
                version,
                package_revision: 1,
                sha256: Some(hex(&digest)),
                published_unix_ms: Some(now_unix_millis()),
                folder_name,
                description: meta.description.unwrap_or_default(),
                authors: meta.authors,
                homepage_url: meta.homepage_url,
                download_url: Some(format!("/v1/mods/{slug}/revisions/1/download")),
                size_bytes: Some(size),
            };
            let json = serde_json::to_vec_pretty(&entry)?;
            self.store
                .put_opts(
                    &self.store_path(Self::revision_metadata_path(&slug, 1)),
                    Bytes::from(json).into(),
                    PutMode::Create.into(),
                )
                .await
                .with_context(|| format!("writing revision 1 metadata for {slug}"))?;
            self.create_current(&slug, 1).await?;
            Ok::<ModCatalogEntry, anyhow::Error>(entry)
        }
        .await;
        if result.is_err() {
            self.release_reservation(reservation).await;
        }
        result
    }

    pub async fn add_revision(
        &self,
        mod_id: &str,
        bytes: Vec<u8>,
        meta: ModUploadMeta,
    ) -> Result<ModCatalogEntry> {
        let mut upload = self.begin_upload().await?;
        upload.write(&bytes);
        self.finalize_revision(upload, mod_id, meta).await
    }

    pub async fn finalize_revision(
        &self,
        upload: ArtifactUpload,
        mod_id: &str,
        mut meta: ModUploadMeta,
    ) -> Result<ModCatalogEntry> {
        if !is_single_path_component(mod_id) {
            bail!("invalid mod id '{mod_id}'");
        }
        let ArtifactUpload {
            temp,
            writer,
            hasher,
            size,
        } = upload;
        writer
            .finish()
            .await
            .with_context(|| format!("finishing upload {temp}"))?;
        if size == 0 {
            let _ = self.store.delete(&temp).await;
            bail!("mod artifact is empty");
        }
        let digest = hasher.finalize();
        let sha256 = hex(&digest);
        let Some(mut current) = self.current_state(mod_id).await? else {
            let _ = self.store.delete(&temp).await;
            bail!("mod id '{mod_id}' does not exist");
        };
        let latest = self
            .get_revision(mod_id, current.revision)
            .await?
            .ok_or_else(|| anyhow::anyhow!("current revision for '{mod_id}' is missing"))?;
        if latest.sha256.as_deref() == Some(sha256.as_str()) {
            let _ = self.store.delete(&temp).await;
            return Ok(latest);
        }

        let version = match meta.version.as_deref() {
            Some(value) => sanitize_version(value)
                .ok_or_else(|| anyhow::anyhow!("invalid version '{value}'"))?,
            None => latest.version.clone(),
        };
        let folder_name = normalize_folder_name(
            meta.folder_name
                .as_deref()
                .or(latest.folder_name.as_deref()),
        )?;
        // A revision that keeps its folder skips the catalog collision lookup, so an entry caught
        // in a pair that predates this rule can still publish; taking another entry's folder is
        // refused, and the folder is still reserved for this publish.
        let claim_token = meta.claim_token.take();
        let claim_take = claim_token
            .as_deref()
            .map_or(ClaimTake::Plain, ClaimTake::Consume);
        let previous = InstallClaim::resolve(mod_id, &latest.mod_id, latest.folder_name.as_deref());
        let claim = InstallClaim::resolve(mod_id, mod_id, folder_name.as_deref());
        let reservation = match &claim {
            Some(claim) => match self
                .reserve_install_folder(claim, previous.as_ref(), claim_take)
                .await
            {
                Ok(reservation) => Some(reservation),
                Err(error) => {
                    let _ = self.store.delete(&temp).await;
                    return Err(error);
                }
            },
            None => None,
        };
        // A revision that moves to a new folder has to give the old one back, or the folder it
        // left would stay reserved by an entry that no longer installs there. Only after the new
        // revision is current: a failed publish keeps the folder it still has.
        let superseded = previous.clone().filter(|previous| {
            claim.as_ref().is_some_and(|claim| {
                install_folder_key(&previous.folder) != install_folder_key(&claim.folder)
            })
        });
        let result = async {
            let mut revision = current.revision + 1;
            loop {
                let artifact_path = self.store_path(Self::revision_artifact_path(mod_id, revision));
                match self.place_temp_if_absent(&temp, &artifact_path).await {
                    Ok(()) => break,
                    Err(object_store::Error::AlreadyExists { .. }) => revision += 1,
                    Err(error) => return Err(error.into()),
                }
            }

            let entry = ModCatalogEntry {
                mod_id: mod_id.to_string(),
                app_name: meta.app_name.or(latest.app_name),
                actver: meta.actver.or(latest.actver),
                compatible_actvers: if !meta.compatible_actvers.is_empty() {
                    normalize_actvers(&meta.compatible_actvers)
                } else if let Some(actver) = meta.actver {
                    vec![actver]
                } else {
                    latest.compatible_actvers
                },
                version_tag: meta.version_tag.or(latest.version_tag),
                compatible: false,
                name: if meta.name.trim().is_empty() {
                    latest.name
                } else {
                    meta.name
                },
                version,
                package_revision: revision,
                sha256: Some(sha256),
                published_unix_ms: Some(now_unix_millis()),
                folder_name,
                description: meta.description.unwrap_or(latest.description),
                authors: if meta.authors.is_empty() {
                    latest.authors
                } else {
                    meta.authors
                },
                homepage_url: meta.homepage_url.or(latest.homepage_url),
                download_url: Some(format!("/v1/mods/{mod_id}/revisions/{revision}/download")),
                size_bytes: Some(size),
            };
            let metadata_path = self.store_path(Self::revision_metadata_path(mod_id, revision));
            self.store
                .put_opts(
                    &metadata_path,
                    Bytes::from(serde_json::to_vec_pretty(&entry)?).into(),
                    PutMode::Create.into(),
                )
                .await
                .with_context(|| format!("writing revision {revision} metadata for {mod_id}"))?;
            if self
                .set_current(mod_id, revision, current.update.take())
                .await
                .is_err()
            {
                let state = self.current_state(mod_id).await?.ok_or_else(|| {
                    anyhow::anyhow!("current revision for '{mod_id}' disappeared")
                })?;
                if state.revision < revision {
                    self.set_current(mod_id, revision, state.update).await?;
                }
            }
            Ok::<ModCatalogEntry, anyhow::Error>(entry)
        }
        .await;
        match &result {
            Ok(_) => {
                if let Some(previous) = &superseded {
                    self.release_claim_for(&previous.folder, mod_id).await;
                }
            }
            Err(_) => self.release_reservation(reservation).await,
        }
        result
    }

    /// Stream a published mod's artifact: `(size_bytes, byte-stream)`, or `None` if absent.
    pub async fn artifact_stream(&self, mod_id: &str) -> Result<Option<(u64, ArtifactStream)>> {
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(None);
        };
        self.revision_stream(mod_id, current.revision).await
    }

    pub async fn revision_stream(
        &self,
        mod_id: &str,
        revision: u64,
    ) -> Result<Option<(u64, ArtifactStream)>> {
        if !is_single_path_component(mod_id) {
            return Ok(None);
        }
        let Some(path) = self.artifact_object(mod_id, revision).await? else {
            return Ok(None);
        };
        match self.store.get(&path).await {
            Ok(result) => {
                let size = result.meta.size as u64;
                Ok(Some((size, result.into_stream())))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Signed direct URL for a published mod's artifact, or `None` when the backend is not
    /// externally reachable/signable or the artifact is absent.
    pub async fn signed_artifact_url(&self, mod_id: &str) -> Result<Option<(u64, String)>> {
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(None);
        };
        self.signed_revision_url(mod_id, current.revision).await
    }

    pub async fn signed_revision_url(
        &self,
        mod_id: &str,
        revision: u64,
    ) -> Result<Option<(u64, String)>> {
        if !is_single_path_component(mod_id) {
            return Ok(None);
        }
        let Some(signer) = &self.signer else {
            return Ok(None);
        };
        let Some(path) = self.artifact_object(mod_id, revision).await? else {
            return Ok(None);
        };
        let size = match self.store.head(&path).await {
            Ok(meta) => meta.size as u64,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let url = signer
            .signed_url(Method::GET, &path, self.signed_url_ttl)
            .await?;
        Ok(Some((size, url.to_string())))
    }

    /// Raw bytes of a published mod's artifact, or `None` if absent. Rejects ids that could
    /// escape the store prefix.
    pub async fn artifact_bytes(&self, mod_id: &str) -> Result<Option<Vec<u8>>> {
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(None);
        };
        self.revision_bytes(mod_id, current.revision).await
    }

    pub async fn revision_bytes(&self, mod_id: &str, revision: u64) -> Result<Option<Vec<u8>>> {
        if !is_single_path_component(mod_id) {
            return Ok(None);
        }
        let Some(path) = self.artifact_object(mod_id, revision).await? else {
            return Ok(None);
        };
        match self.store.get(&path).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Delete a mod from the store: its `mod.json` and its artifact. Returns `true` if the
    /// mod existed (its `mod.json` was present). Rejects unsafe ids.
    pub async fn delete_mod(&self, mod_id: &str) -> Result<bool> {
        if !is_single_path_component(mod_id) {
            return Ok(false);
        }
        if self.current_state(mod_id).await?.is_none() {
            return Ok(false);
        }
        let prefix = self.store_path(ObjectPath::from(format!("{mod_id}/")));
        let mut objects = self.store.list(Some(&prefix));
        let mut paths = Vec::new();
        while let Some(object) = objects.next().await {
            paths.push(object?.location);
        }
        for path in paths {
            self.store.delete(&path).await?;
        }
        // The entry is gone, so its folder has to be free again; a claim left behind would
        // reserve a folder no publish can see an owner for.
        self.release_claims_of(mod_id).await?;
        Ok(true)
    }

    pub async fn list_mods(&self, query: &ListModsQuery) -> Result<Vec<ModCatalogEntry>> {
        let mut mods = self.read_all(query).await?;
        apply_mod_filters(&mut mods, query);
        Ok(mods)
    }

    pub async fn get_mod(&self, mod_id: &str) -> Result<Option<ModCatalogEntry>> {
        if !is_single_path_component(mod_id) {
            return Ok(None);
        }
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(None);
        };
        self.get_revision(mod_id, current.revision).await
    }

    pub async fn get_revision(
        &self,
        mod_id: &str,
        revision: u64,
    ) -> Result<Option<ModCatalogEntry>> {
        if !is_single_path_component(mod_id) || revision == 0 {
            return Ok(None);
        }
        let revision_path = self.store_path(Self::revision_metadata_path(mod_id, revision));
        match self.store.get(&revision_path).await {
            Ok(result) => {
                let bytes = result.bytes().await?;
                return Ok(Some(
                    self.parse_metadata(mod_id, revision, &bytes, false).await?,
                ));
            }
            Err(object_store::Error::NotFound { .. }) if revision == 1 => {}
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        match self
            .store
            .get(&self.store_path(Self::metadata_path(mod_id)))
            .await
        {
            Ok(result) => {
                let bytes = result.bytes().await?;
                Ok(Some(self.parse_metadata(mod_id, 1, &bytes, true).await?))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn list_revisions(&self, mod_id: &str) -> Result<Vec<ModCatalogEntry>> {
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(Vec::new());
        };
        let mut revisions = Vec::new();
        for revision in (1..=current.revision).rev() {
            if let Some(entry) = self.get_revision(mod_id, revision).await? {
                revisions.push(entry);
            }
        }
        Ok(revisions)
    }

    pub async fn set_revision_compatibility(
        &self,
        mod_id: &str,
        revision: u64,
        compatible_actvers: &[i32],
    ) -> Result<Option<ModCatalogEntry>> {
        if compatible_actvers.is_empty() || compatible_actvers.iter().any(|value| *value <= 0) {
            bail!("at least one positive compatible game version is required");
        }
        let Some(mut entry) = self.get_revision(mod_id, revision).await? else {
            return Ok(None);
        };
        entry.compatible_actvers = normalize_actvers(compatible_actvers);
        let revision_path = self.store_path(Self::revision_metadata_path(mod_id, revision));
        let metadata_path = match self.store.head(&revision_path).await {
            Ok(_) => revision_path,
            Err(object_store::Error::NotFound { .. }) if revision == 1 => {
                self.store_path(Self::metadata_path(mod_id))
            }
            Err(error) => return Err(error.into()),
        };
        self.put_replacing(
            &metadata_path,
            Bytes::from(serde_json::to_vec_pretty(&entry)?),
            &format!("updating revision {revision} metadata for {mod_id}"),
        )
        .await?;
        Ok(Some(entry))
    }

    /// The store key a folder's claim lives at: one object per folded folder.
    fn claim_path(&self, folder: &str) -> ObjectPath {
        let key = install_folder_key(folder);
        self.store_path(ObjectPath::from(format!(
            "{CLAIM_PREFIX}/{}.json",
            hex(key.as_bytes())
        )))
    }

    /// Read the claim stored at `path` and when the object was written, or `None` when no publish
    /// holds that folder.
    async fn read_claim_state(
        &self,
        path: &ObjectPath,
    ) -> Result<Option<(StoredClaim, SystemTime)>> {
        match self.store.get(path).await {
            Ok(result) => {
                let written = SystemTime::from(result.meta.last_modified);
                let bytes = result.bytes().await?;
                let claim: StoredClaim = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parsing install-folder claim {path}"))?;
                Ok(Some((claim, written)))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Read the claim stored at `path`, or `None` when no publish holds that folder.
    async fn read_claim(&self, path: &ObjectPath) -> Result<Option<StoredClaim>> {
        Ok(self
            .read_claim_state(path)
            .await?
            .map(|(claim, _written)| claim))
    }

    /// The entry holding `claim`'s folder in the reservation index, if it is a different one.
    async fn claim_holder(&self, claim: &InstallClaim) -> Result<Option<InstallClaim>> {
        match self.read_claim(&self.claim_path(&claim.folder)).await? {
            Some(holder) if holder.mod_id != claim.directory => Ok(Some(InstallClaim {
                directory: holder.mod_id,
                folder: holder.folder,
            })),
            _ => Ok(None),
        }
    }

    /// Take the install folder for this publish, atomically.
    ///
    /// Reading the catalog cannot decide between two publishers that both see a free folder:
    /// each finishes its read before either writes. The reservation is one object per folder,
    /// created with a conditional put (`If-None-Match: *` on S3, `hard_link` on the local store),
    /// so exactly one put succeeds and every later attempt reads the winner — including a publish
    /// that is still finishing and has no catalog entry yet.
    ///
    /// Entries published before reservations existed hold their folder without a claim object, so
    /// the read-only lookup still runs first and answers for them; two new claims then serialize
    /// on the put. `previous` is the folder this entry already holds: keeping it is not a new
    /// claim, so it neither needs the lookup nor refuses on another holder (a pair that predates
    /// the rule can still receive revisions, and the sweep keeps reporting that pair); the put is
    /// still attempted, because that is how such an entry acquires the claim object.
    ///
    /// `take` says how the caller relates to the claim: a publisher reserving before it streams
    /// writes its token, the upload that follows consumes it, and a publish that never reserved
    /// leaves the body alone apart from the move record. A claim carrying a token whose lease is
    /// still running is a publish whose artifact is still on its way, so neither another
    /// reservation nor an upload without that token may take it; once the lease runs out it is
    /// nobody's, and the next reservation takes it over.
    async fn reserve_install_folder(
        &self,
        claim: &InstallClaim,
        previous: Option<&InstallClaim>,
        take: ClaimTake<'_>,
    ) -> Result<InstallFolderReservation> {
        let path = self.claim_path(&claim.folder);
        let keeps_folder = previous.is_some_and(|previous| {
            install_folder_key(&previous.folder) == install_folder_key(&claim.folder)
        });
        if !keeps_folder {
            if let Some(other) = self.install_folder_holder(claim, previous).await? {
                bail!("{}", folder_claim_refusal(claim, &other));
            }
        }
        // Written only by a reservation: the upload that follows clears it by consuming it.
        let token = take.token_hash();
        // The publisher's own deadline, pushed forward by every renewal while it packs and streams.
        // A claim taken without a reservation has no lease and stays judged by write time, exactly
        // as before leases existed.
        let lease_until = token
            .is_some()
            .then(|| now_unix_millis() + RESERVATION_LEASE_MS);
        let stored = StoredClaim {
            mod_id: claim.directory.clone(),
            folder: claim.folder.clone(),
            // The folder this publish moves the entry off, which is what a sweep reads to tell a
            // move that has not landed from a claim nothing needs. Keeping a folder leaves none.
            replaces: previous
                .filter(|_| !keeps_folder)
                .map(|previous| previous.folder.clone()),
            token,
            lease_until,
            // The body this claim would hold as its first generation; every later write adds one.
            generation: 1,
        };
        let payload = Bytes::from(serde_json::to_vec(&stored)?);
        // The `Create` takes a free folder; every later attempt re-reads a claim another writer
        // changed and decides again against that body. The generation makes the rewrite
        // conditional, so a decision made about one body cannot land on another.
        for _ in 0..CLAIM_WRITE_ATTEMPTS {
            match self
                .store
                .put_opts(&path, payload.clone().into(), PutMode::Create.into())
                .await
            {
                Ok(_) => return Ok(InstallFolderReservation { path, owned: true }),
                // Another publish got there first, or this entry already holds the folder: what
                // this call may write is decided against the body that is there now.
                Err(object_store::Error::AlreadyExists { .. }) => {}
                Err(error) => return Err(error.into()),
            }
            let Some(holder) = self.read_claim(&path).await? else {
                // Released between the put and the read: the folder is free, but this call did
                // not make the claim and must not act as if it holds one.
                return Ok(InstallFolderReservation { path, owned: false });
            };
            if holder.mod_id != claim.directory {
                if keeps_folder {
                    return Ok(InstallFolderReservation { path, owned: false });
                }
                let holder = InstallClaim {
                    directory: holder.mod_id,
                    folder: holder.folder,
                };
                bail!("{}", folder_claim_refusal(claim, &holder));
            }
            // Our own claim, from an earlier revision or a publish of this entry that already
            // won it. What to write depends on how this call relates to any token it holds.
            let kept = holder.token.as_deref();
            let live = holder.lease_held(now_unix_millis());
            let plain = StoredClaim {
                token: None,
                lease_until: None,
                ..stored.clone()
            };
            let (body, owned) = match &take {
                // A publish that cannot prove it made the reservation must not take the folder
                // from under the publisher that is still streaming into it — including a
                // revision that keeps its folder, whose claim is reserved before it packs.
                ClaimTake::Plain => match kept {
                    Some(_) if live => bail!(
                        "install folder '{}' is reserved by a publish of '{}' that has not finished; upload with its claimToken or try again later",
                        claim.folder,
                        claim.directory
                    ),
                    // The reservation stopped renewing, so it is nobody's now: clearing the
                    // token is what lets this entry publish again.
                    _ => (plain, false),
                },
                // A second reservation must not take over the first's lease: the first would
                // stream an artifact it can only lose at the end, the delay this ticket exists
                // to remove. A lease that ran out is a publisher that stopped renewing, so the
                // entry may reserve its own folder again instead of being refused until an
                // operator edits the store.
                ClaimTake::Reserve(_) => match kept {
                    Some(_) if live => bail!(
                        "{}",
                        folder_reserved_refusal(&claim.folder, &claim.directory)
                    ),
                    Some(_) | None => (stored.clone(), true),
                },
                // The upload for the reservation: the token must be the one in the claim. It is
                // consumed here, before the revision is written, so no abort can remove the claim
                // once the entry installs into the folder.
                ClaimTake::Consume(token) => match kept {
                    Some(current) if current != claim_token_hash(token) => bail!(
                        "install folder '{}' is reserved by another publish of '{}'; this upload no longer holds it",
                        claim.folder,
                        claim.directory
                    ),
                    Some(_) => (plain, true),
                    // Already committed by an explicit commit or a retry: nothing to consume,
                    // and nothing this call may release.
                    None => (plain, false),
                },
            };
            // The body carries the generation it was decided against; a renewal or a consume that
            // landed since then moved it, and the write is refused instead of overwriting it.
            let mut body = body;
            body.generation = holder.generation;
            if body != holder && !self.rewrite_claim(&path, &body).await? {
                // A renewal moved the lease, the upload consumed the token, or another publisher
                // took a lapsed claim: this decision was about a body that is no longer there, so
                // make it again against the one that won.
                continue;
            }
            return Ok(InstallFolderReservation { path, owned });
        }
        bail!(
            "install folder '{}' is being claimed by several publishes of '{}'; try again",
            claim.folder,
            claim.directory
        )
    }

    /// Write a claim body over the object already holding that folder, if that object still holds
    /// the generation this body was decided against.
    ///
    /// `claim.generation` is the generation the caller read. The write stores the body with that
    /// number one higher, and only lands while the stored body still carries the number it was
    /// read at: a renewal that read a token-bearing claim therefore cannot write it back after the
    /// upload consumed it, and two renewals cannot both move a lease from the same body. A `false`
    /// answer means the claim changed, and the caller decides again from what is stored now.
    async fn rewrite_claim(&self, path: &ObjectPath, claim: &StoredClaim) -> Result<bool> {
        if !self.lock_claim(path).await? {
            return Ok(false);
        }
        let outcome = match self.read_claim(path).await {
            Ok(Some(current)) if current.generation == claim.generation => self
                .put_replacing(
                    path,
                    Bytes::from(serde_json::to_vec(&claim.next_generation())?),
                    &format!("updating install-folder claim {path}"),
                )
                .await
                .map(|()| true),
            // Gone, or rewritten after the caller read it: the decision was about a body that is
            // no longer there.
            Ok(_) => Ok(false),
            Err(error) => Err(error),
        };
        self.unlock_claim(path).await?;
        outcome
    }

    /// Delete the claim at `path` while `removable` still accepts the body stored there.
    ///
    /// The body and its write time are read again with the claim's write window held, so a
    /// renewal, a consume or a takeover that landed since the caller decided is seen, and the
    /// delete is refused instead of removing the claim of a publisher that just took it.
    async fn delete_claim_if(
        &self,
        path: &ObjectPath,
        removable: impl Fn(&StoredClaim, SystemTime) -> bool,
    ) -> Result<bool> {
        if !self.lock_claim(path).await? {
            return Ok(false);
        }
        let outcome = match self.read_claim_state(path).await {
            Ok(Some((body, written))) if !removable(&body, written) => Ok(false),
            Ok(_) => match self.store.delete(path).await {
                // Gone is gone: another release of the same claim got there first.
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(true),
                Err(error) => Err(error.into()),
            },
            Err(error) => Err(error),
        };
        self.unlock_claim(path).await?;
        outcome
    }

    /// Take a claim's write window, so a read-check-write on it is one step for every writer that
    /// goes through this store.
    ///
    /// `object_store` 0.11 has no conditional update on the local backend, and its S3 `Update`
    /// mode is a per-backend code path. A marker object taken with the same `If-None-Match: *` /
    /// `hard_link` rule the claim itself uses is therefore one rule for both backends. A marker
    /// left behind by a master that died inside the window is cleared once it is older than the
    /// window could last, so a crash cannot freeze a folder for good.
    async fn lock_claim(&self, path: &ObjectPath) -> Result<bool> {
        let marker = claim_write_marker(path);
        for attempt in 1..=CLAIM_WRITE_ATTEMPTS {
            match self
                .store
                .put_opts(
                    &marker,
                    Bytes::from_static(b"").into(),
                    PutMode::Create.into(),
                )
                .await
            {
                Ok(_) => return Ok(true),
                // The marker is a new name, so a refusal is a scanner holding the directory rather
                // than a rename over an object; wait it out like a marker another writer holds.
                Err(error) if attempt < REPLACE_ATTEMPTS && is_refused_rename(&error) => {
                    tokio::time::sleep(Duration::from_millis(CLAIM_WRITE_RETRY_MS)).await;
                }
                Err(object_store::Error::AlreadyExists { .. }) => {
                    if self.claim_marker_expired(&marker).await? {
                        match self.store.delete(&marker).await {
                            Ok(()) | Err(object_store::Error::NotFound { .. }) => continue,
                            Err(error) => return Err(error.into()),
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(CLAIM_WRITE_RETRY_MS)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(false)
    }

    /// Release a claim's write window, so the next writer may take it.
    ///
    /// Best effort: the body the window protected has already been written, so a marker a scanner
    /// holds open must not turn that write into a reported failure — a claim whose token the
    /// publisher never received would hold the folder with nobody able to release it. A marker
    /// left behind blocks other writers only until it is older than [`CLAIM_WRITE_STALE_MS`].
    async fn unlock_claim(&self, path: &ObjectPath) -> Result<()> {
        let marker = claim_write_marker(path);
        let mut attempt = 1;
        loop {
            match self.store.delete(&marker).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => return Ok(()),
                Err(error) if attempt < REPLACE_ATTEMPTS && is_refused_rename(&error) => {
                    tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt))).await;
                    attempt += 1;
                }
                Err(_) => return Ok(()),
            }
        }
    }

    /// Whether a write window was left behind by a writer that stopped inside it.
    async fn claim_marker_expired(&self, marker: &ObjectPath) -> Result<bool> {
        match self.store.head(marker).await {
            Ok(meta) => Ok(SystemTime::now()
                .duration_since(SystemTime::from(meta.last_modified))
                .unwrap_or(Duration::ZERO)
                >= Duration::from_millis(CLAIM_WRITE_STALE_MS)),
            // Released between the failed create and this read: the next attempt can take it.
            Err(object_store::Error::NotFound { .. }) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    /// Replace an object's bytes, retrying a rename the filesystem briefly refuses.
    ///
    /// The local backend replaces an object by renaming over it, and Windows answers that rename
    /// with `ACCESS_DENIED` while another process (a virus scanner, the search indexer) holds the
    /// freshly written file open. Every caller writes the whole body it already computed, so a
    /// short retry is the same write; without it a scanner's timing becomes a publish failure.
    /// `context` names the object in the error the caller returns.
    async fn put_replacing(&self, path: &ObjectPath, payload: Bytes, context: &str) -> Result<()> {
        let mut attempt = 1;
        let error = loop {
            match self.store.put(path, payload.clone().into()).await {
                Ok(_) => return Ok(()),
                Err(error) if attempt < REPLACE_ATTEMPTS && is_refused_rename(&error) => {
                    tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt))).await;
                    attempt += 1;
                }
                Err(error) => break error,
            }
        };
        Err(error).with_context(|| context.to_string())
    }

    /// Give back a reservation this publish owns because the publish it guarded failed.
    ///
    /// A claim this call created, or one whose token it consumed, is this publish's: leaving it
    /// would refuse the folder to everyone with no entry and no artifact coming. A claim written
    /// by an earlier publish of this entry is never removed here.
    async fn release_reservation(&self, reservation: Option<InstallFolderReservation>) {
        let Some(reservation) = reservation.filter(|reservation| reservation.owned) else {
            return;
        };
        if let Err(error) = self.store.delete(&reservation.path).await {
            tracing::warn!(
                path = %reservation.path,
                %error,
                "releasing an install-folder reservation failed"
            );
        }
    }

    /// Drop the claim for a folder this entry has moved off, when the claim is this entry's.
    async fn release_claim_for(&self, folder: &str, mod_id: &str) {
        let path = self.claim_path(folder);
        match self.read_claim(&path).await {
            Ok(Some(holder)) if holder.mod_id == mod_id => {
                if let Err(error) = self.store.delete(&path).await {
                    tracing::warn!(
                        %path,
                        %error,
                        "releasing a superseded install-folder reservation failed"
                    );
                }
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(
                %path,
                %error,
                "reading a superseded install-folder reservation failed"
            ),
        }
    }

    /// Drop every claim this entry holds. Called on delete, or the entry's folder would stay
    /// reserved by a catalog entry that no longer exists.
    async fn release_claims_of(&self, mod_id: &str) -> Result<()> {
        let prefix = self.store_path(ObjectPath::from(format!("{CLAIM_PREFIX}/")));
        let mut paths = Vec::new();
        let mut listing = self.store.list(Some(&prefix));
        while let Some(object) = listing.next().await {
            let location = match object {
                Ok(object) => object.location,
                // Nothing claimed yet. The local backend skips the missing directory and S3
                // returns an empty listing, but a backend that reports it must not fail a delete.
                Err(object_store::Error::NotFound { .. }) => break,
                Err(error) => return Err(error.into()),
            };
            // A concurrently written reservation stages as `<name>#<n>`; only finished claim
            // objects are readable.
            if !location.filename().is_some_and(is_claim_file) {
                continue;
            }
            match self.read_claim(&location).await? {
                Some(holder) if holder.mod_id == mod_id => paths.push(location),
                _ => {}
            }
        }
        for path in paths {
            // Conditional like every other claim delete: a renewal that read this claim before the
            // entry was deleted is not overwritten by removing it here. A claim a writer still
            // holds is left to the sweep, which finds an entry that no longer exists.
            self.delete_claim_if(&path, |holder, _written| holder.mod_id == mod_id)
                .await?;
        }
        Ok(())
    }

    /// Claim objects no catalog entry needs.
    ///
    /// A publish takes its folder's claim and only then makes the entry current, and a revision
    /// that moves an entry takes the new folder's claim — recording the folder it leaves in that
    /// claim — and releases the old one only after the new revision is current. A publish that
    /// stopped in between (or whose release failed) leaves an object that refuses a folder to
    /// everyone with no entry installing there.
    ///
    /// The folder a claim records it replaces is what tells those apart from a move still running:
    /// a claim on a folder the entry does not install into, which names any other folder as the
    /// one it replaces, cannot belong to a move in flight, so a sweep can act on it at once. One
    /// that names the entry's current folder is exactly a move between taking its destination and
    /// writing its revision — `MovePending` — and only its age says whether that publisher is
    /// still alive. The one state no record separates is a claim left on a folder the entry has
    /// since moved back to, whose body an in-flight move would write identically: that one waits
    /// out the age floor too.
    ///
    /// `current_folders` maps every catalog directory to the folder its current revision resolves
    /// to, or `None` when that metadata was unreadable: a claim naming a directory outside the map
    /// is orphaned, and one whose entry this sweep could not read is left alone, because nothing
    /// says which folder the entry uses.
    async fn unused_claims(
        &self,
        current_folders: &std::collections::BTreeMap<String, Option<String>>,
    ) -> Result<Vec<UnusedClaim>> {
        let prefix = self.store_path(ObjectPath::from(format!("{CLAIM_PREFIX}/")));
        let mut unused = Vec::new();
        let now = now_unix_millis();
        let mut listing = self.store.list(Some(&prefix));
        while let Some(object) = listing.next().await {
            let meta = match object {
                Ok(meta) => meta,
                // Nothing is claimed: the local backend skips a missing directory and S3 returns
                // an empty listing, but a backend that reports it must not fail a sweep.
                Err(object_store::Error::NotFound { .. }) => break,
                Err(error) => return Err(error.into()),
            };
            // A concurrent put stages as `<name>#<n>` on the local backend; only a finished claim
            // is one, and an object listed but deleted before the read is simply gone.
            if !meta.location.filename().is_some_and(is_claim_file) {
                continue;
            }
            let Some(claim) = self.read_claim(&meta.location).await? else {
                continue;
            };
            let reason = match current_folders.get(&claim.mod_id) {
                // The publish never wrote its entry.
                None => UnusedClaimReason::NoEntry,
                Some(Some(folder)) => {
                    if install_folder_key(folder) == install_folder_key(&claim.folder) {
                        // The entry installs here, so the claim is normally the folder's record.
                        // A token on it is a reservation for the next revision; once its lease has
                        // run out the publisher stopped renewing and the catalog supplies the
                        // folder without it.
                        if claim.token.is_some() && !claim.lease_held(now) {
                            UnusedClaimReason::LapsedReservation
                        } else {
                            continue;
                        }
                    } else {
                        match claim.replaces.as_deref() {
                            // A move onto this folder, off the one the entry still installs into:
                            // its revision is not current yet.
                            Some(replaces)
                                if install_folder_key(replaces) == install_folder_key(folder) =>
                            {
                                UnusedClaimReason::MovePending {
                                    from: folder.clone(),
                                }
                            }
                            _ => UnusedClaimReason::MovedAway {
                                folder: folder.clone(),
                            },
                        }
                    }
                }
                // The entry's metadata was unreadable: nothing says which folder it uses.
                Some(None) => continue,
            };
            unused.push(UnusedClaim {
                path: meta.location,
                claim: InstallClaim {
                    directory: claim.mod_id.clone(),
                    folder: claim.folder.clone(),
                },
                reason,
                claimed_at: SystemTime::from(meta.last_modified),
                lease_until: claim.lease_until,
                body: claim,
            });
        }
        Ok(unused)
    }

    /// Delete the unused claims a publish cannot still need, and answer with what the sweep did.
    ///
    /// Only a claim a running publish can still explain waits for `min_age` — one whose entry has
    /// not been written yet, and one a move has taken without making its revision current. There,
    /// removing a fresh claim would hand its folder to a second publisher while the first is still
    /// working, the race the claim exists to decide. A claim the catalog has already moved past is
    /// decided: no entry installs where it stands and no move describes it, so its age is beside
    /// the point and it goes at once. A sweep passes a few minutes; a test passes zero.
    ///
    /// A reservation's lease outranks the age floor: a publisher that keeps renewing is still
    /// packing or streaming, however long ago it took the claim, and deleting its folder would
    /// hand it to a second publisher — exactly the race the reservation exists to decide. Only a
    /// claim whose lease has run out (or that never had one) falls back to the age floor, which is
    /// how a reservation whose publisher crashed is reclaimed. A lapsed reservation on the folder
    /// the entry installs into is a decision rather than a guess: the catalog supplies the folder
    /// without the claim, so it goes at once.
    ///
    /// The scan is a snapshot, and the folder it found unused can be released and taken by another
    /// publish before the delete. Each claim is therefore read again first: a different body, or a
    /// write newer than the one the scan saw, means the folder has a publisher the scan never
    /// judged, so the object is left and answered as retaken.
    pub async fn remove_unused_claims(
        &self,
        claims: &[UnusedClaim],
        min_age: Duration,
    ) -> Result<UnusedClaimSweep> {
        let now = SystemTime::now();
        let now_ms = now_unix_millis();
        let mut sweep = UnusedClaimSweep::default();
        for unused in claims {
            if unused.lease_held(now_ms) {
                continue;
            }
            if unused.reason.may_still_be_finishing() {
                let age = now
                    .duration_since(unused.claimed_at)
                    .unwrap_or(Duration::ZERO);
                if age < min_age {
                    continue;
                }
            }
            // The body is read again with the claim's write window held, so a reservation taken
            // after the scan, or a renewal that moved the lease, is seen and the delete refused.
            match self
                .delete_claim_if(&unused.path, |body, written| {
                    // Written after the scan, or holding a body it never read: the folder has a
                    // publisher the scan did not judge, so the delete is not the scan's to make.
                    body == &unused.body && written <= unused.claimed_at
                })
                .await
            {
                // Gone is gone: another sweep, or a release of the same claim, got there first.
                Ok(true) => sweep.removed.push(unused.path.clone()),
                Ok(false) => sweep.retaken.push(unused.path.clone()),
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("removing unused install-folder claim {}", unused.path)
                    });
                }
            }
        }
        Ok(sweep)
    }

    /// Answer whether a publish could claim an install folder, without writing anything.
    ///
    /// This is the read-only twin of `reserve_install_folder`: the publisher sees only its own
    /// manifest, so it asks the catalog before it packs the source folder and streams the
    /// artifact. The answer — including the refusal wording — is the one the upload would give,
    /// but it writes nothing, so two publishers can both pass it. `papa publish` reserves
    /// (`reserve_folder_claim`) and falls back to this answer only against a master that predates
    /// the reservation.
    ///
    /// `mod_id` is the entry the publish would use and `folder_name` its manifest value (trimmed
    /// by the caller). A revision that names no folder keeps the one it already holds, exactly as
    /// `finalize_revision` decides, and is exempt because keeping a folder is not a new claim; a
    /// new entry falls back to `@<mod_id>`.
    pub async fn check_install_folder(
        &self,
        mod_id: &str,
        folder_name: Option<&str>,
    ) -> Result<InstallFolderClaim> {
        if !is_single_path_component(mod_id) {
            bail!("invalid mod id '{mod_id}'");
        }
        let (claim, previous) = self.resolve_claim(mod_id, folder_name).await?;
        let keeps_folder = previous.as_ref().is_some_and(|previous| {
            install_folder_key(&previous.folder) == install_folder_key(&claim.folder)
        });
        // The reservation is what the upload will contend for, so it answers first; an entry
        // published before reservations existed still answers through the read-only lookup.
        let holder = if keeps_folder {
            // A revision that keeps its folder is refused by its own entry's live reservation —
            // the second publisher of one entry — which a claim held by another directory cannot
            // report.
            match self.read_claim(&self.claim_path(&claim.folder)).await? {
                Some(held) if held.lease_held(now_unix_millis()) => Some(InstallClaim {
                    directory: held.mod_id,
                    folder: held.folder,
                }),
                _ => None,
            }
        } else {
            match self.claim_holder(&claim).await? {
                Some(holder) => Some(holder),
                None => {
                    self.install_folder_holder(&claim, previous.as_ref())
                        .await?
                }
            }
        };
        Ok(InstallFolderClaim {
            mod_id: mod_id.to_string(),
            folder: claim.folder.clone(),
            available: holder.is_none(),
            claimed_by: holder.as_ref().map(|other| other.directory.clone()),
            refusal: holder.map(|other| {
                if other.directory == claim.directory {
                    folder_reserved_refusal(&claim.folder, &claim.directory)
                } else {
                    folder_claim_refusal(&claim, &other)
                }
            }),
        })
    }

    /// Reserve the install folder a publish is about to use, before the artifact is packed and
    /// streamed.
    ///
    /// `papa publish` calls this before packing (a large mod packs for minutes) and before a byte
    /// of the artifact is sent. The upload refuses the same claim, but only once the whole
    /// compressed body has arrived — so of two publishers that both pass the read-only preflight,
    /// the loser streams an artifact it can only lose at the end. This writes the claim now and
    /// answers the token that holds it, so the loser is refused at the reservation instead, and
    /// the winner sends the token back with its upload.
    ///
    /// A revision that keeps its folder used to reserve nothing, on the argument that the folder
    /// is already the entry's. That is true of other entries and false of the entry itself: two
    /// publishes of one entry could both pass the read-only answer and both stream. Keeping a
    /// folder now takes the same claim and the same token as any other publish; the claim already
    /// exists, so the reservation rewrites its token and lease. The lease is what keeps a crashed
    /// publisher from refusing its own entry forever: a lease that ran out is taken over by the
    /// next reservation, and the sweep reports the lapsed claim meanwhile.
    ///
    /// A reservation is given a lease ([`RESERVATION_LEASE_MS`]) and answers how often to renew it,
    /// because the pack and transfer it covers can outlast any fixed age: the sweep reads a live
    /// lease instead of guessing from the claim's write time.
    pub async fn reserve_folder_claim(
        &self,
        mod_id: &str,
        folder_name: Option<&str>,
    ) -> Result<FolderClaimReservation> {
        if !is_single_path_component(mod_id) {
            bail!("invalid mod id '{mod_id}'");
        }
        let (claim, previous) = self.resolve_claim(mod_id, folder_name).await?;
        let token = new_claim_token()?;
        self.reserve_install_folder(&claim, previous.as_ref(), ClaimTake::Reserve(&token))
            .await?;
        Ok(FolderClaimReservation {
            mod_id: mod_id.to_string(),
            folder: claim.folder,
            renew_after_ms: Some(RESERVATION_RENEW_AFTER_MS),
            token: Some(token),
        })
    }

    /// Push a reservation's lease forward while its publisher is still packing or streaming.
    ///
    /// The reservation is taken before the source folder is packed and held until the upload lands,
    /// which for a large mod can take far longer than [`RESERVATION_LEASE_MS`]. A sweep that judged
    /// the claim only by its write time would then take a live reservation for an abandoned one;
    /// the lease is the publisher's own deadline, and this is how it moves. Only the token that
    /// took the claim may renew it, so a second publish cannot keep another's reservation alive.
    ///
    /// A token-less claim is one the upload already consumed (or committed): there is nothing left
    /// to renew, and answering that as success is what makes a renewal that raced the upload
    /// harmless. A renewal that read the token before the consume is refused by the claim's
    /// generation instead of writing the token back, so the folder it guarded is not refused to
    /// the next revision for a lease nobody holds.
    pub async fn renew_folder_claim(&self, mod_id: &str, folder: &str, token: &str) -> Result<()> {
        let path = self.claim_path(folder);
        for _ in 0..CLAIM_WRITE_ATTEMPTS {
            let Some((mut stored, _written)) = self.read_claim_state(&path).await? else {
                bail!("no install-folder reservation is held for '{folder}'");
            };
            if stored.mod_id != mod_id {
                bail!(
                    "folder '{folder}' is reserved by catalog entry '{}', not '{mod_id}'",
                    stored.mod_id
                );
            }
            match stored.token.as_deref() {
                Some(current) if current == claim_token_hash(token).as_str() => {
                    let now = now_unix_millis();
                    // A lease that still has more than two thirds of its life is what the
                    // reservation (or the previous renewal) just wrote: extend it only once it has
                    // aged enough to be worth the write, which keeps the first renewal a request
                    // and no rewrite.
                    if stored
                        .lease_until
                        .is_none_or(|until| until - now <= RESERVATION_RENEW_MARGIN_MS)
                    {
                        stored.lease_until = Some(now + RESERVATION_LEASE_MS);
                        // The write is conditional on the body this renewal read: if the upload
                        // consumed the token meanwhile, it is refused instead of writing the token
                        // back, and the lease is gone with it.
                        if self.rewrite_claim(&path, &stored).await? {
                            return Ok(());
                        }
                        // The claim changed under this renewal. Reading it again is what decides
                        // whether there is still a lease to move: a consume answers success, an
                        // abort answers that no reservation is held.
                        continue;
                    }
                    return Ok(());
                }
                None => return Ok(()),
                Some(_) => {
                    bail!("the reservation for folder '{folder}' belongs to another publish")
                }
            }
        }
        bail!("the reservation for folder '{folder}' changed while it was being renewed; try again")
    }

    /// Keep the folder a reservation holds once its publisher has written the revision.
    ///
    /// The upload that carries the token consumes it, so a successful publish has already
    /// committed by the time it answers; this call then finds a token-less claim and succeeds,
    /// which is what makes it safe to send unconditionally after a publish. What it must never do
    /// is clear a token another publish holds, so that is refused.
    pub async fn commit_folder_claim(&self, mod_id: &str, folder: &str, token: &str) -> Result<()> {
        let path = self.claim_path(folder);
        // Commit says the revision is written. Before that, the only step left in the reservation
        // is a publish that has not happened, and clearing the token would take away the abort that
        // could still give the folder back.
        let installs_here = match self.stored_entry(mod_id).await? {
            Some((_, entry)) => {
                InstallClaim::resolve(mod_id, &entry.mod_id, entry.folder_name.as_deref())
                    .is_some_and(|current| {
                        install_folder_key(&current.folder) == install_folder_key(folder)
                    })
            }
            None => false,
        };
        if !installs_here {
            bail!("catalog entry '{mod_id}' does not install into folder '{folder}' yet");
        }
        for _ in 0..CLAIM_WRITE_ATTEMPTS {
            let Some((mut stored, _written)) = self.read_claim_state(&path).await? else {
                bail!("no install-folder reservation is held for '{folder}'");
            };
            if stored.mod_id != mod_id {
                bail!(
                    "folder '{folder}' is reserved by catalog entry '{}', not '{mod_id}'",
                    stored.mod_id
                );
            }
            match stored.token.as_deref() {
                Some(current) if current == claim_token_hash(token).as_str() => {
                    stored.token = None;
                    stored.lease_until = None;
                    // Conditional like a renewal: a renewal that moved the lease between the read
                    // and this write is not overwritten with a body that never saw it.
                    if self.rewrite_claim(&path, &stored).await? {
                        return Ok(());
                    }
                }
                // Already consumed by the upload, or by an earlier commit: the folder is the
                // entry's.
                None => return Ok(()),
                Some(_) => {
                    bail!("the reservation for folder '{folder}' belongs to another publish")
                }
            }
        }
        bail!(
            "the reservation for folder '{folder}' changed while it was being committed; try again"
        )
    }

    /// Give the folder back before anything was written, so another publish can take it at once
    /// instead of waiting out the sweep's age floor.
    ///
    /// Only the reservation's own token can release it. A revision that keeps its folder reserves
    /// the folder its entry already installs into, so the catalog cannot be the test here: once
    /// the upload has made the revision current the token is gone, and a caller holding the old
    /// one finds nothing to match. The delete is conditional too: the body is read again with the
    /// claim's write window held, so a reservation another publish took after this one lapsed is
    /// left alone and the caller is told the folder belongs to that publish.
    pub async fn abort_folder_claim(&self, mod_id: &str, folder: &str, token: &str) -> Result<()> {
        let path = self.claim_path(folder);
        let expected = claim_token_hash(token);
        for _ in 0..CLAIM_WRITE_ATTEMPTS {
            let Some((stored, _written)) = self.read_claim_state(&path).await? else {
                bail!("no install-folder reservation is held for '{folder}'");
            };
            if stored.mod_id != mod_id {
                bail!(
                    "folder '{folder}' is reserved by catalog entry '{}', not '{mod_id}'",
                    stored.mod_id
                );
            }
            if stored.token.as_deref() != Some(expected.as_str()) {
                bail!("the reservation for folder '{folder}' belongs to another publish");
            }
            // The delete is conditional on the token still being the one this caller reserved
            // with: a renewal that moved the lease in between is deleted with it, which is what
            // the caller asked for, while a takeover by another publish is left alone.
            if self
                .delete_claim_if(&path, |current, _written| {
                    current.mod_id == mod_id && current.token.as_deref() == Some(expected.as_str())
                })
                .await?
            {
                return Ok(());
            }
        }
        bail!(
            "the reservation for folder '{folder}' changed while it was being released; try again"
        )
    }

    /// Resolve the claim a publish of `mod_id` would take, and the folder its current revision
    /// installs into. `folder_name` is the manifest value (trimmed by the caller): absent means
    /// the `@<mod_id>` fallback for a new entry, or the folder the entry already holds.
    async fn resolve_claim(
        &self,
        mod_id: &str,
        folder_name: Option<&str>,
    ) -> Result<(InstallClaim, Option<InstallClaim>)> {
        let current = self.stored_entry(mod_id).await?;
        let folder_name = match current.as_ref() {
            Some((_, entry)) => folder_name.or(entry.folder_name.as_deref()),
            None => folder_name,
        };
        let folder_name = normalize_folder_name(folder_name)?;
        let claim = InstallClaim::resolve(mod_id, mod_id, folder_name.as_deref())
            .ok_or_else(|| anyhow::anyhow!("invalid install folder for '{mod_id}'"))?;
        let previous = current.as_ref().and_then(|(_, entry)| {
            InstallClaim::resolve(mod_id, &entry.mod_id, entry.folder_name.as_deref())
        });
        Ok((claim, previous))
    }

    /// The other entry that already installs into `claim`'s folder, if any.
    ///
    /// A claim that matches the folder this entry already holds is not a new claim, so it skips
    /// the lookup and the entry is never blocked by itself (or by a pair that predates the rule).
    async fn install_folder_holder(
        &self,
        claim: &InstallClaim,
        previous: Option<&InstallClaim>,
    ) -> Result<Option<InstallClaim>> {
        let folder = install_folder_key(&claim.folder);
        if previous.is_some_and(|previous| install_folder_key(&previous.folder) == folder) {
            return Ok(None);
        }
        for directory in self.catalog_directories().await? {
            if directory == claim.directory {
                continue;
            }
            let Some((_, entry)) = self.stored_entry(&directory).await? else {
                continue;
            };
            let Some(other) =
                InstallClaim::resolve(&directory, &entry.mod_id, entry.folder_name.as_deref())
            else {
                continue;
            };
            if install_folder_key(&other.folder) == folder {
                return Ok(Some(other));
            }
        }
        Ok(None)
    }

    /// Sweep the catalog for entries the service should not be serving: stored names the current
    /// one-component rule rejects, and current artifacts that are missing or no longer hash to
    /// the `sha256` recorded beside them.
    ///
    /// `list_mods` cannot answer the name half: it resolves every directory through `get_mod`,
    /// which *refuses* a bad `mod_id` and so hides exactly the entries this looks for. The sweep
    /// enumerates directory names from the listing and reads each current entry without entry
    /// validation (and without the legacy-metadata rewrite `parse_metadata` performs). The
    /// artifact half resolves the object through `artifact_object`, the same helper a download
    /// uses, so a legacy revision 1 is read from its pre-revision path instead of being reported
    /// missing. A legacy entry that predates `sha256` has nothing stored to compare against, so
    /// only its existence is checked — `parse_metadata` backfills the hash on the next read, and
    /// a sweep must not write.
    ///
    /// Only the current revision is checked: older revisions are history, but the current one is
    /// what `/v1/mods` serves and what a client installs next.
    ///
    /// The claim half is the reservation index: a claim object naming a directory the catalog does
    /// not have, or naming one whose current revision installs into another folder, holds a folder
    /// nobody can publish into and no entry installs from.
    ///
    /// The install folder is the one thing checked *across* entries. `papa install` and the
    /// engine's `ModInstallDir` join `folderName` verbatim and fall back to `@<modId>` when the
    /// entry carries none, and the install transaction clears the destination first — so two
    /// entries naming one folder silently replace each other. An entry whose names the rule
    /// rejects cannot install at all (already reported), so it cannot collide with one that can.
    pub async fn scan_catalog(&self) -> Result<CatalogScan> {
        let mut scan = CatalogScan::default();
        // Install folder, folded to lower case, -> the entries claiming it. The fold is the
        // Windows installer's view, where `CSLA` and `csla` are one directory; the claims keep
        // their own spelling for the report. Filled during the single pass so a collision cannot
        // see a different catalog than the name and artifact checks.
        let mut claims: std::collections::BTreeMap<String, Vec<InstallClaim>> =
            std::collections::BTreeMap::new();
        let catalog = self.catalog_directories().await?;
        // The folder each directory's current revision installs into, filled by the pass below.
        // `None` marks an entry whose metadata is unreadable: the claim half leaves those alone,
        // because it cannot say which folder the entry needs.
        let mut current_folders: std::collections::BTreeMap<String, Option<String>> =
            catalog.iter().map(|name| (name.clone(), None)).collect();
        for directory in &catalog {
            let directory = directory.clone();
            scan.entries_checked += 1;
            if !is_single_path_component(&directory) {
                scan.rejected_names.push(InvalidCatalogName {
                    directory: directory.clone(),
                    field: "mod_id",
                    value: directory.clone(),
                });
            }
            let Some((revision, entry)) = self.stored_entry(&directory).await? else {
                continue;
            };
            // An empty stored id is a legacy entry: `parse_metadata` derives it from the
            // directory, so there is nothing stored to reject.
            if !entry.mod_id.is_empty()
                && entry.mod_id != directory
                && !is_single_path_component(&entry.mod_id)
            {
                scan.rejected_names.push(InvalidCatalogName {
                    directory: directory.clone(),
                    field: "mod_id",
                    value: entry.mod_id.clone(),
                });
            }
            if let Some(folder) = entry.folder_name.as_deref() {
                let trimmed = folder.trim();
                // The service stores the trimmed value, so surrounding whitespace is already a
                // name the current rule would not have written, even when the rest is fine.
                if trimmed != folder || (!trimmed.is_empty() && !is_single_path_component(trimmed))
                {
                    scan.rejected_names.push(InvalidCatalogName {
                        directory: directory.clone(),
                        field: "folderName",
                        value: folder.to_string(),
                    });
                }
            }

            // The folder a client would unpack this entry into, resolved exactly as the installer
            // resolves it. An entry that cannot be installed is left out of the collision map
            // entirely, because `ModInstallDir` refuses it before any join.
            if let Some(claim) =
                InstallClaim::resolve(&directory, &entry.mod_id, entry.folder_name.as_deref())
            {
                current_folders.insert(directory.clone(), Some(claim.folder.clone()));
                claims
                    .entry(install_folder_key(&claim.folder))
                    .or_default()
                    .push(claim);
            }

            scan.artifacts_checked += 1;
            let Some(path) = self.artifact_object(&directory, revision).await? else {
                scan.damaged_artifacts.push(DamagedArtifact {
                    directory,
                    revision,
                    fault: ArtifactFault::Missing,
                });
                continue;
            };
            let Some(expected) = entry.sha256 else {
                continue;
            };
            let result = self.store.get(&path).await?;
            let actual = sha256_stream(result.into_stream()).await?;
            if !actual.eq_ignore_ascii_case(&expected) {
                scan.damaged_artifacts.push(DamagedArtifact {
                    directory,
                    revision,
                    fault: ArtifactFault::Sha256Mismatch { expected, actual },
                });
            }
        }
        // A BTreeMap gives deterministic folder order; each group keeps the catalog's directory
        // order, because `catalog_directories` is a BTreeSet.
        scan.install_collisions = claims
            .into_values()
            .filter(|claims| claims.len() > 1)
            .map(|claims| InstallCollision { claims })
            .collect();
        // Read against the same directories and entries as the pass above, so a claim cannot be
        // called unused from a snapshot older or newer than the one just checked.
        scan.unused_claims = self.unused_claims(&current_folders).await?;
        Ok(scan)
    }

    /// Directory names under the store prefix that carry a catalog entry, taken from the
    /// `mod.json` / `current` markers in the listing.
    async fn catalog_directories(&self) -> Result<std::collections::BTreeSet<String>> {
        let mut directories = std::collections::BTreeSet::new();
        let mut listing = self.store.list(self.list_prefix());
        while let Some(meta) = listing.next().await {
            let location = meta?.location;
            if location.filename() == Some(MOD_METADATA_FILE)
                || location.filename() == Some(CURRENT_FILE)
            {
                let relative = self.strip_prefix(&location);
                if let Some((mod_id, _)) = relative.split_once('/') {
                    directories.insert(mod_id.to_string());
                }
            }
        }
        Ok(directories)
    }

    /// Read a directory's current revision and entry without validating its names and without
    /// changing it — a sweep must not rewrite legacy metadata the way `parse_metadata` does.
    async fn stored_entry(&self, directory: &str) -> Result<Option<(u64, ModCatalogEntry)>> {
        let Some(current) = self.current_state(directory).await? else {
            return Ok(None);
        };
        for path in [
            Self::revision_metadata_path(directory, current.revision),
            Self::metadata_path(directory),
        ] {
            match self.store.get(&self.store_path(path)).await {
                Ok(result) => {
                    let bytes = result.bytes().await?;
                    let entry = serde_json::from_slice(&bytes)
                        .with_context(|| format!("parsing mod.json for {directory}"))?;
                    return Ok(Some((current.revision, entry)));
                }
                Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    async fn read_all(&self, query: &ListModsQuery) -> Result<Vec<ModCatalogEntry>> {
        let mut mods = Vec::new();
        for mod_id in self.catalog_directories().await? {
            let entry = if has_compatibility_filter(query) {
                self.latest_compatible_revision(&mod_id, query).await?
            } else {
                self.get_mod(&mod_id).await?
            };
            if let Some(entry) = entry {
                mods.push(entry);
            }
        }

        mods.sort_by(|lhs, rhs| {
            lhs.name
                .to_lowercase()
                .cmp(&rhs.name.to_lowercase())
                .then_with(|| lhs.mod_id.cmp(&rhs.mod_id))
        });
        Ok(mods)
    }

    async fn latest_compatible_revision(
        &self,
        mod_id: &str,
        query: &ListModsQuery,
    ) -> Result<Option<ModCatalogEntry>> {
        let Some(current) = self.current_state(mod_id).await? else {
            return Ok(None);
        };
        for revision in (1..=current.revision).rev() {
            if let Some(entry) = self.get_revision(mod_id, revision).await? {
                if is_mod_compatible(&entry, query) {
                    return Ok(Some(entry));
                }
            }
        }
        Ok(None)
    }

    async fn parse_metadata(
        &self,
        dir_id: &str,
        revision: u64,
        bytes: &[u8],
        legacy: bool,
    ) -> Result<ModCatalogEntry> {
        let mut metadata: ModCatalogEntry = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing mod.json for {dir_id}"))?;
        let mut changed = false;

        if metadata.mod_id.is_empty() {
            metadata.mod_id = dir_id.to_string();
            changed = true;
        }
        if metadata.package_revision != revision {
            changed = true;
        }
        metadata.package_revision = revision;

        let artifact_path = if legacy {
            Self::artifact_path(&metadata.mod_id)
        } else {
            Self::revision_artifact_path(&metadata.mod_id, revision)
        };
        let stored_artifact_path = self.store_path(artifact_path);
        let artifact_head = match self.store.head(&stored_artifact_path).await {
            Ok(head) => Some(head),
            Err(object_store::Error::NotFound { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        if metadata.size_bytes.unwrap_or(0) == 0 {
            if let Some(head) = &artifact_head {
                metadata.size_bytes = Some(head.size as u64);
                changed = true;
            }
        }
        if metadata.sha256.is_none() && artifact_head.is_some() {
            let result = self.store.get(&stored_artifact_path).await?;
            metadata.sha256 = Some(sha256_stream(result.into_stream()).await?);
            changed = true;
        }
        if legacy && changed {
            self.put_replacing(
                &self.store_path(Self::metadata_path(dir_id)),
                Bytes::from(serde_json::to_vec_pretty(&metadata)?),
                &format!("updating legacy metadata for {dir_id}"),
            )
            .await?;
        }
        if artifact_head.is_some() {
            metadata.download_url = Some(format!(
                "/v1/mods/{}/revisions/{revision}/download",
                metadata.mod_id
            ));
        }

        Ok(metadata)
    }
}

fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn normalize_prefix(prefix: Option<&str>) -> Option<ObjectPath> {
    prefix
        .map(str::trim)
        .map(|value| value.trim_matches('/'))
        .filter(|value| !value.is_empty())
        .map(ObjectPath::from)
}

pub(crate) fn slugify(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut out = String::new();
    let mut pending_dash = false;
    // NFKD decomposes accented letters into base + combining mark (Č -> C + caron), so the
    // base survives the ascii filter and the mark is dropped — i.e. transliterate ČSLA -> csla.
    for ch in name.nfkd() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else if ('\u{0300}'..='\u{036f}').contains(&ch) {
            // Combining diacritic from NFKD — skip it without breaking the word.
        } else {
            pending_dash = true;
        }
    }
    out
}

/// The refusal two entries in one folder produce, worded once so the upload and the
/// `folder-claim` preflight cannot report the same conflict differently.
fn folder_claim_refusal(claim: &InstallClaim, other: &InstallClaim) -> String {
    if other.folder == claim.folder {
        format!(
            "folder '{}' is already claimed by catalog entry '{}'; publish with a different folderName",
            claim.folder, other.directory
        )
    } else {
        format!(
            "folder '{}' is already claimed by catalog entry '{}' as '{}' (one folder on a case-insensitive filesystem); publish with a different folderName",
            claim.folder, other.directory, other.folder
        )
    }
}

/// The refusal a second publish of one entry meets while the first holds its folder's claim.
///
/// Worded once so the reservation, the upload's preflight and any other reader of the claim
/// cannot describe the same wait differently.
fn folder_reserved_refusal(folder: &str, directory: &str) -> String {
    format!(
        "install folder '{folder}' is already reserved by another publish of '{directory}'; try again once it finishes or releases the folder"
    )
}

fn sanitize_version(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 64 {
        return None;
    }
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .then(|| value.to_string())
}

fn content_hash8(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The object a claim's write window is held in: `<hex>.json.writing`.
///
/// It is not a claim (`is_claim_file` accepts only `<hex>.json`), so neither a sweep nor a
/// release ever mistakes a window another master is holding for a publisher's hold on a folder.
fn claim_write_marker(path: &ObjectPath) -> ObjectPath {
    ObjectPath::from(format!("{path}.writing"))
}

/// True for a finished claim object's file name, `<hex>.json`. Excludes the local backend's
/// staged uploads (`<hex>.json#1`), which exist only while a concurrent put is running.
fn is_claim_file(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// True when the store refused a write because the destination is open elsewhere.
///
/// The local backend replaces an object by renaming over it, and Windows answers that rename
/// with `ERROR_ACCESS_DENIED` while another process (a virus scanner, the search indexer) holds
/// the freshly written file open. The I/O error sits at different depths depending on where the
/// backend failed, so the whole chain is walked.
fn is_refused_rename(error: &object_store::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            return io.kind() == std::io::ErrorKind::PermissionDenied;
        }
        source = current.source();
    }
    false
}

fn normalize_actvers(values: &[i32]) -> Vec<i32> {
    let mut values = values
        .iter()
        .copied()
        .filter(|value| *value > 0)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn has_compatibility_filter(query: &ListModsQuery) -> bool {
    query.app_name.is_some() || query.actver.is_some() || query.version_tag.is_some()
}

fn apply_mod_filters(mods: &mut Vec<ModCatalogEntry>, query: &ListModsQuery) {
    let text_filter = query.q.as_ref().map(|value| value.to_lowercase());

    mods.retain_mut(|entry| {
        entry.compatible = is_mod_compatible(entry, query);
        if (query.app_name.is_some() || query.actver.is_some() || query.version_tag.is_some())
            && !entry.compatible
        {
            return false;
        }

        if let Some(filter) = &text_filter {
            let haystacks = [
                entry.mod_id.to_lowercase(),
                entry.name.to_lowercase(),
                entry.description.to_lowercase(),
            ];
            if !haystacks.iter().any(|value| value.contains(filter)) {
                return false;
            }
        }

        true
    });

    if let Some(limit) = query.limit {
        mods.truncate(limit);
    }
}

fn is_mod_compatible(entry: &ModCatalogEntry, query: &ListModsQuery) -> bool {
    if let Some(app_name) = query.app_name.as_deref() {
        if let Some(entry_app) = entry.app_name.as_deref() {
            if entry_app != app_name {
                return false;
            }
        }
    }
    if let Some(actver) = query.actver {
        if !entry.compatible_actvers.is_empty() {
            if !entry.compatible_actvers.contains(&actver) {
                return false;
            }
        } else if entry
            .actver
            .is_some_and(|entry_actver| entry_actver != actver)
        {
            return false;
        }
    }
    if let Some(version_tag) = query
        .version_tag
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        if let Some(entry_tag) = entry
            .version_tag
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            if entry_tag != version_tag {
                return false;
            }
        }
    }
    has_compatibility_filter(query)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use sha2::{Digest, Sha256};

    use super::{
        claim_token_hash, hex, install_folder_key, now_unix_millis, sha256_stream, ArtifactFault,
        ArtifactStream, ClaimTake, DamagedArtifact, InstallClaim, InstallCollision,
        InvalidCatalogName, ModStore, ModUploadMeta, UnusedClaimReason,
    };
    use crate::model::{ListModsQuery, ModCatalogEntry};
    use tempfile::tempdir;

    struct TrackedChunk {
        data: &'static [u8],
        live: Arc<AtomicUsize>,
    }

    impl AsRef<[u8]> for TrackedChunk {
        fn as_ref(&self) -> &[u8] {
            self.data
        }
    }

    impl Drop for TrackedChunk {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn artifact_hash_releases_each_stream_chunk_before_reading_the_next() {
        const CHUNKS: [&[u8]; 3] = [b"large-", b"legacy-", b"artifact"];
        let live = Arc::new(AtomicUsize::new(0));
        let stream = stream::iter(CHUNKS).then({
            let live = Arc::clone(&live);
            move |data| {
                assert_eq!(live.load(Ordering::SeqCst), 0);
                live.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(Bytes::from_owner(TrackedChunk {
                    data,
                    live: Arc::clone(&live),
                })))
            }
        });
        let stream: ArtifactStream = Box::pin(stream);

        let hash = sha256_stream(stream).await.unwrap();
        assert_eq!(hash, super::hex(&Sha256::digest(b"large-legacy-artifact")));
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }

    fn streamed_meta(name: &str, version: &str) -> ModUploadMeta {
        ModUploadMeta {
            name: name.to_string(),
            version: Some(version.to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn scan_fills_missing_size_and_download_url_from_disk() {
        let root = tempdir().unwrap();
        let mod_dir = root.path().join("legacy-mod");
        std::fs::create_dir_all(&mod_dir).unwrap();
        // A hand-written / pre-sizeBytes manifest: no sizeBytes, no downloadUrl.
        std::fs::write(
            mod_dir.join("mod.json"),
            r#"{"modId":"legacy-mod","name":"Legacy","version":"1.0","description":"old"}"#,
        )
        .unwrap();
        std::fs::write(mod_dir.join("legacy-mod.pbo.zst"), vec![0u8; 4096]).unwrap();

        let store = ModStore::local(root.path()).unwrap();
        let mods = store
            .list_mods(&ListModsQuery {
                app_name: Some("CWR".to_string()),
                actver: Some(303),
                ..ListModsQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(mods.len(), 1);
        // Size comes from the artifact in the store; the link is the (relative) download route.
        assert_eq!(mods[0].size_bytes, Some(4096));
        assert_eq!(
            mods[0].download_url.as_deref(),
            Some("/v1/mods/legacy-mod/revisions/1/download")
        );
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(mod_dir.join("mod.json")).unwrap()).unwrap();
        assert_eq!(persisted["sha256"], mods[0].sha256.as_deref().unwrap());
        assert_eq!(persisted["sizeBytes"], 4096);
    }

    #[tokio::test]
    async fn publish_then_get_and_download_roundtrips() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let entry = store
            .publish_mod(&super::PublishModInput {
                name: "Synthetic Core Pack".to_string(),
                app_name: Some("CWR".to_string()),
                actver: Some(302),
                compatible_actvers: Vec::new(),
                version_tag: Some("rc1".to_string()),
                version: Some("1.0".to_string()),
                description: Some("demo".to_string()),
                authors: vec!["bis".to_string()],
                homepage_url: None,
                file: vec![1, 2, 3, 4, 5],
            })
            .await
            .unwrap();
        assert_eq!(entry.mod_id, "synthetic-core-pack");
        assert_eq!(entry.app_name.as_deref(), Some("CWR"));
        assert_eq!(entry.actver, Some(302));
        assert_eq!(entry.version_tag.as_deref(), Some("rc1"));

        let fetched = store.get_mod("synthetic-core-pack").await.unwrap().unwrap();
        assert_eq!(fetched.name, "Synthetic Core Pack");

        // The artifact arrives already compressed; the service stores and serves it verbatim,
        // and size_bytes is exactly that uploaded (download) size.
        let stored = store.artifact_bytes("synthetic-core-pack").await.unwrap();
        assert_eq!(stored.as_deref(), Some([1, 2, 3, 4, 5].as_slice()));
        assert_eq!(entry.size_bytes, Some(5));

        assert!(store.artifact_bytes("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn streamed_upload_stores_chunks_verbatim() {
        // The streamed path (begin_upload -> write chunks -> finalize_mod) writes the already-
        // compressed artifact verbatim. Drive it with a multi-chunk payload and assert the stored
        // artifact is the exact concatenation, sized to the total. Broken-state delta: dropping or
        // re-encoding a chunk would change the stored bytes or the size.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();

        let payload: Vec<u8> = (0..512 * 1024).map(|i| (i * 31 + 7) as u8).collect();
        let mut upload = store.begin_upload().await.unwrap();
        for chunk in payload.chunks(60 * 1024) {
            upload.write(chunk);
        }
        let entry = store
            .finalize_mod(upload, streamed_meta("Streamed Mod", "1.0"))
            .await
            .unwrap();
        assert_eq!(entry.mod_id, "streamed-mod");

        let stored = store.artifact_bytes("streamed-mod").await.unwrap().unwrap();
        assert_eq!(stored, payload);
        assert_eq!(entry.size_bytes, Some(payload.len() as u64));
    }

    #[tokio::test]
    async fn reserved_placement_supports_streamed_publish_and_concurrent_updates() {
        let root = tempdir().unwrap();
        let mut store = ModStore::local(root.path()).unwrap();
        store.artifact_placement = super::ArtifactPlacement::ReservedRename;

        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[1]);
        store
            .finalize_mod(upload, streamed_meta("Reserved Mod", "1.0"))
            .await
            .unwrap();
        assert!(root
            .path()
            .join("reserved-mod/revisions/1/reserved-mod.pbo.zst.reserved")
            .exists());

        let left = store.clone();
        let right = store.clone();
        let (left, right) = tokio::join!(
            left.add_revision("reserved-mod", vec![2], super::ModUploadMeta::default()),
            right.add_revision("reserved-mod", vec![3], super::ModUploadMeta::default())
        );
        let mut allocated = vec![
            left.unwrap().package_revision,
            right.unwrap().package_revision,
        ];
        allocated.sort_unstable();
        assert_eq!(allocated, vec![2, 3]);
        assert!(root
            .path()
            .join("reserved-mod/revisions/2/reserved-mod.pbo.zst.reserved")
            .exists());
        assert!(root
            .path()
            .join("reserved-mod/revisions/3/reserved-mod.pbo.zst.reserved")
            .exists());
        assert_eq!(
            store
                .get_mod("reserved-mod")
                .await
                .unwrap()
                .unwrap()
                .package_revision,
            3
        );
    }

    #[tokio::test]
    async fn streamed_empty_upload_is_rejected() {
        // A streamed upload with no bytes must be rejected, not stored as a 0-byte artifact.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();

        let upload = store.begin_upload().await.unwrap(); // no writes
        let error = store
            .finalize_mod(upload, streamed_meta("Empty Mod", "1.0"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("empty"), "got: {error}");
        assert!(store.get_mod("empty-mod-1.0").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn publish_rejects_a_folder_name_that_could_leave_the_mods_root() {
        // `folderName` is the directory every client installs into (`<modsRoot>/<folderName>`),
        // so a manifest value carrying a separator or `..` must be refused at publish — by the
        // same check `mod_id` gets — instead of being stored and re-advertised to every client.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();

        for folder in [
            r"..\..\Documents\x",
            "../../payload",
            "@mod/nested",
            r"C:\mods",
            "..",
        ] {
            let mut meta = streamed_meta("Traversal Mod", "1.0");
            meta.folder_name = Some(folder.to_string());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(&[1, 2, 3]);
            let error = store.finalize_mod(upload, meta).await.unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("invalid folder name '{folder}'"),
                "folder name {folder:?} was not rejected"
            );
            assert!(
                store.get_mod("traversal-mod").await.unwrap().is_none(),
                "folder name {folder:?} created a catalog entry"
            );
            assert!(
                !root.path().join("traversal-mod").exists(),
                "folder name {folder:?} still placed the artifact"
            );
        }

        // A revision republish is the same gate, whether the value is new or carried from the
        // current revision; a legitimate one is trimmed and kept.
        let mut meta = streamed_meta("Traversal Mod", "1.0");
        meta.folder_name = Some("nested-ok".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[4, 5, 6]);
        store.finalize_mod(upload, meta).await.unwrap();

        let mut meta = streamed_meta("Traversal Mod", "2.0");
        meta.folder_name = Some("../escaped".to_string());
        let error = store
            .add_revision("traversal-mod", vec![7, 8, 9], meta)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "invalid folder name '../escaped'");
        let current = store.get_mod("traversal-mod").await.unwrap().unwrap();
        assert_eq!(current.package_revision, 1);
        assert_eq!(current.folder_name.as_deref(), Some("nested-ok"));

        let mut meta = streamed_meta("Traversal Mod", "3.0");
        meta.folder_name = Some("  CSLA  ".to_string());
        let published = store
            .add_revision("traversal-mod", vec![9, 9, 9], meta)
            .await
            .unwrap();
        assert_eq!(published.folder_name.as_deref(), Some("CSLA"));
    }

    #[tokio::test]
    async fn publish_refuses_a_folder_name_another_entry_already_claims() {
        // The install destination is cleared before unpacking, so a second entry in one folder
        // deletes the first one's files. Both entries are individually valid: only the pair is
        // wrong, which is why the refusal has to look across the catalog.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut meta = streamed_meta("Alpha", "1.0");
        meta.folder_name = Some("shared".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha");
        store.finalize_mod(upload, meta).await.unwrap();

        for folder in ["shared", "SHARED"] {
            let mut meta = streamed_meta("Beta", "1.0");
            meta.folder_name = Some(folder.to_string());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(b"beta");
            let error = store.finalize_mod(upload, meta).await.unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("already claimed by catalog entry 'alpha'"),
                "{folder:?}: {message}"
            );
            if folder == "SHARED" {
                assert!(message.contains("as 'shared'"), "{folder:?}: {message}");
            }
        }

        assert!(store.get_mod("beta").await.unwrap().is_none());
        assert!(
            !root.path().join("beta").exists(),
            "a refused publish placed a revision"
        );
    }

    #[tokio::test]
    async fn check_install_folder_answers_before_a_publish_writes() {
        // `papa publish` cannot see the catalog, so it asks first: the answer must be the one the
        // upload would give, and asking must not create the entry or its directory.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut meta = streamed_meta("Alpha", "1.0");
        meta.folder_name = Some("shared".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha");
        store.finalize_mod(upload, meta).await.unwrap();

        let free = store
            .check_install_folder("beta", Some("shared-two"))
            .await
            .unwrap();
        assert!(free.available, "{free:?}");
        assert_eq!(free.mod_id, "beta");
        assert_eq!(free.folder, "shared-two");
        assert_eq!(free.claimed_by, None);
        assert_eq!(free.refusal, None);

        for folder in ["shared", "SHARED"] {
            let claim = store
                .check_install_folder("beta", Some(folder))
                .await
                .unwrap();
            assert!(!claim.available, "{folder:?}: {claim:?}");
            assert_eq!(claim.claimed_by.as_deref(), Some("alpha"));
            let refusal = claim.refusal.as_deref().unwrap();
            assert!(
                refusal.contains("already claimed by catalog entry 'alpha'"),
                "{folder:?}: {refusal}"
            );
            if folder == "SHARED" {
                assert!(refusal.contains("as 'shared'"), "{folder:?}: {refusal}");
            }
        }

        // No folder name is the `@<modId>` fallback, so an entry without one holds `@<its id>`.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"omega");
        store
            .finalize_mod(upload, streamed_meta("Omega", "1.0"))
            .await
            .unwrap();
        let fallback = store.check_install_folder("omega", None).await.unwrap();
        assert!(fallback.available, "{fallback:?}");
        assert_eq!(fallback.folder, "@omega");
        let taken = store
            .check_install_folder("gamma", Some("@omega"))
            .await
            .unwrap();
        assert!(!taken.available, "{taken:?}");
        assert_eq!(taken.claimed_by.as_deref(), Some("omega"));

        // The questions wrote nothing.
        assert!(store.get_mod("beta").await.unwrap().is_none());
        assert!(store.get_mod("gamma").await.unwrap().is_none());
        assert!(!root.path().join("beta").exists());
        assert!(!root.path().join("gamma").exists());
    }

    #[tokio::test]
    async fn check_install_folder_keeps_a_revisions_own_folder() {
        // A revision names no folder: it keeps the one it holds, so its own entry must not block
        // it, and a pair that predates the rule must not lock either member out of revisions.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [("Alpha", "alpha-dir"), ("Beta", "beta-dir")] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = Some(folder.to_string());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }

        let kept = store.check_install_folder("alpha", None).await.unwrap();
        assert!(kept.available, "{kept:?}");
        assert_eq!(kept.folder, "alpha-dir");

        // A case-only respelling is the folder it already holds, not a new claim.
        let respelled = store
            .check_install_folder("alpha", Some("ALPHA-DIR"))
            .await
            .unwrap();
        assert!(respelled.available, "{respelled:?}");
        assert_eq!(respelled.folder, "ALPHA-DIR");

        let moved = store
            .check_install_folder("alpha", Some("beta-dir"))
            .await
            .unwrap();
        assert!(!moved.available, "{moved:?}");
        assert_eq!(moved.claimed_by.as_deref(), Some("beta"));

        // A pair a pre-rule publish left behind: `delta` carries no folder name (`@delta`) and
        // `epsilon` was rewritten onto it. Keeping `@delta` is allowed; taking it fresh is not.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"delta");
        store
            .finalize_mod(upload, streamed_meta("Delta", "1.0"))
            .await
            .unwrap();
        let mut meta = streamed_meta("Epsilon", "1.0");
        meta.folder_name = Some("epsilon-dir".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"epsilon");
        store.finalize_mod(upload, meta).await.unwrap();
        relabel_stored_folder(root.path(), "epsilon", "@delta");

        let kept = store.check_install_folder("delta", None).await.unwrap();
        assert!(kept.available, "{kept:?}");
        assert_eq!(kept.folder, "@delta");
        let fresh = store
            .check_install_folder("zeta", Some("@delta"))
            .await
            .unwrap();
        assert!(!fresh.available, "a new entry reached a pre-rule collision");
        assert!(fresh.claimed_by.is_some(), "{fresh:?}");
    }

    #[tokio::test]
    async fn publish_refuses_the_folder_another_entry_reaches_by_fallback() {
        // No folderName means `@<modId>`: the two manifests never name each other, so the
        // publisher cannot see this pair and only the service can refuse it.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha");
        store
            .finalize_mod(upload, streamed_meta("Alpha", "1.0"))
            .await
            .unwrap();

        let mut meta = streamed_meta("Beta", "1.0");
        meta.folder_name = Some("@alpha".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"beta");
        let error = store.finalize_mod(upload, meta).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("already claimed by catalog entry 'alpha'"),
            "{error}"
        );

        // The same entry with no folder name claims `@beta` and is free.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"beta");
        let published = store
            .finalize_mod(upload, streamed_meta("Beta", "1.0"))
            .await
            .unwrap();
        assert_eq!(published.folder_name, None);
    }

    #[tokio::test]
    async fn a_revision_keeps_its_folder_but_cannot_take_another_entries() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [("Alpha", "alpha-dir"), ("Beta", "beta-dir")] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = Some(folder.to_string());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }

        // Carrying the current folder forward is not a new claim.
        let kept = store
            .add_revision(
                "alpha",
                b"alpha-two".to_vec(),
                streamed_meta("Alpha", "2.0"),
            )
            .await
            .unwrap();
        assert_eq!(kept.package_revision, 2);
        assert_eq!(kept.folder_name.as_deref(), Some("alpha-dir"));

        for folder in ["beta-dir", "BETA-DIR"] {
            let mut meta = streamed_meta("Alpha", "3.0");
            meta.folder_name = Some(folder.to_string());
            let error = store
                .add_revision("alpha", b"alpha-three".to_vec(), meta)
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("already claimed by catalog entry 'beta'"),
                "{folder:?}: {error}"
            );
        }
        let current = store.get_mod("alpha").await.unwrap().unwrap();
        assert_eq!(current.package_revision, 2);
        assert_eq!(current.folder_name.as_deref(), Some("alpha-dir"));

        // A free folder is still allowed.
        let mut meta = streamed_meta("Alpha", "4.0");
        meta.folder_name = Some("alpha-new".to_string());
        let moved = store
            .add_revision("alpha", b"alpha-four".to_vec(), meta)
            .await
            .unwrap();
        assert_eq!(moved.package_revision, 3);
        assert_eq!(moved.folder_name.as_deref(), Some("alpha-new"));
    }

    #[tokio::test]
    async fn a_revision_under_a_preexisting_collision_still_publishes() {
        // A catalog can already contain a pair this rule would now refuse. Refusing every
        // revision of either member would strand an owner who cannot fix the other entry, so a
        // revision that keeps its folder skips the lookup; the sweep keeps reporting the pair.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [("Alpha", None), ("Beta", Some("beta-dir"))] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = folder.map(str::to_string);
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }

        // Rewrite beta the way a pre-rule publish left it: folderName `@alpha`, which alpha
        // reaches by fallback.
        relabel_stored_folder(root.path(), "beta", "@alpha");

        let revised = store
            .add_revision(
                "alpha",
                b"alpha-two".to_vec(),
                streamed_meta("Alpha", "2.0"),
            )
            .await
            .unwrap();
        assert_eq!(revised.package_revision, 2);

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.install_collisions.len(), 1, "{scan:?}");
        assert_eq!(scan.install_collisions[0].folder(), "@alpha");
    }

    /// Publish `name` as a first revision in `folder` (or `@<name>` when it is `None`).
    async fn publish_first(
        store: &ModStore,
        name: &str,
        folder: Option<&str>,
    ) -> anyhow::Result<ModCatalogEntry> {
        let mut meta = streamed_meta(name, "1.0");
        meta.folder_name = folder.map(str::to_string);
        let mut upload = store.begin_upload().await?;
        upload.write(name.as_bytes());
        store.finalize_mod(upload, meta).await
    }

    /// The finished claim objects under `<root>/_claims` (a concurrent put stages as `<name>#<n>`).
    fn claim_files(root: &std::path::Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(root.join("_claims")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort();
        names
    }

    /// Write the claim a failed release leaves behind: the object the reservation lives at,
    /// naming `mod_id` and the folder as written.
    fn leave_claim(root: &std::path::Path, folder: &str, mod_id: &str) {
        let directory = root.join("_claims");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!(
                "{}.json",
                hex(install_folder_key(folder).as_bytes())
            )),
            serde_json::json!({ "modId": mod_id, "folder": folder }).to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn two_concurrent_publishes_cannot_both_take_one_folder() {
        // Both publishers pass the catalog read — nothing claims `shared` yet — and only then
        // write. The reservation is what decides between them, so exactly one may pass, whichever
        // order the scheduler runs them in.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let left = store.clone();
        let right = store.clone();
        let (alpha, beta) = tokio::join!(
            publish_first(&left, "Alpha", Some("shared")),
            publish_first(&right, "Beta", Some("SHARED"))
        );

        let (winner, loser) = match (&alpha, &beta) {
            (Ok(entry), Err(error)) => (entry.mod_id.clone(), error.to_string()),
            (Err(error), Ok(entry)) => (entry.mod_id.clone(), error.to_string()),
            _ => panic!("both publishers passed one folder: alpha: {alpha:?}, beta: {beta:?}"),
        };
        assert!(
            loser.contains(&format!("already claimed by catalog entry '{winner}'")),
            "{winner} won, loser got: {loser}"
        );

        let published = store.list_mods(&ListModsQuery::default()).await.unwrap();
        assert_eq!(published.len(), 1, "{published:?}");
        assert_eq!(published[0].mod_id, winner);
        assert_eq!(
            claim_files(root.path()).len(),
            1,
            "{:?}",
            claim_files(root.path())
        );
    }

    #[tokio::test]
    async fn a_reservation_refuses_before_the_winner_has_a_catalog_entry() {
        // A publish takes the folder at finalize time, before its revision is current. The
        // reservation has to answer then: the loser's catalog read would otherwise still see a
        // free folder, which is exactly how both used to pass.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let reservation = store
            .reserve_install_folder(&claim("alpha", "shared"), None, ClaimTake::Plain)
            .await
            .unwrap();

        let refused = publish_first(&store, "Beta", Some("SHARED"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already claimed by catalog entry 'alpha'"),
            "{refused}"
        );
        // The preflight answers with the reservation too, so the publisher stops before packing.
        let answer = store
            .check_install_folder("beta", Some("SHARED"))
            .await
            .unwrap();
        assert!(!answer.available, "{answer:?}");
        assert_eq!(answer.claimed_by.as_deref(), Some("alpha"));

        // Releasing the reservation frees the folder even though no entry ever appeared.
        assert!(store.get_mod("alpha").await.unwrap().is_none());
        store.release_reservation(Some(reservation)).await;
        publish_first(&store, "Beta", Some("SHARED")).await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_publish_releases_the_folder_it_reserved() {
        // The artifact placement fails after the folder is taken (a stray revision-1 object). The
        // claim must fail with the publish, or the folder would be lost to an entry that never
        // existed.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("beta/revisions/1")).unwrap();
        std::fs::write(root.path().join("beta/revisions/1/beta.pbo.zst"), b"stray").unwrap();

        let error = publish_first(&store, "Beta", Some("shared"))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("placing revision 1 artifact"),
            "{error}"
        );
        assert!(store.get_mod("beta").await.unwrap().is_none());
        assert!(
            claim_files(root.path()).is_empty(),
            "{:?}",
            claim_files(root.path())
        );

        // The same publish succeeds once the stray object is out of the way.
        std::fs::remove_file(root.path().join("beta/revisions/1/beta.pbo.zst")).unwrap();
        let published = publish_first(&store, "Beta", Some("shared")).await.unwrap();
        assert_eq!(published.folder_name.as_deref(), Some("shared"));
    }

    #[tokio::test]
    async fn a_reservation_holds_the_folder_before_anything_is_streamed() {
        // The read-only preflight answers both publishers of a free folder with `available`; the
        // reservation is what decides between them, and it does so before either packs. The token
        // stays with the publisher: the object only records its hash.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();

        let reservation = store.reserve_folder_claim("alpha", None).await.unwrap();
        assert_eq!(reservation.mod_id, "alpha");
        assert_eq!(reservation.folder, "@alpha");
        let token = reservation
            .token
            .expect("a new folder is reserved with a token");
        assert_eq!(token.len(), 64, "{token}");

        let stored = store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.mod_id, "alpha");
        assert_eq!(
            stored.token.as_deref(),
            Some(claim_token_hash(&token).as_str())
        );
        assert!(
            !serde_json::to_string(&stored).unwrap().contains(&token),
            "the raw token is in the claim object"
        );

        // Another entry naming the same folder is refused here, before packing.
        let refused = store
            .reserve_folder_claim("beta", Some("@alpha"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already claimed by catalog entry 'alpha'"),
            "{refused}"
        );
        // And a second publish of the same entry must not take the lease, or the first streams an
        // artifact it can only lose at the end.
        let again = store.reserve_folder_claim("alpha", None).await.unwrap_err();
        assert!(
            again
                .to_string()
                .contains("already reserved by another publish of 'alpha'"),
            "{again}"
        );

        // The sweep reads the lease as a publish that has not written its entry yet, which is
        // exactly what keeps `--fix` away from it for the age floor.
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].reason, UnusedClaimReason::NoEntry);
        assert!(scan.unused_claims[0].reason.may_still_be_finishing());
    }

    #[tokio::test]
    async fn the_upload_consumes_the_reservation_token() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        // An upload that cannot prove it made the reservation is refused, even though its
        // artifact has arrived: the folder belongs to the publish that is still streaming.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha artifact");
        let refused = store
            .finalize_mod(upload, streamed_meta("Alpha", "1.0"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("reserved by a publish of 'alpha' that has not finished"),
            "{refused}"
        );
        assert!(store.get_mod("alpha").await.unwrap().is_none());

        // A token that is not the claim's is no better than none: another publish holds it.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha artifact");
        let wrong = store
            .finalize_mod(
                upload,
                ModUploadMeta {
                    claim_token: Some("not-the-token".to_string()),
                    ..streamed_meta("Alpha", "1.0")
                },
            )
            .await
            .unwrap_err();
        assert!(
            wrong.to_string().contains("this upload no longer holds it"),
            "{wrong}"
        );

        // The upload carrying the token consumes it and publishes.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha artifact");
        let entry = store
            .finalize_mod(
                upload,
                ModUploadMeta {
                    claim_token: Some(token.clone()),
                    ..streamed_meta("Alpha", "1.0")
                },
            )
            .await
            .unwrap();
        assert_eq!(entry.package_revision, 1);
        let stored = store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.token, None, "a published claim cannot be aborted");

        // The token is gone, so an abort arriving after the publish has nothing to release; commit
        // is idempotent, which makes it safe to send after every publish.
        let abort = store
            .abort_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap_err();
        assert!(
            abort.to_string().contains("belongs to another publish"),
            "{abort}"
        );
        store
            .commit_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_reservation_lease_outlives_the_pack_and_upload_it_covers() {
        // The reservation is taken before the source is packed, and the sweep's age floor is
        // shorter than a large mod's pack plus transfer. The lease is the publisher's own deadline:
        // a sweep may act on the claim only once it has run out, and a renewal pushes it forward.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        // Each phase takes its own folder, so no claim object is rewritten twice in a row: on
        // Windows a scanner holding a freshly written file makes that rename fail.
        let live = store.reserve_folder_claim("alpha", None).await.unwrap();
        assert!(live.renew_after_ms.is_some(), "{live:?}");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert!(
            scan.unused_claims[0].lease_held(now_unix_millis()),
            "{scan:?}"
        );
        // `--fix` at zero age still leaves it: the lease, not the write time, says the publish is
        // still running.
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert!(swept.removed.is_empty(), "{swept:?}");
        assert_eq!(claim_files(root.path()).len(), 1, "the live lease survived");

        // A lease that ran out is a reservation whose publisher stopped renewing: the folder is
        // reclaimed at once instead of being refused until a write time ages out.
        store
            .reserve_folder_claim("beta", Some("beta-dir"))
            .await
            .unwrap();
        let lapsed_path = store.claim_path("beta-dir");
        let mut stored = store.read_claim(&lapsed_path).await.unwrap().unwrap();
        stored.lease_until = Some(now_unix_millis() - 1);
        store.rewrite_claim(&lapsed_path, &stored).await.unwrap();
        let scan = store.scan_catalog().await.unwrap();
        let lapsed = scan
            .unused_claims
            .iter()
            .find(|unused| unused.claim.directory == "beta")
            .expect("the lapsed claim is still unused");
        assert!(!lapsed.lease_held(now_unix_millis()));
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(swept.removed, vec![lapsed_path], "{swept:?}");
        assert_eq!(
            claim_files(root.path()).len(),
            1,
            "only the lapsed claim goes"
        );

        // A publisher renews once before it packs, moments after the reservation wrote the claim:
        // that lease is still fresh, so the renewal answers success without rewriting the object —
        // the write a Windows scanner can refuse.
        let held = store
            .reserve_folder_claim("gamma", Some("gamma-dir"))
            .await
            .unwrap();
        let token = held.token.unwrap();
        let held_path = store.claim_path("gamma-dir");
        let fresh = store.read_claim(&held_path).await.unwrap().unwrap();
        store
            .renew_folder_claim("gamma", "gamma-dir", &token)
            .await
            .unwrap();
        assert_eq!(
            store.read_claim(&held_path).await.unwrap().unwrap(),
            fresh,
            "a fresh lease is not rewritten"
        );

        // Once it has aged, the renewal moves the deadline and leaves the token alone, so a
        // publisher that keeps working keeps its folder however long that takes.
        let mut aged = fresh.clone();
        aged.lease_until = Some(now_unix_millis() - 1);
        store.rewrite_claim(&held_path, &aged).await.unwrap();
        store
            .renew_folder_claim("gamma", "gamma-dir", &token)
            .await
            .unwrap();
        let renewed = store.read_claim(&held_path).await.unwrap().unwrap();
        assert!(
            renewed
                .lease_until
                .is_some_and(|until| until > now_unix_millis()),
            "{renewed:?}"
        );
        assert_eq!(
            renewed.token, fresh.token,
            "a renewal moves the lease, not the token"
        );
    }

    #[tokio::test]
    async fn only_the_holder_can_renew_and_a_finished_publish_has_nothing_to_renew() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        let refused = store
            .renew_folder_claim("alpha", "@alpha", "not-the-token")
            .await
            .unwrap_err();
        assert!(
            refused.to_string().contains("belongs to another publish"),
            "{refused}"
        );
        let missing = store
            .renew_folder_claim("alpha", "@beta", &token)
            .await
            .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("no install-folder reservation"),
            "{missing}"
        );

        // The upload consumes the token; a renewal that races it is a no-op success, so it cannot
        // turn a successful publish into a reported failure.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha artifact");
        store
            .finalize_mod(
                upload,
                ModUploadMeta {
                    claim_token: Some(token.clone()),
                    ..streamed_meta("Alpha", "1.0")
                },
            )
            .await
            .unwrap();
        store
            .renew_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap();
        assert_eq!(
            store
                .read_claim(&store.claim_path("@alpha"))
                .await
                .unwrap()
                .unwrap()
                .lease_until,
            None
        );
    }

    #[tokio::test]
    async fn a_token_left_on_an_installed_folder_is_cleared_once_its_lease_runs_out() {
        // A body left on an installed folder the way a renewal that raced the upload's consume used
        // to leave one: a token with a lease. With a keeps-folder revision that body looks exactly
        // like a live reservation, so the lease is what separates them: while it runs, a revision
        // without the token is refused; once it has run out, the token is nobody's and the next
        // revision clears it. (The generation now stops the renewal from writing that body at all;
        // a store edited by an older master can still hold one.)
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let path = store.claim_path("alpha-dir");
        let mut stored = store.read_claim(&path).await.unwrap().unwrap();
        stored.token = Some(claim_token_hash("a-renewal-that-raced"));
        stored.lease_until = Some(now_unix_millis() + 60_000);
        assert!(store.rewrite_claim(&path, &stored).await.unwrap());

        let kept_meta = || {
            let mut meta = streamed_meta("Alpha", "2.0");
            meta.folder_name = Some("alpha-dir".to_string());
            meta
        };
        let refused = store
            .add_revision("alpha", b"alpha-two".to_vec(), kept_meta())
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("reserved by a publish of 'alpha' that has not finished"),
            "{refused}"
        );

        // Re-read first: the write is conditional on the generation the caller read, and the
        // write above moved the claim to the next one.
        let mut stored = store.read_claim(&path).await.unwrap().unwrap();
        stored.lease_until = Some(now_unix_millis() - 1);
        assert!(store.rewrite_claim(&path, &stored).await.unwrap());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), kept_meta())
            .await
            .unwrap();

        let cleared = store.read_claim(&path).await.unwrap().unwrap();
        assert_eq!(cleared.token, None, "{cleared:?}");
        assert_eq!(cleared.lease_until, None, "{cleared:?}");
    }

    #[tokio::test]
    async fn a_renewal_holding_a_stale_body_cannot_write_a_consumed_token_back() {
        // The window the claim's generation closes. A renewal reads the token-bearing body, the
        // upload consumes the token, and the renewal then writes what it read. Without a
        // generation that body lands with the token and a lease nobody holds, refusing the folder
        // to the next revision; the generation refuses the write instead.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let path = store.claim_path("alpha-dir");
        let held = store
            .reserve_folder_claim("alpha", Some("alpha-dir"))
            .await
            .unwrap()
            .token
            .unwrap();

        // The body a renewal read just before the upload consumed its token.
        let stale = store.read_claim(&path).await.unwrap().unwrap();
        assert!(stale.token.is_some(), "{stale:?}");
        let owned = store
            .reserve_install_folder(
                &claim("alpha", "alpha-dir"),
                Some(&claim("alpha", "alpha-dir")),
                ClaimTake::Consume(&held),
            )
            .await
            .unwrap();
        assert!(owned.owned);

        assert!(
            !store.rewrite_claim(&path, &stale).await.unwrap(),
            "a write decided against the pre-consume body must not land"
        );
        let current = store.read_claim(&path).await.unwrap().unwrap();
        assert_eq!(current.token, None, "{current:?}");
        assert_eq!(current.lease_until, None, "{current:?}");
        assert!(current.generation > stale.generation, "{current:?}");

        // So the next revision of the same entry is accepted at once, not after a lease.
        let mut meta = streamed_meta("Alpha", "2.0");
        meta.folder_name = Some("alpha-dir".to_string());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), meta)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_claim_write_window_serializes_writers_and_is_released_even_when_refused() {
        // Two writers that read the same body must not both write it. The marker that serializes
        // them is not a claim — the sweep only reads `<hex>.json` — and a writer that loses the
        // window answers a refusal rather than overwriting the body inside it.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        store.reserve_folder_claim("alpha", None).await.unwrap();
        let path = store.claim_path("@alpha");
        let body = store.read_claim(&path).await.unwrap().unwrap();
        let markers = || {
            let Ok(entries) = std::fs::read_dir(root.path().join("_claims")) else {
                return 0;
            };
            entries
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".writing"))
                .count()
        };

        assert!(store.lock_claim(&path).await.unwrap(), "the window is free");
        assert_eq!(markers(), 1, "the window is one marker object");
        assert!(
            !store.rewrite_claim(&path, &body).await.unwrap(),
            "a writer that cannot take the window must not write"
        );
        assert_eq!(
            store.read_claim(&path).await.unwrap().unwrap(),
            body,
            "the refused write changed nothing"
        );
        // The window is not a claim, so a scan sees exactly the one claim it protects.
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(
            claim_files(root.path()).len(),
            1,
            "{:?}",
            claim_files(root.path())
        );

        store.unlock_claim(&path).await.unwrap();
        assert_eq!(markers(), 0, "the window is released");
        assert!(
            store.rewrite_claim(&path, &body).await.unwrap(),
            "the same writer can take the window again"
        );
        assert_eq!(
            store.read_claim(&path).await.unwrap().unwrap().generation,
            body.generation + 1,
            "the rewrite is the next generation"
        );
    }

    #[tokio::test]
    async fn deleting_an_entry_does_not_delete_a_claim_under_a_writer() {
        // Deleting an entry drops the claims that name it, but not the claim another master is
        // writing: the delete takes the claim's window first, so an in-flight renewal cannot write
        // its body back over a claim this delete already saw.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", None).await.unwrap();
        let path = store.claim_path("@alpha");
        assert!(store.lock_claim(&path).await.unwrap());

        store.release_claims_of("alpha").await.unwrap();
        assert!(
            store.read_claim(&path).await.unwrap().is_some(),
            "a claim a writer holds is not deleted under it"
        );

        store.unlock_claim(&path).await.unwrap();
        store.release_claims_of("alpha").await.unwrap();
        assert!(store.read_claim(&path).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_failed_upload_gives_back_the_reservation_it_consumed() {
        // The token is consumed before placement, so by the time the artifact is placed this
        // publish owns the claim. A failure after that must take the claim with it, or the folder
        // stays refused to everyone with no entry and no artifact behind it.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("alpha/revisions/1")).unwrap();
        std::fs::write(
            root.path().join("alpha/revisions/1/alpha.pbo.zst"),
            b"stray",
        )
        .unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha artifact");
        let error = store
            .finalize_mod(
                upload,
                ModUploadMeta {
                    claim_token: Some(token),
                    ..streamed_meta("Alpha", "1.0")
                },
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("placing revision 1 artifact"),
            "{error}"
        );
        assert!(
            store
                .read_claim(&store.claim_path("@alpha"))
                .await
                .unwrap()
                .is_none(),
            "the reservation survived the publish it was consumed by"
        );
        // The folder is free for another entry at once.
        let next = store
            .reserve_folder_claim("beta", Some("@alpha"))
            .await
            .unwrap();
        assert!(next.token.is_some(), "{next:?}");
    }

    #[tokio::test]
    async fn a_reservation_cannot_be_committed_before_its_publish() {
        // Commit is what a publisher sends once its revision is written; before that the token is
        // the only way to give the folder back, so clearing it early would strand the folder.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        let refused = store
            .commit_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("does not install into folder '@alpha' yet"),
            "{refused}"
        );
        store
            .abort_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap();
        assert!(store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn aborting_a_reservation_frees_the_folder_at_once() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        // Only the reservation's own token can release it.
        let wrong = store
            .abort_folder_claim("alpha", "@alpha", "not-the-token")
            .await
            .unwrap_err();
        assert!(
            wrong.to_string().contains("belongs to another publish"),
            "{wrong}"
        );

        store
            .abort_folder_claim("alpha", "@alpha", &token)
            .await
            .unwrap();
        assert!(store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .is_none());
        // The freed folder is taken immediately, without waiting out the sweep's age floor.
        let next = store
            .reserve_folder_claim("beta", Some("@alpha"))
            .await
            .unwrap();
        assert!(next.token.is_some(), "{next:?}");
        assert_eq!(next.folder, "@alpha");
    }

    #[tokio::test]
    async fn a_second_publish_of_one_entry_is_refused_before_it_streams() {
        // Keeping a folder is not a new claim, but it is the same folder: two publishes of one
        // entry used to both pass the read-only answer and both stream, and the first rule that
        // caught them was the artifact placement after the whole upload. The reservation stops the
        // second one here.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();

        let first = store.reserve_folder_claim("alpha", None).await.unwrap();
        assert_eq!(first.folder, "alpha-dir");
        assert!(first.renew_after_ms.is_some(), "{first:?}");
        let token = first
            .token
            .expect("a revision that keeps its folder is reserved too");
        let stored = store
            .read_claim(&store.claim_path("alpha-dir"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.token.as_deref(),
            Some(claim_token_hash(&token).as_str())
        );
        assert!(stored.lease_until.is_some(), "{stored:?}");

        let refused = store.reserve_folder_claim("alpha", None).await.unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already reserved by another publish of 'alpha'"),
            "{refused}"
        );
        // The read-only answer an older client sees agrees, naming the entry it already knows.
        let answer = store.check_install_folder("alpha", None).await.unwrap();
        assert!(!answer.available, "{answer:?}");
        assert_eq!(answer.claimed_by.as_deref(), Some("alpha"));
        assert!(
            answer
                .refusal
                .as_deref()
                .is_some_and(|refusal| refusal.contains("already reserved by another publish")),
            "{answer:?}"
        );

        // The upload that carries the token is that reservation's publish and consumes it.
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha revision two");
        let entry = store
            .finalize_revision(
                upload,
                "alpha",
                ModUploadMeta {
                    claim_token: Some(token),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(entry.package_revision, 2);
        let consumed = store
            .read_claim(&store.claim_path("alpha-dir"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(consumed.token, None, "{consumed:?}");
        assert_eq!(consumed.lease_until, None, "{consumed:?}");
    }

    #[tokio::test]
    async fn aborting_a_reservation_that_keeps_the_folder_releases_it() {
        // The catalog already installs into the folder, which is why abort must not use it as a
        // test: the token decides, or a failed revision could never give its reservation back.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();

        store
            .abort_folder_claim("alpha", "alpha-dir", &token)
            .await
            .unwrap();
        assert!(store
            .read_claim(&store.claim_path("alpha-dir"))
            .await
            .unwrap()
            .is_none());
        // The entry is untouched and its folder is free to reserve again.
        assert_eq!(
            store.get_mod("alpha").await.unwrap().unwrap().folder_name,
            Some("alpha-dir".to_string())
        );
        let again = store.reserve_folder_claim("alpha", None).await.unwrap();
        assert!(again.token.is_some(), "{again:?}");
        assert_eq!(again.folder, "alpha-dir");
    }

    #[tokio::test]
    async fn a_lapsed_reservation_is_taken_over_by_the_next_publish() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", None).await.unwrap();
        let first_token = store
            .reserve_folder_claim("alpha", None)
            .await
            .unwrap()
            .token
            .unwrap();
        let path = store.claim_path("@alpha");
        let mut stored = store.read_claim(&path).await.unwrap().unwrap();
        stored.lease_until = Some(now_unix_millis() - 1);
        store.rewrite_claim(&path, &stored).await.unwrap();

        // A publisher that stopped renewing is not a publish any more: the entry can reserve its
        // own folder again without waiting for the sweep.
        let second = store.reserve_folder_claim("alpha", None).await.unwrap();
        let second_token = second.token.expect("the folder is reserved again");
        assert_ne!(first_token, second_token);
        let taken = store.read_claim(&path).await.unwrap().unwrap();
        assert_eq!(
            taken.token.as_deref(),
            Some(claim_token_hash(&second_token).as_str())
        );
        assert!(
            taken
                .lease_until
                .is_some_and(|until| until > now_unix_millis()),
            "{taken:?}"
        );
        // The publisher that lost the lease still cannot use its token.
        let stale = store
            .renew_folder_claim("alpha", "@alpha", &first_token)
            .await
            .unwrap_err();
        assert!(
            stale.to_string().contains("belongs to another publish"),
            "{stale}"
        );
    }

    #[tokio::test]
    async fn a_reservation_on_a_folder_the_entry_keeps_is_reported_once_its_lease_runs_out() {
        // The claim is the folder's record while the entry installs there, so it is not unused.
        // A token on it is a reservation for the next revision; a live lease says that publish is
        // still working, a lapsed one says it stopped — and the catalog holds the folder without
        // the claim, so the sweep can remove it.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let path = store.claim_path("alpha-dir");
        assert!(store.scan_catalog().await.unwrap().unused_claims.is_empty());

        let reservation = store
            .reserve_folder_claim("alpha", Some("alpha-dir"))
            .await
            .unwrap();
        assert!(reservation.token.is_some(), "{reservation:?}");
        assert!(store.scan_catalog().await.unwrap().unused_claims.is_empty());

        let mut stored = store.read_claim(&path).await.unwrap().unwrap();
        stored.lease_until = Some(now_unix_millis() - 1);
        store.rewrite_claim(&path, &stored).await.unwrap();
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(
            scan.unused_claims[0].reason,
            UnusedClaimReason::LapsedReservation
        );
        assert!(!scan.unused_claims[0].reason.may_still_be_finishing());
        assert!(!scan.is_clean(), "{scan:?}");
        // `--fix` needs no age floor: the deadline is the decision.
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(swept.removed, vec![path.clone()], "{swept:?}");
        assert!(store.read_claim(&path).await.unwrap().is_none());
        // The entry still installs into the folder, which is free to reserve again.
        let again = store.reserve_folder_claim("alpha", None).await.unwrap();
        assert_eq!(again.folder, "alpha-dir");
        assert!(again.token.is_some(), "{again:?}");
    }

    #[tokio::test]
    async fn a_move_reserves_its_destination_and_aborting_leaves_the_old_folder() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", None).await.unwrap();

        let reservation = store
            .reserve_folder_claim("alpha", Some("alpha-new"))
            .await
            .unwrap();
        assert_eq!(reservation.folder, "alpha-new");
        let token = reservation
            .token
            .expect("a move onto a new folder reserves it");
        let stored = store
            .read_claim(&store.claim_path("alpha-new"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.replaces.as_deref(), Some("@alpha"));
        assert_eq!(
            stored.token.as_deref(),
            Some(claim_token_hash(&token).as_str())
        );
        let refused = store
            .reserve_folder_claim("beta", Some("alpha-new"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already claimed by catalog entry 'alpha'"),
            "{refused}"
        );

        // Aborting leaves the entry where it was, with the folder it installs into still held.
        store
            .abort_folder_claim("alpha", "alpha-new", &token)
            .await
            .unwrap();
        assert!(store
            .read_claim(&store.claim_path("alpha-new"))
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store.get_mod("alpha").await.unwrap().unwrap().folder_name,
            None
        );
        assert!(store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .is_some());

        // Reserving again and publishing with the token commits the move and releases the old
        // folder, exactly as a move without a reservation does.
        let token = store
            .reserve_folder_claim("alpha", Some("alpha-new"))
            .await
            .unwrap()
            .token
            .unwrap();
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(b"alpha revision two");
        let entry = store
            .finalize_revision(
                upload,
                "alpha",
                ModUploadMeta {
                    claim_token: Some(token),
                    folder_name: Some("alpha-new".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(entry.folder_name.as_deref(), Some("alpha-new"));
        assert_eq!(
            store
                .read_claim(&store.claim_path("alpha-new"))
                .await
                .unwrap()
                .unwrap()
                .token,
            None
        );
        assert!(store
            .read_claim(&store.claim_path("@alpha"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn deleting_a_mod_frees_the_folder_it_reserved() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("shared"))
            .await
            .unwrap();
        assert_eq!(claim_files(root.path()).len(), 1);

        assert!(store.delete_mod("alpha").await.unwrap());
        assert!(
            claim_files(root.path()).is_empty(),
            "{:?}",
            claim_files(root.path())
        );
        publish_first(&store, "Beta", Some("shared")).await.unwrap();
    }

    #[tokio::test]
    async fn a_revision_that_moves_releases_the_folder_it_left() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let mut meta = streamed_meta("Alpha", "2.0");
        meta.folder_name = Some("alpha-new".to_string());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), meta)
            .await
            .unwrap();
        // One claim, not two: the folder it left went back.
        assert_eq!(
            claim_files(root.path()).len(),
            1,
            "{:?}",
            claim_files(root.path())
        );

        publish_first(&store, "Beta", Some("alpha-dir"))
            .await
            .unwrap();
        let refused = publish_first(&store, "Gamma", Some("alpha-new"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already claimed by catalog entry 'alpha'"),
            "{refused}"
        );
        // The entry still holds the folder it moved to.
        let kept = store.check_install_folder("alpha", None).await.unwrap();
        assert!(kept.available, "{kept:?}");
        assert_eq!(kept.folder, "alpha-new");
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_claim_that_names_no_catalog_entry() {
        // A publish takes its folder's claim and only then makes the entry current. A crash in
        // between leaves exactly this: a folder nobody can publish into and no entry to explain
        // it. Taking a reservation directly reproduces the leftover without a paused future.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let reservation = store
            .reserve_install_folder(&claim("alpha", "shared"), None, ClaimTake::Plain)
            .await
            .unwrap();
        assert!(reservation.owned);

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.entries_checked, 0, "{scan:?}");
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].claim, claim("alpha", "shared"));
        assert_eq!(scan.unused_claims[0].reason, UnusedClaimReason::NoEntry);
        assert!(!scan.is_clean(), "{scan:?}");

        // A claim an entry holds is not unused: it is how the entry keeps its folder.
        publish_first(&store, "Beta", Some("beta-dir"))
            .await
            .unwrap();
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.entries_checked, 1, "{scan:?}");
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].claim.directory, "alpha");
    }

    #[tokio::test]
    async fn removing_an_orphaned_claim_frees_its_folder() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let reservation = store
            .reserve_install_folder(&claim("ghost", "shared"), None, ClaimTake::Plain)
            .await
            .unwrap();
        assert!(reservation.owned);
        let refused = publish_first(&store, "Beta", Some("shared"))
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("already claimed by catalog entry 'ghost'"),
            "{refused}"
        );

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        // A claim a publish may still be finishing is left alone: deleting it would let a second
        // publisher take the folder the first one is still writing into.
        let kept = store
            .remove_unused_claims(&scan.unused_claims, Duration::from_secs(3600))
            .await
            .unwrap();
        assert!(kept.removed.is_empty(), "{kept:?}");
        assert!(kept.retaken.is_empty(), "{kept:?}");
        assert_eq!(claim_files(root.path()).len(), 1);

        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(swept.removed.len(), 1, "{swept:?}");
        assert!(
            claim_files(root.path()).is_empty(),
            "{:?}",
            claim_files(root.path())
        );
        assert!(store.scan_catalog().await.unwrap().is_clean());

        // The folder is publishable again, and the sweep stays clean with the new claim.
        publish_first(&store, "Beta", Some("shared")).await.unwrap();
        let scan = store.scan_catalog().await.unwrap();
        assert!(scan.is_clean(), "{scan:?}");
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off() {
        // A move takes the new folder's claim before the revision is current and releases the old
        // folder only after it. A release that failed leaves the old folder held by an entry that
        // no longer installs there — the same dead folder as an orphan, but named by a live entry,
        // so the orphan rule alone never sees it. The claim records nothing about a move onto its
        // folder, which is what says that no move in flight can need it.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let mut meta = streamed_meta("Alpha", "2.0");
        meta.folder_name = Some("alpha-new".to_string());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), meta)
            .await
            .unwrap();
        // The move recorded the folder it left in the claim it took, which is what lets the sweep
        // decide about `alpha-dir` below without waiting out a grace period. A release that did
        // not fail has already removed that claim; recreate the failed one.
        let moved_to = store
            .read_claim(&store.claim_path("alpha-new"))
            .await
            .unwrap()
            .expect("the move holds the folder it moved to");
        assert_eq!(moved_to.replaces.as_deref(), Some("alpha-dir"));
        leave_claim(root.path(), "alpha-dir", "alpha");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.entries_checked, 1, "{scan:?}");
        // One, not two: the claim on the folder the entry moved to is still needed.
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].claim, claim("alpha", "alpha-dir"));
        assert_eq!(
            scan.unused_claims[0].reason,
            UnusedClaimReason::MovedAway {
                folder: "alpha-new".to_string()
            }
        );
        assert!(!scan.unused_claims[0].reason.may_still_be_finishing());
        assert!(!scan.is_clean(), "{scan:?}");

        // The claim is not a move that could still land, so no grace period applies: a sweep that
        // would leave any running publish alone removes it anyway.
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::from_secs(3600))
            .await
            .unwrap();
        assert_eq!(swept.removed.len(), 1, "{swept:?}");
        let scan = store.scan_catalog().await.unwrap();
        assert!(scan.is_clean(), "{scan:?}");
        let kept = store.check_install_folder("alpha", None).await.unwrap();
        assert!(kept.available, "{kept:?}");
        assert_eq!(kept.folder, "alpha-new");

        // The folder it moved off is publishable again.
        publish_first(&store, "Beta", Some("alpha-dir"))
            .await
            .unwrap();
        assert!(store.scan_catalog().await.unwrap().is_clean());
    }

    #[tokio::test]
    async fn a_move_that_has_not_written_its_revision_is_left_for_its_age() {
        // A move records the folder it leaves in the claim it takes, and only then makes the new
        // revision current. Until that write lands the claim sits on a folder the entry does not
        // install into — the same shape as a failed release — and the record is what tells them
        // apart: deleting the destination of a running move would hand the folder it is about to
        // publish into to a second entry.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let reservation = store
            .reserve_install_folder(
                &claim("alpha", "alpha-new"),
                Some(&claim("alpha", "alpha-dir")),
                ClaimTake::Plain,
            )
            .await
            .unwrap();
        assert!(reservation.owned);

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].claim, claim("alpha", "alpha-new"));
        assert_eq!(
            scan.unused_claims[0].reason,
            UnusedClaimReason::MovePending {
                from: "alpha-dir".to_string()
            }
        );
        assert!(scan.unused_claims[0].reason.may_still_be_finishing());

        // Young enough to be a publisher between its claim and its revision: kept.
        let kept = store
            .remove_unused_claims(&scan.unused_claims, Duration::from_secs(3600))
            .await
            .unwrap();
        assert!(kept.removed.is_empty(), "{kept:?}");
        assert!(kept.retaken.is_empty(), "{kept:?}");
        // Old enough that no publish is still running: the move never landed.
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(swept.removed.len(), 1, "{swept:?}");
        assert!(store.scan_catalog().await.unwrap().is_clean());
        // The entry still installs where it did, and keeps the folder it has.
        let held = store.check_install_folder("alpha", None).await.unwrap();
        assert!(held.available, "{held:?}");
        assert_eq!(held.folder, "alpha-dir");
    }

    #[tokio::test]
    async fn a_move_refreshes_the_folder_its_claim_says_it_replaces() {
        // A claim can outlive the move that wrote it: `Create` keeps the first body, so a move
        // onto a folder this entry already claims would leave the folder of an older move in
        // place. That record is what a sweep decides by, so the move has to rewrite it — a stale
        // body makes a running move look like a leftover, and the sweep would remove the folder
        // the move is publishing into.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let first = store
            .reserve_install_folder(
                &claim("alpha", "alpha-new"),
                Some(&claim("alpha", "alpha-dir")),
                ClaimTake::Plain,
            )
            .await
            .unwrap();
        assert!(first.owned);
        store.release_reservation(Some(first)).await;

        // Moving back onto `alpha-dir`: its claim is this entry's own, so the reservation cannot
        // create it and has to record the folder being left instead.
        let back = store
            .reserve_install_folder(
                &claim("alpha", "alpha-dir"),
                Some(&claim("alpha", "alpha-new")),
                ClaimTake::Plain,
            )
            .await
            .unwrap();
        assert!(!back.owned);
        let stored = store
            .read_claim(&store.claim_path("alpha-dir"))
            .await
            .unwrap()
            .expect("the claim on alpha-dir is still there");
        assert_eq!(stored.replaces.as_deref(), Some("alpha-new"));
    }

    #[tokio::test]
    async fn a_sweep_leaves_a_claim_taken_again_after_the_scan() {
        // A scan is a snapshot, and the folder it found unused can hold a different claim by the
        // time `--fix` deletes. Here the stale claim is released and another entry publishes into
        // the folder: the object at the same path now names that entry, and deleting it would hand
        // a live folder to a second publisher.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let mut meta = streamed_meta("Alpha", "2.0");
        meta.folder_name = Some("alpha-new".to_string());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), meta)
            .await
            .unwrap();
        leave_claim(root.path(), "alpha-dir", "alpha");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(
            scan.unused_claims[0].reason,
            UnusedClaimReason::MovedAway {
                folder: "alpha-new".to_string()
            }
        );
        let path = scan.unused_claims[0].path.clone();

        // The stale claim goes and the folder is taken before this sweep runs; the scan cannot
        // see either, and its decision is all it has.
        store.store.delete(&path).await.unwrap();
        publish_first(&store, "Beta", Some("alpha-dir"))
            .await
            .unwrap();
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert!(swept.removed.is_empty(), "{swept:?}");
        assert_eq!(swept.retaken, vec![path.clone()], "{swept:?}");
        let held_claim = store
            .read_claim(&path)
            .await
            .unwrap()
            .expect("the folder is still claimed");
        assert_eq!(held_claim.mod_id, "beta");
        assert_eq!(held_claim.folder, "alpha-dir");
        assert!(store.scan_catalog().await.unwrap().is_clean());
        let held = store.check_install_folder("beta", None).await.unwrap();
        assert!(held.available, "{held:?}");
        assert_eq!(held.folder, "alpha-dir");
    }

    #[tokio::test]
    async fn a_sweep_leaves_a_claim_written_again_after_the_scan() {
        // Comparing bodies alone would miss the same entry taking the same folder again: a
        // release and a fresh publish of one mod write the same `{ modId, folder }`. The scan also
        // records when the object was written, so a claim written after that read is left even
        // when its body is unchanged — the scan decided about an object that no longer exists.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        publish_first(&store, "Alpha", Some("alpha-dir"))
            .await
            .unwrap();
        let mut meta = streamed_meta("Alpha", "2.0");
        meta.folder_name = Some("alpha-new".to_string());
        store
            .add_revision("alpha", b"alpha-two".to_vec(), meta)
            .await
            .unwrap();
        leave_claim(root.path(), "alpha-dir", "alpha");
        // Old enough that the age floor on a possible running publish could not keep it either.
        let stale = root.path().join("_claims").join(format!(
            "{}.json",
            hex(install_folder_key("alpha-dir").as_bytes())
        ));
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3 * 3600))
            .unwrap();

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        assert_eq!(scan.unused_claims[0].claim, claim("alpha", "alpha-dir"));

        // The same body again, after the scan: only the write time says the folder was retaken.
        leave_claim(root.path(), "alpha-dir", "alpha");
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert!(swept.removed.is_empty(), "{swept:?}");
        assert_eq!(swept.retaken, vec![scan.unused_claims[0].path.clone()]);

        // A later scan reads the object that stands now, and nothing changes after it: removed.
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.unused_claims.len(), 1, "{scan:?}");
        let swept = store
            .remove_unused_claims(&scan.unused_claims, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(swept.removed.len(), 1, "{swept:?}");
        assert!(store.scan_catalog().await.unwrap().is_clean());
    }

    // The catalog-name boundary is one fixture for both languages: this test, the Rust unit test
    // in mserver/Client/src/catalog.rs and the C++ case `ModInstallDir refuses catalog names
    // that are not one path component` (tests/unit/engine/Poseidon/Core/test_mod_install.cpp)
    // read tests/fixtures/mod_names/catalog_name_cases.json, so a reserved character added there
    // fails every suite at once instead of being re-typed per language.
    fn catalog_name_cases(key: &str) -> Vec<String> {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/mod_names/catalog_name_cases.json"
        ));
        let json: serde_json::Value =
            serde_json::from_str(fixture).expect("the catalog name cases fixture is valid JSON");
        json[key]
            .as_array()
            .unwrap_or_else(|| panic!("'{key}' is an array in the fixture"))
            .iter()
            .map(|case| {
                case.as_str()
                    .unwrap_or_else(|| panic!("'{key}' holds strings"))
                    .to_owned()
            })
            .collect()
    }

    #[tokio::test]
    async fn publish_holds_both_manifest_names_to_the_engine_component_set() {
        // The shared `is_single_path_component` mirrors Foundation::IsSinglePathComponent, the
        // rule the installer, the addon resolver and the MP mission resolver apply before a join.
        // Both manifest identities are held to it: `mod_id`, because it becomes
        // `<mod_id>/<mod_id>.pbo.zst` in the store, and `folderName`, because it becomes
        // `<modsRoot>/<folderName>` on every client. A name the engine would refuse to install
        // must not publish.
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let refused = catalog_name_cases("refused");
        let accepted = catalog_name_cases("accepted");
        // A list that parsed to nothing would pass vacuously, so both sides carry a floor.
        assert!(refused.len() >= 20, "refused cases: {refused:?}");
        assert!(accepted.len() >= 8, "accepted cases: {accepted:?}");

        for name in &refused {
            let mut meta = streamed_meta("Character Mod", "1.0");
            meta.mod_id = Some(name.clone());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(&[1, 2, 3]);
            let error = store.finalize_mod(upload, meta).await.unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("invalid mod id '{name}'"),
                "mod id {name:?} was not rejected"
            );
            assert!(
                store
                    .list_mods(&ListModsQuery::default())
                    .await
                    .unwrap()
                    .is_empty(),
                "mod id {name:?} created a catalog entry"
            );
        }

        for name in &refused {
            let mut meta = streamed_meta("Character Mod", "1.0");
            meta.folder_name = Some(name.clone());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(&[1, 2, 3]);
            let error = store.finalize_mod(upload, meta).await.unwrap_err();
            // The message carries the normalized value: `folderName` is trimmed first, so
            // `".. "` reports `'..'` while a `mod_id` (below) reports its exact spelling.
            assert_eq!(
                error.to_string(),
                format!("invalid folder name '{}'", name.trim()),
                "folder name {name:?} was not rejected"
            );
            assert!(!root.path().join("character-mod").exists());
        }

        // The accepted side of the same list: an interior run of dots is one component on every
        // filesystem, so `ver2.0`-style names and `Sth..else` publish, keep their exact spelling
        // and round-trip through the store.
        for name in &accepted {
            let mut meta = streamed_meta("Character Mod", "1.0");
            meta.mod_id = Some(name.clone());
            meta.folder_name = Some(name.clone());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(&[4, 5, 6]);
            let published = store.finalize_mod(upload, meta).await.unwrap();
            assert_eq!(&published.mod_id, name);
            assert_eq!(published.folder_name.as_deref(), Some(name.as_str()));
            assert_eq!(
                store.artifact_bytes(name).await.unwrap().as_deref(),
                Some(&[4u8, 5, 6][..]),
                "{name:?} did not round-trip through the store"
            );
        }
    }

    #[tokio::test]
    async fn scan_catalog_is_clean_for_a_published_catalog() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [("Plain Mod", None), ("Folder Mod", Some("nested-ok"))] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = folder.map(str::to_string);
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(&[1, 2, 3]);
            store.finalize_mod(upload, meta).await.unwrap();
        }

        let scan = store.scan_catalog().await.unwrap();
        assert!(scan.is_clean(), "{scan:?}");
        assert_eq!(scan.entries_checked, 2);
        assert_eq!(scan.artifacts_checked, 2);
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_stored_folder_name_the_rule_rejects() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut meta = streamed_meta("Character Mod", "1.0");
        meta.folder_name = Some("nested-ok".to_string());
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[1, 2, 3]);
        store.finalize_mod(upload, meta).await.unwrap();

        // Rewrite the stored revision the way a publish from before the rule could have left it.
        let metadata = root.path().join("character-mod/revisions/1/mod.json");
        let mut entry: ModCatalogEntry =
            serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        entry.folder_name = Some("..\\..\\Documents".to_string());
        std::fs::write(&metadata, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();

        // The API still hands the entry out; only the sweep sees the name.
        let served = store.list_mods(&ListModsQuery::default()).await.unwrap();
        assert_eq!(served.len(), 1, "the tampered entry is still served");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.entries_checked, 1);
        assert_eq!(
            scan.rejected_names,
            vec![InvalidCatalogName {
                directory: "character-mod".to_string(),
                field: "folderName",
                value: "..\\..\\Documents".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_stored_mod_id_that_disagrees_with_its_directory() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[1, 2, 3]);
        store
            .finalize_mod(upload, streamed_meta("Character Mod", "1.0"))
            .await
            .unwrap();

        let metadata = root.path().join("character-mod/revisions/1/mod.json");
        let mut entry: ModCatalogEntry =
            serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        entry.mod_id = "a*b".to_string();
        std::fs::write(&metadata, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(
            scan.rejected_names,
            vec![InvalidCatalogName {
                directory: "character-mod".to_string(),
                field: "mod_id",
                value: "a*b".to_string(),
            }]
        );
    }

    // Windows cannot create a directory whose name the rule rejects (`*`, a trailing dot or a
    // trailing space are all illegal there), so this half of the sweep is exercised on the
    // platforms where such a directory can exist.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn scan_catalog_reports_a_directory_whose_mod_id_is_not_one_component() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let directory = root.path().join("a*b");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("mod.json"),
            r#"{"modId":"a*b","name":"Bad","version":"1.0"}"#,
        )
        .unwrap();

        // The blind spot the sweep closes: `/v1/mods` cannot return this entry at all, because
        // it resolves each directory through `get_mod`, which refuses the id.
        let listed = store.list_mods(&ListModsQuery::default()).await.unwrap();
        assert!(listed.is_empty(), "listed: {listed:?}");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.entries_checked, 1);
        assert_eq!(
            scan.rejected_names,
            vec![InvalidCatalogName {
                directory: "a*b".to_string(),
                field: "mod_id",
                value: "a*b".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_current_artifact_that_is_missing() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[1, 2, 3]);
        let published = store
            .finalize_mod(upload, streamed_meta("Broken Mod", "1.0"))
            .await
            .unwrap();
        std::fs::remove_file(
            root.path()
                .join("broken-mod/revisions/1/broken-mod.pbo.zst"),
        )
        .unwrap();

        // The catalog still advertises the entry: it is the download that would 404.
        let served = store.list_mods(&ListModsQuery::default()).await.unwrap();
        assert_eq!(served.len(), 1, "the entry is still listed");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.artifacts_checked, 1);
        assert_eq!(
            scan.damaged_artifacts,
            vec![DamagedArtifact {
                directory: "broken-mod".to_string(),
                revision: published.package_revision,
                fault: ArtifactFault::Missing,
            }]
        );
    }

    #[tokio::test]
    async fn scan_catalog_reports_a_current_artifact_whose_sha256_no_longer_matches() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mut upload = store.begin_upload().await.unwrap();
        upload.write(&[1, 2, 3]);
        let published = store
            .finalize_mod(upload, streamed_meta("Tampered Mod", "1.0"))
            .await
            .unwrap();
        let expected = published.sha256.clone().expect("publish records a sha256");
        std::fs::write(
            root.path()
                .join("tampered-mod/revisions/1/tampered-mod.pbo.zst"),
            b"replaced bytes",
        )
        .unwrap();

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(
            scan.damaged_artifacts,
            vec![DamagedArtifact {
                directory: "tampered-mod".to_string(),
                revision: 1,
                fault: ArtifactFault::Sha256Mismatch {
                    expected,
                    actual: super::hex(&Sha256::digest(b"replaced bytes")),
                },
            }]
        );
    }

    #[tokio::test]
    async fn scan_catalog_finds_a_legacy_artifact_at_its_pre_revision_path() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let mod_dir = root.path().join("legacy-mod");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(mod_dir.join("legacy-mod.pbo.zst"), b"legacy bytes").unwrap();
        let sha = super::hex(&Sha256::digest(b"legacy bytes"));
        std::fs::write(
            mod_dir.join("mod.json"),
            format!(
                r#"{{"modId":"legacy-mod","name":"Legacy","version":"1.0","description":"old","sha256":"{sha}"}}"#
            ),
        )
        .unwrap();

        // A revision-1 entry with no `revisions/` directory is served from
        // `<mod>/<mod>.pbo.zst`; the sweep must resolve the same object, not report it missing.
        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.artifacts_checked, 1);
        assert!(
            scan.damaged_artifacts.is_empty(),
            "{:?}",
            scan.damaged_artifacts
        );
    }

    fn claim(directory: &str, folder: &str) -> InstallClaim {
        InstallClaim {
            directory: directory.to_string(),
            folder: folder.to_string(),
        }
    }

    /// Rewrite a published entry's `folderName` in place, the way a catalog that predates the
    /// publish-time claim check stored a collision.
    fn relabel_stored_folder(root: &std::path::Path, directory: &str, folder: &str) {
        let path = root.join(directory).join("revisions/1/mod.json");
        let mut stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored["folderName"] = serde_json::Value::String(folder.to_string());
        std::fs::write(&path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn scan_catalog_reports_two_entries_that_install_into_one_folder() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [
            ("Shared One", Some("shared")),
            ("Shared Two", Some("shared-two")),
            ("Solo", Some("solo")),
        ] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = folder.map(str::to_string);
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }
        // A publish that claims `shared` is refused now, so the pair can only come from a catalog
        // that predates the publish-time check; store it the way that catalog did.
        relabel_stored_folder(root.path(), "shared-two", "shared");

        // Both entries are otherwise perfect: only the pair is wrong, which is the reason the
        // per-entry pass could not report it.
        let scan = store.scan_catalog().await.unwrap();
        assert!(scan.rejected_names.is_empty(), "{scan:?}");
        assert!(scan.damaged_artifacts.is_empty(), "{scan:?}");
        assert_eq!(
            scan.install_collisions,
            vec![InstallCollision {
                claims: vec![claim("shared-one", "shared"), claim("shared-two", "shared")],
            }]
        );
        // Both spellings agree, so the report can use the plain same-folder row.
        assert!(scan.install_collisions[0].spells_one_folder());
        assert!(!scan.is_clean());
    }

    #[tokio::test]
    async fn scan_catalog_catches_a_folder_name_that_shadows_the_mod_id_fallback() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [
            // No folder name: every client installs this into `@alpha`.
            ("Alpha", None),
            ("Beta", Some("beta-dir")),
        ] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = folder.map(str::to_string);
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }
        // The same folder, claimed by hand on a different entry before the publish check.
        relabel_stored_folder(root.path(), "beta", "@alpha");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(
            scan.install_collisions,
            vec![InstallCollision {
                claims: vec![claim("alpha", "@alpha"), claim("beta", "@alpha")],
            }]
        );
    }

    // Windows is where the installer runs, so `CSLA` and `csla` are one directory there even
    // though they are two store keys. The claims keep their own spelling for the report.
    #[tokio::test]
    async fn scan_catalog_folds_case_when_matching_install_folders() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for (name, folder) in [("Upper", "upper-dir"), ("Lower", "csla")] {
            let mut meta = streamed_meta(name, "1.0");
            meta.folder_name = Some(folder.to_string());
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store.finalize_mod(upload, meta).await.unwrap();
        }
        // A second entry claiming `CSLA` is refused at publish now; store the pre-rule value.
        relabel_stored_folder(root.path(), "upper", "CSLA");

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(
            scan.install_collisions,
            vec![InstallCollision {
                claims: vec![claim("lower", "csla"), claim("upper", "CSLA")],
            }]
        );
        assert!(!scan.install_collisions[0].spells_one_folder());
    }

    #[tokio::test]
    async fn scan_catalog_does_not_collide_install_folders_the_installer_refuses() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        for name in ["First", "Second"] {
            let mut upload = store.begin_upload().await.unwrap();
            upload.write(name.as_bytes());
            store
                .finalize_mod(upload, streamed_meta(name, "1.0"))
                .await
                .unwrap();
        }
        // Two entries carrying the same rejected folder name: both are reported as names, but a
        // folder the installer never joins cannot overwrite anything.
        for directory in ["first", "second"] {
            let metadata = root
                .path()
                .join(format!("{directory}/revisions/1/mod.json"));
            let mut entry: ModCatalogEntry =
                serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
            entry.folder_name = Some("a*b".to_string());
            std::fs::write(&metadata, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
        }

        let scan = store.scan_catalog().await.unwrap();
        assert_eq!(scan.rejected_names.len(), 2, "{scan:?}");
        assert!(scan.install_collisions.is_empty(), "{scan:?}");
    }

    #[test]
    fn a_refused_rename_is_recognised_through_the_store_error_chain() {
        let denied = object_store::Error::Generic {
            store: "LocalFileSystem",
            source: Box::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        };
        assert!(super::is_refused_rename(&denied));

        let missing = object_store::Error::Generic {
            store: "LocalFileSystem",
            source: Box::new(std::io::Error::from(std::io::ErrorKind::NotFound)),
        };
        assert!(!super::is_refused_rename(&missing));
        assert!(!super::is_refused_rename(
            &object_store::Error::NotImplemented
        ));
    }

    #[test]
    fn slugify_transliterates_diacritics() {
        assert_eq!(super::slugify("Žlutý Modul"), "zluty-modul");
        assert_eq!(super::slugify("Synthetic Core Pack"), "synthetic-core-pack");
        assert_eq!(super::slugify("Žluťoučký Kůň"), "zlutoucky-kun");
        assert_eq!(super::slugify("@Fixture Mod!"), "fixture-mod");
    }

    #[tokio::test]
    async fn delete_removes_mod_and_artifact_from_store() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        store
            .publish_mod(&super::PublishModInput {
                name: "Temp Mod".to_string(),
                app_name: None,
                actver: None,
                compatible_actvers: Vec::new(),
                version_tag: None,
                version: Some("1.0".to_string()),
                description: None,
                authors: Vec::new(),
                homepage_url: None,
                file: vec![9, 9, 9],
            })
            .await
            .unwrap();
        assert!(store.get_mod("temp-mod").await.unwrap().is_some());
        store
            .add_revision("temp-mod", vec![8, 8, 8], super::ModUploadMeta::default())
            .await
            .unwrap();

        // Delete returns true (existed) and removes both the entry and the artifact.
        assert!(store.delete_mod("temp-mod").await.unwrap());
        assert!(store.get_mod("temp-mod").await.unwrap().is_none());
        assert!(store.artifact_bytes("temp-mod").await.unwrap().is_none());
        assert!(store.revision_bytes("temp-mod", 1).await.unwrap().is_none());
        assert!(store.revision_bytes("temp-mod", 2).await.unwrap().is_none());
        assert!(store
            .list_mods(&ListModsQuery::default())
            .await
            .unwrap()
            .is_empty());

        // Deleting again, or a never-existing / unsafe id, returns false.
        assert!(!store.delete_mod("temp-mod").await.unwrap());
        assert!(!store.delete_mod("never-existed").await.unwrap());
        assert!(!store.delete_mod("../escape").await.unwrap());
    }

    #[tokio::test]
    async fn stable_id_keeps_immutable_revision_history() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        let first = store
            .publish_mod(&super::PublishModInput {
                name: "Stable Mod".to_string(),
                app_name: Some("CWR".to_string()),
                actver: Some(302),
                compatible_actvers: Vec::new(),
                version_tag: Some("rc1".to_string()),
                version: Some("1.0".to_string()),
                description: Some("first".to_string()),
                authors: vec!["Author".to_string()],
                homepage_url: Some("https://example.invalid/stable".to_string()),
                file: vec![1, 2, 3],
            })
            .await
            .unwrap();
        assert_eq!(first.mod_id, "stable-mod");
        assert_eq!(first.package_revision, 1);

        let second = store
            .add_revision("stable-mod", vec![4, 5, 6], super::ModUploadMeta::default())
            .await
            .unwrap();
        assert_eq!(second.mod_id, "stable-mod");
        assert_eq!(second.package_revision, 2);
        assert_eq!(second.name, "Stable Mod");
        assert_eq!(second.version, "1.0");
        assert_eq!(second.description, "first");

        let latest = store.get_mod("stable-mod").await.unwrap().unwrap();
        assert_eq!(latest.package_revision, 2);
        assert_eq!(
            store
                .revision_bytes("stable-mod", 1)
                .await
                .unwrap()
                .unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            store
                .revision_bytes("stable-mod", 2)
                .await
                .unwrap()
                .unwrap(),
            vec![4, 5, 6]
        );
        let history = store.list_revisions("stable-mod").await.unwrap();
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.package_revision)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );

        let restored = store
            .add_revision("stable-mod", vec![1, 2, 3], super::ModUploadMeta::default())
            .await
            .unwrap();
        assert_eq!(restored.package_revision, 3);
        assert_eq!(
            store
                .get_mod("stable-mod")
                .await
                .unwrap()
                .unwrap()
                .package_revision,
            3
        );
    }

    #[tokio::test]
    async fn catalog_selects_latest_compatible_revision() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        store
            .publish_mod(&super::PublishModInput {
                name: "Compatible Mod".to_string(),
                app_name: Some("CWR".to_string()),
                actver: Some(303),
                compatible_actvers: vec![303, 305],
                version_tag: None,
                version: Some("1.0".to_string()),
                description: None,
                authors: Vec::new(),
                homepage_url: None,
                file: vec![1, 2, 3],
            })
            .await
            .unwrap();
        store
            .add_revision(
                "compatible-mod",
                vec![4, 5, 6],
                super::ModUploadMeta {
                    actver: Some(305),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let for_303 = store
            .list_mods(&ListModsQuery {
                app_name: Some("CWR".to_string()),
                actver: Some(303),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(for_303.len(), 1);
        assert_eq!(for_303[0].package_revision, 1);
        assert_eq!(
            for_303[0].download_url.as_deref(),
            Some("/v1/mods/compatible-mod/revisions/1/download")
        );

        let for_305 = store
            .list_mods(&ListModsQuery {
                app_name: Some("CWR".to_string()),
                actver: Some(305),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(for_305.len(), 1);
        assert_eq!(for_305[0].package_revision, 2);
        assert_eq!(for_305[0].compatible_actvers, [305]);

        let for_306 = store
            .list_mods(&ListModsQuery {
                app_name: Some("CWR".to_string()),
                actver: Some(306),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(for_306.is_empty());
    }

    #[tokio::test]
    async fn legacy_revision_compatibility_updates_metadata() {
        let root = tempdir().unwrap();
        let mod_dir = root.path().join("legacy-compatible");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(
            mod_dir.join("mod.json"),
            r#"{"modId":"legacy-compatible","app":"CWR","actver":303,"name":"Legacy Compatible","version":"1.0","description":"fixture","downloadUrl":"/v1/mods/legacy-compatible/download"}"#,
        )
        .unwrap();
        std::fs::write(mod_dir.join("legacy-compatible.pbo.zst"), [1, 2, 3]).unwrap();

        let store = ModStore::local(root.path()).unwrap();
        let query = ListModsQuery {
            app_name: Some("CWR".to_string()),
            actver: Some(305),
            ..Default::default()
        };
        assert!(store.list_mods(&query).await.unwrap().is_empty());

        let migrated = store
            .set_revision_compatibility("legacy-compatible", 1, &[305, 303, 305])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(migrated.compatible_actvers, [303, 305]);
        assert_eq!(
            store.list_mods(&query).await.unwrap()[0]
                .download_url
                .as_deref(),
            Some("/v1/mods/legacy-compatible/revisions/1/download")
        );
        let metadata: ModCatalogEntry =
            serde_json::from_slice(&std::fs::read(mod_dir.join("mod.json")).unwrap()).unwrap();
        assert_eq!(metadata.compatible_actvers, [303, 305]);
    }

    #[tokio::test]
    async fn identical_current_upload_is_idempotent() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        store
            .publish_mod(&super::PublishModInput {
                name: "Idempotent Mod".to_string(),
                app_name: None,
                actver: None,
                compatible_actvers: Vec::new(),
                version_tag: None,
                version: Some("1.0".to_string()),
                description: None,
                authors: Vec::new(),
                homepage_url: None,
                file: vec![7, 8, 9],
            })
            .await
            .unwrap();

        let repeated = store
            .add_revision(
                "idempotent-mod",
                vec![7, 8, 9],
                super::ModUploadMeta::default(),
            )
            .await
            .unwrap();
        assert_eq!(repeated.package_revision, 1);
        assert_eq!(
            store.list_revisions("idempotent-mod").await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_updates_allocate_distinct_revisions_without_regressing_latest() {
        let root = tempdir().unwrap();
        let store = ModStore::local(root.path()).unwrap();
        store
            .publish_mod(&super::PublishModInput {
                name: "Concurrent Mod".to_string(),
                app_name: None,
                actver: None,
                compatible_actvers: Vec::new(),
                version_tag: None,
                version: Some("1.0".to_string()),
                description: None,
                authors: Vec::new(),
                homepage_url: None,
                file: vec![1],
            })
            .await
            .unwrap();

        let left = store.clone();
        let right = store.clone();
        let (left, right) = tokio::join!(
            left.add_revision("concurrent-mod", vec![2], super::ModUploadMeta::default()),
            right.add_revision("concurrent-mod", vec![3], super::ModUploadMeta::default())
        );
        let mut allocated = vec![
            left.unwrap().package_revision,
            right.unwrap().package_revision,
        ];
        allocated.sort_unstable();
        assert_eq!(allocated, vec![2, 3]);
        assert_eq!(
            store
                .get_mod("concurrent-mod")
                .await
                .unwrap()
                .unwrap()
                .package_revision,
            3
        );
    }
}
