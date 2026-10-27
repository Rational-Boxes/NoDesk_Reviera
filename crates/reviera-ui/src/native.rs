// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Native surfaces for the viewport, per platform:
//!
//! - Linux on X11: the panel's own X11 window.
//! - Linux on Wayland: a subsurface kept over the panel (see `native/gtk.c`).
//! - Windows: the panel's `HWND`.
//! - macOS: the panel's `NSView`; `wgpu` attaches a `CAMetalLayer` to it.

use std::ffi::c_void;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

#[cfg(target_os = "linux")]
mod gtk {
    use std::ffi::{c_int, c_ulong, c_void};

    unsafe extern "C" {
        pub fn reviera_scale_factor(widget: *mut c_void) -> c_int;
        pub fn reviera_x11_handles(
            widget: *mut c_void,
            xid: *mut c_ulong,
            xdisplay: *mut *mut c_void,
        ) -> c_int;
        pub fn reviera_wayland_create(
            widget: *mut c_void,
            display: *mut *mut c_void,
            surface: *mut *mut c_void,
            code: *mut c_int,
        ) -> *mut c_void;
        pub fn reviera_wayland_update(view: *mut c_void);
        pub fn reviera_wayland_destroy(view: *mut c_void);
    }
}

/// The native surface behind the viewport panel.
#[derive(Debug)]
pub struct NativeView {
    pub display: RawDisplayHandle,
    pub window: RawWindowHandle,
    /// The toolkit widget the surface belongs to (read for GTK's scale factor).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    widget: *mut c_void,
    /// The Wayland subsurface, owned by this view.
    #[cfg(target_os = "linux")]
    wayland: Option<*mut c_void>,
}

impl NativeView {
    /// Creates the native surface for a widget's native handle (`wxWindow::GetHandle()`).
    pub fn new(widget: *mut c_void) -> Result<Self, String> {
        if widget.is_null() {
            return Err("the viewport window has no native handle yet".into());
        }
        Self::create(widget)
    }

    #[cfg(target_os = "linux")]
    fn create(widget: *mut c_void) -> Result<Self, String> {
        use raw_window_handle::{
            WaylandDisplayHandle, WaylandWindowHandle, XlibDisplayHandle, XlibWindowHandle,
        };
        use std::ptr::NonNull;

        let (mut xid, mut xdisplay) = (0, std::ptr::null_mut());
        match unsafe { gtk::reviera_x11_handles(widget, &mut xid, &mut xdisplay) } {
            1 => {
                return Ok(Self {
                    display: RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                        NonNull::new(xdisplay),
                        0,
                    )),
                    window: RawWindowHandle::Xlib(XlibWindowHandle::new(xid)),
                    widget,
                    wayland: None,
                });
            }
            0 => return Err("the viewport window is not realised yet".into()),
            -2 => return Err("no native X11 window could be created for the viewport".into()),
            _ => {}
        }
        let (mut display, mut surface, mut code) = (std::ptr::null_mut(), std::ptr::null_mut(), 0);
        let view =
            unsafe { gtk::reviera_wayland_create(widget, &mut display, &mut surface, &mut code) };
        match (NonNull::new(display), NonNull::new(surface), code) {
            (Some(d), Some(s), 1) => Ok(Self {
                display: RawDisplayHandle::Wayland(WaylandDisplayHandle::new(d)),
                window: RawWindowHandle::Wayland(WaylandWindowHandle::new(s)),
                widget,
                wayland: Some(view),
            }),
            (_, _, -1) => Err("the GTK display is neither X11 nor Wayland".into()),
            _ => Err("the Wayland compositor offers no subsurfaces".into()),
        }
    }

    #[cfg(target_os = "windows")]
    fn create(widget: *mut c_void) -> Result<Self, String> {
        use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
        let hwnd = std::num::NonZeroIsize::new(widget as isize).ok_or("no HWND")?;
        Ok(Self {
            display: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
            window: RawWindowHandle::Win32(Win32WindowHandle::new(hwnd)),
            widget,
        })
    }

    #[cfg(target_os = "macos")]
    fn create(widget: *mut c_void) -> Result<Self, String> {
        use raw_window_handle::{AppKitDisplayHandle, AppKitWindowHandle};
        let view = std::ptr::NonNull::new(widget).ok_or("no NSView")?;
        Ok(Self {
            display: RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
            window: RawWindowHandle::AppKit(AppKitWindowHandle::new(view)),
            widget,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    fn create(_widget: *mut c_void) -> Result<Self, String> {
        Err("the 3D viewport is not available on this platform yet".into())
    }

    /// Device pixels per logical pixel (GTK scale factor on Linux; wxWidgets reports logical sizes).
    pub fn scale_factor(&self) -> f64 {
        #[cfg(target_os = "linux")]
        {
            f64::from(unsafe { gtk::reviera_scale_factor(self.widget) }.max(1))
        }
        #[cfg(not(target_os = "linux"))]
        {
            1.0
        }
    }

    /// Keeps a Wayland subsurface over the panel after layout changes.
    pub fn update(&self) {
        #[cfg(target_os = "linux")]
        if let Some(v) = self.wayland {
            unsafe { gtk::reviera_wayland_update(v) }
        }
    }
}

impl Drop for NativeView {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(v) = self.wayland.take() {
            unsafe { gtk::reviera_wayland_destroy(v) }
        }
    }
}
