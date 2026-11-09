# G3c-panel-wrappers — the GI dev-panel tab draws through the `Dev::` wrappers

Status: done-unverified (UI change; the compiling path and the source invariant are
green, the rendered filter needs a GPU run)

## Problem

`FIXTURE-SKIP-1` left this visible as known failure 1:

> `Dev panel controls all go through the search filter` — the GI panel in
> `Dev/Debug/DebugOverlay.cpp` calls `ImGui::SliderFloat/SliderInt/Combo/ColorEdit3/
> TextUnformatted/InputText/IsItemHovered` directly instead of the `Dev::` wrappers,
> so those rows escape the panel search filter.

The panel's search works by filtering **at the draw call**: `Dev::SliderFloat` and
friends decide per control whether the current query matches its label (and, one frame
later, its help text), and skip the widget when it does not. A raw `ImGui::SliderFloat`
compiles, draws, and keeps working — it is simply invisible to the filter, with no
error anywhere. On the GI tab that meant a query like "sun" returned the section
headings and the explanations but none of the controls that own them: fifteen settings
across both stages (probe basis, weight, interior/sky occlusion, spacing, rays,
hysteresis, albedos, gains, ray length, sun-proxy samples/radius/gain).

## Approach

Every control the GI tab draws now goes through its `Dev::` counterpart — a name
substitution, no signature or ordering change:

| tab section | controls migrated |
| --- | --- |
| Probe basis (REN-GI-009) | `Dev::Combo` |
| Probe gather/shading | `Dev::SliderFloat` x3 (`Weight`, `Interior AO share`, `Indoor sky occlusion`) |
| Probe grid + albedo | `Dev::SliderFloat` x3, `Dev::SliderInt` x1, `Dev::ColorEdit3` x1 |
| Sun proxy (stage 2) | `Dev::SliderInt` x1, `Dev::SliderFloat` x2 |

The wrappers are pass-throughs with no filter typed (`g_lastShown && ImGui::…`), so the
tab behaves exactly as before in normal use, and each migrated control now also picks up
the `Dev::PanelHelp` paragraph printed beneath it as searchable text.

Nothing else in `DrawGiTab` needed changing: it already used `Dev::Checkbox`,
`Dev::PanelHelp`, `Dev::PanelSeparator`, and `ResetToDefaultsButton`, and it holds no
`IsItemHovered`/tooltip pair, so there is no `Dev::PanelItemHovered`/`PanelTooltip`
conversion to make here.

## Files

- `engine/Poseidon/Dev/Debug/DebugOverlay.cpp`: `DrawGiTab()` — 14 lines, `ImGui::` →
  `Dev::` for `Combo`, `SliderFloat` (x9), `SliderInt` (x2), `ColorEdit3` (x1).

## Toggle

n/a. The panel search filter *is* the toggle and it is unchanged: empty filter = straight
pass-through (the pre-change behaviour exactly); a typed query now also reaches the GI
rows.

## Evidence

`PoseidonTests` rebuilt and the panel case re-run with the test's own needles.

| run | before | after |
| --- | --- | --- |
| `PoseidonTests "Dev panel controls all go through the search filter"` | 25 assertions: 18 passed, **7 failed** | the GI needles are gone; 4 of the 7 checks still fail on other tabs (below) |
| `PoseidonTests [dev]` | — | 27 cases: 26 passed, 1 failed; 383 assertions: 379 passed, 4 failed |
| raw hits reported inside `DrawGiTab` (lines 4874-4906) | 14 (`SliderFloat` 10, `SliderInt` 2, `Combo` 1, `ColorEdit3` 1) | **0** |
| `cmake --build … --target PoseidonTests` | — | exit 0 (clang-cl, only the pre-existing `-fdelayed-template-parsing` warning) |
| `python scripts/proteus/check-format.py` | — | OK (40 changed files, 30 C++ scoped) |

The four assertions that still fail are the bypasses this ticket did not touch, all
outside the GI tab:

| needle | where | what it is |
| --- | --- | --- |
| `InputText` | `DebugOverlay.cpp:1569` | Zeus throw — CfgAmmo class name field |
| `ColorEdit3` | `DebugOverlay.cpp:1613` | head torch colour picker |
| `IsItemHovered` | `DebugOverlay.cpp:3902` | grass "Enfusion blade atlas" tooltip |
| `IsItemHovered` | `SmokeTab.cpp` (31 sites) | Smoke tab checkbox tooltips |
| `TextUnformatted` | `MapEditorTab.cpp:58` | modal confirm question (a table-less message, likely an intentional exception) |

So the file-scan test stays red until those tabs are migrated; this ticket removes the
GI rows from that list and does not weaken the check.

## Risks

- Filtered-out controls still `return false`, so `changed` is not set by a control the
  user cannot see — same as every other tab; no settings write changes.
- `Dev::Combo` passes `popupMaxHeightInItems = -1` to `ImGui::Combo`, which is the
  default the raw call was already getting.
- The panel's per-frame attribution (`g_lastShown`) is what pairs a control with the
  `PanelHelp` beneath it; migrating the sliders means the GI explanations are now hidden
  when their control is filtered out, which is the intended behaviour and is visible only
  while a query is typed.

## Follow-ups

- Migrate the remaining bypasses so the invariant can turn green: the three
  `DebugOverlay.cpp` sites above, `SmokeTab.cpp`'s 31 `IsItemHovered` calls, and a
  decision on `MapEditorTab.cpp`'s modal message (migrate or allow-list in the test).
- Once those are done, remove the panel case from `G3b-cpp-tests`' `ctest -E` exclusion
  list so CI enforces the invariant instead of skipping it.
- No test asserts the *rendered* filter for the GI tab; the guard is source-level only.

## Verify on GPU

1. Launch the game with the dev panel (`` Ctrl+` ``) and open the **GI** tab.
2. Type `sun` into the panel search. Expect the sun-proxy rows (samples, radius, gain)
   plus their explanations, and nothing else — in particular no probe-weight slider.
3. Type `albedo`. Expect *Ground albedo* (colour swatch) and *Wall/roof albedo*, with
   their help text; the section headings and separators should be gone.
4. Type `zzz`. Expect an empty tab, not a skeleton of headings.
5. Clear the search. The tab must look byte-for-byte like before this ticket: same
   order, same defaults, and dragging *Weight* / ticking *Enabled* still changes the
   image immediately.
