<#
.SYNOPSIS
    Verify a produced MP4 with ffprobe (DEVELOPMENT / CI ACCEPTANCE TOOL ONLY).

.DESCRIPTION
    Development/CI acceptance tool for SnapCap -- NOT shipped with the product.
    Per docs/DESIGN.md §16.1 (encode-pipeline validation) and §16.3 (AC-2 / AC-3),
    this script runs ffprobe against a recorded MP4 and asserts:
      * video codec is h264 (and, when -ExpectAudio or an audio stream exists, aac)
      * resolution matches -ExpectWidth / -ExpectHeight (when supplied)
      * frame rate matches -ExpectFps (when supplied)
      * duration is >= -MinDurationSec (when supplied)
      * audio stream presence when -ExpectAudio is set

    ffprobe is a dev-only dependency; it is never bundled with SnapCap.

.EXAMPLE
    .\verify_recording.ps1 -Path .\out.mp4 -ExpectWidth 1920 -ExpectHeight 1080 -ExpectFps 60 -MinDurationSec 60 -ExpectAudio
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [int]$ExpectWidth,
    [int]$ExpectHeight,
    [double]$ExpectFps,
    [double]$MinDurationSec,
    [switch]$ExpectAudio
)

$script:failures = [System.Collections.Generic.List[string]]::new()

function Assert-Condition {
    param(
        [bool]$Ok,
        [string]$Message
    )
    if ($Ok) {
        Write-Host "PASS: $Message"
    }
    else {
        Write-Host "FAIL: $Message"
        $script:failures.Add($Message)
    }
}

function Convert-FrameRate {
    param([string]$Value)
    if (-not $Value) { return 0.0 }
    if ($Value -match '^\s*(\d+(?:\.\d+)?)\s*/\s*(\d+(?:\.\d+)?)\s*$') {
        $den = [double]$Matches[2]
        if ($den -ne 0) { return ([double]$Matches[1]) / $den }
        return 0.0
    }
    if ($Value -match '^\s*\d+(?:\.\d+)?\s*$') { return [double]$Value }
    return 0.0
}

# --- Preconditions -----------------------------------------------------------
if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    Write-Host "FAIL: file not found: $Path"
    exit 1
}

if (-not (Get-Command ffprobe -ErrorAction SilentlyContinue)) {
    Write-Host "FAIL: ffprobe not found in PATH (dev/CI dependency, not shipped)"
    exit 1
}

# --- Run ffprobe -------------------------------------------------------------
$ffprobeArgs = @(
    '-v', 'error',
    '-show_entries', 'stream=index,codec_type,codec_name,width,height,r_frame_rate,duration',
    '-show_entries', 'format=duration',
    '-of', 'json',
    $Path
)

$raw = & ffprobe @ffprobeArgs 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) {
    Write-Host "FAIL: ffprobe exited with code $LASTEXITCODE"
    Write-Host $raw
    exit 1
}

try {
    $data = $raw | ConvertFrom-Json
}
catch {
    Write-Host "FAIL: could not parse ffprobe JSON output"
    Write-Host $raw
    exit 1
}

$streams = @($data.streams)
$video = @($streams | Where-Object { $_.codec_type -eq 'video' } | Select-Object -First 1)
$audio = @($streams | Where-Object { $_.codec_type -eq 'audio' })

# --- Assertions --------------------------------------------------------------
Assert-Condition ($video.Count -gt 0) 'video stream present'
if ($video.Count -eq 0) {
    Write-Host "RESULT: FAIL (1 failure)"
    exit 1
}
$v = $video[0]

Assert-Condition ($v.codec_name -eq 'h264') "video codec is h264 (got '$($v.codec_name)')"

if ($PSBoundParameters.ContainsKey('ExpectWidth')) {
    Assert-Condition ([int]$v.width -eq $ExpectWidth) "video width = $ExpectWidth (got $($v.width))"
}
if ($PSBoundParameters.ContainsKey('ExpectHeight')) {
    Assert-Condition ([int]$v.height -eq $ExpectHeight) "video height = $ExpectHeight (got $($v.height))"
}
if ($PSBoundParameters.ContainsKey('ExpectFps')) {
    $fps = Convert-FrameRate $v.r_frame_rate
    Assert-Condition ([math]::Abs($fps - $ExpectFps) -lt 0.5) "frame rate ~= $ExpectFps (got $([math]::Round($fps, 3)))"
}

$duration = 0.0
if ($data.format -and $data.format.duration) {
    $duration = [double]$data.format.duration
}
elseif ($v.duration) {
    $duration = [double]$v.duration
}
if ($PSBoundParameters.ContainsKey('MinDurationSec')) {
    Assert-Condition ($duration -ge $MinDurationSec) "duration >= $MinDurationSec s (got $([math]::Round($duration, 2)) s)"
}

if ($ExpectAudio) {
    Assert-Condition ($audio.Count -gt 0) 'audio stream present'
    if ($audio.Count -gt 0) {
        Assert-Condition ($audio[0].codec_name -eq 'aac') "audio codec is aac (got '$($audio[0].codec_name)')"
    }
}
elseif ($audio.Count -gt 0) {
    # An audio stream exists without being required: still validate its codec.
    Assert-Condition ($audio[0].codec_name -eq 'aac') "audio codec is aac (got '$($audio[0].codec_name)')"
}

# --- Result ------------------------------------------------------------------
if ($script:failures.Count -gt 0) {
    Write-Host "RESULT: FAIL ($($script:failures.Count) failure(s))"
    exit 1
}

Write-Host "RESULT: PASS"
exit 0
