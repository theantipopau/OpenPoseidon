# UI-THEME-C1 — Editor popups keep the retail backdrop under themed buttons

## Scope (UI-THEME-C follow-up, no behavior change)

UI-THEME-C applied `EnableProteusTheme()` to all 10 editor popup constructors.
That path hides the retail `controlsBackground` frames — correct for fullscreen
screens with Proteus art, wrong for popups over the live 3D editor, which have
no art backdrop of their own and ended up with floating buttons and no panel
behind them.

## Fix (+10, no handler/label/ID/layout changes)

- `UIControls.hpp`: new `EnableProteusPopupTheme()` — sets `_proteusTheme`
  (themed `MenuButton`s via the existing `OnCreateCtrl` path) plus
  `_proteusKeepRetailBackdrop`, which exempts the dialog from the
  `LoadControlBackground` hide in `UIContainers.cpp:315`.
- All 10 popup ctors (`UIArcade.cpp`, `UIArcadeMarker.cpp`,
  `UIArcadeWaypoint.cpp`, `UIMap.hpp` x6, `UIMapDialogs.cpp`) call the popup
  variant. Button creation (`UIContainers.cpp:524`) stays on `_proteusTheme`
  alone — popups want themed buttons, just with the retail frame kept.

Also in this ticket: two gate leftovers fixed so `check-all` passes on the
branch — `ROADMAP.md` regenerated (DX12-1 ledger entry, +6) and rustfmt's
`main_camera_tuple`/`demand_view_snapshot` reflow in `lib.rs`
(formatting-only, pre-existing drift exposed by adjacency to the DX12-1 insert).

## Verification

- `check-all`: OK (assets 72/72, roadmap current, selftests PASS, format
  hunk-clean on 19 changed files).
- `Poseidon` lib links green (only the pre-existing
  `-fdelayed-template-parsing` warning).
- `[menu],[splash],[inGameUI]` 19 assertions / 6 cases pass; broad `[UI]`
  230 cases 224 passed / 0 failed / 6 skipped, exit 0.
- No GPU/editor-screenshot verification possible headless: `gpu_verified`
  stays false; owner confirms visually (editor dialogs now show themed buttons
  on the dark retail panel) on the next GPU run.

## Follow-ups

- Owner visual check of the themed editor dialogs on GPU.
- Fullscreen editor canvas theming needs a design decision first (unchanged).
