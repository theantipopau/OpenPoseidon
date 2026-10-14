# Proteus local checks: runs every fast local gate. G3 will extend this list (CI calls the same entry points).
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$failed = $false

Write-Host '== asset registry =='
python "$root/scripts/proteus/check-assets.py"
if ($LASTEXITCODE -ne 0) { $failed = $true }

if ($failed) { Write-Host 'check-all: FAIL'; exit 1 }
Write-Host 'check-all: OK'
