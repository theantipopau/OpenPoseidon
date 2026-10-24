#!/usr/bin/env python3
"""Hunk-scoped format check for Proteus (G3).

Checks formatting of CHANGED LINES only (vs a base commit), so pre-existing
drift can never fail it:

- Rust: `rustfmt --check` recurses from a crate root (lib.rs) into every
  module, and `clang-format --dry-run` flags whole files. Both therefore fail
  on drift the ticket never touched. Here we run the same tools but only fail
  when a reported violation lands inside a line the ticket added
  (`git diff -U0 <base>` added ranges, expanded by a few lines of fuzz because
  rustfmt's "Diff in" headers reference the pre-format numbering).

Usage: python scripts/proteus/check-format.py --base <sha>
Exit: 0 clean (or nothing to check / checker missing with warning),
      1 violations on changed lines.
      --selftest exercises the parsers on canned input (no tools needed).
"""

import argparse
import bisect
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
VS_CLANG_FORMAT = (
    "C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/Llvm/x64"
    "/bin/clang-format.exe"
)
# Fuzz per tool: rustfmt's "Diff in" headers reference pre-format numbering, so
# tolerate small shifts; clang-format XML offsets address the worktree file
# exactly, so no fuzz (fuzz would spill pre-existing in-statement reflow drift
# onto adjacent added lines -- proven on DebugOverlay.cpp DrawTemporalTab).
FUZZ_RS = 3
FUZZ_CPP = 0


def run(cmd, **kw):
    # Decode as UTF-8 explicitly: `text=True` uses the locale encoding, and on Windows
    # that is cp1252, which raises on the UTF-8 punctuation (curly quotes, arrows) that
    # the docs and comments already contain. A diff touching one of those lines crashed
    # this gate with an AttributeError on stdout=None instead of reporting a verdict.
    return subprocess.run(
        cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=ROOT, **kw
    )


def changed_files(base):
    p = run(["git", "diff", "--name-only", "--diff-filter=ACMRT", base])
    if p.returncode != 0:
        raise SystemExit(f"check-format: git diff failed: {p.stderr.strip()}")
    return [l.strip().replace("\\", "/") for l in p.stdout.splitlines() if l.strip()]


def added_ranges(base, files):
    """Repo-relative path -> list of (start, end) NEW-file line ranges added vs base."""
    if not files:
        return {}
    p = run(["git", "diff", "-U0", base, "--"] + files)
    if p.returncode != 0:
        raise SystemExit(f"check-format: git diff -U0 failed: {p.stderr.strip()}")
    ranges, cur = {}, None
    for line in p.stdout.splitlines():
        m = re.match(r"^\+\+\+ b/(.*)$", line)
        if m:
            cur = m.group(1)
            ranges.setdefault(cur, [])
            continue
        m = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if m and cur is not None:
            start, count = int(m.group(1)), int(m.group(2) or 1)
            if count > 0:
                ranges[cur].append((start, start + count - 1))
    return ranges


def in_added(ranges, path, lineno, fuzz):
    return any(s - fuzz <= lineno <= e + fuzz for s, e in ranges.get(path, []))


def repo_rel(p):
    """Normalise a tool-reported path to repo-relative forward-slash form."""
    p = p.strip()
    # Extended-length prefix arrives with native separators (\\?\C:\...) -- strip
    # BEFORE slash normalisation (afterwards it would read //?/ and be missed).
    for pre in ("\\\\?\\", "\\?\\"):
        if p.startswith(pre):
            p = p[len(pre):]
            break
    p = p.replace("\\", "/")
    if p.startswith("//?/"):
        p = p[4:]
    if os.path.isabs(p):
        try:
            p = os.path.relpath(p, ROOT).replace("\\", "/")
        except ValueError:
            pass
    return p


def check_rust(base, files, ranges):
    eddy = lambda f: "2024" if f.startswith("engine/WgpuRenderer/rust/") else "2021"
    violations = []
    for f in files:
        if not f.endswith(".rs"):
            continue
        absf = os.path.join(ROOT, f)
        p = run(["rustfmt", "--edition", eddy(f), "--check", absf])
        if p.returncode == 0:
            continue
        # rustfmt --check exits 1 with "Diff in <file>:<line>:" headers (it may
        # also recurse into sibling modules of a crate root -- those resolve to
        # paths outside `files` and are ignored by the membership test below).
        for m in re.finditer(r"^Diff in (.+?):(\d+):", p.stdout + p.stderr, re.M):
            rp = repo_rel(m.group(1))
            ln = int(m.group(2))
            if rp in ranges and in_added(ranges, rp, ln, FUZZ_RS):
                violations.append(f"{rp}:{ln}")
    return sorted(set(violations))


def find_clang_format():
    return shutil.which("clang-format") or (
        VS_CLANG_FORMAT if os.path.isfile(VS_CLANG_FORMAT) else None
    )


def check_cpp(files, ranges):
    cf = find_clang_format()
    if cf is None:
        print("check-format: WARNING: clang-format not found, skipping C++ check")
        return []
    cpp = [f for f in files if re.search(r"\.(cpp|hpp|h|c|cc)$", f)]
    violations = []
    for f in cpp:
        absf = os.path.join(ROOT, f)
        with open(absf, "rb") as fh:
            content = fh.read()
        newlines = [m.start() for m in re.finditer(b"\n", content)]
        p = run([cf, "--output-replacements-xml", absf])
        for m in re.finditer(r"<replacement offset='(\d+)'", p.stdout):
            off = int(m.group(1))
            ln = bisect.bisect_right(newlines, off) + 1
            if in_added(ranges, f, ln, FUZZ_CPP):
                violations.append(f"{f}:{ln}")
    return sorted(set(violations))


def selftest():
    # added_ranges parser
    diff = (
        "diff --git a/foo.rs b/foo.rs\n"
        "+++ b/foo.rs\n"
        "@@ -10,0 +11,3 @@ ctx\n"
        "@@ -20,5 +24,0 @@ ctx\n"
        "+++ b/empty.rs\n"
    )
    # parse via the real function with git output stubbed out: emulate by
    # monkeypatching run()
    global run
    real_run = run
    try:
        run = lambda cmd, **kw: type("P", (), {"returncode": 0, "stdout": diff, "stderr": ""})()
        got = added_ranges("BASE", ["foo.rs"])
    finally:
        run = real_run
    assert got == {"foo.rs": [(11, 13)], "empty.rs": []}, got
    assert in_added(got, "foo.rs", 12, 3) and not in_added(got, "foo.rs", 50, 3)
    assert not in_added(got, "foo.rs", 8, 0) and in_added(got, "foo.rs", 11, 0)
    # repo_rel normalisation (extended-length prefix with native separators).
    # Build the input programmatically to avoid backslash-escape soup: it must
    # denote a path UNDER ROOT for relpath to resolve inside the repo.
    win_abs = chr(92) * 2 + "?" + chr(92) + os.path.join(ROOT, "engine", "x.rs").replace("/", chr(92))
    assert repo_rel(win_abs) == "engine/x.rs", win_abs
    assert repo_rel("//?/" + ROOT + "/engine/x.rs") == "engine/x.rs"
    assert repo_rel(os.path.join(ROOT, "engine", "x.rs")) == "engine/x.rs"
    # rustfmt header parsing incl. recursion into unchanged files
    sample = "Diff in /repo/engine/WgpuRenderer/rust/src/lib.rs:42:\nDiff in /repo/other.rs:7:\n"
    hits = [(repo_rel(m.group(1)), int(m.group(2))) for m in re.finditer(r"^Diff in (.+?):(\d+):", sample, re.M)]
    assert hits[0][1] == 42 and hits[1][1] == 7, hits
    # clang-format offset -> line mapping
    blob = b"int a;\nint b;\nint c;\n"
    nls = [m.start() for m in re.finditer(b"\n", blob)]
    assert bisect.bisect_right(nls, 0) + 1 == 1
    assert bisect.bisect_right(nls, 8) + 1 == 2
    print("check-format: selftest OK (hunks, paths, headers, offsets)")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default=None)
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    base = args.base or "HEAD~1"
    files = changed_files(base)
    if not files:
        print("check-format: no changed files")
        return 0
    ranges = added_ranges(base, files)
    bad = check_rust(base, files, ranges) + check_cpp(files, ranges)
    if bad:
        print("check-format: FAIL -- violations on changed lines:")
        for v in bad:
            print(f"  {v}")
        return 1
    n_rs = sum(1 for f in files if f.endswith(".rs"))
    n_cpp = sum(1 for f in files if re.search(r"\.(cpp|hpp|h|c|cc)$", f))
    print(f"check-format: OK ({len(files)} changed files; {n_rs} rs, {n_cpp} c++ scoped)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
