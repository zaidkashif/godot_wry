//! Android-specific WebView integration for godot_wry.
//!
//! # Architecture
//!
//! On Android, WRY does **not** use raw window handles (HWND / NSView / XID).
//! Instead it attaches an `android.webkit.WebView` directly to the running
//! Android Activity's view hierarchy through JNI.
//!
//! The integration requires two things to happen in order:
//!
//! 1. **`JNI_OnLoad`** — called by Android's runtime the instant our `.so` is
//!    `dlopen`-ed by Godot's APK loader. We store the `JavaVM*` here.
//!
//! 2. **`nativeInit(activity)`** — a JNI function called from `WryActivity.onCreate()`
//!    on the **UI thread**. This is where we call `wry::android_setup()` with the
//!    Activity reference, JNIEnv, and the main thread's Looper. This MUST happen
//!    on the UI thread because:
//!    - WRY creates a `RustWebChromeClient` using `registerForActivityResult`
//!    - Android requires this to be called before the Activity is STARTED
//!    - The main Looper must be the one processing WRY's pipe messages
//!
//! After `nativeInit` completes, `WebViewBuilder::build()` can be called from
//! any thread — WRY internally sends messages through a pipe to the MainPipe
//! running on the UI thread's Looper.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Once;

// ---------------------------------------------------------------------------
// State tracking
// ---------------------------------------------------------------------------

static JAVA_VM_PTR: AtomicUsize = AtomicUsize::new(0);
static ANDROID_SETUP_DONE: AtomicBool = AtomicBool::new(false);
static INIT_GUARD: Once = Once::new();

/// Returns true if `wry::android_setup()` has been called successfully.
pub fn is_android_ready() -> bool {
    ANDROID_SETUP_DONE.load(Ordering::Acquire)
}

/// Reconstruct a `jni::JavaVM` wrapper from our stored raw pointer.
/// Currently unused but kept for potential future Android-specific features.
#[allow(dead_code)]
fn get_vm() -> ManuallyDrop<jni::JavaVM> {
    let ptr = JAVA_VM_PTR.load(Ordering::Acquire);
    assert!(
        ptr != 0,
        "[godot_wry] JavaVM not initialised — JNI_OnLoad was never called."
    );
    unsafe { ManuallyDrop::new(jni::JavaVM::from_raw(ptr as *mut jni::sys::JavaVM).unwrap()) }
}

// ---------------------------------------------------------------------------
// JNI_OnLoad — process entry point
// ---------------------------------------------------------------------------

/// Called by the Android dynamic linker immediately after our `.so` is loaded.
/// We just store the `JavaVM*` pointer. DO NOT initialize ndk_context here!
/// Godot's GDExtension system will initialize ndk_context during extension load.
/// We will update it with the Activity pointer in nativeInit().
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jni::sys::jint {
    JAVA_VM_PTR.store(vm as usize, Ordering::Release);
    // DO NOT call ndk_context::initialize_android_context() here!
    // GDExtension will do it when the extension is loaded.
    jni::sys::JNI_VERSION_1_6
}

// ---------------------------------------------------------------------------
// nativeInit — called from WryActivity.onCreate() on the UI thread
// ---------------------------------------------------------------------------

/// JNI entry point called from `WryActivity.onCreate()`.
///
/// This runs on the **main/UI thread** which is exactly what WRY requires.
/// It receives the Activity reference directly (no reflection needed) and
/// calls `wry::android_setup()` with the correct thread's Looper.
/// 
/// Protected by Once guard to ensure it's only initialized once, even if
/// called multiple times during startup sequence.
#[no_mangle]
pub unsafe extern "C" fn Java_com_example_godotwry_WryActivity_nativeInit(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    activity: jni::objects::JObject,
) {
    INIT_GUARD.call_once(|| {
        let vm_ptr = JAVA_VM_PTR.load(Ordering::Acquire);
        if vm_ptr == 0 {
            // This shouldn't happen — JNI_OnLoad runs before any JNI call.
            return;
        }

        // Create a GlobalRef so the Activity won't be garbage collected.
        let activity_global = env
            .new_global_ref(&activity)
            .expect("[godot_wry] Failed to create global ref for Activity");

        // DO NOT call ndk_context::initialize_android_context() here!
        // Godot's GDExtension system already initialized it during extension load.
        // Calling it again causes: assertion failed: previous.is_none()
        //
        // Instead, we just ensure wry::android_setup gets the Activity reference
        // and the main thread's Looper. WRY will handle the Android integration.

        // Get the current thread's Looper (this IS the main thread's Looper
        // because WryActivity.onCreate() runs on the UI thread).
        let looper = ndk::looper::ThreadLooper::for_thread()
            .expect("[godot_wry] No Looper on UI thread — this should never happen");

        // Create a new JNIEnv reference for wry::android_setup.
        // We need a separate one because android_setup takes ownership.
        let env_for_wry = jni::JNIEnv::from_raw(env.get_native_interface())
            .expect("[godot_wry] Failed to create JNIEnv for WRY");

        wry::android_setup(
            "com.example.wry",
            env_for_wry,
            &looper,
            activity_global.clone(),
        );

        ANDROID_SETUP_DONE.store(true, Ordering::Release);
        // Successfully initialized — nativeInit will skip on subsequent calls
        
        // Intentionally forget the GlobalRef — ndk_context owns the raw pointer
        // and it must survive the process lifetime.
        std::mem::forget(activity_global);
    });
}
