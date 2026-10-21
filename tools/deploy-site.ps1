# Refreshes the website's download (docs/download/OneClickOptimizer-Setup.exe)
# with the latest release, checked against the release's SHA-256 checksums.
# Visitors download it from the site itself and never land on GitHub.
#
# You normally don't need this: the release workflow puts each new installer
# into docs/download on main, and Vercel redeploys the site from there.
#
#   Refresh by hand:   powershell -ExecutionPolicy Bypass -File tools\deploy-site.ps1
#                      then commit and push docs/download
#   Deploy without git (Vercel CLI, only docs/ is uploaded):
#                      npx vercel login   (once)
#                      powershell -ExecutionPolicy Bypass -File tools\deploy-site.ps1 -Deploy
param([switch]$Deploy)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$site = Join-Path (Split-Path $PSScriptRoot) 'docs'
if (-not (Test-Path (Join-Path $site 'index.html'))) { throw "No website found in $site" }

$repo = 'Mitovoid-AI/one-click-optimizer'
$setup = 'OneClickOptimizer-Setup.exe'
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'oneclick-site-deploy' }
Write-Host "Latest release: $($release.tag_name)"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("oco-site-" + [guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
  foreach ($name in $setup, 'SHA256SUMS.txt') {
    $asset = $release.assets | Where-Object { $_.name -eq $name }
    if (-not $asset) { throw "Release $($release.tag_name) has no $name" }
    Invoke-WebRequest $asset.browser_download_url -OutFile (Join-Path $tmp $name) -UseBasicParsing
  }
  # The installer must match the checksum published with the release.
  $want = (Get-Content (Join-Path $tmp 'SHA256SUMS.txt') | Where-Object { $_ -match "\s\*?$([regex]::Escape($setup))$" } | ForEach-Object { ($_ -split '\s+')[0].ToLower() } | Select-Object -First 1)
  $got = (Get-FileHash (Join-Path $tmp $setup) -Algorithm SHA256).Hash.ToLower()
  if (-not $want -or $want -ne $got) { throw "$setup failed its checksum (expected $want, got $got); nothing changed" }
  $out = Join-Path $site 'download'
  New-Item -ItemType Directory -Force $out | Out-Null
  Copy-Item (Join-Path $tmp $setup) (Join-Path $out $setup) -Force
} finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "docs/download/$setup is now $($release.tag_name) (checksum verified)."
if (-not $Deploy) { Write-Host 'Commit and push docs/download to publish it.'; return }

if (-not (Test-Path (Join-Path $site '.vercel\project.json'))) {
  Write-Host 'First deploy: answer the questions (project name: oneclick-optimizer, code directory: ./).'
  npx --yes vercel --cwd "$site"
}
npx --yes vercel deploy --cwd "$site" --prod
