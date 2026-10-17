# UI-THEME-C — Proteus theme on the mission-editor dialogs (Phase A continued)

## Scope (UI-THEME-A/B follow-up: editor popups, no behavior change)

The mission editor's dialogs kept their striped retail backgrounds and unthemed
buttons. This ticket applies the Proteus theme to all 10 editor popup
constructors:

- `DisplayArcadeUnit`, `DisplayArcadeMarker`, `DisplayArcadeSensor`,
  `DisplayArcadeEffects` (defined in `UIArcadeMarker.cpp`),
  `DisplayArcadeWaypoint`, `DisplayArcadeGroup`,
  `DisplayTemplateSave`, `DisplayTemplateLoad`, `DisplayIntel`
  (`engine/Poseidon/UI/Map/UIMap.hpp` inline ctors + `UIArcade.cpp`,
  `UIArcadeMarker.cpp`, `UIArcadeWaypoint.cpp`, `UIMapDialogs.cpp`)
- Plus `DisplayInsertMarker` (`UIMapDialogs.cpp`)

## Fix (+14, no handler/label/ID/layout changes)

Each constructor calls `EnableProteusTheme()` before its retail `Load()`, so
retail buttons are created as `ProteusTheme::MenuButton` and retail
`controlsBackground` frames hide via the existing `_proteusTheme` path
(`UIContainers.cpp:315,524`).

Deliberately **no art backdrop**: unlike the fullscreen screens of A/B, these
dialogs are popups over the live 3D editor (`_enableDisplay = false`) — a
full-bleed wash would cover the scene the user is editing. The fullscreen
editor canvas itself (`DisplayMapEditor`, just sets the cursor) and the
in-game HUD (`DisplayMission`) are out of scope.

Format note: `UIMap.hpp` is legacy tab-indented (pre-existing drift); the new
lines use spaces per `.clang-format` (`UseTab: Never`) so the hunk-scoped
format gate passes without touching surrounding lines.

## Verification

- `Poseidon` lib + `PoseidonTests` build green (0 errors).
- `[menu],[splash],[inGameUI]` 19/6 pass; broad `[UI]` 230 cases 224/0/6 exit 0.
- `check-all` OK (format hunk-clean on all 5 files).
- No GPU/editor-screenshot verification possible headless: `gpu_verified` stays
  false; owner confirms visually (open editor → unit/marker/group dialogs) on
  the next GPU run.

## Follow-ups

- Owner visual check of the themed editor dialogs on GPU.
- Fullscreen editor canvas theming (if wanted) is a bigger piece of work and
  needs a design decision first (the 3D view must stay unobstructed).
