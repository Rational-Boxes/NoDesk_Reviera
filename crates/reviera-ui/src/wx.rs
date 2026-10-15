// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Bindings to `native/wxshim.cpp`: wxWidgets features wxDragon does not wrap yet.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use wxdragon::prelude::*;

unsafe extern "C" {
    fn reviera_nb_bind(
        notebook: *mut c_void,
        callback: extern "C" fn(*mut c_void, c_int, c_int),
        user: *mut c_void,
    );
    fn reviera_nb_set_style(notebook: *mut c_void);
    fn reviera_nb_split(notebook: *mut c_void, page: usize, direction: c_int);
    fn reviera_nb_unsplit_all(notebook: *mut c_void);
    fn reviera_nb_page_index(notebook: *mut c_void, page: *mut c_void) -> c_int;
    fn reviera_nb_save_layout(notebook: *mut c_void, groups: *mut c_int) -> *mut c_void;
    fn reviera_nb_layout_covers(notebook: *mut c_void, layout: *const c_void) -> c_int;
    fn reviera_nb_load_layout(notebook: *mut c_void, layout: *const c_void);
    fn reviera_nb_free_layout(layout: *mut c_void);
    fn reviera_html_create(panel: *mut c_void) -> *mut c_void;
    fn reviera_html_bind(
        html: *mut c_void,
        callback: extern "C" fn(*mut c_void, *const c_char),
        user: *mut c_void,
    );
    fn reviera_html_set_page(html: *mut c_void, page: *const c_char, anchor: *const c_char);
}

/// wxWidgets' own HTML window, filling a panel: the help where no web view is available.
#[derive(Debug, Clone, Copy)]
pub struct HtmlView(*mut c_void);

type LinkHandler = Box<dyn Fn(String)>;

extern "C" fn link_trampoline(user: *mut c_void, href: *const c_char) {
    // SAFETY: `user` is the handler leaked in `HtmlView::new`; `href` is a C string.
    let handler = unsafe { &*(user as *const LinkHandler) };
    let href = unsafe { CStr::from_ptr(href) }.to_string_lossy().into_owned();
    handler(href);
}

impl HtmlView {
    /// Fills `panel` with an HTML window; `on_link` gets each clicked link (other than anchors
    /// within the page, which it scrolls to itself).
    pub fn new(panel: &Panel, on_link: impl Fn(String) + 'static) -> Self {
        let raw = unsafe { reviera_html_create(ptr(panel)) };
        // Lives as long as the window: a few bytes, once per help tab opened.
        let user = Box::into_raw(Box::new(Box::new(on_link) as LinkHandler));
        unsafe { reviera_html_bind(raw, link_trampoline, user as *mut c_void) };
        Self(raw)
    }

    /// Shows `html`, scrolled to `anchor` when it is not empty.
    pub fn set_page(&self, html: &str, anchor: &str) {
        let html = CString::new(html.replace('\0', "")).expect("no NULs");
        let anchor = CString::new(anchor.replace('\0', "")).expect("no NULs");
        unsafe { reviera_html_set_page(self.0, html.as_ptr(), anchor.as_ptr()) }
    }
}

/// A saved arrangement of the view tabs: tab groups, their pages and divider positions.
#[derive(Debug)]
pub struct Layout {
    raw: *mut c_void,
    /// How many tab groups it has (1 when the views are all tabs).
    pub groups: usize,
}

impl Drop for Layout {
    fn drop(&mut self) {
        unsafe { reviera_nb_free_layout(self.raw) }
    }
}

/// The notebook's current layout.
pub fn save_layout(notebook: &AuiNotebook) -> Layout {
    let mut groups: c_int = 0;
    let raw = unsafe { reviera_nb_save_layout(ptr(notebook), &mut groups) };
    Layout {
        raw,
        groups: usize::try_from(groups).unwrap_or(0),
    }
}

impl Layout {
    /// Whether every page now in the notebook is in this layout.
    pub fn covers(&self, notebook: &AuiNotebook) -> bool {
        unsafe { reviera_nb_layout_covers(ptr(notebook), self.raw) != 0 }
    }

    /// Restores the layout (pages it does not know join the main group).
    pub fn restore(&self, notebook: &AuiNotebook) {
        unsafe { reviera_nb_load_layout(ptr(notebook), self.raw) }
    }
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

/// Gives every view tab a close button, and lets the tabs be dragged: reordered within a tab
/// group, moved between groups, and dropped at an edge to tile.
pub fn enable_tab_dragging(notebook: &AuiNotebook) {
    unsafe { reviera_nb_set_style(ptr(notebook)) }
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
