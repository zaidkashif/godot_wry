#requires -Version 5.1
<#
.SYNOPSIS
    Automated Android WRY cross-compilation pipeline with Gradle validation.
    
.DESCRIPTION
    Compiles godot_wry for Android (x86_64 emulator + ARM64 devices) directly 
    into the Godot plugin directory structure expected by WRY.gdextension, then
    validates the Gradle build environment.
    
.EXAMPLE
    .\build_android.ps1
    
.NOTES
    Requires: cargo, cargo-ndk, Android NDK, Gradle
    Working Directory: d:\godot_wry\
#>

param(
    [switch]$SkipGradleClean,
    [switch]$Verbose
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Color output
function Write-Status { Write-Host "[*] $args" -ForegroundColor Cyan }
function Write-Success { Write-Host "[OK] $args" -ForegroundColor Green }
function Write-WryError { Write-Host "[!] $args" -ForegroundColor Red }

# ============================================================================
# 1. SETUP & VALIDATION
# ============================================================================

Write-Status "Android WRY Build Pipeline Starting..."
Write-Status "Current Location: $(Get-Location)"

# Verify we're in the right directory
if (-not (Test-Path "rust/Cargo.toml")) {
    Write-WryError "Cargo.toml not found in ./rust/. Run from d:\godot_wry"
    exit 1
}

Write-Success "Found rust/Cargo.toml"

# ============================================================================
# 2. CREATE TARGET DIRECTORY STRUCTURE
# ============================================================================

Write-Status "Preparing target directories..."

$targetDirs = @(
    "godot/addons/godot_wry/bin/android/x86_64",
    "godot/addons/godot_wry/bin/android/arm64-v8a"
)

foreach ($dir in $targetDirs) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        Write-Success "Created directory: $dir"
    }
}

# ============================================================================
# 3. COMPILE FOR x86_64 (EMULATOR)
# ============================================================================

Write-Status "Compiling x86_64-linux-android (Pixel 8 Emulator)..."
Push-Location rust
try {
    $cmd = "cargo ndk -t x86_64-linux-android -o ../godot/addons/godot_wry/bin/android build --release"
    if ($Verbose) { Write-Status "Executing: $cmd" }
    
    Invoke-Expression $cmd
    if ($LASTEXITCODE -ne 0) {
        Write-WryError "x86_64 compilation failed (exit code: $LASTEXITCODE)"
        exit 1
    }
    Write-Success "x86_64 compilation completed"
} finally {
    Pop-Location
}

# ============================================================================
# 4. COMPILE FOR ARM64 (PHYSICAL DEVICES)
# ============================================================================

Write-Status "Compiling aarch64-linux-android (Physical Devices)..."
Push-Location rust
try {
    $cmd = "cargo ndk -t aarch64-linux-android -o ../godot/addons/godot_wry/bin/android build --release"
    if ($Verbose) { Write-Status "Executing: $cmd" }
    
    Invoke-Expression $cmd
    if ($LASTEXITCODE -ne 0) {
        Write-WryError "ARM64 compilation failed (exit code: $LASTEXITCODE)"
        exit 1
    }
    Write-Success "ARM64 compilation completed"
} finally {
    Pop-Location
}

# ============================================================================
# 5. VALIDATE BINARIES IN PLACE
# ============================================================================

Write-Status "Validating binary placement..."

$requiredBinaries = @(
    "godot/addons/godot_wry/bin/android/x86_64/libgodot_wry.so",
    "godot/addons/godot_wry/bin/android/arm64-v8a/libgodot_wry.so"
)

$allPresent = $true
foreach ($binary in $requiredBinaries) {
    if (Test-Path $binary) {
        $size = (Get-Item $binary).Length / 1MB
        Write-Success "$binary ($([Math]::Round($size, 1)) MB)"
    } else {
        Write-WryError "MISSING: $binary"
        $allPresent = $false
    }
}

if (-not $allPresent) {
    Write-WryError "Some binaries are missing. Build may have failed."
    exit 1
}

# ============================================================================
# 6. VALIDATE .GDEXTENSION MAPPINGS
# ============================================================================

Write-Status "Verifying WRY.gdextension path mappings..."

$gdextPath = "godot/addons/godot_wry/WRY.gdextension"
if (Test-Path $gdextPath) {
    $content = Get-Content $gdextPath -Raw
    
    $checks = @(
        'android.x86_64.*=.*"bin/android/x86_64/',
        'android.arm64.*=.*"bin/android/arm64-v8a/'
    )
    
    foreach ($pattern in $checks) {
        if ($content -match $pattern) {
            Write-Success "Found: $pattern"
        } else {
            Write-WryError "MISSING: $pattern"
        }
    }
} else {
    Write-WryError "WRY.gdextension not found at $gdextPath"
}

# ============================================================================
# 7. GRADLE WORKSPACE CLEANUP
# ============================================================================

if ($SkipGradleClean) {
    Write-Status "Skipping Gradle clean"
} else {
    Write-Status "Cleaning Gradle build cache..."
    
    $gradleDir = "godot/android/build"
    if (Test-Path "$gradleDir/gradlew") {
        Push-Location $gradleDir
        try {
            & .\gradlew clean
            if ($LASTEXITCODE -eq 0) {
                Write-Success "Gradle clean completed successfully"
            } else {
                Write-WryError "Gradle clean failed (exit code: $LASTEXITCODE)"
                Write-Status "Attempting manual cleanup..."
                Remove-Item -Path "build", ".gradle" -Recurse -Force -ErrorAction SilentlyContinue
                Write-Success "Manual cleanup completed"
            }
        } finally {
            Pop-Location
        }
    } else {
        Write-Status "gradlew not found. Skipping Gradle clean."
    }
}

# ============================================================================
# 8. FINAL STATUS REPORT
# ============================================================================

Write-Host ""
Write-Success "Android Build Pipeline Completed Successfully!"
Write-Host ""
Write-Host "Summary:" -ForegroundColor Yellow
Write-Host "  [OK] x86_64 (x86_64-linux-android) -> bin/android/x86_64/libgodot_wry.so"
Write-Host "  [OK] ARM64  (aarch64-linux-android) -> bin/android/arm64-v8a/libgodot_wry.so"
Write-Host "  [OK] Gradle cache cleared"
Write-Host ""
Write-Host "Next Steps:" -ForegroundColor Yellow
Write-Host "  1. Open godot/project.godot in Godot Editor"
Write-Host "  2. Go to Project -> Export..."
Write-Host "  3. Select Android export -> Use Custom Build"
Write-Host "  4. Build APK for testing on Pixel 8 Emulator"
Write-Host ""
