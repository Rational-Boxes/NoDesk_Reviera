// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Editing rebar sets on their contextual tab (STR-RBR-006, STR-RBR-015): Remove Bar, Split Set
//! and Move Bar act on the bar clicked; Reset Bars brings them back; Propagate copies the set to
//! the hosts clicked (Enter finishes); Pick New Host moves it to the host clicked; Splice cuts its
//! straight bars with a lap; Coupler joins the bar ends clicked (one, or two). Segments are edited
//! in Properties.

use glam::Vec3;
use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::rebar_view_ui::{BarPick, BarPurpose};
use super::{Gui, error};

/// Propagate or Pick New Host in progress: the sets and the hosts clicked so far.
#[derive(Debug, Clone)]
pub struct HostPick {
    pub propagate: bool,
    pub sets: Vec<ElementId>,
    pub hosts: Vec<ElementId>,
}

/// Coupler in progress: the bar ends that can be clicked (set, bar, end, point) and the picks.
#[derive(Debug, Clone)]
pub struct EndPick {
    pub ends: Vec<(ElementId, usize, &'static str, [f64; 3])>,
    pub picked: Vec<usize>,
}

impl Gui {
    fn one_set(&mut self, tool: &str) -> Option<ElementId> {
        let sets = self.selected_reinforcement(true);
        match sets[..] {
            [s] => Some(s),
            _ => {
                self.frame.set_status_text(&format!("{tool}: select one rebar set"), 0);
                None
            }
        }
    }

    /// Remove Bar, Split Set or Move Bar: click the bar.
    pub(crate) fn start_bar_edit(&mut self, purpose: BarPurpose) {
        let frame = self.frame;
        let Some(set) = self.one_set("Edit Bars") else { return };
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [set] })) else { return };
        let bars: Vec<Vec<[f64; 3]>> = i[0]["bars"].as_array().into_iter().flatten().map(|b| serde_json::from_value(b["points"].clone()).unwrap_or_default()).collect();
        let indices: Vec<usize> = serde_json::from_value(i[0]["bar_indices"].clone()).unwrap_or_default();
        let view = self.active_view().and_then(|v| v.element).unwrap_or(ElementId(0));
        let end = self.frame;
        self.end_tool(&end);
        self.bar_pick = Some(BarPick { purpose, set, view, bars, indices, picked: Vec::new() });
        frame.set_status_text(
            match purpose {
                BarPurpose::Remove => "Remove Bar: click the bar to remove (Esc ends)",
                BarPurpose::Split => "Split Set: click the first bar of the new set",
                _ => "Move Bar: click the bar to move",
            },
            0,
        );
    }

    /// The bar clicked in Remove Bar, Split Set or Move Bar.
    pub(crate) fn bar_edit(&mut self, k: usize) {
        let frame = self.frame;
        let Some(p) = self.bar_pick.clone() else { return };
        let index = p.indices.get(k).copied().unwrap_or(k);
        let result = match p.purpose {
            BarPurpose::Remove => self.run("rebar.remove_bar", json!({ "rebar_id": p.set, "index": index })),
            BarPurpose::Split => {
                self.end_bar_pick();
                self.run("rebar.split_set", json!({ "rebar_id": p.set, "at_index": k }))
            }
            BarPurpose::Move => {
                self.end_bar_pick();
                let d = TextEntryDialog::builder(&frame, "Move the bar along the set by (mm, + or −):", "Move Bar").with_default_value("50").build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let Some(mm) = d.get_value().and_then(|v| v.trim().parse::<f64>().ok()) else { return };
                self.run("rebar.move_bar_in_set", json!({ "rebar_id": p.set, "bar_index": index, "delta": mm / 1000.0 }))
            }
            BarPurpose::Present => return,
        };
        match result {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                // Remove Bar goes on with the bars left.
                if p.purpose == BarPurpose::Remove {
                    self.end_bar_pick();
                    let _ = self.run("selection.set", json!({ "ids": [p.set] }));
                    self.start_bar_edit(BarPurpose::Remove);
                } else {
                    frame.set_status_text("Done", 0);
                }
            }
            Err(e) => {
                self.end_bar_pick();
                error(&frame, &format!("The set was not changed:\n{e}"));
            }
        }
    }

    pub(crate) fn reset_rebar_bars(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Reset Bars") else { return };
        match self.run("rebar.reset_bars", json!({ "rebar_id": set })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Removed and moved bars are back", 0);
            }
            Err(e) => error(&frame, &format!("The bars were not reset:\n{e}")),
        }
    }

    /// Edit Constraints (STR-RBR-007): each handle of the selected sketched set kept to a host
    /// face's cover, a face, or another bar in the host, with its offset. While the dialog is open
    /// the bar and its references are drawn in the view.
    pub(crate) fn edit_rebar_constraints(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Edit Constraints") else { return };
        let info = match self.run("rebar.constraints", json!({ "rebar_id": set })) {
            Ok(i) => i,
            Err(e) => return error(&frame, &format!("Edit Constraints:\n{e}")),
        };
        let Some(store) = self.store_now() else { return };
        // The other sets in the same host, as bar references.
        let host = store.get(set).and_then(reviera_core::rebar::rebar_recipe).map(|r| r.host).unwrap_or_default();
        let others: Vec<ElementId> = store
            .iter()
            .filter(|e| e.id() != set && reviera_core::rebar::rebar_recipe(e).is_some_and(|r| r.host == host))
            .map(|e| e.id())
            .collect();
        let faces: Vec<String> = info["faces"].as_array().into_iter().flatten().filter_map(|f| f["name"].as_str().map(str::to_owned)).collect();
        let mut options: Vec<(String, Value)> = vec![("Free".into(), json!({ "kind": "none" }))];
        options.extend(faces.iter().map(|f| (format!("Cover: {f}"), json!({ "kind": "cover", "face": f }))));
        options.extend(faces.iter().map(|f| (format!("Face: {f}"), json!({ "kind": "surface", "face": f }))));
        options.extend(others.iter().map(|o| (format!("Bar of set {o}"), json!({ "kind": "bar", "set": o.0 }))));
        let current = |c: &Value| -> usize {
            let t = &c["target"];
            options
                .iter()
                .position(|(_, o)| match t["kind"].as_str() {
                    Some("bar") => o["kind"] == "bar" && store.get(ElementId(o["set"].as_u64().unwrap_or(0))).is_some_and(|e| e.guid().to_string() == t["rebar"].as_str().unwrap_or("")),
                    Some(k) => o["kind"] == k && o["face"] == t["face"],
                    None => false,
                })
                .unwrap_or(0)
        };
        let handles: Vec<Value> = info["handles"].as_array().cloned().unwrap_or_default();
        let centre: Vec<Value> = self
            .run("rebar.info", json!({ "rebar_ids": [set] }))
            .ok()
            .and_then(|i| i[0]["bars"][0]["points"].as_array().cloned())
            .unwrap_or_default();
        self.constraint_overlay(&centre, &handles);
        let f = super::reinforcement_ui::Form::new(&frame, "Edit Constraints");
        let names: Vec<String> = options.iter().map(|o| o.0.clone()).collect();
        let rows: Vec<(String, Choice, TextCtrl, usize, String)> = handles
            .iter()
            .map(|h| {
                let id = h["id"].as_str().unwrap_or("").to_owned();
                let label = match id.as_str() {
                    "start" => "Start".to_owned(),
                    "end" => "End".to_owned(),
                    s => format!("Segment {}", s.trim_start_matches("segment:").parse::<usize>().map_or(0, |k| k + 1)),
                };
                let was = if h["constraint"].is_null() { 0 } else { current(&h["constraint"]) };
                let off = h["constraint"]["offset"].as_f64().map_or(String::new(), |v| format!("{}", (v * 1e6).round() / 1000.0));
                let c = f.choice(&label, &names, was);
                let t = f.text("    offset (mm)", &off, "Further in from the reference; empty for 0 from a cover, else where the handle is now");
                (id, c, t, was, off)
            })
            .collect();
        let ok = f.show("Constrained handles follow the host; a reference must be within 6° of parallel (of square, for an end)", "Apply");
        let picks: Vec<(String, usize, String, usize, String)> = rows
            .iter()
            .map(|(id, c, t, was, off)| (id.clone(), c.get_selection().map_or(0, |i| i as usize), t.get_value().trim().to_owned(), *was, off.clone()))
            .collect();
        f.dialog.destroy();
        self.constraint_lines.clear();
        if !ok {
            return self.refresh_scene(&frame);
        }
        let mut failed = Vec::new();
        let mut changed = 0;
        for (id, k, off, was, was_off) in picks {
            if k == was && off == was_off {
                continue;
            }
            let o = &options[k.min(options.len() - 1)].1;
            let mut args = json!({ "rebar_id": set, "handle_id": id, "target_kind": o["kind"] });
            match o["kind"].as_str() {
                Some("bar") => args["target_ref"] = o["set"].clone(),
                Some("cover" | "surface") => args["target_ref"] = o["face"].clone(),
                _ => {}
            }
            if let Ok(v) = off.parse::<f64>() {
                args["offset"] = json!(v / 1000.0);
            }
            match self.run("rebar.constrain", args) {
                Ok(_) => changed += 1,
                Err(e) => failed.push(format!("{id}: {e}")),
            }
        }
        self.refresh_scene(&frame);
        if !failed.is_empty() {
            error(&frame, &format!("Some constraints were not set:\n{}", failed.join("\n")));
        }
        frame.set_status_text(&format!("Edit Constraints: {changed} handle(s) changed"), 0);
    }

    /// Draws a set's bar and each constrained handle's reference: a line to its plane and a
    /// cross on it.
    fn constraint_overlay(&mut self, centre: &[Value], handles: &[Value]) {
        use reviera_render::plan::Lines;
        let plan_z = self.active_view().and_then(|v| v.plan.as_ref()).map(|pl| pl.range.cut as f32 + 0.004);
        let width = if plan_z.is_some() { 0.6 } else { 0.1 };
        let v3 = |v: &Value| -> Option<[f64; 3]> { Some([v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?]) };
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, plan_z.unwrap_or(p[2] as f32)];
        let pts: Vec<[f64; 3]> = centre.iter().filter_map(v3).collect();
        let bar: Vec<[[f32; 3]; 2]> = pts.windows(2).map(|w| [f(w[0]), f(w[1])]).collect();
        let mut target = Vec::new();
        for h in handles {
            let (Some(at), Some(dir)) = (v3(&h["at"]), v3(&h["direction"])) else { continue };
            let (Some(p), Some(n)) = (v3(&h["target_point"]), v3(&h["target_normal"])) else { continue };
            let d = (at[0] - p[0]) * n[0] + (at[1] - p[1]) * n[1] + (at[2] - p[2]) * n[2];
            let foot = [at[0] - n[0] * d, at[1] - n[1] * d, at[2] - n[2] * d];
            target.push([f(at), f(foot)]);
            // A cross on the plane: along the handle and square to it.
            let side = [n[1] * dir[2] - n[2] * dir[1], n[2] * dir[0] - n[0] * dir[2], n[0] * dir[1] - n[1] * dir[0]];
            for a in [dir, side] {
                target.push([f([foot[0] - a[0] * 0.1, foot[1] - a[1] * 0.1, foot[2] - a[2] * 0.1]), f([foot[0] + a[0] * 0.1, foot[1] + a[1] * 0.1, foot[2] + a[2] * 0.1])]);
            }
        }
        // Drawn over the host, which hides the bar inside it.
        self.constraint_lines = vec![
            Lines { color: reviera_render::CONSTRAINT_BAR, width: width * 2.0, segments: bar },
            Lines { color: reviera_render::CONSTRAINT_TARGET, width, segments: target },
        ];
        // Drawn now: the dialog is modal.
        if let Some(id) = self.active {
            self.draw(id);
        }
    }

    /// Path Distribution (STR-RBR-014): the selected set's first bar carried along a path — its
    /// host's axis, along or up, or typed points.
    pub(crate) fn distribute_along_path(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Path Distribution") else { return };
        let kind = self.store_now().and_then(|s| {
            let r = s.get(set).and_then(reviera_core::rebar::rebar_recipe)?;
            let h = reviera_core::GlobalId::parse(&r.host).and_then(|g| s.find_by_guid(&g)).and_then(|i| s.get(i))?;
            reviera_core::rebar::host_check(&s, h).ok()
        });
        use reviera_core::rebar::HostKind;
        let mut paths: Vec<(&str, &str)> = match kind {
            Some(HostKind::Beam | HostKind::Column) => vec![("axis", "Its host's axis")],
            Some(HostKind::Wall) => vec![("up", "Up its wall"), ("along", "Along its wall")],
            _ => Vec::new(),
        };
        paths.push(("", "The points below"));
        let f = super::reinforcement_ui::Form::new(&frame, "Path Distribution");
        let which = f.choice("Path", &paths.iter().map(|p| p.1.to_owned()).collect::<Vec<_>>(), 0);
        let points = f.lines("Points (mm)", "", "x, y, z per line, starting where the main bar lies, square to it");
        let from = f.text("From (mm along it)", "", "Empty for its start");
        let to = f.text("To (mm along it)", "", "Empty for its end");
        let rule = f.choice("Layout", &["Maximum spacing".into(), "Fixed number".into()], 0);
        let amount = f.text("Spacing (mm) or number", "200", "");
        if !f.show("The main bar must lie square to the path where it starts; each bar stays square to the path", "Distribute") {
            f.dialog.destroy();
            return;
        }
        let k = which.get_selection().map_or(0, |i| i as usize).min(paths.len() - 1);
        let pts: Vec<[f64; 3]> = points
            .get_value()
            .lines()
            .filter_map(|l| {
                let v: Vec<f64> = l.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect();
                (v.len() == 3).then(|| [v[0] / 1000.0, v[1] / 1000.0, v[2] / 1000.0])
            })
            .collect();
        let mm = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok().map(|v| v / 1000.0);
        let v = amount.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let layout = if rule.get_selection() == Some(1) { json!({ "rule": "fixed_number", "n": v.round().max(1.0) as usize }) } else { json!({ "rule": "maximum_spacing", "spacing": v / 1000.0 }) };
        let mut args = json!({ "main_rebar_id": set, "layout": layout });
        if paths[k].0.is_empty() {
            args["path"] = json!(pts);
        } else {
            args["path_ref"] = json!(paths[k].0);
        }
        if let (Some(a), Some(b)) = (mm(&from), mm(&to)) {
            args["range"] = json!([a, b]);
        }
        f.dialog.destroy();
        match self.run("rebar.distribute_along_path", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Path distribution: placed as element {v}"), 0);
            }
            Err(e) => error(&frame, &format!("The bars were not distributed:\n{e}")),
        }
    }

    /// Morphed (STR-RBR-014): longitudinal bars through the selected stirrup set.
    pub(crate) fn distribute_morphed(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Morphed Distribution") else { return };
        let bars = self.run("rebar_bar_type.list", json!({})).unwrap_or_default();
        let ids: Vec<String> = bars.as_array().into_iter().flatten().filter_map(|b| b["id"].as_str().map(str::to_owned)).collect();
        let names: Vec<String> = bars.as_array().into_iter().flatten().filter_map(|b| b["name"].as_str().map(str::to_owned)).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Morphed Distribution");
        let bar = f.choice("Bar type", &names, 4);
        let by = f.choice("Spread", &["A number round the stirrups".into(), "A spacing along the first (mm)".into()], 0);
        let amount = f.text("Number or spacing", "8", "");
        if !f.show("Bars run through every stirrup, touching it, and follow the stirrups when they change", "Place") {
            f.dialog.destroy();
            return;
        }
        let v = amount.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let mut args = json!({ "driver_stirrup_set_id": set, "bar_type": ids.get(bar.get_selection().map_or(0, |i| i as usize)).cloned().unwrap_or_default() });
        if by.get_selection() == Some(1) {
            args["spacing"] = json!(v / 1000.0);
        } else {
            args["count"] = json!(v.round().max(1.0) as usize);
        }
        f.dialog.destroy();
        match self.run("rebar.distribute_morphed", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Morphed distribution: placed as element {v}"), 0);
            }
            Err(e) => error(&frame, &format!("The bars were not placed:\n{e}")),
        }
    }

    /// Reset Constraints: the selected sketched sets' handles freed where they are.
    pub(crate) fn reset_rebar_constraints(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Reset Constraints") else { return };
        match self.run("rebar.reset_constraints", json!({ "rebar_id": set })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Constraints removed; the bar stays where it is", 0);
            }
            Err(e) => error(&frame, &format!("The constraints were not reset:\n{e}")),
        }
    }

    /// Convert to Varying (`true`) or to Standard (STR-RBR-005).
    pub(crate) fn convert_rebar_sets(&mut self, varying: bool) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Select rebar sets first", 0);
        }
        let name = if varying { "rebar.convert_to_varying" } else { "rebar.convert_to_standard" };
        match self.run(name, json!({ "rebar_ids": sets })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} set(s) now {}", sets.len(), if varying { "varying" } else { "standard" }), 0);
            }
            Err(e) => error(&frame, &format!("The sets were not converted:\n{e}")),
        }
    }

    /// Propagate (`true`) or Pick New Host: click hosts.
    pub(crate) fn start_rebar_host_pick(&mut self, propagate: bool) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Select rebar sets first", 0);
        }
        let end = self.frame;
        self.end_tool(&end);
        self.rebar_host_pick = Some(HostPick { propagate, sets, hosts: Vec::new() });
        frame.set_status_text(
            if propagate {
                "Propagate: click the hosts to copy the set to; Enter finishes, Esc cancels"
            } else {
                "Pick New Host: click the new host"
            },
            0,
        );
    }

    pub(crate) fn end_rebar_host_pick(&mut self) -> bool {
        self.rebar_host_pick.take().is_some()
    }

    pub(crate) fn rebar_host_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(host) = self.host_element_at(x, y, panel) else {
            return frame.set_status_text("Click a concrete column, beam, structural floor or wall", 0);
        };
        let Some(p) = &mut self.rebar_host_pick else { return };
        if p.propagate {
            if !p.hosts.contains(&host) {
                p.hosts.push(host);
            }
            let n = p.hosts.len();
            let hosts = p.hosts.clone();
            let _ = self.run("selection.set", json!({ "ids": hosts }));
            self.selection_changed();
            return frame.set_status_text(&format!("Propagate: {n} host(s); Enter copies the set to them"), 0);
        }
        let sets = p.sets.clone();
        self.rebar_host_pick = None;
        match self.run("rebar.pick_new_host", json!({ "rebar_ids": sets, "host_id": host })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("The set moved to its new host", 0);
            }
            Err(e) => error(&frame, &format!("The set was not moved:\n{e}")),
        }
    }

    /// The element under the cursor, ignoring rebar drawn over it.
    fn host_element_at(&self, x: i32, y: i32, panel: &Panel) -> Option<ElementId> {
        let store = self.store_now()?;
        let id = self.element_at(x, y, panel)?;
        let e = store.get(id)?;
        reviera_core::rebar::host_check(&store, e).is_ok().then_some(id)
    }

    /// Enter finishes Propagate.
    pub(crate) fn rebar_host_key(&mut self, key: i32) -> bool {
        if !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) || self.rebar_host_pick.as_ref().is_none_or(|p| !p.propagate) {
            return false;
        }
        let frame = self.frame;
        let p = self.rebar_host_pick.take().expect("checked");
        if p.hosts.is_empty() {
            frame.set_status_text("Propagate: no hosts picked", 0);
            return true;
        }
        match self.run("rebar.propagate", json!({ "rebar_ids": p.sets, "target_host_ids": p.hosts })) {
            Ok(out) => {
                self.refresh_scene(&frame);
                let made = out["created"].as_array().map_or(0, Vec::len);
                let failed: Vec<String> = out["failed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| format!("{}: {}", f[0], f[1].as_str().unwrap_or("")))
                    .collect();
                if failed.is_empty() {
                    frame.set_status_text(&format!("{made} set(s) made"), 0);
                } else {
                    error(&frame, &format!("{made} set(s) made; not in:\n{}", failed.join("\n")));
                }
            }
            Err(e) => error(&frame, &format!("Nothing was propagated:\n{e}")),
        }
        true
    }

    /// Splice: where, lap and stagger.
    pub(crate) fn splice_rebar(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Splice") else { return };
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [set] })) else { return };
        let full = i[0]["lengths"][0].as_f64().unwrap_or(0.0);
        let d = TextEntryDialog::builder(
            &frame,
            &format!("The bars are {:.0} mm long. Splice at (mm from their start), lap (mm, blank for 40 diameters), stagger (mm):", full * 1000.0),
            "Splice",
        )
        .with_default_value(&format!("{:.0}, , 0", full * 500.0))
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let text = d.get_value().unwrap_or_default();
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        let mm = |k: usize| parts.get(k).and_then(|v| v.parse::<f64>().ok()).map(|v| v / 1000.0);
        let Some(at) = mm(0) else { return error(&frame, "Give where the splice goes, in mm") };
        let mut args = json!({ "rebar_id": set, "mode": "length", "length": at, "stagger": mm(2).unwrap_or(0.0) });
        if let Some(l) = mm(1) {
            args["lap"] = json!(l);
        }
        match self.run("rebar.splice", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Spliced into two sets that overlap by the lap", 0);
            }
            Err(e) => error(&frame, &format!("The set was not spliced:\n{e}")),
        }
    }

    /// Coupler: click a bar end of the selected sets, then a second (or Enter for one).
    pub(crate) fn start_coupler(&mut self) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Coupler: select the rebar sets whose bar ends to join", 0);
        }
        let mut ends = Vec::new();
        for s in sets {
            let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [s] })) else { continue };
            for (k, b) in i[0]["bars"].as_array().into_iter().flatten().enumerate() {
                let pts: Vec<[f64; 3]> = serde_json::from_value(b["points"].clone()).unwrap_or_default();
                if let (Some(a), Some(z)) = (pts.first(), pts.last()) {
                    ends.push((s, k, "start", *a));
                    ends.push((s, k, "end", *z));
                }
            }
        }
        let end = self.frame;
        self.end_tool(&end);
        self.end_pick = Some(EndPick { ends, picked: Vec::new() });
        frame.set_status_text("Coupler: click a bar end, then the end to join it to (Enter for one end)", 0);
    }

    pub(crate) fn end_coupler(&mut self) -> bool {
        self.end_pick.take().is_some()
    }

    pub(crate) fn coupler_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(v) = self.active_view() else { return };
        let size = v.panel.get_client_size();
        let Some(p) = &self.end_pick else { return };
        let near = p
            .ends
            .iter()
            .enumerate()
            .filter_map(|(k, e)| {
                let s = v.camera.project(Vec3::new(e.3[0] as f32, e.3[1] as f32, e.3[2] as f32), size.width as f32, size.height as f32)?;
                let d = (s.0 - x as f32).hypot(s.1 - y as f32);
                (d < 12.0).then_some((k, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        let Some(k) = near else {
            return frame.set_status_text("Coupler: click near a bar end of the selected sets", 0);
        };
        if let Some(p) = &mut self.end_pick {
            p.picked.push(k);
            if p.picked.len() < 2 {
                return frame.set_status_text("Coupler: click the other bar end, or Enter for a coupler on this end", 0);
            }
        }
        self.place_coupler();
    }

    pub(crate) fn coupler_key(&mut self, key: i32) -> bool {
        if self.end_pick.is_none() || !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            return false;
        }
        self.place_coupler();
        true
    }

    fn place_coupler(&mut self) {
        let frame = self.frame;
        let Some(p) = self.end_pick.take() else { return };
        if p.picked.is_empty() {
            return frame.set_status_text("Coupler: no bar end picked", 0);
        }
        let refs: Vec<Value> = p
            .picked
            .iter()
            .map(|k| {
                let e = &p.ends[*k];
                json!({ "rebar_id": e.0, "bar": e.1, "end": e.2 })
            })
            .collect();
        match self.run("rebar_coupler.place", json!({ "bar_end_refs": refs })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Coupler placed", 0);
            }
            Err(e) => error(&frame, &format!("The coupler was not placed:\n{e}")),
        }
    }
}
