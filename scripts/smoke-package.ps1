param([string]$Target = "x86_64-pc-windows-msvc")

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$binaries = Join-Path $repoRoot "apps\desktop\src-tauri\binaries"
foreach ($name in "analysis-worker", "amt-worker") {
  $sidecar = Join-Path $binaries "$name-$Target.exe"
  if (-not (Test-Path -LiteralPath $sidecar)) { throw "Missing packaged sidecar: $sidecar" }
  $hello = '{"protocol_version":1,"type":"request","request_id":"req-smoke","method":"shutdown","params":{}}' | & $sidecar
  if ($LASTEXITCODE -ne 0 -or $hello -notmatch '"type":"hello"') { throw "Packaged $name did not emit a valid handshake" }
}

$ffmpeg = Join-Path $binaries "ffmpeg.exe"
$ffprobe = Join-Path $binaries "ffprobe.exe"
if (-not (Test-Path -LiteralPath $ffmpeg) -or -not (Test-Path -LiteralPath $ffprobe)) { throw "Bundle ffmpeg.exe and ffprobe.exe in $binaries" }
& $ffprobe -version | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Packaged ffprobe failed" }
