# MSERVER-PREFLIGHT-1 — a publish is refused before it packs and uploads, not after

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-FOLDER-CLAIM-1`. That ticket made the upload refuse a
folder another catalog entry already holds, but the refusal arrived after the whole multipart
body had been received — and only the service can see the catalog it refuses against.

## Problem

An entry installs into `<modsRoot>/<folderName>`, or `@<modId>` when it carries none, and the
install transaction clears that directory before unpacking, so two entries in one folder replace
each other. `POST /v1/mods` now answers `409` for the second claim, but:

- the publisher spends the upload before hearing it, and `papa publish` packs a folder first
  (`Pbo::pack_dir`) and zstd-compresses it at level 19 — minutes for a multi-GB mod — so the
  rejected work is the expensive part, not the discarded temp object;
- the conflict is not visible in the manifest. The fallback is the invisible case (`alpha` with no
  folder name occupies `@alpha`, which `beta` can claim by publishing `folderName: "@alpha"`), and
  a case-only pair (`CSLA` / `csla`) is one directory on the shipped Windows client.

`papa publish` already asks the master one question before it packs — `GET /v1/mods/<id>`, to
decide between a first publish and a revision. The folder is the second thing it cannot know.

## Approach

- **One answer, read-only.** `ModStore::check_install_folder(mod_id, folder_name)` reads the
  entry's current revision with the non-mutating `stored_entry`, resolves the folder the publish
  would claim (`folderName` verbatim; a revision that names none keeps the one it holds, exactly
  as `finalize_revision` decides; otherwise `@<modId>`), and asks `install_folder_holder`. That is
  the lookup extracted from `ensure_install_folder_available`, so the preflight and the upload
  cannot answer differently. `folder_claim_refusal` words the refusal once for both; a revision
  that keeps its folder skips the lookup, so an entry can never be blocked by itself or by a pair
  that predates the rule.
- **`GET /v1/catalog/folder-claim`.** Query parameters `modId` or `name` (the id is derived from
  the name as a first publish does), and `folderName`. It answers `200` with
  `InstallFolderClaim { modId, folder, available, claimedBy, refusal }`: `available: false` names
  the other entry and carries the exact message the upload would fail with. Bad names are `400`,
  mods disabled is `503`; the route is public like `GET /v1/mods`, because it reveals only which
  folders the public catalog already resolves to. It lives under `/v1/catalog/` rather than
  `/v1/mods/<something>` so a mod carrying that id is never shadowed by the route.
- **`papa publish` asks before it packs.** The call sits after the identity/name rules and the
  new-vs-revision lookup, and before `run_upload` — which is where `build_artifact_reader` and
  `compress_artifact` live. A refusal prints `publish refused before uploading: <message>` and
  exits 1 with nothing packed and nothing sent. The folder name is percent-encoded per UTF-8 byte,
  so a name with a space, `&`, `+` or a non-ASCII letter is the value the publisher meant.
- **An older master does not block a publish.** A `404` on the query means the server predates the
  endpoint: the CLI warns on stderr and continues, because the upload still runs the same check.
  The fallback is "no early answer", never a publish the catalog would have refused.

## Behaviour

| publish | before packing | upload |
| --- | --- | --- |
| new entry, `--folder-name shared`, another entry holds `shared` | refused, exit 1, message names the other entry | never sent |
| new entry, `--folder-name SHARED`, another entry holds `shared` | refused; message shows both spellings | never sent |
| new entry with no folder name, another entry holds `@<slug>` | refused (the fallback claim) | never sent |
| new entry with no folder name, `@<slug>` free | allowed | published |
| revision keeping its folder (even respelled in another case) | allowed; the entry cannot block itself | published |
| revision moving onto another entry's folder | refused | never sent |
| master predating the endpoint | warning, continue | still refused by the upload if claimed |

## Files

- `mserver/MasterService/src/mods.rs` — `check_install_folder`, `install_folder_holder` extracted
  from the publish check, `folder_claim_refusal`; `slugify` is `pub(crate)` for the name-derived
  id; two store tests.
- `mserver/MasterService/src/model.rs` — `InstallFolderClaim`.
- `mserver/MasterService/src/service.rs` — one passthrough.
- `mserver/MasterService/src/http.rs` — the route, `FolderClaimQuery`, the handler, OpenAPI path
  and schema.
- `mserver/MasterService/src/lib.rs` — one endpoint test and the OpenAPI assertions.
- `mserver/CLI/src/main.rs` — `check_folder_claim`, `percent_encode_query_value` and the call in
  `run_publish`; one encoding test.
- `mserver/CLI/tests/cli_roundtrip.rs` — a stub master and two binary tests.
- `docs/proteus/MSERVER-PREFLIGHT-1.md`

## Evidence

Store tests (`mods.rs`):

- `check_install_folder_answers_before_a_publish_writes` — a published `alpha`/`shared` makes
  `shared` and `SHARED` unavailable for `beta` (the case-only refusal names `as 'shared'`), while
  `shared-two` is free; an entry with no folder name holds `@omega`, so `@omega` is refused for
  another id; the questions create no entry and no directory.
- `check_install_folder_keeps_a_revisions_own_folder` — a revision naming no folder gets its
  current `alpha-dir`, a case-only respelling is allowed, moving onto `beta-dir` is refused; after
  `epsilon` is rewritten onto `delta`'s `@delta` fallback the way a pre-rule publish left it,
  `delta` may keep its folder while a new entry may not take it.

Endpoint test (`lib.rs`): `http_folder_claim_answers_before_an_upload` publishes
`claim-alpha`/`shared` through the real router, then reads
`/v1/catalog/folder-claim?modId=claim-beta&folderName=SHARED` (200, `available: false`,
`claimedBy: claim-alpha`, the refusal text), a free folder (200, available), the entry's own folder
with no `folderName` (200, `shared`, available), and `?name=Claim%20Beta` (200, id
`claim-beta`, folder `@claim-beta`). Three bad queries are 400; the catalog still lists one entry
and neither `claim-beta` nor `@claim-beta` was created. The OpenAPI test asserts both the new path
and the schema.

CLI tests: `folder_claim_query_values_are_percent_encoded` covers the reserved set, `@`, space,
`&`, `+`, `=`, `%` and non-ASCII. `cli_roundtrip.rs` runs the real binary against a stub master:
`publish_refuses_a_claimed_folder_before_packing_or_uploading` answers the catalog lookup 404 and
the preflight `available: false`, points the source at a path that cannot pack, and asserts the
claim refusal appears while `is neither a folder nor a file` does not — the claim was checked
first — with the two request lines recorded. `publish_reaches_the_source_once_the_claim_is_free`
answers `available: true` and asserts the pack error follows, with `folderName=Free%20Folder`
arriving as one encoded value.

Counts: `papa-bear-master-service` **92 lib + 7 bin + 8 integration**, 0 failed (89 + 7 + 8
before; +2 store, +1 endpoint). `papa-bear-cli` **19 unit + 4 integration**, 0 failed (18 + 2
before). `papa-bear-client` 15 unchanged. `cargo clippy -p papa-bear-master-service -p
papa-bear-cli --all-targets` exits 0 with no warning in the new code. `check-format` OK;
`check-all` OK; roadmap regenerated.

Manual run on Windows against the built binaries, with a temp SQLite directory and mods root:

```
$ papa publish <alpha folder> --mod-id alpha --name Alpha --folder-name shared
published 'Alpha' as alpha, version 9fee5cd2, package revision 1 (52 bytes compressed) -> …
$ curl "…/v1/catalog/folder-claim?modId=beta&folderName=SHARED"
{"modId":"beta","folder":"SHARED","available":false,"claimedBy":"alpha","refusal":"folder 'SHARED' is already claimed by catalog entry 'alpha' as 'shared' (one folder on a case-insensitive filesystem); publish with a different folderName"}
$ papa publish <missing path> --mod-id gamma --name Gamma --folder-name SHARED
Error: publish refused before uploading: folder 'SHARED' is already claimed by catalog entry 'alpha' as 'shared' (…)
$ papa publish <alpha folder> --mod-id beta --name Beta --folder-name shared-two
published 'Beta' as beta, …
```

The missing path proves the ordering: the source error would have said `is neither a folder nor a
file`. `mods/_incoming/` stayed empty, the catalog listed the two entries, and `check-names
--mods-dir` printed `checked 2 catalog entries, no rejected names, no shared install folders,
every artifact intact` with exit 0. Temp state removed and the server stopped.

## Risks

- One extra request per publish. It is a catalog listing on the service side (two reads per other
  entry, the same cost the upload already pays), and it is dwarfed by the pack it can save.
- Still not a reservation: two publishers can both see a folder free and both upload, exactly as
  they can both create one `modId`. The upload check is the authority; the preflight only moves
  the common case earlier. An atomic claim marker is the fix if that ever becomes real.
- The answer can go stale between the query and the upload. The upload re-checks, so the failure
  mode is a late 409, not a bad catalog.
- A master that predates the endpoint silently narrows the preflight to the upload check. The
  warning on stderr is the only signal; it should be removed (back to a hard failure) once the
  deployed master serves the route.
- The preflight requires the catalog to be reachable before the source is read at all, so a
  network outage now fails earlier than the pack did. That is the same failure a publish would hit
  a moment later.

## Follow-ups

- An atomic claim marker (conditional put) would make both the preflight and the upload exact,
  instead of two compatible best-effort checks.
- Once production runs this version, drop the `404` fallback so a missing endpoint is loud.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- server --listen 127.0.0.1:8080 --admin-api-key dev --mods-dir <mods root>
papa publish <one>   --master http://127.0.0.1:8080 --admin-key dev --mod-id alpha --name Alpha --folder-name shared
papa publish <other> --master http://127.0.0.1:8080 --admin-key dev --mod-id beta  --name Beta  --folder-name SHARED
```

Expect the second publish to stop with
`publish refused before uploading: folder 'SHARED' is already claimed by catalog entry 'alpha' as 'shared' (one folder on a case-insensitive filesystem); publish with a different folderName`,
before any packing output, and `GET /v1/catalog/folder-claim?modId=beta&folderName=SHARED` to
report the same answer as JSON. A revision that keeps its own folder must still publish.
