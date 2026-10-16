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
| G3 | CI workflow + local check-all parity | done-unverified | no | `proteus/G3-ci` |
| G4 | Roadmap and renderer-systems ledger | done-unverified | no | `proteus/G4-roadmap-ledger` |

## Phase T — GPU verification tooling

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| T18a | Machine-readable GPU/CPU timing line (PERF-CAP) | done-unverified | no | `proteus/T18-perf-capture` |
| T18b | Perf capture script + scene/region definitions | done-unverified | no | `proteus/T18b-capture-script` |
| T19 | Golden-image harness (shots + SSIM compare + capture) | done-unverified | no | `proteus/T19-golden-harness` |
| TOOLCHAIN-1 | VS-bundled LLVM candidates in FindLLVMSanitizer.cmake | done-unverified | no | `proteus/main (direct)` |

## Phase A — Renderer issues

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| A1 | Half-resolution GTAO with bilateral upsample (LIT-010 lever) | done-unverified | no | `proteus/ao-half-res` |

## Phase B — Image quality

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| BACKEND-VERIFY-1 | Build + test verification of merged upstream backend; Oct-14 crash triage | done-unverified | no | `proteus/main (direct, follows UPSTREAM-SYNC-1)` |

## Phase F — Research (proposals only)

| ID | Title | Status | GPU | Branch |
| --- | --- | --- | --- | --- |
| FIXTURE-FONT-1 | Authored font fixtures + tolerant post-build copy | done-unverified | no | `proteus/main (direct)` |
| FIXTURE-VOICE-1 | Restored omitted voice fixture files; 2 failing UI tests fixed | done-unverified | no | `proteus/main (direct)` |

## Unsorted

- W0a: Upscaler interface + Native TAA backend
- W0-ORDER: Retained-admission race intermittently renders empty worlds
- UPSTREAM-SYNC-1: Merge upstream/main (CLI11 C++20 overlay + DB/loadout scripting)
- MENU-FFUR-1: Proteus shell neutralizes inherited menu artwork (FFUR shutdown leak)
- UI-THEME-A: Proteus art backdrop on Mods/Multiplayer/Configure (Phase A slice)
- SHUTDOWN-T-1: SHUTDOWN-TIME phase probes in UnloadGameData + live proof
