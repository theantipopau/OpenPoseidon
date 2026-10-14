#!/usr/bin/env python3
"""T18b: parse engine logs into perf CSV rows.

Inputs (all tolerant regexes over the densest real formats seen in-tree):
  PERF-CAP gpu120 g0=0.123 ... | cpu120 c0=0.011 ...   (POSEIDON_GPU_STATS=1, T18a)
  frame avg/p95 ms 12.3/15.1 (60 fps, 1234 draws) sim=.. ...   (World.cpp CPU line)
  BENCHMARK RESULT: 300 frames in 10.2s = 29.4 avg FPS

Outputs: one dict row per run {label, res, scene, backend, gpu{frame,regions...},
cpu_total_ms, cpu_fps, draws, bench_fps} + percentile summaries across --iters runs.

Unknown trailing g/c indices are kept as r<index> columns (never dropped).
Usage: python perf_parse.py --selftest   (synthetic lines, no game needed)
"""
import csv
import io
import json
import os
import re
import statistics
import sys

try:
    from gpu_regions import NAMES
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from gpu_regions import NAMES

PERFCAP = re.compile(r"PERF-CAP gpu120((?:\s+g\d+=[\d.]+)+)\s+\|\s+cpu120((?:\s+c\d+=[\d.]+)+)")
CELL = re.compile(r"\s+[gc](\d+)=([\d.]+)")
CPU = re.compile(
    r"frame avg/p95 ms ([\d.]+)/([\d.]+) \(([\d.]+) fps, ([\d.]+) draws\)")
BENCH = re.compile(r"BENCHMARK RESULT: (\d+) frames in ([\d.]+)s = ([\d.]+) avg FPS")


def col_name(kind, idx):
    if kind == "g":
        return "gpu_" + NAMES.get(idx, f"r{idx}").lower().replace(" ", "_").replace("/", "_").replace(":", "").replace("-", "_").replace("(", "").replace(")", "")
    return f"cpu_r{idx}"


def parse_log(text):
    """Return (perfcap_rows, cpu_rows, bench_rows); perfcap takes the LAST complete window."""
    perf, cpu, bench = [], [], []
    for line in text.splitlines():
        m = PERFCAP.search(line)
        if m:
            g = {}
            for mm in re.finditer(r"g(\d+)=([\d.]+)", m.group(1)):
                g[int(mm.group(1))] = float(mm.group(2))
            c = {}
            for mm in re.finditer(r"c(\d+)=([\d.]+)", m.group(2)):
                c[int(mm.group(1))] = float(mm.group(2))
            perf.append((g, c))
            continue
        m = CPU.search(line)
        if m:
            cpu.append({"cpu_avg_ms": float(m.group(1)), "cpu_p95_ms": float(m.group(2)),
                        "cpu_fps": float(m.group(3)), "draws": float(m.group(4))})
            continue
        m = BENCH.search(line)
        if m:
            bench.append({"bench_frames": int(m.group(1)), "bench_secs": float(m.group(2)),
                          "bench_fps": float(m.group(3))})
    return perf, cpu, bench


def run_row(label, res, scene, backend, text):
    perf, cpu, bench = parse_log(text)
    row = {"label": label, "res": res, "scene": scene, "backend": backend}
    if perf:
        g, c = perf[-1]
        for i, v in sorted(g.items()):
            row[col_name("g", i)] = round(v, 3)
        for i, v in sorted(c.items()):
            row[col_name("c", i)] = round(v, 3)
    if cpu:
        row.update(cpu[-1])
    if bench:
        row.update(bench[-1])
    return row


def summarize(rows):
    """p50/p95/p99 across iterations for every numeric column. p95/p99 of few samples
    are reported as-is (max-dominated) — the record tells owners to use --iters 5+."""
    keys = sorted({k for r in rows for k, v in r.items() if isinstance(v, (int, float))})
    out = []
    for k in keys:
        vals = sorted(r[k] for r in rows if isinstance(r.get(k), (int, float)))
        if not vals:
            continue
        out.append({"metric": k, "n": len(vals),
                    "p50": round(statistics.median(vals), 3),
                    "p95": round(vals[min(len(vals) - 1, int(0.95 * (len(vals) - 1) + 0.5))], 3),
                    "p99": round(vals[-1], 3)})
    return out


def write_csv(rows, path):
    cols = ["label", "res", "scene", "backend"]
    extra = sorted({k for r in rows for k in r if k not in cols})
    with open(path, "w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=cols + extra)
        w.writeheader()
        w.writerows(rows)


def selftest():
    log6070 = ("PERF-CAP gpu120 g0=0.100 g25=12.000 g29=2.200 g99=0.500 | cpu120 c0=0.010 c63=1.000\n"
               "frame avg/p95 ms 12.3/15.1 (60 fps, 1234 draws) sim=1.1/2.0\n"
               "BENCHMARK RESULT: 300 frames in 10.2s = 29.4 avg FPS\n")
    perf, cpu, bench = parse_log(log6070)
    assert len(perf) == 1 and perf[0][0][25] == 12.0 and perf[0][0][99] == 0.5, perf
    assert cpu[0]["cpu_fps"] == 60.0 and cpu[0]["draws"] == 1234.0, cpu
    assert bench[0]["bench_fps"] == 29.4, bench
    row = run_row("ab", "1080p", "open", "vulkan", log6070)
    assert row["gpu_gpu_frame_total"] == 12.0, row
    assert row["gpu_r99"] == 0.5, "unknown indices kept as r<index>"
    assert row["cpu_fps"] == 60.0
    rows = [run_row(f"i{i}", "1080p", "open", "vulkan",
                    log6070.replace("g25=12.000", f"g25=1{i}.000")) for i in range(5)]
    s = {m["metric"]: m for m in summarize(rows)}
    assert s["gpu_gpu_frame_total"]["p50"] == 12.0, s["gpu_gpu_frame_total"]
    assert s["gpu_gpu_frame_total"]["n"] == 5
    buf = io.StringIO()
    w = csv.DictWriter(buf, fieldnames=["label", "a"])
    w.writeheader()
    w.writerow({"label": "x", "a": 1})
    assert "label,a" in buf.getvalue()
    # last-window rule: two PERF-CAP lines -> last wins
    two = log6070 + log6070.replace("g25=12.000", "g25=20.000")
    assert run_row("l", "r", "s", "b", two)["gpu_gpu_frame_total"] == 20.0
    # empty log -> bare row, no crash
    assert run_row("e", "r", "s", "b", "nothing here\n")["label"] == "e"
    print("perf_parse: selftest OK (parse, unknown-index, last-window, summary, csv)")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        selftest()
    else:
        print("usage: perf_parse.py --selftest  (library; see perf-capture.ps1)")
