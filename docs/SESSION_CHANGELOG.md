# Session changelog (agents)

Cross-day, cross-agent record of what has been done in this repo's sessions, newest
first. So the next day/agent can see where things stand at a glance.

Detail lives elsewhere, and this file points at it rather than repeating it:

- `docs/CHANGELOG.md` — feature-level log of fork changes (UI, renderer, build).
- `docs/proteus/RUN_LOG.md` — minute-by-minute log of the Proteus workstream runs.
- `docs/proteus/ledger.yaml` — ticket status (`proposed → in-progress → done-unverified
  → verified`, plus `gpu_verified`), files, toggles, evidence, follow-ups.
- `docs/proteus/ROADMAP.md` — generated from `renderer-systems.yaml` (`gen-roadmap.py`).
- Per-ticket records in `docs/proteus/<ticket>.md` are the PR descriptions.

## 2026-10-15 — Proteus phases G/T/W + W0-ORDER proof (branch `proteus/main`, `fdc58b9`..`e0e9808`, 21 commits)

One day running the whole background queue, then proving the capture stack end-to-end:

- **G1–G4** tooling/docs: `AGENTS.md` agent guide (+ `CLAUDE.md` pointer); asset
  registry `assets/registry.yaml` (72 rows) + strict `check-assets.py`; CI workflow
  `.github/workflows/proteus.yml` (5 jobs) with local parity `check-all.ps1`;
  `renderer-systems.yaml` (32 systems) + generated `ROADMAP.md`.
- **T18a** `POSEIDON_GPU_STATS=1` emits `PERF-CAP` lines (89 GPU + CPU timer regions,
  120-frame means, default off). **T18b** `perf-capture.ps1` + `scenes.yaml` (6 scenes
  over dev-missions) + `perf_run.py`/`perf_parse.py`; first live RX 9070 XT captures
  (133 fps open/1440p); sidecar-BOM bug found and fixed.
- **T19** golden-image harness: `shots.yaml` (6 shots + time triggers), `imgdiff.py`
  (stdlib PNG + SSIM/MAE), `golden-compare.py` (report, exits 0/1/5),
  `golden-capture.ps1` — baselines live OUTSIDE the repo, only `-Bless` writes them;
  crash-dir watch on `crash-*.dmp`.
- **W0a** upscaler interface + Native TAA backend (`WGR_NATIVE_TAA=1` +
  `upscalerQuality=100`), motion-vector audit wiring, hunk-scoped `check-format.py`
  (the whole-tree format gate was unpassable on drift it never touched).
  **`gpu_verified: true`** — populated TAA off/on A/B at 1440p PASS (open 1.4e-06,
  woodland 3.4e-06 = correct static convergence; ride 0.248 = vehicle-timing-bound,
  motion-isolated proof deferred to W0c).
- **W0-ORDER** (filed as a retained-admission race): root cause was **the test mission
  never booting** — `StageTestMissionForGame` dropped the mission directory for file
  inputs (`Missions/mission.sqm`), so `ProcessFullName` silently failed, zero
  `StartAutoTest` lines appeared, and every capture rendered the menu cutscene (f191,
  0 objects). The engine's "ordering bug" warning fires with no world loaded — red
  herring; the one "populated" run was live UI input into a real campaign dialog.
  Fixed staging (`Missions/<name>/`, no toggle, pure correctness) and added
  `check-capture.py` judge (`INPUT_DIALOG`/`EMPTY_KNOWN`/`NO_BOOT`/`NO_CENSUS`/
  `ADMITTED_LOW`) wired into both capture scripts: 3 attempts, exit 6, bless refusal,
  `perf/rejected/` quarantine. **Proof:** 10× back-to-back per shot = 58/60 VALID
  first attempt (ride 12/12 after 2 clean pre-trigger early quits). T19 blessed 6/6,
  then 3× calibration + rerun set per-shot thresholds in `shots.yaml` (from 5 samples
  each: open 0.02/3 … interior 0.60/40); menu backdrop scores 0.78–0.87 against every
  baseline, so all gates have teeth. **`gpu_verified: true`**.
- Verification: `check-all: OK` on merged `proteus/main`; both capture scripts
  PSParser-clean; a live perf iter through `perf-capture.ps1` wrote a VALID CSV +
  sidecar (that run caught a null-`$outDir` crash in the new rejected/ wiring — fixed
  as `e0e9808` and re-verified). `git status` clean apart from the artifacts below.
- Collateral: **T18b fps baselines are menu-era and stale — re-baseline needed.**
- This session's tail: created `docs/SESSION_CHANGELOG.md`, committed the 8 pending
  perf CSV/JSON artifacts from the verification runs, pushed `proteus/main` + the
  ticket branches + `dev/matt-fork-main` to origin (first push of the Proteus
  workstream; previously only up to `5efc6a9` was on GitHub).

## 2026-10-14 — Step 0 baseline + renderer session (branch `dev/matt-fork-main`)

- **Step 0**: `docs/proteus/STATE_OF_REPO.md` (build / default-on-vs-opt-in / open
  items / divergence, claims marked VERIFIED vs ASSUMED) and
  `docs/proteus/CHANGELOG_BACKGROUND.md` — the baseline every later ticket reads.
- **`5efc6a9`** renderer session: DX12 fixed on AMD, FSR 4.1.1 running, flat
  debriefing and editor UI (see `docs/CHANGELOG.md` for the FSR 4 entry and its
  open risks: FFX resource-state contract, motion-vector scale sign).
- **A1** half-res GTAO was already implemented default-on; pinned with
  `gtao_half_res_march_stays_pinned_to_the_bilateral_upsample_contract` (8/8 GTAO
  CPU tests) — commit `fdc58b9` landed at the head of the 10-15 run.
- Open question carried over: **crash cluster of Oct 14, 12:36–12:42** — 10 minidumps
  in 6 minutes under `<Documents>/Cold War Assault/crashes/`, strings-triage
  inconclusive (overlay DLLs only); needs windbg/VS + matching PDBs.

## 2026-10-11/12 — Fork bootstrap + Proteus UI overhaul (branch `dev/matt-fork-main`)

- Fork bootstrap: fork docs (`FORK_ROADMAP.md`, `ASSET_PIPELINE.md`, `KNOWN_ISSUES.md`,
  `CHANGELOG.md`), OFP Evos asset audit (341 PBOs, nothing copied in), `mods.json`
  launcher wiring + anchored gitignore, `win-x64-clang-rwdi` preset green, launch
  probe (blocked on missing Remastered fonts).
- Main menu overhaul, one commit per fix: window title `Proteus [WGPU]`, full-bleed
  key art + vignette, glass sidebar, Barlow Condensed (SIL OFL, committed), cyan
  accent buttons, logo + version footer; shared `ProteusTheme` design system then
  Options / Campaign Select / Single Missions restyled on it; Proteus loading screen
  and title effects; video + display settings pages; Proteus icon; `FindPicture`
  absolute paths and mod-font acceptance in the engine.
- 10-12: 3D draws in the post-tonemap UI phase (menu/world compositing), mission
  editor, loading screens and UI content updates.

## 2026-10-07/08 — Upstream base (context, not our sessions)

- `250374d` Open Poseidon Engine source release (koosoli/OpenPoseidon) + README/docs
  clean-up through `cc8b9f4`. This fork's work starts at `3675360`.

## Open threads (carry between sessions)

1. **Re-baseline T18b fps numbers** — the tracked 09:27 capture is menu-era.
2. **Tighten the interior golden threshold** (0.60 has the thinnest margin over the
   f191 floor of 0.84) — needs sim-phase determinism work (W0c territory).
3. **Ride's 2 pre-trigger early quits** (clean `Shutdown: begin`, no end lines) —
   retry-contained; investigate if it recurs (candidate: AutoTest quit racing the 20s
   trigger).
4. **cargo: 4 pre-existing failures + full-run segfault** (stale ABI-21-vs-22
   constant, aerial-perspective role test, 2 GI real-device tests) — reproduce
   identical on clean HEAD; assigned to owning tickets, ABI constant NOT bumped blind.
5. **CI workflow never observed green on a runner** — owner's first run flips it;
   fix fallout as G3a.
6. **perf/ retention + gitignore housekeeping** — 8 capture artifacts committed as
   evidence this session; decide the steady-state rule (and `Aspect_Ratio.hpp`,
   still untracked at repo root, owner's call).
7. **Oct 14 crash cluster** — unexplained; windbg needed.
8. **W0-ORDER residue**: re-offer existing objects when the renderer path comes live
   (or register the listener before placement).
9. **Order queue**: TEST-001 → housekeeping → W1 audit → W0b (per-file PROVENANCE
   record first if it ports a pass) → W0c (Spatial + Quality/Balanced/Performance +
   T18/T19 verify at 1440p/4K).
