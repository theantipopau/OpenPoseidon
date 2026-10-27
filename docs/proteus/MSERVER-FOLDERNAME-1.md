# MSERVER-FOLDERNAME-1 — a publish manifest `folderName` that leaves the mods root is refused

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Found by `PATH-JOIN-AUDIT-1` as the last open follow-up of its finding 1. Sibling of
`MODINSTALL-PATH-1`, on the write side of the same value: the installer no longer joins an
unchecked folder name onto the mods root, but the server that hands it out never checked it.

## Problem

`ModCatalogEntry.folder_name` is advertised to every client and is the directory the
workshop installer installs into (`<modsRoot>/<folderName>`, falling back to `@<modId>`),
and the install transaction then renames that path to `.backup-N` and finishes with
`remove_all` (`MODINSTALL-PATH-1`).

The value arrives from the publish manifest's `folderName` multipart field
(`http.rs:747`), is filtered only for emptiness (`http.rs:807`), and was copied straight
into revision metadata by both publish paths:

```rust
folder_name: meta.folder_name.map(|value| value.trim().to_string()).filter(...) // finalize_mod
folder_name: meta.folder_name.or(latest.folder_name),                            // finalize_revision
```

`mod_id` — the other manifest-supplied identity, and the one that actually becomes a store
key — is checked at every path entry point (`is_safe_segment` then; now the shared
`papa_bear_client::catalog::is_single_path_component`). `folderName` was not checked at
all, so `folderName: "..\..\Documents\x"` published cleanly and every client was then told
to install there. `..` also survived the engine's newer guard only in the sense that the
install is now refused locally — the catalog still carried, served and displayed it.

## Approach

One helper next to the predicate it reuses (`is_safe_segment`, `mods.rs:1038`), because
the requirement is the one the ticket states: `folderName` gets what `mod_id` gets.

```rust
fn normalize_folder_name(value: Option<&str>) -> Result<Option<String>> // mods.rs:1061
```

- trims, like the old `finalize_mod` code did;
- absent/empty stays `None`, so the documented `@<modId>` fallback is unchanged;
- otherwise `is_safe_segment` — at the time of this ticket no `/`, no `\`, no `..`; since
  `MSERVER-PATHCHARSET-1` the engine's one-component rule — or
  `bail!("invalid folder name ...")`.

Both publish paths call it:

| site | placement | on refusal |
| --- | --- | --- |
| `finalize_mod` (`mods.rs:494`) | with the other pre-flight checks, before the artifact is placed | deletes the finished temp object, as every other rejection there does, and returns |
| `finalize_revision` (`mods.rs:616`) | after the idempotent-early-return, before the revision placement loop | returns; the current revision is untouched |

`finalize_revision` validates the *combined* value (`meta.folder_name` or `latest`), so a
revision cannot carry forward a legacy unvalidated name either: an entry whose folder name
was published through the hole can only be republished by supplying a valid `folderName`.
`publish_mod` writes `folder_name: None` and `dev_seed` passes `None`, so neither needs a
change. `http.rs` needs none either — it already maps any non-`already exists`/`does not
exist` finalize error to `400 BAD_REQUEST`, which is the right answer here.

The check was deliberately the coarse `is_safe_segment` rather than the engine's stricter
`Foundation::IsSinglePathComponent` — that value was validated for *containment* at publish
while the engine still refused anything it could not install. That split was closed by
`MSERVER-PATHCHARSET-1`, which rewrote the predicate to the engine's set; nothing in this
ticket's placements or error mapping changed.

## Behaviour

| `folderName` | result |
| --- | --- |
| `@CSLA` / `carwars2.5` / `"  CSLA  "` | published as before, trimmed to `CSLA` |
| absent, `""`, `"   "` | published with no folder name; clients use `@<modId>` |
| `..\..\Documents\x`, `../../payload`, `@mod/nested`, `C:\mods`, `..` | `400`, message `invalid folder name '<value>'`; nothing is stored, and the temp upload is deleted |

## Files

- `mserver/MasterService/src/mods.rs` — `normalize_folder_name`, both call sites, the store test.
- `mserver/MasterService/src/lib.rs` — the endpoint test (400 + nothing written).
- `docs/proteus/MSERVER-FOLDERNAME-1.md`

## Evidence

`mods::tests::publish_rejects_a_folder_name_that_could_leave_the_mods_root`
(`mods.rs:1416`): five hostile names are pushed through `finalize_mod`; each must fail with
the exact message, must not create a catalog entry, and must not place an artifact. Then a
revision publish with `../escaped` is refused while the current revision and its folder
name stay put, and `"  CSLA  "` is accepted trimmed.

`tests::http_mod_publish_rejects_a_folder_name_that_could_leave_the_mods_root`
(`lib.rs:1314`) is the same rule through the real endpoint: a multipart publish with
`folderName: ..\..\Documents\x` answers **400** with `invalid folder name`, `GET /v1/mods`
stays empty and no `<modsRoot>/folder-name-mod` appears; the same manifest with
`@folder-name-mod` returns 200 and the entry carries that folder name verbatim. That pins
the status mapping too — nothing but the existing "already exists"/"does not exist" cases
turn into 409/404.

Broken-state delta, measured: with the guard short-circuited (`if false && …`, file
restored afterwards) the test fails on the first name with
`unwrap_err() on an Ok value: ModCatalogEntry { … folder_name: Some("..\..\Documents\x") }`
— i.e. it observes precisely the stored escape, not just a missing error.

- `cargo test -p papa-bear-master-service --lib`: **73 passed / 0 failed** (serial, and
  three consecutive parallel runs before the HTTP case was added). The crate had 71 tests
  before; the two new ones pass individually too.
- `cargo clippy -p papa-bear-master-service --all-targets`: no warning from the new
  hunks. The crate carries pre-existing pedantic warnings elsewhere in the file (the
  `finalize_mod` `match`/`format!`-collect pair and the >100-line threshold, which
  `finalize_revision` already crossed before this change); they are left alone.
- `check-format.py` (rustfmt, edition 2021) and `check-all`: see the run log entry.

One transient failure was seen before it settled: an initial full parallel run failed
`delete_removes_mod_and_artifact_from_store` and `catalog_selects_latest_compatible_revision`
with `Generic LocalFileSystem error: Unable to rename file: Access is denied. (os error 5)`.
Neither test passes a folder name, both passed in eight subsequent runs of the same
configuration (and the same two on unmodified HEAD), and the serial run is green, so this is
the known Windows rename/scan flake rather than a consequence of the guard — recorded here
so it is not mistaken for green-washing later.

## Risks

- A mod published *through* the hole with a hostile folder name can no longer be
  republished without a valid `folderName`. That is the intended outcome; the value was
  uninstallable anyway once `MODINSTALL-PATH-1` landed.
- At the time of this ticket `is_safe_segment` rejected `..` anywhere in the value, so a
  legitimate name containing two consecutive dots (`Sth..else`) was refused. That
  over-rejection was removed in `MSERVER-PATHCHARSET-1`; no catalog entry used such a name.

## Follow-ups

- (done) Widen the server-side rule to the engine's `IsSinglePathComponent` set
  (`* ? " < > | :`, control characters, trailing dot/space) so a published name is
  installable by construction rather than merely contained — see `MSERVER-PATHCHARSET-1`.
- (done) The catalog can still serve entries published before this check with a folder name
  that fails it; the `check-names` sweep now reports them — see `MSERVER-CHECK-NAMES-1`.
  (That ticket's working name was `--check-folder-names`; it covers both manifest names.)

## Verify

Nothing to verify on a GPU. Re-run `cargo test -p papa-bear-master-service --lib` (73 cases
when this landed; 74 after `MSERVER-PATHCHARSET-1`) and, against a live local service,
`POST /v1/mods` with `folderName=..\..\x` — expect
`400 invalid folder name '..\..\x'` and no `mod.json` written.
