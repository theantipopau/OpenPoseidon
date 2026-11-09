# MSERVER-ORPHAN-CLAIMS-1 — a leftover claim is named by the sweep and removable by `--fix`

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-FOLDER-RESERVE-1`. That ticket made the install folder a
conditional reservation, so two concurrent publishes cannot both pass — and, in exchange, a publish
that stops between taking the claim and making its entry current leaves a folder refused to
everyone with nothing in the catalog to explain it.

## Problem

`_claims/<hex>.json` is removed by three paths, all of them in the same process that wrote it: the
publish's own failure path (`release_reservation`, only for a claim this call created), a revision
that moves folders, and `delete_mod`. A crash, a killed process or a failed delete skips all three,
and the object stays:

```
$ papa publish … --mod-id ghost --folder-name shared      # killed after the conditional put
_claims/736861726564.json = {"modId":"ghost","folder":"shared"}
$ papa publish … --mod-id alpha --folder-name shared
Error: publish rejected: HTTP 409: folder 'shared' is already claimed by catalog entry 'ghost' …
```

The refusal is correct but unrecoverable from the catalog alone: `GET /v1/mods` cannot show a
claim, `check-names` did not look at `_claims/`, and the operator's only option was to know the
object layout and delete the file by hand. The sweep already audits the other two ways a catalog
can be wrong (a name the rule rejects, an artifact that no longer hashes); a claim with no entry is
the third, and it is the only one that blocks future publishes rather than failing downloads.

## Approach

- **The scan compares claims with the directories it just read.** `scan_catalog` keeps the
  `BTreeSet` from `catalog_directories` and, after the entry pass, lists `_claims/` once:
  `read_claim` gives the entry the claim names, and a claim whose `modId` is not in that set is an
  `OrphanedClaim { path, claim, claimed_at }`. The same set is used for the name, collision and
  artifact checks, so a claim cannot be reported against a different snapshot than its entry was.
  Staged local-backend objects (`<hex>.json#1`) are skipped, as in `release_claims_of`; an object
  listed and then deleted before the read is dropped, not an error. `CatalogScan::is_clean` now
  covers the new list.
- **`claimed_at` is the object's modification time.** A claim is taken at finalize, *after* the
  upload, and the entry follows within a few object writes — so a claim that is minutes old cannot
  belong to a publish still working, and one that is seconds old can. That is the only signal the
  store has without adding a lease or a commit record.
- **`--fix` removes the old ones.** `ModStore::remove_orphaned_claims(orphans, min_age)` deletes
  every claim at least `min_age` old and answers with the paths removed; a delete that finds the
  object already gone counts as removed, so two sweeps (or a `delete_mod`) racing is not an error.
  `check-names --fix` passes five minutes (`ORPHAN_CLAIM_MIN_AGE`) and a run without the flag stays
  read-only. A skipped claim keeps the run failing and says why.
- **The rows and the summary.** One row per claim, in the same shape as the other findings:
  `orphan claim: 'shared' is reserved by 'ghost', which is not a catalog entry (claimed 3h 0m ago)`,
  with `; removed` or `; kept, a publish may still be finishing` appended under `--fix`. A removed
  claim no longer counts as a problem, so the run exits 0 once the folder is free — otherwise
  `--fix` would fail on the thing it just fixed. The clean sentence gains `no orphaned folder
  claims`, and the failing summary counts `N orphaned claims` beside the existing categories.

## Behaviour

| situation | sweep |
| --- | --- |
| claim names a catalog directory | not reported; the claim is how the entry holds its folder |
| claim names no catalog directory | one row with folder, claimed entry and age; exit 1 |
| `--fix`, claim ≥ 5 minutes old | row says `removed`, the object is deleted, the run can pass |
| `--fix`, claim younger than 5 minutes | row says `kept, a publish may still be finishing`; exit 1 |
| `_claims` absent, empty, or holding a staged `<hex>.json#1` | nothing reported |
| claim body unreadable | the sweep fails with the object path rather than guessing |

## Files

- `mserver/MasterService/src/mods.rs` — `OrphanedClaim`, `CatalogScan::orphaned_claims` (and
  `is_clean`), `ModStore::orphaned_claims`, `ModStore::remove_orphaned_claims`; the directory set is
  captured for both halves; two store tests.
- `mserver/MasterService/src/main.rs` — `check-names --fix`, `ORPHAN_CLAIM_MIN_AGE`, `format_age`,
  `sweep_orphaned_claims`, the row and summary wording.
- `mserver/MasterService/tests/check_names.rs` — `write_claim`/`age_claim`, four binary tests, the
  updated clean sentence.
- `docs/proteus/MSERVER-ORPHAN-CLAIMS-1.md`

## Evidence

Store tests (`mods.rs`):

- `scan_catalog_reports_a_claim_that_names_no_catalog_entry` — a reservation taken directly for
  `alpha`/`shared` (no entry, the crash shape) is the one orphan, its folder and directory are
  reported, and the scan is not clean; publishing `Beta`/`beta-dir` afterwards leaves the orphan
  list at one, so a claim an entry holds is not reported.
- `removing_an_orphaned_claim_frees_its_folder` — the orphan refuses `Beta`/`shared`; removing it
  with a one-hour minimum removes nothing and leaves the object, removing it with zero deletes it
  and leaves one fewer claim; the scan is clean, `Beta` publishes into the folder, and the scan
  stays clean with the new claim.

Binary tests (`tests/check_names.rs`, real `papa-bear-master-service` against temp directories):

- `a_claim_that_names_no_catalog_entry_is_reported` — exact row and `1 orphaned claim in 1 catalog
  entry`, exit 1.
- `a_claim_held_by_a_catalog_entry_is_not_reported` — the same folder with an entry holding it is
  clean and prints `no orphaned folder claims`.
- `fixing_removes_an_orphaned_claim_and_passes` — a claim backdated ten minutes is removed by
  `--fix`, the row says `; removed`, `check-names: removed 1 orphaned install-folder claim` is
  printed, the object is gone, and a second sweep exits 0.
- `fixing_keeps_a_claim_a_publish_may_still_be_finishing` — a fresh claim survives `--fix`, the row
  says `; kept, a publish may still be finishing`, and the run still fails.

Broken-state delta: with the `scan.orphaned_claims = …` assignment removed,
`removing_an_orphaned_claim_frees_its_folder` fails on `left: 0, right: 1` and three binary tests
fail by name (`a_claim_that_names_no_catalog_entry_is_reported`,
`fixing_keeps_a_claim_a_publish_may_still_be_finishing`,
`fixing_removes_an_orphaned_claim_and_passes`); restoring it is green.

Counts: `papa-bear-master-service` **99 lib + 7 bin + 12 integration**, 0 failed (97 + 7 + 8
before; +2 store, +4 binary). `papa-bear-cli` 19 unit + 4 integration and `papa-bear-client` 15,
unchanged and green. `cargo clippy -p papa-bear-master-service --all-targets` has no warning in the
new code: lib stays at its 32, and the bin drops from 9 to 8 because splitting
`sweep_orphaned_claims` out of `run_check_names` put that function back under the 100-line
`too_many_lines` threshold. `rustfmt --check` on the three files is clean; `check-format` and
`check-all` OK; roadmap regenerated.

Manual run on Windows against the built binary, a temp mods root with one valid entry:

```
$ papa-bear-master-service check-names --mods-dir tmp/orphan-live/mods
orphan claim: 'shared' is reserved by 'ghost', which is not a catalog entry (claimed 0s ago)
Error: check-names: 1 orphaned claim in 1 catalog entry            (exit 1)

$ papa-bear-master-service check-names --fix --mods-dir tmp/orphan-live/mods
orphan claim: 'shared' is reserved by 'ghost', which is not a catalog entry (claimed 0s ago; kept, a publish may still be finishing)
Error: check-names: 1 orphaned claim in 1 catalog entry            (exit 1, object still there)

# LastWriteTime moved back three hours, then:
$ papa-bear-master-service check-names --fix --mods-dir tmp/orphan-live/mods
orphan claim: 'shared' is reserved by 'ghost', which is not a catalog entry (claimed 3h 0m ago; removed)
check-names: removed 1 orphaned install-folder claim
check-names: checked 1 catalog entry, no rejected names, no shared install folders, no orphaned folder claims, every artifact intact   (exit 0)

$ ls tmp/orphan-live/mods/_claims
$ papa-bear-master-service check-names --mods-dir tmp/orphan-live/mods
check-names: checked 1 catalog entry, no rejected names, no shared install folders, no orphaned folder claims, every artifact intact   (exit 0)
```

Temp state removed.

## Risks

- **The five minutes are a heuristic, not knowledge.** A publish takes its claim after the upload
  and finishes within a few object writes, so a stalled writer holding a claim for longer than the
  grace would have it removed under it. The alternative — a lease — can expire under a slow writer
  the same way, and a commit record is the bigger change `MSERVER-FOLDER-RESERVE-1` already named
  as a follow-up.
- **`--fix` cannot free a folder that a pre-reservation entry holds without a claim.** There is
  nothing to report or remove; that protection is the catalog read, as before.
- **A claim naming an entry that exists but no longer owns that folder is not reported.** That is a
  failed `release_claim_for` after a move, not a claim with no entry. It still refuses the old
  folder to a new publisher, so the sweep is incomplete for that shape (see follow-ups).
- **A malformed claim body fails the sweep.** `read_claim` parses the object and an error names the
  path; the sweep does not skip a file it cannot understand, because skipping would hide exactly
  the object an operator needs to see.
- One extra `_claims/` listing per sweep, and one delete per removed claim. `--fix` is N deletes on
  S3 and has no batch form; a catalog with a long crash history pays for it once.

## Follow-ups

- (done) Report a claim whose folder disagrees with the entry's current revision (a failed release
  after a move), with the same `--fix`; the entry is the authority on which claim it should hold.
  See `MSERVER-STALE-CLAIMS-1.md`.
- Reserve before the upload (a claim endpoint with a token plus commit/abort), so the losing
  publisher stops before streaming and the claim lifetime covers only the transfer.

## Superseded

The first follow-up landed in `MSERVER-STALE-CLAIMS-1.md`: `OrphanedClaim` became `UnusedClaim`
with a `reason`, so one list, one age rule and one `--fix` cover both a claim that names no entry and
a claim left on a folder its entry has moved off. The shapes, rows and count above that say "orphaned
claims" now read "unused claims"; the orphan row itself is unchanged, and the risk that named a
failed release after a move as unseen no longer applies.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
# create a leftover by hand: {"modId":"ghost","folder":"shared"} in <mods root>/_claims/<hex>.json
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>          # row, exit 1
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # kept, exit 1
# backdate the file five minutes, then:
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # removed, exit 0
```

Expect the row to name the folder, the entry and the claim's age; a fresh claim to survive `--fix`
with `kept, a publish may still be finishing`; and the second `--fix` to delete the object and exit
0. `papa publish … --folder-name shared` must then succeed instead of answering `409`.
