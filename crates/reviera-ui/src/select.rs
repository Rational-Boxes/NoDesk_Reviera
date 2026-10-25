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

/// Whether the segment `a`–`b` crosses an edge of the box `lo`–`hi` (screen coordinates).
fn segment_crosses_box(a: (f32, f32), b: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let corners = [lo, (hi.0, lo.1), hi, (lo.0, hi.1)];
    let cross = |p: (f32, f32), q: (f32, f32), r: (f32, f32), s: (f32, f32)| {
        let d = |u: (f32, f32), v: (f32, f32), w: (f32, f32)| {
            (v.0 - u.0) * (w.1 - u.1) - (v.1 - u.1) * (w.0 - u.0)
        };
        (d(p, q, r) > 0.0) != (d(p, q, s) > 0.0) && (d(r, s, p) > 0.0) != (d(r, s, q) > 0.0)
    };
    (0..4).any(|i| cross(a, b, corners[i], corners[(i + 1) % 4]))
}

/// The quick menu's own items: a title, then the booleans by index into [`BOOLEANS`].
pub(crate) const ID_QUICK_TITLE: i32 = wxdragon::id::ID_HIGHEST + 100;
pub(crate) const ID_QUICK_FIRST: i32 = wxdragon::id::ID_HIGHEST + 101;
/// Toggles the selection's temporary translucency in the view.
pub(crate) const ID_QUICK_TRANSLUCENT: i32 = wxdragon::id::ID_HIGHEST + 110;

/// The boolean modes of `overlap.resolve` the quick menu offers.
pub(crate) const BOOLEANS: &[&str] = &["cut_a", "cut_b", "intersect", "union", "join", "accept"];

/// The groups of the quick menu: separated in the popup, tiers of the radial menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickGroup {
    Transform,
    Visibility,
    Element,
    Overlap,
}

/// One item of the quick menu: its label (with `&` mnemonics), the wx id it runs and its group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuickItem {
    pub label: String,
    pub id: i32,
    pub enabled: bool,
    pub group: QuickGroup,
}

impl QuickItem {
    fn new(label: &str, id: i32, group: QuickGroup) -> Self {
        Self {
            label: label.to_owned(),
            id,
            enabled: true,
            group,
        }
    }
}

/// The quick menu: the selection's name (empty over nothing) and the items.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuickMenu {
    pub title: String,
    pub items: Vec<QuickItem>,
}

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
        // In Edit Selection mode picks add to or remove from the set being edited.
        if self.sel_edit.is_some() {
            return self.sel_edit_pick(ids);
        }
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
        let mut ids = self.scene().boxed(
            &|p| camera.project(p, w, h),
            lo,
            hi,
            mode,
            &keep,
            &visibility,
        );
        // In plans, grid lines too (drawn as datums, not in the 3D scene): a window takes those
        // wholly inside it, a crossing those it touches (UI-SEL-002; At Grids needs them).
        if let (Some(z), Some(store)) = (
            v.plan.as_ref().map(|p| p.range.cut as f32),
            self.store_now(),
        ) {
            let inside = |q: (f32, f32)| q.0 >= lo.0 && q.0 <= hi.0 && q.1 >= lo.1 && q.1 <= hi.1;
            for g in reviera_core::grids::list(&store) {
                let end = |p: [f64; 2]| {
                    camera.project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
                };
                let (Some(a), Some(b)) = (end(g.curve.start), end(g.curve.end)) else {
                    continue;
                };
                let hit = match mode {
                    BoxMode::Window => inside(a) && inside(b),
                    BoxMode::Crossing => {
                        inside(a) || inside(b) || segment_crosses_box(a, b, lo, hi)
                    }
                };
                if hit && !ids.contains(&g.id) {
                    ids.push(g.id);
                }
            }
        }
        self.select_ids(&ids, combine);
        self.update_grips();
        let n = self.selection().len();
        self.frame
            .set_status_text(&format!("{n} element(s) selected"), 0);
    }

    /// The quick modify menu for a middle click at (`x`, `y`): `None` over nothing, with a tool
    /// running, or on a sheet.
    pub(crate) fn quick_menu(&mut self, x: i32, y: i32, panel: &Panel, add: bool) -> Option<Menu> {
        let q = self.quick_items(x, y, panel, add)?;
        let menu = Menu::builder().build();
        if !q.title.is_empty() {
            menu.append(ID_QUICK_TITLE, &q.title, "", ItemKind::Normal);
            menu.enable_item(ID_QUICK_TITLE, false);
        }
        let mut group = None;
        for item in &q.items {
            if group.is_some_and(|g| g != item.group) || (group.is_none() && !q.title.is_empty())
            {
                menu.append_separator();
            }
            group = Some(item.group);
            menu.append(item.id, &item.label, "", ItemKind::Normal);
            if !item.enabled {
                menu.enable_item(item.id, false);
            }
        }
        Some(menu)
    }

    /// The items of the quick modify menu for a middle click at (`x`, `y`), shared by the popup
    /// menu (UI-SEL-006) and the radial menu (UI-SEL-007): `None` over nothing, with a tool
    /// running, or on a sheet. Over nothing the title is empty.
    pub(crate) fn quick_items(
        &mut self,
        x: i32,
        y: i32,
        panel: &Panel,
        add: bool,
    ) -> Option<QuickMenu> {
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
            .or_else(|| self.space_label_at(x, y))
            .or_else(|| self.mep_legend_at(x, y))
            .or_else(|| self.element_at(x, y, panel))
        else {
            let id = self.ribbon.id_of("inspect.reset").filter(|_| temporary)?;
            return Some(QuickMenu {
                title: String::new(),
                items: vec![QuickItem::new("&Reset Hide/Isolate (show all)", id, QuickGroup::Visibility)],
            });
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
        let mut items = Vec::new();
        let mut item = |action: &str, label: &str, group: QuickGroup| {
            if let Some(id) = self.ribbon.id_of(action) {
                items.push(QuickItem::new(label, id, group));
            }
        };
        use QuickGroup::{Element, Transform, Visibility};
        item("element.move", "&Move", Transform);
        item("element.copy", "&Copy", Transform);
        item("element.rotate", "&Rotate", Transform);
        item("element.mirror_pick", "Mirror – &Pick Axis", Transform);
        item("element.mirror_draw", "Mirror – &Draw Axis", Transform);
        let all = |f: &dyn Fn(&reviera_core::Element) -> bool| {
            ids.iter().all(|i| store.get(*i).is_some_and(f))
        };
        if all(&|e| reviera_core::walls::wall_recipe(e).is_some()) {
            item("wall.flip", "&Flip Wall", Transform);
        }
        if all(&|e| reviera_core::inserts::kind_of(e).is_some()) {
            item("insert.flip_hand", "Flip &Hand", Transform);
            item("insert.flip_facing", "Flip &Facing", Transform);
        }
        if ids.len() == 1
            && all(&|e| reviera_core::mep_runs::segment_recipe(e).is_some_and(|s| matches!(s.domain, reviera_core::mep::Domain::Duct | reviera_core::mep::Domain::Pipe)))
        {
            item("ui.pane.sizing_calculator", "&Sizing Calculator", Transform);
        }
        item("inspect.hide", "&Hide in View", Visibility);
        let label = if self.all_translucent(&ids) {
            "Make &Opaque"
        } else {
            "Make &Translucent"
        };
        let mut late = vec![QuickItem::new(label, ID_QUICK_TRANSLUCENT, Visibility)];
        let mut item = |action: &str, label: &str, group: QuickGroup| {
            if let Some(id) = self.ribbon.id_of(action) {
                late.push(QuickItem::new(label, id, group));
            }
        };
        item("inspect.isolate", "&Isolate", Visibility);
        if temporary {
            item("inspect.reset", "&Reset Hide/Isolate (show all)", Visibility);
        }
        item("inspect.selection_box", "Section &Box", Visibility);
        item("component.create", "Create &Component…", Element);
        let pinned = |e: &reviera_core::Element| reviera_core::modify::is_pinned(e);
        if ids
            .iter()
            .any(|i| store.get(*i).is_some_and(|e| !pinned(e)))
        {
            item("element.pin", "Pi&n", Element);
        }
        if ids.iter().any(|i| store.get(*i).is_some_and(pinned)) {
            item("element.unpin", "&Unpin", Element);
        }
        item("element.delete", "&Delete", Element);
        items.extend(late);
        if let [a, b] = ids[..] {
            items.extend(self.quick_booleans(a, b));
        }
        Some(QuickMenu { title, items })
    }

    /// The booleans of the quick menu for two selected elements, when they overlap.
    fn quick_booleans(&mut self, a: ElementId, b: ElementId) -> Vec<QuickItem> {
        let found = self.run(
            "overlap.find",
            json!({ "element_ids": [a], "against": [b], "include_accepted": true }),
        );
        let overlap: Option<Value> = found
            .ok()
            .and_then(|v| v["overlaps"].as_array()?.first().cloned());
        let heading = ID_QUICK_FIRST + BOOLEANS.len() as i32;
        let Some(o) = overlap else {
            let mut none = QuickItem::new("No overlap to combine", heading, QuickGroup::Overlap);
            none.enabled = false;
            return vec![none];
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
        let mut title = QuickItem::new(&format!("Overlap of {amount}:"), heading, QuickGroup::Overlap);
        title.enabled = false;
        let mut out = vec![title];
        for (k, label) in labels.iter().enumerate() {
            out.push(QuickItem::new(label, ID_QUICK_FIRST + k as i32, QuickGroup::Overlap));
        }
        self.quick_pair = Some(QuickPair {
            a,
            b,
            a_label,
            b_label,
        });
        out
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
