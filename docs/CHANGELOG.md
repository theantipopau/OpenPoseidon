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
- Sub-screen pass, started:
  - Shared `ProteusTheme` module (`engine/Poseidon/UI/ProteusTheme.*`). A screen opts in with `ControlsContainer::EnableProteusTheme()`. Its buttons become the themed button, and its retail background frames load hidden.
  - Main menu now uses the shared builders. MODS and LOCAL MAPS are removed from it. LOCAL MAPS crashes in `ScanLocalVehicles`, and MODS goes nowhere.
  - Options main page restyled as a centred panel with a cyan top border. The 3D laptop is hidden. Its links are Proteus items routed to the same handlers. BACK and Escape exit to the main menu.
  - Options sub-pages (Video, Audio, Controls, Difficulty) are not restyled yet.
  - Campaign Select restyled. Every campaign is listed on the left and scrolls. Selecting one shows its name and overview on the right, with the mission history beneath. BACK, DIFFICULTY, REPLAY and BEGIN run along the bottom. The retail 3D book is not loaded, so the page is built from 2D controls. Overviews are each campaign's own HTML, so some show only a picture. The statistics block is not shown in this layout.
  - Single Missions restyled, on the same pattern as the campaign page. The mission list is on the left, and folders open in place with `..` to go back. The selected mission's overview is on the right. BACK, DIFFICULTY, RESUME (when a save exists) and PLAY run along the bottom. Escape returns to the main menu. Play is not yet tested in this pass.
  - The campaign and single-mission pages share their layout helpers (`ProteusControl`, `ProteusColour`, `ProteusRule`, `AddHtmlStyles`) in `OptionsUIImpl.cpp`.
  - Still pending: Multiplayer, Mission Editor launcher, and the resolution tiers. `docs/UI_SCREENS.md` has the full table.
