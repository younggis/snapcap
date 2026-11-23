# SnapCap text-annotation end-to-end acceptance (real UI over WebView2 CDP).
#
# Verifies that text annotations commit on a real Enter key and that the chosen
# color is applied — regressions caused by a globally-registered Enter hotkey
# swallowing the key and by color not reaching the annotation op.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\e2e_text_annotation.ps1
#   powershell -ExecutionPolicy Bypass -File tests\acceptance\e2e_text_annotation.ps1 -Exe <path>
param(
  [string]$Exe = '',
  [string]$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
  [int]$Port = 0
)

$ErrorActionPreference = 'Stop'
if (-not $Exe) { $Exe = Join-Path $Repo 'releases\snapcap.exe' }
if (-not (Test-Path $Exe)) { throw "SnapCap executable not found: $Exe" }
$Exe = (Resolve-Path $Exe).Path
$flowScript = Join-Path $PSScriptRoot 'cdp_text_flow.mjs'
if (-not (Test-Path $flowScript)) { throw "missing $flowScript" }
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) { throw 'node not found in PATH (required for the CDP driver)' }

$work = Join-Path $env:TEMP ('snapcap-text-e2e-' + [guid]::NewGuid().ToString('N'))
$configDir = Join-Path $work 'config'
$outputDir = Join-Path $work 'out'
New-Item -ItemType Directory -Force -Path $configDir, $outputDir | Out-Null
$settingsJson = @{ paths = @{ output_dir = $outputDir } } | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText(
  (Join-Path $configDir 'settings.json'), $settingsJson,
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
    try { $null = Invoke-RestMethod "http://127.0.0.1:$Port/json/list" -TimeoutSec 2; break }
    catch {
      if ((Get-Date) -gt $deadline) { throw "WebView2 CDP did not start on port ${Port}" }
      Start-Sleep -Milliseconds 250
    }
  }

  & $node.Source $flowScript $Port
  if ($LASTEXITCODE -ne 0) { throw "CDP text flow failed with exit code $LASTEXITCODE" }

  $ok = $true
  Write-Output 'RESULT: PASS'
} catch {
  Write-Output "RESULT: FAIL - $($_.Exception.Message)"
  Write-Output "WORKDIR $work"
  exit 1
} finally {
  if ($app -and (Get-Process -Id $app.Id -ErrorAction SilentlyContinue)) {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  }
  if ($null -eq $oldCdp) { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  else { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldCdp }
  if ($null -eq $oldConfig) { Remove-Item Env:SNAPCAP_CONFIG_DIR -ErrorAction SilentlyContinue }
  else { $env:SNAPCAP_CONFIG_DIR = $oldConfig }
  if ($ok) { Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue }
}
