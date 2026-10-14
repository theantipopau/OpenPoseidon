#!/usr/bin/env python3
"""T19 compare: candidate PNGs vs blessed baselines -> markdown report.

Exits: 0 all PASS | 1 any FAIL/size-mismatch | 2 bad args |
       5 missing baseline or candidate file (never silently passes; never writes placeholders).
"""
import argparse
import datetime
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from imgdiff import DiffError, compare


def load_shots(path):
    try:
        import yaml
    except ImportError:
        print("golden-compare: need pyyaml")
        sys.exit(2)
    with open(path, encoding="utf-8") as fh:
        return yaml.safe_load(fh)["shots"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--shots", required=True)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--report", required=True)
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    only = {s.strip() for s in a.only.split(",") if s.strip()}
    rows = []
    for s in load_shots(a.shots):
        name = s["name"]
        if only and name not in only:
            continue
        cand = os.path.join(a.candidate, name + ".png")
        base = os.path.join(a.baseline, name + ".png")
        if not os.path.isfile(base):
            print(f"golden-compare: shot '{name}': no baseline at {base} — run golden-capture -Bless first")
            sys.exit(5)
        if not os.path.isfile(cand):
            print(f"golden-compare: shot '{name}': no candidate at {cand}")
            sys.exit(5)
        try:
            r = compare(base, cand)
            ok = r["dissim"] <= s["max_dissim"] and r["color_mae"] <= s["max_color_mae"]
            rows.append((name, "PASS" if ok else "FAIL", r, s, ""))
        except DiffError as e:
            rows.append((name, "FAIL", {"dissim": float("nan"), "color_mae": float("nan"),
                                        "w": 0, "h": 0}, s, str(e)))
    fails = sum(1 for r in rows if r[1] == "FAIL")
    with open(a.report, "w", encoding="utf-8") as fh:
        fh.write(f"# Golden compare — {datetime.datetime.now():%Y-%m-%d %H:%M}\n\n")
        fh.write(f"baseline `{a.baseline}` vs candidate `{a.candidate}`\n\n")
        fh.write("| shot | result | dissim (max) | color_mae (max) | note |\n|---|---|---|---|---|\n")
        for name, res, r, s, note in rows:
            fh.write(f"| {name} | **{res}** | {r['dissim']:.5f} ({s['max_dissim']}) | "
                     f"{r['color_mae']:.3f} ({s['max_color_mae']}) | {note} |\n")
        fh.write(f"\n{len(rows) - fails}/{len(rows)} PASS\n")
    print(f"golden-compare: {len(rows) - fails}/{len(rows)} PASS -> {a.report}")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
