# MSERVER-CHECK-NAMES-1 — a `check-names` sweep for entries published before the rule

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Last follow-up of the name-rule chain (`MSERVER-FOLDERNAME-1`, `MSERVER-PATHCHARSET-1`,
`MSERVER-CLI-RULE-1`, `MSERVER-NORMALIZE-1`, `MSERVER-NAME-CASES-1`). Those stop new
refusals from being published; nothing could enumerate the entries that were published before
the rule existed, because the catalog cannot show them.

## Problem

`ModStore::get_mod` returns `None` for a `mod_id` that is not one path component, and
`read_all` resolves every directory through it — so `/v1/mods` silently omits exactly the
entries whose stored names the rule now rejects. A `folderName` that predates
`MSERVER-FOLDERNAME-1` is worse: the entry is still served with the hostile value, and that
value becomes `<modsRoot>/<folderName>` on every client that installs it. An operator had no
way to ask "which catalog entries carry a name the service itself would refuse today?".

## Approach

- `ModStore::scan_names() -> Result<NameScan>` (`mods.rs`) enumerates directory names straight
  from the store listing (`mod.json` / `current` markers) and reads each directory's current
  entry with a private `stored_entry`, which has **no** id gate and does **not** run
  `parse_metadata` — that function rewrites legacy metadata, and a sweep must not mutate the
  catalog.
- The report is one row per offending field, so an entry can list twice:
  `InvalidCatalogName { directory, field, value }` with `field` being `mod_id` or `folderName`
  and `value` verbatim (the report shows what is stored, not a normalization).
  - the directory name is not one component;
  - the stored `modId` differs from its directory and is not one component (it still ends up in
    `downloadUrl`);
  - `folderName` differs from its trim (the service only stores trimmed values) or is not one
    component. Absent, empty or whitespace-only is **not** a violation: it means "no folder
    name", the `@<modId>` fallback.
- Only the current revision is inspected. Older revisions are history; the current one is what
  `/v1/mods` serves and what a client installs next.
- `check-names` subcommand (`main.rs`): `--mods-dir <dir>`, or the `MODS_S3_*` bucket the server
  itself uses. One line per row, then a non-zero exit with a summary, so the sweep can gate a
  maintenance window:

```
old-mod: folderName '../..' is not one path component
padded: folderName '  CSLA  ' is not one path component
Error: check-names: 2 rejected names in 3 catalog entries
```

- `read_all` now uses the same `catalog_directories` helper, so the listing logic exists once.
- Renamed by the follow-up: `scan_names`/`NameScan` are now `scan_catalog`/`CatalogScan`, with the
  name rows under `rejected_names`, because the same pass also reports artifacts (see
  `MSERVER-CHECK-ARTIFACTS-1`).

## Behaviour

| stored state | reported |
| --- | --- |
| directory `a*b`, stored `modId` whatever | `mod_id 'a*b'` |
| directory `character-mod`, stored `modId` `a*b` | `mod_id 'a*b'` |
| `folderName` `../..`, `a*b`, `trailing.`, `  CSLA  ` | `folderName '<verbatim>'` |
| `folderName` absent / `""` / `"   "` | no (means `@<modId>`) |
| names that pass the one-component rule | no |

## Files

- `mserver/MasterService/src/mods.rs` — `InvalidCatalogName`, `NameScan`, `scan_names`,
  `catalog_directories`, `stored_entry`, the three store tests.
- `mserver/MasterService/src/main.rs` — `check-names` subcommand, `run_check_names`, summary
  and exit code.
- `mserver/MasterService/tests/check_names.rs` — the binary-level cases (new file).
- `docs/proteus/MSERVER-CHECK-NAMES-1.md`

## Evidence

Store tests: `scan_names_is_clean_for_a_published_catalog` (two published entries, no rows);
`scan_names_reports_a_stored_folder_name_the_rule_rejects` (rewrites the stored revision to
`..\..\Documents`, asserts `list_mods` still serves the entry **and** the sweep reports it);
`scan_names_reports_a_stored_mod_id_that_disagrees_with_its_directory` (`modId` `a*b` under
`character-mod`). `scan_names_reports_a_directory_whose_mod_id_is_not_one_component` asserts
the blind spot directly — `list_mods` returns nothing for a directory named `a*b` while the
sweep counts it and reports it.

Binary tests (`tests/check_names.rs`, via `CARGO_BIN_EXE_…`): a clean two-entry directory exits
0 with `check-names: checked 2 catalog entries, no rejected names`; a `folderName` of
`..\..\Documents` exits 1, prints
`bad-folder: folderName '..\..\Documents' is not one path component` and
`check-names: 1 rejected name in 1 catalog entry`. Both run on Windows; the third case
(directory named `a*b`) is `#[cfg(not(windows))]` because Windows cannot create such a
directory — it was compile-checked here by temporarily dropping the `cfg` and running
`cargo test --no-run`, but it can only *execute* on the Linux job.

Manual run, three legacy-style entries (one clean, `../..` and `  CSLA  `):

```
old-mod: folderName '../..' is not one path component
padded: folderName '  CSLA  ' is not one path component
Error: check-names: 2 rejected names in 3 catalog entries
```

Counts: `papa-bear-master-service` **77 lib + 7 bin + 2 integration**, 0 failed (74 lib before
this ticket; +3 store, +2 binary). One full `--lib` run flaked
`concurrent_updates_allocate_distinct_revisions_without_regressing_latest` at
`advancing concurrent-mod to revision 2` — the Windows rename race already recorded in
`MSERVER-FOLDERNAME-1` and `MSERVER-NORMALIZE-1`; that test uses no catalog names and the
next two full runs plus three isolated runs were green (77 passed). Clippy back at its baseline after the sweep's `is_clean`
became `const fn` and one long `if` condition moved its brace (31 lib / 40 lib test / 8 bin).
`check-format` OK.

## Risks

- A corrupt `mod.json` aborts the sweep with `parsing mod.json for <dir>` — the same behaviour
  `list_mods` has. One unreadable entry therefore hides the rest of the report; fixing the entry
  and re-running is the intended flow.
- Older revisions are not scanned. A hostile `folderName` that only exists in revision 1 is
  invisible once revision 2 is current — and is also not what clients install.
- On Windows the directory-name branch cannot exist, so only the Linux job exercises it.
- `stored_entry` parses `ModCatalogEntry` directly; a future field with a serde default would
  keep parsing, but one without would fail the sweep exactly where `list_mods` fails too.

## Follow-ups

- A `--fix` mode that rewrites trimmable folder names and refuses the rest is deliberately not
  included: renaming a catalog directory changes the store key, and any signed URL or client
  cache that already holds it.
- (done) The sweep also reports entries whose artifact is missing or whose `sha256` disagrees;
  see `MSERVER-CHECK-ARTIFACTS-1`.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo clippy -p papa-bear-master-service --all-targets
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
```

Against a production bucket, set `MODS_S3_*` exactly as the server does and omit `--mods-dir`;
expect exit 0 on a clean catalog, and one line per offending value plus a non-zero exit
otherwise.
