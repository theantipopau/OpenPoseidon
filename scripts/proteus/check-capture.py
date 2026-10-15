#!/usr/bin/env python3
"""Capture-validity gate for Proteus (W0-ORDER).

A game run is VALID only if the log proves the requested test mission
actually booted AND the world populated. Anything else (menu idle, wrong
mission via UI input, half-loaded world) is INVALID and must never be
blessed or summarised.

Checks (in order):
  1. INPUT_DIALOG -- the run went through the single-mission dialog
     ("DIAG single PLAY: starting intro"), i.e. live UI input launched some
     other mission. Always INVALID, whatever else the log says.
  2. EMPTY_KNOWN -- the candidate PNG's md5 is the known GModeIntro menu
     backdrop (f191...). Tripwire, needs --png.
  3. NO_BOOT -- no "Mission start phase: StartAutoTest total" line: the
     staged test mission never booted (e.g. W0-ORDER staging bug).
  4. NO_CENSUS -- booted but no admission census lines at all: fail closed.
  5. ADMITTED_LOW -- last census admitted count below --min-admitted.

Usage: check-capture.py --log <game.log> --min-admitted <n> [--png <file>]
Exit: 0 VALID (prints VALID ...), 1 INVALID (prints INVALID <REASON> ...).
  --selftest runs canned fixtures (no game needed).
"""

import argparse
import hashlib
import os
import re
import sys

# md5 prefix of the known GModeIntro menu-backdrop frame (W0-ORDER: five
# captures, byte-identical full file hash starting f191f13a). A candidate
# matching this never played a mission.
EMPTY_WORLD_MD5 = "f191f13a"

BOOT_RE = re.compile(r"Mission start phase: StartAutoTest total")
CENSUS_RE = re.compile(r"(\d+) objects offered, (\d+) admitted")
DIALOG_RE = re.compile(r"DIAG single PLAY: starting intro")
SRC_RE = re.compile(r"Test mission:\s*(\S+)")


def md5_of(path):
    h = hashlib.md5()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def is_denylisted(digest):
    return digest.startswith(EMPTY_WORLD_MD5)


def judge(log_text, min_admitted, png_digest=None):
    """Return (valid: bool, reason: str, detail: str)."""
    src = SRC_RE.search(log_text)
    src = src.group(1) if src else "?"
    if DIALOG_RE.search(log_text):
        return False, "INPUT_DIALOG", f"run went through the mission dialog (src={src})"
    if png_digest is not None and is_denylisted(png_digest):
        return False, "EMPTY_KNOWN", f"candidate matches menu-backdrop denylist (src={src})"
    if not BOOT_RE.search(log_text):
        return False, "NO_BOOT", f"test mission never booted (src={src})"
    matches = CENSUS_RE.findall(log_text)
    if not matches:
        return False, "NO_CENSUS", "booted but no admission census lines"
    offered, admitted = int(matches[-1][0]), int(matches[-1][1])
    if admitted < min_admitted:
        return (
            False,
            "ADMITTED_LOW",
            f"admitted={admitted} < min_admitted={min_admitted} (offered={offered})",
        )
    return True, "OK", f"booted, admitted={admitted} (offered={offered}, src={src})"


def selftest():
    good = (
        "Test mission: dev-missions/terrain-brush.Intro/mission.sqm -> /tmp/x/Missions/terrain-brush.Intro/mission.sqm\n"
        "Mission start phase: StartAutoTest total 1234 ms -- frames from here\n"
        "Retained admission census: 178487 objects offered, 173564 admitted (97.2%), 388 distinct shapes\n"
    )
    menu = (
        "Test mission: dev-missions/terrain-brush.Intro/mission.sqm -> /tmp/x/Missions/mission.sqm\n"
        "Retained admission census: 0 objects offered, 0 admitted (0.0%), 0 distinct shapes\n"
        "Retained admission census: NOTHING was ever offered.\n"
    )
    dlg = menu + "DIAG single PLAY: starting intro, world=noe dir=missions/__cur_sp.noe/ file=RHII\n"
    low = good.replace("173564 admitted", "49 admitted").replace("178487 objects", "50 objects")
    noboot_census = "Retained admission census: 5 objects offered, 5 admitted\n"
    assert judge(good, 500) == (True, "OK", judge(good, 500)[2]) and judge(good, 500)[0]
    v, r, _ = judge(menu, 500)
    assert not v and r == "NO_BOOT", (v, r)
    v, r, _ = judge(dlg, 500)
    assert not v and r == "INPUT_DIALOG", (v, r)
    v, r, d = judge(low, 500)
    assert not v and r == "ADMITTED_LOW" and "admitted=49" in d, (v, r, d)
    v, r, _ = judge(noboot_census, 1)
    assert not v and r == "NO_BOOT", (v, r)
    boot_nocensus = "Mission start phase: StartAutoTest total 9 ms\n"
    v, r, _ = judge(boot_nocensus, 1)
    assert not v and r == "NO_CENSUS", (v, r)
    assert is_denylisted("f191f13adeadbeef") and not is_denylisted("e248000000000000")
    print("check-capture: selftest OK (valid/menu/dialog/low/nocensus/denylist)")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", default=None)
    ap.add_argument("--min-admitted", type=int, default=500)
    ap.add_argument("--png", default=None)
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    if not args.log or not os.path.isfile(args.log):
        print("INVALID NO_LOG no game log to judge")
        return 1
    with open(args.log, "r", encoding="utf-8", errors="replace") as fh:
        text = fh.read()
    digest = md5_of(args.png) if args.png else None
    valid, reason, detail = judge(text, args.min_admitted, digest)
    print(f"{'VALID' if valid else 'INVALID'} {reason} {detail}")
    return 0 if valid else 1


if __name__ == "__main__":
    sys.exit(main())
