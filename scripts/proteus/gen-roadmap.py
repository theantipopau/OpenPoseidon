#!/usr/bin/env python3
"""G4: generate docs/proteus/ROADMAP.md from docs/proteus/ledger.yaml.

Usage: python scripts/proteus/gen-roadmap.py [--check]
  default: rewrite ROADMAP.md. --check: exit 1 if ROADMAP.md is stale.
"""
import os
import sys

try:
    import yaml
except ImportError:
    print("gen-roadmap: ERROR: PyYAML is required", file=sys.stderr)
    sys.exit(2)

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LEDGER = os.path.join(REPO, "docs", "proteus", "ledger.yaml")
ROADMAP = os.path.join(REPO, "docs", "proteus", "ROADMAP.md")

PHASES = [
    ("STEP", "Setup"),
    ("G", "Phase G — Governance"),
    ("T", "Phase T — GPU verification tooling"),
    ("A", "Phase A — Renderer issues"),
    ("B", "Phase B — Image quality"),
    ("C", "Phase C — Legacy asset uplift"),
    ("D", "Phase D — Performance and stability"),
    ("E", "Phase E — Player-facing"),
    ("F", "Phase F — Research (proposals only)"),
]


def phase_of(ticket_id):
    for prefix, _title in PHASES:
        if ticket_id == prefix or ticket_id.startswith(prefix) or \
                (prefix == "STEP" and ticket_id.startswith("STEP")):
            return prefix
    return "?"


def render(tickets):
    lines = ["# Proteus roadmap", "",
             "<!-- GENERATED from ledger.yaml by scripts/proteus/gen-roadmap.py. Do not edit by hand. -->",
             ""]
    by_phase = {}
    for t in tickets:
        by_phase.setdefault(phase_of(t.get("id", "?")), []).append(t)
    for prefix, title in PHASES:
        rows = sorted(by_phase.get(prefix, []), key=lambda t: t.get("id", ""))
        if not rows:
            continue
        lines += [f"## {title}", "",
                  "| ID | Title | Status | GPU | Branch |",
                  "| --- | --- | --- | --- | --- |"]
        for t in rows:
            gpu = "yes" if t.get("gpu_verified") else "no"
            lines.append(f"| {t.get('id','')} | {t.get('title','')} | "
                         f"{t.get('status','')} | {gpu} | `{t.get('branch','')}` |")
        lines += [""]
    leftovers = by_phase.get("?", [])
    if leftovers:
        lines += ["## Unsorted", ""]
        for t in leftovers:
            lines.append(f"- {t.get('id')}: {t.get('title')}")
        lines += [""]
    return "\n".join(lines)


def main():
    with open(LEDGER, encoding="utf-8") as fh:
        tickets = (yaml.safe_load(fh) or {}).get("tickets", [])
    text = render(tickets)
    if "--check" in sys.argv:
        try:
            with open(ROADMAP, encoding="utf-8") as fh:
                current = fh.read()
        except OSError:
            print("gen-roadmap: FAIL (ROADMAP.md missing)")
            return 1
        if current != text:
            print("gen-roadmap: FAIL (ROADMAP.md stale — run gen-roadmap.py)")
            return 1
        print("gen-roadmap: OK (ROADMAP.md current)")
        return 0
    with open(ROADMAP, "w", encoding="utf-8") as fh:
        fh.write(text)
    print(f"gen-roadmap: wrote ROADMAP.md ({len(tickets)} tickets)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
