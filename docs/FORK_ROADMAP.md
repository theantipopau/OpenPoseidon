# Fork Roadmap

Source: `OPENPOSEIDON_KICKSTART.md` (kept outside this repo). Status key: done, in progress, blocked, not started.

Fork: `theantipopau/OpenPoseidon`, upstream `koosoli/OpenPoseidon`. Working branch: `dev/matt-fork-main`.

## Phase 1: Repository setup

| Item | Status | Notes |
| --- | --- | --- |
| Fork and clone | done | `origin` = fork, `upstream` = `koosoli/OpenPoseidon` |
| Create `dev/matt-fork-main` | done | Branched from `main` |
| Build `win-x64-clang-rwdi` | in progress | Configure running; see [KNOWN_ISSUES.md](KNOWN_ISSUES.md) for the setup workarounds |
| Engine launches, dev panel opens | blocked | Needs a successful build and a local game install |
| Docs structure | done | This folder |
| Asset audit | done | [ASSET_PIPELINE.md](ASSET_PIPELINE.md) |
| Mod load order assessed | done | See P1.1 below |

## Phase 2: Asset integration

| Item | Status | Notes |
| --- | --- | --- |
| 2.1 Source assets located | done | Local OFP Evos folder, outside repo |
| 2.2 Asset audit | done | [ASSET_PIPELINE.md](ASSET_PIPELINE.md); full PBO list kept outside repo |
| 2.3 Layered loading | configured only | Command line: `-mod=@ww4mod25;@Evos`. Integration blocked on a licence decision |
| 2.4 Blender addon | documented | `apps/tools/BlenderAddon/` not modified |

## Phase 3: Engine improvements

### P1 Stability and usability

- **P1.1 Mod load order.** Largely exists. `engine/Poseidon/Core/ModSystem.cpp` and `ModCollection.cpp` mount an ordered list from `--mod=`. The last-listed mod wins. RES mounts below the default packs. Load order is logged at INFO level (`Mod path set`, `Loaded mod`). Gaps against the kickstart: no `mods.cfg`/`mods.json` config file, and the log goes to the engine log, not necessarily the dev console (not yet verified). Next step: confirm the dev-console path, then add the config file.
- **P1.2 4K/ultrawide HUD scaling.** Not started.
- **P1.3 Keybinding system.** Not started.

### P2 Visual quality

- **P2.1 Texture upscaling hook / anisotropic filtering / sharpening.** Not started.
- **P2.2 Shadow quality scaling.** Not started.
- **P2.3 FOV slider.** Not started.

### P3 Gameplay

- **P3.1 3D iron sights.** Not started.
- **P3.2 AI line-of-sight.** Not started. The README notes "Improved AI" with gunfire hearing, orders and suppression. Existing LOS code has not been reviewed yet.
- **P3.3 Join-in-progress multiplayer.** Not started. The README notes MP is a separate path; `mserver/` not yet reviewed.

### P4 Tooling

- **P4.1 P3D re-exporter.** Not started.
- **P4.2 PAA batch converter.** Not started. Check existing PAA code under `tools/` and `engine/` first.
- **P4.3 PBO extractor.** Not started. Check existing PBO code under `tools/` and `engine/` first.

## Phase 4: CWR3 asset conversion pipeline

Not started. Per the kickstart, do not start until Phases 1–3 are complete.

## Next decisions for Matt

1. Confirm local-only mounting of the WW4 and Evos mods is acceptable while their licences are unclear.
2. Confirm the Phase 1 checklist can be marked complete once the build and launch are verified.
