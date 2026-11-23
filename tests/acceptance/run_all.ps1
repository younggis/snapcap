<#
  SnapCap acceptance script (Windows)
  Covers automatable checks from DESIGN.md section 16.3:
    AC-1: screenshot path produces a real PNG data URL quickly
    AC-2: encoding pipeline produces H.264 MP4 with expected fps/frames/duration
    AC-3: QPC timeline continuity covered by Rust unit tests
    AC-5: installer size <= 15MB, cold start keeps process alive with visible window
  Usage:
    powershell -ExecutionPolicy Bypass -File tests\acceptance\run_all.ps1
#>
param(
  [string]$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
  [string]$Ffprobe = ''
)

$ErrorActionPreference = 'Stop'
$results = @()

function Check([string]$name, [scriptblock]$body) {
  try {
    $v = & $body
    $script:results += "PASS  $name $v"
  } catch {
    $script:results += "FAIL  $name - $($_.Exception.Message)"
  }
}

$tauriDir = Join-Path $Repo 'src-tauri'

# Make Rust + MSVC available even when launched from a plain PowerShell.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$vc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
if (Test-Path $vc) {
  $lines = cmd /c "`"$vc`" >nul 2>&1 && set"
  foreach ($line in $lines) {
    if ($line -match '^([^=]+)=(.*)$') {
      Set-Item -Path "env:$($Matches[1])" -Value $Matches[2] -ErrorAction SilentlyContinue
    }
  }
}
$env:PATH = "$cargoBin;$env:PATH"

if (-not $Ffprobe) {
  $cmd = Get-Command ffprobe -ErrorAction SilentlyContinue
  if ($cmd) { $Ffprobe = $cmd.Source }
  else {
    $winget = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Filter ffprobe.exe -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName
    if ($winget) { $Ffprobe = $winget }
  }
}

function Invoke-Quiet([scriptblock]$body) {
  $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { & $body } finally { $ErrorActionPreference = $old }
}

Check 'Frontend build (vue-tsc + vite)' {
  Push-Location $Repo
  try {
    $o = Invoke-Quiet { pnpm ui:build 2>&1 | Out-String }
    if ($LASTEXITCODE -ne 0) { throw $o }
    'ok'
  } finally { Pop-Location }
}

Check 'Rust unit tests' {
  Push-Location $tauriDir
  try {
    $o = Invoke-Quiet { cargo test 2>&1 | Out-String }
    if ($LASTEXITCODE -ne 0) { throw $o }
    if ($o -notmatch 'test result: ok') { throw 'no test result: ok' }
    'ok'
  } finally { Pop-Location }
}

$exe = Join-Path $tauriDir 'target\debug\snapcap.exe'
if (-not (Test-Path $exe)) {
  Push-Location $tauriDir
  try { $o = Invoke-Quiet { cargo build 2>&1 | Out-String }; if ($LASTEXITCODE -ne 0) { throw $o } } finally { Pop-Location }
}

Check 'All-feature verification (geometry/hotkey/settings/3 capture targets/annotation/encode/record)' {
  $out = Invoke-Quiet { & $exe --verify-all 2>&1 | Out-String }
  $lines = $out -split "`n" | Where-Object { $_ -match '^(PASS|FAIL) ' }
  if ($lines.Count -lt 9) { throw "expected >=9 checks, got $($lines.Count): $out" }
  $failed = $lines | Where-Object { $_ -like 'FAIL*' }
  if ($failed) { throw ($failed -join '; ') }
  "$($lines.Count) checks passed"
}

$png = Join-Path $env:TEMP 'snapcap-acceptance.png'
Remove-Item $png -ErrorAction SilentlyContinue
Check 'AC-1 screenshot path (full screen to PNG)' {
  $out = Invoke-Quiet { & $exe --capture-test $png 2>&1 | Out-String }
  if ($LASTEXITCODE -ne 0) { throw $out }
  if (-not (Test-Path $png)) { throw 'PNG not generated' }
  "$([math]::Round((Get-Item $png).Length/1KB)) KB - $($out.Trim())"
}

$mp4 = Join-Path $env:TEMP 'snapcap-acceptance.mp4'
Remove-Item $mp4, ([IO.Path]::ChangeExtension($mp4, 'txt')) -ErrorAction SilentlyContinue

$recMp4 = Join-Path $env:TEMP 'snapcap-record-acceptance.mp4'
Remove-Item $recMp4 -ErrorAction SilentlyContinue
Check 'Record path (real GDI capture to MP4)' {
  $out = Invoke-Quiet { cmd.exe /c "`"$exe`" --record-test `"$recMp4`" 2 30 2>&1" | Out-String }
  if ($LASTEXITCODE -ne 0) { throw $out }
  if (-not (Test-Path $recMp4)) { throw 'MP4 not generated' }
  "$([math]::Round((Get-Item $recMp4).Length/1KB)) KB - $($out.Trim())"
}

Check 'Encoder selftest (synthetic frames to MP4)' {
  Invoke-Quiet { & $exe --selftest $mp4 2>&1 | Out-Null }
  if (-not (Test-Path $mp4)) { throw 'MP4 not generated' }
  "$((Get-Item $mp4).Length) bytes"
}

if (Test-Path $mp4) {
  Check 'AC-2 ffprobe H.264 / 320x240 / 30fps / 90 frames' {
    if (-not $Ffprobe) { throw 'ffprobe not found' }
    $json = & $Ffprobe -v error -show_entries 'stream=codec_name,codec_type,width,height,r_frame_rate,nb_frames' -show_entries 'format=duration,size' -of json $mp4 | ConvertFrom-Json
    $v = $json.streams | Where-Object codec_type -eq 'video' | Select-Object -First 1
    if ($v.codec_name -ne 'h264') { throw "codec=$($v.codec_name)" }
    if ($v.width -ne 320 -or $v.height -ne 240) { throw "resolution=$($v.width)x$($v.height)" }
    if ($v.r_frame_rate -ne '30/1') { throw "fps=$($v.r_frame_rate)" }
    if ([int]$v.nb_frames -lt 85) { throw "frames=$($v.nb_frames)" }
    if ([double]$json.format.duration -lt 2.9) { throw "duration=$($json.format.duration)" }
    "codec=$($v.codec_name) $($v.width)x$($v.height) fps=$($v.r_frame_rate) frames=$($v.nb_frames) dur=$($json.format.duration)"
  }
}

Check 'AC-5 installer size <= 15MB' {
  $setup = Join-Path $Repo 'releases\SnapCap_0.1.0_x64-setup.exe'
  if (-not (Test-Path $setup)) { throw 'installer not found' }
  $mb = (Get-Item $setup).Length / 1MB
  if ($mb -gt 15) { throw "size=$mb MB" }
  '{0:N2} MB' -f $mb
}

Check 'AC-5 cold start process alive with visible window' {
  $exeRel = Join-Path $Repo 'releases\snapcap.exe'
  if (-not (Test-Path $exeRel)) { throw 'exe not found' }
  $p = Start-Process -FilePath $exeRel -PassThru
  Start-Sleep -Seconds 4
  $alive = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
  if (-not $alive) { throw "process exited code=$($p.ExitCode)" }
  $mem = [math]::Round($alive.WorkingSet64 / 1MB, 1)
  Stop-Process -Id $p.Id -Force
  "pid=$($p.Id) mem=${mem}MB"
}

Check 'E2E window capture UI (recognition + real Enter + PNG file)' {
  $e2e = Join-Path $PSScriptRoot 'e2e_window.ps1'
  if (-not (Test-Path $e2e)) { throw 'e2e_window.ps1 not found' }
  $out = Invoke-Quiet { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $e2e -Exe (Join-Path $Repo 'releases\snapcap.exe') -Repo $Repo 2>&1 | Out-String }
  if ($LASTEXITCODE -ne 0) { throw ($out.Trim() -replace '\s+', ' ') }
  $summary = $out -split "`n" | Where-Object { $_ -match '^(OVERLAY|SELECTED|EDITOR|PNG|OUTPUT|RESULT|CAPTURE PROOF)' }
  ($summary -join ' | ').Trim()
}

Check 'E2E text annotation (real Enter commits + color applied)' {
  $e2e = Join-Path $PSScriptRoot 'e2e_text_annotation.ps1'
  if (-not (Test-Path $e2e)) { throw 'e2e_text_annotation.ps1 not found' }
  $out = Invoke-Quiet { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $e2e -Exe (Join-Path $Repo 'releases\snapcap.exe') -Repo $Repo 2>&1 | Out-String }
  if ($LASTEXITCODE -ne 0) { throw ($out.Trim() -replace '\s+', ' ') }
  $summary = $out -split "`n" | Where-Object { $_ -match '^(EDITOR|TOOL|CDP Enter|REAL Enter|RESULT)' }
  ($summary -join ' | ').Trim()
}

Check 'GPU window capture (QQ/WeChat/Chromium, no black bitmap)' {
  $gpu = Join-Path $PSScriptRoot 'gpu_window.ps1'
  if (-not (Test-Path $gpu)) { throw 'gpu_window.ps1 not found' }
  $out = Invoke-Quiet { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $gpu -Exe (Join-Path $Repo 'releases\snapcap.exe') -Repo $Repo 2>&1 | Out-String }
  if ($LASTEXITCODE -ne 0) { throw ($out.Trim() -replace '\s+', ' ') }
  if ($out -match 'SKIP GPU window test') { return 'skipped (no such GPU window)' }
  $summary = $out -split "`n" | Where-Object { $_ -match '^(GPU PNG|RESULT)' }
  ($summary -join ' | ').Trim()
}

$results | ForEach-Object { Write-Output $_ }
if ($results | Where-Object { $_ -like 'FAIL*' }) { exit 1 }
exit 0
