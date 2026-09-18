param(
  [string]$Target = "x86_64-pc-windows-msvc",
  [switch]$Amt,
  [string]$FfmpegDirectory
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$binaryRoot = Join-Path $repoRoot "apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binaryRoot | Out-Null

function Build-Worker([string]$WorkerDirectory, [string]$ModuleName, [string]$Name) {
  $workerPath = Join-Path $repoRoot $WorkerDirectory
  $workPath = Join-Path $workerPath ".package-build"
  $packageEnvironment = Join-Path $workerPath ".package-venv"
  $previousEnvironment = $env:UV_PROJECT_ENVIRONMENT
  $env:UV_PROJECT_ENVIRONMENT = $packageEnvironment
  try {
    & uv sync --directory $workerPath --group package --link-mode copy
    if ($LASTEXITCODE -ne 0) { throw "Could not prepare the package environment for $Name" }
    & uv run --directory $workerPath --no-sync pyinstaller --noconfirm --clean --onefile `
      --name $Name --collect-all $ModuleName --distpath $workPath\dist --workpath $workPath\work `
      --specpath $workPath\spec --paths "$workerPath\src" "$workerPath\src\$ModuleName\__main__.py"
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed for $Name" }
  }
  finally {
    $env:UV_PROJECT_ENVIRONMENT = $previousEnvironment
  }
  $source = Join-Path $workPath "dist\$Name.exe"
  if (-not (Test-Path -LiteralPath $source)) { throw "PyInstaller did not create $source" }
  Copy-Item -LiteralPath $source -Destination (Join-Path $binaryRoot "$Name-$Target.exe") -Force
}

Build-Worker "python\analysis-worker" "music_analyzer_worker" "analysis-worker"
if ($Amt) { Build-Worker "python\amt-worker" "music_analyzer_amt" "amt-worker" }

if ($FfmpegDirectory) {
  foreach ($name in "ffmpeg.exe", "ffprobe.exe") {
    $source = Join-Path $FfmpegDirectory $name
    if (-not (Test-Path -LiteralPath $source)) {
      throw "Missing $name in -FfmpegDirectory $FfmpegDirectory"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $binaryRoot $name) -Force
  }
}
