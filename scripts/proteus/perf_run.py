#!/usr/bin/env python3
"""T18b driver: logs -> CSV + PERF-SUMMARY json on stdout.

Usage: perf_run.py --csv OUT.csv --res RES --scene SCENE --backend BACKEND
           --tag LABEL_PREFIX LOG [LOG ...]
Each LOG becomes one row labelled <prefix>#<n>. Prints one PERF-SUMMARY line.
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from perf_parse import run_row, summarize, write_csv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", required=True)
    ap.add_argument("--res", required=True)
    ap.add_argument("--scene", required=True)
    ap.add_argument("--backend", required=True)
    # Parallel repeatable lists (labels may contain '=', paths may too — never join them
    # with a separator). --run and --log must appear the same number of times, in order.
    ap.add_argument("--run", action="append", default=[])
    ap.add_argument("--log", action="append", default=[])
    a = ap.parse_args()
    if len(a.run) != len(a.log) or not a.run:
        ap.error("--run LABEL and --log PATH must pair up (same count, >= 1)")
    rows = []
    for label, path in zip(a.run, a.log):
        with open(path, encoding="utf-8", errors="replace") as fh:
            rows.append(run_row(label, a.res, a.scene, a.backend, fh.read()))
    write_csv(rows, a.csv)
    print("PERF-SUMMARY " + json.dumps(summarize(rows)))


if __name__ == "__main__":
    main()
