[CmdletBinding(SupportsShouldProcess)]
param(
  [ValidateSet("demucs-4", "demucs-6-experimental")]
  [string]$Model = "demucs-4",
  [string]$Workspace,
  [switch]$Verify,
  [switch]$Plan
)

$ErrorActionPreference = "Stop"

function Default-Workspace {
  if ($env:LOCAL_MUSIC_ANALYZER_WORKSPACE) { return [System.IO.Path]::GetFullPath($env:LOCAL_MUSIC_ANALYZER_WORKSPACE) }
  if ($env:LOCALAPPDATA) { return (Join-Path $env:LOCALAPPDATA "LocalMusicAnalyzer\workspace") }
  return (Join-Path (Get-Location) "workspace")
}

function Profile([string]$Id) {
  switch ($Id) {
    "demucs-4" { return [PSCustomObject]@{ Id = "demucs-4"; Filename = "htdemucs.yaml"; Stems = @("vocals", "drums", "bass", "other") } }
    "demucs-6-experimental" { return [PSCustomObject]@{ Id = "demucs-6-experimental"; Filename = "htdemucs_6s.yaml"; Stems = @("vocals", "drums", "bass", "other", "guitar", "piano") } }
  }
  throw "Unsupported Demucs profile: $Id"
}

function Read-Json([string]$Path) {
  try { return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }
  catch { throw "Invalid JSON metadata at $Path" }
}

function Write-Json([string]$Path, [object]$Value, [int]$Depth = 4) {
  $json = $Value | ConvertTo-Json -Depth $Depth
  [System.IO.File]::WriteAllText($Path, $json, [System.Text.UTF8Encoding]::new($false))
}

function Test-Bundle([string]$Directory, [object]$Profile) {
  $markerPath = Join-Path $Directory ".lma-model.json"
  $inventoryPath = Join-Path $Directory ".lma-bundle.json"
  $modelPath = Join-Path $Directory $Profile.Filename
  if (-not (Test-Path -LiteralPath $modelPath -PathType Leaf) -or (Get-Item -LiteralPath $modelPath).Length -le 0) { throw "Missing or empty model configuration: $modelPath" }
  if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) { throw "Missing Local Music Analyzer marker: $markerPath" }
  $marker = Read-Json $markerPath
  if ($marker.model_id -ne $Profile.Id -or $marker.model_filename -ne $Profile.Filename) { throw "Model marker does not match profile $($Profile.Id)." }
  if (-not (Test-Path -LiteralPath $inventoryPath -PathType Leaf)) { throw "Missing installer inventory: $inventoryPath. Reinstall this bundle with install-demucs.ps1." }
  $inventory = Read-Json $inventoryPath
  if ($inventory.model_id -ne $Profile.Id -or $inventory.model_filename -ne $Profile.Filename) { throw "Installer inventory does not match profile $($Profile.Id)." }
  if ($null -eq $inventory.files -or @($inventory.files).Count -lt 2) { throw "Installer inventory has no complete model file set." }
  foreach ($file in @($inventory.files)) {
    $candidate = Join-Path $Directory $file.relative_path
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "Bundle file is missing: $($file.relative_path)" }
    $item = Get-Item -LiteralPath $candidate
    if ($item.Length -ne [Int64]$file.size_bytes) { throw "Bundle file size changed: $($file.relative_path)" }
    $hash = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $file.sha256) { throw "Bundle file checksum changed: $($file.relative_path)" }
  }
}

$profile = Profile $Model
$workspaceRoot = if ($Workspace) { [System.IO.Path]::GetFullPath($Workspace) } else { Default-Workspace }
$modelsRoot = Join-Path $workspaceRoot "models"
$destination = Join-Path $modelsRoot $profile.Id

if ($Plan) {
  [PSCustomObject]@{ Model = $profile.Id; Configuration = $profile.Filename; Stems = $profile.Stems -join ", "; Destination = $destination; Network = "Explicit download only during this command" } | Format-List
  exit 0
}
if ($Verify) {
  Test-Bundle $destination $profile
  Write-Output "Verified $($profile.Id): $destination"
  exit 0
}
if (Test-Path -LiteralPath $destination) { throw "A bundle already exists at $destination. Run with -Verify to validate it; this installer never overwrites a local model." }
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { throw "uv is required. Install uv, then rerun this command." }
if (-not $PSCmdlet.ShouldProcess($destination, "download and install $($profile.Id)")) { exit 0 }

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$workerDirectory = Join-Path $repositoryRoot "python\analysis-worker"
if (-not (Test-Path -LiteralPath $workerDirectory -PathType Container)) { throw "Analysis worker directory was not found: $workerDirectory" }
New-Item -ItemType Directory -Force -Path $modelsRoot | Out-Null
$staging = Join-Path $modelsRoot (".{0}.install-{1}" -f $profile.Id, [guid]::NewGuid().ToString("N"))
$published = $false

try {
  New-Item -ItemType Directory -Path $staging | Out-Null
  $discoveryCode = @'
import json
import logging
import pathlib
import sys
from audio_separator.separator import Separator

model_directory, filename = sys.argv[1:]
separator = Separator(model_file_dir=model_directory, log_level=logging.CRITICAL)
for group in separator.list_supported_model_files().values():
    for model in group.values():
        if model.get('filename') == filename:
            print(json.dumps([pathlib.PurePosixPath(item).name for item in model.get('download_files', [])]))
            raise SystemExit(0)
raise SystemExit('Unsupported model: ' + filename)
'@
  $expectedJson = & uv run --directory $workerDirectory python -c $discoveryCode $staging $profile.Filename
  if ($LASTEXITCODE -ne 0) { throw "Could not resolve the expected files for $($profile.Id)." }
  # PowerShell 5.1 treats a JSON array piped through ConvertFrom-Json as one
  # pipeline object. Parse it as a single input string so the weight names stay
  # an actual array.
  $expectedFiles = ConvertFrom-Json -InputObject ($expectedJson -join [Environment]::NewLine)
  if ($expectedFiles.Count -lt 2) { throw "The model registry returned an incomplete file list for $($profile.Id)." }

  & uv run --directory $workerDirectory audio-separator --model_file_dir $staging --model_filename $profile.Filename --download_model_only
  if ($LASTEXITCODE -ne 0) { throw "audio-separator could not download $($profile.Id)." }
  foreach ($filename in $expectedFiles) {
    $file = Join-Path $staging $filename
    if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -le 0) { throw "Download did not produce the required model file: $filename" }
  }

  Write-Json (Join-Path $staging ".lma-model.json") @{ model_id = $profile.Id; model_filename = $profile.Filename }
  $inventoryFiles = Get-ChildItem -LiteralPath $staging -File | Where-Object { $_.Name -notin ".lma-model.json", ".lma-bundle.json" } | Sort-Object Name | ForEach-Object { [ordered]@{ relative_path = $_.Name; size_bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() } }
  Write-Json (Join-Path $staging ".lma-bundle.json") ([ordered]@{ schema_version = 1; model_id = $profile.Id; model_filename = $profile.Filename; installed_at = [DateTime]::UtcNow.ToString("o"); files = @($inventoryFiles) })
  Test-Bundle $staging $profile
  Move-Item -LiteralPath $staging -Destination $destination
  $published = $true
  Write-Output "Installed $($profile.Id): $destination"
}
finally {
  if (-not $published -and (Test-Path -LiteralPath $staging)) { Remove-Item -LiteralPath $staging -Recurse -Force }
}
