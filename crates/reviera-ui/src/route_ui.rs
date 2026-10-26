// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Route Analysis (ANA-ROUTE-001/002): **Path of Travel** (click the start, then the
//! end, in a plan; the route keeps the clearance and passes through doors), **Update Paths**
//! (works every path of the plan out again after the model changed), **Route Settings** (zone,
//! walking speed, clearance, ignored categories) and **Paths of Travel** (a list with length and
//! time). Plans draw each path in green with its length and time at its end; a path is picked
//! by clicking near it.

use reviera_core::ElementId;
use reviera_core::path_of_travel as pot;
use reviera_render::annotation::{Label, label_strokes};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PATH_COLOR: [f32; 4] = [0.1, 0.6, 0.25, 1.0];

impl Gui {
    /// A click with the Path of Travel tool: the start, then the end.
    pub(crate) fn path_click(&mut self, p: [f64; 2]) {
        let frame = self.frame;
        let Some(plan) = self.active_view().filter(|v| v.plan.is_some()).and_then(|v| v.element) else { return };
        let Some(start) = self.route_start.take() else {
            self.route_start = Some(p);
            return frame.set_status_text("Path of Travel: click the end point", 0);
        };
        match self.run("path_of_travel.create", json!({ "view_id": plan, "start": start, "end": p })) {
            Ok(id) => {
                let info = self.run("path_of_travel.list", json!({ "view_id": plan })).ok().and_then(|v| v.as_array()?.iter().find(|x| x["id"] == id).cloned());
                let (l, t) = info.map_or((0.0, 0.0), |i| (i["length"].as_f64().unwrap_or(0.0), i["time"].as_f64().unwrap_or(0.0)));
                frame.set_status_text(&format!("Path of travel: {l:.2} m, {t:.0} s. Click the next start, or Esc"), 0);
            }
            Err(e) => frame.set_status_text(&format!("{} — click a new start", e), 0),
        }
        self.refresh_scene(&frame);
        if let Some(doc) = self.doc {
            self.plans_redraw(doc);
        }
    }

    /// Update Paths: every path of the active plan.
    pub(crate) fn update_paths(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().filter(|v| v.plan.is_some()).and_then(|v| v.element) else {
            return frame.set_status_text("Update Paths works in plan views", 0);
        };
        match self.run("path_of_travel.update", json!({ "view_id": plan })) {
            Ok(v) => frame.set_status_text(&format!("{} path(s) re-routed", v.as_array().map_or(0, Vec::len)), 0),
            Err(e) => error(&frame, &e),
        }
        if let Some(doc) = self.doc {
            self.plans_redraw(doc);
        }
    }

    /// Route Settings.
    pub(crate) fn route_settings_dialog(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let s = pot::settings(&store);
        let f = super::reinforcement_ui::Form::new(&frame, "Route Settings");
        let bottom = f.text("Zone bottom (m)", &format!("{}", s.zone_bottom), "Above the level");
        let top = f.text("Zone top (m)", &format!("{}", s.zone_top), "Above the level");
        let speed = f.text("Walking speed (m/s)", &format!("{}", s.walking_speed), "");
        let clear = f.text("Clearance (m)", &format!("{}", s.clearance), "Kept from obstacles");
        let ignored = f.text("Ignored categories", &s.ignored_categories.join(", "), "Category keys, e.g. cat.furniture");
        if !f.show("Run Update Paths afterwards to re-route existing paths.", "OK") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let cats: Vec<String> = ignored.get_value().split([',', ' ']).filter(|k| !k.trim().is_empty()).map(|k| k.trim().to_owned()).collect();
        let args = json!({ "zone_bottom": num(&bottom), "zone_top": num(&top), "walking_speed": num(&speed), "clearance": num(&clear), "ignored_categories": cats });
        f.dialog.destroy();
        if let Err(e) = self.run("route_settings.set", args) {
            error(&frame, &e);
        }
    }

    /// Paths of Travel: a list with length and time.
    pub(crate) fn paths_dialog(&mut self) {
        let frame = self.frame;
        let list = match self.run("path_of_travel.list", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|p| {
                let routed = !p["route"].is_null();
                vec![
                    p["id"].to_string(),
                    if routed { format!("{:.2}", p["length"].as_f64().unwrap_or(0.0)) } else { "No route".into() },
                    if routed { format!("{:.1}", p["time"].as_f64().unwrap_or(0.0)) } else { String::new() },
                    p["waypoints"].as_array().map_or(0, Vec::len).to_string(),
                ]
            })
            .collect();
        super::mep_loads_ui::list_dialog(&frame, "Paths of Travel", "Each path's length and walking time (length ÷ walking speed).", &[("Path", 80), ("Length m", 90), ("Time s", 80), ("Waypoints", 80)], &rows, &[], None);
    }

    /// The paths of a plan: route lines and a label with length and time; selected ones lit.
    pub(crate) fn path_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let (Some(e), Some(plan)) = (v.element, v.plan.clone()) else { return Vec::new() };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let Some(guid) = store.get(e).map(|x| x.guid().to_string()) else { return Vec::new() };
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let z = plan.range.cut as f32 + 0.004;
        let h = 2.5 * plan.scale / 1000.0;
        let mut normal = Lines { color: PATH_COLOR, width: 0.35, segments: Vec::new() };
        let mut lit = Lines { color: reviera_render::HIGHLIGHT, width: 0.5, segments: Vec::new() };
        for x in store.iter() {
            let Some(d) = pot::path_data(x).filter(|d| d.view == guid) else { continue };
            let target = if selected.contains(&x.id()) { &mut lit } else { &mut normal };
            let Some(route) = &d.route else {
                target.segments.push([[d.start[0] as f32, d.start[1] as f32, z], [d.end[0] as f32, d.end[1] as f32, z]]);
                continue;
            };
            for w in route.windows(2) {
                target.segments.push([[w[0][0] as f32, w[0][1] as f32, z], [w[1][0] as f32, w[1][1] as f32, z]]);
            }
            // An arrowhead at the end and the length and time beside it.
            if let [.., a, b] = route[..] {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = dx.hypot(dy).max(1e-9);
                let (ux, uy) = ((dx / l) as f32, (dy / l) as f32);
                let tip = [b[0] as f32, b[1] as f32];
                for s in [1.0f32, -1.0] {
                    let back = [tip[0] - ux * h - s * uy * h * 0.5, tip[1] - uy * h + s * ux * h * 0.5];
                    target.segments.push([[tip[0], tip[1], z], [back[0], back[1], z]]);
                }
                let label = Label { origin: [tip[0] + h, tip[1] + h * 0.5, z], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], height: h, text: format!("{:.2} M  {:.0} S", d.length, d.time) };
                target.segments.extend(label_strokes(&[label]));
            }
        }
        vec![normal, lit]
    }

    /// The path of travel near pixel (`x`, `y`) in the active plan.
    pub(crate) fn path_at(&mut self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let plan = v.plan.as_ref()?;
        let guid = self.app.document(v.doc)?.doc.store().get(v.element?)?.guid().to_string();
        let tol = 1.5 * plan.scale as f64 / 1000.0;
        let p = self.plan_xy(x, y)?;
        let store = self.store_now()?;
        store.iter().find_map(|e| {
            let d = pot::path_data(e).filter(|d| d.view == guid)?;
            let route = d.route.unwrap_or_else(|| vec![d.start, d.end]);
            route
                .windows(2)
                .any(|w| {
                    let (a, b) = (w[0], w[1]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = (dx * dx + dy * dy).max(1e-12);
                    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
                    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy) <= tol
                })
                .then(|| e.id())
        })
    }
}
