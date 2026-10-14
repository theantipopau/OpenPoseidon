#!/usr/bin/env python3
"""Regenerate scripts/proteus/gpu_regions.py from EngineWgpu.cpp kNames."""
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(REPO, "engine", "WgpuRenderer", "EngineWgpu.cpp")
OUT = os.path.join(REPO, "scripts", "proteus", "gpu_regions.py")

src = open(SRC, encoding="utf-8").read()
m = re.search(r"kNames\[WGR_GPU_TIMER_REGION_COUNT\] = \{(.*?)\};", src, re.S)
if not m:
    raise SystemExit("gen-regions: kNames block not found")
names = re.findall(r'"([^"]*)"', m.group(1))
print(f"gen-regions: {len(names)} region names")
with open(OUT, "w", encoding="utf-8") as f:
    f.write('"""GPU timer region index -> short display name.\n')
    f.write('GENERATED from EngineWgpu.cpp kNames (WgrGpuTimerRegion order, append-only).\n')
    f.write('Regenerate with scripts/proteus/gen-regions.py; parsers tolerate unknown indices.\n')
    f.write('"""\nNAMES = {\n')
    for i, n in enumerate(names):
        f.write(f"    {i}: {n.split('(')[0].strip()!r},\n")
    f.write("}\n")
