# MSERVER-FOLDER-RESERVE-1 — an install folder is reserved, not just read

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Closes the follow-up of `MSERVER-FOLDER-CLAIM-1` and `MSERVER-PREFLIGHT-1`. Those two added a
publish-time check and a publisher preflight, both of which read the catalog. A read cannot decide
between two publishers that overlap: each finishes its read before either writes, so both see the
folder free and both publish.

## Problem

An entry installs into `<modsRoot>/<folderName>`, or `@<modId>` when it carries none, and the install
transaction clears that directory before unpacking. Two entries that resolve to one folder replace
each other, which is why publish refuses a folder another entry holds. But the refusal is a
`catalog_directories` + `stored_entry` scan: a snapshot, with no write that can lose the race.

```
publisher A            publisher B            catalog
scan: free             scan: free             (nothing claims shared)
                                               <- both scans happen here
claim shared  -------- write A ------------>  entry A, folder shared
                       claim shared  ------>  entry B, folder shared   <- both passed
```

The preflight has the same shape and says so; it only moves the losing refusal earlier for the
common case. The deterministic answer has to be a write whose outcome the store orders for us.

## Approach

- **One object per folded folder.** `_claims/<hex of the lowercased folder>.json`, body
  `{"modId": "<catalog directory>", "folder": "<folder as written>"}`. The hex keeps any name the
  component rule accepts (non-ASCII, `@`, C1) a valid object key on both backends, and the fold is
  the Windows client's view — `Shared` and `SHARED` are one directory there, so they are one claim
  here. Claim objects are ordinary files under a top-level directory, never a `mod.json` or
  `current` marker, so `catalog_directories` does not mistake them for entries.
- **Conditional put.** `ModStore::reserve_install_folder` writes the object with
  `PutMode::Create`: S3/R2 answer `If-None-Match: *` in a single PUT, and the local backend uses
  `hard_link`. Exactly one publisher's put succeeds; the other gets `AlreadyExists`, reads the
  winner, and is refused with the same `folder_claim_refusal` text the catalog check uses. The
  conditional put was already required for revision metadata, so no backend gains a new dependency.
- **Keeping a folder is not a new claim.** A revision that keeps its folder (compared folded)
  skips the advisory catalog lookup and never refuses on another holder, so an entry caught in a
  pair that predates the rule can still publish — and the sweep keeps reporting the pair. It still
  attempts the put: that is how an entry published before reservations existed acquires its claim
  object on its next revision that reaches this point (a revision whose artifact hash is unchanged
  returns earlier, as it always has), and an `AlreadyExists` naming a different entry is ignored in
  that case rather than turned into a refusal.
- **Legacy entries still answer through the read.** Entries published before this ticket have no
  claim object, so the read-only `install_folder_holder` lookup runs first, unchanged, and a new
  claim then serializes on the put. Two new claims cannot both pass; a new claim pointed at a
  legacy entry's folder is refused before the put.
- **Give the claim back.** A publish that fails after taking the claim deletes it **only when this
  call created it** (`InstallFolderReservation.created`), so a loser can never remove the winner's
  claim, and a retried publish of an existing entry cannot remove its own earlier claim. A revision
  that moves folders releases the old claim after the new revision is current (a failed move keeps
  the folder it still has). `delete_mod` releases every claim naming the entry, so deleting a mod
  frees its folder instead of reserving it forever.
- **The preflight reads the reservation first.** `check_install_folder` asks the claim object
  before the catalog, so it reports a folder taken by a publish that has no catalog entry yet —
  the in-flight window that the upload refuses in — and then falls back to the read for legacy
  entries. The answer, including the refusal wording, is now the one the upload will give.
- **Robust listing.** `delete_mod`'s cleanup only parses finished `<hex>.json` objects; the local
  backend stages an upload as `<hex>.json#1`, which a concurrent put can leave for an instant.

## Behaviour

| situation | outcome |
| --- | --- |
| new entry, free folder | conditional put wins, entry publishes, claim names it |
| two new entries, same folded folder, overlapping | exactly one publishes; the other is `409` naming the winner |
| new entry, folder held by a pre-reservation entry | refused by the catalog read, before any claim write |
| revision keeps its folder | allowed; the claim object is written if it was missing, never refused by its own entry |
| revision moves to a free folder | claims the new folder, then releases the old one |
| revision moves onto a claimed folder | `409`; the current revision is untouched and the old claim stays |
| publish fails after claiming (placement, metadata, current) | the claim this call created is deleted; the folder is free |
| entry deleted | every claim naming it is deleted |

## Files

- `mserver/MasterService/src/mods.rs` — `CLAIM_PREFIX`/`StoredClaim`/`InstallFolderReservation`,
  `claim_path`, `read_claim`, `claim_holder`, `reserve_install_folder`, `release_reservation`,
  `release_claim_for`, `release_claims_of`, `is_claim_file`; the three publish paths reserve, and
  `delete_mod` and `check_install_folder` use the index; five store tests.
- `docs/proteus/MSERVER-FOLDER-RESERVE-1.md`

## Evidence

Store tests (`mods.rs`):

- `two_concurrent_publishes_cannot_both_take_one_folder` — `tokio::join!` of two real
  `finalize_mod` publishes (`Alpha`/`shared`, `Beta`/`SHARED`, both cloned stores): exactly one is
  `Ok`, the loser's message names the winner, `list_mods` holds one entry, and `_claims` holds one
  object. Neither scan can see the other, so only the put can produce this.
- `a_reservation_refuses_before_the_winner_has_a_catalog_entry` — a reservation is taken directly
  for `alpha`/`shared` (no entry exists), then `Beta` is refused at `finalize_mod` and the
  preflight reports `available: false`, `claimedBy: alpha`; releasing the reservation lets `Beta`
  take the folder. This is the in-flight window, without needing a paused future.
- `a_failed_publish_releases_the_folder_it_reserved` — a stray `beta/revisions/1/beta.pbo.zst`
  makes placement fail after the claim; the failure keeps no claim object and no entry, and the
  same publish succeeds on retry.
- `deleting_a_mod_frees_the_folder_it_reserved` — publish, `delete_mod`, no claim left, another
  entry publishes into the folder.
- `a_revision_that_moves_releases_the_folder_it_left` — `alpha-dir` → `alpha-new` leaves one claim;
  `Beta` takes `alpha-dir`; `Gamma` is refused `alpha-new` naming `alpha`, whose own folder is
  still available to it.

Counts: `papa-bear-master-service` **97 lib + 7 bin + 8 integration**, 0 failed (92 + 7 + 8
before; +5 store). `papa-bear-client` 15, `papa-bear-cli` 19 unit + 4 integration, unchanged and
green. `cargo clippy -p papa-bear-master-service --all-targets` exits 0 with no warning in the new
code (the pre-existing `too_many_lines` on `finalize_revision` remains). `check-format` and
`check-all` OK; roadmap regenerated.

Manual run on Windows against the built binaries, temp SQLite directory and mods root, two
2,000,000-byte random sources published at the same instant into one folder:

```
$ papa publish src-alpha --mod-id alpha --name Alpha --folder-name Shared     (exit 1)
Error: publish rejected: HTTP 409: folder 'Shared' is already claimed by catalog entry 'beta' as 'SHARED' (one folder on a case-insensitive filesystem); publish with a different folderName
$ papa publish src-beta  --mod-id beta  --name Beta  --folder-name SHARED     (exit 0)
published 'Beta' as beta, version a9d1447d, package revision 1 (2000161 bytes compressed) -> …
$ cat store/_claims/736861726564.json
{"modId":"beta","folder":"SHARED"}
```

Both preflights passed (the catalog was empty when they ran), so the 409 is the reservation, not the
read. The catalog listed one entry, `_incoming/` was empty, and the follow-through behaved the same
way: `GET /v1/catalog/folder-claim?modId=gamma&folderName=sHaReD` answered `available:false`,
`claimedBy:beta`; a third publish stopped at `publish refused before uploading: …`; `beta` revision
2 with `--folder-name beta-new` replaced its claim (`626574612d6e6577.json`) and `gamma` then took
`Shared`; `DELETE /v1/mods/beta` released `beta-new` and `delta` took it. Final `check-names
--mods-dir` printed `checked 2 catalog entries, no rejected names, no shared install folders, every
artifact intact`, exit 0. Temp state removed and the server stopped.

## Risks

- **A stale claim blocks its folder.** A crash between the conditional put and the release leaves
  `_claims/<hex>.json` naming an entry that never appeared; later publishes are refused with that
  entry's name until the object is deleted. No automatic expiry: a lease short enough to heal
  itself could expire under a slow writer and hand the folder to a second publisher — the race the
  claim exists to stop. Recovery is an operator deleting the object (or accepting a publish of the
  same mod id, which reuses the claim).
- **The claim is taken when the upload finalizes.** Two publishers still stream their bodies before
  one loses; only the folder decision is ordered. Reserving before the upload needs a
  reserve/commit protocol and a lease, which is a bigger change than this ticket.
- **Pre-reservation entries stay read-only.** An entry published before this ticket carries no
  claim object until its next revision; until then its folder is protected by the catalog read
  alone. That is the same guarantee as before for those entries, not a regression, but it is not
  atomic.
- **`_claims` is an ordinary top-level name.** A mod id of exactly `_claims` would share the
  directory; publishing it is already an odd administrative act, and deleting it removes every
  reservation — which only relaxes the check back to the catalog read.
- One extra object per claimed folder, one put per publish, and `delete_mod` lists the whole
  `_claims` directory to find the departing entry's claims.

## Follow-ups

- Report orphaned claims (a claim naming no catalog entry) from `check-names`, with a `--fix` that
  removes them, so a crashed publish is visible instead of silent.
- Reserve before the upload (a claim endpoint that returns a token, plus commit/abort) so the
  losing publisher stops at the claim instead of after the stream.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
cargo run -p papa-bear-master-service -- server --listen 127.0.0.1:8080 --admin-api-key dev --mods-dir <mods root>
papa publish <one>   --master http://127.0.0.1:8080 --admin-key dev --mod-id alpha --name Alpha --folder-name Shared
papa publish <other> --master http://127.0.0.1:8080 --admin-key dev --mod-id beta  --name Beta  --folder-name SHARED
```

Launch both publishes at the same instant. Expect exactly one `published …` and one
`publish rejected: HTTP 409: folder '…' is already claimed by catalog entry '…'`, one entry in
`GET /v1/mods`, and one object in `<mods root>/_claims/` whose body names the winner. A revision
that keeps its folder must still publish, and `DELETE /v1/mods/<winner>` must free the folder.
