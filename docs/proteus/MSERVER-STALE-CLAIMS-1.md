# MSERVER-STALE-CLAIMS-1 — a claim on a folder the entry moved off is named by the sweep and removable by `--fix`

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-ORPHAN-CLAIMS-1`. That ticket taught `check-names` to find
claims naming no catalog entry. It recorded this shape as still invisible: a claim naming an entry
that exists but has moved to another folder, which is what a failed release after a move leaves.

## Problem

`finalize_revision` moves an entry in three steps: it takes the new folder's claim, makes the new
revision current, and only then releases the claim on the folder it left. The release
(`release_claim_for`) deletes the object only when it still names this entry, and logs a warning
when the delete fails. Nothing retries it, and no later step notices: a crashed process, a failed
delete or a lost response after the current pointer moved leaves

```
_claims/616c7068612d646972.json = {"modId":"alpha","folder":"alpha-dir"}
alpha/revisions/2/mod.json      = { "folderName": "alpha-new", … }      ← current
```

The old folder is now refused to every publisher — `reserve_install_folder` answers `already
claimed by catalog entry 'alpha'` — while `alpha` installs into `alpha-new` and every catalog view
says nothing about `alpha-dir`. The orphan rule cannot see it because the entry the claim names is
right there, and the collision rule cannot either because only one entry resolves to the folder. The
operator's only recourse was to know the claim layout and delete the object by hand.

## Approach

- **One claim list, two reasons.** `OrphanedClaim` became `UnusedClaim { path, claim, reason,
  claimed_at }` with `UnusedClaimReason::{NoEntry, MovedTo { folder }}`. Both shapes are one
  `_claims/` listing, one age rule and one removal loop; only the row and the reason differ.
  `CatalogScan::orphaned_claims` is `unused_claims`, `remove_orphaned_claims` is
  `remove_unused_claims`, and `is_clean` covers the same single list.
- **The entry's current revision is the authority.** `scan_catalog` already resolves every
  directory's install folder into the collision map; it now records the same value into
  `current_folders: BTreeMap<String, Option<String>>`, keyed by catalog directory. A claim naming a
  directory outside that map is `NoEntry`; a claim naming one inside it whose folded folder differs
  is `MovedTo`. The comparison uses `install_folder_key`, the same Windows fold as the reservation,
  so `CSLA` and `csla` are one folder and a claim on either spelling is not stale.
- **Unreadable entries are left alone.** The map's value is `None` when `stored_entry` could not
  read the current metadata or when the names do not resolve (`InstallClaim::resolve`). The sweep
  cannot say which folder such an entry installs into, so it reports nothing rather than guess; the
  rejected names and the collision list already describe those entries.
- **The age floor covers both directions of a move.** A move takes the new claim *before* the
  revision is current, so during the move the new claim also disagrees with the entry's current
  folder. A publish takes its first claim before it writes the entry, as before. `--fix` therefore
  keeps any unused claim younger than five minutes (`UNUSED_CLAIM_MIN_AGE`, renamed from
  `ORPHAN_CLAIM_MIN_AGE`) and says `kept, a publish may still be finishing`. A run without `--fix`
  stays read-only; it can show a move that is in flight, which is the same audit-versus-live
  limitation the orphan half already has.
- **Rows name the other folder.** `stale claim: 'alpha-dir' is reserved by 'alpha', which now
  installs into 'alpha-new' (claimed 3h 0m ago)`, with `; removed` or `; kept, …` under `--fix`; the
  orphan row is unchanged. The clean sentence reads `no unused folder claims` and the failing
  summary counts `N unused claims`, because the summary is per list, not per reason.

## Behaviour

| situation | sweep |
| --- | --- |
| claim names no catalog directory | `orphan claim` row; exit 1 |
| claim names an entry that installs into that folder | not reported |
| claim names an entry whose current revision installs into another folder | `stale claim` row naming the entry's folder; exit 1 |
| claim names an entry whose current metadata is unreadable | not reported (nothing says which folder it uses) |
| `--fix`, claim ≥ 5 minutes old | row says `removed`, the object is deleted, the run can pass |
| `--fix`, claim younger than 5 minutes | row says `kept, a publish may still be finishing`; exit 1 |

## Files

- `mserver/MasterService/src/mods.rs` — `UnusedClaim`, `UnusedClaimReason`,
  `CatalogScan::unused_claims` (and `is_clean`), `ModStore::unused_claims` (now takes the per-entry
  folder map), `ModStore::remove_unused_claims`; one new store test, the orphan tests updated.
- `mserver/MasterService/src/main.rs` — `UNUSED_CLAIM_MIN_AGE`, `sweep_unused_claims`, the two rows
  and the summary wording, the `--fix` help text, the command help.
- `mserver/MasterService/tests/check_names.rs` — two binary tests for the moved-folder shape, the
  updated clean sentence and summary in the existing cases.
- `docs/proteus/MSERVER-STALE-CLAIMS-1.md`

## Evidence

Store test (`mods.rs`):

- `scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` — publishes `alpha` into
  `alpha-dir`, moves it to `alpha-new` (which releases `alpha-dir`), then writes the claim a failed
  release would leave. The scan reports exactly one unused claim, `alpha`/`alpha-dir`, with
  `MovedTo { folder: "alpha-new" }` and is not clean; the claim on `alpha-new` is still needed and
  not reported. Removing with zero deletes it, the scan is clean, `alpha` still answers `alpha-new`
  from `check_install_folder`, and `Beta` can publish into `alpha-dir`.

Binary tests (`tests/check_names.rs`, real `papa-bear-master-service` against temp directories):

- `a_claim_on_the_folder_an_entry_moved_off_is_reported` — the exact row and `check-names: 1 unused
  claim in 1 catalog entry`, exit 1.
- `fixing_removes_a_claim_on_the_folder_the_entry_left` — a claim backdated ten minutes is removed
  by `--fix`, the row says `; removed`, `check-names: removed 1 unused install-folder claim` is
  printed, the object is gone, and a second sweep exits 0.
- The four orphan tests pass unchanged apart from the renamed summary (`1 unused claim`) and clean
  sentence (`no unused folder claims`); `a_claim_held_by_a_catalog_entry_is_not_reported` now also
  pins that a matching folder is not stale.

Broken-state delta: with the `current_folders.insert(…)` line removed,
`scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` fails on `left: 0, right: 1` and
both binary tests fail by name; restoring it is green.

Counts: `papa-bear-master-service` **100 lib + 7 bin + 14 integration**, 0 failed (99 + 7 + 12
before; +1 store, +2 binary). `papa-bear-cli` 19 + 4 and `papa-bear-client` 15, unchanged and green.
`cargo clippy -p papa-bear-master-service --all-targets` has no warning in the new code (lib stays
at 32, bin at 8); no warning points into the new functions or the new test. `rustfmt --check` on the
three files is clean.

Manual run on Windows against the built binary, a temp mods root with one entry whose folder is
`alpha-new` and a claim left on `alpha-dir`:

```
$ papa-bear-master-service check-names --mods-dir tmp/stale-live/mods
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 0s ago)
Error: check-names: 1 unused claim in 1 catalog entry            (exit 1)

$ papa-bear-master-service check-names --fix --mods-dir tmp/stale-live/mods
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 0s ago; kept, a publish may still be finishing)
Error: check-names: 1 unused claim in 1 catalog entry            (exit 1, object still there)

# LastWriteTime moved back three hours, then:
$ papa-bear-master-service check-names --fix --mods-dir tmp/stale-live/mods
stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed 3h 0m ago; removed)
check-names: removed 1 unused install-folder claim
check-names: checked 1 catalog entry, no rejected names, no shared install folders, no unused folder claims, every artifact intact   (exit 0)

$ ls tmp/stale-live/mods/_claims
$ papa-bear-master-service check-names --mods-dir tmp/stale-live/mods
check-names: checked 1 catalog entry, no rejected names, no shared install folders, no unused folder claims, every artifact intact   (exit 0)
```

Temp state removed.

Note on flakiness: the package's lib tests hit the recorded Windows local-store rename race when run
in parallel (`add_revision` → `rename_if_not_exists`, `Access is denied`), in a different test on
each attempt — `scan_catalog_reports_a_claim_on_the_folder_an_entry_moved_off` once, then
`catalog_selects_latest_compatible_revision` and `stable_id_keeps_immutable_revision_history`, then
`publish_rejects_a_folder_name_that_could_leave_the_mods_root`. Every affected test passes alone,
and `-- --test-threads=1` is green at 100 + 7 + 14; the race predates this ticket and is recorded in
`MSERVER-FOLDERNAME-1`, `MSERVER-NORMALIZE-1` and `MSERVER-CHECK-NAME-1`.

## Risks

- **The five minutes are a heuristic, not knowledge.** It now also covers the new claim a move takes
  before the revision is current: a move stalled longer than the grace could lose the claim on the
  folder it is moving *to*, after which the entry holds that folder unclaimed — the pre-reservation
  state, not a wrong refusal. The alternative (a claim body naming the intended folder, or a commit
  record) is the real fix; the sweep is a maintenance-window tool either way.
- **A report-only run can show a move in flight.** It compares the claim list with the current
  revisions and has no transaction around the two, exactly as the orphan half can show a publish
  that has taken its folder and not yet written its entry. The sweep is meant to gate a maintenance
  window, not to run continuously against a busy catalog.
- **An entry whose current metadata is unreadable is not judged.** Its claim could be stale and is
  left in place; the name and artifact halves still report the entry itself when they can see it.
- **A pre-reservation entry still holds its folder without a claim.** Nothing to report or remove;
  the catalog lookup still protects it, and the next revision that keeps the folder creates the
  object.
- One `_claims/` listing per sweep, unchanged; `--fix` is one delete per removed claim, as before.

## Follow-ups

- Make the move atomic from the sweep's point of view: a claim body carrying the folder the entry is
  moving to (or a commit record written with `current`) would let `check-names` decide a mismatch
  without the age floor, and would close the stalled-move opening above.
- Reserve before the upload (a claim endpoint with a token plus commit/abort), so the losing
  publisher stops before streaming and the claim lifetime covers only the transfer.

## Superseded

The first follow-up landed in `MSERVER-CLAIM-MOVES-1.md`: a claim body now records the folder its
move leaves (`replaces`), so the sweep names a `MovePending` claim instead of guessing from its age
and removes a `MovedAway` one at any age. `UnusedClaimReason::MovedTo { folder }` is now
`MovedAway { folder }` plus `MovePending { from }`; the `stale claim` row, the summary and the
orphan rows are unchanged, while a claim a move has just taken is reported as `pending move`
instead of waiting out the floor as a stale one. The risk that a move stalled longer than the grace
could lose its destination claim is closed for everything the record can describe, as recorded in
the new ticket.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
# create the leftover by hand: an entry whose folderName is alpha-new at <mods root>/alpha/mod.json,
# and {"modId":"alpha","folder":"alpha-dir"} in <mods root>/_claims/<hex of alpha-dir>.json
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>          # row, exit 1
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # kept, exit 1
# backdate the file five minutes, then:
cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <mods root>    # removed, exit 0
```

Expect the row to name the folder, the entry and the folder the entry moved to; a fresh claim to
survive `--fix` with `kept, a publish may still be finishing`; and the second `--fix` to delete the
object and exit 0, after which another entry can publish into the old folder.
