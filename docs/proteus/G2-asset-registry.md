# G2 — Asset and licence registry

Branch: `proteus/G2-asset-registry`. Status: done-unverified (needs owner eyeball of registry).

## Problem

Any file could enter `assets/` (or sky/icons/screenshots) with no licence record; the brief's
ASSET-010 gate doesn't exist in-tree. No automated check stopped unregistered art.

## Approach

- `assets/registry.yaml`: 72 per-file entries (path, source, author, licence, SPDX, date_added,
  notes) covering `assets/**`, `resources/sky/*`, `resources/icon-*`, `screenshots/**`.
  `date_added` is `unknown` for everything (registry is new; provenance predates it).
- `scripts/proteus/check-assets.py`: exits 1 on unregistered on-disk files, stale entries,
  missing fields, or duplicates; exit 2 if PyYAML/scanner fails.
- `scripts/proteus/check-all.ps1`: local aggregate entry point (asset check now; G3 appends CI parity).
- `docs/proteus/PROVENANCE.md`: provenance log seeded with fork-inherited art (upstream logos
  marked UNVERIFIED on exact hash).

## Files

- `assets/registry.yaml` (new), `scripts/proteus/check-assets.py` (new),
  `scripts/proteus/check-all.ps1` (new), `docs/proteus/PROVENANCE.md` (new).

## Toggle and default

n/a — tooling only. The check is strict by default (any unregistered file fails).

## Evidence

- `python3 scripts/proteus/check-assets.py` → `OK (72 files, all registered)`, exit 0.
- Negative test: added `assets/tmp-probe.bin` → `FAIL (1 problem(s))`, exit 1; removed → OK again.
- No compiled code touched; `cargo test`/CMake not re-run (no build surface).

## Risks

- Registry `date_added` values are all `unknown` — honest but weak for audit; fill in from git
  history as files are touched.
- Upstream logo exact hash UNVERIFIED (see PROVENANCE.md).

## Follow-ups

- G3: call `check-assets.py` from CI; extend `check-all` with the format checks.
- Owner: confirm the 72 registry rows (especially `openposeidon_logo.*` retirement plan).

## Verify on GPU

n/a — no renderer change. Owner runs: `python scripts/proteus/check-assets.py`
(expect OK) from the repo root; on their GPU machine the same command must pass before screenshots count as evidence for later tickets.
