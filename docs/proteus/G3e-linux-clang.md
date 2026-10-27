# G3e-linux-clang — PoseidonGame builds with clang in GNU mode

Status: done-unverified (Linux built and linked locally on Ubuntu 26.04 / clang 21.1.8; the walk-in run
40456099564 is fully green, including the Windows build and the 3,887-case C++ gate)

## Problem

`G3d-ci-repair` got the Linux job past its setup failures — vcpkg now installs all 42 ports and
`cmake --preset linux-x64-clang-rwdi` configures to completion — and the job reached the compiler
for the first time in this line's history:

```
[16/708] Building CXX object engine/Poseidon/CMakeFiles/Poseidon.dir/AI/AICenter.cpp.o
Ballistics.hpp:46:1: error: declaration conflicts with target of using declaration already in scope
```

The remaining red was therefore no longer tooling; it was the engine. clang in GNU mode rejects
code MSVC accepts, and 692 of the job's 708 build edges had never been compiled for Linux at all.
The named error was one instance of one bug class, with an unknown number behind it.

## Approach

Reproduce locally instead of through CI. Every CI round trip here costs ~20 minutes (configure,
42-port vcpkg install, then the build), and ninja stops at the first failing edge, so a CI-only loop
would have found one error class per ~20 minutes.

- **Environment:** Ubuntu 26.04 (WSL2), clang 21.1.8, CMake 4.2.3, ninja 1.13.2, a full vcpkg clone
  with the repo's manifest (`x64-linux-clang`, 42 ports), built out of tree at `/root/bld` from
  `/mnt/e/Proteus/OpenPoseidon` so the working tree stays clean.
- **Enumerate, do not iterate:** `cmake --build /root/bld --target PoseidonGame -- -k 0 -j 16`.
  `-k 0` keeps going after a failure, so one pass lists every TU that does not compile.
- **Then prove it:** `ninja -t clean` and a full rebuild, so nothing passes on a stale object.
- **Two local gaps, neither a repo bug:** `python3-venv` (vcpkg's libxcrypt/libsystemd ports set up
  a virtualenv) and `clang-format` (`CMakeLists.txt:381` requires it for the `Format` targets). The
  `ubuntu-latest` runner image has both.

## What clang rejected

Eight errors, in three bug classes:

| Site | clang | Cause |
| --- | --- | --- |
| `World/Entities/Weapons/Ballistics.hpp:46` | declaration conflicts with target of using declaration; then "reference to 'AmmoType' is ambiguous" at `Network/NetworkIface.hpp:392` | a global `class AmmoType;` beside `namespace Poseidon { class AmmoType; } using Poseidon::AmmoType;` in `Core/Types.hpp` declares a second class of that name. MSVC accepts the pair; clang then has two candidates for every unqualified use |
| `World/Physics/PhysicsWorld.hpp:17`, `World/Physics/PhysicsBackend.hpp:13` | same | same, for `LODShape` |
| `Dev/Diag/ProbeMeshes.hpp:14` | same | same, for `LODShapeWithShadow` |
| `Core/Game/GameLoop.cpp:125` | expected unqualified-id | `::Sleep(...)`: on POSIX `Foundation/platform.hpp` defines `Sleep(ms)` as a function-like macro onto `sleepMs()`, so the leading `::` is left stranded in front of the expansion |
| `Graphics/Rendering/Draw/Font.cpp:2` | 'windows.h' file not found | unconditional Windows include; the TU's only Windows use was `GetModuleFileNameA`/`MAX_PATH` |
| `UI/ProteusTheme.cpp:31-32` | use of undeclared identifier 'MAX_PATH' | `GetModuleFileNameA(..., MAX_PATH)` for "assets beside the executable" |
| `Random/randomGen.cpp:3` | 'intrin.h' file not found (from clang's own `intrin.h:12`) | `<intrin.h>` is the MSVC header; clang's resource-dir copy forwards to it. `_ReturnAddress()` is the only thing used |

The `AmmoType` fix alone would not have turned the job green: it was the first of the eight.

**The two executable-path sites share a fix.** `Fonts/` and `assets/ui` are resolved relative to the
running binary, and that resolution is the same operation in both. Rather than paste a POSIX branch
twice, `Foundation::getExecutablePath()` was added to `Common/PlatformPaths.{hpp,_win.cpp,_posix.cpp}`
(Windows `GetModuleFileNameW`; Linux `/proc/self/exe`; macOS `_NSGetExecutablePath`), where the other
"ask the platform where I am" helpers already live.

**`_ReturnAddress()`** became one helper, `RngDrawReturnAddress()`, with the MSVC intrinsic and
`__builtin_return_address(0)` behind the same call site; `<intrin.h>` stays, guarded by `_MSC_VER`.
clang-cl defines `_MSC_VER`, so it takes the intrinsic branch and the include — unchanged from before.

## Files

- `engine/Poseidon/World/Entities/Weapons/Ballistics.hpp`
- `engine/Poseidon/World/Physics/PhysicsWorld.hpp`, `engine/Poseidon/World/Physics/PhysicsBackend.hpp`
- `engine/Poseidon/Dev/Diag/ProbeMeshes.hpp`
- `engine/Poseidon/Core/Game/GameLoop.cpp`
- `engine/Poseidon/Foundation/Common/PlatformPaths.hpp`, `PlatformPaths_posix.cpp`, `PlatformPaths_win.cpp`
- `engine/Poseidon/Graphics/Rendering/Draw/Font.cpp`
- `engine/Poseidon/UI/ProteusTheme.cpp`
- `engine/Random/randomGen.cpp`

## Toggle and default

n/a — no behaviour change. Forward declarations move into the namespace their class is actually in;
the executable path is resolved by the same three platform calls as before; `Sleep(...)` is still the
Win32 API on Windows (`::Sleep` and `Sleep` name the same function there, and the macro does not exist
off POSIX); the RNG diagnostic prints the same address on MSVC.

## Evidence

- **Enumerating build** (`-k 0`, before any fix): 673 of 681 edges built, 8 errors in 6 files, exactly
  the table above.
- **Clean rebuild after the fixes:** `ninja -t clean`, then 673 C++ TUs compiled, exit 0, and
  `Linking CXX executable apps/cwr/Game/ProteusGame` — the engine and the game target build and link
  on Linux for the first time on this line.
- **CI (run 40450018964, head `3d8dad1`):** `build (linux-x64-clang-rwdi)` succeeded — `toolchain`, the
  vcpkg restore, `cmake --preset linux-x64-clang-rwdi`, then 707/708 edges with no `error:` line and
  `Linking CXX executable apps/cwr/Game/ProteusGame`. `asset registry`, `format (changed hunks)` and
  `rust unit tests (Lavapipe)` were already green; the Windows and validation-layer jobs are skipped on PR
  runs by design.
- **Windows, local (job commands):** `cmake --build build/win-x64-clang-rwdi --target PoseidonGame`
  exit 0, `ProteusGame.exe` linked; the six test targets built with no error and
  `ctest -E "<three named cases>" --output-on-failure --no-tests=error` reported **3,887 tests, 100 %
  passed**, 138 s — identical to the pre-change result, so the portability edits are inert there.
- **CI walk-in run 40456099564 (workflow_dispatch, head `17f9296`):** all seven jobs green —
  `build (win-x64-clang-rwdi)`, `C++ unit tests (win-x64-clang-rwdi)` (3,887 passed, 148.8 s),
  `build (linux-x64-clang-rwdi)`, `rust unit tests (Lavapipe)`, `rust unit tests (validation layer,
  serial)` (its first run), `asset registry`, `format (changed hunks)`. It is the first fully green run
  of this workflow on the line.
- **Repo gates:** `check-format.py --base <merge-base>` OK (137 changed files); `check-assets.py`,
  `gen-roadmap.py --check` and the `perf_parse`/`imgdiff` selftests OK.
- The earlier `git diff` of the branch's own DX12-1 terrain fix is what the Rust job's first compile
  caught (`DX12-1a`, already landed); no Rust source is touched here.

## Risks

- **The Windows jobs still do not run on ordinary PR pushes** — they are main-push/dispatch-gated, so
  their coverage of these edits came from the manual dispatch above and will not repeat per PR.
- **The macOS branch** (`_NSGetExecutablePath`) is uncompiled; the file already had macOS branches and
  macOS is not in CI.
- **clang 21 locally vs clang 18 on the runner.** No diagnostic here is version-specific, but the
  runner is the authority.
- **Build, not run.** No GPU and no game data here; nothing was rendered or simulated.

## Follow-ups

- `Font.cpp`'s flat fallback still formats `"Fonts\\%s"` with a backslash; on Linux that is a filename
  containing a backslash, so the fallback can never resolve. It is a silent miss, not a compile error.
- Nothing open for this ticket; `Font.cpp`'s backslash fallback above is the next piece of the port.

## Verify on GPU

n/a — no renderer change.
