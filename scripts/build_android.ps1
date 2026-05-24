# Cross-compile Rust code for Android using cargo-ndk
# Outputs binaries to godot/addons/godot_wry/bin/android/{x86_64,arm64-v8a}

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$rustDir = Join-Path $projectRoot "rust"
$outputBaseDir = Join-Path $projectRoot "godot\addons\godot_wry\bin\android"

Write-Host "=== Android Cross-Compilation Pipeline ===" -ForegroundColor Cyan
Write-Host "Project root: $projectRoot"
Write-Host "Rust source: $rustDir"
Write-Host "Output directory: $outputBaseDir"
Write-Host ""

# Create target directories
Write-Host "Creating target directories..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path "$outputBaseDir\x86_64" | Out-Null
New-Item -ItemType Directory -Force -Path "$outputBaseDir\arm64-v8a" | Out-Null

# Copy Svelte UI build assets directly to Gradle assets directory
Write-Host "Copying Svelte UI build assets to Gradle project assets..." -ForegroundColor Yellow
$srcUiBuild = Join-Path $projectRoot "godot\addons\godot_wry\examples\character_creator_ui_demo\ui\build"
$destUiBuild = Join-Path $projectRoot "godot\android\build\assets\addons\godot_wry\examples\character_creator_ui_demo\ui\build"

if (Test-Path $srcUiBuild) {
    $destParent = Split-Path -Parent $destUiBuild
    New-Item -ItemType Directory -Force -Path $destParent | Out-Null
    if (Test-Path $destUiBuild) {
        Remove-Item -Path $destUiBuild -Recurse -Force
    }
    Copy-Item -Path $srcUiBuild -Destination $destUiBuild -Recurse -Force
    Write-Host "✅ Copied UI assets successfully!" -ForegroundColor Green
} else {
    Write-Warning "Svelte build assets folder not found at: $srcUiBuild"
}

# Compile for x86_64 (emulator)
Write-Host "Compiling for x86_64-linux-android (emulator)..." -ForegroundColor Yellow
Push-Location $rustDir
cargo ndk -t x86_64-linux-android -o "$outputBaseDir" build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ x86_64 compilation failed!" -ForegroundColor Red
    exit 1
}
Pop-Location

# Compile for aarch64 (physical devices)
Write-Host "Compiling for aarch64-linux-android (physical devices)..." -ForegroundColor Yellow
Push-Location $rustDir
cargo ndk -t aarch64-linux-android -o "$outputBaseDir" build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ aarch64 compilation failed!" -ForegroundColor Red
    exit 1
}
Pop-Location

# Validate binaries
Write-Host "Validating binary placement..." -ForegroundColor Yellow
$x86Binary = Join-Path $outputBaseDir "x86_64\libgodot_wry.so"
$armBinary = Join-Path $outputBaseDir "arm64-v8a\libgodot_wry.so"

if (-Not (Test-Path $x86Binary)) {
    Write-Host "❌ x86_64 binary not found: $x86Binary" -ForegroundColor Red
    exit 1
}

if (-Not (Test-Path $armBinary)) {
    Write-Host "❌ arm64-v8a binary not found: $armBinary" -ForegroundColor Red
    exit 1
}

$x86Size = (Get-Item $x86Binary).Length / 1MB
$armSize = (Get-Item $armBinary).Length / 1MB

Write-Host "✅ x86_64 binary: $(Get-Item $x86Binary | Select-Object -ExpandProperty FullName) ($([math]::Round($x86Size, 1)) MB)" -ForegroundColor Green
Write-Host "✅ arm64-v8a binary: $(Get-Item $armBinary | Select-Object -ExpandProperty FullName) ($([math]::Round($armSize, 1)) MB)" -ForegroundColor Green
Write-Host ""

Write-Host "Copying binaries to Gradle libs folder for standalone builds..." -ForegroundColor Yellow
$gradleLibsDir = Join-Path $projectRoot "godot\android\build\libs\debug"
New-Item -ItemType Directory -Force -Path "$gradleLibsDir\x86_64" | Out-Null
New-Item -ItemType Directory -Force -Path "$gradleLibsDir\arm64-v8a" | Out-Null
Copy-Item -Path $x86Binary -Destination "$gradleLibsDir\x86_64\" -Force
Copy-Item -Path $armBinary -Destination "$gradleLibsDir\arm64-v8a\" -Force
Write-Host "✅ Copied binaries successfully!" -ForegroundColor Green
Write-Host ""

# Clean Gradle cache
Write-Host "Cleaning Gradle cache..." -ForegroundColor Yellow
Push-Location (Join-Path $projectRoot "godot\android\build")
.\gradlew clean
Pop-Location

Write-Host "✅ Build complete! Ready for Godot export." -ForegroundColor Green
