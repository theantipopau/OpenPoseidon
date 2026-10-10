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
