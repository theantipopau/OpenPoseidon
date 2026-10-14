# G4 — Roadmap and renderer-systems ledger

Branch: `proteus/G4-roadmap-ledger`. Status: done-unverified (defaults need owner spot-checks).

## Problem

No roadmap or renderer inventory existed: 30+ GPU systems with toggles scattered across env
gates, settings structs and dev-panel tabs, and no single file saying what is on by default.

## Approach

- `docs/proteus/renderer-systems.yaml`: 32 systems (name, file, toggle, default, GPU timer
  region, owning ticket), filled by reading `gpu_timers.rs`, the WGSL/Rust sources,
  `EngineWgpu.cpp` gates and `Engine.hpp` defaults. Notable verification: `AoSettings.enabled
  = true` (Engine.hpp:1132) confirms GTAO ships default-on, resolving the stale "Default OFF"
  comment (which describes only the env-override semantics).
- `scripts/proteus/gen-roadmap.py`: generates `docs/proteus/ROADMAP.md` from `ledger.yaml`
  (phase-grouped, ID-sorted); `--check` mode fails when stale.
- `check-all.ps1` now also runs `gen-roadmap.py --check`.

## Files

- `docs/proteus/renderer-systems.yaml` (new), `scripts/proteus/gen-roadmap.py` (new),
  `docs/proteus/ROADMAP.md` (generated), `scripts/proteus/check-all.ps1` (extended).

## Toggle and default

n/a — tooling/docs only. Rows with `default: UNVERIFIED` need tracing as their tickets land.

## Evidence

- `renderer-systems.yaml` parses (32 systems); `gen-roadmap.py` write → `--check` OK round-trip.
- `check-all.ps1` → OK exit 0 (assets 72/72, roadmap current, no changed sources).
- No compiled code touched; `cargo test`/CMake not re-run.

## Risks

- Several defaults marked UNVERIFIED (water backend default, GI, bloom, DoF, godrays, far tier…).
  Wrong defaults mislead E22 preset mapping — each owning ticket must confirm its row.
- Timer-region names mirror `gpu_timers.rs` today; renames there must update the yaml (no automated check).

## Follow-ups

- A2–B12 tickets confirm/UNVERIFY their rows as they land.
- Consider a CI job running `gen-roadmap.py --check` (one line in proteus.yml; left for G3a batch).

## Verify on GPU

n/a. Owner spot-check: open `renderer-systems.yaml`, confirm the GTAO and grass rows match the
dev-panel tabs on their machine.
