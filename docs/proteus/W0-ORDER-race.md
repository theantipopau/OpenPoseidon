# W0-ORDER — test mission never boots; captures are menu/wrong-mission (in-progress)

## Status

Root cause found with log evidence (below). Fix + guards + proof pending.

## Root cause (evidence-backed, 2026-10-15)

The "retained-admission race" is **not an engine ordering bug**. The test
mission never boots in ANY run; captures are the menu cutscene, and one run
caught real UI input launching the wrong mission.

1. `StageTestMissionForGame` (`apps/cwr/Game/GameApplication.cpp:761`)
   drops the mission directory for file inputs: `dev-missions/
   terrain-brush.Intro/mission.sqm` stages as `.../mission-smoke/<rand>/
   Missions/mission.sqm` — the `Missions/<name>/` level its own caller
   comment (line 650) promises is lost (`srcPath.filename()` keeps only
   `mission.sqm`; the directory-input and .pbo arms are unaffected).
2. The bare path resolves (the file exists) so the exit-44 guard passes, but
   `ProcessFullName` never boots it: **zero** `Mission start phase`
   (`StartAutoTest`) lines appear in any capture log, populated or empty.
   The game falls through to `StartRandomCutscene` — the GModeIntro menu
   background world. No error is logged on this path.
3. Menu world ⇒ correctly 0 objects offered/admitted ⇒ deterministic
   backdrop ⇒ byte-identical `f191f13a` across runs. The engine's
   "ordering bug, not a filter" warning fires whenever no world is loaded,
   including the menu — it is a red herring here, not the diagnosis.
4. The "populated" run (`e248…`, 178,487 offered) played the operator's
   campaign mission (`DIAG single PLAY: IDC_OK … file=RHII SinglePlayer\
   02MirovAflame`), confirmed through the real `DisplaySingleMission::
   OnButtonClicked` handler. No auto-confirm path exists in code (the two
   `AutoTest` hits in UI code are quit-flow and workshop-fetch skip), so
   this was live UI input while the window had focus — not a race either.

## Collateral

- T18b perf baselines (e.g. 133 fps open/1440p) and the T19 "live open.png"
  were captured the same way: most likely **menu-cutscene fps**, and the
  one "real scene" was the wrong mission. Both need re-baselining after
  the fix (this ticket's proof runs do it).
- `f191f13a` = GModeIntro menu backdrop on this machine. It becomes the
  denylist tripwire, not a golden.

## Fix (implemented, in branch)

- Engine (`apps/cwr/Game/GameApplication.cpp`, no toggle — pure correctness):
  file inputs now stage as `Missions/<parent-dir-name>/mission.sqm`
  (directory and .pbo arms unchanged). First run after rebuild:
  `.../Missions/terrain-brush.Intro/mission.sqm`, `ParseMission 1 group`,
  `SwitchLandscape world 'intro'`, `StartAutoTest total 462 ms` — the
  mission boots. terrain-brush offers 21 objects, all refused `not-static`
  (1 dynamic group, correct: dynamics stay on the CPU path), so its
  `min_admitted` is 0 and the boot-proof is its gate.
- `scripts/proteus/check-capture.py` (new, selftest PASS): shared judge —
  exit 0 VALID / 1 INVALID with reasons `INPUT_DIALOG` (went through the
  SP dialog), `EMPTY_KNOWN` (f191f13a menu-backdrop denylist),
  `NO_BOOT` (no StartAutoTest), `NO_CENSUS`, `ADMITTED_LOW`. Verified
  against the real W0a logs: menu run → `INVALID EMPTY_KNOWN`,
  campaign-contaminated run → `INVALID INPUT_DIALOG`.
- `golden-capture.ps1`: per-shot 3 attempts, exit 6 when still INVALID,
  `-Bless` only reachable with all shots VALID. `perf-capture.ps1`: same
  per-iter retries; persistent INVALID logs move to `perf/rejected/` and
  are excluded from the summary; zero-valid config exits 6; sidecar
  records valid/rejected counts. Exits documented in both headers.
- Thresholds from first good runs (`shots.yaml` + `scenes.yaml`):
  open/woodland/ride/manual `min_admitted: 0` (Intro missions have no
  statics — woodland offered 14, ride 153, manual 238, all not-static),
  interior/coast `min_admitted: 150000` (both admit 173,540 of 178,263 —
  shared noe-island statics; PNGs verified pairwise-distinct).
- Readiness trigger deferred: boot-proof + admitted + 20–40 s settle
  triggers suffice if the 10× proof is clean; revisit only on evidence.

## Proof (done 2026-10-15, RX 9070 XT)

- **10× back-to-back per shot: 58/60 VALID on first attempt.** open,
  woodland, interior, coast, manual 10/10; ride 8/10 + 2 `exit 4`
  (game quit cleanly ~20 s after launch with no PNG — a launch-flake
  class distinct from INVALID content; ride then went 4/4 VALID extra for
  12/12). Offered counts vary run to run on dynamic missions (ride 38–154,
  manual 221–238: vehicle/proxy churn) while admitted stays 0 by design;
  .noe missions admit a rock-steady 173,540/178,263 every run.
- The 2 ride flakes motivated a harness change (in-branch): clean-exit
  missing-PNG/BENCHMARK now retries within the 3 attempts instead of
  exiting 4 immediately (genuine timeouts stay fatal). Same for
  perf-capture (missing BENCHMARK RESULT).
- **Populated Native-TAA off/on A/B at 1440p** (Native mode, scale 1.0):
  open dissim 1.4e-06 / mae 0.005, woodland 3.4e-06 / 0.036 — static
  scenes converge exactly as theory predicts. ride (motion) dissim 0.248 /
  mae 6.5 — dominated by wall-clock vehicle timing across runs, so it
  bounds but does not isolate the TAA effect; motion-isolated proof (fixed
  camera + moving subjects) is a W0c follow-up. Verdict: PASS → W0a
  `gpu_verified: true`.
- **T19 bless**: 6/6 VALID blessed to `%LOCALAPPDATA%/Proteus/goldens/1080p`.
- **3× re-compare calibration + 1**: cal-1/2: 4/6 PASS (ride/manual stable
  ~0.25 — wall-clock vehicle phase); cal-3: 1/6 (flake cluster —
  woodland 0.359, interior 0.542, coast 0.117 with identical admitted
  counts and settled streaming: sim-phase variance, quadrant-localised to
  smoke/water/bottom-frame content); cal-4 rerun: all three back in
  bounds → flakes, not drift. Thresholds set from 5 samples per shot
  (`shots.yaml`): open 0.02/3.0, woodland 0.40/30, ride 0.35/10, interior
  0.60/40, coast 0.15/15, manual 0.35/10. Teeth verified: menu-backdrop
  f191 vs every baseline = dissim 0.78–0.87, mae ~100 — 2–80× above the
  loosest threshold, so the gates still catch the failure that matters.
  Tighter thresholds (esp. interior) await sim-determinism work.

## Follow-ups

- Sim-phase determinism (fixed-seed smoke/weapon-bob/water vs wall-clock
  load variance) to tighten interior/woodland/coast thresholds.
- Frame-since-mission-entry triggers as an alternative to wall-clock.
- Motion-isolated TAA proof (W0c ghosting shots).
