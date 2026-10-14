# T19 — Golden-image harness (done-unverified)

## Problem

Perf numbers (T18) catch slowdowns but not wrong pixels. Renderer work in
Phase A/B needs a pixel-level regression gate: fixed shots, captured
deterministically, compared perceptually, with baselines only ever updated by
explicit blessing.

## Approach

No engine change — the game already had everything: `--auto-screenshot
"trigger:path,..."` (frame- or time-based triggers, `GameApplication.cpp`
`ParseAutoScreenshots`), auto-exit 2 frames after the last capture, and PNG
output via `GEngine->Screenshot`.

- `scripts/proteus/shots.yaml` — 6 shots over the T18b dev-missions
  (open/woodland/ride/interior/coast/manual), each with a time trigger
  (20–40s; wall-clock, so load hitches don't shift the shot — what matters is
  streaming has settled), a description, and starting thresholds
  (`max_dissim` 0.02–0.04, `max_color_mae` 3–5). Capture resolution fixed at
  1080p (goldens test pixels, not fill rate). Determinism rests on each
  mission.sqm's fixed `randomSeed` + fixed Intel and a static spawn camera.
- `scripts/proteus/imgdiff.py` — stdlib-only PNG compare (`struct`+`zlib`;
  8-bit non-interlaced RGB/RGBA, all 5 filter types). Metric: mean SSIM over
  8x8 box windows via integral images (O(N)) on luma downscaled to ≤320px
  wide, plus color MAE on downscaled RGB. SSIM (Wang et al. 2004) implemented
  from the published formula — no dependency, no vendored code (recorded in
  PROVENANCE.md; FLIP rejected: needs a display model, no benefit at
  golden-threshold use). `--selftest` builds synthetic PNGs and asserts:
  identical→0, +8 lift→small dissim/mae≈8, 2px gradient shift→tiny,
  noise→destroyed (0.98), size mismatch→`DiffError`, RGBA accepted, codec
  roundtrip lossless. Runs in ~0.3s.
- `scripts/proteus/golden-compare.py` — candidate dir vs baseline dir →
  markdown report (`| shot | result | dissim (max) | color_mae (max) |`),
  per-shot thresholds from shots.yaml. Exits: 0 all-PASS · 1 any-FAIL ·
  5 missing baseline/candidate (names the path, says to run `-Bless`; never
  writes placeholders, never silently passes).
- `scripts/proteus/golden-capture.ps1` — `-Res 1080p -Shots open,coast
  [-Backend dx12] [-Bless]`. Mirrors `Start.ps1` mod wiring, runs
  `--test-mission <mission> --auto-screenshot "trigger:out.png" --no-dev`
  from the exe dir, completion ground truth = PNG exists (same lesson as
  T18b: process exit code lies on the fast-shutdown path). Exits mirror
  perf-capture: 2 no exe · 3 no data/unknown shot · 4 timeout/missing PNG.
  **Baselines live outside the repo** (`$env:PROTEUS_GOLDENS`, else
  `%LOCALAPPDATA%/Proteus/goldens/<res>`) — no LFS, nothing to commit — and
  only `-Bless` writes there.
- Crash watch: the game writes minidumps to `<Documents>/Cold War
  Assault/crashes/` (GamePaths UserContentDir — NOT Temp, found while
  answering a crash-log question this ticket). The script snapshots
  `crash-*.dmp` before the first shot and warns with the dump path if any
  appear, since a crash can hide behind a successful-looking exit.
- `imgdiff.py --selftest` wired into CI (assets job) and `check-all.ps1`.

## Files

| File | Change |
|---|---|
| `scripts/proteus/shots.yaml` | NEW — 6 shots, triggers, starting thresholds |
| `scripts/proteus/imgdiff.py` | NEW — PNG codec + SSIM/MAE + selftest |
| `scripts/proteus/golden-compare.py` | NEW — compare + markdown report, exits 0/1/2/5 |
| `scripts/proteus/golden-capture.ps1` | NEW — driver + `-Bless` + crash watch |
| `docs/proteus/PROVENANCE.md` | SSIM-from-scratch algorithm note |
| `.github/workflows/proteus.yml`, `scripts/proteus/check-all.ps1` | run imgdiff selftest |

## Toggle and default

No engine toggle. Defaults: 1080p, all shots, vulkan, candidate-only (no
bless without the switch).

## Evidence

- `imgdiff.py --selftest`: PASS — `identical=0 lift=0.0040 shift=0.00010
  noise=0.982` (0.3s). Thresholds sit 5–10× above gradient-shift noise and
  25× below full-noise destruction.
- `golden-compare.py`: PASS path exit 0 (`1/1 PASS`), FAIL path exit 1 with
  markdown report (`coast` noise: dissim 0.98260 vs max 0.04), missing
  baseline exit 5 — all on synthetic PNGs.
- **Live engine shot**: `golden-capture.ps1 -Shots open` ran the real game
  (RX 9070 XT, vulkan, 1920×1080) and wrote a 1.9MB `open.png`; `imgdiff`
  reads it natively (`1920x1080`, self-compare 0.00000). Capture path proven;
  no bless performed (baselines are Matt's call).
- Two script bugs found by running, fixed, verified: PS case-insensitivity
  (`$shots` table clobbered the `-Shots` param — renamed `$shotList`;
  perf-capture is immune: `$Scene` vs `$scenes` differ) and
  `-RedirectStandardInput NUL` (PS 5.1 resolves NUL to a missing file →
  FileNotFoundException; same launch shape as proven perf-capture instead).

## Risks

- Thresholds are starting points from synthetic separation, not from real
  frame-to-frame variance — first bless + a same-day re-compare must confirm
  or tighten them (coast water sim is the likeliest flaker; it got the
  loosest thresholds + longest trigger).
- Time triggers assume streaming settles by 20–40s on Matt's GPU; slower
  disks/GPUs may need higher triggers — raise, don't bless half-loaded.
- Crash watch keys off `MyDocuments` redirection; override via
  `POSEIDON_USER_CONTENT_DIR` setups is not detected (documented follow-up).

## Follow-ups

- First bless + threshold confirmation pass (Matt, on GPU).
- Night + rain shot variants (copied mission dirs, same pattern as T18b note).
- Crash-watch honouring `POSEIDON_USER_CONTENT_DIR` override.
- Oct 14 crash cluster (10 dumps, 12:36–12:42): still unexplained, needs
  windbg/VS + PDBs and what Matt was running — see RUN_LOG question.

## Verify on GPU (for Matt)

```powershell
# Capture all 6 candidates (~5 min incl. loads)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/golden-capture.ps1
# Inspect the PNGs, then bless ONLY if they look right
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/golden-capture.ps1 -Bless
# Next-day / post-change re-compare
python scripts/proteus/golden-compare.py --shots scripts/proteus/shots.yaml `
  --candidate <new-cand-dir> --baseline $env:LOCALAPPDATA/Proteus/goldens/1080p --report /tmp/gold.md
```

- **Good:** all 6 PNGs written, no crash warning, report `6/6 PASS` on
  immediate re-compare (determinism smoke test).
- **Bad:** exit 4 + no PNG (read the game log tail), crash-dir warning
  (see `crashes/crash-*.dmp` + tell me what you were running), FAIL on
  re-compare with no code change (streaming flake → raise trigger).
