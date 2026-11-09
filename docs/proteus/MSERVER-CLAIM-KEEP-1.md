# MSERVER-CLAIM-KEEP-1 — a revision that keeps its folder reserves it too, so two publishes of one entry stop before streaming

Status: done-unverified (master and CLI; nothing to verify on a GPU)

Closes the second follow-up of `MSERVER-CLAIM-RESERVE-1` and the follow-up of
`MSERVER-CLAIM-LEASE-1`. Reservations covered a new entry and a move, but a revision that kept
its folder reserved nothing: the folder was already the entry's, so no other entry could take it,
and a token there could never be swept while the catalog explained the folder. That left the two
publishes of one entry — the optimistic-lock case the claim exists to decide — with nothing between
them and the artifact placement.

## Problem

```

t=0     publish A of alpha, folder alpha-dir, revision 2
t=0     publish B of alpha, folder alpha-dir, revision 2   (another machine, same manifest)
        GET /v1/catalog/folder-claim -> available:true for both
        POST /v1/catalog/folder-claim -> no token, nothing written, 200 for both
        both pack and stream
t=9     A writes revision 2 and its entry
t=10    B is refused at artifact placement, after the whole upload
```

The reservation route answered `{"modId":"alpha","folder":"alpha-dir"}` with no token, and the
upload took the claim with `ClaimTake::Plain`. The only rule that separated the two was the
placement loop in `finalize_revision`, which runs after the stream.

The obvious reason for the old behaviour was sweepability: a token on a claim whose entry installs
into the folder is never reported as unused, so a publisher that died would have refused its own
entry forever. The lease added by `MSERVER-CLAIM-LEASE-1` is what makes the reservation safe to
take now — a lapsed lease is a decision, not a guess.

## Approach

- **Every reservation is a token, including a kept folder.** `reserve_folder_claim` mints one
  unconditionally and answers `renewAfterMs` beside it. The claim object already exists (the
  publish that installed the entry wrote it), so the reservation rewrites it with the new token and
  lease through the same `InstallFolderReservation`. `FolderClaimReservation::token` and
  `renew_after_ms` remain `Option` on the wire: a master older than this ticket answers no token
  for a kept folder, and the CLI then uploads exactly as before.
- **A live token refuses both reservations and plain uploads; a lapsed one is nobody's.** The
  `Reserve` arm of `reserve_install_folder` now bails only while `StoredClaim::lease_held` is true
  and takes the claim over once it is not. The `Plain` arm refuses while the lease runs (the old
  CLI's upload, or a `claimToken`-less upload of one entry) and clears a lapsed token, which is
  what a leftover token written back by a renewal that raced a consume becomes after its lease.
  Two publishes of one entry therefore meet `already reserved by another publish of 'alpha'` at
  the reservation, before either packs.
- **Abort no longer asks the catalog.** A kept-folder reservation is taken while the entry already
  installs into the folder, so the old `already installs into folder` check would have made a
  failed revision's abort impossible. The token is the test: the upload consumes it before the
  revision is written, so a token-less claim cannot be aborted by a caller holding the old token.
  The one way a token reappears on a finished publish is the renewal/consume race; deleting that
  claim only means the next publish writes the claim the catalog already explains.
- **The preflight answers for the entry's own folder.** `check_install_folder` used to skip the
  reservation lookup when the revision kept its folder. It now reads that claim and reports a live
  lease as unavailable, with `claimedBy` naming the caller's own entry, so a client that still uses
  the read-only route is stopped before packing as well.
- **The sweep can age a kept-folder reservation out.** `scan_catalog`'s claim pass adds
  `UnusedClaimReason::LapsedReservation` for a claim whose entry installs into its folder and whose
  token's lease has run out. The catalog supplies the folder without the claim, so
  `may_still_be_finishing` is false and `--fix` removes it at any age. The row reads
  `lapsed reservation: '<folder>' is reserved for '<entry>', whose reservation stopped renewing`.
  A live keeps-folder reservation is not listed at all: it is the folder's record while its
  publisher works, and nothing about it is unused.

## Behaviour

| publish | claim | result |
| --- | --- | --- |
| first publish of an entry | free | token, lease, `renewAfterMs` (unchanged) |
| revision that keeps its folder | no token | token, lease, `renewAfterMs` |
| second publish of one entry while the first reserves | live token | 409 at the reservation, before packing |
| preflight of the same entry | live token | `available:false`, `claimedBy` = that entry |
| upload with the reservation's token | live token | consumes it, writes the revision |
| upload without it (older CLI) | live token | 409 after the body, before placement |
| upload without it | lapsed token | proceeds, clears the token |
| reservation of the same folder | lapsed token | takes it over with a new token and lease |
| abort with the reservation's token | any | 204, claim deleted |
| abort with the old token after a publish | no token | 409 `belongs to another publish` |
| `--fix` with a live keeps-folder reservation | live token on an installed folder | not listed; clean |
| `--fix` with a lapsed one | lapsed token on an installed folder | removed at once |

## Files

- `mserver/MasterService/src/mods.rs` — `StoredClaim::lease_held`, the lease-aware `Reserve` and
  `Plain` arms, `folder_reserved_refusal`, the token for every reservation, the preflight reading
  its own claim, `abort_folder_claim` dropping the catalog test, `UnusedClaimReason::LapsedReservation`
  in `unused_claims`, and the new store tests.
- `mserver/MasterService/src/main.rs` — the lapsed-reservation row and the sweep's doc.
- `mserver/MasterService/src/model.rs` — the reservation and preflight docs.
- `mserver/MasterService/src/http.rs` — the abort route's description.
- `mserver/MasterService/src/lib.rs` — the route test's keeps-folder half; `claim` moved beside it.
- `mserver/MasterService/tests/check_names.rs` — a lapsed reservation on an installed folder.
- `mserver/CLI/src/main.rs` — the two comments a token-less answer now means an older master.
- `mserver/CLI/tests/cli_roundtrip.rs` — a kept-folder reservation renews; a token-less answer does
  not.
- `docs/proteus/MSERVER-CLAIM-KEEP-1.md`

## Evidence

Store tests (`mods.rs`):

- `a_second_publish_of_one_entry_is_refused_before_it_streams` — publishing `alpha` into
  `alpha-dir`, reserving it again answers a 64-character token and `renewAfterMs`; the claim holds
  the token's hash and a lease; a second reservation of `alpha` fails `already reserved by another
  publish of 'alpha'`; the preflight answers `available:false`, `claimedBy:"alpha"` and the same
  refusal; the upload carrying the token lands revision 2 and leaves the claim token- and
  lease-free.
- `aborting_a_reservation_that_keeps_the_folder_releases_it` — the abort deletes the claim even
  though the entry installs there, the entry is unchanged, and the folder can be reserved again.
- `a_lapsed_reservation_is_taken_over_by_the_next_publish` — after ageing the lease, the same
  entry reserves the folder with a different token and a new deadline, and the old token's renewal
  is answered `belongs to another publish`.
- `a_reservation_on_a_folder_the_entry_keeps_is_reported_once_its_lease_runs_out` — a live
  reservation on the installed folder is absent from `scan_catalog`; with the lease in the past it
  is one `LapsedReservation` that is not `may_still_be_finishing`, `--fix` removes it at zero age,
  and the folder is free afterwards.
- `a_token_left_on_an_installed_folder_is_cleared_once_its_lease_runs_out` — a token written back
  by a racing renewal refuses a token-less revision while its lease runs and is cleared by that
  revision once the lease has run out.

Route test (`lib.rs`):

- `http_folder_reservation_stops_a_second_publisher_before_an_upload` — after the first publish,
  a keeps-folder reservation answers a token and `renewAfterMs`, a second one answers 409, the
  read-only route answers `available:false` with `claimedBy:"alpha"`, and abort answers 204. The
  earlier abort-after-publish expectation now reads `belongs to another publish`.

`check-names` (`tests/check_names.rs`):

- `fixing_removes_a_lapsed_reservation_on_an_installed_folder` — the report prints the lapsed row
  and exits 1 without deleting; `--fix` removes the object, prints `removed 1 unused install-folder
  claim` and exits 0; the following sweep is clean.

CLI (`tests/cli_roundtrip.rs`):

- `publish_reserves_a_revision_that_keeps_its_folder` — with the catalog lookup answering 200, the
  request order is `GET /v1/mods/beta`, `POST /v1/catalog/folder-claim`,
  `POST …/folder-claim/renew`, then `POST …/folder-claim/abort` when the source cannot be packed.
- `publish_uploads_when_an_older_master_reserves_nothing_for_a_kept_folder` — a token-less answer
  leaves two requests and no renew or abort, the compatibility path.

Broken-state deltas, measured one at a time and restored:

- making the `Reserve` arm refuse every token fails
  `a_lapsed_reservation_is_taken_over_by_the_next_publish` on the second `unwrap`;
- dropping the `LapsedReservation` arm from `unused_claims` fails
  `fixing_removes_a_lapsed_reservation_on_an_installed_folder` at its first row assertion.

Counts: `papa-bear-master-service` **120 lib + 7 bin + 17 integration**, `papa-bear-cli` **20 + 7**,
`papa-bear-client` **15**, 0 failed (117 + 7 + 16, 20 + 5 and 15 before). `cargo fmt` is clean on
the three crates; `cargo clippy --all-targets` exits 0 with no diagnostic in the new items;
`check-all` is OK and the roadmap regenerates to 59 tickets.

Manual run on Windows with the built binaries, a temp SQLite database and mods root
(`127.0.0.1:18120`, admin key `keep-key`):

```
$ papa publish src-alpha --mod-id alpha --name Alpha --folder-name alpha-dir
published 'Alpha' as alpha, version 9fee5cd2, package revision 1 (52 bytes compressed)

$ POST /v1/catalog/folder-claim {"modId":"alpha"}
200 {"modId":"alpha","folder":"alpha-dir","token":"4d71…e1d7","renewAfterMs":200000}
_claims/616c7068612d646972.json =
    {"modId":"alpha","folder":"alpha-dir","token":"c4ca…70a3","lease_until":1792997096059}

$ POST /v1/catalog/folder-claim {"modId":"alpha"}          (a second publish of one entry)
409 install folder 'alpha-dir' is already reserved by another publish of 'alpha'; try again once it
    finishes or releases the folder
$ GET /v1/catalog/folder-claim?modId=alpha
200 {"available":false,"claimedBy":"alpha","refusal":"install folder 'alpha-dir' is already
     reserved by another publish of 'alpha'; …"}

$ check-names --fix --mods-dir mods        (lease still running)
check-names: checked 1 catalog entry, no rejected names, no shared install folders,
no unused folder claims, every artifact intact                              (exit 0)
$ (lease_until := 1) check-names --mods-dir mods
lapsed reservation: 'alpha-dir' is reserved for 'alpha', whose reservation stopped renewing
    (claimed 8s ago)                                                        (exit 1)
$ check-names --fix --mods-dir mods
lapsed reservation: … (claimed 9s ago; removed)
check-names: removed 1 unused install-folder claim …                        (exit 0)

$ POST reserve -> 200 token A; (lease_until := 1); POST reserve -> 200 token B
$ POST …/folder-claim/renew with token A
409 the reservation for folder 'alpha-dir' belongs to another publish
$ POST …/folder-claim/abort with token B
204; _claims empty; GET /v1/mods still [('alpha', 'alpha-dir')]
```

Temp state removed and the server stopped.

## Risks

- **A leftover token refuses its own entry for the rest of its lease.** A renewal that read the
  claim before the upload consumed the token writes it back with a fresh deadline; a keeps-folder
  revision without that token is refused until the lease runs out (at most ten minutes), where the
  previous ticket cleared it at once. The lease is the only thing that can separate it from a live
  reservation, and clearing it immediately is the race this ticket closes. Closed by
  `MSERVER-CLAIM-CAS-1`: the renewal's write is conditional on the claim's generation, so it is
  refused instead of writing the consumed token back.
- **A live keeps-folder reservation is invisible to `check-names`.** It is the folder's record, so
  a report-only sweep prints the clean sentence instead of a held row; an operator cannot see a
  publisher that is packing slowly. Only a lapsed one is a row.
- **The refusal is a refusal, not a queue.** The second publish must be run again after the first
  finishes; a script that retries immediately sees 409 until then.
- **A mixed fleet changes behaviour.** An older CLI uploads without a token, so it is now refused
  while a newer publisher holds the folder it also keeps. That is the intended serialization, but
  it is a visible failure where two old publishes would have raced to placement.
- **Abort no longer names the catalog.** An abort with a token that a racing renewal wrote back
  deletes the claim of an installed entry. The folder stays the entry's (the catalog resolves it)
  and the next reservation writes the claim again, but the record is briefly absent. Closed by
  `MSERVER-CLAIM-CAS-1`: a renewal can no longer write a consumed token back, and abort's delete is
  itself re-checked against the body under the claim's write window.
- **`claimedBy` can name the caller.** A preflight for a second publish of one entry answers
  `claimedBy:"alpha"` to `alpha`; a client that assumes a different entry may word it oddly.
- **The deadline is still the master's clock.** Takeover, like renewal, trusts it; a clock that
  jumps backwards delays a reclaim rather than freeing a live folder.

## Follow-ups

- (done, `MSERVER-CLAIM-CAS-1`) A conditional claim write so a renewal cannot race the upload's
  consume, which also removes the ten-minute leftover-token window. The claim carries a generation
  and its rewrite is a compare-and-set inside a per-claim write window, because the local object
  store has no `PutMode::Update`.
- Show a live keeps-folder reservation in `check-names` without counting it as a problem, so an
  operator can see a long pack instead of the clean sentence.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
# the kept folder by hand: start the service and publish alpha into alpha-dir, then
#   POST /v1/catalog/folder-claim {"modId":"alpha"}   -> 200 + token + renewAfterMs
#   POST /v1/catalog/folder-claim {"modId":"alpha"}   -> 409 already reserved by another publish
#   GET  /v1/catalog/folder-claim?modId=alpha         -> available:false, claimedBy:alpha
#   check-names --fix --mods-dir <root>               -> clean while the lease runs
#   set lease_until in _claims/<hex>.json to 1
#   check-names --mods-dir <root>                     -> lapsed reservation, exit 1
#   check-names --fix --mods-dir <root>               -> removed, exit 0
papa publish <folder> --admin-key <key> --mod-id alpha --name Alpha --folder-name alpha-dir
```

Expect a second reservation of the same entry to answer 409 before it packs, the live claim to be
absent from the sweep, a lapsed one to be removed at once, and the entry to keep installing into
`alpha-dir` throughout.
