# MSERVER-CHECK-COLLISIONS-1 — the `check-names` sweep reports entries that share one install folder

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Third follow-up of `MSERVER-CHECK-NAMES-1`, and the one `MSERVER-CHECK-ARTIFACTS-1` listed as "a
catalog-level collision the per-entry pass cannot see". The sweep could ask whether each entry is
well-named and downloadable. It could not ask whether two entries would land in the same folder.

## Problem

The install destination is `<modsRoot>/<folderName>`, or `<modsRoot>/@<modId>` when the entry
carries no folder name: `papa install` builds exactly that (`CLI/src/main.rs`, `run_install`) and
the engine builds it again (`ModInstallDir`, `engine/Poseidon/Core/ModInstall.cpp`). The install
transaction then clears the destination before unpacking — `decode_and_unpack` removes the existing
directory, and the engine's swap discards the preserved one.

So two catalog entries that resolve to one folder are not two mods lying side by side; the second
install deletes the first one's files. Both entries are individually valid: the names pass
`is_single_path_component`, the artifacts hash, each `mod.json` is honest. Publishing could not catch
it at the time either — the publisher sees its own manifest, not the rest of the catalog; the
publish-time refusal that closes this arrived later in `MSERVER-FOLDER-CLAIM-1`.

Two shapes of the same collision:

- the same string twice, which fails on every filesystem;
- two spellings that differ only in case (`CSLA` / `csla`). The installer ships on Windows, where
  those are one directory, so this is the same silent overwrite even though the strings differ.

The fallback is the subtle case. An entry with no `folderName` claims `@<modId>`, and nothing stops
another entry from publishing `folderName: "@alpha"` while `alpha` itself has no folder name. The
two manifests never mention each other.

## Approach

- One pass, as before. `scan_catalog` already walks every current entry; it now records each one's
  resolved folder into a `BTreeMap<String, Vec<InstallClaim>>` (`folder.to_lowercase()` ->
  claims) while it checks names and artifacts, so the collision half cannot see a different catalog
  than the other two.
- Resolution is the installer's, not an approximation: `folderName` **verbatim** when it is not
  blank after a trim (the CLI's `filter(|s| !s.trim().is_empty())`), otherwise `@<modId>` using the
  served id — the stored `modId` when non-empty, else the directory, which is what
  `parse_metadata` does for a legacy entry.
- The grouping key is the folder lowercased. `InstallClaim` keeps the as-written folder beside the
  catalog directory, so a collision report can show *which* entry writes *which* spelling rather
  than only the folded key.
- An entry that cannot be installed is left out of the map. `ModInstallDir` refuses the join when
  either the id or the folder is not one path component, so an uninstallable entry can never
  overwrite an installable one; its names are already reported as `rejected_names`, and counting
  them again as a collision would be duplicate noise.
- Groups of more than one become `InstallCollision { claims }`, ordered by folded folder
  (`BTreeMap`) and by directory within a group (`catalog_directories` is a `BTreeSet`), so two runs
  over one catalog print the same report.
- `check-names` prints one row per group, between the name rows and the artifact rows. A group whose
  claims all spell the folder the same way uses the plain row; a mixed-case group names each entry
  with its own folder:

```
install collision: 'first-mod' and 'second-mod' install into the same folder 'shared'
install collision: 'csla-lower' ('csla') and 'csla-upper' ('CSLA') name one folder on a case-insensitive filesystem
Error: check-names: 2 install collisions in 5 catalog entries
```

A clean sweep now reads `check-names: checked N catalog entries, no rejected names, no shared
install folders, every artifact intact`.

## Behaviour

| current entries | reported |
| --- | --- |
| two entries with `folderName: "shared"` | one row: both directories, folder `shared` |
| `alpha` with no folder name, `beta` with `@alpha` | one row: folder `@alpha` |
| `CSLA` and `csla` | one row, with both spellings, as one folder on a case-insensitive filesystem |
| `CSLA`, `csla` and `Csla` | one row listing all three claims |
| three entries claiming `shared` | one row listing all three |
| two entries whose `folderName` is `a*b`, `..\..\x` or padded | no row; both are `rejected_names` (the installer never joins them) |
| every folder distinct | no row |

## Files

- `mserver/MasterService/src/mods.rs` — `InstallClaim`, `InstallCollision` (with `spells_one_folder`
  and `folder`), the new `CatalogScan` field and `is_clean` term, the claim map in `scan_catalog`,
  the doc paragraph on the sweep, four store tests and a `claim` helper.
- `mserver/MasterService/src/main.rs` — the two collision row shapes, `quoted`, `joined`, the
  collision count in the summary, the clean line.
- `mserver/MasterService/tests/check_names.rs` — three new binary-level cases, plus the clean
  message.
- `docs/proteus/MSERVER-CHECK-COLLISIONS-1.md`

## Evidence

Store tests (`mods.rs`):
`scan_catalog_reports_two_entries_that_install_into_one_folder` publishes three entries — two with
`shared`, one with `solo` — and asserts exactly one collision naming both claims while
`rejected_names` and `damaged_artifacts` stay empty (the case that a per-entry check cannot see),
plus `spells_one_folder() == true`;
`scan_catalog_catches_a_folder_name_that_shadows_the_mod_id_fallback` publishes `alpha` with no
folder name and `beta` with `@alpha`, and asserts one collision on `@alpha`;
`scan_catalog_folds_case_when_matching_install_folders` publishes `CSLA` and `csla` and asserts one
collision carrying both spellings and `spells_one_folder() == false`;
`scan_catalog_does_not_collide_install_folders_the_installer_refuses` rewrites two published
entries to `folderName: "a*b"` and asserts two rejected names and zero collisions.

Binary tests (`tests/check_names.rs`, via `CARGO_BIN_EXE_…`):
`two_entries_that_install_into_one_folder_are_reported` asserts the exact row and
`check-names: 1 install collision in 2 catalog entries` with exit 1;
`a_folder_name_that_shadows_the_fallback_is_a_collision` does the same for `alpha`/`beta`, which
have different-looking manifests; `case_only_install_folders_are_a_collision` asserts the
case-insensitive row with both spellings.

Manual runs on Windows against a generated store of five entries (`shared` twice, `csla`/`CSLA`,
one `solo`):

```
install collision: 'csla-lower' ('csla') and 'csla-upper' ('CSLA') name one folder on a case-insensitive filesystem
install collision: 'first-mod' and 'second-mod' install into the same folder 'shared'
Error: check-names: 2 install collisions in 5 catalog entries
```

and on a mixed catalog (one collision plus one deleted artifact):

```
install collision: 'first-mod' and 'second-mod' install into the same folder 'shared'
broken-mod: revision 1 artifact is missing
Error: check-names: 1 install collision, 1 damaged artifact in 3 catalog entries
```

A clean two-entry directory printed `check-names: checked 2 catalog entries, no rejected names, no
shared install folders, every artifact intact` and exited 0.

Counts: `papa-bear-master-service` **84 lib + 7 bin + 8 integration**, 0 failed (80 + 7 + 5 before
the collision work; +4 store, +3 integration). Clippy back at its baseline after the new doc
comments were split at their first paragraph (31 lib / 40 lib test / 8 bin, no warning in the new
code). `check-format` OK; `check-all` OK; roadmap regenerated.

One full-package run failed a single lib test before the case fold and its name was lost to the
pipeline (the run was filtered to result lines); the fourteen full runs afterwards — eight of the
whole package, six of `--lib` — were green at 83/0, and every run after the fold was green at 84/0.
The new cases allocate a per-test temp directory and perform no renames, so nothing here shares the
known Windows rename path, but the failure is recorded rather than smoothed over.

## Risks

- The fold is `str::to_lowercase`, an approximation of NTFS's own upcase table rather than a model
  of it. For the ASCII and accented-Latin names mods use it agrees with Windows; exotic folds
  (`ß`/`SS`, dotless `ı`, expansion cases) are not guaranteed. A pair that Windows treats as one
  folder but this fold does not would still be missed.
- A case-only pair fails the sweep even if every client were on a case-sensitive filesystem, where
  the two folders are genuinely distinct. Accepted: the installer is a Windows game client, and the
  failure mode there is silent deletion of another mod's files.
- Only the current revision of each entry participates, consistent with the other halves: an old
  revision's folder name is not what a client installs.
- An entry whose current `mod.json` is unreadable is skipped by this half too — there is no name to
  resolve — and the run already aborted on the parse error.
- The collision is reported, not prevented *by this ticket*. A new claim is refused at publish time
  since `MSERVER-FOLDER-CLAIM-1`; the sweep stays the audit for catalogs that predate it and is the
  only view that sees a pair two concurrent publishes slipped through.

## Follow-ups

- Model Windows' upcase table instead of `to_lowercase`, or gate folding on the client platform, if
  the catalog ever serves clients that do not fold case.
- (done) Refuse a publish whose `folderName` is already claimed by another entry; it changes the
  publish contract and waited on an owner decision — see `MSERVER-FOLDER-CLAIM-1`.
- A `--fix` mode is absent for the same reason as in `MSERVER-CHECK-NAMES-1`: the fix is a
  re-publish with a different folder name, not an edit of stored metadata.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
```

Expect `… no shared install folders …` and exit 0 on a healthy catalog; one
`install collision: …` line per shared folder — the plain row for one spelling, the
case-insensitive row for a case-only pair — alongside any name and artifact rows, and exit 1
otherwise. Against production, set `MODS_S3_*` exactly as the server does and omit `--mods-dir`.
