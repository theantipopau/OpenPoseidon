# T19: golden-image capture. Runs each shot's mission, fires --auto-screenshot at the
# shot's time trigger (engine auto-exits 2 frames after the last capture), collects PNGs.
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/golden-capture.ps1 `
#     [-Res 1080p] [-Shots open,coast] [-Backend dx12] [-Bless]
# Baselines live OUTSIDE the repo: $env:PROTEUS_GOLDENS, else %LOCALAPPDATA%/Proteus/goldens/<res>.
# Only -Bless writes there, and only when YOU pass it. Compare with golden-compare.py.
# Validity (W0-ORDER): every shot's game log must prove the test mission booted
# (StartAutoTest) and populated (admitted >= min_admitted); the candidate PNG
# must not match the menu-backdrop denylist. INVALID shots retry 2x, then the
# run fails and -Bless refuses. check-capture.py is the shared judge.
# Exits: 0 ok | 2 no built exe | 3 no game data / unknown shot | 4 launch/timeout/missing PNG | 6 invalid content.
param(
  [ValidateSet('1080p', '1440p')][string]$Res = '1080p',
  [string]$Shots = '',
  [ValidateSet('vulkan', 'dx12')][string]$Backend = 'vulkan',
  [switch]$Bless
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$Sizes = @{ '1080p' = @(1920, 1080); '1440p' = @(2560, 1440) }
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'

$exe = Join-Path $root 'dist/x64-win-rwdi/ProteusGame.exe'
if (-not (Test-Path $exe)) { Write-Host 'golden-capture: no built exe (run the win-x64-clang-rwdi build first)'; exit 2 }
$mods = Join-Path $root 'mods.json'
if (-not (Test-Path $mods)) { Write-Host 'golden-capture: mods.json missing (copy mods.example.json first)'; exit 3 }
$cfg = (Get-Content $mods -Raw) | ConvertFrom-Json
$data = $cfg.data_path
if (-not $data -or -not (Test-Path $data)) { Write-Host "golden-capture: data_path missing or not found: $data"; exit 3 }
$modArgs = @('-C', $data)
if ($cfg.mods_dir) { $modArgs += @('--mods-dir', $cfg.mods_dir) }
if ($cfg.mods) { $modArgs += "--mod=$($cfg.mods -join ';')" }

# Minimal shots.yaml reader (same no-dependency regex style as perf-capture.ps1).
$text = Get-Content (Join-Path $PSScriptRoot 'shots.yaml') -Raw
# NOTE: PowerShell variables are case-insensitive, so the table CANNOT be called $shots
# (it would clobber the -Shots param). $shotList it is.
$shotList = @()
foreach ($m in [regex]::Matches($text, '(?ms)- name: (\S+)\s*\r?\n\s*mission: (\S+)\s*\r?\n\s*trigger: (\S+)')) {
  $shotList += [pscustomobject]@{ Name = $m.Groups[1].Value; Mission = $m.Groups[2].Value; Trigger = $m.Groups[3].Value; MinAdmitted = 0 }
}
# min_admitted lives later in each shot's block; default 500 when absent.
foreach ($s in $shotList) {
  $blk = [regex]::Match($text, "(?ms)- name: $($s.Name)\b(.*?)(?=- name:|\z)").Groups[1].Value
  $mm = [regex]::Match($blk, 'min_admitted:\s*(\d+)')
  $s.MinAdmitted = if ($mm.Success) { [int]$mm.Groups[1].Value } else { 500 }
}
$want = if ($Shots) { $Shots -split ',' } else { $shotList | ForEach-Object { $_.Name } }
foreach ($s in $want) {
  if ($shotList.Name -notcontains $s) { Write-Host "golden-capture: unknown shot '$s' (see shots.yaml)"; exit 3 }
}

$baseRoot = if ($env:PROTEUS_GOLDENS) { $env:PROTEUS_GOLDENS } else { Join-Path $env:LOCALAPPDATA "Proteus/goldens/$Res" }
$candDir = Join-Path ([IO.Path]::GetTempPath()) "golden-$Stamp"
New-Item -ItemType Directory -Force -Path $candDir | Out-Null
if ($Bless) { New-Item -ItemType Directory -Force -Path $baseRoot | Out-Null }

# Crash watch: the game writes minidumps to <Documents>/Cold War Assault/crashes/ (via
# GamePaths::UserContentDir, NOT Temp). Snapshot before, diff after — a crash can hide behind
# a successful-looking exit, e.g. in the auto-exit fast-shutdown path after the PNG is written.
$crashDir = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Cold War Assault/crashes'
$crashBefore = @()
if (Test-Path $crashDir) { $crashBefore = @(Get-ChildItem $crashDir -Filter 'crash-*.dmp' | ForEach-Object { $_.Name }) }

if ($Backend -eq 'dx12') { $env:WGPU_BACKEND = 'dx12' } else { $env:WGPU_BACKEND = 'vulkan' }
$w, $h = $Sizes[$Res]
foreach ($s in ($shotList | Where-Object { $want -contains $_.Name })) {
  $missionPath = Join-Path $root $s.Mission
  if (-not (Test-Path $missionPath)) { Write-Host "golden-capture: shot '$($s.Name)' mission missing: $missionPath"; exit 3 }
  $out = Join-Path $candDir ($s.Name + '.png')
  # Always keep a game log next to the candidate: without it a broken PNG (black
  # frame, early capture, crash without dump) is undiagnosable after the fact.
  $glog = Join-Path $candDir ($s.Name + '.log')
  Write-Host "== shot $($s.Name) [$($s.Trigger)] =="
  # PS 5.1 Start-Process splits ARRAY elements on spaces — quote every path explicitly.
  $args = ($modArgs + @('--window', '--width', $w, '--height', $h,
    '--test-mission', $missionPath, '--auto-screenshot', "$($s.Trigger):$out",
    '--no-dev', '--log-file', $glog) | ForEach-Object {
      if ($_ -match '\s') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
    }) -join ' '
  $valid = $false
  for ($att = 1; $att -le 3 -and -not $valid; $att++) {
    if ($att -gt 1) { Write-Host "golden-capture: retrying shot '$($s.Name)' (attempt $att/3)" }
    Push-Location (Split-Path $exe -Parent)
    try {
      # Same launch shape as perf-capture.ps1 (proven): no stdin redirect — PS 5.1 resolves
      # '-RedirectStandardInput NUL' to a missing file and Start-Process throws FileNotFoundException.
      $p = Start-Process -FilePath $exe -ArgumentList $args -PassThru -NoNewWindow
      if (-not $p.WaitForExit(600000)) { try { $p | Stop-Process -Force } catch {}; Write-Host 'golden-capture: game timed out'; exit 4 }
    } finally { Pop-Location }
    # Missing PNG after a TIMEOUT stays fatal (exit 4 below covers attempt 3); a
    # clean early quit (W0-ORDER ride: game exits before the trigger fires)
    # retries like INVALID content — the next attempt usually captures fine.
    if (-not (Test-Path $out)) {
      Write-Host "golden-capture: shot '$($s.Name)' produced no PNG (see game log)"
      if ($att -ge 3) { exit 4 }
      continue
    }
    $verdict = & python (Join-Path $PSScriptRoot 'check-capture.py') --log $glog --min-admitted $s.MinAdmitted --png $out 2>&1
    $code = $LASTEXITCODE
    Write-Host "golden-capture: validity: $($verdict -join ' ')"
    if ($code -eq 0) { $valid = $true }
  }
  if (-not $valid) { Write-Host "golden-capture: shot '$($s.Name)' INVALID after 3 attempts (see $glog)"; exit 6 }
  if ($Bless) { Copy-Item $out (Join-Path $baseRoot ($s.Name + '.png')) -Force }
}
if (Test-Path $crashDir) {
  $newCrashes = @(Get-ChildItem $crashDir -Filter 'crash-*.dmp' | Where-Object { $crashBefore -notcontains $_.Name })
  foreach ($c in $newCrashes) { Write-Host "golden-capture: WARNING: new crash dump during capture: $($c.FullName)" }
}
# -Bless runs only after every requested shot validated (the loop exits 6
# otherwise), so reaching here with $Bless means all candidates are VALID.
if ($Bless) { Write-Host "golden-capture: blessed $($want.Count) VALID shot(s) -> $baseRoot" }
else { Write-Host "golden-capture: wrote $($want.Count) VALID candidate(s) -> $candDir" }
