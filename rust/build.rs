// build.rs — godot_wry
//
// WRY's Android backend generates Kotlin/Java glue files at compile time.
// These files (WryActivity.kt, etc.) define the Android Activity that hosts
// the WebView and the JNI bridge that WRY's Rust code calls back into.
//
// Three environment variables MUST be set before cross-compiling for Android:
//
//   WRY_ANDROID_PACKAGE
//       The reversed-domain package name + app name in snake_case,
//       e.g. "com.example.mygame.godot_wry"
//       This must match the package declared in your Godot Android export
//       settings → Application → Package.
//
//   WRY_ANDROID_LIBRARY
//       The bare library name (without "lib" prefix or ".so" suffix),
//       i.e. "godot_wry"  (matches the [package] name in Cargo.toml).
//
//   WRY_ANDROID_KOTLIN_FILES_OUT_DIR
//       Absolute path to the directory where the Kotlin source files should
//       be written, e.g. the `src/main/kotlin/com/example/mygame/` folder
//       inside the Godot Android export's gradle project.
//       When using Godot's built-in Android exporter this is typically under:
//       <project>/android/build/src/main/kotlin/<package/path>/
//
// WRY checks these vars internally during its own build script.
// Our build.rs just propagates cargo:rerun-if-env-changed directives so
// that Cargo re-runs this script whenever the vars change.

fn main() {
    // Tell Cargo to re-run this build script if the Android configuration
    // environment variables change.
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_PACKAGE");
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_LIBRARY");
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");

    // Emit a helpful compile-time warning if the vars are missing during an
    // Android cross-compilation attempt.
    #[cfg(target_os = "android")]
    {
        if std::env::var("WRY_ANDROID_PACKAGE").is_err() {
            println!(
                "cargo:warning=WRY_ANDROID_PACKAGE is not set. \
                 WRY cannot generate Android Kotlin glue files. \
                 Set it to your reversed-domain package name, e.g. \
                 com.example.mygame.godot_wry"
            );
        }
        if std::env::var("WRY_ANDROID_LIBRARY").is_err() {
            println!(
                "cargo:warning=WRY_ANDROID_LIBRARY is not set. \
                 Set it to 'godot_wry' to match the crate name."
            );
        }
        if std::env::var("WRY_ANDROID_KOTLIN_FILES_OUT_DIR").is_err() {
            println!(
                "cargo:warning=WRY_ANDROID_KOTLIN_FILES_OUT_DIR is not set. \
                 Set it to the path where WRY should write WryActivity.kt, \
                 inside your Godot Android export's gradle source tree."
            );
        }
    }
}
