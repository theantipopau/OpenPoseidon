# Asset Pipeline

Status: Phase 2.2 audit complete. Phase 2.3 layering is configured only by command line; nothing is integrated or committed yet.

## Source

Local OFP Evos install folder (path kept outside this repository). The folder is a full Operation Flashpoint / Cold War Assault install with mods. It contains base game data, bundled tools and DLLs, and two mod packs.

Nothing from this folder may be committed. Layers are mounted from local paths only.

## Inventory summary

| Type | Count | Notes |
| --- | --- | --- |
| PBO | 341 | Models, textures, configs and missions are packed in PBOs |
| PAA | 114 | 85 under `@Evos`, 25 under `Templates`, 4 under `SPTemplates` |
| PAC | 11 | All under `@Evos` |
| P3D | 1 loose | Most model data is inside PBOs, not loose files |
| BMP / OGG / WRP / CSV / SQS / SQM | 421 / 67 / 7 / 33 / 136 / 32 | Mixed; mostly base data and mission content |
| DLL / EXE | 43 / 38 | Game executables, DirectX wrappers and bundled tools |

Loose P3D/PAA/PAC is much rarer than the kickstart assumed. Evos HQ content is almost entirely inside PBOs, so Phase 4.3 (PBO extraction) matters more than loose-file handling.

## PBO categories

| Folder | PBOs | Category | What it replaces or overlaps | Status |
| --- | --- | --- | --- | --- |
| `@Evos/Addons/` | 56 (51 excl. `Optional 102 version/`) | Evos HQ models, textures, sounds, editor tools | 10 basenames overlap `Res/AddOns/`: 6G30, ABox, Bizon, G36A, Kozl, Mm-1, Noe, Steyr, Su25, XMS | Candidate layer 2 (override) |
| `@ww4mod25/addons/` | 46 | WW4 infantry, weapons, anims, editor | No overlap with `Res/AddOns/` | Candidate layer 1 (base mod) |
| `@Evos/Campaigns/` | 16 | Evos campaigns | Mission content, not a layer | Not layered |
| `@Evos/Dta/` (incl. `HWTL/`) | 6 | Evos data overrides (anim, fonts, voice, HWTL data) | Overrides base `Dta/` | Review before layering |
| `@ww4mod25/dta/` | 3 | WW4 anim and script data | Overrides base `Dta/` | Review before layering |
| `@Evos/Optional files/` | 4 | Optional tools and content | Not game content | Not layered |
| `Bonus/WW4 anims Only/` | 4 | Optional WW4 animation add-on | Optional | Not layered |
| `Dta/`, `Dta/HWTL/`, `Res/Dta/` | 15 + 3 + 12 | Base game data (terrain, sound, data3D, anims, fonts) | Base layer | Base only; never copy |
| `Addons/`, `Res/AddOns/` | 22 + 26 | Original OFP addons | Base layer | Base only; never copy |
| `Campaigns/`, `Res/Campaigns/` | 2 + 1 | Original campaigns | Base layer | Base only; never copy |
| `Missions/`, `MPMissions/`, `Templates/`, `SPTemplates/` | 18 + 62 + 31 + 5 | Missions and templates | Not mods | Not layered |

Overlap method: PBO basename, case-insensitive, compared against `Res/AddOns/`. `@ww4mod25` and `@Evos` overlap in three addon names only: `largeditor`, `ww4_modanim`, `ww4_pistolanim`. These are the only conflicts that need a deliberate override decision.

## Layering (Phase 2.3)

The engine already implements ordered layers (assessment in [FORK_ROADMAP.md](FORK_ROADMAP.md#p11--mod-load-order-system)). Last-listed mod wins, and RES (base content) mounts below the default packs.

```
-mod=@ww4mod25;@Evos
```

This matches the kickstart layer order: base game, then WW4 (layer 1), then Evos (layer 2, override). No code change is needed for the mount order.

## Licensing flags (stop-and-report items)

No licence file gives redistribution or modification rights for the mod content. Local runtime loading is fine. Committing or redistributing any derived content is not, until someone with the rights confirms otherwise.

| Source | Finding | Action |
| --- | --- | --- |
| WW4 Modpack 2.5 (`Readme-WW4modpack25.txt`) | Community mod credits only. No redistribution or modification terms. | Local use only. Ask the authors before any redistribution. |
| `@Evos` (Evos MOD) | No licence or readme file in the mod folder. | Local use only. Ask the authors before any redistribution. |
| Base game data (`Dta/`, `Res/`, root `Addons/`, `Campaigns/`) | Bohemia Interactive retail content. | Never commit or redistribute. |
| Bundled tools (`MakePBO.exe`, `UnPBO.exe`, `ODOL_Explorer`, `WrpTool`, DirectX installers, `d3d8.dll`, `dxwrapper.dll`, `fwatch`, Bandicam) | Third-party binaries. Licence not checked. | Do not copy into the repo. Use only as local tools if needed. |

Decision needed from Matt before Phase 2.3 integration proceeds: confirm that local-only mounting of these two mods is acceptable under the kickstart rules, and whether any Evos or WW4 derived content may ever be published.

## Blender addon (Phase 2.4)

`apps/tools/BlenderAddon/` is documented here and not modified. A P3D re-exporter is needed for Phase 4.1.

## Game data source (CWA)

Local development use of a legally owned Arma: Cold War Assault install is the working assumption. Matt has confirmed ownership.

**Correction to an earlier premise.** The continuation brief stated that Bohemia released CWA as freeware, so its data could be hosted server-side. Checking that against public sources does not support it:

- CWA is a commercial release. Bohemia has run free giveaways before, including a 20th-anniversary offer on GOG. That is not a standing freeware release. ([overclock3d](https://overclock3d.net/news/software/arma-cold-war-assault-is-currently-available-for-free-on-gog/))
- Steam offers a free Remastered demo. The full data and campaigns are paid. ([Steam store](https://store.steampowered.com/app/65790/?cc=ar), [GamingOnLinux](https://www.gamingonlinux.com/2026/06/arma-cold-war-assault-remastered-out-with-a-demo-and-its-open-source/))
- Bohemia's installer notice limits use to people with a valid licence or CD key for a qualifying title. ([BI store](https://store.bistudio.com/products/arma-cold-war-assault-mac-linux-digital-steam-key))
- Reporting describes the engine as GPL-3.0-or-later and the game assets as APL-SA. The asset terms still restrict redistribution. ([GamingOnLinux](https://www.gamingonlinux.com/2026/06/arma-cold-war-assault-remastered-out-with-a-demo-and-its-open-source/))

Conclusion: serving the retail data from a public web server is redistribution and is **not authorised**. Do not implement it, and do not describe the data as freeware in any doc. The version "1.99" in the brief is unverified and not recorded here.

## Web build target (Phase 5, proposal only)

Goal: a browser-playable build. Hosting is the open question. Options, with the decision still with Matt:

1. A WASM engine that loads data the user supplies from their own legal install. Nothing is redistributed. This is the default that needs no licence decision.
2. Server-hosted data. Blocked until the asset licence is clarified in writing.

```
Web host
├── /engine/        WASM build of the engine
├── /assets/        Supplied at runtime, or hosted only after licence clearance
└── /index.html     Shell and loading screen
```

Design rules for every phase from now on (see [FORK_ROADMAP.md](FORK_ROADMAP.md#phase-5-web-build)):

- No hardcoded Windows paths. Use the engine's virtual file system.
- No platform-specific file I/O outside the abstraction layer.
- New asset loading should work with HTTP range-request streaming, not only local disk reads.
- Log every WASM-incompatible API in [KNOWN_ISSUES.md](KNOWN_ISSUES.md).
