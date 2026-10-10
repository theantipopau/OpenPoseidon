# Changelog

Running log of changes made by agents on this fork. Newest first.

## Unreleased

- Forked `koosoli/OpenPoseidon` to `theantipopau/OpenPoseidon`, cloned locally, created branch `dev/matt-fork-main`.
- Added fork docs: `FORK_ROADMAP.md`, `ASSET_PIPELINE.md`, `KNOWN_ISSUES.md`, `CHANGELOG.md`.
- Audited the local OFP Evos asset folder (341 PBOs). Results are in `ASSET_PIPELINE.md`. Nothing from the folder was copied into the repo.
- Assessed P1.1 mod load order: the engine already mounts ordered mods (`ModSystem`, `ModCollection`).
- Configured `win-x64-clang-rwdi`: configure passes.
- Added `mods.example.json` (committed) and `mods.json` (gitignored). `scripts/Start.ps1` reads `mods.json` for `data_path`, `mods_dir` and the `--mod=` list.
- Added anchored `.gitignore` rules for game and mod folders at the repo root.
- Corrected the freeware premise for CWA. The web architecture is recorded as a proposal, not an authorised plan.
- Elevated the PBO extractor (P4.3) to P1.5. Added a Phase 5 proposal section.
- Re-attributed the docs commit to the `theantipopau` identity.
- Built `PoseidonGame` (`win-x64-clang-rwdi`): green. Output in `dist/x64-win-rwdi/`.
- Launch probe against the local OFP Evos folder: binary runs, window launch aborts on missing Remastered fonts. Mod mounting not yet verified. See KNOWN_ISSUES.
- Main menu overhaul (`docs/PROTEUS_UI_OVERHAUL_PROMPT.md`), one commit per fix:
  - Window title is `Proteus [WGPU]` (and `Proteus [GL33]`), keeping the renderer tag.
  - Full-bleed background art, with a generated dark edge vignette over it.
  - Glass sidebar on the left: 32% of the window, navy at 82% opacity, 3px cyan left border. Replaces the right-hand grey panel.
  - Barlow Condensed Regular and SemiBold (SIL OFL 1.1) in `assets/fonts`, registered as menu faces and resolved beside the executable.
  - Buttons: transparent, left-aligned. Focus (mouse or keyboard) shows a cyan wash, a 3px cyan accent and a 4px label slide. The slide is a step, not an animation.
  - Logo: 80x80, centred in the sidebar header, with the PROTEUS title below and a 1px cyan separator above the buttons.
  - Version string moved to the sidebar footer, muted, as `PROTEUS  v<build>`. The bottom-bar text is removed.
  - Logo registered at `assets/ui/logo.png` (512px, downscaled from the 1254px source). The superseded `proteus_logo` files are removed.
- Deviations from the spec, because the menu is built on the engine's Control system, not ImGui:
  - ImGui is not used in the menu path. Its draw calls are replaced by `CStatic` controls and a `CButton` subclass.
  - Hover is tracked through keyboard and mouse focus, since controls have no separate hover state.
  - Letter-spacing is not applied; the engine has no tracking.
  - The font files are committed (OFL allows it), so no download script is needed.
- Engine changes made for the overhaul: `FindPicture` accepts absolute paths to existing files, and the font resolver finds faces beside the executable. The required-font check accepts mod fonts.
