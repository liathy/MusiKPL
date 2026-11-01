# Builds the Linux release binary in a Docker container (Docker Desktop must
# be running) and puts it in dist\MusiKPL-linux-x86_64.
#
# The container uses Debian 11, whose older system libraries make the binary
# run on Ubuntu 20.04 and newer. Downloaded crates and build files are kept
# in Docker volumes, so later builds are quick. No system libraries are
# needed to build: the window and graphics libraries are loaded at runtime.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

docker info --format '{{.ServerVersion}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Docker is not running. Start Docker Desktop first.' }

$build = @'
set -e
cargo build --release -p MusiKPL
mkdir -p /src/dist
cp /build/target/release/MusiKPL /src/dist/MusiKPL-linux-x86_64
'@

docker run --rm `
    -v "${root}:/src" `
    -v musikpl-cargo-registry:/usr/local/cargo/registry `
    -v musikpl-linux-target:/build/target `
    -e CARGO_TARGET_DIR=/build/target `
    -w /src `
    rust:1-bullseye `
    bash -c ($build -replace "`r", '')
if ($LASTEXITCODE -ne 0) { throw 'The Linux build failed' }
Get-Item (Join-Path $root 'dist\MusiKPL-linux-x86_64') | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
