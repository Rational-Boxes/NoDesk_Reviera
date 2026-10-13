// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Bindings to `native/wxshim.cpp`: wxWidgets features wxDragon does not wrap yet.

use std::ffi::{c_int, c_void};

use wxdragon::prelude::*;

unsafe extern "C" {
    fn reviera_nb_bind(
        notebook: *mut c_void,
        callback: extern "C" fn(*mut c_void, c_int, c_int),
        user: *mut c_void,
    );
    fn reviera_nb_split(notebook: *mut c_void, page: usize, direction: c_int);
    fn reviera_nb_unsplit_all(notebook: *mut c_void);
    fn reviera_nb_page_index(notebook: *mut c_void, page: *mut c_void) -> c_int;
}

/// A notebook event: the page index it concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEvent {
    Changed(usize),
    /// The close button was clicked; the page is not closed (the handler decides).
    Close(usize),
    MiddleClick(usize),
    RightClick(usize),
}

type Handler = Box<dyn Fn(TabEvent)>;

extern "C" fn trampoline(user: *mut c_void, kind: c_int, page: c_int) {
    let Ok(page) = usize::try_from(page) else {
        return;
    };
    let event = match kind {
        1 => TabEvent::Changed(page),
        2 => TabEvent::Close(page),
        3 => TabEvent::MiddleClick(page),
        4 => TabEvent::RightClick(page),
        _ => return,
    };
    // SAFETY: `user` is the handler leaked in `on_tab_event`, alive for the program.
    let handler = unsafe { &*(user as *const Handler) };
    handler(event);
}

fn ptr(w: &impl WxWidget) -> *mut c_void {
    w.handle_ptr() as *mut c_void
}

/// Calls `f` for the notebook's tab events. Closing is always vetoed; `f` closes pages itself.
pub fn on_tab_event(notebook: &AuiNotebook, f: impl Fn(TabEvent) + 'static) {
    // Lives as long as the notebook, which lives as long as the main window.
    let user = Box::into_raw(Box::new(Box::new(f) as Handler));
    unsafe { reviera_nb_bind(ptr(notebook), trampoline, user as *mut c_void) }
}

#[derive(Debug, Clone, Copy)]
pub enum Side {
    Right = 2,
    Bottom = 4,
}

/// Moves a page into a new tab group on `side`.
pub fn split(notebook: &AuiNotebook, page: usize, side: Side) {
    unsafe { reviera_nb_split(ptr(notebook), page, side as c_int) }
}

/// Merges every tab group back into one.
pub fn unsplit_all(notebook: &AuiNotebook) {
    unsafe { reviera_nb_unsplit_all(ptr(notebook)) }
}

/// The index of a page, if it is in the notebook.
pub fn page_index(notebook: &AuiNotebook, page: &impl WxWidget) -> Option<usize> {
    usize::try_from(unsafe { reviera_nb_page_index(ptr(notebook), ptr(page)) }).ok()
}
