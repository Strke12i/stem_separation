param(
  [string]$Target = "x86_64-pc-windows-msvc",
  [Parameter(Mandatory = $true)]
  [string]$FfmpegDirectory
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

$buildArguments = @{ Target = $Target; FfmpegDirectory = $FfmpegDirectory; Amt = $true }
& (Join-Path $PSScriptRoot "build-sidecars.ps1") @buildArguments

& (Join-Path $PSScriptRoot "smoke-package.ps1") -Target $Target

Push-Location (Join-Path $repoRoot "apps\desktop")
try {
  & npm.cmd run tauri -- build --config src-tauri/tauri.release.conf.json
  if ($LASTEXITCODE -ne 0) { throw "Tauri bundle build failed" }
}
finally {
  Pop-Location
}
