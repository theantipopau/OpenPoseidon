# Proteus local checks (G3): asset registry + format of CHANGED files (never whole-tree,
# so pre-existing drift can't fail it). CI (.github/workflows/proteus.yml) runs the same gates.
# Missing optional tools warn-and-skip locally; CI installs them and is strict.
# New files: `git add -N <file>` (intent-to-add) before running, so they are visible to
# `git diff`. Untracked leftovers (e.g. someone else's headers) are never swept in.
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$failed = $false

Write-Host '== asset registry =='
python "$root/scripts/proteus/check-assets.py"
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host '== roadmap freshness =='
python "$root/scripts/proteus/gen-roadmap.py" --check
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host '== perf parser selftest =='
python "$root/scripts/proteus/perf_parse.py" --selftest
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host '== imgdiff selftest =='
python "$root/scripts/proteus/imgdiff.py" --selftest
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host '== format (changed hunks vs proteus/main) =='
Set-Location $root
$base = (git merge-base HEAD proteus/main 2>$null)
if (-not $base) { $base = 'HEAD~1' }
# Hunk-scoped (check-format.py): plain rustfmt recurses from a crate root into
# every module and clang-format flags whole files, so both fail on drift the
# ticket never touched. New files: `git add -N <file>` (intent-to-add) first so
# they are visible to `git diff`; untracked leftovers are never swept in.
python "$root/scripts/proteus/check-format.py" --base $base
if ($LASTEXITCODE -ne 0) { $failed = $true }

if ($failed) { Write-Host 'check-all: FAIL'; exit 1 }
Write-Host 'check-all: OK'
