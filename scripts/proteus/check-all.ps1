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

Write-Host '== format (changed files vs proteus/main) =='
Set-Location $root
$base = (git merge-base HEAD proteus/main 2>$null)
if (-not $base) { $base = 'HEAD~1' }
$changed = @(git diff --name-only --diff-filter=ACMRT $base) | Sort-Object -Unique

$rs = @($changed | Where-Object { $_ -like '*.rs' })
if ($rs.Count -eq 0) { Write-Host 'no changed .rs files' }
else {
  foreach ($f in $rs) {
    $ed = if ($f -like 'engine/WgpuRenderer/rust/*') { '2024' } else { '2021' }
    rustfmt --edition $ed --check $f
    if ($LASTEXITCODE -ne 0) { $failed = $true }
  }
}

$cpp = @($changed | Where-Object { $_ -match '\.(cpp|hpp|h|c|cc)$' })
if ($cpp.Count -eq 0) { Write-Host 'no changed C++ files' }
else {
  $cf = (Get-Command clang-format -ErrorAction SilentlyContinue)
  if (-not $cf) {
    $vsCf = 'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang-format.exe'
    if (Test-Path $vsCf) { $cf = $vsCf } else { $cf = $null }
  }
  if (-not $cf) { Write-Host 'WARNING: clang-format not found, skipping C++ format check'; }
  else { & $cf --dry-run --Werror @cpp; if ($LASTEXITCODE -ne 0) { $failed = $true } }
}

if ($failed) { Write-Host 'check-all: FAIL'; exit 1 }
Write-Host 'check-all: OK'
