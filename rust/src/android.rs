//! Android-specific WebView integration for godot_wry.
//!
//! # Architecture
//!
//! On Android, WRY does **not** use raw window handles (HWND / NSView / XID).
//! Instead it attaches an `android.webkit.WebView` directly to the running
//! Android Activity's view hierarchy through JNI.
//!
//! The integration requires three things to happen in order:
//!
//! 1. **`JNI_OnLoad`** — called by Android's runtime the instant our `.so` is
//!    `dlopen`-ed by Godot's APK. We store the `JavaVM*` here and call
//!    `wry::android_setup` so WRY knows the runtime is ready.
//!
//! 2. **`initialize_android_context`** — called from the GDExtension node's
//!    `ready()` lifecycle. By this point the Activity is alive, so we can
//!    reflect through `ActivityThread` to get the `Activity` jobject and
//!    register it with `ndk_context`, which WRY's Android backend reads.
//!
//! 3. **`build_android_webview`** — called from `lib.rs`'s `build_webview()`
//!    Android branch. Constructs a `wry::WebView` attached to the Activity.
//!
//! # WRY Android Binding Macro
//!
//! `wry::android_binding!` is declared in `lib.rs` at crate root (required).
//! It generates the JNI symbols that the Java-side `WryActivity` and
//! `WryWebView` call back into for IPC messages, page-load events, etc.
//!
//! # Required Java / Kotlin Side
//!
//! WRY generates `WryActivity.kt` at compile time (via `build.rs` env vars).
//! Godot's Android export must be configured to use `WryActivity` as the main
//! Activity (or a class that extends it). See the implementation plan for the
//! full Godot export configuration steps.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};

use godot::prelude::godot_error;
use wry::WebViewBuilder;

// ---------------------------------------------------------------------------
// JavaVM global pointer
// ---------------------------------------------------------------------------
//
// We store the JavaVM pointer as a `usize` in an AtomicUsize so it is
// inherently Send + Sync without needing a Mutex.  The value is written
// exactly once (in JNI_OnLoad) and then only ever read.
//
// Using `AtomicUsize` rather than `OnceLock<*mut jni::sys::JavaVM>` avoids
// the "raw pointer is not Send" problem while still being race-free.

static JAVA_VM_PTR: AtomicUsize = AtomicUsize::new(0);

/// Reconstruct a `jni::JavaVM` wrapper from our stored raw pointer.
///
/// The returned value is wrapped in `ManuallyDrop` so that the underlying
/// `JavaVM` is **not** destroyed when the wrapper is dropped — the JVM is
/// owned by the Android runtime, not by us.
///
/// # Panics
/// Panics if `JNI_OnLoad` has not been called yet.
fn get_vm() -> ManuallyDrop<jni::JavaVM> {
    let ptr = JAVA_VM_PTR.load(Ordering::Acquire);
    assert!(
        ptr != 0,
        "[godot_wry] JavaVM not initialised — JNI_OnLoad was never called. \
         Make sure libgodot_wry.so is loaded before the GDExtension node enters the tree."
    );
    // SAFETY: ptr was set from a valid *mut JavaVM in JNI_OnLoad and is valid
    // for the lifetime of the process.  ManuallyDrop prevents double-free.
    unsafe { ManuallyDrop::new(jni::JavaVM::from_raw(ptr as *mut jni::sys::JavaVM).unwrap()) }
}

// ---------------------------------------------------------------------------
// JNI_OnLoad — process entry point
// ---------------------------------------------------------------------------

/// Called by the Android dynamic linker immediately after our `.so` is loaded.
///
/// This is the earliest safe point at which JNI is available.  We:
/// - Store the `JavaVM*` so every subsequent thread can attach.
/// - Call `wry::android_setup` to register WRY's internal JNI bootstrap.
///
/// The Android Activity is **not** available yet at this point.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jni::sys::jint {
    // Store the VM pointer atomically. This is written exactly once.
    JAVA_VM_PTR.store(vm as usize, Ordering::Release);

    // Seed ndk_context with the VM pointer and a null Activity for now.
    // The Activity is set in `initialize_android_context()` once the node
    // enters the scene tree and the Activity is guaranteed to exist.
    ndk_context::initialize_android_context(vm as *mut c_void, std::ptr::null_mut());

    // Inform WRY's Android backend that the JVM is ready.
    // This must be called before any WebView is built.
    wry::android_setup(|loader| {
        // `loader` is a JNI callback that WRY uses to load its internal
        // Java classes.  We attach the current thread and invoke it.
        let vm = get_vm();
        let env = vm.attach_current_thread_permanently();
        match env {
            Ok(mut env) => loader(&mut env),
            Err(e) => {
                // Log but don't panic — a panic in JNI_OnLoad will crash the
                // whole process with no useful stack trace.
                godot_error!("[godot_wry] android_setup: failed to attach JNI thread: {e}");
            }
        }
    });

    jni::sys::JNI_VERSION_1_6
}

// ---------------------------------------------------------------------------
// Activity context initialisation
// ---------------------------------------------------------------------------

/// Obtains the current Android `Activity` via JNI reflection and registers it
/// with `ndk_context`.
///
/// Must be called **after** `JNI_OnLoad` and once Godot's Activity is running
/// (i.e., from the GDExtension node's `ready()` callback).
///
/// Uses the standard (if unofficial) `ActivityThread.currentActivity()` idiom
/// to locate the activity without needing an explicit reference passed in.
pub fn initialize_android_context() {
    let vm = get_vm();

    let mut env = vm
        .attach_current_thread()
        .expect("[godot_wry] initialize_android_context: failed to attach JNI thread");

    // ── Reflect through android.app.ActivityThread to get the Activity ────
    //
    // ActivityThread.currentActivityThread() → ActivityThread
    // ActivityThread.getActivity()           → android.app.Activity

    let activity_thread_class = env
        .find_class("android/app/ActivityThread")
        .expect("[godot_wry] Could not find android/app/ActivityThread");

    let current_thread = env
        .call_static_method(
            &activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[],
        )
        .expect("[godot_wry] ActivityThread.currentActivityThread() failed")
        .l()
        .expect("[godot_wry] currentActivityThread() returned non-object");

    let activity = env
        .call_method(
            &current_thread,
            "getActivity",
            "()Landroid/app/Activity;",
            &[],
        )
        .expect("[godot_wry] ActivityThread.getActivity() failed")
        .l()
        .expect("[godot_wry] getActivity() returned non-object");

    if activity.is_null() {
        godot_error!(
            "[godot_wry] getActivity() returned null. \
             Ensure WryActivity (or a subclass) is the main Android Activity in your \
             Godot export settings."
        );
        return;
    }

    // Promote to a GlobalRef so the object won't be GC'd.
    // We intentionally leak this ref — the Activity must stay alive for as
    // long as our WebView exists (i.e., the process lifetime).
    let activity_global = env
        .new_global_ref(&activity)
        .expect("[godot_wry] Failed to create global ref for Activity");

    let vm_ptr = JAVA_VM_PTR.load(Ordering::Acquire);

    // SAFETY: both pointers are valid for the process lifetime.
    unsafe {
        ndk_context::initialize_android_context(
            vm_ptr as *mut c_void,
            activity_global.as_raw() as *mut c_void,
        );
    }

    // Intentionally forget the GlobalRef wrapper — the raw pointer is now
    // owned by ndk_context and must survive the process lifetime.
    std::mem::forget(activity_global);
}

// ---------------------------------------------------------------------------
// WebView construction
// ---------------------------------------------------------------------------

/// Builds and returns an Android `wry::WebView`.
///
/// Unlike the desktop path, there is no parent window handle.  WRY reads the
/// `Activity` jobject from `ndk_context::android_context()` internally and
/// creates a `WebView` widget inside the Activity's view hierarchy.
///
/// # Arguments
/// * `url`  — Initial URL to navigate to (takes priority over `html`).
/// * `html` — Inline HTML string to load if no URL is provided.
///
/// Both arguments use `Option<String>` so the caller can map them directly
/// from the GodotClass export fields without branching in `lib.rs`.
pub fn build_android_webview(
    url: Option<String>,
    html: Option<String>,
    devtools: bool,
    user_agent: Option<String>,
    incognito: bool,
    autoplay: bool,
) -> Result<wry::WebView, wry::Error> {
    // Ensure the Activity is registered before we ask WRY to build anything.
    initialize_android_context();

    let mut builder = WebViewBuilder::new();

    // Content source — URL takes priority.
    builder = match (url.as_deref(), html.as_deref()) {
        (Some(u), _) if !u.is_empty() => builder.with_url(u),
        (_, Some(h)) if !h.is_empty() => builder.with_html(h),
        _ => builder.with_url("about:blank"),
    };

    // Apply supported WebView attributes.
    builder = builder
        .with_devtools(devtools)
        .with_incognito(incognito)
        .with_autoplay(autoplay);

    if let Some(ua) = user_agent.as_deref() {
        if !ua.is_empty() {
            builder = builder.with_user_agent(ua);
        }
    }

    // On Android, `build()` (not `build_as_child`) is the correct call.
    // WRY attaches the WebView to the running Activity automatically.
    builder.build()
}
