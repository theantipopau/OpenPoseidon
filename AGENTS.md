# AGENTS.md — Proteus agent guide

Proteus is a fork of OpenPoseidon (community-modernised Poseidon engine) at
`theantipopau/OpenPoseidon`. You make small, evidence-backed, reversible improvements.
Never add AI features to the engine or its tools.

## Layout

- `engine/Poseidon/` — C++ engine (sim, world, UI, assets, formats).
- `engine/WgpuRenderer/` — C++ backend glue + `rust/` (`wgpu_renderer` crate: all GPU passes).
- `engine/PoseidonGL33/` — legacy GL backend. `engine/PoseidonOpenAL/` — audio.
- `engine/Trident/` — test orchestrator (Rust). `mserver/` — master server (Rust).
- `apps/cwr/Game/` — game target `PoseidonGame`, shipped as `ProteusGame.exe` → `dist/`.
- `scripts/` — PowerShell build/launch/measure helpers (`Build.ps1`, `Start.ps1`, `CwaCommon.ps1`).
- `docs/proteus/` — run state: `STATE_OF_REPO.md`, `ledger.yaml` (single ticket ledger),
  `RUN_LOG.md`, `CHANGELOG_BACKGROUND.md`, per-ticket `<ID>-<name>.md` records.
- You need your own game data install; retail/mod data is never in the repo and paths come
  only from env/config (`mods.json`, gitignored — copy `mods.example.json`).

## Build and test (Windows, what actually works)

```powershell
$env:VCPKG_ROOT = 'E:\dev\vcpkg'   # full clone, not --depth 1
$env:VCPKG_KEEP_ENV_VARS = 'LLVM_ROOT'
$env:LLVM_ROOT = '<VS2022>\VC\Tools\Llvm\x64'
cmake --build build/win-x64-clang-rwdi --target PoseidonGame   # preset: win-x64-clang-rwdi
```

Prefer direct `cmake --build` over `scripts/Build.ps1` (the wrapper can abort on cargo
stderr warnings despite a successful build). Details and workarounds: `docs/KNOWN_ISSUES.md`.

```powershell
ctest --test-dir build/win-x64-clang-rwdi --output-on-failure   # C++ tests (build first)
cargo test -p wgpu_renderer --lib -- <filter>                   # Rust unit tests, headless OK
```

GPU/device tests need a real adapter; CPU-only tests (naga parse/validate, shader-source
contracts, maths) run anywhere. Launch: `scripts/Start.ps1 --window` (reads `mods.json`).

## Branch model

- Integration: `proteus/main`. Ticket branches fork from its tip: `proteus/<TICKET>-<short-name>`.
- One ticket = one branch = one small PR. Merge done tickets with `--no-ff`, message has the ID.
- Never push to or open PRs against upstream `koosoli/OpenPoseidon`. Never commit retail/mod data.

## Standing rules

- Formats, missions, saves, MP netcode and simulation determinism are untouchable unless the
  ticket says otherwise. Dev Tools/Zeus stay disabled in MP.
- Every visual/behavioural change ships a toggle restoring legacy behaviour; state the default
  in `ledger.yaml`. No drive-by refactors. ~400-line cap per ticket (excl. tests/docs) — split if bigger.
- New code: fix clang-tidy/shader warnings in your hunks only; never touch pre-existing
  rustfmt/clang-format drift elsewhere; never suppress lints or weaken/skip tests.
- New assets need a `assets/registry.yaml` entry + licence (G2). Provenance for ported code goes
  in `docs/proteus/PROVENANCE.md` (source repo, hash, path, licence) before merge.
- User-facing text says "Proteus", never "Operation Flashpoint"/"Arma" as product names.

## Definition of Done (per ticket)

1. Build green on runnable presets + `cargo test` green for affected crates.
2. CPU-side tests for new behaviour where possible; otherwise the record says why (GPU-only).
3. `docs/proteus/<ID>-<name>.md`: problem, approach, files, toggle + default, evidence, risks,
   follow-ups, "Verify on GPU" checklist (exact T18/T19 commands, good vs bad).
4. `ledger.yaml` updated (`done-unverified` after merge; only the owner sets `verified`).
5. Lines appended to `CHANGELOG_BACKGROUND.md` and `RUN_LOG.md`.

## Toggle convention

- Startup-fixed env gates (`WGR_*`, read once, logged): A/B-safe — two runs of one binary differ
  in exactly one term. `0`/`1` values restore the legacy path exactly.
- Live settings (`GtaoSettings`, dev-panel controls, `graphics.cfg`): `enabled=false` gates a pass;
  quality knobs document their legacy value. `render_gtao`-style functions early-return when off.
- No GPU here: implement, cover CPU-side, mark `done-unverified`, add the GPU checklist.
