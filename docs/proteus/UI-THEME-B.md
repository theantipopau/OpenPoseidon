# UI-THEME-B — Proteus art backdrop on Server / Login / NewUser / Revert (Phase A continued)

## Scope (UI-THEME-A follow-up: remaining legacy screens, no behavior change)

UI-THEME-A covered Mods / Multiplayer / Configure. This ticket applies the same
two-line pattern to the four remaining legacy screens named in its follow-ups:

- `DisplayServer` (`engine/Poseidon/UI/DisplayUI.hpp`) — behind `RscDisplayServer`
- `DisplayLogin` (`engine/Poseidon/UI/DisplayUI.hpp`) — behind `RscDisplayLogin`
- `DisplayNewUser` (`engine/Poseidon/UI/DisplayUIMenus.cpp`) — behind `RscDisplayNewUser`
- `DisplayRevert` (`engine/Poseidon/UI/OptionsUIImpl.cpp`) — behind `RscDisplayRevert`

## Fix (+26/-1 lines, no handler/label/ID/layout changes)

Each constructor now calls `EnableProteusTheme()` then
`ProteusTheme::AddArtBackdrop(this, cfg, 9790)` **before** its retail `Load()`,
so the art washes paint first and the retail controls paint on top. Retail
`controlsBackground` frames hide automatically via the existing `_proteusTheme`
path, and retail buttons are created as `ProteusTheme::MenuButton` with no
handler changes. IDC `9790` is per-screen safe (other themed screens reuse it in
their own instances). `DisplayUI.hpp` and `DisplayUIMenus.cpp` gain the
`ProteusTheme.hpp` include (`OptionsUIImpl.cpp` already had it).

## Verification

- `Poseidon` lib links green (only pre-existing warnings; lib timestamp newer
  than all three edited sources).
- `PoseidonTests` links green; `[menu],[splash],[inGameUI]` 19 assertions /
  6 cases pass.
- Broad `[UI]` 230 cases: 224 pass / 0 fail / 6 skipped, exit 0 (fully green).
- `check-all` OK (format hunk-clean on all three files; roadmap current at
  20 tickets after regen).
- No GPU/menu-screenshot verification possible headless: `gpu_verified` stays
  false; owner confirms visually (Host → Login → New user → Revert) on the next
  GPU run.

## Follow-ups

- Phase B (focus order/readability) and Phase C (settings restructure) build on
  the completed Phase A coverage (7 screens: Mods, Multiplayer, Configure,
  Server, Login, NewUser, Revert).
