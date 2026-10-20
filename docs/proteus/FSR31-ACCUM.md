# FSR31-ACCUM — FSR 3.1 consumes the agreed temporal accumulator

## Scope (FSR 3.1 accumulation core, v2)

The v1 backend (UPSCALER-2) upscaled the current jittered frame with
jitter-compensated EASU+RCAS — stable, but single-frame: no history reuse, no
reconstruction beyond the jitter fix. This ticket wires the real accumulation
core without adding a second history of its own.

## Fix

- `lib.rs` orchestration: `native_history_wanted` now also runs the
  `Temporal::render_debug_resolve` accumulator when FSR 3.1 is on at any
  render <= output (previously Native TAA at 100% only). Over-runs when
  DLSS/FSR 4 wins the chain — the same accepted tradeoff as the native path;
  predicting the chain would fork chain logic into the orchestration.
- `fsr31.rs` evaluate: when `ctx.native_history` is present, EASU-upscales
  the accumulated (stable, unjittered) image with PLAIN viewport constants
  plus RCAS; at render == output that is Native AA mode. When no accumulator
  ran (no velocity target), falls back to the v1 jitter-compensated
  single-frame path. Per-path one-time log lines say which ran.
- Disocclusion rejection lives in the resolve's depth compare
  (`temporal.wgsl`); this backend adds no second history buffer, honouring
  "the one accumulator every temporal backend must agree on" (upscaler.rs).
- `reset`/`reset_reason` need no backend handling: the resolve discards
  internally, and history-sized rebuilds on render-size change already exist
  (`r.size != render_size`).

## Verification

- `cargo test -p wgpu_renderer --lib fsr31` 3/3 pass; `upscaler` 9/9 pass
  (exit 0, pipefail-preserved). Caps/ids unchanged (still id 6).
- `check-all` OK (format hunk-clean on 21 changed files).
- No GPU run headless: `gpu_verified` stays false. Owner check: FSR 3.1 mode
  on Vulkan should log `upscaler: active is now FSR 3.1` plus the new
  `(accumulator + EASU + RCAS)` line; compare shimmer/ghosting against the v1
  single-frame line on the same scene. `Shot-Mission.ps1` jeep-ride both
  backends per DX12-1.

## Follow-ups

- Output-res history (accumulate after upscale) would beat this
  accumulate-then-upsample on sub-pixel detail; needs render-res velocity
  sampled at output res — a bigger, riskier change, only if v2 underwhelms.
- Busy combat mission CPU baseline (`Shot-Mission.ps1` on an AI-heavy
  mission) still open from DX12-1.
- Native TAA `graphics.cfg` lane decision still open.
