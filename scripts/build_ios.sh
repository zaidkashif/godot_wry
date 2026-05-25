#!/usr/bin/env bash
#
# scripts/build_ios.sh — cross-compile godot_wry for iOS and package it for Godot.
#
# This is the iOS counterpart of build_android.ps1. It:
#   1. Cross-compiles the Rust GDExtension for:
#        - aarch64-apple-ios       (physical iPhone/iPad hardware)
#        - aarch64-apple-ios-sim   (iOS Simulator on Apple Silicon Macs)
#   2. Wraps each slice in a flat iOS `.framework` (binary + Info.plist with an
#      @rpath install name) and combines them into a single
#      `libgodot_wry.xcframework` via `xcodebuild -create-xcframework`.
#      (A device-only `.framework` is also emitted for tooling that wants one.)
#   3. Optionally rebuilds the Svelte demo UI so the `res://` assets Godot packs
#      into the .ipa are fresh. NOTE: on iOS the web assets are served from the
#      Godot .pck via `FileAccess` (see protocols.rs), NOT copied into a native
#      bundle folder — Godot's exporter handles bundling `res://` automatically.
#   4. Drops the finished artifacts into godot/addons/godot_wry/bin/ios/.
#
# Requirements: rustup, an Xcode toolchain (xcodebuild, lipo, install_name_tool),
# and — for step 3 — Node/npm. Run from anywhere; paths are resolved relatively.
#
# Usage:
#   scripts/build_ios.sh [--debug] [--device-only] [--sim-only] [--skip-ui]
#
set -euo pipefail

# ── pretty output ───────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  C_INFO=$'\033[36m'; C_OK=$'\033[32m'; C_ERR=$'\033[31m'; C_RST=$'\033[0m'
else
  C_INFO=""; C_OK=""; C_ERR=""; C_RST=""
fi
status()  { echo "${C_INFO}[*]${C_RST} $*"; }
success() { echo "${C_OK}[OK]${C_RST} $*"; }
fail()    { echo "${C_ERR}[!]${C_RST} $*" >&2; }
die()     { fail "$*"; exit 1; }

# ── configuration ─────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_DIR="$PROJECT_ROOT/rust"
BIN_DIR="$PROJECT_ROOT/godot/addons/godot_wry/bin/ios"
UI_DIR="$PROJECT_ROOT/godot/addons/godot_wry/examples/character_creator_ui_demo/ui"

LIB_NAME="libgodot_wry"            # cdylib output is libgodot_wry.dylib
DEVICE_TARGET="aarch64-apple-ios"
SIM_ARM_TARGET="aarch64-apple-ios-sim" # Apple Silicon simulator
SIM_X86_TARGET="x86_64-apple-ios"      # Intel / Rosetta simulator.
# NOTE: Godot's official iOS export templates ship an x86_64-ONLY simulator
# engine lib, so a simulator app links as x86_64 (under Rosetta on Apple
# Silicon). We therefore build BOTH simulator arches and lipo them into one
# universal sim slice, so the extension matches whatever arch Xcode links.
MIN_IOS_VERSION="14.0"             # Metal (Mobile/Forward+ renderer) requires iOS 14+
# CFBundleIdentifier for the framework. MUST NOT contain underscores — iOS
# bundle-id validation only allows alphanumerics, hyphens and dots.
BUNDLE_ID="doceazedo.godotwry.libgodotwry"

PROFILE="release"
CARGO_PROFILE_FLAG="--release"
BUILD_DEVICE=1
BUILD_SIM=1
SKIP_UI=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)       PROFILE="debug"; CARGO_PROFILE_FLAG="" ;;
    --device-only) BUILD_SIM=0 ;;
    --sim-only)    BUILD_DEVICE=0 ;;
    --skip-ui)     SKIP_UI=1 ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//' | head -n 32
      exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
  shift
done

[[ "$BUILD_DEVICE" == 0 && "$BUILD_SIM" == 0 ]] && die "--device-only and --sim-only are mutually exclusive."

# ── 1. preflight ──────────────────────────────────────────────────────────────
status "godot_wry iOS build — profile=$PROFILE"
command -v rustup     >/dev/null || die "rustup not found. Install the Rust toolchain first."
command -v cargo      >/dev/null || die "cargo not found."
command -v xcodebuild >/dev/null || die "xcodebuild not found. Install Xcode + command line tools."
[[ -f "$RUST_DIR/Cargo.toml" ]]  || die "rust/Cargo.toml not found at $RUST_DIR."

status "Ensuring rustup targets are installed..."
[[ "$BUILD_DEVICE" == 1 ]] && rustup target add "$DEVICE_TARGET" >/dev/null
[[ "$BUILD_SIM"    == 1 ]] && { rustup target add "$SIM_ARM_TARGET" >/dev/null; rustup target add "$SIM_X86_TARGET" >/dev/null; }
success "Targets ready."

# ── 2. (optional) rebuild the Svelte demo UI ──────────────────────────────────
# Godot packs res:// (including the built UI under .../ui/build/) into the .pck
# inside the exported .ipa. We only need the build output to exist in the repo;
# Godot does the bundling. This step just refreshes it.
if [[ "$SKIP_UI" == 0 && -f "$UI_DIR/package.json" ]]; then
  if command -v npm >/dev/null; then
    status "Rebuilding Svelte demo UI (res:// assets)..."
    ( cd "$UI_DIR" && { [[ -d node_modules ]] || npm ci || npm install; } && npm run build )
    success "Demo UI rebuilt at $UI_DIR/build"
  else
    fail "npm not found — skipping UI rebuild (using committed build/). Pass --skip-ui to silence."
  fi
else
  status "Skipping Svelte UI rebuild."
fi

# ── 3. cross-compile ──────────────────────────────────────────────────────────
build_target() {
  local target="$1"
  status "Compiling $target ($PROFILE)..."
  ( cd "$RUST_DIR" && cargo build --target "$target" $CARGO_PROFILE_FLAG )
  local dylib="$RUST_DIR/target/$target/$PROFILE/$LIB_NAME.dylib"
  [[ -f "$dylib" ]] || die "Expected $dylib but it was not produced."
  success "Built $target"
}
[[ "$BUILD_DEVICE" == 1 ]] && build_target "$DEVICE_TARGET"
[[ "$BUILD_SIM"    == 1 ]] && { build_target "$SIM_ARM_TARGET"; build_target "$SIM_X86_TARGET"; }

# ── 4. package each slice as a flat iOS .framework ────────────────────────────
# iOS frameworks are FLAT (unlike versioned macOS frameworks):
#   libgodot_wry.framework/
#     libgodot_wry         <- the binary (no extension, == CFBundleExecutable)
#     Info.plist
# The binary's install name MUST be @rpath-relative so the dynamic loader finds
# it once the framework is embedded in the app bundle.
write_info_plist() {
  local fw="$1" supported_platform="$2" dt_platform="$3"
  cat > "$fw/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>$LIB_NAME</string>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>$LIB_NAME</string>
	<key>CFBundlePackageType</key>
	<string>FMWK</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0.0</string>
	<key>CFBundleVersion</key>
	<string>1.0.0</string>
	<key>CFBundleSupportedPlatforms</key>
	<array>
		<string>$supported_platform</string>
	</array>
	<key>MinimumOSVersion</key>
	<string>$MIN_IOS_VERSION</string>
	<key>DTPlatformName</key>
	<string>$dt_platform</string>
</dict>
</plist>
PLIST
}

# Build a flat .framework from a single-arch target.
make_framework() {
  local target="$1" supported_platform="$2" dt_platform="$3" out_dir="$4"
  local dylib="$RUST_DIR/target/$target/$PROFILE/$LIB_NAME.dylib"
  local fw="$out_dir/$LIB_NAME.framework"
  rm -rf "$fw"; mkdir -p "$fw"
  cp "$dylib" "$fw/$LIB_NAME"
  install_name_tool -id "@rpath/$LIB_NAME.framework/$LIB_NAME" "$fw/$LIB_NAME"
  write_info_plist "$fw" "$supported_platform" "$dt_platform"
  echo "$fw"
}

# Build a flat .framework whose binary is a universal lipo of several targets.
make_universal_framework() {
  local supported_platform="$1" dt_platform="$2" out_dir="$3"; shift 3
  local fw="$out_dir/$LIB_NAME.framework"
  local dylibs=()
  local t
  for t in "$@"; do dylibs+=("$RUST_DIR/target/$t/$PROFILE/$LIB_NAME.dylib"); done
  rm -rf "$fw"; mkdir -p "$fw"
  lipo -create "${dylibs[@]}" -output "$fw/$LIB_NAME"
  install_name_tool -id "@rpath/$LIB_NAME.framework/$LIB_NAME" "$fw/$LIB_NAME"
  write_info_plist "$fw" "$supported_platform" "$dt_platform"
  echo "$fw"
}

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
XC_ARGS=()

mkdir -p "$BIN_DIR"

if [[ "$BUILD_DEVICE" == 1 ]]; then
  status "Packaging device framework (iPhoneOS)..."
  DEV_FW="$(make_framework "$DEVICE_TARGET" "iPhoneOS" "iphoneos" "$STAGE/device")"
  XC_ARGS+=(-framework "$DEV_FW")
  # Also publish a standalone device framework (matches the godot-rust book layout).
  rm -rf "$BIN_DIR/$LIB_NAME.ios.framework"
  cp -R "$DEV_FW" "$BIN_DIR/$LIB_NAME.ios.framework"
  success "Device framework -> $BIN_DIR/$LIB_NAME.ios.framework"
fi

if [[ "$BUILD_SIM" == 1 ]]; then
  status "Packaging universal simulator framework (arm64 + x86_64)..."
  SIM_FW="$(make_universal_framework "iPhoneSimulator" "iphonesimulator" "$STAGE/sim" "$SIM_ARM_TARGET" "$SIM_X86_TARGET")"
  XC_ARGS+=(-framework "$SIM_FW")
  success "Universal simulator framework staged ($(lipo -archs "$SIM_FW/$LIB_NAME"))."
fi

# ── 5. create the .xcframework (device + simulator) ───────────────────────────
status "Creating $LIB_NAME.xcframework..."
rm -rf "$BIN_DIR/$LIB_NAME.xcframework"
xcodebuild -create-xcframework "${XC_ARGS[@]}" -output "$BIN_DIR/$LIB_NAME.xcframework"
success "xcframework -> $BIN_DIR/$LIB_NAME.xcframework"

# ── 6. report ─────────────────────────────────────────────────────────────────
echo
success "iOS build complete."
echo "Artifacts in: $BIN_DIR"
ls -1 "$BIN_DIR"
cat <<NEXT

Next steps:
  1. Confirm godot/addons/godot_wry/WRY.gdextension has the ios.* entries
     pointing at bin/ios/$LIB_NAME.xcframework.
  2. Open godot/project.godot in Godot, then Project > Export... > iOS.
  3. Export the Xcode project, open it in Xcode, pick a Simulator (Apple
     Silicon) or a device, and Run.

See IOS_BUILD_COMMANDS.md for the full workflow and troubleshooting.
NEXT
