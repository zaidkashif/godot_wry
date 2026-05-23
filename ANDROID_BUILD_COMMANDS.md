# Android WRY Build Commands Reference

## Automated Pipeline (Recommended)
Run from project root (`d:\godot_wry`):

```powershell
powershell -ExecutionPolicy Bypass -File .\build_android.ps1
```

**Optional flags:**
- `-Verbose` — Display all build commands
- `-SkipGradleClean` — Skip Gradle cache cleanup

---

## Manual Build Commands

### Prerequisites
```powershell
# Verify prerequisites are installed
cargo --version
cargo ndk --version
```

### 1. Compile for x86_64 (Pixel 8 Emulator)
From `d:\godot_wry\rust\`:
```powershell
cargo ndk -t x86_64-linux-android -o ../godot/addons/godot_wry/bin/android build --release
```

**Output location:**
```
godot/addons/godot_wry/bin/android/x86_64/libgodot_wry.so
```

### 2. Compile for ARM64 (Physical Devices)
From `d:\godot_wry\rust\`:
```powershell
cargo ndk -t aarch64-linux-android -o ../godot/addons/godot_wry/bin/android build --release
```

**Output location:**
```
godot/addons/godot_wry/bin/android/arm64-v8a/libgodot_wry.so
```

### 3. Clean Gradle Build Cache
From `d:\godot_wry\godot\android\build\`:
```powershell
.\gradlew clean
```

---

## Verification Checklist

✓ **Binary Locations:**
- [ ] `godot/addons/godot_wry/bin/android/x86_64/libgodot_wry.so` (6.2 MB)
- [ ] `godot/addons/godot_wry/bin/android/arm64-v8a/libgodot_wry.so` (6.7 MB)

✓ **WRY.gdextension Mappings:**
```ini
[libraries]
android.x86_64         = "bin/android/x86_64/libgodot_wry.so"
android.arm64          = "bin/android/arm64-v8a/libgodot_wry.so"
```

✓ **Gradle Cache Cleared:**
```
godot/android/build/build/ (removed)
godot/android/build/.gradle/ (removed)
```

---

## Godot Export Flow

After successful compilation:

1. **Open Godot Editor**
   ```
   godot/project.godot
   ```

2. **Configure Android Export**
   - Project → Export...
   - Select or create "Android" export preset
   - Check "Use Custom Build"
   - Configure Target API/Min API as needed (API 34+ recommended)

3. **Export APK**
   - Export → Android (*.apk)
   - File will be generated with embedded WRY native library

4. **Deploy to Emulator**
   ```powershell
   adb install -r export.apk
   ```

---

## Troubleshooting

### Binaries not found after build
- Ensure cargo-ndk is installed: `cargo install cargo-ndk`
- Check Android NDK path is set in environment
- Run `powershell -ExecutionPolicy Bypass -File .\build_android.ps1 -Verbose`

### WebView not rendering
- Verify binary files exist in exact paths shown above
- Confirm Gradle clean was executed (clears cached layouts)
- Check logcat: `adb logcat | grep -i wry`

### Gradle clean fails
- The script will auto-fallback to manual cleanup (`Remove-Item build, .gradle`)
- If manual cleanup fails, delete `godot/android/build/build/` and `godot/android/build/.gradle/` manually

---

## Build Timeline

| Stage | Time |
|-------|------|
| x86_64 compilation | ~4-5s (incremental) |
| ARM64 compilation | ~20-25s (first build) |
| Binary copy | <1s |
| Gradle clean | ~20s |
| **Total** | **~1 minute** |

---

## Directory Structure After Build

```
godot/addons/godot_wry/
├── bin/
│   └── android/
│       ├── x86_64/
│       │   └── libgodot_wry.so          (6.2 MB - Emulator)
│       └── arm64-v8a/
│           └── libgodot_wry.so          (6.7 MB - Devices)
├── icons/
├── WRY.gdextension                       (Maps paths above)
├── index.html
└── examples/

godot/android/build/
├── libs/
│   ├── debug/
│   │   ├── x86_64/
│   │   └── arm64-v8a/
│   └── release/
│       ├── x86_64/
│       └── arm64-v8a/
└── ... (Gradle structure)
```

---

## Next: Integration with Godot Export

Once binaries are in place (`godot/addons/godot_wry/bin/android/`), Godot's `.gdextension` loader will:

1. **Detect architecture** from build target
2. **Load correct .so** from configured path
3. **Initialize JNI bridge** via `WryActivity.nativeInit()`
4. **Attach WebView** to Godot's surface via `onWebViewCreate()`

The APK built by Gradle will automatically embed these native libraries.

---

**Last Updated:** May 23, 2026  
**Status:** ✓ Production Ready
