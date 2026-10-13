[CmdletBinding()]
param(
    [switch]$Build,
    [switch]$SkipRust
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$DownloadRoot = Join-Path ([IO.Path]::GetTempPath()) "ytmusic-dependencies-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $DownloadRoot -Force | Out-Null

function Download-File([string]$Url, [string]$Destination) {
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
}

try {
    if (-not $SkipRust -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        $rustup = Join-Path $DownloadRoot 'rustup-init.exe'
        Download-File 'https://win.rustup.rs/x86_64' $rustup
        Write-Host 'Installing the stable Rust toolchain...'
        $process = Start-Process -FilePath $rustup -ArgumentList '-y', '--default-toolchain', 'stable', '--profile', 'minimal' -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "rustup-init.exe failed with exit code $($process.ExitCode)." }
    }

    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

    $ytDlpPath = Join-Path $ProjectRoot 'yt-dlp.exe'
    Download-File 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe' $ytDlpPath

    Write-Host 'Finding the latest 64-bit Windows mpv build...'
    $headers = @{ 'User-Agent' = 'YTMusic dependency installer' }
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest' -Headers $headers
    $asset = $release.assets |
        Where-Object { $_.name -match '(?i)^mpv-x86_64-.*\.(zip|7z)$' -and $_.name -notmatch '(?i)(source|debug|dev|v3)' } |
        Select-Object -First 1
    if (-not $asset) { throw 'Could not find a 64-bit mpv archive in the latest Windows release.' }

    $mpvZip = Join-Path $DownloadRoot $asset.name
    Download-File $asset.browser_download_url $mpvZip
    $mpvExtract = Join-Path $DownloadRoot 'mpv'
    New-Item -ItemType Directory -Path $mpvExtract -Force | Out-Null
    if ($mpvZip -match '(?i)\.zip$') {
        Expand-Archive -Path $mpvZip -DestinationPath $mpvExtract -Force
    }
    else {
        tar -xf $mpvZip -C $mpvExtract
        if ($LASTEXITCODE -ne 0) { throw 'Windows tar could not extract the mpv archive.' }
    }
    $mpvSource = Get-ChildItem -Path $mpvExtract -Filter 'mpv.exe' -File -Recurse | Select-Object -First 1
    if (-not $mpvSource) { throw 'The downloaded mpv archive did not contain mpv.exe.' }
    Copy-Item $mpvSource.FullName (Join-Path $ProjectRoot 'mpv.exe') -Force

    if (-not (Test-Path $ytDlpPath) -or -not (Test-Path (Join-Path $ProjectRoot 'mpv.exe'))) {
        throw 'One or more dependencies were not installed.'
    }

    Write-Host ''
    Write-Host 'Dependencies installed:' -ForegroundColor Green
    Write-Host "  $ProjectRoot\mpv.exe"
    Write-Host "  $ProjectRoot\yt-dlp.exe"

    if ($Build) {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
            throw 'Cargo is not available. Open a new PowerShell window and run this script again.'
        }
        Push-Location $ProjectRoot
        try {
            cargo build --release
            $releaseDir = Join-Path $ProjectRoot 'dist'
            New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
            Copy-Item (Join-Path $ProjectRoot 'target\release\MusiKPL.exe') $releaseDir -Force
            Copy-Item (Join-Path $ProjectRoot 'mpv.exe') $releaseDir -Force
            Copy-Item (Join-Path $ProjectRoot 'yt-dlp.exe') $releaseDir -Force
            Write-Host "Portable build created at $releaseDir" -ForegroundColor Green
        }
        finally { Pop-Location }
    }
}
finally {
    if (Test-Path $DownloadRoot) { Remove-Item $DownloadRoot -Recurse -Force }
}
