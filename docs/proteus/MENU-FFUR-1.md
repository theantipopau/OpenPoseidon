# MENU-FFUR-1 — Proteus shell neutralizes inherited menu artwork (FFUR leak)

## Symptom (user report, confirmed)

`F F U R` appears as a full-screen splash during game shutdown, with
`@ffsx2007` loaded. Reporter confirmed: splash-during-exit (window gone or
frozen), FFUR/SLX test mod loaded.

## Root cause

- The only fullscreen FFUR artwork wired into any display class is
  `RscDisplayMain.CM1` = `\ffur_fx\FFURGUImodern.paa` in the local-only,
  git-ignored `@ffsx2007/Bin/RESOURCE.CPP` (line 7471). No `FFUR` string
  exists anywhere in engine source; there is no quit splash, no outro
  runner, and the shutdown progress splash draws Proteus art only
  (`ProteusTheme::DrawBackdropArt`, `assets/ui/`).
- With a menu-overriding mod loaded, `DisplayMain` loads the mod's
  `RscDisplayMain` (`IsMenuOverriddenByMod()` true) and, under
  `proteusMenu=true`, hides every inherited control and paints its own
  Proteus backdrop. Hiding alone leaves the mod's fullscreen backdrop one
  visibility/teardown hole away from painting — the FFUR logo lingering
  over the quit flow (shutdown windows last 30–70 s on the last frame).

## Fix (`engine/Poseidon/UI/OptionsUIApp.cpp`, +35)

In the `proteusMenu` takeover block, after hiding inherited controls and
before adding Proteus art: unbind picture content (`SetText("")`) from
every inherited background/foreground `CStatic` that is `ST_PICTURE` /
`ST_TILE_PICTURE`, plus a `.paa`-suffix fallback for style-inheritance
edge cases. With no texture bound nothing can paint regardless of
hide-state holes; buttons/IDs stay valid (controls are kept, not removed).

Safety chain (all read, not assumed): `FindPicture("")` early-returns
("used for nullptr texture"), `GlobLoadTexture("")` returns nullptr,
`CStatic::OnDraw` null-guards `_texture`, and `SetText` clears `_textCls`
so no language refresh resurrects the art.

## Verification

- `Poseidon` lib + `PoseidonTests` link green (only pre-existing warnings).
- `check-format` OK hunk-scoped; `check-all` green after `gen-roadmap`
  regen (roadmap staleness was pre-existing HEAD drift).
- Targeted: `[menu],[splash],[inGameUI]` → 19 assertions / 6 cases pass.
- Broad UI (`[UI]`, 220 cases): 212 pass, 2 fail, 6 skipped. Both failures
  (`test_mission_language_detector.cpp:124` Czech-voice flag; temp-dir
  case-sensitivity case) proven pre-existing: stash-built pre-change binary
  fails identically (same line, same 1-fail/1-pass isolated split).
- Full binary still impossible in-tree: `GetTestFixturePath` aborts on the
  omitted binary fixtures (`font/pac/jpg/pbo/rtm` dirs + prebuilt
  `.paa`/`.p3d`; `ASSET_SOURCES.md` documents the omission). Manually
  staged the existing fixture dirs into the build dir (env-only, no repo
  change) to run everything runnable.
- No GPU/menu-screenshot verification possible headless: `gpu_verified`
  stays false; owner confirms visually on next run.

## Follow-ups

- Kozlice/WW4 replacement stays open: merge mechanism mapped
  (`ParseAllAddonConfigs` dependency order → `Pars.Update`; a replacer only
  wins if ordered last and loaded), needs a live @PRMOD repro.
- Pending web-tech research (upscalers, browser packaging) feeds the
  proposal in `NEXT_PHASE_AUDIT.md`.
