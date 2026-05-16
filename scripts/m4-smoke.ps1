# m4-smoke.ps1 — M4 acceptance test preparation script
#
# Spawns geulosd (server-host) and geulos-echo-app in the background,
# waits 2 seconds for them to initialize, then prints instructions for
# the next manual step (launching the compositor).
#
# Usage:
#   .\scripts\m4-smoke.ps1
#
# Prerequisites:
#   cargo build -p geulos-server-host -p geulos-echo-app -p geulos-compositor

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $root "target\debug"

$serverBin = Join-Path $binDir "geulosd.exe"
$echoBin   = Join-Path $binDir "geulos-echo-app.exe"
$compBin   = Join-Path $binDir "geulos-compositor.exe"

foreach ($bin in @($serverBin, $echoBin, $compBin)) {
    if (-not (Test-Path $bin)) {
        Write-Error "Binary not found: $bin`nRun: cargo build -p geulos-server-host -p geulos-echo-app -p geulos-compositor"
    }
}

Write-Host "[m4-smoke] Starting geulosd (server-host)..."
$serverProc = Start-Process -FilePath $serverBin -PassThru -WindowStyle Minimized

Write-Host "[m4-smoke] Waiting 1 s for server to bind..."
Start-Sleep -Seconds 1

Write-Host "[m4-smoke] Starting geulos-echo-app..."
$echoProc = Start-Process -FilePath $echoBin -PassThru -WindowStyle Minimized

Write-Host "[m4-smoke] Waiting 2 s for echo-app to mount objects..."
Start-Sleep -Seconds 2

Write-Host ""
Write-Host "============================================================"
Write-Host "  Server PID : $($serverProc.Id)"
Write-Host "  Echo-app PID: $($echoProc.Id)"
Write-Host ""
Write-Host "  Background processes are ready."
Write-Host "  Now start the compositor manually in a new terminal:"
Write-Host ""
Write-Host "    .\target\debug\geulos-compositor.exe"
Write-Host ""
Write-Host "  Expected: 800x600 window 'GeulOS Compositor (M4)' showing"
Write-Host "  a Container with Text (count: 0) and a Button."
Write-Host "  Clicking the Button should increment the counter."
Write-Host ""
Write-Host "  To stop background processes when done:"
Write-Host "    Stop-Process -Id $($serverProc.Id), $($echoProc.Id)"
Write-Host "============================================================"
