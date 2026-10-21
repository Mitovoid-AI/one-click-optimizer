# Puts the website on Vercel. Only the docs/ folder is uploaded: the app's
# source code never leaves your PC.
#
# Before uploading, it copies the latest release's files into docs/download/
# (checked against the release's SHA-256 checksums), so visitors download from
# the site itself and never land on GitHub.
#
#   First time only:  npx vercel login        (opens your browser to sign in)
#   Every deploy:     powershell -ExecutionPolicy Bypass -File tools\deploy-site.ps1
#   Files only:       ... deploy-site.ps1 -FetchOnly   (refresh docs/download, no upload)
#
# The first run asks a few setup questions (see README, "Website"); later runs
# deploy straight to production. Run it again after every new release.
param([switch]$FetchOnly)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$site = Join-Path (Split-Path $PSScriptRoot) 'docs'
if (-not (Test-Path (Join-Path $site 'index.html'))) { throw "No website found in $site" }

$repo = 'Mitovoid-AI/one-click-optimizer'
$files = 'OneClickOptimizer-Setup.exe', 'OneClickOptimizer.exe', 'oneclick-cli.exe', 'SHA256SUMS.txt'
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'oneclick-site-deploy' }
Write-Host "Latest release: $($release.tag_name)"
$out = Join-Path $site 'download'
$tmp = Join-Path $out '.incoming'
New-Item -ItemType Directory -Force $tmp | Out-Null
foreach ($name in $files) {
  $asset = $release.assets | Where-Object { $_.name -eq $name }
  if (-not $asset) { throw "Release $($release.tag_name) has no $name" }
  Invoke-WebRequest $asset.browser_download_url -OutFile (Join-Path $tmp $name) -UseBasicParsing
}
# Every program must match the checksum published with the release.
$sums = @{}
foreach ($line in Get-Content (Join-Path $tmp 'SHA256SUMS.txt')) {
  $parts = $line -split '\s+', 2
  if ($parts.Count -eq 2) { $sums[$parts[1].Trim().TrimStart('*')] = $parts[0].ToLower() }
}
foreach ($name in $files | Where-Object { $_ -like '*.exe' }) {
  $got = (Get-FileHash (Join-Path $tmp $name) -Algorithm SHA256).Hash.ToLower()
  if ($sums[$name] -ne $got) { throw "$name failed its checksum (expected $($sums[$name]), got $got); nothing was deployed" }
}
foreach ($name in $files) { Move-Item -Force (Join-Path $tmp $name) (Join-Path $out $name) }
Remove-Item $tmp -Force
[IO.File]::WriteAllText((Join-Path $out 'version.txt'), $release.tag_name.TrimStart('v'))
Write-Host "docs/download now holds $($release.tag_name), checksums verified."
if ($FetchOnly) { return }

if (-not (Test-Path (Join-Path $site '.vercel\project.json'))) {
  Write-Host 'First deploy: answer the questions (project name: oneclick-optimizer, code directory: ./).'
  npx --yes vercel --cwd "$site"
}
npx --yes vercel deploy --cwd "$site" --prod
