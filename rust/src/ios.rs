//! iOS-specific WebView integration for godot_wry.
//!
//! # How iOS differs from Android
//!
//! On **Android**, WRY does not use a window handle at all — it attaches an
//! `android.webkit.WebView` to the Activity via JNI/`ndk_context`, which is why
//! the Android path needs `wry::android_setup()` and a `JNI_OnLoad` hook (see
//! `android.rs`).
//!
//! On **iOS**, WRY's backend is plain `WKWebView` and it *does* take a real
//! window handle. `raw_window_handle::UiKitWindowHandle` wraps a `UIView*`, and
//! WRY adds its `WKWebView` as a **subview** of that view. So the iOS path looks
//! much more like the macOS/AppKit path than the Android path:
//!
//!   * There is **no** `ios_setup()` / `JNI_OnLoad` equivalent — nothing to
//!     initialise before building.
//!   * We hand WRY the `UIView*` returned by Godot's `DisplayServer`
//!     (`WINDOW_VIEW` → `godotView`) and optionally the `UIViewController*`
//!     (`WINDOW_HANDLE` → root view controller).
//!
//! # Why we still need this module ("View Grafting")
//!
//! Godot renders the 3D scene into `godotView` (an OpenGL/Metal-backed
//! `UIView`). When WRY adds its `WKWebView` as a subview, two things can make
//! the web UI either invisible or fully occlude the 3D scene:
//!
//!   1. **Z-order** — depending on when the webview is created relative to
//!      Godot's own layers, it may sit *behind* the rendering surface. We
//!      explicitly `bringSubviewToFront:` it.
//!   2. **Opacity** — a `WKWebView` is opaque by default and paints a white
//!      background, which would hide the Godot scene entirely. We force
//!      `isOpaque = NO` and `backgroundColor = [UIColor clearColor]` on both the
//!      web view and its internal `UIScrollView` so the 3D scene shows through
//!      wherever the page itself is transparent.
//!
//! This is the iOS counterpart of the Android "force transparent" logic in
//! `lib.rs`. Everything here is **best-effort**: every step is guarded and
//! failures only log, never panic — a webview that is merely opaque is far
//! better than a crash.
//!
//! # Threading
//!
//! All UIKit calls here MUST run on the main thread. The only caller is
//! `WebView::create_webview()` which runs inside Godot's `_process()` loop, i.e.
//! the main thread. Do not call these functions from a WRY IPC/page-load
//! callback (those fire on background threads).

use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{class, msg_send};
use std::ffi::c_void;

use godot::classes::display_server::HandleType;
use godot::classes::DisplayServer;
use godot::obj::Singleton;
use godot::prelude::{godot_print, godot_warn};

/// Pointer to Godot's main `UIView` — the `godotView` that the engine renders
/// the 3D scene into. This is the view WRY parents its `WKWebView` to.
///
/// Maps to `DisplayServerIOS::window_get_native_handle(WINDOW_VIEW)`, which
/// returns `AppDelegate.viewController.godotView`.
pub fn get_godot_ui_view() -> *mut c_void {
    let display_server = DisplayServer::singleton();
    display_server.window_get_native_handle(HandleType::WINDOW_VIEW) as *mut c_void
}

/// Pointer to Godot's root `UIViewController`.
///
/// Maps to `DisplayServerIOS::window_get_native_handle(WINDOW_HANDLE)`, which
/// returns `AppDelegate.viewController`. WRY does not strictly require this, but
/// `UiKitWindowHandle` accepts it as an optional field and some WebKit features
/// (e.g. presenting file pickers) behave better when it is provided.
pub fn get_godot_view_controller() -> *mut c_void {
    let display_server = DisplayServer::singleton();
    display_server.window_get_native_handle(HandleType::WINDOW_HANDLE) as *mut c_void
}

/// Walk `parent_view`'s subviews, find the `WKWebView` that WRY just added, and:
///   * force it (and its scroll view) transparent, and
///   * raise it to the front of the view stack.
///
/// `parent_view` must be the same `UIView*` that was handed to WRY's
/// `WebViewBuilder::build()` (i.e. Godot's `godotView`). Safe to call more than
/// once; if no `WKWebView` is found yet it simply logs and returns.
///
/// # Safety
/// Sends Objective-C messages to live UIKit objects. Must be called on the main
/// thread with a valid (or null) `parent_view` pointer.
pub fn graft_webview_to_front(parent_view: *mut c_void) {
    if parent_view.is_null() {
        godot_warn!("[Godot WRY] iOS view grafting skipped: godotView pointer is null.");
        return;
    }

    unsafe {
        let parent: *mut AnyObject = parent_view.cast();

        // `parent.subviews` → NSArray<UIView*>
        let subviews: *mut AnyObject = msg_send![parent, subviews];
        if subviews.is_null() {
            godot_warn!("[Godot WRY] iOS view grafting: godotView has no subviews array.");
            return;
        }

        let count: usize = msg_send![subviews, count];
        let wk_class: &AnyClass = class!(WKWebView);

        let mut webview: *mut AnyObject = std::ptr::null_mut();
        for i in 0..count {
            let view: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            if view.is_null() {
                continue;
            }
            let is_webview: Bool = msg_send![view, isKindOfClass: wk_class];
            if is_webview.as_bool() {
                webview = view;
                break;
            }
        }

        if webview.is_null() {
            // Not necessarily an error: on the very first frame WRY may not have
            // inserted the WKWebView yet. The caller can retry on a later frame.
            godot_warn!(
                "[Godot WRY] iOS view grafting: no WKWebView subview found on godotView yet."
            );
            return;
        }

        // [UIColor clearColor] — an autoreleased singleton, fine to use inline.
        let ui_color_class: &AnyClass = class!(UIColor);
        let clear_color: *mut AnyObject = msg_send![ui_color_class, clearColor];

        // Make the web view itself see-through.
        let _: () = msg_send![webview, setOpaque: Bool::new(false)];
        if !clear_color.is_null() {
            let _: () = msg_send![webview, setBackgroundColor: clear_color];
        }

        // The WKWebView's internal UIScrollView also paints a background; clear
        // it too or it will mask the Godot scene behind the page.
        let scroll_view: *mut AnyObject = msg_send![webview, scrollView];
        if !scroll_view.is_null() {
            let _: () = msg_send![scroll_view, setOpaque: Bool::new(false)];
            if !clear_color.is_null() {
                let _: () = msg_send![scroll_view, setBackgroundColor: clear_color];
            }
        }

        // Raise the web UI above Godot's rendering layer.
        let _: () = msg_send![parent, bringSubviewToFront: webview];

        godot_print!("[Godot WRY] iOS WebView grafted to front and made transparent.");
    }
}
