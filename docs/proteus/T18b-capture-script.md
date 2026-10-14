# T18b — perf capture script + scene definitions (done-unverified)

Follow-up to T18a (PERF-CAP line + `perf_parse.py`). This ticket builds the
capture side: a scene table over in-repo dev-missions, a PowerShell driver that
runs the game headless, and a Python log→CSV/summary stage.

## Problem

T18a defined the engine's `[wgr] perf` line format and the CSV/summary parser,
but there was no way to run the game deterministically and produce a
timestamped capture. Manual runs differ in camera path, resolution and
duration, so results cannot be compared across commits or toggles.

## Approach

- `scripts/proteus/scenes.yaml` maps 6 scene names to in-repo dev-missions on
  stock terrains: `open` (terrain-brush.Intro), `woodland` (snow-crawl.Intro),
  `ride` (jeep-ride.Intro), `interior` (smoke-room.noe), `coast`
  (foam-ocean.noe, adds Tw* ocean regions), `manual` (jeep-manual.Intro,
  second Intro viewpoint for A/B sanity). Determinism comes from each
  mission.sqm's fixed `randomSeed` + fixed Intel, with a static camera —
  movement would add run-to-run variance the summary cannot separate from real
  change. Night/rain variants: copy the mission dir and set Intel
  hour/startWeather (do NOT edit these files in place).
- `scripts/proteus/perf-capture.ps1` is the single-command entry:
  `perf-capture.ps1 -Res 1440p -Scene coast [-Iters 3] [-Ab 'WGR_GTAO_SCALE=1,WGR_GTAO_SCALE=2'] [-Backend dx12]`.
  It mirrors `Start.ps1` mod wiring (`mods.json` data_path/mods_dir/mods),
  runs `dist/x64-win-rwdi/ProteusGame.exe --window --width/--height
  --test-mission <mission> --benchmark --no-dev --log-file` from the exe dir,
  with `POSEIDON_GPU_STATS=1` (+ `WGPU_BACKEND` for dx12). `--benchmark` is
  300 frames then self-exits; PERF-CAP emits one 120-frame window line per run,
  so use `-Iters 5+` for meaningful p95/p99 (percentiles are across
  iterations, not frames). `-Ab 'K=V,...'` runs one config arm per entry
  (env-var toggles, sanitised into log names); completion ground truth is
  `BENCHMARK RESULT` in the log, not the process exit code (the benchmark
  fast-shutdown path can leave ExitCode unreadable). PS 5.1 pitfalls
  documented inline: Start-Process array args split on spaces (paths quoted),
  Wait-Process has no -PassThru (use .NET WaitForExit), New-TemporaryFile
  returns FileInfo. Sidecar JSON carries date/commit/res/scene/backend/GPU
  (via Win32_VideoController)/exe/mission/env/ab-arms/iters/summary.
- `scripts/proteus/perf_run.py` is the log→CSV stage: `--csv/--res/--scene/
  --backend` plus paired `--run LABEL --log PATH` lists (parallel lists, never
  joined on a separator since both halves may contain `=`), one row per log,
  `PERF-SUMMARY` JSON on stdout for the sidecar.
- `scripts/proteus/gen-regions.py` regenerates `gpu_regions.py`, the timer
  region index→name map parsed from EngineWgpu.cpp `kNames` (WgrGpuTimerRegion
  order, append-only) for parser display names.
- Exit codes: 0 ok · 2 no built exe · 3 no game data (`mods.json`/data_path/
  unknown scene/missing mission) · 4 launch/timeout/no-BENCHMARK/parse-fail.
  All print a one-line cause, never crash.
- `perf_parse.py --selftest` wired into CI (assets job) and local
  `check-all.ps1` (this ticket's diff).

## Files

| File | Change |
|---|---|
| `scripts/proteus/scenes.yaml` | NEW — 6 scenes → dev-mission dirs + what each stresses |
| `scripts/proteus/perf-capture.ps1` | NEW — single-command driver (incl. BOM-less sidecar fix, below) |
| `scripts/proteus/perf_run.py` | NEW — logs → CSV + PERF-SUMMARY |
| `scripts/proteus/gen-regions.py` + `gpu_regions.py` | NEW — timer-name map generator + output |
| `scripts/proteus/perf_parse.py` | selftest wired into CI + check-all (no logic change) |
| `docs/proteus/perf/20261015-*.csv/.json` | NEW — first real captures (evidence, RX 9070 XT) |
| `.github/workflows/proteus.yml`, `scripts/proteus/check-all.ps1` | run perf selftest |

## Toggle and default

No engine toggle. Defaults: 1440p, 1 iter, vulkan backend, `open` scene. The
engine `--benchmark`/`POSEIDON_GPU_STATS` path (T18a) is unchanged.

## Evidence

- `perf_parse.py --selftest`: PASS (CI + local).
- Two live captures on **AMD Radeon RX 9070 XT / vulkan / 2560x1440**, commit
  `0ed1054`: `20261015-092741-0ed1054-1440p-open` (1 iter, `bench_fps 133.3`,
  300 frames / 2.2 s, 188-column row incl. per-region `gpu_*`/`cpu_*`) and
  `20261015-0ed1054-1440p-open-gtaoAB` (header + 2 A/B rows). Real engine
  output, not synthetic.
- Launch pitfalls found against the real game and fixed: child CWD must be the
  exe dir (shipped assets), stdin-not-a-console prompt-and-exit code 3 fixed
  with `<NUL`-equivalent detachment, space-in-path quoting, ExitCode-vs-log
  ground truth (RUN_LOG 07:47–09:44 debug matrix).
- Sidecar bug found while reading our own output: `Out-File -Encoding utf8`
  writes a BOM that Python's `json.load` rejects. Fixed with
  `[IO.File]::WriteAllText(..., [Text.UTF8Encoding]::new($false))`; probe
  shows `7B 22...` first bytes and Python parses it. (Committed sidecars from
  the verification run still carry the BOM — re-run to regenerate.)
- `check-all.ps1`: asset/roadmap/perf-selftest gates green; FAIL only on
  pre-existing `EngineWgpu.cpp` clang-format drift (not this ticket's hunks).

## Risks

- Single-iteration captures give p50==p95==p99; the record and script header
  both say to use `-Iters 5+` for real percentile comparisons.
- Sidecar `ab` field serialises as a scalar when there is one arm (PS 5.1
  `ConvertTo-Json` single-element-array quirk) — cosmetic; parsers should
  accept string-or-list.
- Absolute game/data paths live in `mods.json` (untracked local config), never
  in the repo.

## Follow-ups

- T19 golden-image harness reuses scenes.yaml missions as its shot list.
- Night + rain scene variants (copied mission dirs with Intel overrides).
- `docs/proteus/perf/` will accumulate dated captures — decide retention
  (git-annex/LFS or out-of-repo) before it grows.

## Verify on GPU (for Matt)

```powershell
# Single capture
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/perf-capture.ps1 -Res 1440p -Scene open
# A/B toggle comparison, 3 iters each arm
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/perf-capture.ps1 -Res 1440p -Scene coast -Iters 3 -Ab 'WGR_GTAO_SCALE=1,WGR_GTAO_SCALE=2'
```

- **Good:** `docs/proteus/perf/<stamp>-<commit>-<res>-<scene>.csv` + BOM-less
  `.json` sidecar with your GPU name; `bench_frames` 300, per-region `gpu_*`
  columns non-zero.
- **Bad:** exit 2 (build first), exit 3 (check `mods.json` data_path),
  exit 4 (game exited without `BENCHMARK RESULT` — read the tail of the
  `perf-<scene>-<res>-<stamp>-*.log` in TEMP).
