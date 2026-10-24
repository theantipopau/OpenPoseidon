# Proteus roadmap

<!-- GENERATED from ledger.yaml by scripts/proteus/gen-roadmap.py. Do not edit by hand. -->

## Setup

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| STEP-0 | State-of-repo baseline | done-unverified | no | `dev/matt-fork-main` |

## Phase G — Governance

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| G1 | Agent guide (AGENTS.md + CLAUDE.md pointer) | done-unverified | no | `proteus/G1-agent-guide` |
| G2 | Asset and licence registry | done-unverified | no | `proteus/G2-asset-registry` |
| G2a-fixture-strays | Fixture binaries that committed tests require were untracked; third-party Aspect_Ratio.hpp sat at the root | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |
| G3 | CI workflow + local check-all parity | done-unverified | no | `proteus/G3-ci` |
| G3a-fmt-base | fmt-changed compared against origin/main, not the Proteus line (red on 169 files) | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |
| G4 | Roadmap and renderer-systems ledger | done-unverified | no | `proteus/G4-roadmap-ledger` |

## Phase T — GPU verification tooling

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| T18a | Machine-readable GPU/CPU timing line (PERF-CAP) | done-unverified | no | `proteus/T18-perf-capture` |
| T18b | Perf capture script + scene/region definitions | done-unverified | no | `proteus/T18b-capture-script` |
| T18c | Populated benchmarks: admitted-count gate + re-baseline | done-unverified | yes | `proteus/main (direct)` |
| T19 | Golden-image harness (shots + SSIM compare + capture) | done-unverified | no | `proteus/T19-golden-harness` |
| TAA-CFG-1 | Native TAA graphics.cfg lane (persistable via Save as default) | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |
| TOOLCHAIN-1 | VS-bundled LLVM candidates in FindLLVMSanitizer.cmake | done-unverified | no | `proteus/main (direct)` |

## Phase A — Renderer issues

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| A1 | Half-resolution GTAO with bilateral upsample (LIT-010 lever) | done-unverified | no | `proteus/ao-half-res` |

## Phase B — Image quality

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| BACKEND-VERIFY-1 | Build + test verification of merged upstream backend; Oct-14 crash triage | done-unverified | no | `proteus/main (direct, follows UPSTREAM-SYNC-1)` |

## Phase D — Performance and stability

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| DX12-1 | DX12 black terrain fix, FSR 4 falls back to FSR 3.1, upscaler transition log | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |

## Phase F — Research (proposals only)

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| FIXTURE-FONT-1 | Authored font fixtures + tolerant post-build copy | done-unverified | no | `proteus/main (direct)` |
| FIXTURE-SKIP-1 | An absent fixture SKIPs its own cases instead of aborting the whole test binary | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |
| FIXTURE-SYNTH-1 | Synthetic texture family + shape fuzz reproducers; wire test_mlod_tagg | done-unverified | no | `proteus/main (direct)` |
| FIXTURE-VOICE-1 | Restored omitted voice fixture files; 2 failing UI tests fixed | done-unverified | no | `proteus/main (direct)` |
| FSR31-ACCUM | FSR 3.1 consumes the agreed temporal accumulator (v2 core) | done-unverified | no | `proteus/DX12-1-dx12-upscaler-fixes` |

## Unsorted

- W0a: Upscaler interface + Native TAA backend
- W0-ORDER: Retained-admission race intermittently renders empty worlds
- UPSTREAM-SYNC-1: Merge upstream/main (CLI11 C++20 overlay + DB/loadout scripting)
- MENU-FFUR-1: Proteus shell neutralizes inherited menu artwork (FFUR shutdown leak)
- UI-THEME-A: Proteus art backdrop on Mods/Multiplayer/Configure (Phase A slice)
- SHUTDOWN-T-1: SHUTDOWN-TIME phase probes in UnloadGameData + live proof
- UPSCALER-1: Upscaler decision (in-house TAAU default; vendor paths optional-only)
- UI-THEME-B: Proteus art backdrop on Server/Login/NewUser/Revert (Phase A continued)
- UI-THEME-C: Proteus theme on the mission-editor dialogs (no art backdrop)
- UI-THEME-C1: Editor popups keep retail backdrop under themed buttons
- RUST-TESTS-1: The four standing cargo test --lib failures (ABI pin, GI test-device limits, NaN atlas UV)
- RUST-TESTS-1a: Parallel cargo test access violation (Vulkan validation layer race) + check-format cp1252 crash
- RUST-TESTS-1b: CI runs the serial validation-layer pass on main
