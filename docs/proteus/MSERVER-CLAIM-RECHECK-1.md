# MSERVER-CLAIM-RECHECK-1 — `--fix` re-reads a claim before deleting it, so a folder taken after the scan survives

Status: done-unverified (audit command; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-CLAIM-MOVES-1`. That ticket made a leftover decidable from
the claim body and the catalog; this one stops the sweep from acting on a decision about an object
that no longer exists.

## Problem

The sweep is two steps with a gap: `scan_catalog` lists `_claims/` and reads every claim, then
`--fix` deletes from that list. Between them a stale claim can be released — by another sweep, a
`delete_mod`, or a failed release that was retried — and the folder taken by a different publish,
which writes its own claim at the same object path:

```
scan:  _claims/<hex alpha-dir>.json = {"modId":"alpha","folder":"alpha-dir"}
       alpha now installs into alpha-new, so the scan decides "stale claim: removed"
       ... another sweep releases it, beta publishes into alpha-dir ...
fix:   _claims/<hex alpha-dir>.json = {"modId":"beta","folder":"alpha-dir"}
```

The sweep then deletes on a decision about the old object and hands a live folder to a third
publisher — the overwrite the claim exists to prevent. The same gap exists for an orphan whose
folder is taken between the scan and the delete.

## Approach

- **The scan keeps what it decided on.** `UnusedClaim` carries the claim body it read (`body:
  StoredClaim`, private, so only the store can build one) beside the write time it already kept in
  `claimed_at`; `StoredClaim` gains `Debug, Clone, PartialEq, Eq` for the comparison.
- **A read that also returns the write time.** `read_claim_state` returns the body and
  `GetResult::meta.last_modified`; `read_claim` is a thin wrapper, so the holders, the reservation
  and the tests are unchanged.
- **Re-read immediately before deleting.** `remove_unused_claims` reads each unused claim again. A
  different body, or a write newer than the scan saw, answers `retaken` and the object is left
  alone; the same body with the same or an older write goes as before, and `NotFound` still counts
  as removed because the sweep was right and someone got there first.
- **The write time matters on its own.** A body check alone would miss the same entry taking the
  same folder again: a release and a fresh publish of one mod write an identical
  `{ "$modId", "folder" }`, and the new object is the only thing that says so.
- **The sweep answers with both lists.** `UnusedClaimSweep { removed, retaken }` replaces the bare
  `Vec<ObjectPath>`. A `check-names` row for a retaken claim reads `; kept, the claim changed after
  the scan`, and the claim still counts as unused in the failing summary — the next scan decides
  about the object that now stands, and a live claim for a live entry makes it clean.
- **The window narrows, it does not close.** `object_store` has no conditional delete, so a write
  between the re-read and the delete is still lost; that interval is now one delete instead of the
  whole scan. Closed by `MSERVER-CLAIM-CAS-1`: the re-read and the delete run inside the claim's
  write window, and a writer that changes the body takes the same window first.

## Behaviour

| scan saw | object at delete time | `--fix` |
| --- | --- | --- |
| an unused claim | same body, same or older write | removed |
| an unused claim | a different body (folder taken by another entry) | kept, `; kept, the claim changed after the scan`, still fails |
| an unused claim | the same body, written after the scan | kept, same wording |
| an unused claim | gone | counted removed (another sweep released it) |

## Files

- `mserver/MasterService/src/mods.rs` — `StoredClaim` derives; `UnusedClaim::body`;
  `UnusedClaimSweep`; `read_claim_state` beside `read_claim`; `remove_unused_claims` re-reads and
  answers both lists; `unused_claims` stores the body; two store tests and five call sites adapted.
- `mserver/MasterService/src/main.rs` — the `UnusedClaimSweep` import, the sweep doc, the new kept
  wording, the summary and count from `sweep.removed`.
- `docs/proteus/MSERVER-CLAIM-RECHECK-1.md`

## Evidence

Store tests (`mods.rs`):

- `a_sweep_leaves_a_claim_taken_again_after_the_scan` — publish `alpha` into `alpha-dir`, move it to
  `alpha-new` with `add_revision` (which releases `alpha-dir`), recreate the failed-release claim,
  and the scan reports one `MovedAway` claim. The claim is then deleted and `beta` publishes into
  `alpha-dir` — both after the scan — and a sweep holding that scan answers `removed: []` and
  `retaken: [the same path]`. The object still reads `beta` / `alpha-dir`, the catalog scan is
  clean, and `beta` holds the folder.
- `a_sweep_leaves_a_claim_written_again_after_the_scan` — the same leftover backdated three hours
  (so the age floor on a possible running publish could not keep it either), scanned, then rewritten
  with an identical body. The sweep over the old scan leaves it and names it retaken, which is what
  the write-time comparison is for; a fresh scan reads the object that stands, nothing writes after
  it, and that sweep removes it, leaving the catalog clean.

Broken-state delta, measured together and then restored:

- with the re-read block removed, both new tests fail on `UnusedClaimSweep { removed: [_claims/
  616c7068612d646972.json], retaken: [] }` — the sweep deleted the folder it had been told to
  inspect;
- with only the write-time comparison dropped (`body != unused.body` alone), the body-changed test
  passes and the identical-body test fails the same way, so the two halves are both load-bearing.

Counts: `papa-bear-master-service` **104 lib + 7 bin + 15 integration**, 0 failed (102 + 7 + 15
before; +2 store). `papa-bear-cli` 19 + 4 and `papa-bear-client` 15, unchanged and green. One
default-parallel `--lib` run hit the known Windows local-store rename race on
`scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` (`advancing alpha to revision 2`,
`Access is denied`); the serial run and the next parallel run were green at 104/0. `rustfmt --check`
on the two files is clean. `cargo clippy -p papa-bear-master-service --all-targets` exits 0 with no
diagnostic in the new items (the `mods.rs` warnings are on older test lines and the `main.rs` pair is
the pre-existing `resolve_db_url` doc). `check-all` OK with the roadmap at 56 tickets.

Manual run on Windows with the built binary and two temp roots:

```
$ papa-bear-master-service check-names --mods-dir tmp/claim-recheck/stale
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 55s ago)
Error: check-names: 1 unused claim in 1 catalog entry                       (exit 1)

$ papa-bear-master-service check-names --fix --mods-dir tmp/claim-recheck/stale
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 55s ago; removed)
check-names: removed 1 unused install-folder claim
check-names: checked 1 catalog entry, … every artifact intact               (exit 0, no backdating)

$ papa-bear-master-service check-names --fix --mods-dir tmp/claim-recheck/pending
pending move: 'alpha-new' is reserved for 'alpha', which still installs into 'alpha-dir' (claimed 55s ago; kept, a publish may still be finishing)
Error: check-names: 1 unused claim in 1 catalog entry                       (exit 1, object still there)
```

The retaken row itself cannot be produced from one process — it needs a second publish inside the
scan-to-delete gap — so that wording is covered by the store tests and the delta above, not by a
manual run. Temp state removed.

## Risks

- **Still a window between the re-read and the delete.** Without a conditional delete the object can
  be replaced after the check; the interval is now the delete itself, not the scan. Closed by
  `MSERVER-CLAIM-CAS-1`: `delete_claim_if` holds the claim's write window across the re-read and the
  delete, and every claim writer takes that window before it writes.
- **A rewrite the storage clock cannot see.** S3 and R2 report `Last-Modified` in whole seconds, so
  the same body rewritten inside the scan's second is not detected by the time comparison. The body
  comparison still catches a different claim.
- **Clock skew or a restored object.** An object recreated from a backup with an older write time and
  the same body would read as unchanged. The local backend compares file modification times, which a
  copy can preserve.
- **A report-only run still shows the scan's decision.** `check-names` without `--fix` writes
  nothing, so it can describe a claim that a publisher takes while it prints; the row is a snapshot,
  as before.
- **A retaken claim still fails the run.** That is deliberate — the folder is still claimed and the
  process did not establish that it is unused — and the next scan decides about the object that now
  stands.
- **A malformed body on the re-read is an error.** A claim that cannot be parsed fails the sweep
  rather than being skipped, the same as in the scan.

## Follow-ups

- Reserve before the upload (a claim endpoint with a token plus commit/abort), so the losing
  publisher stops before streaming and the claim lifetime covers only the transfer.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>
# a failed release: an entry whose folderName is alpha-new at <mods root>/alpha/mod.json, plus
#   {"modId":"alpha","folder":"alpha-dir"} in <mods root>/_claims/<hex of alpha-dir>.json
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # removed at once
# the retaken path needs a publisher inside the gap: start `papa publish` into the stale folder
# while `check-names --fix` runs over a scan that saw the leftover, and the row must read
#   ; kept, the claim changed after the scan
```

Expect the first command to delete the claim without any backdating and exit 0, and a run that
catches a publish in the gap to keep the object and exit 1.
