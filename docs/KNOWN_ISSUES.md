# Known Issues

Build and runtime issues found during setup. Each entry notes whether it is fixed in this repo.

## Game name

- The shipped client is **ProteusGame.exe** (CMake target still `PoseidonGame`; `OUTPUT_NAME` set in `apps/cwr/Game/CMakeLists.txt`). The engine is Poseidon and keeps its name. Scripts and docs were renamed to match. Old `OpenPoseidon.exe` copies in a game folder are removed by `scripts/Deploy.ps1 -PruneStale` and warned about otherwise.

## Open: WW4 units do not replace the defaults (Kozlice addon)

- The WW4 addons log `requires addon 'Kozlice'`, so their unit replacements never register.
- `Kozlice` is in three copies of `kozl.pbo` in the OFP Evos folder: `Addons\kozl.pbo` (2008, no product), `@Evos\Addons\kozl.pbo` (product `OFP: Resistance`, has `config.cpp` with `CfgPatches` class `Kozlice`), and `Res\AddOns\kozl.pbo`. All three share the prefix `kozl`.
- `AddonSystem::ParseAddonConfig` only sees a prefix's `config.cpp` through the file server, and it does not find `kozl\config.cpp`. Its `config.bin` fallback then returns true without registering anything. A stream parse of the bank's `config.cpp` returned zero entries, because `ParamFile::Parse(QIStream&)` skips the preprocessing that `Parse(name)` does, and `Preprocess` opens files by name.
- Fix options, not yet done: (a) remove the two older `kozl.pbo` copies from the game folder, which the mod layer should make unnecessary; (b) give the preprocessor an input-buffer entry point, so a bank's `config.cpp` can be parsed while it is mounted. (a) is a data change for the owner to make; (b) is an engine change.

## Fixed: DX12 renderer crashed on this machine

- `scripts/Start.ps1 -Dx12` (`WGPU_BACKEND=dx12`) on the RX 9070 XT: once run crashed with an access violation in `amdxc64.dll` about 4 seconds after renderer start; a control run with FSR 1 and no FSR 4 stalled after `prepass discard parity` with no frames. The Vulkan path renders normally. FSR 4 needs DX12, so it cannot be tested until this is fixed. Next step: check whether the stall predates the FSR 4 changes (build the previous commit).

## Repository layout

- **`CLAUDE.md` is missing.** The kickstart expects agent instructions and build notes at the root. Nothing exists. The README "Current limitations" section is the closest substitute and is used for the known-limitations list. Not fixed in repo; flagged for Matt.
- **`docs/` did not exist before setup.** Created with this change (`FORK_ROADMAP.md`, `ASSET_PIPELINE.md`, `KNOWN_ISSUES.md`, `CHANGELOG.md`).
- **Kickstart assumptions not matching the code.** Phase 3 assumes features may be missing. P1.1 (mod load order) already exists; see [FORK_ROADMAP.md](FORK_ROADMAP.md).

## Build setup (Windows, `win-x64-clang-rwdi`)

- **`LLVM_ROOT` not found by default.** `cmake/FindLLVMSanitizer.cmake` looks only in `C:\Program Files\LLVM` and `D:\Program Files\LLVM`. Visual Studio 2022 Community ships clang-cl under `VC\Tools\Llvm\x64`, which the finder does not check. Workaround: set `LLVM_ROOT` to that folder. Not fixed in repo.
- **vcpkg strips the environment for its compiler detection.** Setting `LLVM_ROOT` alone fails with `LLVM not found` during vcpkg's `detect_compiler` sub-build. Workaround: also set `VCPKG_KEEP_ENV_VARS=LLVM_ROOT`.
- **Shallow vcpkg clone breaks the pinned baseline.** The manifest baseline `170bd3bf...` and the box3d registry commit `d87340ac...` are old. A `--depth 1` clone lacks them. Use a full clone (`git fetch --unshallow` if already shallow).
- **`VCPKG_ROOT` default is `C:\dev\vcpkg`.** `scripts/Build.ps1` falls back to that path. This machine uses `E:\dev\vcpkg`, set explicitly.
- **`clang-format` not on PATH.** `CMakeLists.txt:381` runs `find_program(CLANG_FORMAT_EXE ... REQUIRED)`. `scripts/Build.ps1` adds only `C:\Program Files\LLVM\bin` to PATH. Workaround: add `...\Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin` to PATH for the session. Not fixed in repo.
- **vcpkg wants CMake 4.4.3 and 7-Zip 26.03 and PowerShell 7.6.6.** It downloads them itself. The system CMake 4.4.1 is not used by vcpkg.
- **Ninja not found in outer CMake.** Seen as `CMAKE_MAKE_PROGRAM is not set` after a failed vcpkg step. Likely a side effect of the vcpkg failure; re-check once vcpkg succeeds.

## Build wrapper

- **`scripts/Build.ps1` reports failure after a successful Rust step.** The run stopped at the `wgpu_renderer` cargo step, with no `FAILED:` line and no `EXIT=` marker. Running the same `cargo rustc` command directly exited 0. The likely cause is that `Build.ps1` sets `$ErrorActionPreference = 'Stop'`, so cargo's warnings on stderr become a terminating error. Not verified. Workaround: build with `cmake --build build/win-x64-clang-rwdi --target PoseidonGame` directly. Not fixed in repo.

## Game data and launch

- **Retail install not found by the registry lookup.** `scripts/CwaCommon.ps1` looks up Steam app 65790 through the registry and the `libraryfolders.vdf` manifests. On this machine it returns nothing, so `scripts/Start.ps1` aborts. Workaround: set `data_path` in `mods.json`. Start.ps1 now honours it.
- **Freeware premise is incorrect.** The continuation brief said CWA is freeware and could be hosted server-side. Public sources do not support that. See [ASSET_PIPELINE.md](ASSET_PIPELINE.md#game-data-source-cwa).
- **Launch and build scripts are Windows-only.** `Build.ps1`, `Start.ps1` and `CwaCommon.ps1` depend on PowerShell, the registry and Windows paths. Fine for the Windows desktop build. Any web build needs its own launch path. Logged for Phase 5.

## Runtime

- **Window launch aborts on missing Remastered fonts.** `ProteusGame.exe -C <dir> --window` exits 1 with `FontSystem: required font missing: Fonts\cwr_{title,body,mono,serif,hand}.ttf`. The probe used the local OFP Evos folder as the data directory, and it lacks the Remastered files. Those come from `scripts/Install.ps1`, which copies `fonts`, `dtaExt`, `AddOns\cwr_logo.pbo` and `BIN` from the Steam **Demo** install into the retail install. Both installs must be found, and the registry lookup finds neither. Not fixed in repo; blocked on the install paths.
- **Mod mounting not verified.** The probe aborted before `ModSystem::SetModPath` logged `Mod path set` / `Loaded mod`. Retest once the installs are in place.
- **Build warnings.** The `PoseidonGame` build logged 763 `warning:` lines (clang-cl and the Rust crate). No errors. Not triaged.
- **Engine config is written to the user profile.** First run created `%APPDATA%\CWR\audio.cfg`. Expected; noted so it isn't mistaken for repo state.

## Main menu art

- **Picture lookup ignored absolute paths.** `FindPicture` only searched the mission directory, `dtaExt\`, and PBO banks under `data\`. Menu art shipped in `assets\ui` never resolved. Fixed: an absolute path to an existing file is now used as-is.
- **2048-pixel background hangs the renderer.** A 2048-wide PAA in a picture control produced `wgr_bindless_texture_bind` validation errors, and the window stopped at "Wait a moment…". `CStatic` caps pictures at 1024 (`SetMaxSize(1024)`), so oversized art should be rejected or downscaled rather than hang. The menu art is kept at 1024 wide. Not fixed in the engine.
- **Art packed in a mod PBO does not resolve.** `GetPictureName` looks in `data\` through the bank selected by path prefix, and the base game's `data` bank wins. A `data`-prefixed PBO in a mod is never checked. Art therefore ships loose beside the executable.

## Status update 2026-10-14

- **Launch works on this machine.** The game builds (`scripts\Build.ps1 -Target PoseidonGame`, wrapped as `cmd /c "powershell -NoProfile -File ..."` to avoid the cargo-stderr abort) and runs with `scripts\Start.ps1 --window`. The "Runtime" and "Game data and launch" entries above that say launch is blocked are out of date for this machine. Logs are in `Documents\Cold War Assault\logs`.
- **Mod order.** `mods.json` lists `@proteus-units` last, so it wins over FFUR/PR. Its `bin/resource.cpp` and `STRINGTABLE.CSV` load at runtime and need no rebuild.
- **Editor and debriefing UI** (Proteus theme, 3D unit preview, flat after-action report) live in `@proteus-units` plus `UIArcade.cpp`, `UIMapDialogs.cpp`, `resincl.hpp`. They are untested in a full mission.
- **Wgpu display phase.** The UI pass format mismatch (`Rgba8Unorm` vs `Rgba16Float`) validation loop is fixed.
- **Profiling.** `World.cpp` logs `frame avg/p95 ms ...` with per-phase timings every 300 frames. No gameplay baseline captured yet.
- **Still open:** DX12 crash (blocks FSR 4), 2048px picture hang, missing `.rtm` animation files, no `CLAUDE.md`.
- **DX12 crash fixed (2026-10-14).** Cause: AMD's shader compiler (`amdxc64`, RX 9070 XT) crashed compiling the main object shader when `frame.wgsl::fog_sun_reach` did four `textureLoad`s of the fog ground grid (the same crash hit the terrain pipeline). It now loads three corners and picks the fourth by triangle, which gives the same result. `WGPU_DX12_COMPILER=fxc` does not help (it only delays the same crash). Separately, `fsr4.rs` passed the FFX context handle to `ffxDispatch` instead of a pointer to it, which crashed in the AMD loader once DX12 got that far. `Start.ps1 -Dx12` now runs and the log shows `FSR 4 context created ... (FFX upscaler 4.1.1)`. DX12 shader compile makes the first start slower.
- **DX12 black terrain fixed (2026-10-17).** On DX12 the terrain drew flat black or white with no ground textures. wgpu-hal's DX12 backend copies only the descriptors a partially bound array is given, but places the bindings after the array using its full declared size, so the index map, detail texture and jitter map in terrain group 2 (which follows the 4096-entry ground array) read as null. `Terrain::new` now pads the array on DX12 (`partially_bound` off there). Vulkan is unchanged. Checked with `scripts/Shot-Mission.ps1` (harness launch + screenshots): DX12 and Vulkan match on the jeep-ride, foliage-rays and tw-showcase dev missions, no device loss.
