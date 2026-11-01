# Builds the Android app (android/) into dist\:
#   MusiKPL-android-arm64.apk
#
# 1. The Rust engine (mobile/, with core/) for arm64 phones, with cargo-ndk.
# 2. Its Kotlin bindings, with UniFFI.
# 3. The app, with Gradle.
#
# -Quick builds the engine faster (no whole-program optimization) and a
# debug APK, for trying changes. -Install puts it on the connected phone
# (adb, USB or wireless) and starts it.
#
# Needs: Android Studio (SDK, NDK, its Java), and once:
#   rustup target add aarch64-linux-android
#   cargo install cargo-ndk
param([switch]$Quick, [switch]$Install)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$env:ANDROID_HOME = $sdk
if (-not $env:ANDROID_NDK_HOME) {
    $ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:ANDROID_NDK_HOME = $ndk.FullName
}
if (-not $env:JAVA_HOME) {
    $env:JAVA_HOME = 'C:\Program Files\Android\Android Studio\jbr'
}
$app = Join-Path $root 'android'
$jniLibs = Join-Path $app 'app\src\main\jniLibs'
$profile = if ($Quick) { 'quick' } else { 'release' }

Push-Location $root
try {
    Write-Host "Building the engine for arm64 ($profile)..."
    cargo ndk -t arm64-v8a --platform 26 -o $jniLibs build -p musikpl-mobile --profile $profile
    if ($LASTEXITCODE -ne 0) { throw 'The engine did not build' }
    # Only MusiKPL's own library: dependencies are linked into it.
    Get-ChildItem (Join-Path $jniLibs 'arm64-v8a') -Filter '*.so' |
        Where-Object { $_.Name -ne 'libmusikpl_mobile.so' } |
        Remove-Item -Force

    Write-Host 'Generating the Kotlin bindings...'
    cargo build -q -p musikpl-mobile --lib
    $library = Join-Path $root 'target\debug\musikpl_mobile.dll'
    cargo run -q -p musikpl-mobile --bin uniffi-bindgen -- generate --library $library --language kotlin --no-format --out-dir (Join-Path $app 'app\src\main\java')
    if ($LASTEXITCODE -ne 0) { throw 'The bindings were not generated' }
} finally {
    Pop-Location
}

Push-Location $app
try {
    $task = if ($Quick) { 'assembleDebug' } else { 'assembleRelease' }
    Write-Host "Building the app ($task)..."
    & .\gradlew.bat $task --console=plain
    if ($LASTEXITCODE -ne 0) { throw 'The app did not build' }
} finally {
    Pop-Location
}

$variant = if ($Quick) { 'debug' } else { 'release' }
$apk = Get-ChildItem (Join-Path $app "app\build\outputs\apk\$variant") -Filter '*.apk' | Select-Object -First 1
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$target = Join-Path $dist 'MusiKPL-android-arm64.apk'
Copy-Item $apk.FullName $target -Force
Write-Host ("{0}  {1:N1} MB" -f $target, ((Get-Item $target).Length / 1MB))

if ($Install) {
    $adb = Join-Path $sdk 'platform-tools\adb.exe'
    & $adb install -r $target
    if ($LASTEXITCODE -ne 0) { throw 'The app was not installed' }
    & $adb shell am start -n io.github.liathy.musikpl/.MainActivity | Out-Null
}
