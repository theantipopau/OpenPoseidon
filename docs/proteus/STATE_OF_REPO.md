# Proteus — State of Repo (Step 0)

Date: 2026-10-14. Fork: `theantipopau/OpenPoseidon`, branch `dev/matt-fork-main`
(33 commits ahead of `koosoli/OpenPoseidon` upstream `main`). Upstream remote configured.
Every claim below is marked **VERIFIED** (ran it / read the file) or **ASSUMED** (inferred, not checked).

## 1. What builds (VERIFIED unless noted)

- **Preset that works:** `win-x64-clang-rwdi` (RelWithDebInfo, `x64-windows-clang`,
  toolchain `cmake/toolchains/win-x64-clang.cmake`). VERIFIED via
  `cmake/presets/windows.json`, `OpenPoseidon/docs/FORK_ROADMAP.md`,
  `CHANGELOG.md`, `KNOWN_ISSUES.md`.
- **Build command that works:** `cmake --build build/win-x64-clang-rwdi --target PoseidonGame`
  → `dist/x64-win-rwdi/ProteusGame.exe` (CMake target still `PoseidonGame`, `OUTPUT_NAME`
  set in `apps/cwr/Game/CMakeLists.txt`). VERIFIED (docs + log claims; binary not rebuilt here).
- **Known build quirks (VERIFIED, from KNOWN_ISSUES.md):** needs full (non-shallow) vcpkg clone;
  set `VCPKG_ROOT` + `VCPKG_KEEP_ENV_VARS=LLVM_ROOT`; `LLVM_ROOT` must point at VS2022's
  `VC\Tools\Llvm\x64`; `scripts/Build.ps1` can report failure after a successful Rust step
  (`$ErrorActionPreference='Stop'` turns cargo stderr warnings fatal) — direct `cmake --build`
  is the workaround. 763 warnings (clang-cl + Rust) logged, untriaged.
- **Rust workspace:** `engine/WgpuRenderer` (`wgpu_renderer` crate), `engine/Trident`,
  `mserver/`. VERIFIED (Cargo.toml/Cargo.lock present, `engine/WgpuRenderer/rust/src/` listing).
- **Launch (VERIFIED per 2026-10-14 status update in KNOWN_ISSUES):** builds and runs via
  `scripts/Start.ps1 --window`; earlier "blocked on Remastered fonts" entries are stale *on that
  machine*. Retail + Demo installs + `mods.json` (`data_path`, `mods_dir`, `--mod=` list) required.
  **ASSUMED** still true on this machine — no build/run attempted in this pass (Windows-only
  toolchain; this host session did not compile).
- **No `CLAUDE.md` at repo root (VERIFIED — glob + KNOWN_ISSUES confirm missing).**
  No `docs/roadmap/`, `status-ledger.yaml`, or `renderer-systems-ledger.yaml` anywhere
  (VERIFIED — greps return nothing; the brief's ledger paths do not exist yet).
  No `docs/architecture/` folder (VERIFIED). No `.github/workflows/` dir — only `FUNDING.yml`
  (VERIFIED), so the brief's "ASSET-010 CI" has **no matching workflow in this tree**; treat as
  a brief-invented gate until proven otherwise.

## 2. Default-on vs prototype/opt-in (VERIFIED from UPSTREAM_README + source)

Default-on / shipped in normal builds:
- WGPU renderer (Vulkan default on Windows; DX12 via `Start.ps1 -Dx12` / `WGPU_BACKEND=dx12`).
- GL33 backend still exists (`engine/PoseidonGL33/`) — legacy path, kept alongside WGPU.
- GTAO + bent normals, probe-grid GI, layered volumetric fog, volumetric smoke/clouds,
  tidewater ocean (FFT, foam, wakes, underwater, caustics), procedural grass with gust fronts,
  rain/puddles/wetness, snow depth/deformation, destructible terrain, Box3D ragdolls (default on,
  dev-tab toggle), contact + sun/local shadows, muzzle illumination, FSR 1, HUD/telemetry
  (`frame avg/p95 ms` per-phase log every 300 frames), crash stacks 120 deep.
- Proteus UI shell: main menu, Options main page, Campaign/Single-mission select, editor dialogs,
  flat debriefing, unit info — all on by default (ProteusTheme module).

Prototype / opt-in / conditional:
- **DLSS:** optional, needs local NGX SDK; distributable builds disable it (BUILDING.md).
- **FSR 4.1.1 (new, DX12-only):** `engine/WgpuRenderer/rust/src/fsr4.rs`; needs signed AMD DLLs
  beside exe or `WGR_FSR4_DIR`; `graphics.cfg dlssMode=3`; falls back off DX12. Author-flagged
  unverified risks (FFX resource-state contract, motion-vector sign). Fixed two real bugs
  2026-10-14 (AMD `amdxc64` fog-shader crash via 3-load triangle derivation; FFX handle-vs-pointer).
- **Boot-sole normal/parallax relief:** opt-in prototype. FFV beyond stock Jeep, gradual-spotting AI
  switches, swimming anim addon: limited/conditional. Multithreading (enkiTS) covers selected
  systems only, not everything. Zeus/dev tools disabled in MP (by design).

## 3. Open tickets / known problems (VERIFIED from KNOWN_ISSUES.md + CHANGELOG.md)

1. WW4 units don't replace defaults — duplicate `kozl.pbo` prefix, `ParseAddonConfig` misses
   `config.cpp`; fix options (a) data cleanup, (b) preprocessor buffer entry point. **Open.**
2. 2048px menu PAA hangs renderer (`wgr_bindless_texture_bind`); art kept ≤1024. **Open.**
3. Missing `.rtm` animation files. **Open.** 4. No gameplay perf baseline captured yet
   (telemetry exists, no CSV baseline). **Open.** 5. Build warnings untriaged; clang-tidy/shader
   warnings not audited. **Open.** 6. Single-mission return-to-menu bug under diagnosis
   (uncommitted `DIAG` lines). **Open.** 7. Campaign/single-mission Play path + editor/debriefing
   UI untested in full mission. **Open.** 8. DX12 first-start slow (shader compile). Accepted.
9. Brief's queue items (GTAO half-res LIT-010, water edge/CDLOD/PERF-001, moon phase/light,
   grass clumps GRS-030, RND-030 legacy-path audit, TAA+jitter already present?, HDR review,
   bloom, SSR puddles, shadow stabilisation audit, wind unification, decals, texture workbench,
   material inference, terrain detail/triplanar, impostors/HLOD, pipeline cache, frame-pacing
   script, golden-image Trident tests, settings presets/FOV/cap, a11y, HRTF audio, WASM/ray-query
   spikes) have **no corresponding ledger entries or ticket files in-tree** — they are brief-only
   until each is evidence-checked against source. TAA/upscaler/tonemap/bloom/motion-blur/shadow
   code demonstrably exists (`temporal.rs`, `upscaler.rs`, `tonemap.rs`, `exposure.rs`,
   `bloom.rs`, `dof.rs`, `godrays.rs`); exact toggle/off-switch per feature **ASSUMED**, must be
   verified per ticket.

## 4. Fork divergence from koosoli/OpenPoseidon (VERIFIED: 33 commits, `git log upstream/main..HEAD`)

- Proteus branding + Barlow Condensed fonts + `assets/ui` art; window title `Proteus [WGPU/GL33]`;
  shipped exe `ProteusGame.exe`.
- Shared `ProteusTheme` UI module; restyled main menu, Options main, Campaign/Single-mission,
  editor dialogs (+3D unit preview), flat debriefing, unit info; MODS/LOCAL MAPS removed from menu.
- `FindPicture` absolute-path fallback; mod-font acceptance in required-font check.
- FSR 4 backend (`fsr4.rs`), Video-page upscaler choice, dev-panel id 4, tuning bit 6.
- DX12 AMD shader fix (fog 3-load), FFX dispatch pointer fix, post-tonemap UI display pass,
  per-phase frame telemetry, deeper crash stacks.
- Tooling/docs: `mods.json`/`mods.example.json` + Start.ps1 integration, `scripts/Deploy.ps1`
  stale-exe pruning, `docs/{FORK_ROADMAP,ASSET_PIPELINE,KNOWN_ISSUES,CHANGELOG,UI_SCREENS}.md`,
  `.gitignore` game-data rules. Engine/gameplay/simulation untouched by design.
- Uncommitted/working-tree: `Aspect_Ratio.hpp` untracked at `OpenPoseidon/` root; branch 1 ahead
  of origin. No retail/mod data in repo (VERIFIED: audit docs + `.gitignore`).

## 5. Hard-rule compliance baseline for future tickets

- Formats/saves/compat: untouched so far. Off-switches: UI theme is per-screen opt-in
  (`EnableProteusTheme()`); renderer levers mostly via graphics config/dev-panel/env
  (`WGPU_BACKEND`, `WGR_*`), but **no single global "legacy renderer" toggle verified** — each
  ticket must name its own toggle.
- One-branch-per-ticket not yet started (this file is Step 0, committed directly or as
  `proteus/step0-state-of-repo`). No MP/netcode touched. No new assets added in this pass.
- `docs/proteus/` is **new in this pass** (did not exist before).
