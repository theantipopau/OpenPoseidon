# FIXTURE-SKIP-1 — an absent fixture SKIPs its own cases; it does not kill the binary

Status: done-unverified (3,996 cases now run and report; nothing GPU-verified)

## What was wrong

`tests/unit/engine/Poseidon/test_fixtures.hpp` called `abort()` from `GET_FIXTURE` when a
fixture was missing. `copy-fixtures.cmake` (FIXTURE-2) had been made tolerant of the
release's deliberate omissions, and its comment said tests would "abort at runtime
(documented, not silent)" — but the effect was the opposite of documented:

- The whole `PoseidonTests` binary died at the **first** absent file. Measured on this
  checkout: exit 127 at `p3d/simple_proxy.p3d`, **0 test cases and 0 assertions
  reported**. Every case registered after that point — 3,996 of them — silently never
  ran, and nothing in the output said so.
- Nothing could be gated. The only usable runs were hand-picked tag filters that
  happened to sort before the first missing file.
- `PoseidonCoreTests` could not even be **built**: its `POST_BUILD` step ran a raw
  `cmake -E copy_directory` on `tests/fixtures/pbo`, which the source release omits, so
  the link step failed after a successful compile. FIXTURE-2 had only been applied to
  the other test target.
- `Support/test_fixtures.hpp` was a second, divergent copy of the same helpers: same
  macro names, no missing-file handling, no repo-walk resolution. Which behaviour a test
  got depended on which spelling of the include path it used.

## What changed

- `GET_FIXTURE` / `REQUIRE_FIXTURE` now `SKIP` the calling case, naming the file. A
  process-wide set records each distinct absent fixture and an `atexit` reporter prints
  one manifest line per file at the end of the run, so the run cannot be quiet about
  what it could not cover:

  ```
  === 34 fixture file(s) absent from this checkout; the cases that needed them were SKIPPED ===
    audio/click.wss
    ...
  ```

- `Support/test_fixtures.hpp` forwards to the single definition instead of duplicating it.
- `PoseidonCoreTests` stages fixtures through `scripts/proteus/copy-fixtures.cmake`
  (missing dirs warn, they do not fail the link); its extra dirs are `EXISTS`-guarded.
- A `SKIP` raised inside a `CHECK`/`REQUIRE` argument is caught by that assertion's
  exception translation and reported as a **failure**, so `GET_FIXTURE` must be hoisted
  into a local first. That is now stated in the header, and the 13 call sites that hit it
  (JPG 9, PNG 3, WRP 1) were hoisted.
- Tests that hand-built fixture paths or checked existence themselves now skip instead
  of failing: `TcPbo` (5 cases; its helper also resolves the repo copy now),
  `audio/wave-to-lip` (2), `audio/voice-lang-*` (3, via `RequireFixtureMedia`),
  `p3d/flat_quad.p3d` billboard (1), and the four `[archive-source-*]` arms that need
  their own dedicated `WGR_OBJECT_STREAM_*` process (they now skip like their
  `[archive-source-hot-retire]` sibling already did).

## Two real defects the abort had been hiding

- `I-05: FontSystem shutdown clears FreeType caches before engine destruction` grepped a
  fixed 2000-character window after `void UnloadGameData(bool keepEngine)`. SHUTDOWN-T-1's
  phase probes pushed `DestroyEngine()` past it, so the position came back `npos` and the
  ordering invariant could not be checked at all. It now scans to the next top-level
  definition.
- The sim-stage census had no classification for `Physics::GetPhysicsWorld()` in
  `Dev/Harness/HarnessBuiltins.cpp` (socket-driven status commands) or
  `World/Physics/CorpseArticulation.hpp` / `SoldierOld.cpp` (the automatic-ragdoll terrain
  registration, reached from the soldier's simulate). Both are now classified, the
  soldier path as `InTick(Vehicles)` because that is where it runs.

## Verification

| run | before | after |
| --- | --- | --- |
| `PoseidonTests.exe` | exit 127, 0 cases, 0 assertions, no summary | 3,996 cases: 3,765 passed, 3 failed, 228 skipped; 888,085 assertions: 888,076 passed, 9 failed |
| `PoseidonCoreTests.exe` | did not build (POST_BUILD copy of the absent `tests/fixtures/pbo`) | builds; 895 cases: 867 passed, 1 failed, 27 skipped; 10,232 assertions: 10,231 passed, 1 failed |

34 distinct fixtures were absent; every one is a documented omission family from
`FIXTURE-SYNTH-1.md` / `ASSET_SOURCES.md` (`p3d/`, `mlod/`, `pbo/`, `rtp`-ish `rtm/`,
`wrp/`, `jpg/`, `png/`, `audio/`, `qstream/`, `.sqs`/`.sqf`, evaluator books).

## Known failures left visible (not fixed here)

These are real findings, not fixture noise. Left red on purpose — a green run must mean
something.

1. `Dev panel controls all go through the search filter` — the GI panel in
   `Dev/Debug/DebugOverlay.cpp` calls `ImGui::SliderFloat/SliderInt/Combo/ColorEdit3/
   TextUnformatted/InputText/IsItemHovered` directly (≈1569-1613, 4874-4906) instead of
   the `Dev::` wrappers, so those rows escape the panel search filter.
2. `WGPU instance translation storage belongs exclusively to the consumer` —
   `DrainInstanceOps` now looks up `_modelIdOf.find` (offset ~1425) before the slot
   invalidation (offset ~4153), reversing the ordering invariant the guard encodes.
3. `Compat smoke: each generation's models are the revision that generation ships` —
   machine content, not code: the local Steam DayZ install's
   `structures_industrial.pbo` has 318 off-revision entries (`CementWorks/...`).
4. `PoseidonCoreTests`: `QIFStreamB FileExist` resolves
   `..\@voicemod\voice\Jari\reportstatus.wss` to a file inside the mod, i.e. a path that
   escapes its prefix still resolves.

## Follow-ups

- Add a C++ test job to CI (`build-windows` builds `PoseidonGame` only, so `ctest` has
  no test executables to run).
- `docs/proteus/BUILDING.md` documents `cmake --build --preset win-x64-clang-rwdi`, but
  `CMakePresets.json` defines no build presets; use `cmake --build build/<preset>`.
- The four findings above deserve their own tickets before this gate can be made
  blocking.
