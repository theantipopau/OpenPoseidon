# T18a — Machine-readable GPU/CPU timing line (PERF-CAP)

Branch: `proteus/T18-perf-capture`. Status: done-unverified (build green; needs GPU run).

## Problem

T18's capture script needs per-pass GPU ms columns, but the log only carried gated
summaries (local-shadow means, wake means, WGR_FRAME_STATS water split) plus the CPU
`frame avg/p95` line. No full-region dump existed for a harness to parse.

## Approach

One gated block in the existing per-frame stats publisher (`EngineWgpu.cpp`, next to the
`kSayGpu`/`kSayWake` blocks, reusing their `gpu[]`/`cpu[]` arrays and -1-means-zero rule):
`POSEIDON_GPU_STATS=1` emits 120-frame means of every published region. Default off —
unset variable restores the previous log byte-for-byte.

## Files

- `engine/WgpuRenderer/EngineWgpu.cpp` (+38): `kSayPerfCap` block.

## Toggle and default

- `POSEIDON_GPU_STATS=1` on; unset/0 = off (default). Off-switch = previous log.

## Line format (contract for T18b's parser)

```
PERF-CAP gpu120 g0=0.123 g1=0.000 ... g88=1.234 | cpu120 c0=0.011 ... c88=0.000
```

- Indices are `WgrGpuTimerRegion` (append-only; 89 today). `-1` (pass absent) counts as 0,
  matching the existing mean blocks. Values are 120-frame means, 3 decimals.
- One line per 120 gameplay frames; a 300-frame `--benchmark` run yields 2 lines —
  T18b takes the LAST complete window. Repeated `--iters` runs give percentile samples.

## Evidence

- `cmake --build build/win-x64-clang-rwdi --target WgpuRenderer` → green (only pre-existing
  warning at EngineWgpu.cpp:17876, untouched by this hunk).
- `cmake --build build/win-x64-clang-rwdi --target PoseidonGame` → green,
  `ProteusGame.exe` linked and staged to `dist/x64-win-rwdi`.
- No unit test: 10-line loop over already-published arrays, same pattern as the two adjacent
  blocks; verified by build + review. Parser-side self-tests land in T18b against synthetic lines.
- clang-format: hunk follows surrounding style (4-space, Allman braces); verified by eye —
  `check-all` C++ arm covers it once committed (no C++ drift in-tree to collide with).

## Risks

- Log-volume negligible (one ~2.5 KB line per 120 frames). No behaviour change when off.
- If region count grows, parsers must tolerate unknown trailing indices (T18b handles by index).

## Follow-ups

- T18b: `perf-capture` script + scenes + CSV/summary parser + `--ab` mode.
- T20 overlay can reuse the same `gpu[]`/`cpu[]` publish path.

## Verify on GPU

1. `POSEIDON_GPU_STATS=1 scripts/Start.ps1 --window` (or with `--benchmark`), play >120 frames.
2. Good: `PERF-CAP gpu120 g... | cpu120 c...` lines in the log every ~120 frames, `g25` (frame
   total) > 0. Bad: no lines (flag not picked up — env must reach the game process) or all zeros
   (timers unsupported on that backend — compare with dev-panel Objects tab).
