mod probe;

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use papa_bear_master_service::dev_seed::{seed_dev_data, seed_dev_mods};
use papa_bear_master_service::http::build_router_with_store;
use papa_bear_master_service::mods::{
    ArtifactFault, CatalogScan, ModStore, S3StoreConfig, UnusedClaimReason, UnusedClaimSweep,
};
use papa_bear_master_service::repository::{sqlite_url_for_path, SqliteServerDirectory};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "papa-bear-master-service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the HTTP directory service (sole owner of the SQLite database).
    Server(ServerArgs),
    /// Run the reachability prober loop against a running service.
    Probe(ProbeArgs),
    /// Sweep the mod catalog for rejected `modId`/`folderName` values, shared install folders,
    /// unused install-folder claims and damaged artifacts.
    CheckNames(CheckNamesArgs),
}

#[derive(Args, Debug)]
struct ServerArgs {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,

    #[arg(long, default_value = "tmp/papa-bear-master-service.sqlite3")]
    db: String,

    #[arg(long)]
    admin_api_key: Option<String>,

    #[arg(long)]
    mods_dir: Option<String>,

    /// Trusted header with the real client IP (e.g. `X-Forwarded-For` behind an ingress).
    /// When set, register/heartbeat bind a server's address+id to it instead of the body.
    #[arg(long)]
    client_ip_header: Option<String>,

    /// Require the server token (issued on register) for heartbeat/unregister of an active
    /// row. A token is issued regardless; enable this once publishers send it back.
    #[arg(long)]
    require_server_token: bool,

    #[arg(long)]
    dev: bool,
}

#[derive(Args, Debug)]
struct CheckNamesArgs {
    /// Mods directory to sweep. Omit to use the `MODS_S3_*` bucket the server would use.
    #[arg(long)]
    mods_dir: Option<String>,

    /// Remove the install-folder claims no catalog entry needs: those naming no entry, and the
    /// ones left on a folder an entry has moved off. A claim whose reservation lease is still
    /// running belongs to a publisher that is packing or streaming and is kept, whatever its age;
    /// without one, a claim whose entry is not written yet, or a move that has taken its
    /// destination and not made the new revision current, is kept for five minutes; a claim the
    /// catalog has already moved past is removed whatever its age.
    #[arg(long)]
    fix: bool,
}

#[derive(Args, Debug)]
struct ProbeArgs {
    /// Directory service to read from and report observations to.
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    master_server: String,

    /// Seconds between probe rounds.
    #[arg(long, default_value_t = 60)]
    interval_secs: u64,

    /// Per-server UDP query timeout in milliseconds.
    #[arg(long, default_value_t = 1500)]
    timeout_ms: u64,

    /// Maximum servers probed concurrently.
    #[arg(long, default_value_t = 16)]
    concurrency: usize,

    /// Remove servers with no heartbeat for this many seconds each round (0 disables).
    /// Default 3600s matches the original PowerServer off-board window (60 min).
    #[arg(long, default_value_t = 3600)]
    prune_max_age_secs: u64,

    /// Admin key for `/observe` and `/prune` when the service is protected.
    #[arg(long)]
    admin_api_key: Option<String>,
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();
}

fn main() -> Result<()> {
    init_tracing();
    match Cli::parse().command {
        Command::Server(args) => tokio::runtime::Runtime::new()?.block_on(run_server(args)),
        Command::CheckNames(args) => {
            tokio::runtime::Runtime::new()?.block_on(run_check_names(args))
        }
        Command::Probe(args) => probe::run(&probe::ProbeConfig {
            master_server: args.master_server,
            interval: Duration::from_secs(args.interval_secs),
            timeout: Duration::from_millis(args.timeout_ms),
            concurrency: args.concurrency,
            prune_max_age: (args.prune_max_age_secs > 0)
                .then(|| Duration::from_secs(args.prune_max_age_secs)),
            admin_api_key: args.admin_api_key,
        }),
    }
}

const fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// A name in single quotes, so its own punctuation stays readable in a row.
fn quoted(value: &str) -> String {
    format!("'{value}'")
}

/// `a`, `a and b`, `a, b and c` — already-quoted items, joined for a one-line row.
fn joined(items: &[String]) -> String {
    match items.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// A compact age for a maintenance row: `3d 4h`, `2h 5m`, `45m`, `12s`.
fn format_age(age: Duration) -> String {
    let secs = age.as_secs();
    let (days, hours, minutes) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

/// The age a claim must reach before `check-names --fix` removes it.
///
/// A publish takes its folder's claim and only then makes the entry current, and a move takes the
/// new claim and releases the folder it left only after the new revision is current. A claim
/// younger than this can belong to a publish that is still finishing; deleting that one would hand
/// the folder to a second publisher while the first writes into it — the race the claim decides.
const UNUSED_CLAIM_MIN_AGE: Duration = Duration::from_secs(5 * 60);

/// One-off catalog sweep, in four halves. Names: a `mod_id` is a store key
/// (`<mod_id>/<mod_id>.pbo.zst`) and a `folderName` becomes `<modsRoot>/<folderName>` on every
/// client, so both must be one path component; entries published before the rule tightened are
/// still served. Install folders: two entries that resolve to one folder overwrite each other,
/// because an install clears `<modsRoot>/<folder>` before unpacking. Artifacts: a listed entry
/// whose current object is missing or no longer hashes to its recorded `sha256` is a download that
/// fails — or is refused by the installer after the client has fetched the whole file. Claims: a
/// publish takes its folder's claim before it writes the entry, and a move records the folder it
/// leaves in the claim it takes and releases that folder only after the new revision is current,
/// so a claim naming no catalog directory — or one naming an entry that installs elsewhere and
/// that no claimed move explains — is a folder nothing explains and nobody can take; `--fix`
/// deletes every claim the catalog has already moved past, and one a running publish could still
/// explain only once it is older than [`UNUSED_CLAIM_MIN_AGE`] and its reservation lease has run
/// out. One line per problem, then a
/// failing exit, so the sweep can gate a maintenance window. Read-only otherwise: it never
/// rewrites metadata or moves an object.
async fn run_check_names(args: CheckNamesArgs) -> Result<()> {
    let mods_dir = args
        .mods_dir
        .as_deref()
        .map(resolve_user_path)
        .transpose()?;
    if mods_dir.is_none() && std::env::var("MODS_S3_BUCKET").is_err() {
        bail!("check-names needs --mods-dir, or the MODS_S3_* environment the server uses");
    }
    let store = build_mod_store(mods_dir.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("check-names: no mod store configured"))?;
    let scan = store.scan_catalog().await?;
    for name in &scan.rejected_names {
        println!(
            "{}: {} '{}' is not one path component",
            name.directory, name.field, name.value
        );
    }
    for collision in &scan.install_collisions {
        if collision.spells_one_folder() {
            let directories: Vec<String> = collision
                .claims
                .iter()
                .map(|claim| quoted(&claim.directory))
                .collect();
            println!(
                "install collision: {} install into the same folder '{}'",
                joined(&directories),
                collision.folder()
            );
        } else {
            // Case-only spellings are one directory on Windows, the platform the installer runs
            // on; the row says so instead of pretending the strings are equal.
            let claims: Vec<String> = collision
                .claims
                .iter()
                .map(|claim| format!("{} ({})", quoted(&claim.directory), quoted(&claim.folder)))
                .collect();
            println!(
                "install collision: {} name one folder on a case-insensitive filesystem",
                joined(&claims)
            );
        }
    }
    for artifact in &scan.damaged_artifacts {
        match &artifact.fault {
            ArtifactFault::Missing => println!(
                "{}: revision {} artifact is missing",
                artifact.directory, artifact.revision
            ),
            ArtifactFault::Sha256Mismatch { expected, actual } => println!(
                "{}: revision {} artifact sha256 is '{expected}' but the object hashes to '{actual}'",
                artifact.directory, artifact.revision
            ),
        }
    }
    let unused = sweep_unused_claims(&store, &scan, args.fix).await?;
    let entries = plural(scan.entries_checked, "entry", "entries");
    let mut problems = Vec::new();
    if !scan.rejected_names.is_empty() {
        problems.push(format!(
            "{} rejected {}",
            scan.rejected_names.len(),
            plural(scan.rejected_names.len(), "name", "names")
        ));
    }
    if !scan.install_collisions.is_empty() {
        problems.push(format!(
            "{} install {}",
            scan.install_collisions.len(),
            plural(scan.install_collisions.len(), "collision", "collisions")
        ));
    }
    if !scan.damaged_artifacts.is_empty() {
        problems.push(format!(
            "{} damaged {}",
            scan.damaged_artifacts.len(),
            plural(scan.damaged_artifacts.len(), "artifact", "artifacts")
        ));
    }
    if unused > 0 {
        problems.push(format!(
            "{unused} unused {}",
            plural(unused, "claim", "claims")
        ));
    }
    if problems.is_empty() {
        println!(
            "check-names: checked {} catalog {entries}, no rejected names, no shared install folders, no unused folder claims, every artifact intact",
            scan.entries_checked
        );
        return Ok(());
    }
    bail!(
        "check-names: {} in {} catalog {entries}",
        problems.join(", "),
        scan.entries_checked
    );
}

/// Report the claim objects no catalog entry needs, and remove the ones `--fix` may. Answers how
/// many are left, so the summary counts a fixed leftover as gone.
///
/// A leftover holds its folder against every publish with nothing to explain it, which is why it
/// is worth a row; it is reported after the entry checks because those are what tell a live claim
/// from an unused one. A reservation's lease is the first word: while it is still running its
/// publisher is packing or streaming — however long that takes — and `--fix` leaves the claim no
/// matter how old the object is. A lease that ran out on a folder the entry installs into is a
/// lapsed reservation: the catalog supplies the folder without the claim, so it is removed at
/// once. Otherwise the claim's own record of the folder it replaces
/// splits the rows: one that names the entry's current folder is a move that has not written its
/// revision yet, so `--fix` leaves it while it is younger than [`UNUSED_CLAIM_MIN_AGE`]; one that
/// names any other folder cannot be a move in flight, and goes regardless of age. The sweep reads
/// each claim again before deleting it, so one that was released and taken by another publish
/// after the scan is kept and says so rather than being removed on the scan's older decision.
async fn sweep_unused_claims(store: &ModStore, scan: &CatalogScan, fix: bool) -> Result<usize> {
    let sweep = if fix {
        store
            .remove_unused_claims(&scan.unused_claims, UNUSED_CLAIM_MIN_AGE)
            .await?
    } else {
        UnusedClaimSweep::default()
    };
    let now = SystemTime::now();
    let now_ms = i64::try_from(
        now.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    for unused in &scan.unused_claims {
        let age = now
            .duration_since(unused.claimed_at)
            .unwrap_or(Duration::ZERO);
        let fate = if !fix {
            ""
        } else if sweep.removed.contains(&unused.path) {
            "; removed"
        } else if sweep.retaken.contains(&unused.path) {
            "; kept, the claim changed after the scan"
        } else if unused.lease_held(now_ms) {
            "; kept, its reservation is still being renewed"
        } else {
            "; kept, a publish may still be finishing"
        };
        let folder = quoted(&unused.claim.folder);
        let entry = quoted(&unused.claim.directory);
        match &unused.reason {
            UnusedClaimReason::NoEntry => println!(
                "orphan claim: {folder} is reserved by {entry}, which is not a catalog entry (claimed {} ago{fate})",
                format_age(age)
            ),
            UnusedClaimReason::LapsedReservation => println!(
                "lapsed reservation: {folder} is reserved for {entry}, whose reservation stopped renewing (claimed {} ago{fate})",
                format_age(age)
            ),
            UnusedClaimReason::MovePending { from } => println!(
                "pending move: {folder} is reserved for {entry}, which still installs into {} (claimed {} ago{fate})",
                quoted(from),
                format_age(age)
            ),
            UnusedClaimReason::MovedAway { folder: current } => println!(
                "stale claim: {folder} is reserved by {entry}, which now installs into {} (claimed {} ago{fate})",
                quoted(current),
                format_age(age)
            ),
        }
    }
    if !sweep.removed.is_empty() {
        println!(
            "check-names: removed {} unused install-folder {}",
            sweep.removed.len(),
            plural(sweep.removed.len(), "claim", "claims")
        );
    }
    Ok(scan.unused_claims.len() - sweep.removed.len())
}

async fn run_server(args: ServerArgs) -> Result<()> {
    info!("Starting PAPA BEAR");

    // `--db` accepts either a sqlx URL (postgres://… or sqlite://…) or a bare file path,
    // which is treated as a local SQLite database (created if missing).
    let (db_url, db_path) = resolve_db_url(&args.db)?;
    if let Some(db_path) = &db_path {
        if let Some(parent) = ensure_parent_directory(db_path)? {
            info!("Database directory ready at {}", parent.display());
        }
    }

    let directory = Arc::new(SqliteServerDirectory::open(&db_url).await?);
    info!("Using database {}", redact_db_url(&db_url));

    let mods_path_hint = db_path.clone().unwrap_or_else(|| PathBuf::from("."));
    let mods_dir = resolve_mods_path(args.mods_dir.as_deref(), &mods_path_hint, args.dev)?;
    let mod_store = build_mod_store(mods_dir.as_deref())?;
    info!(
        "Mod store: {}",
        if mod_store.is_some() {
            "enabled"
        } else {
            "disabled"
        }
    );

    if args.dev {
        let seeded_servers = seed_dev_data(&directory, current_time_millis()?).await?;
        let seeded_mods = if let Some(store) = &mod_store {
            seed_dev_mods(store).await?
        } else {
            0
        };
        info!("Loaded dev seed ({seeded_servers} servers, {seeded_mods} mods)");
    }

    let app = build_router_with_store(
        directory,
        args.admin_api_key,
        mod_store,
        args.client_ip_header,
        args.require_server_token,
    );
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    let local_addr = listener.local_addr()?;
    log_listen_urls(local_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Resolve `--db` to a sqlx URL plus, for SQLite, the on-disk path (so its parent directory
/// can be created). A value containing `://` is taken as a URL verbatim; anything else is a
/// local SQLite file path turned into `sqlite://<abs-path>?mode=rwc`.
fn resolve_db_url(value: &str) -> Result<(String, Option<PathBuf>)> {
    if value.contains("://") {
        let path = value
            .strip_prefix("sqlite://")
            .map(|rest| PathBuf::from(rest.split('?').next().unwrap_or(rest)));
        return Ok((value.to_string(), path));
    }
    let path = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        std::env::current_dir()
            .context("resolving current directory for database path")?
            .join(value)
    };
    let url = sqlite_url_for_path(&path);
    Ok((url, Some(path)))
}

fn redact_db_url(db_url: &str) -> String {
    if let Some((scheme, rest)) = db_url.split_once("://") {
        if let Some(at) = rest.find('@') {
            if rest[..at].contains(':') {
                return format!("{scheme}://<credentials>@{}", &rest[at + 1..]);
            }
        }
    }
    db_url.to_string()
}

/// Build the mod artifact store: an S3 (MinIO-compatible) store when `MODS_S3_BUCKET` is set
/// — the deploy path, shared by all replicas — otherwise a local-filesystem store at
/// `mods_dir` for `cargo`-local dev, or none when mods are disabled.
fn build_mod_store(mods_dir: Option<&Path>) -> Result<Option<ModStore>> {
    if let Ok(bucket) = std::env::var("MODS_S3_BUCKET") {
        let endpoint = std::env::var("MODS_S3_ENDPOINT")
            .context("MODS_S3_ENDPOINT must be set alongside MODS_S3_BUCKET")?;
        let region = std::env::var("MODS_S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let access_key = std::env::var("MODS_S3_ACCESS_KEY")
            .context("MODS_S3_ACCESS_KEY must be set alongside MODS_S3_BUCKET")?;
        let secret_key = std::env::var("MODS_S3_SECRET_KEY")
            .context("MODS_S3_SECRET_KEY must be set alongside MODS_S3_BUCKET")?;
        let allow_http = endpoint.starts_with("http://");
        let virtual_hosted_style = env_bool("MODS_S3_VIRTUAL_HOSTED_STYLE", false);
        let prefix = std::env::var("MODS_S3_PREFIX").ok();
        let signed_downloads = env_bool("MODS_S3_SIGNED_DOWNLOADS", false);
        let signed_url_ttl = Duration::from_secs(env_u64("MODS_SIGNED_URL_TTL_SECS", 4 * 60 * 60));
        return Ok(Some(ModStore::s3(&S3StoreConfig {
            bucket: &bucket,
            endpoint: &endpoint,
            region: &region,
            access_key: &access_key,
            secret_key: &secret_key,
            allow_http,
            virtual_hosted_style,
            prefix: prefix.as_deref(),
            signed_downloads,
            signed_url_ttl,
        })?));
    }
    mods_dir.map(ModStore::local).transpose()
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn ensure_parent_directory(path: &Path) -> Result<Option<PathBuf>> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(None);
    };

    fs::create_dir_all(parent)
        .with_context(|| format!("creating database directory {}", parent.display()))?;
    Ok(Some(parent.to_path_buf()))
}

fn log_listen_urls(local_addr: SocketAddr) {
    let base_url = primary_startup_url(local_addr);
    info!("PAPA BEAR listening on {base_url}");
    info!("OpenAPI: {base_url}/openapi/v1.yaml");
}

fn primary_startup_url(local_addr: SocketAddr) -> String {
    match local_addr.ip() {
        IpAddr::V4(ip) if ip == Ipv4Addr::UNSPECIFIED => {
            format!("http://127.0.0.1:{}", local_addr.port())
        }
        IpAddr::V6(ip) if ip == Ipv6Addr::UNSPECIFIED => {
            format!("http://localhost:{}", local_addr.port())
        }
        _ => socket_addr_url(local_addr),
    }
}

#[cfg(test)]
fn startup_urls(local_addr: SocketAddr) -> Vec<String> {
    vec![
        primary_startup_url(local_addr),
        format!("{}/openapi/v1.yaml", primary_startup_url(local_addr)),
    ]
}

fn socket_addr_url(socket_addr: SocketAddr) -> String {
    match socket_addr.ip() {
        IpAddr::V4(_) => format!("http://{socket_addr}"),
        IpAddr::V6(_) => format!("http://[{}]:{}", socket_addr.ip(), socket_addr.port()),
    }
}

fn current_time_millis() -> Result<i64> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("reading current time for dev seed")?;
    i64::try_from(duration.as_millis()).context("converting current time to milliseconds")
}

fn resolve_mods_path(mods_dir: Option<&str>, db_path: &Path, dev: bool) -> Result<Option<PathBuf>> {
    if let Some(mods_dir) = mods_dir {
        return resolve_user_path(mods_dir).map(Some);
    }

    if dev {
        let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
        return Ok(Some(parent.join("papa-bear-master-service-dev-mods")));
    }

    Ok(None)
}

fn resolve_user_path(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .context("resolving current directory for mods path")?
            .join(path))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use tempfile::tempdir;

    use super::{
        ensure_parent_directory, redact_db_url, resolve_db_url, resolve_mods_path, startup_urls,
    };

    // Regression: a bare file path (the default `--db`) must become an authority-free `sqlite:`
    // URL. The old `sqlite://{abs-path}` form broke on Windows — the `url` crate read the drive
    // path as host:port and the service failed to open its database (SQLITE_CANTOPEN).
    #[test]
    fn resolve_db_url_builds_authority_free_sqlite_url_from_bare_path() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("papa-bear.sqlite3");

        let (url, resolved) = resolve_db_url(db_path.to_str().unwrap()).unwrap();

        assert!(url.starts_with("sqlite:"));
        assert!(!url.starts_with("sqlite://"));
        assert!(!url.contains('\\'));
        assert!(url.ends_with("?mode=rwc"));
        assert_eq!(resolved.as_deref(), Some(db_path.as_path()));
    }

    // An explicit sqlx URL (e.g. a deploy's `postgres://…`) is passed through verbatim.
    #[test]
    fn resolve_db_url_passes_explicit_urls_through_verbatim() {
        let (url, resolved) = resolve_db_url("postgres://user:pw@db.host/papa").unwrap();
        assert_eq!(url, "postgres://user:pw@db.host/papa");
        assert_eq!(resolved, None);
    }

    #[test]
    fn redact_db_url_hides_credentials() {
        assert_eq!(
            redact_db_url("postgres://user:pw@db.host/papa"),
            "postgres://<credentials>@db.host/papa"
        );
        assert_eq!(
            redact_db_url("sqlite:/tmp/papa-bear.sqlite3?mode=rwc"),
            "sqlite:/tmp/papa-bear.sqlite3?mode=rwc"
        );
    }

    #[test]
    fn ensure_parent_directory_creates_missing_directory() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("nested/path/papa-bear.sqlite3");

        let created_parent = ensure_parent_directory(&db_path).unwrap();

        let created_parent = created_parent.unwrap();
        assert_eq!(created_parent, temp_dir.path().join("nested/path"));
        assert!(created_parent.exists());
    }

    #[test]
    fn startup_urls_expand_unspecified_bind_to_local_browser_hints() {
        let urls = startup_urls(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            8080,
        )));

        assert_eq!(
            urls,
            vec![
                "http://127.0.0.1:8080".to_string(),
                "http://127.0.0.1:8080/openapi/v1.yaml".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mods_path_defaults_for_dev_mode() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir
            .path()
            .join("data/papa-bear-master-service.sqlite3");

        let mods_path = resolve_mods_path(None, &db_path, true).unwrap();

        assert_eq!(
            mods_path,
            Some(
                temp_dir
                    .path()
                    .join("data/papa-bear-master-service-dev-mods")
            )
        );
    }
}
