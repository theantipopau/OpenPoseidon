# G3 — CI workflow + local check-all parity

Branch: `proteus/G3-ci`. Status: done-unverified (workflow file validated, never observed green on runners).

## Problem

No CI existed (only `FUNDING.yml`). The G2 asset gate and format hygiene had no automated
enforcement; contributors re-derive build steps from tribal knowledge.

## Approach

- `.github/workflows/proteus.yml` (5 jobs): `assets` (check-assets.py), `fmt-changed`
  (rustfmt/clang-format on files changed vs base only), `rust-tests` (`cargo test -p
  wgpu_renderer --lib` on Lavapipe software Vulkan so device tests can run headless),
  `build-linux` (`linux-x64-clang-rwdi` preset, vcpkg + cargo cached), `build-windows`
  (`win-x64-clang-rwdi`, main pushes + manual dispatch only — runners are slow).
- `scripts/proteus/check-all.ps1`: same gates locally — asset check + changed-vs-`proteus/main`
  format checks (per-path Rust edition: `engine/WgpuRenderer/rust/*` = 2024, else 2021;
  VS-bundled clang-format fallback; missing tool warns-and-skips locally, CI is strict).

## Files

- `.github/workflows/proteus.yml` (new), `scripts/proteus/check-all.ps1` (extended).

## Toggle and default

n/a — tooling only.

## Evidence

- Workflow YAML parses (`yaml.safe_load`, 5 jobs); changed-files logic simulated locally.
- `check-all.ps1`: clean tree → OK exit 0; planted bad `.rs` (via `git add -N`) → FAIL exit 1
  with the exact rustfmt diff; removed → OK again. C++ arm proven by a live incident: an
  untracked third-party `Aspect_Ratio.hpp` tripped it, which forced the correct design (see Risks).
- No compiled code touched; full `cargo test`/CMake not re-run.

## Risks

- Workflow never observed green (UNVERIFIED): Lavapipe may or may not satisfy every device test;
  vcpkg full builds may exceed runner time; Windows job may need runner tuning. First green run
  is the owner's, then flip this record's claim.
- `git diff`-based changed-file detection misses purely-untracked files: locally, `git add -N`
  new files first (documented in the script header); in CI all files are committed.

## Follow-ups

- Owner: run once (push to `proteus/main` or workflow_dispatch), paste results; fix fallout as G3a.
- T18/T19 later add their own CI jobs reusing this file's patterns.

## Verify on GPU

n/a — no renderer change. Owner runs: push any branch starting `proteus/` and read the Actions tab;
expect 4 fast jobs green, `build-windows` skipped except on main/manual.
