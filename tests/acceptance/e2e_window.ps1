# SnapCap window-capture end-to-end acceptance (real UI over WebView2 CDP).
#
# Flow: launch a controlled WinForms target window, launch SnapCap with an
# isolated config/output directory, click the "window" entry, select the real
# window in the overlay, press a real Enter key, then verify that the editor
# shows the bitmap and that a PNG with the exact window size was written.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\e2e_window.ps1
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\e2e_window.ps1 -Exe <path> -Port 9333
param(
  [string]$Exe = '',
  [string]$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
  [int]$Port = 0,
  [string]$ArtifactsDir = ''
)

$ErrorActionPreference = 'Stop'

if (-not $Exe) { $Exe = Join-Path $Repo 'releases\snapcap.exe' }
if (-not (Test-Path $Exe)) { throw "SnapCap executable not found: $Exe" }
$Exe = (Resolve-Path $Exe).Path

$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) { throw 'node not found in PATH (required for the CDP driver)' }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class SnapCapE2EWin {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@

$targetScript = Join-Path $PSScriptRoot 'window_target.ps1'
$flowScript = Join-Path $PSScriptRoot 'cdp_window_flow.mjs'
if (-not (Test-Path $targetScript)) { throw "missing $targetScript" }
if (-not (Test-Path $flowScript)) { throw "missing $flowScript" }

if (-not $ArtifactsDir) { $ArtifactsDir = Join-Path $PSScriptRoot 'artifacts' }
New-Item -ItemType Directory -Force -Path $ArtifactsDir | Out-Null
$ArtifactsDir = (Resolve-Path $ArtifactsDir).Path

# Isolated workspace: settings and screenshots never touch the user profile.
$work = Join-Path $env:TEMP ('snapcap-e2e-' + [guid]::NewGuid().ToString('N'))
$configDir = Join-Path $work 'config'
$outputDir = Join-Path $work 'out'
New-Item -ItemType Directory -Force -Path $configDir, $outputDir | Out-Null
$report = Join-Path $work 'target.json'
$settings = [ordered]@{ paths = [ordered]@{ output_dir = $outputDir } }
$settingsJson = $settings | ConvertTo-Json -Depth 4
# Windows PowerShell 5.1 writes a BOM with -Encoding UTF8; serde_json rejects it.
[System.IO.File]::WriteAllText(
  (Join-Path $configDir 'settings.json'),
  $settingsJson,
  (New-Object System.Text.UTF8Encoding($false))
)

if ($Port -le 0) {
  $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
  $listener.Stop()
}

$target = $null
$app = $null
$ok = $false
$oldCdp = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$oldConfig = $env:SNAPCAP_CONFIG_DIR
$savedPng = $null

try {
  # Any leftover instance of the binary under test would hold the global
  # hotkeys; stop only that exact binary, never other SnapCap builds.
  Get-Process snapcap -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $Exe } |
    Stop-Process -Force -ErrorAction SilentlyContinue

  $target = Start-Process powershell.exe -PassThru `
    -RedirectStandardOutput (Join-Path $work 'target.out') `
    -RedirectStandardError (Join-Path $work 'target.err') `
    -ArgumentList @(
      '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass',
      '-File', "`"$targetScript`"", '-Report', "`"$report`""
    )
  $deadline = (Get-Date).AddSeconds(20)
  while (-not (Test-Path $report)) {
    if ((Get-Date) -gt $deadline) { throw 'target window did not report its geometry' }
    Start-Sleep -Milliseconds 200
  }
  $targetInfo = Get-Content $report -Raw | ConvertFrom-Json

  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
  $env:SNAPCAP_CONFIG_DIR = $configDir
  $app = Start-Process -FilePath $Exe -PassThru `
    -RedirectStandardOutput (Join-Path $work 'app.out') `
    -RedirectStandardError (Join-Path $work 'app.err')

  $deadline = (Get-Date).AddSeconds(40)
  while ($true) {
    try {
      $null = Invoke-RestMethod "http://127.0.0.1:$Port/json/list" -TimeoutSec 2
      break
    } catch {
      if ((Get-Date) -gt $deadline) {
        throw "WebView2 CDP did not start on port ${Port}: $($_.Exception.Message)"
      }
      Start-Sleep -Milliseconds 250
    }
  }

  # Commands registered in `setup` are not ready until the main window is
  # actually shown at the end of setup; clicking a hidden page races the
  # backend and fails with "state not managed". Wait for a real visible window.
  $deadline = (Get-Date).AddSeconds(30)
  while ($true) {
    $proc = Get-Process -Id $app.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne 0 -and [SnapCapE2EWin]::IsWindowVisible($proc.MainWindowHandle)) { break }
    if ((Get-Date) -gt $deadline) { throw 'SnapCap main window did not become visible' }
    Start-Sleep -Milliseconds 200
  }
  Write-Output "MAIN window visible hwnd=$($proc.MainWindowHandle)"

  & $node.Source $flowScript $Port $report $outputDir $ArtifactsDir
  if ($LASTEXITCODE -ne 0) { throw "CDP window flow failed with exit code $LASTEXITCODE" }

  $savedPng = @(Get-ChildItem $outputDir -Filter '*.png' -File -ErrorAction SilentlyContinue)
  if ($savedPng.Count -eq 0) { throw "no PNG written to isolated output directory $outputDir" }
  $savedPng = $savedPng[0]

  # A PNG with correct dimensions but alpha=0 pixels still renders as a blank
  # editor canvas. Assert the captured window is actually visible.
  Add-Type -AssemblyName System.Drawing
  $bitmap = [System.Drawing.Bitmap]::FromFile($savedPng.FullName)
  try {
    $opaque = 0
    $sampled = 0
    $colors = New-Object 'System.Collections.Generic.HashSet[string]'
    $step = 3
    for ($y = 0; $y -lt $bitmap.Height; $y += $step) {
      for ($x = 0; $x -lt $bitmap.Width; $x += $step) {
        $c = $bitmap.GetPixel($x, $y)
        $sampled++
        if ($c.A -ge 250) { $opaque++ }
        if ($colors.Count -lt 64) { $null = $colors.Add("$($c.R),$($c.G),$($c.B)") }
      }
    }
    $opaqueRatio = $opaque / [Math]::Max(1, $sampled)
    Write-Output "PNG alpha=$([Math]::Round($opaqueRatio, 3)) colors=$($colors.Count)"
    if ($opaqueRatio -lt 0.95) {
      throw "captured PNG is mostly transparent (alpha ratio $opaqueRatio); editor would show a blank image"
    }
    if ($colors.Count -lt 3) {
      throw "captured PNG has only $($colors.Count) color(s); window content is missing"
    }
  } finally {
    $bitmap.Dispose()
  }

  $ok = $true
  Write-Output "OUTPUT $($savedPng.FullName) $($savedPng.Length) bytes"
  Write-Output 'RESULT: PASS'
} catch {
  Write-Output "RESULT: FAIL - $($_.Exception.Message)"
  Write-Output "WORKDIR $work"
  exit 1
} finally {
  if ($app -and (Get-Process -Id $app.Id -ErrorAction SilentlyContinue)) {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  }
  if ($target -and (Get-Process -Id $target.Id -ErrorAction SilentlyContinue)) {
    Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue
  }
  if ($null -eq $oldCdp) { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  else { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldCdp }
  if ($null -eq $oldConfig) { Remove-Item Env:SNAPCAP_CONFIG_DIR -ErrorAction SilentlyContinue }
  else { $env:SNAPCAP_CONFIG_DIR = $oldConfig }
  if ($ok) { Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue }
}
