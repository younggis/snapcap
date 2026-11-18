# SnapCap GPU-rendered window capture acceptance (real UI over WebView2 CDP).
#
# Unlike the WinForms target in e2e_window.ps1, WeChat / QQ / Chromium windows
# are drawn by DWM (GPU composition). GetWindowDC + BitBlt returns an all-black
# bitmap for them, so this test grabs a real running GPU window through the
# full UI flow and asserts the saved PNG is not blank/black.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\gpu_window.ps1
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\gpu_window.ps1 -ProcessName chrome
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\gpu_window.ps1 -Hwnd 198322
param(
  [string]$Exe = '',
  [string]$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
  [string]$ProcessName = 'QQ',
  [int]$Hwnd = 0,
  [int]$Port = 0
)

$ErrorActionPreference = 'Stop'

if (-not $Exe) { $Exe = Join-Path $Repo 'releases\snapcap.exe' }
if (-not (Test-Path $Exe)) { throw "SnapCap executable not found: $Exe" }
$Exe = (Resolve-Path $Exe).Path
$flowScript = Join-Path $PSScriptRoot 'cdp_window_flow.mjs'
if (-not (Test-Path $flowScript)) { throw "missing $flowScript" }
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) { throw 'node not found in PATH (required for the CDP driver)' }

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SnapCapGpuWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
}
'@

# Make this process DPI-aware so GetWindowRect returns physical pixels, just
# like the DPI-aware SnapCap process. Otherwise a 125% scaled window is reported
# as 860x739 here but captured as 1075x924 by SnapCap.
if (-not [SnapCapGpuWin]::SetProcessDpiAwarenessContext([IntPtr](-4))) {
  [void][SnapCapGpuWin]::SetProcessDPIAware()
}

# Locate a real, visible GPU-rendered window.
if ($Hwnd -le 0) {
  $proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } |
    Select-Object -First 1
  if (-not $proc) {
    Write-Output "SKIP GPU window test: no visible $ProcessName window"
    exit 0
  }
  $Hwnd = [int64]$proc.MainWindowHandle
  $title = $proc.MainWindowTitle
} else {
  $title = (Get-Process | Where-Object { $_.MainWindowHandle -eq $Hwnd } |
    Select-Object -First 1).MainWindowTitle
  if (-not $title) { $title = 'GPU window' }
}
$hwndPtr = [IntPtr]$Hwnd
# A minimized window reports -25600 pseudo coordinates; restore it first, then
# read the real on-screen rectangle (GetWindowRect before restore is useless).
if ([SnapCapGpuWin]::IsIconic($hwndPtr)) {
  [void][SnapCapGpuWin]::ShowWindow($hwndPtr, 9) # SW_RESTORE
  Start-Sleep -Milliseconds 700
}
$rect = New-Object SnapCapGpuWin+RECT
if (-not [SnapCapGpuWin]::GetWindowRect($hwndPtr, [ref]$rect)) {
  throw "GetWindowRect failed for hwnd=$Hwnd"
}
if ($rect.Left -le -30000 -or $rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top) {
  Write-Output "SKIP GPU window test: hwnd=$Hwnd is still minimized/hidden"
  exit 0
}
[void][SnapCapGpuWin]::SetForegroundWindow($hwndPtr)
# Temporarily bring the target above other windows so the overlay's hover
# picks it (and the screen DC captures it instead of an occluding window).
# SWP_NOMOVE | SWP_NOSIZE = 0x3; HWND_TOPMOST = -1, HWND_NOTOPMOST = -2.
$topmost = [IntPtr](-1)
[void][SnapCapGpuWin]::SetWindowPos($hwndPtr, $topmost, 0, 0, 0, 0, 0x3)

Write-Output "TARGET hwnd=$Hwnd rect=$($rect.Left),$($rect.Top) $($rect.Right - $rect.Left)x$($rect.Bottom - $rect.Top) title=$title"

# Isolated workspace: settings and screenshots never touch the user profile.
$work = Join-Path $env:TEMP ('snapcap-gpu-' + [guid]::NewGuid().ToString('N'))
$configDir = Join-Path $work 'config'
$outputDir = Join-Path $work 'out'
$artifactsDir = Join-Path $PSScriptRoot 'artifacts\gpu-window'
New-Item -ItemType Directory -Force -Path $configDir, $outputDir, $artifactsDir | Out-Null
$settingsJson = @{ paths = @{ output_dir = $outputDir } } | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText(
  (Join-Path $configDir 'settings.json'),
  $settingsJson,
  (New-Object System.Text.UTF8Encoding($false))
)
$targetInfo = [ordered]@{
  hwnd   = $Hwnd
  x      = $rect.Left
  y      = $rect.Top
  width  = $rect.Right - $rect.Left
  height = $rect.Bottom - $rect.Top
  title  = $title
}
$report = Join-Path $work 'target.json'
[System.IO.File]::WriteAllText(
  $report,
  ($targetInfo | ConvertTo-Json -Depth 4),
  (New-Object System.Text.UTF8Encoding($false))
)

if ($Port -le 0) {
  $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
  $listener.Stop()
}

$app = $null
$ok = $false
$oldCdp = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$oldConfig = $env:SNAPCAP_CONFIG_DIR
try {
  Get-Process snapcap -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $Exe } |
    Stop-Process -Force -ErrorAction SilentlyContinue

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

  # Bring the target in front again after SnapCap's own window appears.
  [void][SnapCapGpuWin]::SetWindowPos($hwndPtr, $topmost, 0, 0, 0, 0, 0x3)
  [void][SnapCapGpuWin]::SetForegroundWindow($hwndPtr)
  Start-Sleep -Milliseconds 400

  & $node.Source $flowScript $Port $report $outputDir $artifactsDir
  if ($LASTEXITCODE -ne 0) { throw "CDP window flow failed with exit code $LASTEXITCODE" }

  $saved = @(Get-ChildItem $outputDir -Filter '*.png' -File -ErrorAction SilentlyContinue)
  if ($saved.Count -eq 0) { throw "no PNG written to $outputDir" }
  $saved = $saved[0]

  # A GPU window captured via GetWindowDC is a solid black bitmap. Assert the
  # PNG actually contains the rendered content.
  $bitmap = [System.Drawing.Bitmap]::FromFile($saved.FullName)
  try {
    $black = 0; $opaque = 0; $sampled = 0
    $colors = New-Object 'System.Collections.Generic.HashSet[string]'
    for ($y = 0; $y -lt $bitmap.Height; $y += 3) {
      for ($x = 0; $x -lt $bitmap.Width; $x += 3) {
        $c = $bitmap.GetPixel($x, $y)
        $sampled++
        if ($c.A -ge 250) { $opaque++ }
        if ($c.R -le 8 -and $c.G -le 8 -and $c.B -le 8) { $black++ }
        if ($colors.Count -lt 128) { $null = $colors.Add("$($c.R),$($c.G),$($c.B)") }
      }
    }
    $blackRatio = $black / [Math]::Max(1, $sampled)
    $opaqueRatio = $opaque / [Math]::Max(1, $sampled)
    Write-Output "GPU PNG $($bitmap.Width)x$($bitmap.Height) alpha=$([Math]::Round($opaqueRatio,3)) black=$([Math]::Round($blackRatio,3)) colors=$($colors.Count)"
    if ($opaqueRatio -lt 0.95) { throw "GPU capture is transparent (alpha=$opaqueRatio)" }
    if ($blackRatio -gt 0.5) { throw "GPU capture is mostly black (black ratio $blackRatio); GetWindowDC path may be in use" }
    if ($colors.Count -lt 8) { throw "GPU capture has only $($colors.Count) colors; content is missing" }
  } finally {
    $bitmap.Dispose()
  }

  $ok = $true
  Write-Output "OUTPUT $($saved.FullName) $($saved.Length) bytes"
  Write-Output 'RESULT: PASS'
} catch {
  Write-Output "RESULT: FAIL - $($_.Exception.Message)"
  Write-Output "WORKDIR $work"
  exit 1
} finally {
  if ($topmost) {
    [void][SnapCapGpuWin]::SetWindowPos($hwndPtr, [IntPtr](-2), 0, 0, 0, 0, 0x3)
  }
  if ($app -and (Get-Process -Id $app.Id -ErrorAction SilentlyContinue)) {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  }
  if ($null -eq $oldCdp) { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  else { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldCdp }
  if ($null -eq $oldConfig) { Remove-Item Env:SNAPCAP_CONFIG_DIR -ErrorAction SilentlyContinue }
  else { $env:SNAPCAP_CONFIG_DIR = $oldConfig }
  if ($ok) { Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue }
}
