# MSERVER-FOLDER-CLAIM-1 — a publish cannot claim an install folder another entry holds

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the open follow-up of `MSERVER-CHECK-COLLISIONS-1`, which reported entries that install into
one folder and recorded the prevention as needing an owner decision. The owner asked for it: the
sweep stays the audit, and the publish path now refuses the pair it would report.

## Problem

An entry installs into `<modsRoot>/<folderName>`, or `<modsRoot>/@<modId>` when it carries none,
and the install transaction clears that directory before unpacking (`papa install`'s
`decode_and_unpack`; the engine's staged swap). Two entries that resolve to one folder are therefore
not neighbours: the second install deletes the first one's files.

`check-names` reports the pair after the fact, but a publisher cannot see it — the manifest names
only its own mod, and the other entry's folder is catalog state. The fallback is the case nobody
writes down: `alpha` with no folder name occupies `@alpha`, which `beta` can claim by publishing
`folderName: "@alpha"`. A case-only pair (`CSLA` / `csla`) is the same directory on the Windows
installer even though the strings differ.

Before this ticket nothing refused a second claim, so a catalog could collect collisions one
publish at a time and only a manual sweep would notice.

## Approach

- **One resolver.** `InstallClaim::resolve(directory, mod_id, folder_name)` is the sweep's
  resolution, extracted so detection and prevention cannot drift: `folderName` verbatim when it is
  not blank after a trim, otherwise `@<modId>` (the stored id, or the directory for a legacy entry
  whose id is empty), and `None` when `ModInstallDir` would refuse either name. `scan_catalog` now
  builds its claim map through it.
- **One check, three publish paths.** `ModStore::ensure_install_folder_available(claim, previous)`
  enumerates the catalog (`catalog_directories` + the sweep's non-mutating `stored_entry`), skips
  the entry being written, resolves every other entry the same way, and compares with
  `install_folder_key` (the lowercased folder — the Windows view).
  - `finalize_mod` (new entry, streamed) resolves the manifest `folderName` or the `@<slug>`
    fallback. A refusal deletes the finished temp upload, so nothing is placed.
  - `finalize_revision` resolves the meta value or the current revision's, and passes the entry's
    existing claim as `previous`; a refusal deletes the temp upload before any placement, and the
    current revision stays untouched.
  - `publish_mod` (in-memory path, tests today) carries no folder field, so it claims `@<slug>`.
- **Only a new claim is refused.** A revision that keeps its folder (including a case-only
  respelling) skips the lookup. Without that exemption, an entry whose own manifest is correct
  could be blocked from publishing by another owner's pre-rule entry, and there would be nothing it
  could change — `alpha`'s `@alpha` fallback cannot be renamed away. The sweep keeps such a pair
  visible until the entry that chose the folder moves.
- **HTTP 409.** The handler already maps a duplicate `modId` (`already exists`) to `CONFLICT`; the
  refusal message says `already claimed`, which now takes the same branch. The publisher CLI prints
  the response body unchanged (`publish rejected: HTTP 409: …`).

The lookup is not atomic: two publishers that start together can both list a catalog without each
other and both claim one folder. It is the same best-effort shape as the existing duplicate-`modId`
check, and the sweep remains the backstop. A conditional marker object would close it but adds
state that can itself drift.

## Behaviour

| publish | outcome |
| --- | --- |
| new entry, `folderName: "shared"`, another entry holds `shared` | refused, 409; message names the other directory |
| new entry, `folderName: "SHARED"`, another entry holds `shared` | refused; message shows both spellings and says they are one folder on a case-insensitive filesystem |
| new entry with no folder name, another entry holds `@<slug>` | refused (fallback claim) |
| new entry with no folder name, `@<slug>` free | published |
| new entry whose folder is only claimed by an uninstallable name (`a*b`, traversal) | published; that entry cannot install, so it holds nothing |
| revision keeping its folder, however the other entries look | published |
| revision moving onto another entry's folder, exact or case-only | refused; current revision and artifact unchanged |
| revision moving to a free folder | published |
| revision of a member of a pre-existing collision, keeping its folder | published; the sweep still reports the pair |

## Files

- `mserver/MasterService/src/mods.rs` — `InstallClaim::resolve`, `install_folder_key`, the
  `ensure_install_folder_available` check and its three call sites, four store tests and a
  `relabel_stored_folder` helper; the three collision-sweep fixtures now build their pairs by
  rewriting stored metadata.
- `mserver/MasterService/src/http.rs` — `already claimed` maps to 409 beside `already exists`.
- `mserver/MasterService/src/lib.rs` — one endpoint test.
- `docs/proteus/MSERVER-FOLDER-CLAIM-1.md`

## Evidence

Store tests (`mods.rs`):

- `publish_refuses_a_folder_name_another_entry_already_claims` publishes `alpha` with `shared`, then
  refuses `beta` with `shared` and with `SHARED`; the case-only message contains `as 'shared'`; no
  `beta` directory or entry is left behind.
- `publish_refuses_the_folder_another_entry_reaches_by_fallback` publishes `alpha` with no folder
  name, refuses `beta` claiming `@alpha`, then publishes the same `beta` manifest without a folder
  name (`@beta`).
- `a_revision_keeps_its_folder_but_cannot_take_another_entries` keeps `alpha-dir` on a revision,
  refuses both `beta-dir` and `BETA-DIR` with the revision still at 2 and the folder unchanged, then
  accepts a revision moving to `alpha-new`.
- `a_revision_under_a_preexisting_collision_still_publishes` rewrites `beta` to `@alpha` the way a
  pre-rule publish left it: `alpha`'s next revision publishes, and `scan_catalog` still reports the
  one collision.

The three pre-existing sweep fixtures (`…two_entries…`, `…shadows_the_mod_id_fallback`,
`…folds_case…`) can no longer create their pair through `finalize_mod`, so they publish distinct
folders and relabel one entry's stored `folderName`. That is not a workaround around the check but
a demonstration of it: the pair now only exists in a catalog that predates the rule.

Endpoint test (`lib.rs`): `http_mod_publish_refuses_a_folder_name_another_entry_already_claims`
publishes `folder-one-claim`/`shared`, gets 409 for `folder-two-claim`/`SHARED` with the message
naming `folder-one-claim` and `as 'shared'`, lists one entry, finds no `folder-two-claim` directory,
and publishes the same manifest with `shared-two` for 200.

Manual run on Windows against the built binary, with a temp SQLite directory and mods root:

```
$ curl -F modId=claim-alpha -F name=Alpha -F folderName=shared -F file=@alpha.pbo …
HTTP 200
$ curl -F modId=claim-beta -F name=Beta -F folderName=SHARED -F file=@beta.pbo …
folder 'SHARED' is already claimed by catalog entry 'claim-alpha' as 'shared' (one folder on a case-insensitive filesystem); publish with a different folderName
HTTP 409
$ curl -F modId=claim-beta -F name=Beta -F folderName=shared-two -F file=@beta.pbo …
HTTP 200
```

`mods/_incoming/` was empty afterwards (the refused temp upload was deleted), the catalog listed
two entries with distinct folders, and `check-names --mods-dir …` printed `checked 2 catalog
entries, no rejected names, no shared install folders, every artifact intact` with exit 0.

Counts: `papa-bear-master-service` **89 lib + 7 bin + 8 integration**, 0 failed (84 + 7 + 8 before;
+5 lib). `cargo clippy -p papa-bear-master-service --all-targets` exits 0 and reports no warning in
the new code; the pre-existing `too_many_lines` warning on `finalize_revision` remains, and the
check adds 12 lines inside it. `check-format` OK; `check-all` OK; roadmap regenerated (50 tickets).

## Risks

- The check is a catalog listing, not a reservation. Concurrent publishes of one folder can both
  pass, exactly as two simultaneous creates of one `modId` can; the sweep reports the result. An
  atomic `claims/<folder>` marker is the fix if concurrency ever makes this real, at the cost of a
  second source of truth that deletions and restores would have to maintain.
- Every publish, revision included, lists the catalog and reads each other entry's current
  metadata — two reads per entry beyond the entry being written. The catalog is small; if it grows,
  this becomes the dominant cost of `papa publish` and wants a cached claim index.
- The exemption for an unchanged claim means a pre-existing collision survives revisions from
  either member. That is deliberate: the sweep reports it, and the member that did not choose the
  folder has no remedy. A new entry or a changed folder is always checked.
- The fold is still `str::to_lowercase`, not the NTFS upcase table; exotic folds that Windows equates
  are not grouped here (`MSERVER-CHECK-COLLISIONS-1` records the same approximation for the report).
- The refusal happens in finalize, after the multipart body has been received, so the publisher
  spends the upload before hearing about it. The temp object is deleted; only bandwidth is wasted.
  A preflight query would need a new endpoint.
- `publish_mod` is the in-memory path and only tests call it today; the check is there so a future
  caller cannot bypass it, not because a live route uses it.

## Follow-ups

- A preflight (`--check-folder` on `papa publish`, or a catalog query the CLI can make before
  packing) would refuse before the upload rather than after it.
- A conditional claim marker, or a single-writer publish queue, would make the refusal atomic.
- `check-names --fix` stays absent: the fix is a republish with a different folder name.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo run -p papa-bear-master-service -- server --listen 127.0.0.1:8080 --admin-api-key dev --mods-dir <mods root>
papa publish <one.pbo>   --master http://127.0.0.1:8080 --admin-key dev --folder-name shared
papa publish <other.pbo> --master http://127.0.0.1:8080 --admin-key dev --folder-name SHARED
```

Expect the second publish to print
`publish rejected: HTTP 409: folder 'SHARED' is already claimed by catalog entry '<first>' as 'shared' (one folder on a case-insensitive filesystem); publish with a different folderName`,
and `check-names --mods-dir <mods root>` to end with `no shared install folders` and exit 0. A
revision that keeps its own folder must still publish.
