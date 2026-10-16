# Next-phase audit + roadmap proposal (2026-10-16)

Evidence from the tree, logs, and this week's verification runs. Web-tech
research (upscalers, browser packaging) is still pending from two delegated
lookups; the proposal below marks exactly where those inputs land.

## 1. Renderer: where it stands

- **Backends.** `engine/WgpuRenderer` (Rust/wgpu, DX12 + Vulkan) is the
  primary path; GL33 and dummy remain. The merged backend configures and
  links clean on `win-x64-clang-rwdi` (BACKEND-VERIFY-1: PoseidonTests links,
  PoseidonGame links, `ProteusGame.exe --help` exits 0).
- **Upscaling.** In-tree: `temporal.rs`/`temporal.wgsl` (TAA-style),
  `upscaler.rs`, `dlss_device.rs`/`dlss.rs`, `fsr4.rs`, FSR4 vendor loader at
  `fsr4.rs:217` libloading `amd_fidelityfx_loader_dx12.dll`. No vendor SDK is
  shipped; DLSS/FSR4 are load-time optional.
- **Known renderer facts (measured, not guessed).**
  - 2048-wide PAA in a picture control hangs the renderer
    (`wgr_bindless_texture_bind` validation; `CStatic` caps at 1024).
    Menu art is kept at 1024 wide. Not fixed in the engine.
  - Oct-14 crash dumps: 9 of 10 at identical `amdxc64.dll+0x7c66c0`
    (AMD DX shader compiler), 1 in `amd_fidelityfx_loader_dx12.dll+0x152d`.
    Verdict recorded: AMD DX12/FSR4 testing fallout, not the CLI11 startup
    crash. FSR4 path needs retesting after a driver/SDK update.
  - Shutdown windows of 30–70 s observed in logs (PBO unload); the window
    shows the last presented frame the whole time.

## 2. Browser / web: where it stands

- No web build exists. `docs/FORK_ROADMAP.md` Phase 5 scopes it
  (Emscripten/WASM, OPFS/IndexedDB asset streaming); `engine/Trident`
  (Rust integration harness) is the closest browser-adjacent code and is
  test-only. Nothing in the tree compiles to WASM today.
- Blocker inventory for a C++/Rust engine (pending web-research lookup):
  threading/SIMD/SharedArrayBuffer, `amdxc64`-class native-only paths
  (DLSS/FSR4 DLLs have no web story), asset streaming for gigabyte-scale
  game data. Proposal defers the web spike until after stability (Phase 3 P1)
  is closed.

## 3. Assets and menu shell: where it stands

- **Proteus shell owns the menu.** `DisplayMain` loads the remaster menu
  (`RscDisplayMainRemaster`, `resources/menu/splashLogo.hpp` merged via
  `resource-extra.cpp`), hides inherited controls, paints its own backdrop
  (`assets/ui`: `proteus_background.paa`, `vignette.paa`, `logo.png`) plus a
  `assets/menu/RoughSea.Intro` sea scene fallback (`StartRandomCutscene`).
- **FFUR leak (fixed this turn).** With a menu-overriding mod loaded
  (@ffsx2007: `RscDisplayMain.CM1` = `\ffur_fx\FFURGUImodern.paa`,
  fullscreen), hiding alone left the mod backdrop one
  visibility/teardown hole from painting — seen as the FFUR logo lingering
  over the quit flow. Fix: `DisplayMain` now also unbinds picture content
  (`SetText("")`, null-texture-safe per `CStatic::OnDraw`) from inherited
  menu controls before adding Proteus art. No mod art can paint regardless
  of hide-state holes.
- **Fixtures (structural gap).** `tests/fixtures/` omits `font/pac/jpg/
  pbo/rtm` dirs and all prebuilt binary `.paa`/`.p3d` (only generators
  remain, per `ASSET_SOURCES.md`). Consequence: `PoseidonTests` post-build
  copy fails and the full binary aborts (`GetTestFixturePath`); only
  non-binary-fixture tests run in this tree. Ticketed, not fabricated.
- **WW4/Kozlice (open).** `WW4 units do not replace the defaults` stays
  open. Mechanism mapped: `AddonSystem::ParseAllAddonConfigs` merges in
  `requiredAddons` dependency order via `Pars.Update` — a replacement pack
  only wins if ordered last and actually loaded. Root cause needs a live
  @PRMOD repro (recent sessions ran @ffsx2007, which never mentions kozl).

## 4. Proposal: next phase sequencing

1. **Close P1 stability.** (a) First `proteus.yml` CI run (still pending);
  (b) shutdown-time work (30–70 s PBO unload windows); (c) kozl live repro
  with @PRMOD loaded → fix or ticket with log evidence; (d) T18b fps
  re-baseline (owed since the merge).
2. **Upscaler decision (needs pending research).** Candidates: keep
  temporal/TAAU + CAS-style sharpen (works everywhere including a future
  web target) vs. optional native-only DLSS/FSR4 (no web story, AMD crash
  cluster counsels caution). Decide after the research lookup lands; do not
  ship vendor DLLs.
3. **Fixtures.** Author the missing binary fixtures locally via the
  sanctioned generators where they exist (`make_paa_tagg_fixtures.py`,
  `make_p3dm_fixtures.py`); ticket the rest. Goal: full `PoseidonTests`
  green in-tree, post-build copy included.
4. **Web spike (Phase 5 prep).** Only after 1–3: minimal WASM compilation
  probe of engine + Trident client to enumerate hard blockers before any
  packaging work.

## 5. Acceptance criteria for this proposal

- CI runs green on `proteus/main` at least once.
- `PoseidonTests` full-binary pass documented (or fixture-complete).
- kozl repro log attached to its ticket (fixed or findings-recorded).
- Upscaler choice recorded with research citations; no unsigned vendor
  binaries in the repo.

## 6. Addendum 2026-10-16: tickets landed since this proposal

- **UI-THEME-A** (proposal Phase A slice): Proteus art behind Mods /
  Multiplayer / Configure retail screens, behaviour-neutral. Remaining legacy
  screens + visual GPU check still open.
- **SHUTDOWN-T-1** (proposal item 1b): phase probes live; benchmark self-exit
  total is 289 ms, so the 30–70 s menu-quit path still needs a real
  launch → menu → quit capture. Same run should confirm MENU-FFUR-1 + the new
  backdrops visually.
- **TOOLCHAIN-1** (BACKEND-VERIFY-1 follow-up): VS-bundled LLVM candidates added.
- **FIXTURE-VOICE-1** (proposal item 3, partial): 2 sticking UI failures fixed
  by restoring omitted fixture names; broad `[UI]` is 214/0/6. Remainder:
  `font` decision + `pac/jpg/pbo/rtm` tickets.
- **New evidence against proposal item 1d**: the live benchmark was judged
  VALID with `admitted=0` — T18b re-baseline must wait for the admitted-count
  gate, or its numbers are menu-cutscene fps.
- **FIXTURE-FONT-1** (proposal item 3, continued): authored `dummy.ttf` +
  legacy FXY/PAA pair close the font gap; `copy-fixtures.cmake` makes staging
  warn-not-fail so the tests target builds green. Full binary now stops at
  `paa/synthetic_ai88_lzss.pac`; remaining gaps (`pac`, `jpg`, `pbo`, `rtm`,
  synthetic `paa`/`p3d`/`mlod`) each need the FIXTURE-VOICE-1 treatment.
