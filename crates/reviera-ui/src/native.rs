// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Native window handles for the viewport. On Linux, wxGTK runs on X11 (XWayland on Wayland
//! desktops) so that a child window can be a native X11 window for `wgpu`.

use std::ffi::c_void;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn reviera_prefer_x11();
    fn reviera_x11_handles(
        widget: *mut c_void,
        xid: *mut std::ffi::c_ulong,
        xdisplay: *mut *mut c_void,
    ) -> std::ffi::c_int;
}

/// Restricts the toolkit to X11. Call before the toolkit starts.
pub fn prefer_x11() {
    #[cfg(target_os = "linux")]
    unsafe {
        reviera_prefer_x11()
    }
}

/// The display and window handles behind a widget's native handle (`wxWindow::GetHandle()`).
pub fn handles(widget: *mut c_void) -> Result<(RawDisplayHandle, RawWindowHandle), String> {
    #[cfg(target_os = "linux")]
    {
        use raw_window_handle::{XlibDisplayHandle, XlibWindowHandle};
        let (mut xid, mut display) = (0, std::ptr::null_mut());
        match unsafe { reviera_x11_handles(widget, &mut xid, &mut display) } {
            1 => Ok((
                RawDisplayHandle::Xlib(XlibDisplayHandle::new(std::ptr::NonNull::new(display), 0)),
                RawWindowHandle::Xlib(XlibWindowHandle::new(xid)),
            )),
            0 => Err("the viewport window is not realised yet".into()),
            -1 => Err("the 3D viewport needs GTK on X11 (set GDK_BACKEND=x11)".into()),
            _ => Err("no native window could be created for the viewport".into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = widget;
        Err("the 3D viewport is not available on this platform yet".into())
    }
}
