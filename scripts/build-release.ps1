# Builds the two release files into dist\:
#   MusiKPL.exe        the portable app (mpv and yt-dlp bundled inside)
#   MusiKPL-Setup.exe  the installer, with MusiKPL.exe inside it
# Upload both to the GitHub release: installed copies update through the
# setup, portable copies through MusiKPL.exe.
#
# Needs mpv.exe and yt-dlp.exe in the project root (scripts\install-dependencies.ps1)
# and, for genres without a user key, lastfm-api-key.txt there too.
#
# -Quick builds only dist\MusiKPL.exe, much faster and a little bigger, for
# trying changes; releases are built without it.

param([switch]$Quick)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

foreach ($tool in 'mpv.exe', 'yt-dlp.exe') {
    if (-not (Test-Path (Join-Path $root $tool))) {
        throw "$tool is missing from $root. Run scripts\install-dependencies.ps1 first."
    }
}
if (-not (Test-Path (Join-Path $root 'lastfm-api-key.txt')) -and -not $env:MUSIKPL_LASTFM_KEY) {
    Write-Warning 'No lastfm-api-key.txt: this build will need the user''s own Last.fm key for genres.'
}

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

if ($Quick) {
    cargo build --profile quick -p MusiKPL
    if ($LASTEXITCODE -ne 0) { throw 'Building MusiKPL failed' }
    Copy-Item (Join-Path $root 'target\quick\MusiKPL.exe') $dist -Force
    Get-Item (Join-Path $dist 'MusiKPL.exe') | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
    return
}

cargo build --release -p MusiKPL
if ($LASTEXITCODE -ne 0) { throw 'Building MusiKPL failed' }
# The setup embeds target\release\MusiKPL.exe, so it is built second.
cargo build --release -p musikpl-setup
if ($LASTEXITCODE -ne 0) { throw 'Building the setup failed' }

Copy-Item (Join-Path $root 'target\release\MusiKPL.exe') $dist -Force
Copy-Item (Join-Path $root 'target\release\MusiKPL-Setup.exe') $dist -Force
Get-ChildItem $dist -Filter *.exe | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
