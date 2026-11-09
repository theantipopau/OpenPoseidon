# MSERVER-CLAIM-RESERVE-1 — a publish reserves its install folder before it streams, and the loser is refused instead of losing after the upload

Status: done-unverified (HTTP service; nothing to verify on a GPU)

Closes the last follow-up of `MSERVER-CLAIM-MOVES-1`, repeated on the three tickets before it.
`MSERVER-PREFLIGHT-1` taught `papa publish` to ask about its install folder before packing, but the
answer was a read: two publishers of one folder could both see it free, both pack, and both stream,
and the loser learned the truth only after the whole compressed artifact had arrived.

## Problem

The claim object already decides between concurrent publishes — but both of them acquire it at
`finalize`, after the bytes have been sent:

```
publisher A: GET folder-claim  -> available          publisher B: GET folder-claim -> available
             pack (minutes)                                       pack (minutes)
             POST /v1/mods  (streams 8 GB)                        POST /v1/mods  (streams 8 GB)
             claim take -> ok                                     claim take -> refused, temp deleted
```

The preflight cannot be made authoritative by reading harder: the catalog is a snapshot and the
gap between the read and the put is the whole upload. What a publisher needs is the exclusion
itself, taken before it packs.

## Approach

- **The preflight can reserve.** `POST /v1/catalog/folder-claim` resolves the same claim the upload
  would (`modId` or the slug of `name`, `folderName` or the current revision's folder, the
  `@<modId>` fallback), takes it with the same conditional put, and answers
  `FolderClaimReservation { modId, folder, token }`. A folder another entry holds answers 409 with
  the identical refusal wording, now before a byte is packed. Admin-gated, because a reservation
  refuses the folder to every other publish. The read-only `GET` stays for older publishers.
- **A token only its publisher has.** `new_claim_token` draws 32 random bytes as hex; the claim body
  records only `SHA-256(token)` (`StoredClaim::token`, `serde(default)`, so pre-ticket bodies still
  parse). A reservation that is presented back proves it is the same publish without the store ever
  holding the secret.
- **The upload consumes it.** `papa publish` sends the token as a multipart `claimToken`;
  `finalize_mod`/`finalize_revision` take the claim as `ClaimTake::Consume` instead of `Plain`. The
  body is rewritten token-less *before* the revision is placed, so once the entry installs into the
  folder no abort can remove its claim. A failed publish releases a claim it created *or consumed*
  (`InstallFolderReservation::owned` replaces `created`), because a consumed reservation is this
  publish's; a claim written by an earlier publish of the entry is still left alone.
- **An upload that never reserved cannot walk over a lease.** `ClaimTake::Plain` on a tokened claim
  is refused (`reserved by a publish of '<mod>' that has not finished`), as is a second
  reservation of the same entry (`already reserved by another publish of '<mod>'`), and a token
  that is not the claim's (`this upload no longer holds it`). That last comparison is the hash, so a
  pre-ticket publisher that sends `claimToken` cannot guess its way into a lease.
- **Commit and abort close the lifecycle.** `POST /v1/catalog/folder-claim/commit` clears the token
  and is idempotent — the upload that carried it already committed, so a publisher can send it after
  every successful publish. It refuses while the entry does not install into the folder, because the
  token is then the only way to give the folder back. `POST /v1/catalog/folder-claim/abort` deletes
  the claim only when the token matches and the catalog does not install there: a reservation that
  never became a publish frees its folder at once instead of waiting out the sweep's age floor.
- **A revision that keeps its folder reserves nothing.** The folder is already the entry's and no
  other entry can take it, so the reservation answers no token. This also keeps the sweep honest:
  `unused_claims` deliberately ignores a claim whose entry installs into that folder, so a token left
  on such a claim by a crashed publisher could never be swept and would block that entry's next
  publish forever.
- **A claim body is rewritten through one retrying write.** The local store replaces an object by
  renaming over it, and Windows refuses that rename with `ACCESS_DENIED` while a virus scanner or
  the search indexer holds the freshly written file. Every claim rewrite now goes through
  `ModStore::put_replacing`, which retries that refusal up to eight times with a linear back-off
  (1.4 s at most) and writes the identical body; the same helper carries the `current`, `mod.json`
  and compatibility rewrites, so the flake never reaches a publisher as an error. The full lib
  suite exposed it: 2 of 12 parallel runs failed with `Unable to rename file: Access is denied`,
  and a temporary probe fired on 4 and 13 writes in two passing runs afterwards.
- **The publisher gives the folder back when it fails.** `papa publish` reserves before
  `build_artifact_reader` runs, sends the token with the upload, aborts on any failure (packing,
  compression, HTTP) and commits after a successful publish. A master that predates the route answers
  404: the CLI falls back to the read-only preflight and uploads without a token, as before.

## Behaviour

| request | claim object | answer |
| --- | --- | --- |
| reserve a free folder | created with `token = sha256(t)` | 200 + `t` |
| reserve a folder another entry holds | unchanged | 409, the upload's refusal wording |
| reserve the same entry again | unchanged | 409, `already reserved by another publish` |
| upload with `claimToken = t` | token cleared, `replaces` recorded | 200, entry published |
| upload without the token | unchanged | 409, `reserved by a publish of '<mod>' that has not finished` |
| upload with a different token | unchanged | 409, `this upload no longer holds it` |
| commit after the publish | already token-less | 204 (idempotent) |
| commit before the publish | unchanged | 409, `does not install into folder … yet` |
| abort with `t` before the publish | deleted | 204, folder free at once |
| abort with another token, or after the publish | unchanged | 409 |

## Files

- `mserver/MasterService/src/mods.rs` — `StoredClaim::token`; `ClaimTake`; `new_claim_token` and
  `claim_token_hash`; `reserve_install_folder` takes a `ClaimTake` and rewrites the body for a
  reservation, a move record or a consumed token; `InstallFolderReservation::owned`;
  `reserve_folder_claim`, `commit_folder_claim`, `abort_folder_claim`; `resolve_claim` shared with
  `check_install_folder`; `put_replacing` and `is_refused_rename`, the one overwrite every claim
  body (and `current`, `mod.json`, compatibility) goes through; seven store tests and a check of
  the refusal predicate.
- `mserver/MasterService/src/model.rs` — `FolderClaimReservation`, `FolderClaimToken`.
- `mserver/MasterService/src/service.rs` — the three store calls behind the service.
- `mserver/MasterService/src/http.rs` — `POST /v1/catalog/folder-claim`, `…/commit`, `…/abort`; the
  `claimToken` multipart field; `folder_claim_mod_id` shared with the `GET`; `claim_error` status
  mapping; the upload's conflict mapping now includes `reserved by`.
- `mserver/MasterService/src/lib.rs` — `http_folder_reservation_stops_a_second_publisher_before_an_upload`.
- `mserver/CLI/src/main.rs` — reserve before packing, token in the multipart prelude, abort on
  failure, commit after success, the 404 fallback.
- `mserver/CLI/tests/cli_roundtrip.rs` — the stub drains POST bodies and answers the reservation and
  the abort; the refusal and free-folder cases now assert the new request order.
- `docs/proteus/MSERVER-CLAIM-RESERVE-1.md`

## Evidence

Store tests (`mods.rs`):

- `a_reservation_holds_the_folder_before_anything_is_streamed` — a reservation answers a 64-character
  token and writes only its hash; another entry naming the folder is refused with the upload's
  wording; the same entry reserving again is refused; the sweep reports the lease as `NoEntry` with
  `may_still_be_finishing`, which is the age floor that keeps `--fix` away from it.
- `the_upload_consumes_the_reservation_token` — an upload without the token is refused and leaves no
  entry; a wrong token is refused with `this upload no longer holds it`; the right one publishes,
  leaves a token-less claim, and cannot be aborted afterwards while commit answers `Ok`.
- `a_failed_upload_gives_back_the_reservation_it_consumed` — the token is consumed, then placement
  fails on a stray revision-1 object; the claim is gone and another entry reserves the folder at once.
- `a_reservation_cannot_be_committed_before_its_publish` — commit is refused until the entry installs
  there, and the abort still frees the folder.
- `aborting_a_reservation_frees_the_folder_at_once` — a wrong token cannot release; the right one
  deletes the object and the folder is reserved by another entry immediately.
- `a_revision_that_keeps_its_folder_reserves_nothing` — no token for an entry that already installs
  there, and no token in the body.
- `a_move_reserves_its_destination_and_aborting_leaves_the_old_folder` — a move's reservation records
  `replaces`, refuses another entry, aborts back to the old folder, and a second reservation plus the
  revision upload commits the move and releases the old claim.

Route test (`lib.rs`):

- `http_folder_reservation_stops_a_second_publisher_before_an_upload` — 401 without the admin key;
  200 with a token; 409 for a second entry into the folder; a token-less upload is 409 and advertises
  nothing; the token-carrying upload publishes; abort afterwards is 409 (`already installs`); commit
  is 204 and idempotent.

Broken-state delta, measured and restored:

- consuming the token without owning the claim (`Some(_) => (plain, false)`) makes
  `a_failed_upload_gives_back_the_reservation_it_consumed` fail on
  `the reservation survived the publish it was consumed by`;
- dropping the token comparison in `ClaimToken::Consume` (any token accepted) makes
  `the_upload_consumes_the_reservation_token` fail on `unwrap_err()` of an `Ok(ModCatalogEntry)` —
  the wrong-token upload published;
- the CLI round-trip tests pin the order (`POST /v1/catalog/folder-claim` before the source is
  touched, `…/abort` after the failure), so moving the reservation back after packing fails them by
  the recorded request lines.

Counts: `papa-bear-master-service` **113 lib + 7 bin + 15 integration**, 0 failed (104 + 7 + 15
before; +7 store, +1 route, +1 refusal predicate). `papa-bear-cli` **20 + 4** (19 + 4 before; +1 prelude test, the two
round-trip cases extended in place) and `papa-bear-client` 15, both green. `rustfmt --check` clean on
the seven files. `cargo clippy -p papa-bear-master-service -p papa-bear-cli --all-targets` exits 0
with no diagnostic in a new item (the `mods.rs`/`http.rs` hits are the older functions they were on;
two new ones the first draft introduced — identical match arms in `ClaimTake::token_hash`, a
`by value` error in `claim_error` — were fixed rather than allowed). `check-all` OK with the roadmap
regenerated.

Manual run on Windows with the built binaries and a temp root:

```
$ papa publish tmp/claim-reserve/src-alpha --admin-key secret --mod-id alpha --name Alpha \
    --folder-name alpha-dir
published 'Alpha' as alpha, version 9fee5cd2, package revision 1 (52 bytes compressed) -> …

$ papa publish tmp/claim-reserve/not-a-folder --admin-key secret --mod-id beta --name Beta \
    --folder-name ALPHA-DIR
Error: publish refused before uploading: folder 'ALPHA-DIR' is already claimed by catalog entry
'alpha' as 'alpha-dir' (one folder on a case-insensitive filesystem); publish with a different
folderName
```

The second source does not exist, so reaching the refusal instead of `is neither a folder nor a file`
proves the reservation ran before packing. The claim after the successful publish was
`{"modId":"alpha","folder":"alpha-dir"}` — no token. `POST` for `gamma` into `gamma-dir` answered a
64-character token and wrote `{"modId":"gamma","folder":"gamma-dir","token":"<64 hex>"}`; the abort
answered 204 and left only alpha's claim, and a second abort answered
`no install-folder reservation is held for 'gamma-dir'` (404). Temp state and the service process
removed. The two-publisher race itself is covered by the route test, which is the only place one
process can hold a reservation while another tries to take it.

## Risks

- **The age floor was sized for a claim taken at finalize; a reservation is older by the pack and the
  transfer.** `check-names --fix` running more than five minutes into a large pack or an upload could
  delete the lease. The publisher then loses the folder at finalize (after streaming), or another
  publisher takes it while the first is still sending. Closed by `MSERVER-CLAIM-LEASE-1`: the claim
  carries a lease the publisher renews while it works, and the sweep will not take a claim whose
  lease is running at any age.
- **A publisher that dies between reserving and aborting leaves a token.** The same entry cannot
  reserve the folder again until the sweep's five minutes (or `--fix`), where a pre-ticket crash
  cost nothing because no claim existed yet.
- **A lost reservation response strands its token.** The claim ages out, but the publisher cannot
  abort a token it never saw and the entry is refused meanwhile.
- **Only the reservation's own folder spelling releases it.** Commit and abort address the claim by
  the folder string; a publisher that normalizes it differently gets 404.
- **A same-entry revision still cannot be excluded early.** Keeping the folder reserved nothing (by
  design, for sweepability), so two publishes of one entry could still both stream; the first rule
  that caught them was the entry's own artifact placement. Closed by `MSERVER-CLAIM-KEEP-1`: a kept
  folder is reserved with a lease, so the second publish stops at the reservation.
- **The token comparison is not constant-time.** With a 256-bit SHA-256 value behind an admin-gated
  API the leak has no practical target, but it is a `==` on a secret-adjacent value.
- **A commit before the upload is an error the publisher can fix.** It is refused rather than
  silently accepted; a publisher that treats "publish succeeded" as the signal is not affected.

## Follow-ups

- (done, `MSERVER-CLAIM-LEASE-1`) Let a reservation outlive a long transfer: the claim records a
  `lease_until` the publisher pushes forward through `POST /v1/catalog/folder-claim/renew`, and
  `check-names --fix` will not take a claim whose lease is still running, so a sweep during a 20 GB
  publish cannot drop it.
- (done, `MSERVER-CLAIM-KEEP-1`) Reserve for a revision that keeps its folder too, with a lease the
  sweep can age out, so two publishes of one entry stop before streaming as well.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
# the route, by hand: start the service, then
#   POST /v1/catalog/folder-claim  {"modId":"alpha"}            -> 200 + token
#   POST /v1/catalog/folder-claim  {"modId":"beta","folderName":"@alpha"} -> 409
#   POST /v1/catalog/folder-claim/abort {"modId":"alpha","folder":"@alpha","token":"<t>"} -> 204
papa publish <folder> --admin-key <key> --mod-id alpha --name Alpha --folder-name alpha-dir
papa publish <missing path> --admin-key <key> --mod-id beta --name Beta --folder-name ALPHA-DIR
```

Expect the first publish to print `published …`, the second to print
`publish refused before uploading: folder 'ALPHA-DIR' is already claimed …` without the source being
touched, and the claim object on disk to hold no `token` after the publish. A reservation whose
publish is killed before the abort must still be visible to `check-names` as an orphan claim once it
is older than five minutes.
