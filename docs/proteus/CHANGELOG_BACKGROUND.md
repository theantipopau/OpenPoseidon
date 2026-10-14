# Proteus background changelog (newest last)

- 2026-10-14 Step 0: added `docs/proteus/STATE_OF_REPO.md` (build/default-on-vs-opt-in/open-items/divergence baseline, VERIFIED vs ASSUMED).
- 2026-10-15 T18a (`proteus/T18-perf-capture`): `POSEIDON_GPU_STATS=1` emits 120-frame means of all 89 GPU + CPU timer regions as `PERF-CAP` log lines (WgpuRenderer + PoseidonGame build green).
- 2026-10-15 G4 (`proteus/G4-roadmap-ledger`): `renderer-systems.yaml` (32 systems from code reading) + `gen-roadmap.py` generating `ROADMAP.md` + `--check` wired into `check-all`.
- 2026-10-15 G3 (`proteus/G3-ci`): `.github/workflows/proteus.yml` (5 jobs: assets, fmt-changed, rust-tests on Lavapipe, linux + windows builds) + `check-all.ps1` parity with negative controls.
- 2026-10-15 G2 (`proteus/G2-asset-registry`): `assets/registry.yaml` (72 files) + strict `check-assets.py` + `check-all.ps1` + `PROVENANCE.md`.
- 2026-10-15 G1 (`proteus/G1-agent-guide`): new root `AGENTS.md` agent guide + one-line `CLAUDE.md` pointer.
- 2026-10-15 T19 (`proteus/T19-golden-harness`): golden harness — shots.yaml (6 shots/time-triggers/thresholds) + imgdiff.py (stdlib PNG + SSIM/MAE + selftest) + golden-compare.py (markdown report, exits 0/1/5) + golden-capture.ps1 (-Bless writes outside repo, crash-dir watch); live engine open.png verified readable; selftest in CI + check-all.
- 2026-10-15 T18b (`proteus/T18b-capture-script`): `perf-capture.ps1` single-command capture (6 scenes over dev-missions, --benchmark 300f, -Ab env-toggle arms, exits 2/3/4) + `perf_run.py` logs-to-CSV + `gen-regions.py` timer-name map; first real captures on RX 9070 XT (133fps open/1440p); perf selftest wired into CI + check-all.
- 2026-10-14 A1 (`proteus/ao-half-res`): half-res GTAO already implemented default-on — pinned with `gtao_half_res_march_stays_pinned_to_the_bilateral_upsample_contract` (8/8 GTAO CPU tests pass); 1080p/1440p/4K timings deferred to ticket 18's capture script. See `docs/proteus/A1-gtao-half-res.md`.
