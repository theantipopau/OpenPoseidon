# MSERVER-CLAIM-MOVES-1 — a move records the folder it leaves, so the sweep decides it without the age floor

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-STALE-CLAIMS-1`. That ticket taught `check-names` to name a
claim left on a folder an entry had moved off, but could only tell such a leftover from a move in
flight by its age. This ticket records the folder a move leaves in the claim it takes, so the sweep
decides from the catalog instead of the clock.

## Problem

A claim body was `{ "modId", "folder" }`, which cannot say why it exists. Two states look identical:

```
# a move that has taken alpha-new and has not written its revision yet
_claims/<hex alpha-new>.json = {"modId":"alpha","folder":"alpha-new"}
alpha/revisions/1/mod.json   = { "folderName": "alpha-dir", … }            ← current

# a release that failed after the move landed
_claims/<hex alpha-dir>.json = {"modId":"alpha","folder":"alpha-dir"}
alpha/revisions/2/mod.json   = { "folderName": "alpha-new", … }            ← current
```

Both are "a claim on a folder the entry does not install into", and only elapsed time separated
them: `--fix` waited five minutes before touching either. That cost the sweep its decision — a
report-only run had to guess, and a cleanup could not say why it kept a claim — and it kept the
stalled-move opening from the previous ticket: a move that takes longer than the grace can lose the
claim on the folder it is moving to.

## Approach

- **The claim records the folder it replaces.** `StoredClaim` (the claim body) gains
  `replaces: Option<String>`, set by `reserve_install_folder` from the folder the entry's current
  revision installs into whenever the publish changes folders. It is written in the same conditional
  put that takes the folder, so a claim this code takes never exists without its record.
- **A rejoined claim is rewritten.** Moving back onto a folder this entry still claims finds its own
  object, and `PutMode::Create` will not replace an existing one — so the reservation writes the
  body again with a plain put when it is a move. That write is what keeps the record current; a
  failure fails the publish, because a stale record would let a sweep read a running move as a
  leftover and remove the folder it is publishing into.
- **The sweep compares the record with the commit.** For a claim whose folder is not the entry's
  current folder, `replaces` folded with `install_folder_key` against that folder splits the list:
  equal is `MovePending { from }` — a move between taking its destination and making the new
  revision current — and anything else (or absent, which is every claim written before this ticket)
  is `MovedAway { folder }`, a state no move in flight can explain.
- **Only a possible running publish waits.** `UnusedClaimReason::may_still_be_finishing` is true for
  `NoEntry` (a first publish takes its folder before writing the entry) and `MovePending`; those two
  keep the `UNUSED_CLAIM_MIN_AGE` floor. `MovedAway` is removed at any age: the only step a move has
  left on a folder the catalog has moved past is the delete.
- **Rows say which it is.** `pending move: 'alpha-new' is reserved for 'alpha', which still installs
  into 'alpha-dir' (claimed 0s ago; kept, a publish may still be finishing)` beside the unchanged
  `stale claim: … now installs into …` and `orphan claim: …` rows. The clean sentence, the failing
  summary and `remove_unused_claims`' contract are otherwise unchanged.
- **Old bodies stay readable.** `replaces` is `#[serde(default, skip_serializing_if = "Option::is_none")]`,
  so a claim written before this ticket parses with `None` and reads as `MovedAway` — which is what
  it is once the catalog has moved past it.

## Behaviour

| claim on | record | sweep row | `--fix` |
| --- | --- | --- | --- |
| the entry's current folder | any | not reported | not touched |
| a folder the entry does not install into, `replaces` = current folder | move in flight | `pending move`, exit 1 | kept while younger than 5 minutes, then removed |
| a folder the entry does not install into, any other `replaces` (or none) | release leftover / old move | `stale claim`, exit 1 | removed at any age |
| no catalog entry (and `replaces` = current folder of nothing) | first publish in flight | `orphan claim`, exit 1 | kept while younger than 5 minutes, then removed |

## Files

- `mserver/MasterService/src/mods.rs` — `StoredClaim::replaces`; `read_claim` returns the stored body;
  `reserve_install_folder` records and refreshes it; `UnusedClaimReason::{NoEntry, MovePending,
  MovedAway}` with `may_still_be_finishing`; `ModStore::unused_claims` classifies from the record;
  `remove_unused_claims` applies the age floor only to a claim a publish can still explain; two new
  store tests, one extended.
- `mserver/MasterService/src/main.rs` — the `pending move` row, the `--fix`/command docs, the
  sweep doc that says which claims the floor still protects.
- `mserver/MasterService/tests/check_names.rs` — a `write_move_claim` helper, the fresh-claim removal
  (no backdating) and a new pending-move case.
- `docs/proteus/MSERVER-CLAIM-MOVES-1.md`

## Evidence

Store tests (`mods.rs`):

- `scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` — a real `add_revision` move now
  leaves `replaces: "alpha-dir"` in the claim on `alpha-new`, read back through `read_claim` and
  asserted; the recreated failed-release claim has no record, is reported `MovedAway { folder:
  "alpha-new" }`, and is removed with a **one-hour** minimum — the point of the ticket, since the
  floor no longer applies. The scan is clean afterwards, `alpha` still answers `alpha-new`, and
  `Beta` publishes into `alpha-dir`.
- `a_move_that_has_not_written_its_revision_is_left_for_its_age` — `reserve_install_folder` for
  `alpha-new` with `alpha-dir` as the previous folder (the move's first step). The scan reports
  exactly that claim as `MovePending { from: "alpha-dir" }`, with `may_still_be_finishing() == true`;
  a one-hour minimum keeps it, zero removes it, the scan is clean, and `alpha` still installs into
  and holds `alpha-dir`.
- `a_move_refreshes_the_folder_its_claim_says_it_replaces` — a first move to `alpha-new` creates the
  claim (`created == true`); after releasing it, the move back onto `alpha-dir` finds this entry's
  own claim (`created == false`) and rewrites its body, so `read_claim` on `alpha-dir` says
  `replaces: "alpha-new"` rather than the folder an older move left.

Binary tests (`tests/check_names.rs`, real `papa-bear-master-service` against temp directories):

- `fixing_removes_a_claim_on_the_folder_the_entry_left` — the claim is **not** backdated any more:
  `--fix` removes a fresh one, the row says `; removed`, `check-names: removed 1 unused install-folder
  claim` is printed, the object is gone, and a second sweep exits 0.
- `fixing_keeps_a_fresh_move_that_has_not_landed` — a claim on `alpha-new` whose body says
  `replaces: alpha-dir` while the entry installs into `alpha-dir`: the exact `pending move` row,
  `; kept, a publish may still be finishing`, exit 1, and the object still there.
- The four orphan cases and `a_claim_on_the_folder_an_entry_moved_off_is_reported` keep their
  assertions; the summary (`1 unused claim`) and clean sentence are unchanged.

Broken-state delta, measured together and then restored:

- with `replaces` never recorded (`.filter(|_| !keeps_folder)` forced to `false`),
  `scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` fails on `left: None, right:
  Some("alpha-dir")` and `a_move_that_has_not_written_its_revision_is_left_for_its_age` fails on
  `left: MovedAway { folder: "alpha-dir" }, right: MovePending { from: "alpha-dir" }`;
- with the age gate made unconditional, `fixing_removes_a_claim_on_the_folder_the_entry_left` fails
  on the surviving fresh claim.

Counts: `papa-bear-master-service` **102 lib + 7 bin + 15 integration**, 0 failed (100 + 7 + 14
before; +2 store, +1 binary). `papa-bear-cli` 19 + 4 and `papa-bear-client` 15, unchanged and green.
`rustfmt --check` on the three files is clean. `cargo clippy -p papa-bear-master-service
--all-targets` reports the crate's pre-existing warnings on their older lines; no diagnostic points
into a new item, and `-D warnings` fails on the pre-existing `too_long_first_doc_paragraph` lints in
`papa-bear-client`, unchanged by this ticket. `check-all` OK (72 assets, roadmap at 55 tickets, both
selftests, 89 changed files formatted).

Manual run on Windows with the built binary and a temp root, one entry per shape:

```
$ papa-bear-master-service check-names --mods-dir tmp/claim-moves/away      # current folder alpha-new
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 0s ago)
Error: check-names: 1 unused claim in 1 catalog entry                       (exit 1)

$ papa-bear-master-service check-names --fix --mods-dir tmp/claim-moves/away
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 0s ago; removed)
check-names: removed 1 unused install-folder claim
check-names: checked 1 catalog entry, … every artifact intact               (exit 0 — no backdating)

$ papa-bear-master-service check-names --fix --mods-dir tmp/claim-moves/pending   # claim {} replaces alpha-dir
pending move: 'alpha-new' is reserved for 'alpha', which still installs into 'alpha-dir' (claimed 0s ago; kept, a publish may still be finishing)
Error: check-names: 1 unused claim in 1 catalog entry                       (exit 1, object still there)
```

Temp state removed.

## Risks

- **One shape is still age-floored, and no record can separate it.** A claim left on a folder the
  entry has since moved *back* to is byte-identical to the claim a new move onto that folder writes
  (`replaces` names the folder the entry installs into again), so that leftover waits out the five
  minutes. Every other leftover is decided by the catalog.
- **A publish started before this upgrade writes no record.** If a sweep catches it mid-move, it
  reads as `MovedAway` and may delete its destination claim. The window is the deployment itself; a
  claim taken after the upgrade always carries `replaces`.
- **Two concurrent moves of one entry can still lose the loser's claim.** Both take a destination
  with `replaces` = the same current folder; the one that commits first makes the other's claim read
  `MovedAway`, so a sweep can remove it while that publisher is still working. Revisions of one
  entry are not serialized in any case (both write `current`), and the claim rule does not make
  that worse — but it does not close it either.
- **A failed move leaves a claim that looks pending.** If a move rewrites its own existing claim and
  then fails, the claim names the current folder until it ages out; the destination stays refused
  for five minutes instead of forever. That is the same bound the previous ticket had.
- **The record is read once, the delete comes later.** A move that rewrites the claim between the
  scan and `--fix` is still removed on the older decision. The window existed before (a claim could
  be released and retaken); a content check before the delete is the follow-up below. Closed by
  `MSERVER-CLAIM-RECHECK-1` (the claim is read again immediately before the delete) and
  `MSERVER-CLAIM-CAS-1` (that read and the delete run inside the claim's write window).

## Follow-ups

- (done, `MSERVER-CLAIM-RECHECK-1` and `MSERVER-CLAIM-CAS-1`) Re-read a claim immediately before
  `--fix` deletes it and skip it if its body changed, then take the claim's write window around the
  re-read and the delete so a claim taken after the scan survives without a store-level conditional
  delete.
- Reserve before the upload (a claim endpoint with a token plus commit/abort), so the losing
  publisher stops before streaming and the claim lifetime covers only the transfer.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
# a failed release: an entry whose folderName is alpha-new at <mods root>/alpha/mod.json, plus
#   {"modId":"alpha","folder":"alpha-dir"} in <mods root>/_claims/<hex of alpha-dir>.json
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # removed at once
# a move in flight: an entry whose folderName is alpha-dir, plus
#   {"modId":"alpha","folder":"alpha-new","replaces":"alpha-dir"} in <mods root>/_claims/<hex of alpha-new>.json
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # kept, exit 1
```

Expect the first `--fix` to delete the claim without any backdating and exit 0, and the second to
print `pending move: …` and keep the object; backdating that claim five minutes makes the same
command remove it.
