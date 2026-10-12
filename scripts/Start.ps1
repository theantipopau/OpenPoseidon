<#
.SYNOPSIS
    Launch a built Poseidon binary against the retail (or Demo) game data.
.DESCRIPTION
    Run Install.ps1 once first: the retail game lacks files the remaster needs and
    the engine aborts without them.
.PARAMETER App         Game (default), GameDemo, or Server.
.PARAMETER Preset      CMake preset whose dist output to run (default win-x64-clang-rwdi).
.PARAMETER Demo        Use Demo data instead of retail.
.PARAMETER Fullscreen  Run fullscreen (default windowed).
.PARAMETER LogFile     Also write the engine log here (--log-file).
.PARAMETER Dx12        Run the wgpu renderer on DirectX 12 (WGPU_BACKEND=dx12). Needed for FSR 4;
                       the default is Vulkan. Set WGR_FSR4_DIR to the FidelityFX SDK signedbin folder
                       if the FSR 4 DLLs are not beside the exe.
.EXAMPLE
    .\scripts\Start.ps1
.EXAMPLE
    .\scripts\Start.ps1 -Fullscreen -nosplash
#>
[CmdletBinding()]
param(
    [ValidateSet('Game', 'GameDemo', 'Server')] [string]$App = 'Game',
    [string]$Preset = 'win-x64-clang-rwdi',
    [switch]$Demo,
    [switch]$Fullscreen,
    [string]$LogFile,
    [switch]$Dx12,
    # Position=0 makes the other params named-only, so engine flags (--check,
    # -nosplash, ...) pass through instead of binding to -App.
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)] $GameArgs
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\CwaCommon.ps1"

# Optional local config (gitignored; see mods.example.json). Holds data_path and the mod list.
$repoRoot = Split-Path $PSScriptRoot -Parent
$modsCfg  = if (Test-Path (Join-Path $repoRoot 'mods.json')) { Get-Content (Join-Path $repoRoot 'mods.json') -Raw | ConvertFrom-Json } else { $null }

if (-not $Demo -and $modsCfg -and $modsCfg.data_path)
{
    $data = $modsCfg.data_path
}
elseif ($Demo)
{
    $data = Get-CwaDemoDir
    if (-not $data) { throw "Demo install (Steam App $CwaDemoAppId) not found in registry." }
}
else
{
    $data = Get-CwaRetailDir
    if (-not $data) { throw "Retail install (Steam App $CwaRetailAppId) not found in registry. Set data_path in mods.json." }
}

# dist suffix is the preset's last segment (rwdi/dbg/rel).
$suffix   = ($Preset -split '-')[-1]
# The shipped client is ProteusGame.exe; the other two apps keep their Poseidon* names.
$exeName  = @{ Game = 'ProteusGame.exe'; GameDemo = 'PoseidonGameDemo.exe'; Server = 'PoseidonServer.exe' }[$App]
$exe      = Join-Path $repoRoot "dist\x64-win-$suffix\$exeName"
if (-not (Test-Path $exe)) { throw "Not built: $exe`n  Build first: .\scripts\Build.ps1 -Preset $Preset" }

$argv = @('-C', $data)
# Last-listed mod wins, so list mods lowest priority first.
if ($modsCfg -and $modsCfg.mods_dir) { $argv += @('--mods-dir', $modsCfg.mods_dir) }
if ($modsCfg -and $modsCfg.mods)     { $argv += "--mod=$($modsCfg.mods -join ';')" }
if (-not $Fullscreen) { $argv += '--window' }
if ($LogFile)         { $argv += @('--log-file', $LogFile) }
if ($GameArgs)        { $argv += $GameArgs }
# The renderer picks its wgpu backend from the environment. DX12 is opt-in: FSR 4 runs only there.
if ($Dx12)            { $env:WGPU_BACKEND = 'dx12'; Write-Host "Renderer:  DirectX 12 (WGPU_BACKEND=dx12)" }

Write-Host "Launching: $exe"
Write-Host "Data:      $data`n"

# The engine resolves its shipped assets (assets\menu\RoughSea.Intro, the sea menu scene) against the
# working directory, so run from the folder that holds the exe rather than the caller's directory.
Push-Location (Split-Path $exe -Parent)
try { & $exe @argv } finally { Pop-Location }
exit $LASTEXITCODE
