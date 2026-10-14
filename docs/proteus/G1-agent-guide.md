# G1 — Agent guide (AGENTS.md + CLAUDE.md pointer)

Branch: `proteus/G1-agent-guide`. Status: done-unverified (docs only, nothing GPU-dependent).

## Problem

The fork had no agent instructions (`KNOWN_ISSUES.md` flags the missing `CLAUDE.md`);
every run re-derived layout, working build commands, branch model and toggle conventions.

## Approach

One non-Claude-specific `AGENTS.md` at repo root (layout, verified build/test commands,
branch model, standing rules, Definition of Done, ledger usage, toggle convention) plus a
one-line `CLAUDE.md` pointer, per the ticket spec.

## Files

- `AGENTS.md` (new), `CLAUDE.md` (new).

## Toggle and default

n/a — docs only, no behaviour change.

## Evidence

- Docs-only change: no build or test surface affected. Content cross-checked against
  `cmake/presets/windows.json`, root `Cargo.toml` workspace members, `scripts/Build.ps1`,
  and `docs/KNOWN_ISSUES.md` during writing.
- `cargo test` / build not re-run: no compiled file touched (same exemption as a pure-doc PR).

## Risks

- Build notes can rot (paths, LLVM version). Mitigation: G3 CI will exercise the documented
  commands; owner reports drift via KNOWN_ISSUES.

## Follow-ups

- G3 wires CI to the commands documented here; fix the guide if CI needs different steps.

## Verify on GPU

n/a — no renderer change. Read-check only: `AGENTS.md` commands match your machine's paths.
