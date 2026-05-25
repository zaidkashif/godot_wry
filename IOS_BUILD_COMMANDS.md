# iOS WRY Build Commands Reference

The iOS counterpart of `ANDROID_BUILD_COMMANDS.md`. It produces the native
GDExtension binary for iPhone/iPad **and** the Apple Silicon iOS Simulator, then
walks through exporting and running the demo from Godot → Xcode → Simulator.

> **Status:** iOS support was implemented mirroring the proven Android pipeline.
> The Rust/packaging layer is complete; the **on-device / on-simulator run is the
> verification step** (see *Verification & open points* at the bottom). It has not
> yet been run end-to-end on this machine because it requires the Rust iOS
> toolchain + Godot iOS export templates.

---

## Automated Pipeline (Recommended)

Run from the project root:

```bash
scripts/build_ios.sh
```

**Optional flags:**
- `--debug` — build the debug profile (default: release)
- `--device-only` — only `aarch64-apple-ios` (skip the simulator slice)
- `--sim-only` — only `aarch64-apple-ios-sim`
- `--skip-ui` — don't rebuild the Svelte demo UI (use the committed `build/`)

The script:
1. Installs the rustup targets if missing.
2. (Optionally) rebuilds the Svelte UI so the `res://` assets are fresh.
3. Cross-compiles both iOS targets.
4. Wraps each slice in a flat iOS `.framework` and combines them into
   `bin/ios/libgodot_wry.xcframework` (also emits a device-only
   `libgodot_wry.ios.framework`).

---

## Manual Build Commands

### Prerequisites
```bash
cargo --version
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
xcodebuild -version
```

### 1. Compile for physical devices (ARM64)
From `rust/`:
```bash
cargo build --target aarch64-apple-ios --release --locked
# -> target/aarch64-apple-ios/release/libgodot_wry.dylib
```

### 2. Compile for the Simulator (Apple Silicon)
From `rust/`:
```bash
cargo build --target aarch64-apple-ios-sim --release --locked
# -> target/aarch64-apple-ios-sim/release/libgodot_wry.dylib
```

### 3. Package as an xcframework
For each slice, build a flat `Name.framework/` containing the binary (renamed to
`libgodot_wry`, with an `@rpath` install name) and an `Info.plist`
(`CFBundlePackageType=FMWK`, `CFBundleSupportedPlatforms`, `DTPlatformName`).
Then:
```bash
xcodebuild -create-xcframework \
  -framework <device>/libgodot_wry.framework \
  -framework <sim>/libgodot_wry.framework \
  -output godot/addons/godot_wry/bin/ios/libgodot_wry.xcframework
```
`scripts/build_ios.sh` does all of this for you, including the `install_name_tool`
and `Info.plist` steps.

---

## Verification Checklist

✓ **Artifact locations:**
- [ ] `godot/addons/godot_wry/bin/ios/libgodot_wry.xcframework/` (contains
      `ios-arm64/` and `ios-arm64-simulator/`)
- [ ] `godot/addons/godot_wry/bin/ios/libgodot_wry.ios.framework/` (device-only)

✓ **WRY.gdextension mappings:**
```ini
[libraries]
ios.debug   = "bin/ios/libgodot_wry.xcframework"
ios.release = "bin/ios/libgodot_wry.xcframework"
```

✓ **Inspect the xcframework slices:**
```bash
lipo -info godot/addons/godot_wry/bin/ios/libgodot_wry.xcframework/ios-arm64/libgodot_wry
plutil -p   godot/addons/godot_wry/bin/ios/libgodot_wry.xcframework/Info.plist
```

---

## Godot → Xcode → Simulator Flow

1. **Open the project**
   ```
   godot/project.godot
   ```

2. **Install the iOS export templates** (Editor → Manage Export Templates) if not
   already present for your Godot version.

3. **Configure the iOS export preset**
   - Project → Export… → Add… → iOS
   - Set **Bundle Identifier** (e.g. `com.example.godotwry`). Per the
     architecture note in the brief, keep this identifier consistent with any
     other app identity to avoid OS window-interaction filtering.
   - For Simulator runs, ensure the preset targets the simulator destination.

4. **Export the Xcode project**
   - Export to a folder; Godot generates an `.xcodeproj`/workspace and embeds the
     `res://` PCK (which contains the Svelte UI) plus the xcframework.

5. **Open in Xcode and run**
   ```bash
   open <exported>/*.xcodeproj
   ```
   - Pick an **iOS Simulator** (Apple Silicon) or a connected device.
   - For a device build, set your signing **Team** in *Signing & Capabilities*.
   - Press **Run** (⌘R).

6. **Watch logs**
   ```bash
   xcrun simctl spawn booted log stream --predicate 'eventMessage CONTAINS "Godot WRY"'
   ```
   Look for `[Godot WRY] iOS WebView built successfully!` and
   `[Godot WRY] iOS WebView grafted to front and made transparent.`

---

## How the iOS integration works (for reviewers)

- **Window handle:** Unlike Android (which uses `ndk_context` + `wry::android_setup`),
  iOS hands WRY a real `UIView*`. We read Godot's `godotView` via
  `DisplayServer.window_get_native_handle(WINDOW_VIEW)` and the root
  `UIViewController` via `WINDOW_HANDLE`, then wrap them in a
  `raw_window_handle::UiKitWindowHandle`. `build_as_child()` is unsupported on
  iOS, so we call `WebViewBuilder::build()` with that handle (same shape as the
  Android workaround). — `rust/src/lib.rs`, iOS branch of `build_webview`.
- **View grafting / transparency (Phase 3):** WRY adds its `WKWebView` as a
  subview of `godotView`. `rust/src/ios.rs::graft_webview_to_front` walks the
  subviews via `objc2`, sets the web view + its scroll view to
  `isOpaque = NO` / `backgroundColor = clearColor`, and calls
  `bringSubviewToFront:` so the 3D scene renders underneath the web UI.
- **Assets (Phase 4):** No native bundle copying. The `res://` custom protocol is
  served by `protocols.rs::get_res_response` via Godot's `FileAccess`, which
  resolves the packed `.pck` inside the `.ipa`. iOS uses the same WebKit URL
  convention as macOS (`http://res.<path>` rewrite in `load_url`).
- **Input:** Touch is handled natively by `WKWebView`; like Android, the iOS path
  does **not** inject the desktop synthetic mouse/key IPC-forwarding script.

---

## Troubleshooting

### `dyld: Library not loaded` / framework not found at runtime
- The framework binary's install name must be `@rpath/...`. The script sets this
  with `install_name_tool -id`. Verify with:
  ```bash
  otool -D godot/addons/godot_wry/bin/ios/libgodot_wry.xcframework/ios-arm64/libgodot_wry
  ```

### WebView is invisible or fully white (hides the 3D scene)
- White = grafting/transparency didn't apply. Check the log for the grafting
  message. If the `WKWebView` subview wasn't found on the first frame, it may need
  a retry a frame later — see the note in `ios.rs`.

### Page is blank (assets 404)
- Confirm the Svelte UI exists under
  `…/character_creator_ui_demo/ui/build/index.html` in `res://` before export.
- Check the `[WRY Protocol]` logs for the resolved path.

### Simulator can't load the extension
- Ensure the xcframework actually contains the `ios-arm64-simulator` slice
  (`ls …/libgodot_wry.xcframework`). A device-only `.framework` will **not** run
  in the Simulator.

---

## ✅ VERIFIED: full demo running on a physical iPhone (Godot 4.6.3)

The character-creator demo (3D character + transparent WebView overlay) was run
end-to-end on a real iPhone 15. Confirmed in the device log:
```
Initialize godot-rust (API v4.2.stable.official, runtime v4.6.3.stable.official)
Metal 4.0 - Forward Mobile - Apple A16 GPU
[Godot WRY] iOS WebView built successfully!
[Godot WRY] iOS WebView grafted to front and made transparent.
```
The 3D scene renders with the Svelte web UI composited on top (transparent
WKWebView), exactly as on desktop/Android.

### Four gotchas that had to be fixed to get there (all done in this repo)

1. **GDExtension needs an arch tag.** The iOS exporter looks for an `arm64`
   library — `ios.debug`/`ios.release` alone fail with *"No 'arm64' library
   found"*. Use `ios.arm64` / `ios.debug.arm64` / `ios.release.arm64` (see
   `WRY.gdextension`).
2. **A main scene must be set.** `project.godot` had no `run/main_scene`, so the
   exported app loaded the engine + extension then exited cleanly (exit 0, blank).
   Set `run/main_scene` to the demo scene.
3. **Web assets must be in the export filter.** `.html/.js/.css` are *non-resource*
   files; with the default `all_resources` filter they are NOT packed into the
   `.pck`, so `res://…/index.html` 404s on device (works on desktop only because
   files are on disk). Add them via `include_filter` in the export preset
   (`*.html, *.js, *.css, *.json, *.wasm, …` — see `export_presets.cfg`).
4. **App-Store icon must be opaque** and the framework **bundle id must not
   contain underscores** (`build_ios.sh` uses `doceazedo.godotwry.libgodotwry`).

### iOS Simulator caveat (important)

Godot 4.6.3's official iOS **Simulator** engine lib is **x86_64-only**
(`lipo` the template's `libgodot.a` to confirm). On Apple-Silicon Macs the
simulator is arm64, so:
- an arm64-sim app **won't link** (no arm64 engine objects), and
- an x86_64-sim app **won't install** on the arm64 simulator.

→ **Use a real device** (clean: arm64 device template + arm64 WRY slice both
exist), or wait for arm64-sim templates. `build_ios.sh` still emits a universal
(arm64+x86_64) sim slice so the extension side is ready when Godot ships one.

### Running on a physical device (the verified path)

1. `scripts/build_ios.sh` (full — builds device + sim slices into the xcframework).
2. In `project.godot`: ensure `run/main_scene` is set and the renderer is
   **Mobile** (Metal needs iOS 14+, which the preset's `min_ios_version` matches).
3. Export the Xcode project (the headless export validation is buggy — use the
   **GUI** export, or it will report empty config errors).
4. Open the `.xcodeproj` in **Xcode**, select the device, set **Signing &
   Capabilities → your Team** (Automatic), press **Run**. CLI signing fails
   without an interactive Apple-ID login + 2FA, so use the GUI for the first run.
5. On the device, **Trust** the developer cert (Settings → General → VPN & Device
   Management) if prompted. WebKit helper processes take a few seconds to spin up.

**Last updated:** 2026-05-25 — verified on physical iPhone 15 / Godot 4.6.3.
