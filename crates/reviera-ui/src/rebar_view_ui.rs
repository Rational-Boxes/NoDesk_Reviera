// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar in views (STR-RBR-017) and reinforcement numbers (STR-RBR-016).
//!
//! Each view draws its rebar as its states say: tubes (3D views at Fine detail, as solid) or
//! centrelines, the bars its presentation shows with the set's extent line, and unobscured
//! rebar through its host. Clicking near a drawn bar selects its set. The contextual tab of a
//! selected set offers Show All, First/Last, Middle and Select Bars (click bars, Enter
//! finishes), and View Visibility States (a grid of the views; Solid only for 3D views).
//! *Concrete › Settings › Reinforcement Numbers* lists a partition's sequence to renumber,
//! close gaps, change or lock numbers, or select the elements of a number.

use glam::Vec3;
use reviera_core::ElementId;
use reviera_core::rebar_views::{self, Mode};
use reviera_render::plan::Lines;
use reviera_render::{RenderBackend, SceneMesh, Visibility};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const PICKED: [f32; 4] = [0.95, 0.45, 0.0, 1.0];

/// The bar segments a view draws, by set, for picking.
pub(crate) type RebarPicks = Vec<(ElementId, [[f32; 3]; 2])>;

/// What clicking a bar does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarPurpose {
    /// Select Bars: pick bars to show, Enter finishes.
    Present,
    /// Remove Bar, Split Set and Move Bar act on the bar clicked (STR-RBR-006).
    Remove,
    Split,
    Move,
}

/// A bar pick in progress: the set, the view, its bars (with their indices before edits) and
/// the picks.
#[derive(Debug, Clone)]
pub struct BarPick {
    pub purpose: BarPurpose,
    pub set: ElementId,
    pub view: ElementId,
    pub bars: Vec<Vec<[f64; 3]>>,
    pub indices: Vec<usize>,
    pub picked: Vec<usize>,
}

fn f(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

impl Gui {
    /// What view `index` draws of the rebar: bodies it replaces are hidden, unobscured ones drawn
    /// through, partial solids added; returns the centreline and extent lines (normal and on
    /// top) and the segments a click can pick.
    pub(crate) fn rebar_view_drawing(&self, index: usize, vis: &mut Visibility) -> (Vec<Lines>, Vec<Lines>, RebarPicks) {
        let v = &self.views[index];
        // A sheet or schedule draws no model rebar of its own.
        if v.sheet.is_some() || v.schedule.is_some() {
            return Default::default();
        }
        let Some(doc) = self.app.document(v.doc) else { return Default::default() };
        let store = doc.doc.store();
        let view = v.element.and_then(|e| store.get(e));
        let width = if v.plan.is_some() { 0.35 } else { 0.08 };
        // Bars in a dark rebar red, the extent line in dark grey.
        let color = [0.62, 0.08, 0.06, 1.0];
        let ext = [0.2, 0.2, 0.2, 1.0];
        let solid_color = reviera_render::class_color("IfcReinforcingBar");
        let (mut lines, mut top) = (Lines { color, width, segments: Vec::new() }, Lines { color, width, segments: Vec::new() });
        let (mut extent, mut extent_top) = (Lines { color: ext, width, segments: Vec::new() }, Lines { color: ext, width, segments: Vec::new() });
        let mut picks = Vec::new();
        for e in store.iter().filter(|e| rebar_views::is_reinforcement(e)) {
            let Some(d) = rebar_views::drawn(store, view, e) else { continue };
            if d.unobscured {
                vis.xray.insert(d.element);
            }
            if d.own_body {
                continue;
            }
            vis.hidden.insert(d.element);
            if !d.tubes.is_empty() {
                let (points, indices) = rebar_views::tubes_mesh(&d.tubes);
                vis.extra.push(SceneMesh {
                    element: Some(d.element),
                    class: "IfcReinforcingBar".into(),
                    positions: points.iter().map(|p| f(*p)).collect(),
                    indices,
                    color: solid_color,
                });
                for (_, b) in &d.tubes {
                    picks.extend(b.points.windows(2).map(|w| (d.element, [f(w[0]), f(w[1])])));
                }
            }
            let segs: Vec<[[f32; 3]; 2]> = d.centrelines.iter().map(|s| [f(s[0]), f(s[1])]).collect();
            picks.extend(segs.iter().map(|s| (d.element, *s)));
            let ex: Vec<[[f32; 3]; 2]> = d.extent.iter().map(|s| [f(s[0]), f(s[1])]).collect();
            if d.unobscured {
                top.segments.extend(segs);
                extent_top.segments.extend(ex);
            } else {
                lines.segments.extend(segs);
                extent.segments.extend(ex);
            }
        }
        (vec![lines, extent], vec![top, extent_top], picks)
    }

    /// The rebar set whose drawn bar is within a few pixels of (`x`, `y`) in the active view.
    pub(crate) fn rebar_line_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let (x, y) = (x as f32, y as f32);
        v.rebar_picks
            .iter()
            .filter_map(|(id, s)| {
                let a = v.camera.project(Vec3::from(s[0]), w, h)?;
                let b = v.camera.project(Vec3::from(s[1]), w, h)?;
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                let d = (x - (a.0 + dx * t)).hypot(y - (a.1 + dy * t));
                (d < 5.0).then_some((*id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The view a rebar view command applies to: the active one (an unsaved `{3D}` is saved
    /// first).
    fn rebar_view_target(&mut self) -> Option<ElementId> {
        let frame = self.frame;
        let v = self.active_view()?;
        if v.schedule.is_some() || v.sheet.is_some() {
            frame.set_status_text("Rebar presentation is set in a model view", 0);
            return None;
        }
        if v.element.is_none() {
            self.default_3d_view();
        }
        self.active_view().and_then(|v| v.element)
    }

    pub(crate) fn selected_reinforcement(&mut self, sets_only: bool) -> Vec<ElementId> {
        let Some(store) = self.store_now() else { return Vec::new() };
        self.selection()
            .into_iter()
            .filter(|id| {
                store.get(*id).is_some_and(|e| if sets_only { reviera_core::rebar::rebar_recipe(e).is_some() } else { rebar_views::is_reinforcement(e) })
            })
            .collect()
    }

    /// Show All, First/Last, Middle or Select Bars for the selected sets in the active view.
    pub(crate) fn rebar_presentation(&mut self, mode: Mode) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Presentation: select rebar sets first", 0);
        }
        let Some(view) = self.rebar_view_target() else { return };
        if mode == Mode::Select {
            let [set] = sets[..] else {
                return frame.set_status_text("Select Bars: select one rebar set", 0);
            };
            let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [set] })) else { return };
            let bars: Vec<Vec<[f64; 3]>> = i[0]["bars"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|b| serde_json::from_value(b["points"].clone()).unwrap_or_default())
                .collect();
            let indices: Vec<usize> = serde_json::from_value(i[0]["bar_indices"].clone()).unwrap_or_default();
            let end_tool = self.frame;
            self.end_tool(&end_tool);
            self.bar_pick = Some(BarPick { purpose: BarPurpose::Present, set, view, bars, indices, picked: Vec::new() });
            self.bar_pick_preview();
            return frame.set_status_text("Select Bars: click the bars to show (again to drop one); Enter finishes, Esc cancels", 0);
        }
        match self.run("rebar.set_presentation", json!({ "rebar_ids": sets, "view_id": view, "mode": mode })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} set(s) shown {} in this view", sets.len(), match mode {
                    Mode::All => "with every bar",
                    Mode::FirstLast => "by their first and last bars",
                    _ => "by their middle bar",
                }), 0);
            }
            Err(e) => error(&frame, &format!("The presentation was not changed:\n{e}")),
        }
    }

    pub(crate) fn end_bar_pick(&mut self) -> bool {
        if self.bar_pick.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    fn bar_pick_preview(&mut self) {
        let Some(p) = &self.bar_pick else { return };
        let plan = self.active_view().is_some_and(|v| v.plan.is_some());
        let width = if plan { 0.6 } else { 0.12 };
        let segments: Vec<[[f32; 3]; 2]> = p
            .picked
            .iter()
            .filter_map(|i| p.bars.get(*i))
            .flat_map(|b| b.windows(2).map(|w| [f(w[0]), f(w[1])]).collect::<Vec<_>>())
            .collect();
        let lines = vec![Lines { color: PICKED, width, segments }];
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&lines);
        }
        if let Some(v) = self.active_view() {
            v.panel.refresh(false, None);
        }
    }

    pub(crate) fn bar_pick_click(&mut self, x: i32, y: i32) {
        let Some(v) = self.active_view() else { return };
        let Some(p) = &self.bar_pick else { return };
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let (cx, cy) = (x as f32, y as f32);
        let near = p
            .bars
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                b.windows(2)
                    .filter_map(|s| {
                        let a = v.camera.project(Vec3::from(f(s[0])), w, h)?;
                        let c = v.camera.project(Vec3::from(f(s[1])), w, h)?;
                        let (dx, dy) = (c.0 - a.0, c.1 - a.1);
                        let t = (((cx - a.0) * dx + (cy - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                        Some((cx - (a.0 + dx * t)).hypot(cy - (a.1 + dy * t)))
                    })
                    .min_by(f32::total_cmp)
                    .filter(|d| *d < 8.0)
                    .map(|d| (i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        let frame = self.frame;
        let Some(i) = near else {
            return frame.set_status_text("Click on a bar of the set", 0);
        };
        if self.bar_pick.as_ref().is_some_and(|p| p.purpose != BarPurpose::Present) {
            return self.bar_edit(i);
        }
        if let Some(p) = &mut self.bar_pick {
            match p.picked.iter().position(|k| *k == i) {
                Some(k) => {
                    p.picked.remove(k);
                }
                None => p.picked.push(i),
            }
            frame.set_status_text(&format!("Select Bars: {} bar(s) picked; Enter finishes", p.picked.len()), 0);
        }
        self.bar_pick_preview();
    }

    /// Enter finishes Select Bars.
    pub(crate) fn bar_pick_key(&mut self, key: i32) -> bool {
        if self.bar_pick.as_ref().is_none_or(|p| p.purpose != BarPurpose::Present) || !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            return false;
        }
        let frame = self.frame;
        let p = self.bar_pick.clone().expect("checked");
        self.end_bar_pick();
        if p.picked.is_empty() {
            frame.set_status_text("Select Bars: no bars picked, nothing changed", 0);
            return true;
        }
        let mut picked = p.picked.clone();
        picked.sort_unstable();
        match self.run("rebar.set_presentation", json!({ "rebar_ids": [p.set], "view_id": p.view, "mode": "select", "selected_indices": picked })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("The set shows {} picked bar(s) in this view", picked.len()), 0);
            }
            Err(e) => error(&frame, &format!("The presentation was not changed:\n{e}")),
        }
        true
    }

    /// View Visibility States: each view with Unobscured and (3D views) Solid.
    pub(crate) fn rebar_view_states(&mut self) {
        let frame = self.frame;
        let ids = self.selected_reinforcement(false);
        let Some(&first) = ids.first() else {
            return frame.set_status_text("View Visibility States: select rebar sets or area, path or fabric reinforcement first", 0);
        };
        let Ok(states) = self.run("rebar.view_states", json!({ "rebar_id": first })) else { return };
        let states: Vec<Value> = states.as_array().cloned().unwrap_or_default();
        if states.is_empty() {
            return frame.set_status_text("The model has no saved views yet", 0);
        }
        let dialog = Dialog::builder(&frame, "View Visibility States").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let note = StaticText::builder(&dialog)
            .with_label(&format!("{} element(s). Unobscured shows the bars through their host; Solid (3D views) draws them as tubes at Fine detail.", ids.len()))
            .build();
        note.wrap(420);
        sizer.add(&note, 0, SizerFlag::All, 10);
        let window = ScrolledWindow::builder(&dialog).build();
        window.set_scroll_rate(0, 10);
        let grid = FlexGridSizer::builder(0, 3).with_vgap(4).with_hgap(16).build();
        for h in ["View", "Unobscured", "Solid"] {
            let l = StaticText::builder(&window).with_label(h).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        }
        let mut boxes = Vec::new();
        for s in &states {
            let l = StaticText::builder(&window).with_label(s["name"].as_str().unwrap_or("")).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            let u = CheckBox::builder(&window).build();
            u.set_value(s["unobscured"] == true);
            grid.add(&u, 0, SizerFlag::AlignCenterVertical, 0);
            let solid = CheckBox::builder(&window).build();
            solid.set_value(s["solid"] == true);
            if s["is_3d"] != true {
                solid.enable(false);
                solid.set_tooltip("Only 3D views show rebar as solid");
            }
            grid.add(&solid, 0, SizerFlag::AlignCenterVertical, 0);
            boxes.push((s["view_id"].as_u64().unwrap_or(0), s["unobscured"] == true, s["solid"] == true, u, solid));
        }
        window.set_sizer(grid, true);
        window.set_min_size(Size::new(420, 260));
        sizer.add(&window, 1, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let changes: Vec<(u64, Option<bool>, Option<bool>)> = boxes
            .iter()
            .filter_map(|(view, u0, s0, u, s)| {
                let (u1, s1) = (u.get_value(), s.get_value());
                (u1 != *u0 || s1 != *s0).then_some((*view, (u1 != *u0).then_some(u1), (s1 != *s0).then_some(s1)))
            })
            .collect();
        dialog.destroy();
        let mut errors = Vec::new();
        for (view, u, s) in changes {
            let mut args = json!({ "rebar_ids": ids, "view_id": view });
            if let Some(u) = u {
                args["unobscured"] = json!(u);
            }
            if let Some(s) = s {
                args["solid"] = json!(s);
            }
            if let Err(e) = self.run("rebar.set_view_visibility", args) {
                errors.push(e);
            }
        }
        self.refresh_scene(&frame);
        if !errors.is_empty() {
            error(&frame, &format!("Some states were not changed:\n{}", errors.join("\n")));
        }
    }

    /// Concrete › Settings › Reinforcement Numbers.
    pub(crate) fn reinforcement_numbers(&mut self) {
        let frame = self.frame;
        let Ok(list) = self.run("reinforcement_numbering.list", json!({})) else {
            return frame.set_status_text("Open a model first", 0);
        };
        let seqs: Vec<Value> = list.as_array().cloned().unwrap_or_default();
        if seqs.is_empty() {
            return frame.set_status_text("Reinforcement Numbers: there is no rebar or fabric yet", 0);
        }
        // A partition and kind, then its sequence.
        let labels: Vec<String> = seqs
            .iter()
            .map(|s| {
                let p = s["partition"].as_str().unwrap_or("");
                format!("{} — {}", if p.is_empty() { "(none)" } else { p }, if s["kind"] == "fabric" { "Fabric" } else { "Rebar" })
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let pick = SingleChoiceDialog::builder(&frame, "Partition and sequence:", "Reinforcement Numbers", &refs).build();
        if pick.show_modal() != ID_OK {
            return;
        }
        let Some(seq) = usize::try_from(pick.get_selection()).ok().and_then(|i| seqs.get(i)).cloned() else { return };
        let partition = seq["partition"].as_str().unwrap_or("").to_owned();
        let kind = seq["kind"].as_str().unwrap_or("rebar").to_owned();
        let rows: Vec<Value> = seq["rows"].as_array().cloned().unwrap_or_default();

        let dialog = Dialog::builder(&frame, &format!("Reinforcement Numbers — {}", labels[usize::try_from(pick.get_selection()).unwrap_or(0)]))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let top = BoxSizer::builder(Orientation::Horizontal).build();
        let l = StaticText::builder(&dialog).with_label("Start number").build();
        top.add(&l, 0, SizerFlag::AlignCenterVertical | SizerFlag::All, 6);
        let start = TextCtrl::builder(&dialog).with_value(&seq["start"].as_u64().unwrap_or(1).to_string()).build();
        top.add(&start, 0, SizerFlag::All, 6);
        let gaps = CheckBox::builder(&dialog).with_label("Remove gaps").build();
        top.add(&gaps, 0, SizerFlag::AlignCenterVertical | SizerFlag::All, 6);
        let again = CheckBox::builder(&dialog).with_label("Renumber in creation order").build();
        top.add(&again, 0, SizerFlag::AlignCenterVertical | SizerFlag::All, 6);
        sizer.add_sizer(&top, 0, SizerFlag::All, 4);
        let grid = Grid::builder(&dialog).build();
        grid.create_grid(rows.len() as i32, 4, GridSelectionMode::Rows);
        for (c, h) in ["Number", "Count", "Elements", "Locked"].iter().enumerate() {
            grid.set_col_label_value(c as i32, h);
        }
        grid.set_col_size(0, 80);
        grid.set_col_size(1, 60);
        grid.set_col_size(2, 300);
        grid.set_col_size(3, 70);
        grid.set_col_format_bool(3);
        for (r, row) in rows.iter().enumerate() {
            let r = r as i32;
            grid.set_cell_value(r, 0, &row["number"].as_u64().unwrap_or(0).to_string());
            grid.set_cell_value(r, 1, &row["count"].as_u64().unwrap_or(0).to_string());
            grid.set_cell_value(r, 2, row["summary"].as_str().unwrap_or(""));
            grid.set_cell_value(r, 3, if row["locked"] == true { "1" } else { "" });
            grid.set_read_only(r, 1, true);
            grid.set_read_only(r, 2, true);
        }
        grid.set_min_size(Size::new(540, 260));
        sizer.add(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        const SELECT: i32 = 5101;
        let select = Button::builder(&dialog).with_id(SELECT).with_label("Select Elements").build();
        select.set_tooltip("Selects the elements of the row the cursor is on");
        buttons.add(&select, 0, SizerFlag::All, 6);
        select.on_click(move |_| dialog.end_modal(SELECT));
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        loop {
            match dialog.show_modal() {
                SELECT => {
                    let r = grid.get_grid_cursor_row().max(0) as usize;
                    let ids = rows.get(r).map(|row| row["element_ids"].clone()).unwrap_or(json!([]));
                    dialog.destroy();
                    let _ = self.run("selection.set", json!({ "ids": ids }));
                    self.selection_changed();
                    return;
                }
                ID_OK => {}
                _ => {
                    dialog.destroy();
                    return;
                }
            }
            // The edits; a number twice is refused, the other row marked.
            let mut edits = Vec::new();
            let mut taken: std::collections::BTreeMap<u32, usize> = Default::default();
            let mut clash = None;
            for (r, row) in rows.iter().enumerate() {
                let n = grid.get_cell_value(r as i32, 0).trim().parse::<u32>().unwrap_or(0);
                let locked = !grid.get_cell_value(r as i32, 3).trim().is_empty() && grid.get_cell_value(r as i32, 3).trim() != "0";
                if let Some(other) = taken.insert(n, r) {
                    clash = Some((r, other, n));
                }
                let mut e = json!({ "identity": row["identity"] });
                if Some(u64::from(n)) != row["number"].as_u64() {
                    e["number"] = json!(n);
                }
                if locked != (row["locked"] == true) {
                    e["locked"] = json!(locked);
                }
                if e.as_object().is_some_and(|o| o.len() > 1) {
                    edits.push(e);
                }
            }
            if let Some((r, other, n)) = clash {
                for c in 0..4 {
                    grid.set_cell_background_colour(other as i32, c, &Colour::new(255, 200, 190, 255));
                }
                grid.set_grid_cursor(r as i32, 0);
                error(&frame, &format!("Number {n} is already used by another row (marked); give this row another number"));
                continue;
            }
            let mut args = json!({ "partition": partition, "kind": kind, "rows": edits, "remove_gaps": gaps.get_value(), "renumber": again.get_value() });
            if let Ok(s) = start.get_value().trim().parse::<u32>()
                && Some(u64::from(s)) != seq["start"].as_u64()
            {
                args["start"] = json!(s);
            }
            match self.run("reinforcement_numbering.apply", args) {
                Ok(_) => {
                    dialog.destroy();
                    self.refresh_scene(&frame);
                    frame.set_status_text("Reinforcement numbers changed", 0);
                    return;
                }
                Err(e) => {
                    error(&frame, &format!("The numbers were not changed:\n{e}"));
                }
            }
        }
    }
}

/// A rebar annotation tool waiting for its point in a plan (STR-RBR-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnoTool {
    Tag,
    Aligned,
    Linear,
    Bending,
}

impl Gui {
    /// The rebar annotations of plan view `index` as lines: multi-rebar dimensions and bending
    /// details, the selected ones highlighted; also the segments to pick them by.
    pub(crate) fn rebar_annotation_lines(&self, index: usize, selected: &std::collections::HashSet<ElementId>) -> (Vec<Lines>, RebarPicks) {
        let v = &self.views[index];
        let (Some(element), Some(plan)) = (v.element, v.plan.as_ref()) else { return Default::default() };
        let Some(doc) = self.app.document(v.doc) else { return Default::default() };
        let store = doc.doc.store();
        let Some(view) = store.get(element) else { return Default::default() };
        let z = plan.range.cut as f32 + 0.002;
        let scale = plan.scale;
        let width = reviera_core::standards::pen_set(store).annotation_width(1) as f32;
        let (mut normal, mut lit) = (
            Lines { color: [0.0, 0.0, 0.0, 1.0], width, segments: Vec::new() },
            Lines { color: reviera_render::HIGHLIGHT, width, segments: Vec::new() },
        );
        let mut picks = Vec::new();
        for a in reviera_core::rebar_docs::of_view(store, view.guid().as_str()) {
            let (id, segs) = match a {
                reviera_core::rebar_docs::Drawn::Multi { id, drawn, .. } => {
                    let d = reviera_render::annotation::Dimension { points: drawn.points, along: drawn.along, line: drawn.line, texts: vec![drawn.text] };
                    (id, reviera_render::annotation::dimension_lines(&d, scale, z))
                }
                reviera_core::rebar_docs::Drawn::Bending { id, drawn, .. } => {
                    let mut segs: Vec<[[f32; 3]; 2]> = drawn
                        .segments
                        .iter()
                        .map(|s| [[s[0][0] as f32, s[0][1] as f32, z], [s[1][0] as f32, s[1][1] as f32, z]])
                        .collect();
                    // The shape code and segment values under the detail, 2.5 mm high on paper.
                    let h = 2.5 * scale / 1000.0;
                    let labels: Vec<reviera_render::annotation::Label> = drawn
                        .lines
                        .iter()
                        .enumerate()
                        .map(|(k, t)| reviera_render::annotation::Label {
                            origin: [drawn.text_at[0] as f32, drawn.text_at[1] as f32 - h * 1.6 * (k as f32 + 1.0), z],
                            right: [1.0, 0.0, 0.0],
                            up: [0.0, 1.0, 0.0],
                            height: h,
                            text: t.clone(),
                        })
                        .collect();
                    segs.extend(reviera_render::annotation::label_strokes(&labels));
                    (id, segs)
                }
            };
            picks.extend(segs.iter().map(|s| (id, *s)));
            if selected.contains(&id) { &mut lit } else { &mut normal }.segments.extend(segs);
        }
        (vec![normal, lit], picks)
    }

    /// The rebar annotation under pixel (`x`, `y`) of the active plan.
    pub(crate) fn rebar_annotation_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let z = self.views[index].plan.as_ref()?.range.cut as f32;
        let (_, picks) = self.rebar_annotation_lines(index, &Default::default());
        picks
            .into_iter()
            .map(|(id, s)| (id, self.pixel_distance(x, y, [[f64::from(s[0][0]), f64::from(s[0][1])], [f64::from(s[1][0]), f64::from(s[1][1])]], z)))
            .filter(|(_, d)| *d <= 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Rebar Tag, Multi-Rebar (Aligned or Linear) or Bending Detail: click where it goes.
    pub(crate) fn start_rebar_annotation(&mut self, tool: AnnoTool) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Select rebar sets first", 0);
        }
        if !self.in_plan() {
            return frame.set_status_text("Rebar tags, multi-rebar annotations and bending details are placed in plans", 0);
        }
        if matches!(tool, AnnoTool::Tag | AnnoTool::Bending) && sets.len() > 1 {
            return frame.set_status_text("Select one rebar set", 0);
        }
        let Some(view) = self.active_view().and_then(|v| v.element) else { return };
        let end = self.frame;
        self.end_tool(&end);
        self.rebar_anno = Some((tool, sets, view));
        frame.set_status_text(
            match tool {
                AnnoTool::Tag => "Rebar Tag: click where the tag goes",
                AnnoTool::Aligned | AnnoTool::Linear => "Multi-Rebar Annotation: click where the dimension line goes",
                AnnoTool::Bending => "Bending Detail: click where the detail goes",
            },
            0,
        );
    }

    pub(crate) fn end_rebar_annotation(&mut self) -> bool {
        self.rebar_anno.take().is_some()
    }

    pub(crate) fn rebar_annotation_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some((tool, sets, view)) = self.rebar_anno.clone() else { return };
        let Some(p) = self.plan_xy(x, y) else { return };
        let (cmd, args) = match tool {
            AnnoTool::Tag => ("rebar_tag.place", json!({ "rebar_id": sets[0], "view_id": view, "point": p })),
            AnnoTool::Aligned | AnnoTool::Linear => (
                "multi_rebar_annotation.place",
                json!({ "rebar_ids": sets, "view_id": view, "kind": if tool == AnnoTool::Linear { "linear" } else { "aligned" }, "line": p }),
            ),
            AnnoTool::Bending => ("bending_detail.place", json!({ "rebar_id": sets[0], "view_id": view, "point": p })),
        };
        match self.run(cmd, args) {
            Ok(_) => {
                self.rebar_anno = None;
                self.refresh_scene(&frame);
                frame.set_status_text("Placed", 0);
            }
            Err(e) => {
                self.rebar_anno = None;
                error(&frame, &format!("It was not placed:\n{e}"));
            }
        }
    }

    /// Concrete › Bar Bending Schedule: a schedule of the rebar grouped by partition and number.
    pub(crate) fn rebar_schedule(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        match self.run("rebar_schedule.create", json!({})) {
            Ok(v) => {
                self.browser_stale = true;
                self.open_view(doc, v.as_u64().map(ElementId));
            }
            Err(e) => error(&frame, &format!("The schedule was not made:\n{e}")),
        }
    }
}
