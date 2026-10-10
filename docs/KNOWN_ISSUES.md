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
- **vcpkg wants CMake 4.4.3 and 7-Zip 26.03 and PowerShell 7.6.6.** It downloads them itself. The system CMake 4.4.1 is not used by vcpkg.
- **Ninja not found in outer CMake.** Seen as `CMAKE_MAKE_PROGRAM is not set` after a failed vcpkg step. Likely a side effect of the vcpkg failure; re-check once vcpkg succeeds.

## Runtime

Not yet tested. The engine launch and dev-panel check needs a successful build and a local game installation.
