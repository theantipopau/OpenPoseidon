# Fork Roadmap

Source: `OPENPOSEIDON_KICKSTART.md` and `PROTEUS_CONTINUATION_PROMPT.md` (both kept outside this repo). Status key: done, in progress, blocked, not started.

Fork: `theantipopau/OpenPoseidon`, upstream `koosoli/OpenPoseidon`. Working branch: `dev/matt-fork-main`.

## Phase 1: Repository setup

| Item | Status | Notes |
| --- | --- | --- |
| Fork and clone | done | `origin` = fork, `upstream` = `koosoli/OpenPoseidon` |
| Create `dev/matt-fork-main` | done | Branched from `main` |
| Configure `win-x64-clang-rwdi` | done | Passes with the workarounds in [KNOWN_ISSUES.md](KNOWN_ISSUES.md) |
| Build `PoseidonGame` | done | Green (`EXIT=0`) via direct `cmake --build`. Earlier wrapper run failed; see KNOWN_ISSUES. Produces `dist/x64-win-rwdi/ProteusGame.exe` |
| Engine launches, dev panel opens | blocked | Binary runs (`--help` exits 0). Window launch aborts: Remastered fonts missing. Needs Steam retail and Demo installs (see KNOWN_ISSUES) |
| Docs structure | done | This folder |
| Asset audit | done | [ASSET_PIPELINE.md](ASSET_PIPELINE.md); full PBO list kept outside repo |
| Mod load order assessed | done | See P1.1 |

Phase 1 is green on build. The launch item stays open, so the Phase 2 checkpoint is still blocked on the installs.

## Phase 2: Asset integration

| Item | Status | Notes |
| --- | --- | --- |
| 2.1 Source assets located | done | Local OFP Evos folder, outside repo |
| 2.2 Asset audit | done | [ASSET_PIPELINE.md](ASSET_PIPELINE.md) |
| 2.3 Layered loading | configured | `mods.json` + `scripts/Start.ps1` produce `--mods-dir` and `--mod=@ww4mod25;@Evos`. Not yet run end to end |
| 2.4 Blender addon | documented | `apps/tools/BlenderAddon/` not modified |
| Phase 2 checkpoint report | pending | Needs a running build and a retail install |

## Phase 3: Engine improvements

Order: P1.1, then P1.5, then the rest of P1.

### P1 Stability and usability

- **P1.1 Mod load order.** Largely exists. `engine/Poseidon/Core/ModSystem.cpp` and `ModCollection.cpp` mount an ordered list from `--mod=`. The last-listed mod wins. RES mounts below the default packs. Load order is logged at INFO (`Mod path set`, `Loaded mod`). Added this phase: `mods.json` (gitignored) read by `scripts/Start.ps1`, with `mods.example.json` committed. Still to confirm: that the load-order lines reach the dev console, not only the log file.
- **P1.5 PBO extraction utility.** Elevated from Phase 4.3 per the continuation brief. Existing code to reuse first: `engine/Poseidon/Core/ModArchive.cpp`, `ModInstall.cpp`. No `tools/pbo-extract` exists yet. The 341 PBOs make this immediately relevant. Spec is in Phase 4.3 below.
- **P1.2 4K/ultrawide HUD scaling.** Not started.
- **P1.3 Keybinding system.** Not started.

### P2 Visual quality

- **P2.1 Texture upscaling hook / anisotropic filtering / sharpening.** Not started.
- **P2.2 Shadow quality scaling.** Not started.
- **P2.3 FOV slider.** Not started.

### P3 Gameplay

- **P3.1 3D iron sights.** Not started.
- **P3.2 AI line-of-sight.** Not started. Existing LOS code not yet reviewed.
- **P3.3 Join-in-progress multiplayer.** Not started. `mserver/` not yet reviewed.

### P4 Tooling

- **P4.1 P3D re-exporter.** Not started.
- **P4.2 PAA batch converter.** Not started. Check existing PAA code under `tools/` and `engine/` first.
- **P4.3 PBO extractor.** Merged with P1.5. Input: folder of PBOs. Output: extracted tree, preserving internal paths. Supports signed and unsigned OFP/CWA v1.96 PBOs. CLI: `pbo-extract input.pbo output_dir/`. Location: `tools/pbo-extract/`.

## Phase 4: CWR3 asset conversion pipeline

Not started. Per the kickstart, do not start until Phases 1–3 are complete.

## Phase 5: Web build

Proposal only. Not started. Not authorised for implementation.

- Target: the engine runs in a browser. The data source is the open decision. See [ASSET_PIPELINE.md](ASSET_PIPELINE.md#web-build-target-phase-5-proposal-only).
- Standing rules from Phase 3 onward: no hardcoded Windows paths, no platform-specific file I/O outside the abstraction layer, streaming-friendly asset loading, and every WASM-incompatible API logged in KNOWN_ISSUES.
- The WGPU renderer already targets WebGPU, and the Rust code compiles to WASM. The C++ layer is the main bridge. Flag any C++ code that uses Windows-only APIs.

## Decisions

Resolved:
- Mod licence: WW4 and Evos are local-use only. Confirmed by Matt.
- Git identity: `theantipopau`, using the GitHub noreply address for the account. The docs commit is re-attributed to this identity (now `3675360`). It is local and unpushed.
- Phase 4.3 elevated to P1.5.

Open:
1. **CWA data source for the web build.** Public freeware is not supported by public sources. Either clear the asset licence in writing, or limit Phase 5 to user-supplied data.
2. **Retail install not found.** The Steam registry lookup for app 65790 returns nothing on this machine. Set `data_path` in `mods.json` once the install location is known.
3. **Git email.** The noreply address is used. Say if you want a different address on commits.
