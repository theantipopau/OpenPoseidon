# W0a — Upscaler interface + Native TAA backend (done-unverified)

## Problem

Direction update (2026-10-15): Proteus targets the browser (WebGPU); the
native desktop build stays the development/reference platform. DLSS is
native-only/closed and can never run in a browser, so every platform needs an
owned temporal upscaler behind one `Upscaler` interface, with Native (TAA
only) as the reference. W0a is step (a): the interface alignment + `Native`
backend + motion-vector audit. ProteusTemporal (W0b) and Spatial (W0c) build
on it.

## Approach

- `engine/WgpuRenderer/rust/src/upscaler.rs` — `UpscaleContext` (colour,
  depth, motion, jitter, exposure, reactive mask, reset flag),
  `UpscaleOutcome`, `TemporalUpscaler` trait, `BilinearUpscaler`. Native
  backend reuses the REN-TEMP-001K debug-resolve accumulator as its production
  path at 100% scale only (`native_history_wanted` in `temporal.rs`, gated on
  `native_taa_active() && render == output`; DLAA precedent covers the
  post-chain, which is generic over `hdr_upscaled`).
- `lib.rs` — `native_taa_active()`, `want_upscaled` extended for the Native
  TAA storage target, FSR4/DLSS runtime-off wiring kept compile-verified only
  (`gpu_verified: false` for anything vendor).
- `DebugOverlay.cpp` / `Engine.hpp` — upscaler name slot 5 = "native TAA"
  (`ID_NATIVE_TAA`), comment kept in sync with `upscaler.rs` ids.
- Motion-vector audit (W0.4, code-level): object/skin/veg velocity subsets
  are all consumed in the lib.rs frame path (`object_velocity_subset`,
  `skinned_velocity_subset`, `veg_velocity_subset` + GPU-driven retained twin,
  gated by `WGR_VEG_VELOCITY`); velocity debug view pre-exists
  (`WGR_TEMPORAL_DEBUG`). No new missing writer found in code — but the
  audit exposed a live counterpart (see W0-ORDER below).
- `PROVENANCE.md` — `pmndrs/upscaler` (MIT) study reference: repo URL,
  licence, reference commit `96184a6` (main, 2026-10-12, via web lookup,
  UNVERIFIED vs local clone), files studied (design only). **No code copied
  or ported in W0a**; W0b grows a per-file record first if it ports a pass.
- `golden-capture.ps1` — always keeps the game log (`--log-file`) next to
  each candidate PNG; without it this ticket's key finding would have been
  undiagnosable.
- `check-format.py` (NEW) + `check-all.ps1` + `proteus.yml` fmt-changed: the
  format gate was unpassable for ANY `.rs` change — `rustfmt --check lib.rs`
  recurses from the crate root into every module (demand_view/dlss/ffi/
  water_tw/… carry tree-wide pre-existing drift; G3 already logged 938
  rustfmt diffs), and `clang-format --dry-run` flags whole files with
  in-file drift. The gate's own header promises "pre-existing drift can't
  fail it" — now true: both tools run, but only violations landing on
  *added lines* (`git diff -U0`, per-tool fuzz: rustfmt ±3 for pre-format
  numbering, clang-format exact) fail. Two TRUE positives on W0a's own lines
  were fixed (lib.rs:2657 join, lib.rs:3178 split); the DebugOverlay
  reflow-region flags were proven pre-existing (HEAD version violates at the
  same lines) and are now correctly ignored.

## Files

| File | Change |
|---|---|
| `engine/WgpuRenderer/rust/src/upscaler.rs` | interface alignment (`UpscaleContext`/`Outcome`/`TemporalUpscaler`) |
| `engine/WgpuRenderer/rust/src/temporal.rs` | `native_history_wanted` production accumulator |
| `engine/WgpuRenderer/rust/src/lib.rs` | `native_taa_active()`, `want_upscaled`, runtime-off wiring |
| `engine/Poseidon/Dev/Debug/DebugOverlay.cpp` | name slot 5 = "native TAA" |
| `engine/Poseidon/Graphics/Core/Engine.hpp` | comment: slot 5 |
| `docs/proteus/PROVENANCE.md` | pmndrs/upscaler study reference (no code taken) |
| `scripts/proteus/golden-capture.ps1` | per-shot game log |
| `scripts/proteus/check-format.py` | NEW — hunk-scoped format check + selftest |
| `scripts/proteus/check-all.ps1` | format section delegates to check-format.py |
| `.github/workflows/proteus.yml` | fmt-changed uses the same script (same-gates promise) |
| `docs/proteus/W0a-native-upscaler.md` | this record |

## Toggle and default

`WGR_NATIVE_TAA=1` + `upscalerQuality=100` (Native, scale 1.0) selects the
Native TAA accumulator; `0`/unset restores previous behaviour. Nothing else
changes defaults. DLSS/FSR4 paths untouched (compile-verified only).

## Evidence

- Build: `WgpuRenderer` + `PoseidonGame` targets green (win-x64-clang-rwdi).
- `cargo test -p wgpu_renderer --lib`: **716 passed, 4 failed** — all 4 proven
  PRE-EXISTING by re-running on clean HEAD (changes stashed): identical 4
  failures + identical full-run segfault (exit 139). None touch W0a code:
  `exported_abi_version_matches_the_public_contract` (expects 21, code
  default 22 — stale fork-era constant in untouched ffi.rs),
  `every_world_geometry_family_takes_the_same_aerial_perspective`
  (frame.wgsl role assertion, untouched gfx3d), two GI real-device tests.
  Filed as follow-ups for the owning tickets; not fixed here (the ABI
  constant declares a public contract — bumping it blind would be wrong).
- **Populated Native-TAA render on RX 9070 XT**: `golden-capture -Shots open`
  with Native TAA on wrote a real 1920×1080 scene (`e248…`, 1.9MB,
  mottled-terrain stats, min≠max on all channels); its game log shows
  **178,487 objects offered, 173,564 admitted (97.2%)**. The backend runs a
  full scene without crashing or blanking.
- **Empty-world determinism (the W0-ORDER discovery)**: 5 of 9 captures this
  session rendered byte-identical `f191…` (`md5 f191f13a`, SSIM 0.00000
  across three separate runs incl. both TAA settings). Every `f191` run with
  a game log shows **0 objects offered, 0 admitted** with the engine's own
  warning ("world was placed before the GPU-driven path existed… nothing
  re-offers it"). Static camera + static empty scene ⇒ the accumulator
  converges exactly ⇒ TAA on/off byte-identical is CORRECT behaviour, and it
  doubles as a no-drift smoke test of the accumulator.
- `e139…` (TAA off) vs `e248…` (TAA on): imgdiff dissim **0.0040** (8× above
  the 0.0005 noise floor) — consistent with a real TAA effect, but the
  no-TAA run predates game-log capture so its population is UNVERIFIED: this
  number is **evidence, not a claim** (see Risks).
- `check-format.py --selftest`: PASS (hunks, paths, headers, offsets).
  `check-format.py --base proteus/main`: OK on the W0a changeset (caught and
  fixed 2 real violations of mine). `check-all.ps1`: **OK** (assets,
  roadmap, perf selftest, imgdiff selftest, format all green).

## Risks

- The clean populated-TAA-off vs populated-TAA-on A/B is still missing: the
  retained-admission race eats ~5/6 rapid-fire captures, and the one
  populated off-run has no log. W0c (modes + T18/T19 verification) must redo
  this with the race fixed or retried-to-populated (check the log's
  "admitted" line before comparing).
- Tree-wide rustfmt drift (dozens of files) and the 4 red cargo tests remain;
  the gate fix keeps them from blocking tickets but doesn't cure them.
  Whole-tree `cargo fmt` is NOT recommended while other threads edit the
  same files (conflict storm) — owner's call.
- Thresholds still synthetic (T19 caveat stands); `f191`-style empty captures
  would sail through golden-compare as "stable" — the bless flow needs the
  admitted-count check (W0-ORDER proposes it).

## Addendum 2026-10-15 (W0-ORDER proof runs)

Populated Native-TAA off/on A/B at 1440p, Native mode, all sides VALID
(boot-proof + admitted gate): open dissim 1.4e-06 / mae 0.005, woodland
3.4e-06 / 0.036 — static-scene convergence, exactly as predicted; ride
(motion) dissim 0.248 / mae 6.5, dominated by cross-run vehicle timing, so
it bounds but does not isolate the TAA effect. The earlier e139-vs-e248
0.0040 number is superseded by these guarded runs. Motion-isolated proof
moves to W0c ghosting shots.

## Follow-ups

- **W0-ORDER (proposed, blocks W0.4 + T19 goldens)**: intermittent
  0-objects-admitted race; re-offer on renderer-live + admitted-count gate
  in golden-capture.
- W0b ProteusTemporal, W0c Spatial + modes + full T18/T19 verification.
- Cargo failures for owning tickets: ABI-21-vs-22 constant, aerial-perspective
  role test, 2 GI real-device tests + full-run segfault attribution.
- Whole-tree format pass: owner's call (deferred deliberately).

## Verify on GPU (for Matt)

```powershell
# Native TAA on, Native mode (scale 1.0)
$env:WGR_NATIVE_TAA=1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/golden-capture.ps1 -Shots open
# Check the log FIRST: want "N objects offered, M admitted" with M >> 0.
# An "f191"-identical PNG + "0 objects offered" = the W0-ORDER race, not the upscaler.
```

- **Good:** populated log + a full terrain scene PNG; re-run with
  `WGR_NATIVE_TAA=0` (also populated) and imgdiff the pair — expect small
  nonzero dissim, no smearing/ghost trails.
- **Bad:** `0 objects offered` (race — retry after an idle minute or two;
  rapid-fire runs seem to hit it more), crash-dir warning, or PNG with
  uniform min==max channels (blank frame — read the game log tail).
