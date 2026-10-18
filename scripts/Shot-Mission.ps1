<#
.SYNOPSIS
    Launch a dev mission through the test harness, screenshot it and keep the engine log.
.DESCRIPTION
    Uses --test-mission and --harness, so nothing needs a human at the keyboard. Reads
    data_path / mods from mods.json like Start.ps1. Output goes to -OutDir.
.PARAMETER Mission  A dev mission folder (default dev-missions/jeep-ride.Intro).
.PARAMETER Backend  vulkan (default) or dx12.
.PARAMETER Seconds  How long to run after the first screenshot (frame timing accumulates).
.EXAMPLE
    .\scripts\Shot-Mission.ps1 -Backend dx12 -OutDir E:\Proteus\shots\dx12
#>
[CmdletBinding()]
param(
    [string]$Mission = 'dev-missions/jeep-ride.Intro',
    [ValidateSet('vulkan', 'dx12')] [string]$Backend = 'vulkan',
    [int]$Seconds = 20,
    [string]$OutDir = (Join-Path $PSScriptRoot '..\shots'),
    [int]$Width = 1280,
    [int]$Height = 720
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$cfg  = Get-Content (Join-Path $repo 'mods.json') -Raw | ConvertFrom-Json
$exe  = Join-Path $repo 'dist\x64-win-rwdi\ProteusGame.exe'
$missionPath = if ([IO.Path]::IsPathRooted($Mission)) { $Mission } else { Join-Path $repo $Mission }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
if (Get-Process ProteusGame -ErrorAction SilentlyContinue) { throw 'Close the running game first.' }

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
$log = Join-Path $OutDir 'engine.log'
Remove-Item $log -ErrorAction SilentlyContinue

$argv = @('-C', ('"' + $cfg.data_path + '"'))
if ($cfg.mods_dir) { $argv += @('--mods-dir', ('"' + $cfg.mods_dir + '"')) }
if ($cfg.mods)     { $argv += ('--mod=' + ($cfg.mods -join ';')) }
$argv += @('--window', '--no-dev', '--width', "$Width", '--height', "$Height", '--harness', "$port",
           '--test-mission', ('"' + $missionPath + '"'), '--log-file', ('"' + $log + '"'))
if ($Backend -eq 'dx12') { $env:WGPU_BACKEND = 'dx12' } else { Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue }

$p = Start-Process -FilePath $exe -ArgumentList $argv -WorkingDirectory (Split-Path $exe) -PassThru
$client = [Net.Sockets.TcpClient]::new()
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(120)
    while (!$client.Connected) {
        try { $client.Connect('127.0.0.1', $port) }
        catch { if ($p.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'harness connect timed out' }; Start-Sleep -Milliseconds 300 }
    }
    $stream = $client.GetStream(); $stream.ReadTimeout = 60000
    $reader = [IO.StreamReader]::new($stream)
    $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false)); $writer.AutoFlush = $true
    function Send($c) {
        $writer.WriteLine(($c | ConvertTo-Json -Compress))
        do { $line = $reader.ReadLine(); if ($null -eq $line) { throw 'harness closed (game exited or lost its device)' }; $r = $line | ConvertFrom-Json } while ($null -eq $r.ok)
        return $r
    }
    Start-Sleep -Seconds 15
    $r = Send @{ cmd = 'screenshot'; path = (Join-Path $OutDir 'shot1.png') }
    "shot1: ok=$($r.ok)"
    Start-Sleep -Seconds $Seconds
    $r = Send @{ cmd = 'screenshot'; path = (Join-Path $OutDir 'shot2.png') }
    "shot2: ok=$($r.ok)"
} catch {
    Write-Warning "Run ended early: $_"
} finally {
    $exited = $p.HasExited
    if (!$exited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    "game exited on its own: $exited"
    if (Test-Path $log) {
        Select-String -Path $log -Pattern 'frame avg|device lost|UNHANDLED|FSR 4 disabled|Validation' |
            ForEach-Object { $_.Line.Substring([Math]::Min(40, $_.Line.Length)) } | Select-Object -Last 8
    }
}
