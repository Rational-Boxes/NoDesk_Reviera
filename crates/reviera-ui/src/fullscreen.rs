// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Full-screen and focus mode (UI-DOCK-004).
//!
//! - **F11** makes the window full-screen and hides all chrome (menu bar, Quick Access Toolbar,
//!   ribbon, panes, status bar): only the views remain. **Ctrl+Shift+F11** is the same without
//!   full-screen (focus mode). Pressing the key again, Esc with nothing selected and no tool, or
//!   **Exit Full Screen** restores the layout exactly as it was.
//! - Pushing the pointer against the **top** edge for the dwell (150 ms) reveals the menu bar,
//!   the ribbon and an Exit Full Screen button; it hides again 600 ms after the pointer is back
//!   down in the view, on a click in the view, or when a tool starts from it.
//! - Pushing against the **bottom** edge reveals the panes that were shown before, docked along
//!   the bottom, with the status bar; a click in the view or Esc dismisses them.
//! - Reveals never start while a mouse button is held or the radial menu is open.
//!
//! The reveals dock the chrome rather than float it over the view: native panes cannot be laid
//! over the GPU surface on every platform (a Wayland compositor places top-level windows itself),
//! so the view makes room while they show.

use std::time::{Duration, Instant};

use wxdragon::prelude::*;

use super::Gui;
use crate::wx;

/// The panes the bottom reveal brings back (when they were shown), and the toolbars of the top.
pub(crate) const PANES: &[&str] = &[
    "properties",
    "browser",
    "model_tree",
    "components",
    "clash",
    "analytical_checks",
    "results",
];
const TOP: &[&str] = &["quick_access", "ribbon", "fullscreen_exit"];

/// How long the top reveal stays after the pointer is back in the view.
const LINGER: Duration = Duration::from_millis(600);
/// How often the pointer is polled while the mode is on, milliseconds.
const POLL: i32 = 40;
/// The pointer is "at" an edge within this many pixels.
const EDGE: i32 = 2;
/// Within this many pixels below the views' top, the pointer is still on its way to or from the
/// revealed ribbon.
const TOP_BAND: i32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    #[default]
    Off,
    /// Chrome hidden, window as it was.
    Focus,
    /// Chrome hidden and the window full-screen.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    Top,
    Bottom,
}

#[derive(Default)]
pub(crate) struct Chrome {
    pub mode: Mode,
    /// The layout before, restored on leaving.
    saved: Option<String>,
    /// The panes shown before.
    shown: Vec<&'static str>,
    pub revealed: Option<Edge>,
    /// The pointer arrived at an edge, and when.
    pending: Option<(Edge, Instant)>,
    /// When the pointer went back down into the view under a top reveal.
    leaving: Option<Instant>,
    /// The left click that dismissed a reveal: its release is swallowed too.
    pub swallow_up: bool,
    timer: Option<Timer<Frame>>,
    /// The Exit Full Screen toolbar, made on first use.
    exit_bar: Option<AuiToolBar>,
}

/// `perspective` with `panes` docked along the bottom in one row, `height` pixels tall.
pub(crate) fn to_bottom(perspective: &str, panes: &[&str], height: i32) -> String {
    let mut pos = 0;
    let mut parts: Vec<String> = perspective
        .split('|')
        .filter(|p| !p.starts_with("dock_size(3,1,"))
        .map(|part| {
            let name = part
                .split(';')
                .find_map(|f| f.strip_prefix("name="))
                .unwrap_or_default();
            if !panes.contains(&name) {
                return part.to_owned();
            }
            let fields: Vec<String> = part
                .split(';')
                .map(|f| match f.split_once('=') {
                    Some(("dir", _)) => "dir=3".to_owned(),
                    Some(("layer", _)) => "layer=1".to_owned(),
                    Some(("row", _)) => "row=0".to_owned(),
                    Some(("pos", _)) => {
                        pos += 1;
                        format!("pos={}", pos - 1)
                    }
                    Some(("besth", _)) => format!("besth={height}"),
                    _ => f.to_owned(),
                })
                .collect();
            fields.join(";")
        })
        .collect();
    // The dock sizes come after the panes; the final part is empty (a trailing `|`).
    let at = parts.iter().rposition(|p| !p.is_empty()).map_or(parts.len(), |k| k + 1);
    parts.insert(at, format!("dock_size(3,1,0)={height}"));
    parts.join("|")
}

impl Gui {
    /// F11 (`full`) or Ctrl+Shift+F11: enters that mode, leaves it, or switches between the two.
    pub(crate) fn toggle_chrome(&mut self, full: bool) {
        let want = if full { Mode::Full } else { Mode::Focus };
        match self.chrome.mode {
            Mode::Off => self.enter_chrome(want),
            m if m == want => self.leave_chrome(),
            _ => {
                // Between focus mode and full-screen only the window changes.
                self.frame.show_full_screen(full);
                self.chrome.mode = want;
            }
        }
    }

    fn enter_chrome(&mut self, mode: Mode) {
        self.chrome.saved = Some(self.aui.save_perspective());
        self.chrome.shown = PANES
            .iter()
            .copied()
            .filter(|p| self.aui.is_pane_shown(p))
            .collect();
        self.chrome.mode = mode;
        self.chrome.revealed = None;
        self.hide_all_chrome();
        if mode == Mode::Full {
            self.frame.show_full_screen(true);
        }
        self.frame
            .set_status_text("Push the pointer to the top for the menus, to the bottom for the panes", 0);
        self.ensure_chrome_timer();
        if let Some(t) = &self.chrome.timer {
            t.start(POLL, false);
        }
    }

    /// Leaves full-screen or focus mode, restoring the layout as it was.
    pub(crate) fn leave_chrome(&mut self) {
        if self.chrome.mode == Mode::Off {
            return;
        }
        if self.chrome.mode == Mode::Full {
            self.frame.show_full_screen(false);
        }
        self.chrome.mode = Mode::Off;
        self.chrome.revealed = None;
        self.chrome.pending = None;
        self.chrome.leaving = None;
        wx::show_menu_bar(&self.frame, true);
        wx::show_status_bar(&self.frame, true);
        if let Some(p) = self.chrome.saved.take() {
            self.aui.load_perspective(&p, false);
        }
        self.aui.set_pane_shown("fullscreen_exit", false);
        self.aui.update();
        self.frame.set_status_text("Ready", 0);
    }

    fn hide_all_chrome(&mut self) {
        for p in PANES.iter().chain(TOP) {
            self.aui.set_pane_shown(p, false);
        }
        wx::show_menu_bar(&self.frame, false);
        wx::show_status_bar(&self.frame, false);
        self.aui.update();
    }

    /// Shows a reveal, or hides it (`None`).
    pub(crate) fn reveal(&mut self, edge: Option<Edge>) {
        if self.chrome.mode == Mode::Off || self.chrome.revealed == edge {
            return;
        }
        self.chrome.pending = None;
        self.chrome.leaving = None;
        if self.chrome.revealed.is_some() {
            self.hide_all_chrome();
        }
        self.chrome.revealed = edge;
        match edge {
            None => {}
            Some(Edge::Top) => {
                self.ensure_exit_bar();
                wx::show_menu_bar(&self.frame, true);
                for p in TOP {
                    self.aui.set_pane_shown(p, true);
                }
                self.aui.update();
            }
            Some(Edge::Bottom) => {
                let height = (self.frame.get_client_size().height as f32 * 0.4) as i32;
                if let Some(saved) = &self.chrome.saved {
                    let p = to_bottom(saved, &self.chrome.shown, height.max(120));
                    self.aui.load_perspective(&p, false);
                }
                for p in PANES.iter().chain(TOP) {
                    let show = self.chrome.shown.contains(p);
                    self.aui.set_pane_shown(p, show);
                }
                wx::show_status_bar(&self.frame, true);
                self.aui.update();
            }
        }
    }

    /// A left press in a view: dismisses a reveal (and swallows the click); true when it did.
    pub(crate) fn chrome_press(&mut self) -> bool {
        if self.chrome.revealed.is_none() {
            return false;
        }
        self.reveal(None);
        self.chrome.swallow_up = true;
        true
    }

    /// Polls the pointer against the edges while the mode is on: the dwell before a reveal and
    /// the linger after the pointer goes back down into the views. Polling (rather than the
    /// views' mouse events) sees the pointer over the view tabs and the revealed ribbon too.
    fn chrome_tick(&mut self) {
        if self.chrome.mode == Mode::Off {
            if let Some(t) = &self.chrome.timer {
                t.stop();
            }
            return;
        }
        let p = wx::pointer_state(&self.frame, &self.notebook);
        let [_, top, _, bottom] = p.area;
        if p.held || self.radial.is_some() {
            self.chrome.pending = None;
            return;
        }
        let edge = if p.y <= top + EDGE {
            Some(Edge::Top)
        } else if p.y >= bottom - EDGE {
            Some(Edge::Bottom)
        } else {
            None
        };
        match edge {
            Some(e) if self.chrome.revealed != Some(e) => match self.chrome.pending {
                Some((pe, since)) if pe == e => {
                    if since.elapsed() >= self.ui_settings.dwell() {
                        self.reveal(Some(e));
                    }
                }
                _ => self.chrome.pending = Some((e, Instant::now())),
            },
            _ => self.chrome.pending = None,
        }
        // The top reveal lingers while the pointer is over it, and goes once the pointer has
        // been down in the views for a moment.
        if self.chrome.revealed == Some(Edge::Top) {
            if p.y > p.views[0] + TOP_BAND {
                let since = *self.chrome.leaving.get_or_insert_with(Instant::now);
                if since.elapsed() >= LINGER {
                    self.reveal(None);
                }
            } else {
                self.chrome.leaving = None;
            }
        }
    }

    fn ensure_chrome_timer(&mut self) {
        if self.chrome.timer.is_some() {
            return;
        }
        let timer = Timer::new(&self.frame);
        let this = self.this.clone();
        timer.on_tick(move |_| {
            if let Some(gui) = this.upgrade()
                && let Ok(mut g) = gui.try_borrow_mut()
            {
                g.chrome_tick();
            }
        });
        self.chrome.timer = Some(timer);
    }

    /// The Exit Full Screen button of the top reveal: a toolbar at the right of the ribbon row.
    fn ensure_exit_bar(&mut self) {
        if self.chrome.exit_bar.is_some() {
            return;
        }
        let Some(id) = self.ribbon.id_of("ui.fullscreen.toggle") else {
            return;
        };
        let tb = AuiToolBar::builder(&self.frame)
            .with_style(AuiToolBarStyle::Text | AuiToolBarStyle::HorzLayout)
            .build();
        tb.set_tool_bitmap_size(Size::new(16, 16));
        tb.add_tool(id, "Exit Full Screen", "Leave full-screen or focus mode (F11)", WidgetItemKind::Normal);
        crate::ribbon::set_tool_icon(&tb, id, "full_screen");
        tb.set_overflow_visible(false);
        tb.realize();
        self.aui.add_pane_with_info(
            &tb,
            AuiPaneInfo::new()
                .with_name("fullscreen_exit")
                .toolbar_pane()
                .top()
                .layer(2)
                .row(0)
                .position(1)
                .gripper(false)
                .floatable(false)
                .dockable(false)
                .movable(false),
        );
        self.chrome.exit_bar = Some(tb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_move_to_the_bottom_row() {
        let p = "layout2|name=views;caption=;state=768;dir=5;layer=0;row=0;pos=0;prop=100000;bestw=20;besth=20;minw=-1;minh=-1;maxw=-1;maxh=-1;floatx=-1;floaty=-1;floatw=-1;floath=-1|\
name=properties;caption=Properties;state=2044;dir=4;layer=1;row=0;pos=0;prop=100000;bestw=340;besth=600;minw=220;minh=200;maxw=-1;maxh=-1;floatx=-1;floaty=-1;floatw=-1;floath=-1|\
name=browser;caption=Project Browser;state=2044;dir=4;layer=1;row=0;pos=1;prop=100000;bestw=340;besth=260;minw=220;minh=120;maxw=-1;maxh=-1;floatx=-1;floaty=-1;floatw=-1;floath=-1|\
dock_size(5,0,0)=22|dock_size(4,1,0)=342|";
        let b = to_bottom(p, &["properties", "browser"], 300);
        let pane = |n: &str| {
            b.split('|')
                .find(|s| s.contains(&format!("name={n};")))
                .unwrap()
                .to_owned()
        };
        assert!(pane("properties").contains(";dir=3;") && pane("properties").contains(";pos=0;"));
        assert!(pane("browser").contains(";dir=3;") && pane("browser").contains(";pos=1;"));
        assert!(pane("browser").contains(";besth=300;"));
        assert!(pane("views").contains(";dir=5;"));
        assert!(b.contains("|dock_size(3,1,0)=300|"), "{b}");
        assert!(b.ends_with('|'));
    }
}
