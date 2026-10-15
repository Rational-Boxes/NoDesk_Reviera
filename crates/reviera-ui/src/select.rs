// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Selecting several elements (UI-SEL-002) and the quick modify menu (UI-SEL-006).
//!
//! - **Ctrl+click** adds the element under the cursor to the selection; **Shift+click** removes
//!   it.
//! - **Box selection**: dragged left to right, a *window* (solid) selects the elements entirely
//!   inside; right to left, a *crossing* (dashed) those inside or touching. In plans a left drag
//!   that starts on nothing selected boxes; in 3D views, where a left drag orbits, a left drag
//!   with Ctrl or Shift held boxes. Ctrl adds the boxed set and Shift removes it. Only what the
//!   view shows is picked (its view range, section box, slices and hidden elements).
//! - A **middle click** (pressed and released without moving; a middle drag still pans) on an
//!   element opens the quick modify menu for the selection, selecting the element first when it
//!   is not selected (Ctrl keeps the rest). Besides hiding, it toggles the selection's temporary
//!   translucency in the view, and resets the view's temporary hiding and isolation (also from a
//!   middle click on empty space, since hidden elements cannot be clicked). With exactly two elements selected, the menu checks
//!   whether they overlap (`overlap.find`) and offers the booleans (`overlap.resolve`).

use reviera_core::ElementId;
use reviera_render::plan::Lines;
use reviera_render::{BoxMode, RenderBackend};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

/// The quick menu's own items: a title, then the booleans by index into [`BOOLEANS`].
pub(crate) const ID_QUICK_TITLE: i32 = wxdragon::id::ID_HIGHEST + 100;
pub(crate) const ID_QUICK_FIRST: i32 = wxdragon::id::ID_HIGHEST + 101;
/// Toggles the selection's temporary translucency in the view.
pub(crate) const ID_QUICK_TRANSLUCENT: i32 = wxdragon::id::ID_HIGHEST + 110;

/// The boolean modes of `overlap.resolve` the quick menu offers.
pub(crate) const BOOLEANS: &[&str] = &["cut_a", "cut_b", "intersect", "union", "join", "accept"];

/// How a pick changes the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Combine {
    Replace,
    Add,
    Remove,
}

impl Combine {
    /// Ctrl adds, Shift removes (UI-SEL-002).
    pub(crate) fn of(ctrl: bool, shift: bool) -> Self {
        match (ctrl, shift) {
            (true, _) => Combine::Add,
            (false, true) => Combine::Remove,
            _ => Combine::Replace,
        }
    }
}

/// Two selected elements that overlap, for the quick menu's booleans.
#[derive(Debug, Clone)]
pub(crate) struct QuickPair {
    pub a: ElementId,
    pub b: ElementId,
    pub a_label: String,
    pub b_label: String,
}

impl Gui {
    /// Changes the selection by `ids`.
    pub(crate) fn select_ids(&mut self, ids: &[ElementId], combine: Combine) {
        let result = match combine {
            Combine::Replace if ids.is_empty() => self.run("selection.clear", json!({})),
            Combine::Replace => self.run("selection.set", json!({ "ids": ids })),
            _ if ids.is_empty() => return,
            Combine::Add => self.run("selection.add", json!({ "element_ids": ids })),
            Combine::Remove => self.run("selection.remove", json!({ "element_ids": ids })),
        };
        if result.is_ok() {
            self.selection_changed();
        }
    }

    /// A world point just in front of the eye under pixel (`x`, `y`), so lines between such
    /// points draw on the screen over the model.
    /// In plans, on the cut plane (the view range clips anything above it).
    fn screen_point(&self, (x, y): (f32, f32), size: (f32, f32)) -> [f32; 3] {
        let (origin, dir) = self.camera().ray(x, y, size.0, size.1);
        let cut = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.range.cut as f32 + 0.01);
        match cut {
            Some(z) if dir.z.abs() > 1e-6 => (origin + dir * ((z - origin.z) / dir.z)).to_array(),
            _ => (origin + dir * 0.01).to_array(),
        }
    }

    /// Draws the selection box from (`x0`, `y0`) to (`x`, `y`): solid for a window, dashed for
    /// a crossing.
    pub(crate) fn box_motion(&mut self, (x0, y0): (i32, i32), (x, y): (i32, i32), panel: &Panel) {
        let s = panel.get_client_size();
        let size = (s.width as f32, s.height as f32);
        let crossing = x < x0;
        let (x0, y0, x, y) = (x0 as f32, y0 as f32, x as f32, y as f32);
        let corners = [(x0, y0), (x, y0), (x, y), (x0, y)];
        let mut segments = Vec::new();
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            if !crossing {
                segments.push([self.screen_point(a, size), self.screen_point(b, size)]);
                continue;
            }
            // Dashes 6 px long, 4 px apart.
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let mut t = 0.0;
            while t < len {
                let u = (t + 6.0).min(len);
                let at = |d: f32| {
                    let k = if len > 0.0 { d / len } else { 0.0 };
                    (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k)
                };
                segments.push([
                    self.screen_point(at(t), size),
                    self.screen_point(at(u), size),
                ]);
                t += 10.0;
            }
        }
        let lines = Lines {
            color: reviera_render::HIGHLIGHT,
            width: 0.3,
            segments,
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Selects by the box from (`x0`, `y0`) to (`x`, `y`) (UI-SEL-002).
    pub(crate) fn box_select(
        &mut self,
        (x0, y0): (i32, i32),
        (x, y): (i32, i32),
        combine: Combine,
        panel: &Panel,
    ) {
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        let Some(v) = self.active_view() else { return };
        let s = panel.get_client_size();
        let (w, h) = (s.width as f32, s.height as f32);
        let mode = if x >= x0 {
            BoxMode::Window
        } else {
            BoxMode::Crossing
        };
        let lo = (x0.min(x) as f32, y0.min(y) as f32);
        let hi = (x0.max(x) as f32, y0.max(y) as f32);
        let (bottom, top) = v.plan.as_ref().map_or(
            (f32::NEG_INFINITY, f32::INFINITY),
            super::plans::PlanState::drawn,
        );
        let (section_box, slices) = self.view_clip(v);
        let clipping = reviera_render::Clipping::of(section_box.as_ref(), &slices);
        let keep = |p: glam::Vec3| (bottom..=top).contains(&p.z) && clipping.keeps(p);
        let visibility = self.view_visibility(v);
        let camera = *self.camera();
        let ids = self.scene().boxed(
            &|p| camera.project(p, w, h),
            lo,
            hi,
            mode,
            &keep,
            &visibility,
        );
        self.select_ids(&ids, combine);
        self.update_grips();
        let n = self.selection().len();
        self.frame
            .set_status_text(&format!("{n} element(s) selected"), 0);
    }

    /// The quick modify menu for a middle click at (`x`, `y`): `None` over nothing, with a tool
    /// running, or on a sheet.
    pub(crate) fn quick_menu(&mut self, x: i32, y: i32, panel: &Panel, add: bool) -> Option<Menu> {
        self.quick_pair = None;
        if self.doc.is_none() || self.any_tool() || self.in_sheet() {
            return None;
        }
        // Over nothing, only the reset of temporary hiding and isolation (when there is any):
        // hidden elements cannot be clicked.
        let temporary = self
            .active_view()
            .is_some_and(|v| !self.view_visibility(v).is_empty());
        let Some(hit) = self
            .room_label_at(x, y)
            .or_else(|| self.element_at(x, y, panel))
        else {
            let id = self.ribbon.id_of("inspect.reset").filter(|_| temporary)?;
            let menu = Menu::builder().build();
            menu.append(id, "&Reset Hide/Isolate (show all)", "", ItemKind::Normal);
            return Some(menu);
        };
        let mut ids = self.selection();
        if !ids.contains(&hit) {
            let combine = if add { Combine::Add } else { Combine::Replace };
            self.select_ids(&[hit], combine);
            ids = self.selection();
        }
        let store = self.store_now()?;
        let title = if ids.len() == 1 {
            self.element_label(&json!(ids[0]))
        } else {
            format!("{} elements selected", ids.len())
        };
        let menu = Menu::builder().build();
        menu.append(ID_QUICK_TITLE, &title, "", ItemKind::Normal);
        menu.enable_item(ID_QUICK_TITLE, false);
        menu.append_separator();
        let item = |menu: &Menu, action: &str, label: &str| {
            if let Some(id) = self.ribbon.id_of(action) {
                menu.append(id, label, "", ItemKind::Normal);
            }
        };
        item(&menu, "element.move", "&Move");
        item(&menu, "element.copy", "&Copy");
        item(&menu, "element.rotate", "&Rotate");
        item(&menu, "element.mirror_pick", "Mirror – &Pick Axis");
        item(&menu, "element.mirror_draw", "Mirror – &Draw Axis");
        let all = |f: &dyn Fn(&reviera_core::Element) -> bool| {
            ids.iter().all(|i| store.get(*i).is_some_and(f))
        };
        if all(&|e| reviera_core::walls::wall_recipe(e).is_some()) {
            item(&menu, "wall.flip", "&Flip Wall");
        }
        if all(&|e| reviera_core::inserts::kind_of(e).is_some()) {
            item(&menu, "insert.flip_hand", "Flip &Hand");
            item(&menu, "insert.flip_facing", "Flip &Facing");
        }
        menu.append_separator();
        item(&menu, "inspect.hide", "&Hide in View");
        let label = if self.all_translucent(&ids) {
            "Make &Opaque"
        } else {
            "Make &Translucent"
        };
        menu.append(ID_QUICK_TRANSLUCENT, label, "", ItemKind::Normal);
        item(&menu, "inspect.isolate", "&Isolate");
        if temporary {
            item(&menu, "inspect.reset", "&Reset Hide/Isolate (show all)");
        }
        item(&menu, "inspect.selection_box", "Section &Box");
        item(&menu, "component.create", "Create &Component…");
        let pinned = |e: &reviera_core::Element| reviera_core::modify::is_pinned(e);
        if ids
            .iter()
            .any(|i| store.get(*i).is_some_and(|e| !pinned(e)))
        {
            item(&menu, "element.pin", "Pi&n");
        }
        if ids.iter().any(|i| store.get(*i).is_some_and(pinned)) {
            item(&menu, "element.unpin", "&Unpin");
        }
        item(&menu, "element.delete", "&Delete");
        if let [a, b] = ids[..] {
            self.quick_booleans(&menu, a, b);
        }
        Some(menu)
    }

    /// The booleans of the quick menu for two selected elements, when they overlap.
    fn quick_booleans(&mut self, menu: &Menu, a: ElementId, b: ElementId) {
        menu.append_separator();
        let found = self.run(
            "overlap.find",
            json!({ "element_ids": [a], "against": [b], "include_accepted": true }),
        );
        let overlap: Option<Value> = found
            .ok()
            .and_then(|v| v["overlaps"].as_array()?.first().cloned());
        let Some(o) = overlap else {
            let id = ID_QUICK_FIRST + BOOLEANS.len() as i32;
            menu.append(id, "No overlap to combine", "", ItemKind::Normal);
            menu.enable_item(id, false);
            return;
        };
        let (a_label, b_label) = (self.element_label(&json!(a)), self.element_label(&json!(b)));
        let volume = o["volume"].as_f64().unwrap_or(0.0);
        let amount = if volume < 0.01 {
            format!("{:.2} L", volume * 1000.0)
        } else {
            self.format_value(volume, "volume")
        };
        let labels = [
            format!("Cut {a_label} by {b_label}"),
            format!("Cut {b_label} by {a_label}"),
            "Intersect (keep the shared part)".to_owned(),
            format!("Union (fuse {b_label} into {a_label})"),
            "Join (by precedence)".to_owned(),
            "Accept the Overlap".to_owned(),
        ];
        let heading = ID_QUICK_FIRST + BOOLEANS.len() as i32;
        menu.append(
            heading,
            &format!("Overlap of {amount}:"),
            "",
            ItemKind::Normal,
        );
        menu.enable_item(heading, false);
        for (k, label) in labels.iter().enumerate() {
            menu.append(ID_QUICK_FIRST + k as i32, label, "", ItemKind::Normal);
        }
        self.quick_pair = Some(QuickPair {
            a,
            b,
            a_label,
            b_label,
        });
    }

    /// Whether every one of `ids` is drawn translucent in the active view for now.
    fn all_translucent(&self, ids: &[ElementId]) -> bool {
        let Some(v) = self.active_view() else {
            return false;
        };
        let state = self.view_visibility(v);
        !ids.is_empty() && ids.iter().all(|i| state.translucent.contains_key(i))
    }

    /// Makes the selection translucent in the view for now, or opaque again when it all is.
    pub(crate) fn quick_translucent(&mut self) {
        let ids = self.selection();
        let command = if self.all_translucent(&ids) {
            "view.temp_show"
        } else {
            "view.temp_translucent"
        };
        self.temp(command, &ids, json!({ "by": "element" }));
    }

    /// Runs a boolean of the quick menu.
    pub(crate) fn quick_command(&mut self, id: i32) {
        let Some(p) = self.quick_pair.take() else {
            return;
        };
        let Some(mode) = usize::try_from(id - ID_QUICK_FIRST)
            .ok()
            .and_then(|k| BOOLEANS.get(k))
        else {
            return;
        };
        self.resolve_overlap(&json!(p.a), &json!(p.b), &p.a_label, &p.b_label, mode);
        self.selection_changed();
    }
}
