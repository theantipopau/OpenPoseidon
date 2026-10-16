# SHUTDOWN-T-1 — SHUTDOWN-TIME phase probes in UnloadGameData + live proof

## Symptom (audit item: shutdown-time work)

Quit logs show 30–70 s windows on the last presented frame with no breakdown of
where the time goes, so the shutdown work could not start.

## Fix (`engine/Poseidon/Foundation/Platform/Shutdown.cpp`)

`UnloadGameData` now logs a `SHUTDOWN-TIME phase '<name>' took <ms>` INFO line per
phase via a behaviour-neutral RAII timer (`ShutdownPhaseTimer`): `render-stop`,
`delete-world`, `mounts-caches` (incl. `Glob.Clear()` → PBO bank teardown),
`sound`, `configs-banks`, `destroy-engine`, `pars-res-network`, plus an
`unload-game-data-total` line. Teardown order and semantics are unchanged (new
scope braces only); the format hunk was cleaned with the VS-bundled clang-format.

## Live proof (RX 9070 XT, `--benchmark` 1080p/open, self-exiting run)

Fresh `ProteusGame.exe` built from these sources booted, ran 300 frames and shut
down cleanly (capture exit 0; CSV + sidecar in `docs/proteus/perf/`):

- `delete-world 10 ms`, `mounts-caches 0 ms`, `sound 2 ms`,
  `configs-banks 125 ms`, `destroy-engine 149 ms`, `pars-res-network 0 ms`,
  `unload-game-data-total 289 ms`.

Caveat: the benchmark self-exit path is fast — the 30–70 s menu-quit windows come
from a different path, still to be captured with these probes on a real
launch → menu → quit run (ideally with `@ffsx2007` loaded, which also visually
confirms MENU-FFUR-1 and UI-THEME-A).

## Verification

- `Poseidon` lib + `PoseidonGame` link green; `check-all` OK (format clean).
- Side observation (not changed here): the run was judged VALID with
  `admitted=0 (offered=13)` — T18b numbers need the admitted-count gate before
  they count as populated baselines (see ticket-4 proposal).

## Follow-ups

- Real quit-flow log with the probes → actual slow-phase fix.
- `gpu_verified` stays false (no menu/quit screenshot headless).
