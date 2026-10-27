# MSERVER-CLAIM-LEASE-1 — a reservation carries a renewed lease, so `--fix` cannot drop a live publisher's folder

Status: done-unverified (master and CLI; nothing to verify on a GPU)

Closes the first follow-up of `MSERVER-CLAIM-RESERVE-1`. That ticket made a publish take its
install folder before it packs; this one gives that claim a deadline its publisher keeps moving, so
the sweep can tell a long publish from a crashed one.

## Problem

A reservation is taken before the source folder is packed and held until the upload lands. The only
thing keeping `check-names --fix` away from it was the claim object's write time and the five-minute
floor:

```
t=0     papa publish reserves alpha-dir   _claims/<hex>.json  written now, token inside
t=1..   pack (minutes) and upload (minutes to hours)
t=6     check-names --fix scans: no catalog entry for gamma, claim older than five minutes
        -> deletes the claim
t=6     a second publish reserves alpha-dir, because it is free
t=40    the first publish streams its whole artifact and is refused at finalize
```

That is the race the reservation exists to decide, re-opened for exactly the publishes it was built
for: the large ones. Worse than before the reservation, because the first publisher now also loses
the folder it believed it held.

The claim carried no statement from the publisher. "Alive" was inferred from the age of an object
that the publisher had no way to refresh.

## Approach

- **The publisher states its deadline.** `StoredClaim` gains `lease_until` (unix milliseconds,
  `serde(default, skip_serializing_if)`, so every older body still parses). `ClaimTake::Reserve`
  writes `now + RESERVATION_LEASE_MS` (10 minutes); `ClaimTake::Plain` and the upload's
  `ClaimTake::Consume` clear it together with the token, so a claim the catalog explains is decided
  by the catalog, never by a deadline.
- **Renewal is a route, not a guess.** `POST /v1/catalog/folder-claim/renew` takes the same
  `{modId, folder, token}` body as commit and abort and answers 204, 404 (no reservation for that
  folder) or 409 (another publish's token, or another entry's folder). Only the token that took the
  claim may move it. `ModStore::renew_folder_claim` rewrites the body with a new `lease_until` and
  leaves the token untouched. A lease with more than two thirds of its life left — the renewal the
  CLI sends right after the reservation — answers 204 without a write: rewriting the object it has
  just created is the rename a Windows scanner can refuse, and the deadline would move by
  milliseconds anyway.
- **A finished publish has nothing to renew.** A token-less claim answers 204 rather than an error,
  so a renewal that races the upload's consume is harmless instead of turning a successful publish
  into a failure.
- **The master owns the cadence.** `FolderClaimReservation` answers `renewAfterMs` (200 s, a third
  of the lease) beside the token, so the publisher cannot disagree with the master about how long a
  lease lasts and two lost renewals are still inside it.
- **The sweep reads the lease first.** `UnusedClaim` carries `lease_until` and `lease_held(now)`;
  `remove_unused_claims` skips a claim whose lease is still running **at any age**, and reclaims one
  whose lease ran out without waiting for a write time to age — the lease is the statement, the
  clock is only the fallback for claims that never had one (pre-lease reservations and plain
  publishes keep the five-minute floor). The row reads
  `; kept, its reservation is still being renewed`.
- **The publisher renews while it works.** `papa publish` starts a `FolderClaimRenewal` after the
  reservation: the first renewal is sent on the calling thread **before packing** (so a reservation
  the master no longer holds stops the publish before the source is touched, and the deterministic
  order is testable with a source that cannot be packed), and a worker thread repeats it every
  `renewAfterMs` until the publish ends. The guard is dropped — stopping and joining the worker —
  before the commit or abort, so no renewal can land after the folder is given back. The worker uses
  its own agent with a 30-second overall timeout (`renewal_agent`), because a request that never
  answers would otherwise hold that join, and therefore the commit or abort, open indefinitely.
- **A leftover token cannot wedge an installed folder.** A renewal that read the claim just before
  the upload consumed the token writes the token back. Such a token cannot be a live reservation: a
  reservation is only taken while the entry does not install into the folder. The next revision
  that keeps the folder now clears it (`Some(_) if keeps_folder => (plain, false)`) instead of being
  refused `reserved by a publish … that has not finished` forever — the sweep deliberately ignores
  claims the catalog explains, so nothing else would have removed it.
- **`"belongs to another publish"` is a conflict.** `claim_error` maps it to 409, which is what the
  renewal's documented responses promise; commit and abort answer the same status for the same
  condition now, instead of 400.

## Behaviour

| claim | lease | `--fix` | why |
| --- | --- | --- | --- |
| a reservation being renewed | in the future | kept, `; kept, its reservation is still being renewed`, run fails | the publisher is still packing or streaming |
| a reservation whose lease ran out | in the past | removed at once | the publisher stopped renewing (crash) |
| a claim with no lease (pre-lease reservation, plain publish) | absent | age floor as before | nothing states the publisher is alive |
| a claim the entry installs into | any | untouched | not unused |
| renewal while the lease is still fresh | more than two thirds left | 204, no write | the reservation (or the last renewal) just wrote it |
| renewal with another publish's token | — | 409, lease unchanged | only the holder may renew |
| renewal after the upload consumed the token | — | 204 | the reservation already did its work |

## Files

- `mserver/MasterService/src/mods.rs` — `RESERVATION_LEASE_MS`/`RESERVATION_RENEW_AFTER_MS`/
  `RESERVATION_RENEW_MARGIN_MS`, `StoredClaim::lease_until`, the reservation body and the cleared `plain` body, the leftover-token
  arm, `renew_folder_claim`, `UnusedClaim::lease_until`/`lease_held`, the sweep skip, and three
  store tests.
- `mserver/MasterService/src/model.rs` — `FolderClaimReservation::renew_after_ms` (`renewAfterMs`).
- `mserver/MasterService/src/service.rs` — the `renew_folder_claim` wrapper.
- `mserver/MasterService/src/http.rs` — the `/renew` route, its handler and OpenAPI entry, and
  `claim_error`'s conflict message.
- `mserver/MasterService/src/main.rs` — the lease in `--fix`'s help and the sweep's fate wording.
- `mserver/MasterService/src/lib.rs` — `post_json` moved beside the tests that use it, and one
  renewal route test.
- `mserver/MasterService/tests/check_names.rs` — a live lease keeps a claim `--fix` would otherwise
  remove.
- `mserver/CLI/src/main.rs` — `FolderClaimRenewal`, `sleep_until_stopped`, `renewal_interval`,
  `renewAfterMs` parsing, `end_folder_claim`'s `tolerate_missing`, and the start/stop in
  `run_publish`.
- `mserver/CLI/tests/cli_roundtrip.rs` — the renewal before packing and the abort after it.
- `docs/proteus/MSERVER-CLAIM-LEASE-1.md`

## Evidence

Store tests (`mods.rs`):

- `a_reservation_lease_outlives_the_pack_and_upload_it_covers` — a fresh reservation answers
  `renewAfterMs`, its claim reads `lease_held`, and `remove_unused_claims` at **zero** age removes
  nothing (the lease, not the write time, is what keeps it). A second reservation whose lease is
  forced into the past is removed by the same sweep at once, with the live entry's claim left
  alone. A third reservation's first renewal — the one a publisher sends right after reserving —
  answers `Ok` and leaves the object byte-identical (a fresh lease is not rewritten); its lease is
  then forced into the past and a second renewal writes a new `lease_until`, with the token
  unchanged.
- `only_the_holder_can_renew_and_a_finished_publish_has_nothing_to_renew` — another publish's token
  is refused (`belongs to another publish`), a folder with no claim is refused
  (`no install-folder reservation`), and a renewal after the upload consumed the token answers
  `Ok` with the claim still token- and lease-free.
- `a_token_left_on_an_installed_folder_does_not_refuse_the_next_revision` — a token written back onto
  the claim of an installed entry (what a renewal that raced the consume leaves) is cleared by the
  next revision that keeps the folder, and both `token` and `lease_until` end up absent.

Route test (`lib.rs`):

- `http_folder_claim_renewal_answers_only_its_own_publish` — 200 reserve (with `renewAfterMs`), 204
  for its own token, **409** for another token, **404** for a folder no publish holds.

`check-names` (`tests/check_names.rs`):

- `fixing_keeps_a_claim_whose_lease_is_still_running` — an orphan claim whose object is an hour old
  (past the floor) and whose `lease_until` is ten minutes in the future is kept by `--fix` with
  `; kept, its reservation is still being renewed`; the object survives.

CLI (`tests/cli_roundtrip.rs`):

- `publish_renews_its_reservation_while_it_works` — the stub master answers `renewAfterMs`; the
  request order is `GET /v1/mods/beta`, `POST /v1/catalog/folder-claim`,
  `POST …/folder-claim/renew`, then — because the source cannot be packed — `POST …/folder-claim/abort`.
  A free claim still reaches packing, and the abort is last.

Broken-state deltas, measured one at a time and restored:

- dropping the sweep's `lease_held` skip fails `a_reservation_lease_outlives…` and
  `fixing_keeps_a_claim_whose_lease_is_still_running`;
- dropping the `keeps_folder` leftover arm fails
  `a_token_left_on_an_installed_folder_does_not_refuse_the_next_revision`;
- dropping the CLI's immediate renewal leaves three requests (`abort` where `renew` should be) and
  fails `publish_renews_its_reservation_while_it_works` on `left: 3, right: 4`.

Counts: `papa-bear-master-service` **117 lib + 7 bin + 16 integration**, `papa-bear-cli` **20 + 5**,
`papa-bear-client` **15**, 0 failed (113 + 7 + 15, 20 + 4 and 15 before). Six consecutive parallel
`--lib` runs are green after the new tests were spread over separate claim objects; the one initial
failure was the recorded Windows local-store rename race (`Access is denied` while a scanner held a
freshly written claim), which `put_replacing` already retries. `cargo clippy` on master and CLI has
no diagnostic in the new items, `cargo fmt` on both crates is clean, and `check-all` is OK.

Manual run on Windows with the built binaries, a temp SQLite database and mods root
(`127.0.0.1:18116`, admin key `lease-key`):

```
$ papa publish src --mod-id alpha --name Alpha --folder-name alpha-dir
published 'Alpha' as alpha, version 1.0, package revision 1 (4182 bytes compressed) -> …
_claims/616c7068612d646972.json = {"modId":"alpha","folder":"alpha-dir"}          (consumed)

$ POST /v1/catalog/folder-claim {"modId":"gamma","folderName":"gamma-dir"}
200 {"modId":"gamma","folder":"gamma-dir","token":"272e…be25d","renewAfterMs":200000}
_claims/67616d6d612d646972.json = {"modId":"gamma","folder":"gamma-dir",
                                   "token":"fb5e…ac8b7","lease_until":1792990166568}

$ papa-bear-master-service check-names --fix --mods-dir mods
orphan claim: 'gamma-dir' is reserved by 'gamma', which is not a catalog entry
              (claimed 0s ago; kept, its reservation is still being renewed)
Error: check-names: 1 unused claim in 1 catalog entry                            (exit 1)
gamma claim survives: True

$ POST /v1/catalog/folder-claim/renew {"…","token":"272e…be25d"}   (lease still fresh)
204; claim byte-identical: True

$ POST /v1/catalog/folder-claim/renew {"…","token":"nope"}
409 the reservation for folder 'gamma-dir' belongs to another publish
$ (lease_until set to 1) POST /v1/catalog/folder-claim/renew {"…","token":"272e…be25d"}
204; lease_until: 1792990166593; token unchanged: True

$ POST /v1/catalog/folder-claim/abort {"…","token":"272e…be25d"}
204; claims left: ['616c7068612d646972.json']
```

The worker thread's own cadence (200 s) is not observable in a publish that finishes in a second;
its request path is the same one the manual run and the CLI test exercise. Temp state removed and
the server stopped.

## Risks

- **A crashed reservation strands its folder for the lease.** Ten minutes (plus the sweep) instead
  of five; an ordinary failure still aborts at once, and a reappearing publisher cannot extend a
  lease it does not hold.
- **The renewal is a read followed by a write.** `object_store` 0.11 offers no conditional update on
  the local backend, so a renewal that reads the claim before the upload consumes the token writes
  the token back afterwards. The next revision that keeps the folder clears it, but until then a
  plain upload of that claim is refused `reserved by a publish … that has not finished`. The window
  is one renewal request every 200 s. Closed by `MSERVER-CLAIM-CAS-1`: the claim records a
  generation and its rewrite compares it, so the renewal is refused instead of writing the body it
  read.
- **A renewal already in flight when the publish fails can re-create the claim.** The CLI joins the
  worker before aborting, so only a request the master was already handling can land after the
  delete; its claim then carries no token and no entry, and `--fix` removes it once the lease runs
  out. The folder is refused for that lease. Closed by `MSERVER-CLAIM-CAS-1`: the rewrite reads the
  claim again with the write window held, so a claim that is gone is not written back.
- **The renewal a publisher sends right after reserving does not move the deadline.** It is inside
  the first 200 s of a ten-minute lease, and the worker's next attempt 200 s later writes; a master
  clock that jumped forward would make it write, which only extends the lease.
- **A report-only `check-names` still fails during a live publish.** A reservation is reported as an
  unused claim (with `; kept` under `--fix`); a maintenance sweep should not run while publishers
  are packing.
- **The lease is measured on the master's clock.** The publisher never computes a deadline, so no
  client clock participates; a master whose clock jumps backwards could extend a lease it should not,
  which only delays a reclaim.
- **An old master is not renewed.** Without `renewAfterMs` the CLI does not start the worker and
  uploads exactly as before, so a lease-less master keeps the five-minute behaviour.
- **The cadence is the master's, not the network's.** One failed renewal prints a warning and is
  retried a third of a lease later; two consecutive failures would let the lease lapse, which the
  upload's own claim check still decides correctly (only later).
- **A hung renewal delays the publish by at most its timeout.** The worker's agent caps a request at
  30 seconds, so dropping the guard (and with it the commit or abort) cannot wait longer than that
  on a dead connection.

## Follow-ups

- (done, `MSERVER-CLAIM-KEEP-1`) Reserve for a revision that keeps its folder too, with a lease the
  sweep can age out, so two publishes of one entry stop before streaming as well. The reservation
  always mints a token; a live lease refuses a second publish (and a token-less upload) while a
  lapsed one is taken over, and a lapsed claim on a folder the entry installs into is a
  `LapsedReservation` the sweep removes at any age.
- (done, `MSERVER-CLAIM-CAS-1`) A conditional claim write so a renewal cannot race a consume or an
  abort. `StoredClaim::generation` is compared inside a per-claim write window
  (`_claims/<hex>.json.writing`, created with the same conditional-put rule as the claim), so a
  write decided against one body is refused once that body moved.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
# the lease by hand: start the service, then
#   POST /v1/catalog/folder-claim {"modId":"gamma","folderName":"gamma-dir"}   -> 200 + token + renewAfterMs
#   cargo run -p papa-bear-master-service -- check-names --fix --mods-dir <root>
#   POST /v1/catalog/folder-claim/renew {"modId":"gamma","folder":"gamma-dir","token":"<t>"} -> 204
#   POST /v1/catalog/folder-claim/abort {…same body…}                            -> 204
papa publish <folder> --admin-key <key> --mod-id alpha --name Alpha --folder-name alpha-dir
```

Expect `--fix` to keep the reservation with `; kept, its reservation is still being renewed` and
exit 1 while the lease runs, the renewal to answer 204 and move `lease_until` in the claim object,
and the abort to leave only the published entry's claim.
