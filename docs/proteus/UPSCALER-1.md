# UPSCALER-1 — Upscaler decision (done-unverified)

## Input (delegated web research, 2026-10-16)

- **In-house TAAU** stays the default, fallback, and WebGPU path: the only
  option spanning native DX12/Vulkan and a future WebGPU port. Sharpen
  stays optional and restrained (DLSS's own sharpen is deprecated; double
  sharpening is a real risk).
- **DLSS (4.x Transformer)** is the quality leader in 2025–26 blind tests,
  but NVIDIA-only, Streamline-integrated, redistributable only as
  incorporated object code — a native-only optional path, if the uplift
  justifies the cost. Pin a Streamline release; never float SDKs.
- **FSR 3.1.x source route (MIT)** is the cross-vendor native candidate to
  evaluate. **FSR 4 (4.1.x)** is a separate product: big quality step over
  FSR 3, but signed prebuilt DLLs + narrower hardware envelope
  (RX 7000+ per AMD's 4.1.1 notes) — conditional, with a non-ML fallback.
- **No browser path** for DLSS or any official FSR SDK (native DLL model
  vs WebGPU sandbox). The web route is an engine-owned WGSL temporal
  upscaler; `@pmndrs/upscaler` (already in PROVENANCE.md as study-only)
  is evidence the port is feasible, not a dependency.
- Stability: no verified broad `amdxc64.dll` pattern publicly attributable
  to FSR; one unconfirmed lead (Adrenalin 26.6.4 notes on FSR 4.1/RX 7000
  crashes, page timed out on fetch). Our Oct-14 cluster (9 identical
  `amdxc64.dll` AVs + 1 FSR-loader AV) stands as **local** evidence for
  caution, not a general claim.

## Decision

1. Ship and tune in-house TAAU (+ optional mild sharpen). It is the
   reference all A/B comparisons judge against (W0a/W0c pattern).
2. Do NOT ship vendor DLLs. DLSS/FSR4 stay load-time-optional code paths
   (`dlss.rs`, `fsr4.rs` already are); no SDK vendored.
3. Next evaluable step, in order: FSR 3.1.x source integration spike →
   DLSS/Streamline optional path → FSR 4 prototype against the official
   SDK with HW/backend checks. Each behind the same renderer capability
   interface with TAAU fallback.
4. FSR4 path retest after a driver/SDK update (carried over from
   BACKEND-VERIFY-1); the 26.6.4 lead is the first thing to check.

## Verification

Docs-only. No engine change; `gpu_verified` stays false. Sources are
listed in the research handoff (NVIDIA DLSS/Streamline pages, AMD
FidelityFX SDK releases, DF/Notebookcheck/TechPowerUp quality reports,
WebGPU explainer, pmndrs/upscaler).
