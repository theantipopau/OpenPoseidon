# RUST-TESTS-1 — the four standing `cargo test --lib` failures

Branch: `proteus/DX12-1-dx12-upscaler-fixes` (uncommitted, on top of `4b4cca3`).

Before this ticket the crate's own suite ended at **719 passed / 4 failed** (and a
`STATUS_ACCESS_VIOLATION` when the real-device tests ran in parallel). All four were
reproducible on clean HEAD, so they were reported as pre-existing and left assigned to
their owning tickets. Three of the four turned out to be *stale test expectations or a test
harness that did not mirror the production device*; the fourth was a real shader defect.

## 1. `ffi::abi_tests::exported_abi_version_matches_the_public_contract`

The test asserted `wgr_abi_version() == 21`, while the implementation and
`include/wgpu_renderer.hpp` had both moved to **22** (terrain-following fog and coherent
density modulation). The ABI pair was consistent; only the test was stale.

Rather than restate 22 and wait for the next drift, the test now reads
`#define WGR_ABI_VERSION` out of `wgpu_renderer.hpp` with `include_str!` and compares it
against `wgr_abi_version()`. A one-sided bump therefore fails immediately, and an explicit
pin (`assert_eq!(declared, 22)`) keeps a two-sided bump a deliberate, reviewed edit.

Also reflowed the adjacent `old_mesh_fact_summary_pair_...` test: it sits inside
`check-format.py`'s ±3-line fuzz around the edited hunk, so its pre-existing `,80` spacing
drift would have failed the hunk-scoped format gate.

## 2 + 3. `gfx3d::gi::tests::gi_probe_pipeline_builds_on_a_real_device` and `gi_gather_survives_the_object_shader_backend`

Both failed with a wgpu validation error, not an assertion:

```
Too many bindings of type SampledTextures in Stage FRAGMENT, limit is 16, count was 19
Too many bindings of type SampledTextures in Stage COMPUTE,  limit is 16, count was 18
```

Production requests `max_sampled_textures_per_shader_stage` =
`adapter.limits()…clamp(16, 64)` (REN-GI-001); the two headless test helpers stopped at
`Limits::default()`, i.e. 16, so they rejected pipelines the shipped renderer builds. The
helpers now request the same limit the engine does — the tests still compile real shaders on
a real device, they just stop testing a device the engine never creates.

## 4. `shaders::tree_snow_tests::forest_actual_gpu_audited_atlas_crowns_trunks_ground_shelter_and_ablation` — a real bug

`atlas sample 14` is the NaN case (`uv.y = bitcast<f32>(0x7fc00000u)`), expected crown 0.
It sampled **1.0**, and the coverage column followed it (0.606414 = full crown × powder
exposure), so a NaN atlas V painted a full snow crown.

Root cause: the guard was `if (!all(abs(uv) < vec2<f32>(1e20)) || …) { return 0.0; }`.
That is correct under strict IEEE (a NaN compare is false), but the DX12 backend folded the
compare under optimization and let the NaN through to `1.0 - smoothstep(0.27, 0.42, NaN)`.
`tree_snow.wgsl` now gates both UV entry points on `forest_snow_uv_finite`, a bit test on
the exponent (`(bits & 0x7f800000u) != 0x7f800000u`) that a fast-math backend cannot fold
away; the old magnitude test stays as a belt-and-braces bound on huge-but-finite UVs.

## Verification

- `cargo test --lib -- --test-threads=1`: **723 passed / 0 failed / 5 ignored**, exit 0
  (was 719/4, plus a parallel-run access violation that the serial pass does not reproduce).
- `cargo test --lib abi_tests`: 13/13, exit 0.
- The four named tests individually: 3/3 then 1/1, exit 0.
- `check-format.py --base <merge-base>`: OK.
- `check-all`: OK.
- No GPU capture was taken: these are device-init and shader-module assertions, not frames.
  The NaN fix is proved numerically by the acceptance test that caught it.

## Follow-ups

- The parallel `cargo test --lib` access violation is still unexplained; serial runs are
  green. Reproducing it needs a debugger on the real-device tests, which is an owner action.
