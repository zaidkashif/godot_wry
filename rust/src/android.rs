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
//!
//! # THE CRITICAL FIX
//!
//! `ndk::looper::ThreadLooper` must be stored in a `static` so it lives for
//! the entire process lifetime. If it is a local variable inside the
//! `INIT_GUARD.call_once(|| { ... })` closure, Rust drops it the moment the
//! closure returns — but WRY has already stored a raw pointer to it for use
//! by its internal message-pump. Accessing that dangling pointer later causes
//! a SIGABRT via libndk_translation (visible as an anonymous frame in the
//! crash backtrace). Storing it in a `OnceLock<ThreadLooper>` keeps it alive
//! for the process lifetime, matching what WRY expects.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Once, OnceLock};

// ---------------------------------------------------------------------------
// State tracking
// ---------------------------------------------------------------------------

static JAVA_VM_PTR: AtomicUsize = AtomicUsize::new(0);
static ANDROID_SETUP_DONE: AtomicBool = AtomicBool::new(false);
static INIT_GUARD: Once = Once::new();

// ── FIX: store JavaVM and ThreadLooper in statics so they are NEVER dropped.
//
// Previously both were local variables (or reconstructed on demand).
// WRY stores raw pointers into both objects. If either object is freed while
// WRY still holds the pointer the process receives SIGABRT inside
// libndk_translation on the emulator (anonymous frame #02 in the backtrace).
static JAVA_VM: OnceLock<jni::JavaVM> = OnceLock::new();
// Store a leaked pointer as usize to avoid Send/Sync requirements in a static.
static MAIN_LOOPER_PTR: AtomicUsize = AtomicUsize::new(0);

static ASSET_MANAGER_PTR: AtomicUsize = AtomicUsize::new(0);

/// Returns true if `wry::android_setup()` has been called successfully.
pub fn is_android_ready() -> bool {
    ANDROID_SETUP_DONE.load(Ordering::Acquire)
}

/// Reconstruct a `jni::JavaVM` wrapper from our stored raw pointer.
#[allow(dead_code)]
fn get_vm() -> ManuallyDrop<jni::JavaVM> {
    let ptr = JAVA_VM_PTR.load(Ordering::Acquire);
    assert!(
        ptr != 0,
        "[godot_wry] JavaVM not initialised — JNI_OnLoad was never called."
    );
    unsafe { ManuallyDrop::new(jni::JavaVM::from_raw(ptr as *mut jni::sys::JavaVM).unwrap()) }
}

pub fn get_asset_manager_ptr() -> *mut ndk_sys::AAssetManager {
    ASSET_MANAGER_PTR.load(Ordering::Acquire) as *mut ndk_sys::AAssetManager
}

/// Attach the current thread to the JVM as a daemon thread.
pub fn attach_current_thread() -> Option<jni::AttachGuard<'static>> {
    let vm = JAVA_VM.get()?;
    vm.attach_current_thread().ok()
}

// ---------------------------------------------------------------------------
// JNI_OnLoad — process entry point
// ---------------------------------------------------------------------------

/// Called by the Android dynamic linker immediately after our `.so` is loaded.
/// Stores the JavaVM pointer and the full JavaVM wrapper in a static.
/// DO NOT call ndk_context::initialize_android_context() here — Godot's
/// GDExtension system does that during extension load.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jni::sys::jint {
    // Store raw pointer for atomic access.
    JAVA_VM_PTR.store(vm as usize, Ordering::Release);

    // Also store the full JavaVM wrapper in a static so attach_current_thread()
    // can use it without reconstructing from the raw pointer every time.
    if let Ok(jvm) = jni::JavaVM::from_raw(vm) {
        let _ = JAVA_VM.set(jvm);
    }

    jni::sys::JNI_VERSION_1_6
}

// ---------------------------------------------------------------------------
// nativeInit — called from WryActivity.onCreate() on the UI thread
// ---------------------------------------------------------------------------

/// JNI entry point called from `WryActivity.onCreate()`.
///
/// Runs on the **main/UI thread** — exactly what WRY requires because it
/// registers activity-result callbacks and attaches to the main Looper.
///
/// Protected by `INIT_GUARD` so it is only executed once even if called
/// multiple times (e.g. after Activity recreation).
#[no_mangle]
pub unsafe extern "C" fn Java_com_example_godotwry_WryActivity_nativeInit(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    activity: jni::objects::JObject,
) {
    INIT_GUARD.call_once(|| {
        // ── 1. Store the native AssetManager pointer ──────────────────────
        if let Ok(asset_manager_jobject) = env.call_method(
            &activity,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        ) {
            if let Ok(obj) = asset_manager_jobject.l() {
                let raw_asset_manager = ndk_sys::AAssetManager_fromJava(
                    env.get_native_interface(),
                    obj.as_raw(),
                );
                ASSET_MANAGER_PTR.store(raw_asset_manager as usize, Ordering::Release);
            }
        }

        // ── 2. Sanity check — JNI_OnLoad must have run first ─────────────
        let vm_ptr = JAVA_VM_PTR.load(Ordering::Acquire);
        if vm_ptr == 0 {
            // Should never happen — the dynamic linker always calls JNI_OnLoad
            // before any JNI function. Log and bail.
            godot::prelude::godot_error!(
                "[godot_wry] JNI_OnLoad was not called before nativeInit. \
                 The JavaVM pointer is NULL — aborting android_setup."
            );
            return;
        }

        // ── 3. Create a GlobalRef for the Activity ────────────────────────
        // A GlobalRef keeps the Java object alive across JNI calls and threads.
        let activity_global = env
            .new_global_ref(&activity)
            .expect("[godot_wry] Failed to create global ref for Activity");

        // ── 4. Tell ndk_context about our Activity ────────────────────────
        // This updates the slot that ndk_context::android_activity() reads,
        // giving WRY (and any other NDK helper) the correct Activity pointer.
        ndk_context::initialize_android_context(
            vm_ptr as *mut c_void,
            activity_global.as_raw() as *mut c_void,
        );

        // ── 5. Capture the main-thread Looper into a STATIC ───────────────
        //
        // THE CRITICAL FIX IS HERE.
        //
        // `ndk::looper::ThreadLooper::for_thread()` returns an object that
        // wraps the ALooper* for the calling thread (the UI thread / main
        // thread in this case). WRY's android_setup() stores a raw pointer
        // into this object so it can wake the Looper when it needs to run
        // work on the UI thread (e.g. when WebViewBuilder::build() is called
        // from another thread).
        //
        // If the ThreadLooper is a LOCAL variable inside this closure, Rust
        // drops it when call_once() returns — but WRY still holds the raw
        // pointer. The next time WRY tries to use it (typically when the
        // first WebView is built, ~10 seconds later in the Godot startup
        // sequence) the process receives SIGABRT inside libndk_translation.
        //
        // Storing it in MAIN_LOOPER (a OnceLock<ThreadLooper>) ensures it
        // lives for the entire process lifetime, which is exactly as long as
        // WRY needs the pointer to remain valid.
        // We leak the ThreadLooper to keep it alive for the process lifetime.
        // Store the raw pointer as usize to avoid static Send/Sync bounds.
        let looper = ndk::looper::ThreadLooper::for_thread()
            .expect("[godot_wry] No Looper on UI thread — WryActivity.onCreate() must run on the main thread");
        let looper_raw = Box::into_raw(Box::new(looper)) as usize;
        MAIN_LOOPER_PTR.store(looper_raw, Ordering::Release);
        let looper_ref: &ndk::looper::ThreadLooper = unsafe {
            &*(looper_raw as *const ndk::looper::ThreadLooper)
        };

        // ── 6. Initialise WRY's Android backend ───────────────────────────
        // We need a fresh JNIEnv because android_setup() takes ownership of it.
        let env_for_wry = jni::JNIEnv::from_raw(env.get_native_interface())
            .expect("[godot_wry] Failed to create JNIEnv for wry::android_setup");

        wry::android_setup(
            "com/example/godotwry",
            env_for_wry,
            looper_ref,             // ← reference into static, never dangling
            activity_global.clone(),
        );

        ANDROID_SETUP_DONE.store(true, Ordering::Release);

        // ── 7. Keep the GlobalRef alive for the process lifetime ──────────
        // ndk_context holds the raw pointer from activity_global.as_raw().
        // We must NOT drop activity_global or that raw pointer becomes
        // dangling. std::mem::forget() prevents the Drop impl from running.
        std::mem::forget(activity_global);
    });
}