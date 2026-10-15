# T18b: fixed-flyby GPU/CPU timing capture.
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/proteus/perf-capture.ps1 `
#     -Res 1440p -Scene coast [-Iters 3] [-Ab 'WGR_GTAO_SCALE=1,WGR_GTAO_SCALE=2'] [-Backend dx12]
# Runs the game --benchmark (300 frames, self-exits) per config, parses the log with
# perf_parse.py, writes CSV + sidecar JSON to docs/proteus/perf/.
# Validity (W0-ORDER): each run's game log must prove the test mission booted
# (StartAutoTest) and populated (admitted >= min_admitted from scenes.yaml).
# INVALID runs retry 2x, then the log moves to perf/rejected/ and is excluded
# from the summary; a config with zero valid iters fails the capture.
# Exits: 0 ok | 2 no built exe | 3 no game data | 4 launch/timeout failure | 6 invalid content.
# NOTE: --benchmark is 300 frames (~5 s of play per World.cpp comment); PERF-CAP emits one
# 120-frame window line, so each run contributes its last complete window. Use -Iters 5+
# for meaningful p95/p99 (percentiles are across iterations, not frames).
param(
  [ValidateSet('1080p', '1440p', '4K')][string]$Res = '1440p',
  [string]$Scene = 'open',
  [int]$Iters = 1,
  [string]$Ab = '',
  [ValidateSet('vulkan', 'dx12')][string]$Backend = 'vulkan'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$Sizes = @{ '1080p' = @(1920, 1080); '1440p' = @(2560, 1440); '4K' = @(3840, 2160) }
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$Commit = (git -C $root rev-parse --short HEAD 2>$null); if (-not $Commit) { $Commit = 'nogit' }

$exe = Join-Path $root 'dist/x64-win-rwdi/ProteusGame.exe'
if (-not (Test-Path $exe)) { Write-Host 'perf-capture: no built exe (run the win-x64-clang-rwdi build first)'; exit 2 }
$mods = Join-Path $root 'mods.json'
if (-not (Test-Path $mods)) { Write-Host 'perf-capture: mods.json missing (copy mods.example.json first)'; exit 3 }
$cfg = (Get-Content $mods -Raw) | ConvertFrom-Json
$data = $cfg.data_path
if (-not $data -or -not (Test-Path $data)) { Write-Host "perf-capture: data_path missing or not found: $data"; exit 3 }
# Mirror Start.ps1 mod wiring (last-listed mod wins); run from the exe dir for shipped assets.
$modArgs = @('-C', $data)
if ($cfg.mods_dir) { $modArgs += @('--mods-dir', $cfg.mods_dir) }
if ($cfg.mods) { $modArgs += "--mod=$($cfg.mods -join ';')" }

$scenes = Get-Content (Join-Path $PSScriptRoot 'scenes.yaml') -Raw
if ($scenes -notmatch "(?m)^\s+- name: $Scene\s*\r?$") { Write-Host "perf-capture: unknown scene '$Scene' (see scenes.yaml)"; exit 3 }
$mission = [regex]::Match($scenes, "(?ms)- name: $Scene\s*\r?\n\s*mission:\s*(\S+)").Groups[1].Value
$missionPath = Join-Path $root $mission
if (-not (Test-Path $missionPath)) { Write-Host "perf-capture: scene mission missing: $missionPath"; exit 3 }
$minAdmitted = 100
$blk = [regex]::Match($scenes, "(?ms)- name: $Scene\b(.*?)(?=- name:|\z)").Groups[1].Value
$mm = [regex]::Match($blk, 'min_admitted:\s*(\d+)')
if ($mm.Success) { $minAdmitted = [int]$mm.Groups[1].Value }

$env:POSEIDON_GPU_STATS = '1'
if ($Backend -eq 'dx12') { $env:WGPU_BACKEND = 'dx12' } else { $env:WGPU_BACKEND = 'vulkan' }
$w, $h = $Sizes[$Res]
$outDir = Join-Path $root 'docs/proteus/perf'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$rejDir = Join-Path $outDir 'rejected'
$rejected = @()

$configs = if ($Ab) { $Ab -split ',' } else { @('baseline') }
$logs = @()
foreach ($cfg in $configs) {
  for ($i = 1; $i -le $Iters; $i++) {
    # Filenames must not contain '=' (perf_run splits pairs, not strings, but '=' in a name
    # still confuses humans and shells) — sanitise the config arm.
    $safe = ($cfg -replace '[^A-Za-z0-9]+', '_').Trim('_')
    $log = Join-Path ([IO.Path]::GetTempPath()) ("perf-$Scene-$Res-$Stamp-$safe-$i.log")
    $envSkin = @{}
    if ($cfg -ne 'baseline') {
      $kv = $cfg -split '=', 2
      [System.Environment]::SetEnvironmentVariable($kv[0], $kv[1], 'Process')
      $envSkin[$kv[0]] = $kv[1]
    }
    $validRun = $false
    for ($att = 1; $att -le 3 -and -not $validRun; $att++) {
    if ($att -gt 1) { Write-Host "perf-capture: retrying [$i/$Iters] (attempt $att/3)" }
    Write-Host "== run $cfg [$i/$Iters]: $Scene $Res $Backend =="
    # Single quoted command line: PS 5.1 Start-Process splits ARRAY elements on spaces
    # (e.g. data_path 'F:/New folder/...'), so quote every path explicitly.
    $args = ($modArgs + @('--window', '--width', $w, '--height', $h,
      '--test-mission', $missionPath, '--benchmark', '--no-dev', '--log-file', $log) | ForEach-Object {
        if ($_ -match '\s') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
      }) -join ' '
    Push-Location (Split-Path $exe -Parent)
    try {
      $p = Start-Process -FilePath $exe -ArgumentList $args -PassThru -NoNewWindow
      # NOTE: Windows PowerShell 5.1 Wait-Process has no -PassThru; use .NET WaitForExit.
      if (-not $p.WaitForExit(600000)) { try { $p | Stop-Process -Force } catch {}; Write-Host 'perf-capture: game timed out'; exit 4 }
    } finally { Pop-Location }
    $p.Refresh()
    # The benchmark path can exit via a fast shutdown that leaves ExitCode unreadable;
    # BENCHMARK RESULT in the log is the ground truth for a completed run.
    $done = Select-String -LiteralPath $log -Pattern 'BENCHMARK RESULT' -Quiet -ErrorAction SilentlyContinue
    # Missing BENCHMARK RESULT after a TIMEOUT stays fatal; a clean early quit
    # retries like INVALID content (same W0-ORDER lesson as golden-capture).
    if (-not $done) {
      Write-Host "perf-capture: game exited $($p.ExitCode) without BENCHMARK RESULT (see $log)"
      if ($att -ge 3) { exit 4 }
      continue
    }
    $verdict = & python (Join-Path $PSScriptRoot 'check-capture.py') --log $log --min-admitted $minAdmitted 2>&1
    $code = $LASTEXITCODE
    Write-Host "perf-capture: validity: $($verdict -join ' ')"
    if ($code -eq 0) { $validRun = $true }
    } # attempts
    foreach ($k in $envSkin.Keys) { [System.Environment]::SetEnvironmentVariable($k, $null, 'Process') }
    if (-not $validRun) {
      New-Item -ItemType Directory -Force -Path $rejDir | Out-Null
      $rej = Join-Path $rejDir (Split-Path $log -Leaf)
      Move-Item $log $rej -Force
      $rejected += [pscustomobject]@{ Config = $cfg; Iter = $i; Log = $rej }
      Write-Host "perf-capture: iter [$i/$Iters] INVALID after 3 attempts -> rejected/ (excluded from summary)"
      continue
    }
    $logs += [pscustomobject]@{ Config = $cfg; Iter = $i; Log = $log }
  }
}

if ($logs.Count -eq 0) { Write-Host 'perf-capture: no VALID iters (all rejected; see perf/rejected/)'; exit 6 }
$csv = Join-Path $outDir "$Stamp-$Commit-$Res-$Scene.csv"
$runArgs = @()
$counters = @{}
foreach ($l in $logs) {
  if (-not $counters.ContainsKey($l.Config)) { $counters[$l.Config] = 0 }
  $counters[$l.Config] += 1
  $runArgs += @('--run', "$($l.Config)#$($counters[$l.Config])", '--log', $l.Log)
}
# NOTE: New-TemporaryFile returns FileInfo; '>' needs a plain path string, so build one.
$toolOut = Join-Path ([IO.Path]::GetTempPath()) ("perftool-$Stamp.log")
python (Join-Path $PSScriptRoot 'perf_run.py') --csv $csv --res $Res --scene $Scene `
  --backend $Backend @runArgs > $toolOut 2>&1
if ($LASTEXITCODE -ne 0) { Get-Content $toolOut; Write-Host 'perf-capture: parse failed'; exit 4 }
$summaryJson = ((Get-Content $toolOut) | Where-Object { $_ -match '^PERF-SUMMARY ' }) -replace '^PERF-SUMMARY ', ''
Remove-Item $toolOut

$gpuName = 'unknown'
try { $gpuName = ((Get-CimInstance Win32_VideoController | Select-Object -First 1).Name) } catch {}
$sidecar = @{
  date = $Stamp; commit = $Commit; res = "$w`x$h"; scene = $Scene; backend = $Backend
  gpu = $gpuName; driver = 'see log'; exe = $exe; mission = $mission
  env = @{ POSEIDON_GPU_STATS = '1'; WGPU_BACKEND = $env:WGPU_BACKEND }
  ab = $configs; iters = $Iters; validIters = $logs.Count; rejected = @($rejected | ForEach-Object { "$($_.Config)#$($_.Iter): $(Split-Path $_.Log -Leaf)" }); summary = ($summaryJson | ConvertFrom-Json)
} | ConvertTo-Json -Depth 6
# NOTE: Out-File -Encoding utf8 writes a BOM that Python's json.load chokes on, and
# PS 5.1 has no utf8NoBOM — write BOM-less bytes explicitly so our own sidecar stays readable.
[IO.File]::WriteAllText((Join-Path $outDir "$Stamp-$Commit-$Res-$Scene.json"), $sidecar, [Text.UTF8Encoding]::new($false))
Write-Host "perf-capture: wrote $csv (+ sidecar json)"
