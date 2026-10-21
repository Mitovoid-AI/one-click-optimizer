# Builds a release and puts the files to share in dist\:
#   dist\OneClickOptimizer-Setup.exe   <- give this one file to people
#   dist\oneclick-cli.exe              <- optional command-line report
# The setup exe is the app itself: its name makes it open the installer.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item 'target\release\OneClickOptimizer.exe' (Join-Path $dist 'OneClickOptimizer-Setup.exe') -Force
    Copy-Item 'target\release\oneclick-cli.exe' (Join-Path $dist 'oneclick-cli.exe') -Force
    Get-ChildItem $dist | ForEach-Object { "{0,-30} {1,6} KB" -f $_.Name, [math]::Round($_.Length / 1KB) }
} finally {
    Pop-Location
}
