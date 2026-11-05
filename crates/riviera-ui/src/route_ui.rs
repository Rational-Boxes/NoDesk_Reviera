// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Analyze › Route Analysis (ANA-ROUTE-001/002): **Path of Travel** (click the start, then the
//! end, in a plan; the route keeps the clearance and passes through doors), **Update Paths**
//! (works every path of the plan out again after the model changed), **Route Settings** (zone,
//! walking speed, clearance, ignored categories) and **Paths of Travel** (a list with length and
//! time). Plans draw each path in green with its length and time at its end; a path is picked
//! by clicking near it.

use riviera_core::ElementId;
use riviera_core::path_of_travel as pot;
use riviera_render::annotation::{Label, label_strokes};
use riviera_render::plan::Lines;
use serde_json::{Value, json};
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
        let stair = f.text("Stair speed (m/s)", &format!("{}", s.stair_speed), "Along a stair's flights, for egress routes across levels");
        let clear = f.text("Clearance (m)", &format!("{}", s.clearance), "Kept from obstacles");
        let ignored = f.text("Ignored categories", &s.ignored_categories.join(", "), "Category keys, e.g. cat.furniture");
        if !f.show("Run Update Paths afterwards to re-route existing paths.", "OK") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let cats: Vec<String> = ignored.get_value().split([',', ' ']).filter(|k| !k.trim().is_empty()).map(|k| k.trim().to_owned()).collect();
        let args = json!({ "zone_bottom": num(&bottom), "zone_top": num(&top), "walking_speed": num(&speed), "clearance": num(&clear), "ignored_categories": cats, "stair_speed": num(&stair) });
        f.dialog.destroy();
        if let Err(e) = self.run("route_settings.set", args) {
            error(&frame, &e);
        }
    }

    /// Analyze › Route Analysis › Egress Route (ANA-ROUTE-004): the quickest route from a point on
    /// one level to a point on another, by the stairs (and escalators where the egress mode counts
    /// them), reported leg by leg.
    pub(crate) fn egress_route_dialog(&mut self) {
        let frame = self.frame;
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if levels.len() < 2 {
            return error(&frame, "An egress route runs between levels: the model needs two or more.");
        }
        let names: Vec<String> = levels.iter().map(|l| l["name"].as_str().unwrap_or("").to_owned()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Egress Route");
        let from_level = f.choice("From level", &names, names.len() - 1);
        let from = f.text("From point (x, y)", "", "Metres, in that level's plan; empty: click it in a plan");
        let to_level = f.choice("To level", &names, 0);
        let to = f.text("To point (x, y)", "", "Metres: the exit; empty: click it in a plan");
        let mode = f.choice("Escalators", &["Not counted (IBC)".to_owned(), "Counted (NFPA 130)".to_owned(), "Counted (SCDF)".to_owned()], 0);
        let lifts = f.check("Lifts", "Take occupant evacuation elevators too (waiting 60 s for the car)", false);
        let draw = f.check("Plans", "Draw each leg as a path of travel in its level's floor plan", false);
        let ok = f.show("Walks each level round its obstacles (Route Settings) and takes the stairs between levels.", "Find Route");
        let point = |t: &TextCtrl| -> Option<[f64; 2]> {
            let v: Vec<f64> = t.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
            (v.len() == 2).then(|| [v[0], v[1]])
        };
        let (a, b) = (point(&from), point(&to));
        let la = levels[from_level.get_selection().unwrap_or(0) as usize]["id"].clone();
        let lb = levels[to_level.get_selection().unwrap_or(0) as usize]["id"].clone();
        let m = ["ibc", "nfpa130", "scdf"][mode.get_selection().unwrap_or(0) as usize];
        let draw = draw.is_checked();
        let lifts = lifts.is_checked();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut args = json!({ "mode": m, "lifts": lifts, "draw": draw });
        let mut slots = Vec::new();
        match a {
            Some(a) => args["start"] = json!({ "point": a, "level": la }),
            None => slots.push(json!({ "key": "start", "kind": "at", "prompt": "Egress route: click where it starts, in the plan of its level; Esc ends" })),
        }
        match b {
            Some(b) => args["end"] = json!({ "point": b, "level": lb }),
            None => slots.push(json!({ "key": "end", "kind": "at", "prompt": "Egress route: click the exit, in the plan of its level (open that plan first); Esc ends" })),
        }
        if slots.is_empty() {
            return self.egress_route_run(args);
        }
        self.start_picking("egress.route", slots, args);
    }

    /// Runs an egress route (drawn when `draw`) and reports it leg by leg.
    pub(crate) fn egress_route_run(&mut self, mut args: Value) {
        let frame = self.frame;
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let draw = args.as_object_mut().and_then(|o| o.remove("draw")).and_then(|v| v.as_bool()).unwrap_or(false);
        let r = match self.run(if draw { "egress.draw_route" } else { "egress.route" }, args) {
            Ok(r) => r,
            Err(e) => return error(&frame, &format!("No route:\n{e}")),
        };
        let name = |id: &Value| levels.iter().find(|l| l["id"] == *id).and_then(|l| l["name"].as_str()).unwrap_or("?").to_owned();
        let mut lines = Vec::new();
        let (legs, links) = (r["legs"].as_array().cloned().unwrap_or_default(), r["links"].as_array().cloned().unwrap_or_default());
        for (k, g) in legs.iter().enumerate() {
            lines.push(format!("Walk on {}: {:.1} m", name(&g["level"]), g["length"].as_f64().unwrap_or(0.0)));
            if let Some(l) = links.get(k) {
                lines.push(format!("{} from {} to {}: {:.1} m, {:.0} s", match l["kind"].as_str() {
                        Some("escalator") => "Escalator",
                        Some("lift") => "Evacuation lift (with its wait)",
                        _ => "Stair",
                    }, name(&l["from"]), name(&l["to"]), l["length"].as_f64().unwrap_or(0.0), l["time"].as_f64().unwrap_or(0.0)));
            }
        }
        if draw {
            let unplanned: Vec<String> = r["unplanned"].as_array().map(|u| u.iter().map(name).collect()).unwrap_or_default();
            lines.push(format!("\nDrawn as {} path(s) of travel{}", r["paths"].as_array().map_or(0, Vec::len), if unplanned.is_empty() { String::new() } else { format!("; no floor plan on {}", unplanned.join(", ")) }));
        }
        lines.push(format!("\nIn all {:.1} m, {:.0} s ({:.1} min)", r["length"].as_f64().unwrap_or(0.0), r["time"].as_f64().unwrap_or(0.0), r["time"].as_f64().unwrap_or(0.0) / 60.0));
        MessageDialog::builder(&frame, &lines.join("\n"), "Egress Route").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
    }

    /// Analyze › Route Analysis › Platform Clearing (ANA-ROUTE-005): NFPA 130's two tests for a
    /// station platform, reported with the exits, the capacities and the times.
    pub(crate) fn platform_clearing_dialog(&mut self) {
        let frame = self.frame;
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if levels.len() < 2 {
            return error(&frame, "A platform is cleared to another level: the model needs two or more.");
        }
        let names: Vec<String> = levels.iter().map(|l| l["name"].as_str().unwrap_or("").to_owned()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Platform Clearing");
        let platform = f.choice("Platform level", &names, 0);
        let load = f.text("Occupant load", "", "Persons; empty: worked out from the trains below (NFPA 130 5.5.5.1)");
        let trains = f.text("Trains entering at once", "", "Persons on each, one per track, e.g. 1200, 1000");
        let waiting = f.text("Waiting to board", "", "Persons on the platform for those trains");
        let train = f.text("Train along the platform", "", "x1, y1, x2, y2: its ends; the area figure then counts only the standing area (SCDF 2.2.2c)");
        let density = f.choice("Area figure", &["None".to_owned(), "Dense: 7 ft² (0.65 m²) a person".to_owned(), "Outlying: 15 ft² (1.39 m²) a person".to_owned()], 0);
        let remote = f.text("Most remote point (x, y)", "", "Metres, on the platform; empty: click it in the platform's plan");
        let safe_level = f.choice("Point of safety level", &names, names.len() - 1);
        let safe = f.text("Point of safety (x, y)", "", "Metres; empty: click it in a plan");
        let mode = f.choice("Escalators", &["Counted (NFPA 130)".to_owned(), "Not counted".to_owned()], 0);
        let concourse = f.text("Already on the concourse", "0", "Persons leaving through the same fare lines");
        let lifts = f.check("Lifts", "Count occupant evacuation elevators (NFPA 130 5.5.6.3.3; one out, one for firefighters)", false);
        let ok = f.show("The platform must clear within 4 minutes, and its most remote point reach a point of safety within 6.", "Check");
        let point = |t: &TextCtrl| -> Option<[f64; 2]> {
            let v: Vec<f64> = t.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
            (v.len() == 2).then(|| [v[0], v[1]])
        };
        let (a, b) = (point(&remote), point(&safe));
        let typed = load.get_value().trim().to_owned();
        let train_loads: Vec<f64> = trains.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
        let entraining = waiting.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let per_person = [None, Some(0.65), Some(1.39)][density.get_selection().unwrap_or(0) as usize];
        let ends: Vec<f64> = train.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
        let train_ends = (ends.len() == 4).then(|| [[ends[0], ends[1]], [ends[2], ends[3]]]);
        let lp = levels[platform.get_selection().unwrap_or(0) as usize]["id"].clone();
        let ls = levels[safe_level.get_selection().unwrap_or(0) as usize]["id"].clone();
        let m = ["nfpa130", "ibc"][mode.get_selection().unwrap_or(0) as usize];
        let lifts = lifts.is_checked();
        let on_concourse = concourse.get_value().trim().parse::<f64>().unwrap_or(0.0);
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut args = json!({ "level": lp, "mode": m, "lifts": lifts, "concourse_load": on_concourse });
        if typed.is_empty() {
            if train_loads.is_empty() && per_person.is_none() {
                return error(&frame, "Give the occupant load, or the trains (and an area figure) to work it out from.");
            }
            args["occupancy"] = json!({ "trains": train_loads, "entraining": entraining, "area_per_person": per_person, "train": train_ends });
        } else {
            let Ok(n) = typed.parse::<f64>() else { return error(&frame, "The occupant load must be a number.") };
            args["load"] = json!(n);
        }
        let mut slots = Vec::new();
        match a {
            Some(a) => args["remote"] = json!(a),
            None => slots.push(json!({ "key": "remote", "kind": "point", "prompt": "Platform clearing: click the platform's most remote point, in its plan; Esc ends" })),
        }
        match b {
            Some(b) => args["safety"] = json!({ "point": b, "level": ls }),
            None => slots.push(json!({ "key": "safety", "kind": "at", "prompt": "Platform clearing: click the point of safety, in the plan of its level (open that plan first); Esc ends" })),
        }
        if slots.is_empty() {
            return self.platform_clearing_run(args);
        }
        self.start_picking("egress.platform_clearing", slots, args);
    }

    /// Runs NFPA 130's platform clearing and reports it.
    pub(crate) fn platform_clearing_run(&mut self, args: Value) {
        let frame = self.frame;
        let c = match self.run("egress.platform_clearing", args) {
            Ok(c) => c,
            Err(e) => return error(&frame, &format!("Not worked out:\n{e}")),
        };
        let num = |k: &str| c[k].as_f64().unwrap_or(0.0);
        let pass = |k: &str| if c[k] == true { "meets" } else { "FAILS" };
        let basis = match c["load_basis"]["from"].as_str() {
            Some("trains") => " (the trains and those waiting)",
            Some("area") => " (the area figure)",
            _ => "",
        };
        let mut lines = vec![format!("Occupant load {:.0}{basis}", num("load"))];
        lines.push(format!(
            "Exits: stairs {:.0}/min, escalators {:.0}/min counted (of {:.0}, one out), lifts {:.0}/min counted (of {:.0})\nCapacity {:.0} persons a minute\n",
            num("stairs"),
            num("counted"),
            num("escalators"),
            num("lifts_counted"),
            num("lifts"),
            num("capacity")
        ));
        lines.push(format!("Platform cleared in {:.2} min (at most 4): {}", num("platform_time"), pass("meets_platform")));
        for x in c["level_exits"].as_array().cloned().unwrap_or_default() {
            let level = x["level"].clone();
            let name = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().and_then(|a| a.iter().find(|l| l["id"] == level).and_then(|l| l["name"].as_str().map(str::to_owned)))).unwrap_or_default();
            lines.push(format!("Up from {name}: {:.0} a minute, {:.2} min to pass, waiting {:.2} min", x["capacity"].as_f64().unwrap_or(0.0), x["flow_time"].as_f64().unwrap_or(0.0), x["wait"].as_f64().unwrap_or(0.0)));
        }
        for x in c["fare_lines"].as_array().cloned().unwrap_or_default() {
            lines.push(format!("Fare line: {:.0} a minute, {:.2} min to pass, waiting {:.2} min", x["capacity"].as_f64().unwrap_or(0.0), x["flow_time"].as_f64().unwrap_or(0.0), x["wait"].as_f64().unwrap_or(0.0)));
        }
        lines.push(format!(
            "To the point of safety: walking {:.2} min + waiting {:.2} min = {:.2} min (at most 6): {}",
            num("walking_time"),
            num("wait") + num("fare_wait") + num("exit_wait"),
            num("safety_time"),
            pass("meets_safety")
        ));
        for n in c["notes"].as_array().cloned().unwrap_or_default() {
            lines.push(format!("\n{}", n.as_str().unwrap_or("")));
        }
        MessageDialog::builder(&frame, &lines.join("\n"), "Platform Clearing").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
    }

    /// Asks for points one click at a time in plans: each slot's `key` gets the point (`kind`
    /// `point`) or the point and the clicked plan's level (`at`); then `finish` runs.
    pub(crate) fn start_picking(&mut self, finish: &str, slots: Vec<Value>, args: Value) {
        let frame = self.frame;
        let prompt = slots.first().and_then(|s| s["prompt"].as_str()).unwrap_or("Click in a plan; Esc ends").to_owned();
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(json!({ "command": "ui.pick", "finish": finish, "slots": slots, "args": args }));
        }
        frame.set_status_text(&prompt, 0);
    }

    /// A click of a pick sequence: the next point, then the next prompt, or the finish.
    pub(crate) fn picked(&mut self, mut pick: Value) {
        let frame = self.frame;
        let (point, level) = (pick["point"].clone(), pick["level"].clone());
        let Some(slots) = pick["slots"].as_array_mut() else { return };
        if slots.is_empty() {
            return;
        }
        let slot = slots.remove(0);
        let key = slot["key"].as_str().unwrap_or_default().to_owned();
        pick["args"][key.as_str()] = if slot["kind"] == "at" { json!({ "point": point, "level": level }) } else { point };
        if let Some(next) = pick["slots"].get(0).and_then(|s| s["prompt"].as_str()).map(str::to_owned) {
            if let Some(o) = pick.as_object_mut() {
                o.remove("point");
                o.remove("level");
                o.insert("command".into(), json!("ui.pick"));
            }
            if let Some(tool) = self.room_tool.as_mut() {
                tool.layout = Some(pick);
            }
            return frame.set_status_text(&next, 0);
        }
        self.end_room_tool();
        frame.set_status_text("", 0);
        let args = pick["args"].take();
        match pick["finish"].as_str() {
            Some("egress.route") => self.egress_route_run(args),
            Some("egress.platform_clearing") => self.platform_clearing_run(args),
            _ => {}
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
        let mut lit = Lines { color: riviera_render::HIGHLIGHT, width: 0.5, segments: Vec::new() };
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
