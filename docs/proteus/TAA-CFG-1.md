# TAA-CFG-1 — Native TAA gets a graphics.cfg lane (persistable, panel-saved)

## Decision

Native TAA was session-only (dev-panel checkbox + `WGR_NATIVE_TAA` boot seed);
the `Engine.hpp` comment even claimed graphics.cfg "has no lane for it".
The lane now exists: `nativeTaa` (0 = off, nonzero = on) in graphics.cfg,
written by the panel's "Save as default" and applied at boot and live.
No Options-page row — the panel remains the only switch; the file just
remembers it. This closes the open "Native TAA graphics.cfg lane" follow-ups
from UPSCALER-2/HANDOVER, DX12-1 and FSR31-ACCUM.

## Semantics (deliberate)

- `GraphicsApply.cpp` applies with OR-semantics: the file may only ENABLE
  (`if (cfg.nativeTaa != 0) t.nativeTaaOn = true`), never clobber live
  panel/env state. A plain assignment would switch off a panel-enabled
  reference every time any Options row applies.
- Consequence, documented in the tooltip: a saved-ON is sticky until saved
  off again (uncheck + "Save as default" writes 0). The backend itself still
  skips away from 100% scale, so a saved-ON with an upscaler quality is
  inert, not an error.
- `WGR_NATIVE_TAA=1` still seeds at boot through the existing engine-ctor
  path; the cfg lane and the env seed compose (either enables).
- Tolerated as absent in old files (stays 0); factory default 0.

## Files

- `UI/Settings/GraphicsConfig.{hpp,cpp}` — field + Load/Save entries.
- `UI/Settings/GraphicsApply.cpp` — OR-apply with comment.
- `Dev/Debug/DebugOverlay.cpp` — "Save as default" writes the lane + log
  line; checkbox tooltip rewritten (was "cannot persist it"); save tooltip
  lists Native TAA.
- `Graphics/Core/Engine.hpp` — stale "never persisted" comment corrected.
- `tests/.../UI/Settings/test_graphicsConfig.cpp` — default-off + round-trip.

## Verification

- New test passes: `[Settings][GraphicsConfig]` 161 assertions / 29 cases
  (was 28 cases), exit 0.
- `[menu],[splash],[inGameUI]` 19/6 pass; broad `[UI]` 230 cases 224/0/6
  exit 0. `Poseidon` + `PoseidonTests` link green (exit 0; only pre-existing
  warnings/notes).
- `check-all` OK (format hunk-clean on 27 changed files).
- One real bug caught by the build: the first tooltip edit broke the
  closing `");` (compile error, fixed, rebuilt green).
- No GPU run headless: `gpu_verified` stays false. Owner check: check the
  box + "Save as default", restart, confirm the reference persists; uncheck
  + save, restart, confirm off.

## Follow-ups

- Owner GPU check above.
- If saved-ON stickiness ever bites, the fix is a panel write-through (live
  uncheck clears the lane), not assign-semantics in Apply.
