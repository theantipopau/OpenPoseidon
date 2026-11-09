# MSERVER-CLAIM-CAS-1 — a claim write carries the generation it was decided against, so a renewal cannot resurrect a consumed token

Status: done-unverified (master; nothing to verify on a GPU)

Closes the conditional-write follow-up of `MSERVER-CLAIM-LEASE-1` and `MSERVER-CLAIM-KEEP-1`.

The lease told a publisher how long to keep renewing and `--fix` how long to wait; the writes it
guarded were still blind. Every claim writer read a body, decided, and then replaced whatever was
there. One interleaving made that visible: a renewal that read a token-bearing claim just before
the upload consumed it wrote the token back after, with a fresh deadline, so the folder refused its
own entry's next revision for up to ten minutes. Both earlier tickets recorded that window as a
risk; this one closes it by making the write itself carry the body it was decided against.

## Problem

```
t=0     renewal reads _claims/<hex>.json: token t, lease 120 s left, generation g
t=0     the upload consumes it:           token -,  lease -,                 generation g+1
t=0     renewal writes the body it read:  token t, lease 120 s left         (blind overwrite)
t=1..9  a revision of the same entry without the token: 409 reserved by a publish … not finished
t=10    the lease runs out; the next revision (or --fix) finally clears it
```

Two smaller shapes of the same bug:

- **Two renewals from one body.** Both extend the lease and the later write wins; a request that
  computed its deadline earlier can shorten the lease the other just wrote.
- **A renewal in flight when its publish aborts.** It re-creates the claim after the delete, leaving
  a token on a folder with no publisher, which refuses the folder for a lease.

Reading more carefully cannot fix any of them: `object_store` 0.11 has no conditional update for an
existing object on the local backend (only S3's `Update` mode), so a read followed by a plain `put`
is a lost update however recent the read is.

## Approach

- **The claim carries a generation.** `StoredClaim::generation: u64`, `serde(default)` so every body
  written before this ticket reads as 0. The initial `Create` writes 1; every later write stores the
  generation one higher than the one it was decided against.
- **`rewrite_claim` is a compare-and-set.** It answers `bool`: `true` when the stored body still held
  the generation the caller read, `false` when it had moved (or the write window could not be
  taken). Callers re-read and decide again instead of writing a decision about a body that is gone.
- **The read-check-write is one step.** `lock_claim` creates `_claims/<hex>.json.writing` with the
  same `If-None-Match: *` / `hard_link` rule the claim itself uses — one rule for both backends,
  neither of which offers a conditional update of an existing object. `is_claim_file` accepts only
  `<hex>.json`, so neither a sweep nor `release_claims_of` ever sees a window as a claim. A marker
  older than five seconds belongs to a master that died inside the window and is cleared;
  `unlock_claim` is best effort, because the guarded write already landed and a scanner holding the
  marker open must not turn that into a reported failure.
- **A reservation retries its decision.** `reserve_install_folder` keeps the `Create` for a free
  folder; on `AlreadyExists` it reads the winner and decides (take, refuse, keep) as before, but
  writes through the CAS and, when refused, loops to decide against the body that won. The loop is
  bounded at ten attempts and then answers `being claimed by several publishes of …`.
- **Renew and commit loop too.** A renewal that loses to a consume re-reads, finds no token and
  answers success; one that loses to a takeover answers 409; a commit that loses to a renewal
  re-reads and clears the token from the newer body instead of writing a body that never saw the
  renewal.
- **Abort, the sweep and entry deletion delete conditionally.** `delete_claim_if` takes the window,
  reads the body and its write time again, and deletes only while the predicate still holds: abort
  re-checks its own token, the sweep re-checks "the body the scan judged, not written since", and
  `release_claims_of` re-checks that the claim still names the entry being deleted.
- **HTTP status.** `claim_error` maps the new transient refusals (`changed while it was being …`,
  `being claimed by several publishes`) to 409, so a publisher can retry them as the wording says.

The follow-up offered a generation or a tombstone. The generation is what lets a writer prove which
body it saw; the marker is what makes the read-compare-write atomic without a store-level compare.

## Behaviour

| interleaving | before | now |
| --- | --- | --- |
| renewal reads, upload consumes, renewal writes | token and lease back | write refused; renewal re-reads and answers 204 |
| two renewals from one body | both write, last one wins | one wins; the other renews the newer body |
| renewal in flight when its abort deletes the claim | claim re-created with a token | write refused (no body); folder stays free |
| takeover of a lapsed claim races the old renewal | takeover body can be overwritten | the generation decides; the loser re-reads |
| sweep deletes a claim taken after the scan | read-then-delete window | windowed; `retaken`, row `kept, the claim changed after the scan` |
| entry deleted while a renewal holds its claim | claim deleted under the renewal | delete refused; the claim is left to the sweep |
| master dies holding a write window | — | marker cleared by the next writer after five seconds |

## Files

- `mserver/MasterService/src/mods.rs` — `StoredClaim::generation` and `next_generation`, the
  compare-and-set `rewrite_claim`, `lock_claim` / `unlock_claim` / `claim_marker_expired` /
  `claim_write_marker`, `delete_claim_if`, the retry loops in `reserve_install_folder`,
  `renew_folder_claim` and `commit_folder_claim`, the windowed abort, the sweep's windowed delete,
  `release_claims_of` deleting through the same window, the three new constants, and the three
  store tests.
- `mserver/MasterService/src/http.rs` — the renewal route's description and `claim_error`.
- `mserver/CLI/src/main.rs` — the comment on why renewing stops before commit or abort.
- `docs/proteus/MSERVER-CLAIM-CAS-1.md`; the follow-up and risk lists of
  `docs/proteus/MSERVER-CLAIM-LEASE-1.md`, `docs/proteus/MSERVER-CLAIM-KEEP-1.md`,
  `docs/proteus/MSERVER-CLAIM-MOVES-1.md` and `docs/proteus/MSERVER-CLAIM-RECHECK-1.md`, whose
  remaining windows and follow-ups this ticket closes.

The wire format is unchanged: `generation` lives inside the claim object, which no client reads.

## Evidence

Store tests (`mods.rs`):

- `a_renewal_holding_a_stale_body_cannot_write_a_consumed_token_back` — publishing `alpha` into
  `alpha-dir`, reserving the folder again, holding the token-bearing body a renewal read, consuming
  the token with the upload's `ClaimTake::Consume`, then rewriting with that stale body: the write
  is refused, the claim stays token- and lease-free, its generation moved past the stale one, and
  the next revision of the entry is accepted without waiting out a lease.
- `a_claim_write_window_serializes_writers_and_is_released_even_when_refused` — a writer that
  cannot take the window answers a refusal and leaves the body byte-identical; the scan sees the
  one claim and not the window; releasing the window lets the same body be written as the next
  generation.
- `deleting_an_entry_does_not_delete_a_claim_under_a_writer` — with the window held, deleting the
  entry leaves its claim; released, the next delete removes it.

Broken-state deltas, measured one at a time and restored:

- accepting any stored generation in `rewrite_claim` fails
  `a_renewal_holding_a_stale_body_cannot_write_a_consumed_token_back` at `a write decided against
  the pre-consume body must not land`;
- bypassing `lock_claim` in `rewrite_claim` fails
  `a_claim_write_window_serializes_writers_and_is_released_even_when_refused` at `a writer that
  cannot take the window must not write`;
- deleting unconditionally in `release_claims_of` fails
  `deleting_an_entry_does_not_delete_a_claim_under_a_writer` at `a claim a writer holds is not
  deleted under it`.

Counts: `papa-bear-master-service` **123 lib + 7 bin + 17 integration**, `papa-bear-cli` **20 + 7**,
`papa-bear-client` **15**, 0 failed (120 + 7 + 17, 20 + 7 and 15 before). `cargo fmt` is clean on
master and CLI; `cargo clippy` exits 0 with the same 32 library warnings as before and none in a new
item; `check-all` is OK and the roadmap regenerates to 60 tickets.

Manual run on Windows with the built binaries, a temp SQLite database and mods root
(`127.0.0.1:18130`, admin key `cas-key`):

```
$ papa publish src-alpha --mod-id alpha --name Alpha --folder-name alpha-dir
published 'Alpha' as alpha, version 7931d405, package revision 1 (30 bytes compressed)
_claims/616c7068612d646972.json = {"modId":"alpha","folder":"alpha-dir","generation":2}

$ POST /v1/catalog/folder-claim {"modId":"alpha"}
200 {"modId":"alpha","folder":"alpha-dir","token":"11a7…c84c","renewAfterMs":200000}
_claims/616c7068612d646972.json = {"modId":"alpha","folder":"alpha-dir",
                                    "token":"ca11…d4dcb","lease_until":1792999276203,
                                    "generation":3}

$ POST …/folder-claim/renew        (lease still fresh)
204; claim byte-identical: True

$ : > _claims/616c7068612d646972.json.writing ; (lease_until := 1)
$ POST …/folder-claim/renew        (a window another writer holds)
409 the reservation for folder 'alpha-dir' changed while it was being renewed; try again
claim unchanged: token, lease_until 1, generation 3

$ sleep 5 ; POST …/folder-claim/renew
204; marker gone; claim = {…, "lease_until":1792999300941, "generation":4}

$ check-names --mods-dir mods      (a .writing file beside a claim)
orphan claim: 'gamma-dir' is reserved by 'gamma', which is not a catalog entry (claimed 0s ago)
Error: check-names: 1 unused claim in 1 catalog entry                          (exit 1)
   one row for the one claim; the window is not a claim

$ (lease_until := 1 on alpha; fresh marker) check-names --fix --mods-dir mods
lapsed reservation: 'alpha-dir' is reserved for 'alpha', whose reservation stopped renewing
    (claimed 0s ago; kept, the claim changed after the scan)                    (exit 1)
$ sleep 5 ; check-names --fix --mods-dir mods
lapsed reservation: … (claimed 5s ago; removed)
check-names: removed 1 unused install-folder claim                              (exit 1: gamma left)

$ papa publish src-alpha --mod-id alpha --name Alpha --folder-name alpha-dir     (revision 2)
updated 'Alpha' as alpha, version 7931d405, package revision 2 (34 bytes compressed)
_claims/616c7068612d646972.json = {"modId":"alpha","folder":"alpha-dir","generation":2}
```

The server log held no `ERROR` line. The renewal/consume interleaving itself is the store test's,
not the manual run's: the server cannot be paused between its read and its write from the outside.
Temp state removed and the server stopped (port 18130 refuses connections).

## Risks

- **A claim write is now four object operations.** Create the window, read the body, write the body,
  delete the window — three more round trips than the bare rewrite, on every reservation, renewal,
  commit and abort. Renewals are one per ~200 s per publisher, so the cost sits on a path that
  already has a lease's worth of slack.
- **A hung master can still land a stale body if nothing else changed.** Its marker expires after
  five seconds and a later writer clears it; the hung writer's body then still matches the
  generation it read, so it writes — with the deadline it computed before the hang. The body is the
  one it read, so only `lease_until` is older, which delays nothing and can only shorten a lease a
  renewal then extends.
- **The window is a lock with a fixed expiry.** A master that dies inside it blocks writers of that
  claim for up to five seconds instead of forever, and `unlock_claim` swallows a refused delete so a
  successful write is never reported as a failure; a marker left behind is cleared by the next
  writer, not by a sweep.
- **The window is measured on the master's clock.** A clock jumping forward can clear a live
  window; the generation still refuses a write decided against a body that changed, so the worst
  case is the lost update this ticket removes for the ordinary path.
- **Mixed masters keep the old window.** An older master ignores `generation` and rewrites bodies
  unconditionally, defeating the compare for writes that land through it. A whole-fleet upgrade (or
  a restart of the one process) is the supported path.
- **Contention answers a refusal, not a queue.** A writer that cannot take the window in ten
  attempts answers 409 (`changed while it was being …; try again`); the residual refusals are
  transient by construction, and the CLI retries a renewal on its next cadence while commit and
  abort stay best effort.
- **`check-names --fix` can report a claim it did not remove.** A claim whose window is held or
  whose body moved is reported `kept, the claim changed after the scan` and still counts as unused
  (exit 1); the next run removes it.
- **Deleting an entry can leave a claim for the sweep.** If a renewal holds the claim's window at
  the moment of the delete, `release_claims_of` leaves it rather than deleting under the writer;
  the folder stays refused until the sweep removes the claim of an entry that no longer exists.
- **The sweep pays for the window.** Every claim it could delete takes a window; claims it keeps
  are not windowed, so a clean sweep does no extra writes.

## Follow-ups

- Sweep stale write windows too, so a marker left by a master that died is removed even when nothing
  writes its claim again. Today it is ignored by every reader and cleared by the next writer.
- Use S3's `PutMode::Update` when the backend is S3, so a claim write is one request there and the
  marker remains the local-backend path only; skipped here because the two code paths would diverge.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo test -p papa-bear-cli
# the window by hand: start the service and reserve a folder, then
#   : > <mods>/_claims/<hex>.json.writing
#   POST /v1/catalog/folder-claim/renew {"modId":"alpha","folder":"@alpha","token":"<t>"}
#   -> 409 changed while it was being renewed; wait 5 s and POST again -> 204, marker gone
```

Expect a claim body to carry `generation`, each accepted write to move it by one, and no `.writing`
object to survive a completed request.
