# Known Issues

Build and runtime issues found during setup. Each entry notes whether it is fixed in this repo.

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

- **Window launch aborts on missing Remastered fonts.** `OpenPoseidon.exe -C <dir> --window` exits 1 with `FontSystem: required font missing: Fonts\cwr_{title,body,mono,serif,hand}.ttf`. The probe used the local OFP Evos folder as the data directory, and it lacks the Remastered files. Those come from `scripts/Install.ps1`, which copies `fonts`, `dtaExt`, `AddOns\cwr_logo.pbo` and `BIN` from the Steam **Demo** install into the retail install. Both installs must be found, and the registry lookup finds neither. Not fixed in repo; blocked on the install paths.
- **Mod mounting not verified.** The probe aborted before `ModSystem::SetModPath` logged `Mod path set` / `Loaded mod`. Retest once the installs are in place.
- **Build warnings.** The `PoseidonGame` build logged 763 `warning:` lines (clang-cl and the Rust crate). No errors. Not triaged.
- **Engine config is written to the user profile.** First run created `%APPDATA%\CWR\audio.cfg`. Expected; noted so it isn't mistaken for repo state.
