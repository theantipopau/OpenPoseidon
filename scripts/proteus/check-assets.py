#!/usr/bin/env python3
"""G2: fail if any file under the asset dirs is missing from assets/registry.yaml.

Asset dirs: assets/, resources/sky/, resources/icon-*, screenshots/.
Also fails on stale entries (registry lists a file that no longer exists) and on
entries missing required fields. Exit 0 = clean, 1 = violations, 2 = tool error.
"""
import fnmatch
import os
import sys

try:
    import yaml
except ImportError:
    print("check-assets: ERROR: PyYAML is required (python -m pip install pyyaml)",
          file=sys.stderr)
    sys.exit(2)

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REGISTRY = os.path.join(REPO, "assets", "registry.yaml")
REQUIRED = ("path", "source", "author", "licence", "spdx", "date_added")

ASSET_PATTERNS = ("assets/*", "resources/sky/*", "resources/icon-*", "screenshots/*")


def asset_files():
    found = []
    for root, _dirs, files in os.walk(REPO):
        for name in files:
            full = os.path.join(root, name)
            rel = os.path.relpath(full, REPO).replace(os.sep, "/")
            if rel == "assets/registry.yaml":
                continue
            if any(fnmatch.fnmatch(rel, pat) or rel.startswith(pat[:-1])
                   for pat in ASSET_PATTERNS):
                found.append(rel)
    return sorted(found)


def main():
    try:
        with open(REGISTRY, encoding="utf-8") as fh:
            data = yaml.safe_load(fh)
    except (OSError, yaml.YAMLError) as exc:
        print(f"check-assets: ERROR: cannot read registry: {exc}", file=sys.stderr)
        return 2
    entries = (data or {}).get("assets", [])
    errors = []
    seen = set()
    for i, entry in enumerate(entries):
        if not isinstance(entry, dict):
            errors.append(f"entry #{i} is not a mapping")
            continue
        for field in REQUIRED:
            if field not in entry or entry[field] in (None, ""):
                errors.append(f"{entry.get('path', f'entry #{i}')}: missing field '{field}'")
        path = entry.get("path", "")
        if path in seen:
            errors.append(f"{path}: listed twice")
        seen.add(path)
        if path and not os.path.isfile(os.path.join(REPO, path)):
            errors.append(f"{path}: in registry but file is missing (stale entry)")
    on_disk = asset_files()
    for path in on_disk:
        if path not in seen:
            errors.append(f"{path}: on disk but not in assets/registry.yaml")
    if errors:
        print(f"check-assets: FAIL ({len(errors)} problem(s)):")
        for err in errors:
            print(f"  - {err}")
        return 1
    print(f"check-assets: OK ({len(on_disk)} files, all registered)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
