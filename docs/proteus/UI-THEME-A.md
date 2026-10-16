# UI-THEME-A — Proteus art backdrop on Mods / Multiplayer / Configure (Phase A slice)

## Scope (proposal Phase A: theme propagation, no behavior change)

Three legacy retail screens kept their striped backgrounds and unthemed buttons while
the rest of the shell moved to the Proteus design system. This ticket puts Proteus
key art behind their existing retail controls:

- `DisplayMods` (`engine/Poseidon/UI/OptionsUIApp.cpp`) — behind `RscDisplayMods`
- `DisplayMultiplayer` (`engine/Poseidon/UI/DisplayUIMultiplayer.cpp`) — behind `RscDisplayMultiplayer`
- `DisplayConfigure` (`engine/Poseidon/UI/DisplayUI.cpp`) — behind `RscDisplayConfigure`

## Fix (+19 lines, no handler/label/ID/layout changes)

Each constructor now calls `EnableProteusTheme()` then `ProteusTheme::AddArtBackdrop(this,
cfg, 9790)` **before** its retail `Load()`, so the art washes paint first and the
retail lists paint on top. Retail `controlsBackground` frames hide automatically via
the existing `_proteusTheme` path (`LoadControlBackground`), and retail buttons are
created as `ProteusTheme::MenuButton` with no handler changes. IDC `9790` is
per-screen safe (retail IDCs are ~100; other themed screens reuse 9790 in their own
instances). `DisplayUIMultiplayer.cpp` gains the `ProteusTheme.hpp` include (the
other two files already had it).

## Verification

- `Poseidon` lib builds green (only pre-existing warnings).
- `PoseidonTests` links green; `[menu],[splash],[inGameUI]` 19 assertions / 6 cases pass.
- Broad `[UI] ~[font]` 220 cases: 212 pass / 2 fail / 6 skipped at the time — both
  failures the known pre-existing Czech-voice detector cases, later fixed by
  FIXTURE-VOICE-1 (this ticket's code is unrelated to voice detection).
- `check-all` OK (format hunk-clean on all three files).
- No GPU/menu-screenshot verification possible headless: `gpu_verified` stays false;
  owner confirms visually (Mods → Multiplayer → Controls) on the next GPU run.

## Follow-ups

- Remaining legacy screens (`DisplayServer`, `DisplayLogin`, `DisplayNewUser`,
  `DisplayRevert`, …) stay unthemed — same two-line pattern applies per screen.
- Phase B (focus order/readability) and Phase C (settings restructure) build on this.
