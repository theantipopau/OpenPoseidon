# Handover — Upscaler chain: FSR 3.1 + Native TAA wiring (OpenPoseidon)

**For:** the next AI agent picking up this work.
**Scope:** everything done and found in this session, in the `OpenPoseidon/` subtree (its own git repo; the outer workspace root is NOT a git repo — run all git commands with `git -C OpenPoseidon …`).
**Commits:** work is uncommitted on top of `e3ef6fe` ("UI-THEME-C"). 17 modified files + 1 new file (`engine/WgpuRenderer/rust/src/fsr31.rs`). Fixture `.p3d`/`.paa` untracked files are pre-existing synthetics, not ours.

## 1. TL;DR

- **New backend:** `fsr31.rs` (331 lines) — wgpu-native cross-vendor temporal upscaler (jitter-compensated EASU+RCAS). Chain order is now **DLSS > FSR 4 > FSR 3.1 > Native TAA (100% only) > FSR 1 > Bilinear**. Honest v1 scope: NOT the full FFX 3.1 accumulation core (no history reuse / disocclusion rejection yet) — see §4.
- **Native TAA is now live-toggleable** (was env-only): new `nativeTaaOn` session-only flag, dev-panel checkbox, C→Rust flag bit 8 (256), `WGR_NATIVE_TAA` boot seed on both sides so the panel round-trips instead of clobbering.
- **FSR 3.1 is the new Auto default** without DLSS (was FSR 1), plus an explicit Options-menu entry (dlssMode 4) and dev-panel checkbox (flag bit 7 / 128).
- **Capture parity:** game capture (`GameApplication.cpp`) and harness `frame_performance` both emit `upscaler` name 0..6.
- **Verified:** `cargo test -p wgpu_renderer --lib upscaler` 9/9 + `fsr31` 3/3, exit 0 (pipefail-preserved, rerun after the lib.rs bit-8 edit). C++ compile + in-game flip-through NOT run — required before shipping (§6).

## 2. Exact deliverables (paths + interfaces)

### New file
- `OpenPoseidon/engine/WgpuRenderer/rust/src/fsr31.rs` — `Fsr31Upscaler`, pure helper `easu_constants_jittered(render, output, jitter_px)`, `FSR31_CAPS` (id 6, needs temporal, no external sharpen). MIT AMD kernels reused via `include_str!("fsr1.wgsl")`; only jitter compensation is ours.

### Cross-layer contract (must stay in sync — all verified agreeing)
| Layer | IDs | Flags |
|---|---|---|
| `rust/src/upscaler.rs` | `ID_NATIVE=0, ID_DLSS=1, ID_FSR1=2, ID_BILINEAR=3, ID_FSR4=4, ID_NATIVE_TAA=5, ID_FSR31=6` | `UpscaleSettings { fsr31_on, native_taa_on, … }`, `FSR31_CAPS` |
| `rust/src/ffi.rs` + `include/wgpu_renderer.hpp` | `active_upscaler` 0..6 (comments updated, layout unchanged) | `WgrTemporalTuning.flags` bit6=FSR4, **bit7=FSR 3.1 (128), bit8=Native TAA (256)** |
| `Graphics/Core/Engine.hpp` | `TemporalInfo.activeUpscaler` comment 0..6 | `TemporalSettings { fsr31On=true (default!), nativeTaaOn=false (session-only), … }` |
| `WgpuRenderer/EngineWgpu.cpp` | — | `SetTemporalTuning` packs bits 128/256; ctor seeds `_temporalTuning.nativeTaaOn` from `WGR_NATIVE_TAA`; `SetRenderScale` comment now says "else FSR 3.1" |
| `rust/src/lib.rs` | chain push order FSR4 → FSR31 → NativeTAA → spatial | `fsr31_runtime_on` (default true, `WGR_FSR31=0` kills), `native_taa_on` (default false, `WGR_NATIVE_TAA=1` seeds); both live-updated from tuning flags |

Single `WgrTemporalTuning` producer confirmed (only `EngineWgpu::SetTemporalTuning`). No positional constructors of touched structs → defaulted fields break nothing. **C ABI layouts untouched** (comments + flag bits only), hash unchanged.

### UI / capture
- `Dev/Debug/DebugOverlay.cpp` — Temporal tab: new FSR 4 + FSR 3.1 + Native TAA checkboxes with tooltips; `kUpscalerNames` 7 entries; sharpness-live includes id 6; "Native (everything off)" clears all six; Auto-defaults tooltip says "else FSR 3.1". Panel→cfg mapping handles `fsr31On`-only → `dlssMode 4`.
- `UI/Options/GraphicsPage.{cpp,hpp}` — Upscaler row 5→6 entries (`FSR 3.1` label index 5), `dlssMode` mapping `+1` extended to `<=4`, descriptions say "otherwise FSR 3.1" / "FSR 1 / FSR 3.1".
- `UI/Settings/GraphicsApply.cpp` — `t.fsr31On = (mode==-1 || mode==4)`; Auto-without-DLSS → 67% + `temporalOn=true` (Auto always sets temporal now); FSR1 stays on as no-temporal fallback.
- `UI/Settings/GraphicsConfig.hpp` — `dlssMode` docs: `-1=AUTO (DLSS else FSR 3.1)`, `4=FSR 3.1`.
- `apps/cwr/Game/GameApplication.cpp` — capture `kUpscalers` 0..6 + `"upscaler"` string.
- `Dev/Harness/HarnessBuiltins.cpp` — `frame_performance` same 0..6 name table + `"upscaler"` string; fixed stale "else native" comment on `SetRenderScale`.
- `THIRD_PARTY_NOTICES.md` — new "FSR 3.1" MIT section; FSR 4 section pinned to SDK 2.3.0 / Upscaling 4.1.1, Vulkan-route disclaimer.
- `WgpuRenderer/{CMakeLists.txt,rust/Cargo.toml,rust/src/fsr4.rs}` — comment-only: DLSS SDK 310.x tested, Streamline not wired, FSR4 header version.

## 3. Key findings / decisions for the next agent
1. **FSR 3.1 v1 ≠ full FSR 3.1.** It is jitter-compensated EASU+RCAS — a real fix for FSR 1's jitter shimmer, but history reuse / disocclusion / Native-AA mode are staged follow-ups slotting into the same struct/caps/chain position. Don't claim more.
2. **Native TAA is session-only by design** (no graphics.cfg lane). Options page cannot persist it; dev panel + `WGR_NATIVE_TAA=1` only.
3. **FSR 4 is DX12-only** (`WGPU_BACKEND=dx12` + signed DLLs beside exe or `WGR_FSR4_DIR`), RX 7000+ since SDK 2.3. No Vulkan FSR 4 provider. FSR 3.1 is the documented Vulkan route.
4. **Priority disputes resolve by chain order** (first `Done` wins): DLSS(-DLAA) > FSR 4 > FSR 3.1 > Native TAA > FSR 1 > Bilinear.
5. **4 pre-existing full-suite failures** (ABI-version, aerial-perspective, 2× GI probe) are unrelated — left untouched, do not "fix" them in this lane.

## 4. What was verified
- `cargo test -p wgpu_renderer --lib upscaler` → **9/9 pass**; `… fsr31` → **3/3 pass**; exit 0, pipefail-preserved, rerun after final lib.rs edit. Tests pin ids 0..6, backend-id uniqueness, FSR31 caps-vs-impl, jitter-compensation math.
- Edited C++ hunks read back correct; struct-layout/ABI check done by inspection.
- `git -C OpenPoseidon diff --stat HEAD` + `status --short` re-confirmed final state (§2 file list).

## 5. What could NOT run / remaining limitations
- **C++ compile not run** (full engine build) — do this first.
- **No in-game verification**: flip FSR 4 checkbox on DX12 (`Start.ps1 -Dx12`, RX 7000+), Native TAA at 100%, FSR 3.1 on Vulkan; confirm `activeUpscaler` 4/5/6 in panel + captures.
- Native TAA reference needs 100% scale; FSR 3.1 falls back to FSR 1 with temporal off (by design, `needs_temporal_inputs`).

## 6. Suggested next steps (in order)
1. Build the engine (C++) and fix any compile fallout from the 6-entry upscaler arrays.
2. In-game flip-through (§5) on both Vulkan and DX12 backends.
3. Implement the FSR 3.1 accumulation core in `fsr31.rs` (history + disocclusion + Native-AA) + performance measurement vs FSR 1 at 67%.
4. Decide whether Native TAA deserves a graphics.cfg lane (currently deliberately session-only).
5. Commit on top of `e3ef6fe` (don't bundle the pre-existing fixture untracked files unless intended).

## 7. Quick reference commands
```
git -C OpenPoseidon status --short
git -C OpenPoseidon diff --stat HEAD
cargo test -p wgpu_renderer --lib upscaler   # (run from OpenPoseidon/engine/WgpuRenderer/rust)
cargo test -p wgpu_renderer --lib fsr31
set WGR_NATIVE_TAA=1   /   $env:WGR_NATIVE_TAA=1   # seed Native TAA at boot
set WGR_FSR31=0        /   $env:WGR_FSR31=0        # kill FSR 3.1
Start.ps1 -Dx12        # DX12 backend (FSR 4 path)
```
