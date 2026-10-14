# Proteus background changelog (newest last)

- 2026-10-14 Step 0: added `docs/proteus/STATE_OF_REPO.md` (build/default-on-vs-opt-in/open-items/divergence baseline, VERIFIED vs ASSUMED).
- 2026-10-15 G3 (`proteus/G3-ci`): `.github/workflows/proteus.yml` (5 jobs: assets, fmt-changed, rust-tests on Lavapipe, linux + windows builds) + `check-all.ps1` parity with negative controls.
- 2026-10-15 G2 (`proteus/G2-asset-registry`): `assets/registry.yaml` (72 files) + strict `check-assets.py` + `check-all.ps1` + `PROVENANCE.md`.
- 2026-10-15 G1 (`proteus/G1-agent-guide`): new root `AGENTS.md` agent guide + one-line `CLAUDE.md` pointer.
- 2026-10-14 A1 (`proteus/ao-half-res`): half-res GTAO already implemented default-on — pinned with `gtao_half_res_march_stays_pinned_to_the_bilateral_upsample_contract` (8/8 GTAO CPU tests pass); 1080p/1440p/4K timings deferred to ticket 18's capture script. See `docs/proteus/A1-gtao-half-res.md`.
