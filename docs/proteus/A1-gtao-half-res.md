# A1 — Half-resolution GTAO with bilateral upsample (LIT-010 lever)

Branch: `proteus/ao-half-res`. Status: code-complete, measurements pending GPU run.

## Problem

The brief asked for half-resolution GTAO with bilateral upsample, measured at
1080p/1440p/4K. Evidence check showed the implementation **already exists** and is
default-on: `cs_gtao` marches one invocation per 2x2 block (`WGR_GTAO_SCALE`, default 2),
picks the nearest surface in the block as representative, replicates over a full-res
`Rgba16Float` target, and the full-res bilateral blur (depth + normal edge-aware) acts as
the joint upsample. The shader header documents why a separate half-res target plus bespoke
upsample was rejected (little extra bandwidth saved, a second edge-aware filter to get wrong).

## Approach

No renderer change — the design is sound and measured (2.22 ms @1080p on perf_abel, cited in
`GraphicsApply.cpp`). This ticket pins the design against regression with a CPU-only
contract test, so a future edit cannot silently drop the replication, the nearest-surface
pick, or either blur rejection term.

## Files changed

- `engine/WgpuRenderer/rust/src/gfx3d/mod.rs` (+38): `gtao_half_res_march_stays_pinned_to_the_bilateral_upsample_contract`.

## How to toggle it off

- `WGR_GTAO_SCALE=1` restores the legacy per-pixel march (same binary, A/B-safe); the dev
  panel AO quality control drives `Gfx3d::set_gtao_scale` live (clamped 1..=2).
- `GtaoSettings.enabled = false` gates the whole pass.

## Evidence

- `cargo test -p wgpu_renderer --lib -- gtao_blur_is_edge_aware gtao_validates_and_keeps gtao_round_trips gtao_reconstructs gtao_slice gtao_half_res normal_resolve_takes`: **8 passed, 0 failed**.
- New test is rustfmt-clean (repo-wide `rustfmt --check` drift is pre-existing, none of it in this hunk).
- No GPU timings captured in this session: the fixed flyby + retail data + GPU run is ticket 18's
  capture script; GTAO regions (`GtaoPrep`/`GtaoCompute`/`GtaoBlur`) already exist in `gpu_timers.rs`
  for it. Prior art: 2.22 ms @1080p (perf_abel, VERIFIED-as-read in `GraphicsApply.cpp`).
- No screenshots: test-only change, no visual delta by construction.

## Risks

- None functional (additive test, no shader/runtime change). Device tests
  (`gtao_resources_are_valid_on_a_real_device`, sky readback) not run here — need a real GPU.

## Follow-ups

- Ticket 18: run the fixed flyby at 1080p/1440p/4K with `WGR_GTAO_SCALE=1` vs `2`, record
  `GtaoCompute`/`GtaoBlur` columns into `docs/proteus/perf/`.
- Ticket 19: golden-image A/B of scale 1 vs 2 on the forest/interior scenes.
- No `status-ledger.yaml` exists in-tree (see STATE_OF_REPO.md); this file serves as the A1 ledger entry.

## Branding/compat

No assets added. No MP/simulation code touched. Naming uses project-neutral renderer terms only.
