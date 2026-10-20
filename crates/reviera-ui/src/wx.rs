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
    fn reviera_wants_chars(window: *mut c_void);
    fn reviera_bind_tab(
        frame: *mut c_void,
        callback: extern "C" fn(*mut c_void, c_int),
        user: *mut c_void,
    );
    fn reviera_html_bind(
        html: *mut c_void,
        callback: extern "C" fn(*mut c_void, *const c_char),
        user: *mut c_void,
    );
    fn reviera_html_set_page(html: *mut c_void, page: *const c_char, anchor: *const c_char);
    fn reviera_frame_show_menu_bar(frame: *mut c_void, show: c_int);
    fn reviera_frame_show_status_bar(frame: *mut c_void, show: c_int);
    fn reviera_pointer_state(frame: *mut c_void, views: *mut c_void, out: *mut c_int);
}

/// The pointer against the window's edges (UI-DOCK-004), in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerState {
    pub x: i32,
    pub y: i32,
    /// A mouse button is held.
    pub held: bool,
    /// The edges: the display in full-screen, else the window (left, top, right, bottom).
    pub area: [i32; 4],
    /// The views area's top and bottom.
    pub views: [i32; 2],
}

/// Where the pointer is, for edge reveals.
pub fn pointer_state(frame: &Frame, views: &impl WxWidget) -> PointerState {
    let mut o = [0 as c_int; 9];
    unsafe {
        reviera_pointer_state(
            frame.handle_ptr() as *mut c_void,
            views.handle_ptr() as *mut c_void,
            o.as_mut_ptr(),
        )
    };
    PointerState {
        x: o[0],
        y: o[1],
        held: o[2] != 0,
        area: [o[3], o[4], o[5], o[6]],
        views: [o[7], o[8]],
    }
}

/// Shows or hides the frame's menu bar, keeping its accelerators (UI-DOCK-004).
pub fn show_menu_bar(frame: &Frame, show: bool) {
    unsafe { reviera_frame_show_menu_bar(frame.handle_ptr() as *mut c_void, c_int::from(show)) }
}

/// Shows or hides the frame's status bar.
pub fn show_status_bar(frame: &Frame, show: bool) {
    unsafe { reviera_frame_show_status_bar(frame.handle_ptr() as *mut c_void, c_int::from(show)) }
}

/// wxWidgets' own HTML window, filling a panel: the help where no web view is available.
#[derive(Debug, Clone, Copy)]
pub struct HtmlView(*mut c_void);

type LinkHandler = Box<dyn Fn(String)>;

extern "C" fn link_trampoline(user: *mut c_void, href: *const c_char) {
    // SAFETY: `user` is the handler leaked in `HtmlView::new`; `href` is a C string.
    let handler = unsafe { &*(user as *const LinkHandler) };
    let href = unsafe { CStr::from_ptr(href) }
        .to_string_lossy()
        .into_owned();
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

type TabHandler = Box<dyn Fn(bool)>;

extern "C" fn tab_trampoline(user: *mut c_void, shift: c_int) {
    // SAFETY: `user` is the handler leaked in `on_view_tab`.
    let f = unsafe { &*(user as *const TabHandler) };
    f(shift != 0);
}

/// Calls `f` (with Shift's state) for Tab pressed while a view has the focus.
pub fn on_view_tab(frame: &Frame, f: impl Fn(bool) + 'static) {
    let handler: Box<TabHandler> = Box::new(Box::new(f));
    let user = Box::into_raw(handler) as *mut c_void;
    unsafe { reviera_bind_tab(ptr(frame), tab_trampoline, user) }
}

/// Lets a view panel take Tab and arrow keys itself (not move the focus).
pub fn wants_chars(w: &impl WxWidget) {
    unsafe { reviera_wants_chars(ptr(w)) }
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
