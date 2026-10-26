# G3b-cpp-tests — a CI job that builds and runs the C++ test binaries

Status: done-unverified (job written and exercised locally; not yet run on Actions)

## Problem

`FIXTURE-SKIP-1` left this open:

> Add a C++ test job to CI (`build-windows` builds `PoseidonGame` only, so `ctest` has
> no test executables to run).

Three things were true before this ticket, and together they meant no CI run had ever
executed a single C++ test case:

1. **No job built the test targets.** `build-linux` and `build-windows` configure the
   whole project and then build `PoseidonGame` only. Every C++ test is registered with
   CTest per *case* (`catch_discover_tests`), so each entry names an executable that was
   never built — the `ctest` step in those jobs could only find nothing to run or fail on
   a test it could not find.
2. **Their build command was wrong anyway.** Both used
   `cmake --build --preset <name>`, but `CMakePresets.json` defines configure presets
   only, so that form errors out instead of building. `docs/proteus/BUILDING.md` already
   documented the working form (`cmake --build build/<preset>`).
3. **`ctest` aborted project-wide.** `tests/unit/apps/Studio/CMakeLists.txt` registered
   ImGui Test Engine discovery, whose CTest include file is generated only by a
   POST_BUILD step. `PoseidonStudioTests` cannot compile in a default checkout
   (`imgui_te_engine.h` comes from the vcpkg imgui `test-engine` feature, which
   `vcpkg.json` does not request), so the file was never produced and every `ctest`
   invocation died on the missing include before listing a test.

## Approach

- **New `cpp-tests` job** (`.github/workflows/proteus.yml`): `windows-latest`, gated on
  `proteus/main` pushes and manual dispatch like `build-windows`, because a cold
  configure + build of the engine and the test targets is the expensive part. It
  configures the preset, builds the six suites that compile, and runs
  `ctest -E <three named cases> --output-on-failure --no-tests=error`.
- **Windows, not Linux:** the suite is developed and run there
  (`docs/proteus/TOOLCHAIN-1.md`, `AGENTS.md`); nothing here was verified on a Linux
  runner.
- **`--no-tests=error`** so a build or registration that silently produced nothing
  fails the job instead of passing vacuously — the exact failure mode this ticket is
  about.
- **`PoseidonStudioTests` discovery is now conditional on the ImGui Test Engine headers
  being installed** (`find_path(IMGUI_TEST_ENGINE_INCLUDE_DIR imgui_te_engine.h)`), with
  a STATUS line when it is skipped. A discovery step for a binary that cannot be built
  is not coverage; registering it broke every other test.
- **The two `cmake --build --preset` invocations and the two vacuous `ctest` steps in
  the build jobs are fixed/removed**, with a comment pointing at `cpp-tests`.

### What the gate excludes, and why

Exactly three cases are excluded by name (`ctest -E`), nothing else:

| case | why |
| --- | --- |
| `Dev panel controls all go through the search filter` | red on every machine: scans `DebugOverlay.cpp` for `Dev::` wrapper use the panel bypasses (`FIXTURE-SKIP-1` known failure 1) |
| `WGPU instance translation storage belongs exclusively to the consumer` | red on every machine: `DrainInstanceOps` looks up before the slot invalidation, reversing the invariant the guard encodes (known failure 2) |
| `Compat smoke - each generation's models are the revision that generation ships` | machine content: answers from an installed Arma/DayZ corpus (known failure 3). It SKIPs on a runner; on a developer box with an off-revision install it fails, which a gate cannot decide |

Everything else runs. Fixture-dependent cases SKIP themselves (`FIXTURE-SKIP-1`) and the
run lists what it could not cover.

### What is not covered

- `PoseidonStudioTests`: cannot compile without the imgui `test-engine` feature.
- The `~[GameData] ~[gamescan] ~[external-data]` cases: excluded from discovery by the
  pre-existing `OFPR_CATCH_TEST_SPEC` in `tests/unit/engine/Poseidon/CMakeLists.txt`
  whenever `packages/Remaster` is absent, which prints a STATUS line saying so. They need
  retail game data; CI has none.

## Files

- `.github/workflows/proteus.yml`: new `cpp-tests` job; build-invocation fix and
  `ctest`-step removal in `build-linux` / `build-windows`.
- `tests/unit/apps/Studio/CMakeLists.txt`: CTest discovery registered only when the
  ImGui Test Engine headers exist.

## Toggle

n/a (CI configuration and a configure-time guard; no engine behaviour, no runtime path).

## Evidence

Local run of the exact job commands on `win-x64-clang-rwdi`, all six binaries built:

| run | result |
| --- | --- |
| `cmake --build build/win-x64-clang-rwdi -j 4 --target <six>` | exit 0 |
| `ctest -E "<three cases>" --output-on-failure --no-tests=error` | **3,886 tests, 100% passed**, exit 0, 111.8 s |
| same, without the third exclusion | 3,887 tests, 1 failed (the machine-content case above) |
| `PoseidonCoreTests` / `PoseidonTests` / `PoseidonFoundationTests` / `PoseidonEvaluatorTests` / `PoseidonServerTests` / `PoseidonTetrisTests` run directly | 871/0/24, 3,922/3/154, 494 pass, 95 pass, 10 pass, 9 pass |

`ctest -N` now lists tests instead of aborting on the missing Studio discovery file.

## Risks

- The job has not run on Actions yet (it is main/dispatch-gated). Its risk is CI-only:
  cold vcpkg install time on a fresh runner, and any case that behaves differently on a
  runner without a GPU or audio device (audio cases SKIP, and the discovery helper passes
  `--allow-running-no-tests` so a skip-only case is a pass).
- The gate is Windows-only, so Linux behaviour of these binaries is still unverified.

## Follow-ups

- Add the imgui `test-engine` feature to `vcpkg.json` and put `PoseidonStudioTests` in the
  gate; until then the Studio UI suite runs nowhere.
- Fix the three excluded cases and shrink the exclusion list to zero.
- Once a main run establishes the timing, consider running `cpp-tests` on pull requests
  too — today no CI run on a PR executes a C++ test.
