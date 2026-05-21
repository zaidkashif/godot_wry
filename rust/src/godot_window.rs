use godot::classes::display_server::HandleType;
use godot::classes::DisplayServer;
use godot::obj::Singleton;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

// Platform-specific window handle imports — none of these exist on Android.
// The entire HasWindowHandle impl is gated below with cfg(not(target_os = "android")).

#[cfg(all(not(target_os = "android"), target_os = "windows"))]
use {
    std::num::{NonZero, NonZeroIsize},
    raw_window_handle::Win32WindowHandle,
};

#[cfg(all(not(target_os = "android"), target_os = "macos"))]
use {
    raw_window_handle::AppKitWindowHandle,
    std::ffi::c_void,
    std::mem::transmute,
    std::ptr::NonNull,
};

#[cfg(all(not(target_os = "android"), target_os = "linux"))]
use {
    std::ffi::c_ulong,
    raw_window_handle::XlibWindowHandle,
};

/// A thin wrapper around a Godot `window_id` that implements the
/// `raw-window-handle` traits so that WRY can accept it as a parent window.
///
/// On Android this struct is still defined but `HasWindowHandle` is not
/// implemented — the Android WebView path uses `ndk_context` instead of
/// OS window handles. The struct itself is only ever constructed under a
/// `#[cfg(not(target_os = "android"))]` guard in `lib.rs`.
pub struct GodotWindow {
    pub window_id: i32,
}

impl GodotWindow {
    pub fn new(window_id: i32) -> Self {
        Self { window_id }
    }
}

// HasWindowHandle is only meaningful on desktop platforms.
// Android uses wry::android_setup + ndk_context instead of window handles.
#[cfg(not(target_os = "android"))]
impl HasWindowHandle for GodotWindow {
    #[cfg(target_os = "windows")]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let display_server = DisplayServer::singleton();
        let window_handle = display_server
            .window_get_native_handle_ex(HandleType::WINDOW_HANDLE)
            .window_id(self.window_id)
            .done();
        let non_zero_window_handle =
            NonZero::new(window_handle).expect("WindowHandle creation failed");
        unsafe {
            Ok(WindowHandle::borrow_raw(RawWindowHandle::Win32(
                Win32WindowHandle::new({
                    NonZeroIsize::try_from(non_zero_window_handle)
                        .expect("Invalid window_handle")
                }),
            )))
        }
    }

    #[cfg(target_os = "macos")]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let display_server = DisplayServer::singleton();
        let window_handle = display_server
            .window_get_native_handle_ex(HandleType::WINDOW_VIEW)
            .window_id(self.window_id)
            .done();
        unsafe {
            Ok(WindowHandle::borrow_raw(RawWindowHandle::AppKit(
                AppKitWindowHandle::new({
                    let ptr: *mut c_void = transmute(window_handle);
                    NonNull::new(ptr).expect("Id<T> should never be null")
                }),
            )))
        }
    }

    #[cfg(target_os = "linux")]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        use gtk::gdk::prelude::DisplayExtManual;
        use x11_dl::xlib::{
            CWEventMask, SubstructureNotifyMask, SubstructureRedirectMask,
            XSetWindowAttributes, XWindowAttributes, Xlib,
        };

        gtk::init().expect("Failed to initialize gtk");
        if !gtk::gdk::Display::default().unwrap().backend().is_x11() {
            panic!("GDK backend must be X11");
        }
        let xlib = Xlib::open().expect("Failed to open Xlib");

        let display_server = DisplayServer::singleton();
        let window_xid = display_server
            .window_get_native_handle_ex(HandleType::WINDOW_HANDLE)
            .window_id(self.window_id)
            .done();
        let display = display_server
            .window_get_native_handle_ex(HandleType::DISPLAY_HANDLE)
            .window_id(self.window_id)
            .done();

        unsafe {
            let attributes: XWindowAttributes = std::mem::zeroed();
            let mut attributes = std::mem::MaybeUninit::new(attributes).assume_init();

            let ok = (xlib.XGetWindowAttributes)(display as _, window_xid as c_ulong, &mut attributes);
            if ok != 1 {
                panic!("Failed to get X11 window attributes");
            }

            let mut set_attributes: XSetWindowAttributes = std::mem::zeroed();
            set_attributes.event_mask = attributes.all_event_masks
                & !SubstructureNotifyMask
                & !SubstructureRedirectMask;
            let ok = (xlib.XChangeWindowAttributes)(
                display as _,
                window_xid as c_ulong,
                CWEventMask,
                &mut set_attributes,
            );
            if ok != 1 {
                panic!("Failed to change X11 window attributes");
            }
        }

        unsafe {
            Ok(WindowHandle::borrow_raw(RawWindowHandle::Xlib(
                XlibWindowHandle::new({ window_xid as c_ulong }),
            )))
        }
    }
}
