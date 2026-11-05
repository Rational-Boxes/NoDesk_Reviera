// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Architecture › Circulation › Elevator (ARC-VT-002): the elevator's capacity, drive and the
//! levels it serves, its car glass and glass hoistway (ARC-VT-016), then a click in a plan at the
//! middle of its entrance (or a typed point). Check Elevators (ARC-VT-004) lists
//! `elevator.check`'s findings and selects and frames the elevator picked. Escalator (ARC-VT-010)
//! and Check Escalators (ARC-VT-012) do the same for escalators and moving walks; Stairlift
//! (ARC-VT-013) puts a seated or platform lift on the selected stair, and Check Stairlifts
//! (ARC-VT-015) lists `stairlift.check`'s findings.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    pub(crate) fn place_elevator(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return error(&frame, "Open a plan of the lowest level the elevator serves first.");
        };
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let above: Vec<&Value> = levels.iter().filter(|l| l["elevation"].as_f64().unwrap_or(f64::MIN) > plan.elevation + 1e-6).collect();
        if above.is_empty() {
            return error(&frame, "The elevator needs a level above this plan's to serve.");
        }
        let names: Vec<String> = above.iter().map(|l| l["name"].as_str().unwrap_or_default().to_owned()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Elevator");
        let kinds = ["passenger", "hospital", "freight", "vehicle", "lula", "residence", "platform_lift", "dumbwaiter"];
        let kind = f.choice(
            "Kind",
            &["Passenger".to_owned(), "Hospital or service".to_owned(), "Freight".to_owned(), "Vehicle".to_owned(), "LULA (limited use)".to_owned(), "Home (private residence)".to_owned(), "Vertical platform lift".to_owned(), "Dumbwaiter".to_owned()],
            0,
        );
        let top = f.choice("Serves up to", &names, names.len() - 1);
        let load = f.text("Rated load (kg)", "", "Empty: the smallest car that meets the accessibility rule packs and, where needed, takes the stretcher");
        let drive = f.choice("Drive", &["Traction, no machine room".to_owned(), "Traction, machine room".to_owned(), "Hydraulic".to_owned(), "Holeless hydraulic".to_owned()], 0);
        let speed = f.text("Speed (m/s)", "", "Empty: 1.0, or 0.5 for hydraulic drives; small lifts take their own");
        let door = f.choice("Doors", &["As the car comes".to_owned(), "Side opening".to_owned(), "Centre opening".to_owned()], 0);
        let rotation = f.text("Rotation (°)", "0", "0: the landing is toward the bottom of the plan; counter-clockwise");
        let car_glass = f.choice("Car glass", &["None".to_owned(), "Rear (panoramic)".to_owned(), "Rear and sides".to_owned()], 0);
        let walls = f.choice("Hoistway", &["Walls".to_owned(), "Glass at the sides and rear, on steel posts".to_owned(), "No walls".to_owned()], 0);
        let equipment = f.choice("Controller and machine", &["On the right".to_owned(), "On the left".to_owned(), "None".to_owned()], 0);
        let at = f.text("Entrance point (x, y)", "", "Empty: click in the plan at the middle of the entrance");
        let ok = f.show(
            &format!("Sized under the vertical-transport rule packs in use, from {} up; the shaft cuts the floors between. Then click the middle of its entrance.", plan.level_name),
            "Place",
        );
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let drive = ["mrl_traction", "traction", "hydraulic", "holeless"][drive.get_selection().unwrap_or(0) as usize];
        let glass = ["none", "rear", "rear_sides"][car_glass.get_selection().unwrap_or(0) as usize];
        let mut args = json!({
            "command": "elevator.create",
            "kind": kinds[kind.get_selection().unwrap_or(0) as usize],
            "top_level": above[top.get_selection().unwrap_or(0) as usize]["id"],
            "drive": drive,
            "rotation": num(&rotation).unwrap_or(0.0).to_radians(),
            "walls": walls.get_selection() != Some(2),
            "hoistway": if walls.get_selection() == Some(1) { "glass" } else { "walls" },
            "car_glass": glass,
            "equipment": equipment.get_selection() != Some(2),
            "equipment_side": if equipment.get_selection() == Some(1) { "left" } else { "right" },
        });
        if let Some(l) = num(&load) {
            args["load"] = json!(l);
        }
        if let Some(s) = num(&speed) {
            args["speed"] = json!(s);
        }
        match door.get_selection() {
            Some(1) => args["door"] = json!("side"),
            Some(2) => args["door"] = json!("centre"),
            _ => {}
        }
        let typed: Vec<f64> = at.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
        f.dialog.destroy();
        if !ok {
            return;
        }
        if let [x, y] = typed[..] {
            args["point"] = json!([x, y]);
            args["level"] = json!(plan.level);
            return self.layout_bathroom_at(args);
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Elevator: click the middle of its entrance; Esc ends", 0);
        }
    }

    /// After `elevator.create`: what was made, and what Check Elevators finds.
    pub(crate) fn elevator_made(&mut self, made: &Value) {
        let frame = self.frame;
        self.refresh_scene(&frame);
        let problems = self
            .run("elevator.check", json!({ "elevators": [made["elevator"]] }))
            .ok()
            .and_then(|v| v.as_array().map(|a| a.iter().filter(|f| f["severity"] == "error").count()))
            .unwrap_or(0);
        let mm = |v: &Value| v.as_f64().map_or(0.0, |x| x * 1000.0);
        frame.set_status_text(
            &format!(
                "{}: {} kg, {} persons; car {:.0} × {:.0} mm; hoistway {:.0} × {:.0} mm, pit {:.0} mm, overhead {:.0} mm{} ({}); Check Elevators finds {problems} error(s)",
                made["kind"].as_str().unwrap_or("elevator"),
                made["load"],
                made["persons"],
                mm(&made["car"][0]),
                mm(&made["car"][1]),
                mm(&made["hoistway"][0]),
                mm(&made["hoistway"][1]),
                mm(&made["pit"]),
                mm(&made["overhead"]),
                made["power"]["motor_kw"].as_f64().map(|k| format!("; motor about {k:.1} kW")).unwrap_or_default(),
                made["sizing_pack"].as_str().unwrap_or_default()
            ),
            0,
        );
    }

    /// Architecture › Circulation › Check Elevators (ARC-VT-004).
    pub(crate) fn check_elevators(&mut self) {
        let frame = self.frame;
        let in_use: Vec<String> = self
            .run("vt.rule_packs", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|p| p["in_use"] == true)
            .filter_map(|p| p["title"].as_str().map(str::to_owned))
            .collect();
        let found = match self.run("elevator.check", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        let packs = in_use.join("; ");
        if found.is_empty() {
            let d = MessageDialog::builder(&frame, &format!("No problems found under: {packs}.\nHoistway and landing intrusions are listed by Check Clearances."), "Check Elevators")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let severity = if f["severity"] == "note" { "Note" } else { "Error" };
                let mut row = format!("{}. {severity}: {} — {}", k + 1, f["rule"].as_str().unwrap_or_default(), f["message"].as_str().unwrap_or_default());
                if f["confidence"] != "verified" {
                    row.push_str(&format!(" [{}]", f["confidence"].as_str().unwrap_or_default()));
                }
                row
            })
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let errors = found.iter().filter(|f| f["severity"] == "error").count();
        let dialog = SingleChoiceDialog::builder(&frame, &format!("{errors} error(s) and {} note(s) under: {packs}.\nPick one to select and frame the elevator:", found.len() - errors), "Check Elevators", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(f) = picked.and_then(|i| found.get(i)) else {
            return;
        };
        let _ = self.run("selection.set", json!({ "ids": [f["elevator"]] }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(&format!("{} — {} ({}, {})", f["rule"].as_str().unwrap_or_default(), f["description"].as_str().unwrap_or_default(), f["edition"].as_str().unwrap_or_default(), f["source"].as_str().unwrap_or_default()), 0);
    }

    /// Architecture › Circulation › Egress Role (ANA-ROUTE-006): the selected elevators' part in
    /// an evacuation.
    pub(crate) fn elevator_egress_role(&mut self) {
        let frame = self.frame;
        let lifts: Vec<riviera_core::ElementId> = {
            let selected = self.selection();
            let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store()) else {
                return;
            };
            selected.into_iter().filter(|id| store.get(*id).and_then(riviera_core::elevators::elevator).is_some()).collect()
        };
        if lifts.is_empty() {
            return error(&frame, "Select the elevators first (their cars).");
        }
        let roles = ["Neither", "Occupant evacuation elevator (IBC 3008, EN 81-76)", "Fire service access elevator (IBC 3007, EN 81-72)"];
        let labels: Vec<&str> = roles.to_vec();
        let dialog = SingleChoiceDialog::builder(&frame, &format!("The part of {} elevator(s) in an evacuation. Egress routes take occupant evacuation elevators when Lifts is ticked.", lifts.len()), "Egress Role", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).unwrap_or(0);
        dialog.destroy();
        let role = ["none", "occupant_evacuation", "fire_service"][picked.min(2)];
        for id in lifts {
            if let Err(e) = self.run("elevator.set_egress_role", json!({ "elevator_id": id, "role": role })) {
                return error(&frame, &e);
            }
        }
        frame.set_status_text(&format!("Egress role: {}", roles[picked.min(2)]), 0);
    }

    /// Architecture › Circulation › Escalator (ARC-VT-010): an escalator or moving walk from this
    /// plan's level, then a click at the middle of its lower end (or a typed point).
    pub(crate) fn place_escalator(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return error(&frame, "Open a plan of the escalator's lower level first.");
        };
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let above: Vec<&Value> = levels.iter().filter(|l| l["elevation"].as_f64().unwrap_or(f64::MIN) > plan.elevation + 1e-6).collect();
        let mut names: Vec<String> = vec!["None: a horizontal walk on this level".to_owned()];
        names.extend(above.iter().map(|l| l["name"].as_str().unwrap_or_default().to_owned()));
        let f = super::reinforcement_ui::Form::new(&frame, "Escalator or Moving Walk");
        let kind = f.choice("Kind", &["Escalator".to_owned(), "Moving walk".to_owned()], 0);
        let top = f.choice("Up to", &names, usize::from(!above.is_empty()));
        let width = f.choice("Step or pallet width", &["600 mm".to_owned(), "800 mm".to_owned(), "1000 mm".to_owned(), "1200 mm (walks)".to_owned(), "1400 mm (walks)".to_owned()], 2);
        let incline = f.text("Inclination (°)", "", "Empty: 30° for escalators, 12° for inclined walks");
        let speed = f.text("Speed (m/s)", "", "Empty: 0.5, or 0.65 in a station");
        let flat = f.text("Flat steps", "", "Empty: the least the rule packs ask for the rise and speed");
        let duty = f.choice("Duty", &["Commercial".to_owned(), "Transit (heavy duty)".to_owned()], 0);
        let context = f.choice("Setting", &["A building".to_owned(), "An airport".to_owned(), "A station (transit: 0.65 m/s, 6 m run-off)".to_owned()], 0);
        let length = f.text("Walk length (m)", "", "A horizontal walk's length between its working points");
        let pit = f.choice("Horizontal walk", &["On the floor".to_owned(), "In a pit, flush".to_owned()], 0);
        let rotation = f.text("Rotation (°)", "0", "0: it rises toward the right of the plan; counter-clockwise");
        let at = f.text("Lower end point (x, y)", "", "Empty: click in the plan at the middle of its lower end");
        let ok = f.show("Sized under the vertical-transport rule packs in use, with floor openings for its pits and headroom. Then click the middle of its lower end.", "Place");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let step = [0.6, 0.8, 1.0, 1.2, 1.4][width.get_selection().unwrap_or(2) as usize];
        let setting = ["building", "airport", "transit"][context.get_selection().unwrap_or(0) as usize];
        let mut args = json!({
            "command": "escalator.create",
            "kind": if kind.get_selection() == Some(1) { "moving_walk" } else { "escalator" },
            "step_width": step,
            "rotation": num(&rotation).unwrap_or(0.0).to_radians(),
            "duty": if duty.get_selection() == Some(1) { "transit" } else { "commercial" },
            "pit": pit.get_selection() == Some(1),
            "context": setting,
        });
        if let Some(t) = top.get_selection().and_then(|k| (k as usize).checked_sub(1)).and_then(|k| above.get(k)) {
            args["top_level"] = t["id"].clone();
        }
        for (key, t) in [("inclination", &incline), ("speed", &speed), ("length", &length)] {
            if let Some(v) = num(t) {
                args[key] = json!(v);
            }
        }
        if let Some(v) = num(&flat) {
            args["flat_steps"] = json!(v as u32);
        }
        let typed: Vec<f64> = at.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
        f.dialog.destroy();
        if !ok {
            return;
        }
        if let [x, y] = typed[..] {
            args["point"] = json!([x, y]);
            args["level"] = json!(plan.level);
            return self.layout_bathroom_at(args);
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Escalator: click the middle of its lower end; Esc ends", 0);
        }
    }

    /// Architecture › Circulation › Check Lobbies (ANA-ROUTE-007): each evacuation elevator's lobby
    /// on each floor; pick a problem to select and frame the elevator.
    pub(crate) fn check_lobbies(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Check Lobbies");
        let occupants = f.text("Occupants a floor", "", "Empty: the floors at 150 ft² (13.94 m²) a person");
        let ok = f.show("Each lobby holds a quarter of its floor's load at 3 ft² a person and a wheelchair space for each 50, and reaches an exit stair (IBC 3008.6).", "Check");
        let typed = occupants.get_value().trim().parse::<f64>().ok();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let args = typed.map_or_else(|| json!({}), |n| json!({ "occupants": n }));
        let lobbies = match self.run("elevator.check_lobbies", args) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        if lobbies.is_empty() {
            return error(&frame, "No occupant evacuation elevators: mark one with Egress Role first.");
        }
        let problems: Vec<(Value, String)> = lobbies.iter().flat_map(|l| l["problems"].as_array().cloned().unwrap_or_default().into_iter().map(move |p| (l["elevator"].clone(), p.as_str().unwrap_or_default().to_owned()))).collect();
        if problems.is_empty() {
            let d = MessageDialog::builder(&frame, &format!("All {} lobbies are large enough and reach an exit stair.", lobbies.len()), "Check Lobbies").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build();
            d.show_modal();
            return;
        }
        let labels: Vec<&str> = problems.iter().map(|(_, p)| p.as_str()).collect();
        let dialog = SingleChoiceDialog::builder(&frame, &format!("{} problem(s) in {} lobbies. Pick one to select and frame the elevator:", problems.len(), lobbies.len()), "Check Lobbies", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some((lift, text)) = picked.and_then(|i| problems.get(i)).cloned() else { return };
        let _ = self.run("selection.set", json!({ "ids": [lift] }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(&text, 0);
    }

    /// Architecture › Circulation › Inclined Elevator (ARC-VT-023): a car on a straight track up a
    /// slope between landings on two levels, typed or clicked in plans.
    pub(crate) fn place_inclined_elevator(&mut self) {
        let frame = self.frame;
        let levels: Vec<Value> = self.run("level.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if levels.len() < 2 {
            return error(&frame, "An inclined elevator runs between levels: the model needs two or more.");
        }
        let names: Vec<String> = levels.iter().map(|l| l["name"].as_str().unwrap_or("").to_owned()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Inclined Elevator");
        let bottom_level = f.choice("Bottom landing level", &names, 0);
        let bottom = f.text("Bottom landing (x, y)", "", "Where the car's gate is; empty: click it in that level's plan");
        let top_level = f.choice("Top landing level", &names, names.len() - 1);
        let top = f.text("Top landing (x, y)", "", "Empty: click it in that level's plan");
        let sizes = ["small", "medium", "large", "xl", "ada", "hd", "hdx", "hdxl"];
        let size = f.choice("Car", &["3 × 3 ft".to_owned(), "3 × 4 ft".to_owned(), "4 × 4 ft".to_owned(), "4 × 5 ft".to_owned(), "5 × 5 ft (accessible)".to_owned(), "5 × 6 ft".to_owned(), "5 × 7 ft".to_owned(), "5 × 8 ft".to_owned()], 2);
        let use_ = f.choice("Use", &["Commercial (A17.1 5.1)".to_owned(), "Residential (A17.1 5.4)".to_owned()], 0);
        let enclosure = f.choice("Car", &["Enclosed (75 in walls, roof)".to_owned(), "Open (42 in rails)".to_owned()], 0);
        let drives = ["wd", "wdx", "clx", "wdd"];
        let drive = f.choice("Drive", &["Winding drum on a slab (≤ 500 ft, 2000 lb)".to_owned(), "Winding drum on the rail (≤ 155 ft, 1200 lb)".to_owned(), "Continuous loop (1200 lb)".to_owned(), "Double drum on a slab (≤ 500 ft, 3800 lb)".to_owned()], 0);
        let accessible = f.check("Accessible route", "The car must be the accessible one (5 × 5 ft, 36 in door)", false);
        let ok = f.show("The track is an alignment from the bottom landing up to the top one; a level landing is kept clear at each gate.", "Place");
        let point = |t: &TextCtrl| -> Option<[f64; 2]> {
            let v: Vec<f64> = t.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
            (v.len() == 2).then(|| [v[0], v[1]])
        };
        let (a, b) = (point(&bottom), point(&top));
        let lb = levels[bottom_level.get_selection().unwrap_or(0) as usize]["id"].clone();
        let lt = levels[top_level.get_selection().unwrap_or(0) as usize]["id"].clone();
        let size = sizes[size.get_selection().unwrap_or(2) as usize];
        let use_ = if use_.get_selection() == Some(1) { "residential" } else { "commercial" };
        let enclosure = if enclosure.get_selection() == Some(1) { "open" } else { "enclosed" };
        let drive = drives[drive.get_selection().unwrap_or(0) as usize];
        let accessible = accessible.is_checked();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut args = json!({ "size": size, "use": use_, "enclosure": enclosure, "drive": drive, "accessible": accessible });
        let mut slots = Vec::new();
        match a {
            Some(a) => args["bottom"] = json!({ "point": a, "level": lb }),
            None => slots.push(json!({ "key": "bottom", "kind": "at", "prompt": "Inclined elevator: click the bottom landing, in its level's plan; Esc ends" })),
        }
        match b {
            Some(b) => args["top"] = json!({ "point": b, "level": lt }),
            None => slots.push(json!({ "key": "top", "kind": "at", "prompt": "Inclined elevator: click the top landing, in its level's plan (open that plan first); Esc ends" })),
        }
        if slots.is_empty() {
            return self.inclined_elevator_run(args);
        }
        self.start_picking("inclined_elevator.create", slots, args);
    }

    /// Places an inclined elevator and reports it with what its check finds.
    pub(crate) fn inclined_elevator_run(&mut self, args: Value) {
        let frame = self.frame;
        let made = match self.run("inclined_elevator.create", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Could not place the inclined elevator:\n{e}")),
        };
        self.refresh_scene(&frame);
        let id = made["inclined_elevator"].clone();
        let problems: Vec<String> = self
            .run("inclined_elevator.check", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|x| x["inclined_elevator"] == id)
            .filter_map(|x| x["message"].as_str().map(str::to_owned))
            .collect();
        let head = format!(
            "Inclined elevator: {:.1} m of track at {:.1}°, {:.0} s a trip",
            made["length"].as_f64().unwrap_or(0.0),
            made["angle"].as_f64().unwrap_or(0.0),
            made["trip"].as_f64().unwrap_or(0.0)
        );
        if problems.is_empty() {
            frame.set_status_text(&format!("{head}; no problems found"), 0);
        } else {
            let d = MessageDialog::builder(&frame, &format!("{head}\n\n{}", problems.join("\n")), "Inclined Elevator").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning).build();
            d.show_modal();
        }
    }

    /// Architecture › Circulation › Funicular (ARC-VT-024): two counterbalanced cars along the
    /// selected alignment, from its start (the bottom station) to its end.
    pub(crate) fn place_funicular(&mut self) {
        let frame = self.frame;
        let line = {
            let selected = self.selection();
            let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store()) else {
                return;
            };
            selected.into_iter().find(|id| store.get(*id).and_then(riviera_core::alignments::recipe).is_some())
        };
        let Some(line) = line else {
            return error(&frame, "Select the alignment the funicular runs along first (Massing & Site › Alignment), with a profile rising from its start to its end.");
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Funicular");
        let layout = f.choice("Track", &["Two rails, Abt passing loop half way".to_owned(), "Four rails (two tracks)".to_owned()], 0);
        let gauge = f.text("Gauge (mm)", "1000", "762 to 1524 on the reference lines; 1000 and 1435 are common");
        let length = f.text("Car length (m)", "12", "");
        let persons = f.text("Persons a car", "60", "");
        let speed = f.text("Speed (m/s)", "5", "");
        let dwell = f.text("Stop at each station (s)", "60", "");
        let floor = f.choice("Car floor", &["Level (levelling or stepped car, level platforms)".to_owned(), "Along the slope (stepped platforms)".to_owned()], 0);
        let walkway = f.check("Walkway", "An evacuation walkway beside the track", false);
        let ok = f.show("Its start is the bottom station and its end the top one; the drive is at the top.", "Place");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let mut args = json!({
            "alignment_id": line,
            "layout": if layout.get_selection() == Some(1) { "four_rail" } else { "abt_loop" },
            "car_floor": if floor.get_selection() == Some(1) { "stepped" } else { "level" },
            "walkway": walkway.is_checked(),
        });
        if let Some(g) = num(&gauge) {
            args["gauge"] = json!(g / 1000.0);
        }
        for (k, t) in [("car_length", &length), ("speed", &speed), ("dwell", &dwell)] {
            if let Some(v) = num(t) {
                args[k] = json!(v);
            }
        }
        if let Some(p) = num(&persons) {
            args["car_capacity"] = json!(p.max(0.0) as u32);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        let made = match self.run("funicular.create", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Could not place the funicular:\n{e}")),
        };
        self.refresh_scene(&frame);
        frame.set_status_text(
            &format!(
                "Funicular: {:.0} m of track rising {:.0} m, steepest {:.0} %, {:.0} s a trip, {:.0} persons an hour each way",
                made["length"].as_f64().unwrap_or(0.0),
                made["rise"].as_f64().unwrap_or(0.0),
                made["max_grade"].as_f64().unwrap_or(0.0),
                made["trip"].as_f64().unwrap_or(0.0),
                made["capacity"].as_f64().unwrap_or(0.0)
            ),
            0,
        );
    }

    /// The first selected element `keep` accepts.
    fn selected_where(&mut self, keep: impl Fn(&riviera_core::Element) -> bool) -> Option<riviera_core::ElementId> {
        let selected = self.selection();
        let store = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store())?;
        selected.into_iter().find(|id| store.get(*id).is_some_and(&keep))
    }

    /// Architecture › Circulation › APM Guideway (ARC-VT-025): along the selected alignment.
    pub(crate) fn place_apm_guideway(&mut self) {
        let frame = self.frame;
        let Some(line) = self.selected_where(|e| riviera_core::alignments::recipe(e).is_some()) else {
            return error(&frame, "Select the alignment the guideway runs along first (Massing & Site › Alignment).");
        };
        let f = super::reinforcement_ui::Form::new(&frame, "APM Guideway");
        let lanes = f.choice("Lanes", &["Two (dual lane)".to_owned(), "One (single lane)".to_owned()], 0);
        let sep = f.text("Track separation (m)", "4.75", "Between the lanes' centres: 4.6–4.9 m beside the way, 6.7 m at crossovers");
        let walkway = f.check("Walkway", "An emergency walkway along the deck", true);
        let ok = f.show("Each lane's 3.66 m dynamic envelope is kept clear for Check Clearances.", "Place");
        let mut args = json!({ "alignment_id": line, "lanes": if lanes.get_selection() == Some(1) { 1 } else { 2 }, "walkway": walkway.is_checked() });
        if let Ok(v) = sep.get_value().trim().parse::<f64>() {
            args["track_separation"] = json!(v);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("apm.guideway", args) {
            Ok(g) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("APM guideway: {:.0} m, deck {:.2} m wide", g["length"].as_f64().unwrap_or(0.0), g["deck_width"].as_f64().unwrap_or(0.0)), 0);
            }
            Err(e) => error(&frame, &format!("Could not place the guideway:\n{e}")),
        }
    }

    /// Architecture › Circulation › APM Station (ARC-VT-026): on the selected guideway, at a typed
    /// chainage or where it is clicked.
    pub(crate) fn place_apm_station(&mut self) {
        let frame = self.frame;
        let Some(gid) = self.selected_where(|e| riviera_core::apm::guideway(e).is_some()) else {
            return error(&frame, "Select the APM guideway first.");
        };
        let f = super::reinforcement_ui::Form::new(&frame, "APM Station");
        let name = f.text("Name", "", "Empty: Station n");
        let at = f.text("Chainage (m)", "", "Of its middle; empty: click it beside the guideway in a plan");
        let platform = f.choice("Platform", &["Side platforms".to_owned(), "Centre platform (lanes far enough apart)".to_owned()], 0);
        let cars = f.text("Cars a train", "2", "1 to 6; the platform is the train and 6 m");
        let width = f.text("Platform width (m)", "4", "");
        let doors = f.check("Screen doors", "Platform screen doors", true);
        let ok = f.show("Platforms at the vehicle floor, 1 m above the running surface, with the train at them.", "Place");
        let mut args = json!({ "guideway_id": gid, "platform": if platform.get_selection() == Some(1) { "centre" } else { "side" }, "screen_doors": doors.is_checked() });
        if !name.get_value().trim().is_empty() {
            args["name"] = json!(name.get_value().trim());
        }
        if let Ok(v) = cars.get_value().trim().parse::<u32>() {
            args["cars"] = json!(v);
        }
        if let Ok(v) = width.get_value().trim().parse::<f64>() {
            args["platform_width"] = json!(v);
        }
        let typed = at.get_value().trim().parse::<f64>().ok();
        f.dialog.destroy();
        if !ok {
            return;
        }
        match typed {
            Some(s) => {
                args["station"] = json!(s);
                self.apm_station_run(args);
            }
            None => self.start_picking("apm.station", vec![json!({ "key": "at", "kind": "point", "prompt": "APM station: click where its middle is, beside the guideway; Esc ends" })], args),
        }
    }

    /// Places an APM station, its chainage from a clicked point when there is one.
    pub(crate) fn apm_station_run(&mut self, mut args: Value) {
        let frame = self.frame;
        if let Some(p) = args.as_object_mut().and_then(|o| o.remove("at")) {
            let gid = args["guideway_id"].clone();
            let line = self.doc.and_then(|d| self.app.document(d)).and_then(|d| {
                let store = d.doc.store();
                let g = store.resolve(&serde_json::from_value(gid).ok()?)?;
                let guid = riviera_core::apm::guideway(store.get(g)?)?.alignment;
                store.find_by_guid(&riviera_core::GlobalId::parse(&guid)?)
            });
            let Some(line) = line else { return error(&frame, "The guideway's alignment is gone.") };
            match self.run("alignment.station_of", json!({ "alignment_id": line, "point": p })) {
                Ok(v) => args["station"] = v["station"].clone(),
                Err(e) => return error(&frame, &e),
            }
        }
        match self.run("apm.station", args) {
            Ok(st) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("APM station at {:.1}: platform {:.1} m long", st["station"].as_f64().unwrap_or(0.0), st["platform_length"].as_f64().unwrap_or(0.0)), 0);
            }
            Err(e) => error(&frame, &format!("Could not place the station:\n{e}")),
        }
    }

    /// Architecture › Circulation › Ropeway (ARC-VT-027): a gondola line along the selected
    /// alignment, then what its check finds.
    pub(crate) fn place_ropeway(&mut self) {
        let frame = self.frame;
        let Some(line) = self.selected_where(|e| riviera_core::alignments::recipe(e).is_some()) else {
            return error(&frame, "Select the alignment the ropeway runs along first: straight legs, its corners becoming angle stations.");
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Ropeway");
        let cabin = f.text("Persons a cabin", "10", "");
        let speed = f.text("Line speed (m/s)", "6", "");
        let headway = f.text("Seconds between cabins", "15", "");
        let height = f.text("Tower height (m)", "18.3", "Above the ground");
        let spacing = f.text("Largest tower spacing (m)", "152", "");
        let gauge = f.text("Gauge (m)", "6.4", "Between the two directions' ropes");
        let ok = f.show("A monocable gondola: terminals at the ends, angle stations at corners, towers over the site's toposolids (or the level).", "Place");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let mut args = json!({ "alignment_id": line });
        for (k, t) in [("speed", &speed), ("headway", &headway), ("tower_height", &height), ("tower_spacing", &spacing), ("gauge", &gauge)] {
            if let Some(v) = num(t) {
                args[k] = json!(v);
            }
        }
        if let Some(c) = num(&cabin) {
            args["cabin_capacity"] = json!(c.max(1.0) as u32);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        let made = match self.run("ropeway.line", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Could not place the ropeway:\n{e}")),
        };
        self.refresh_scene(&frame);
        let id = made["ropeway"].clone();
        let found: Vec<String> = self
            .run("ropeway.check", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|x| x["ropeway"] == id)
            .filter_map(|x| x["message"].as_str().map(str::to_owned))
            .collect();
        let towers = made["supports"].as_array().map_or(0, |s| s.iter().filter(|x| x["kind"] == "tower").count());
        let head = format!(
            "Ropeway: {:.0} m, {towers} towers, {:.0} persons an hour each way, {:.0} s a trip",
            made["length"].as_f64().unwrap_or(0.0),
            made["capacity"].as_f64().unwrap_or(0.0),
            made["trip"].as_f64().unwrap_or(0.0)
        );
        let d = MessageDialog::builder(&frame, &format!("{head}\n\n{}", found.join("\n")), "Ropeway").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build();
        d.show_modal();
    }

    /// Architecture › Circulation › Fare Line (ARC-VT-022): a row of fare gates on this plan's
    /// level, then a click where it starts (or a typed point).
    pub(crate) fn place_fare_line(&mut self) {
        let frame = self.frame;
        if self.active_view().and_then(|v| v.plan.clone()).is_none() {
            return error(&frame, "Open a plan of the concourse first.");
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Fare Line");
        let kind = f.choice("Gates", &["Paddle gates (0.6 m, 50 a minute)".to_owned(), "Tripod turnstiles (0.5 m, 25 a minute)".to_owned(), "Wide gates (0.9 m, 72 a minute)".to_owned()], 0);
        let count = f.text("How many", "6", "Besides the wide and emergency gates");
        let wide = f.text("Wide gates", "1", "Accessible, 0.9 m clear; first along the line");
        let emergency = f.text("Emergency gates", "0", "Swing gates 1.0 m clear; last along the line");
        let angle = f.text("Runs at (°)", "90", "0: toward the right of the plan; counter-clockwise");
        let at = f.text("Start (x, y)", "", "Empty: click in the plan where it starts");
        let ok = f.show("Cabinets 0.3 × 1.6 m either side of each aisle. Paths of travel walk the aisles; Platform Clearing adds the wait at it.", "Place");
        let n = |t: &TextCtrl, d: u32| t.get_value().trim().parse::<u32>().unwrap_or(d);
        let gate = ["paddle", "tripod", "wide"][kind.get_selection().unwrap_or(0) as usize];
        let args = json!({
            "command": "fare_line.create",
            "kind": gate,
            "count": n(&count, 6),
            "wide": n(&wide, 1),
            "emergency": n(&emergency, 0),
            "angle": angle.get_value().trim().parse::<f64>().unwrap_or(90.0).to_radians(),
        });
        let typed: Vec<f64> = at.get_value().split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else { return };
        if let [x, y] = typed[..] {
            let mut args = args;
            args["point"] = json!([x, y]);
            args["level"] = json!(plan.level);
            return self.layout_bathroom_at(args);
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Fare line: click where it starts; Esc ends", 0);
        }
    }

    /// After `escalator.create`: what was made, and what Check Escalators finds.
    pub(crate) fn escalator_made(&mut self, made: &Value) {
        let frame = self.frame;
        self.refresh_scene(&frame);
        let problems = self
            .run("escalator.check", json!({ "escalators": [made["escalator"]] }))
            .ok()
            .and_then(|v| v.as_array().map(|a| a.iter().filter(|f| f["severity"] == "error").count()))
            .unwrap_or(0);
        let f = |k: &str| made[k].as_f64().unwrap_or(0.0);
        frame.set_status_text(
            &format!(
                "{}: {:.0} mm at {:.1}°, rise {:.2} m, {:.2} m long, {} flat steps, about {:.0} persons/h, motor about {:.1} kW{}; Check Escalators finds {problems} error(s)",
                if made["kind"] == "moving_walk" { "Moving walk" } else { "Escalator" },
                f("step_width") * 1000.0,
                f("inclination"),
                f("rise"),
                f("lower_end") + f("run") + f("upper_end"),
                made["flat_steps"],
                f("capacity"),
                f("motor_kw"),
                if made["intermediate_support"] == true { "; needs an intermediate support" } else { "" },
            ),
            0,
        );
    }

    /// Architecture › Circulation › Check Escalators (ARC-VT-012).
    pub(crate) fn check_escalators(&mut self) {
        let frame = self.frame;
        let found = match self.run("escalator.check", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        if found.is_empty() {
            let d = MessageDialog::builder(&frame, "No problems found.\nHeadroom, side clearance and landing zones are checked by Check Clearances.", "Check Escalators")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .map(|(k, f)| format!("{}. {}: {} — {} [{}]", k + 1, if f["severity"] == "note" { "Note" } else { "Error" }, f["pack"].as_str().unwrap_or_default(), f["message"].as_str().unwrap_or_default(), f["confidence"].as_str().unwrap_or_default()))
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let dialog = SingleChoiceDialog::builder(&frame, "Pick one to select and frame it:", "Check Escalators", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(f) = picked.and_then(|i| found.get(i)) else {
            return;
        };
        let _ = self.run("selection.set", json!({ "ids": [f["escalator"]] }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(&format!("{} — {} ({})", f["rule"].as_str().unwrap_or_default(), f["edition"].as_str().unwrap_or_default(), f["source"].as_str().unwrap_or_default()), 0);
    }

    /// Architecture › Circulation › Stairlift (ARC-VT-013): a seated stairlift or a wheelchair
    /// platform lift on the selected stair.
    pub(crate) fn place_stairlift(&mut self) {
        let frame = self.frame;
        let stair = {
            let selected = self.selection();
            let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store()) else {
                return;
            };
            selected.into_iter().find(|id| store.get(*id).is_some_and(|e| riviera_core::stairs::stair_recipe(e).is_some()))
        };
        let Some(stair) = stair else {
            return error(&frame, "Select the stair for the stairlift first.");
        };
        let sizes = ["800 × 1250 (straight)", "800 × 1000 (straight)", "750 × 900 (straight)", "725 × 1000 (straight)", "675 × 1000 (straight)", "800 × 1220 (curved)", "800 × 1050 (curved)", "800 × 900 (curved)", "700 × 750 (curved)"];
        let mut platforms = vec!["The family's largest".to_owned()];
        platforms.extend(sizes.iter().map(|s| (*s).to_owned()));
        let f = super::reinforcement_ui::Form::new(&frame, "Stairlift");
        let kind = f.choice("Kind", &["Seated stairlift".to_owned(), "Wheelchair platform lift".to_owned()], 0);
        let side = f.choice("Side, going up", &["Left".to_owned(), "Right".to_owned()], 0);
        let platform = f.choice("Platform (mm)", &platforms, 0);
        let family = f.choice("Rail", &["Straight, or curved where the stair turns".to_owned(), "Straight".to_owned(), "Curved".to_owned()], 0);
        let usage = f.choice("Stair", &["Other (keeps its required width)".to_owned(), "In a dwelling (R-3, or within an R-2 unit)".to_owned(), "In an exit enclosure".to_owned()], 0);
        let occupants = f.text("People served", "", "Fewer than 50 lets a stair keep the smaller required width");
        let parks = f.choice("Parks", &["At the foot of the stair".to_owned(), "Clear of the stair (hinged or slide-track rail end, or round a corner)".to_owned()], 0);
        let ok = f.show("Sized under the vertical-transport rule packs in use. Its running envelope, head box and landings show as virtual volumes.", "Place");
        let stair_use = ["other", "dwelling", "exit"][usage.get_selection().unwrap_or(0) as usize];
        let mut args = json!({
            "stair": stair,
            "side": if side.get_selection() == Some(1) { "right" } else { "left" },
            "kind": if kind.get_selection() == Some(1) { "platform" } else { "seated" },
            "stair_use": stair_use,
            "parks_clear": parks.get_selection() == Some(1),
        });
        if let Ok(n) = occupants.get_value().trim().parse::<u32>() {
            args["occupants"] = json!(n);
        }
        if kind.get_selection() == Some(1) {
            let pick = platform.get_selection().unwrap_or(0) as usize;
            if let Some(label) = pick.checked_sub(1).and_then(|k| sizes.get(k)) {
                let mm: Vec<f64> = label.split([' ', '×']).filter_map(|t| t.parse().ok()).collect();
                args["platform"] = json!([mm[0] / 1000.0, mm[1] / 1000.0]);
                args["family"] = json!(if label.contains("curved") { "curved" } else { "straight" });
            } else if let Some(k) = family.get_selection().filter(|k| *k > 0) {
                args["family"] = json!(if k == 2 { "curved" } else { "straight" });
            }
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        let made = match self.run("stairlift.create", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("No stairlift was placed:\n{e}")),
        };
        self.refresh_scene(&frame);
        let found = self.run("stairlift.check", json!({ "stairlifts": [made["stairlift"]] })).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let errors = found.iter().filter(|x| x["severity"] == "error").count();
        let mm = |k: &str| made[k].as_f64().unwrap_or(0.0) * 1000.0;
        let what = match made["platform"].as_array() {
            Some(p) => format!("{} platform lift, {:.0} × {:.0} mm", made["family"].as_str().unwrap_or_default(), p[0].as_f64().unwrap_or(0.0) * 1000.0, p[1].as_f64().unwrap_or(0.0) * 1000.0),
            None => "Seated stairlift".to_owned(),
        };
        frame.set_status_text(
            &format!("{what}: {:.0} mm folded, {:.0} mm in use, {:.2} m rail at {:.1}°; Check Stairlifts finds {errors} error(s) and {} note(s)", mm("folded"), mm("unfolded"), made["rail_length"].as_f64().unwrap_or(0.0), made["incline"].as_f64().unwrap_or(0.0), found.len() - errors),
            0,
        );
    }

    /// Architecture › Circulation › Check Stairlifts (ARC-VT-015).
    pub(crate) fn check_stairlifts(&mut self) {
        let frame = self.frame;
        let found = match self.run("stairlift.check", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        if found.is_empty() {
            let d = MessageDialog::builder(&frame, "No problems found.\nRunning envelopes, head boxes and landings are checked by Check Clearances.", "Check Stairlifts")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .map(|(k, f)| format!("{}. {}: {} — {} [{}]", k + 1, if f["severity"] == "note" { "Note" } else { "Error" }, f["pack"].as_str().unwrap_or_default(), f["message"].as_str().unwrap_or_default(), f["confidence"].as_str().unwrap_or_default()))
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let dialog = SingleChoiceDialog::builder(&frame, "Pick one to select and frame it:", "Check Stairlifts", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(f) = picked.and_then(|i| found.get(i)) else {
            return;
        };
        let _ = self.run("selection.set", json!({ "ids": [f["stairlift"]] }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(&format!("{} — {} ({}, {})", f["rule"].as_str().unwrap_or_default(), f["description"].as_str().unwrap_or_default(), f["edition"].as_str().unwrap_or_default(), f["source"].as_str().unwrap_or_default()), 0);
    }

    /// Architecture › Circulation › Elevator Traffic (ARC-VT-018): each group's up-peak against
    /// the targets for the building's use.
    pub(crate) fn elevator_traffic(&mut self) {
        let frame = self.frame;
        let uses = ["office", "residential", "hotel", "hospital", "clinic"];
        let f = super::reinforcement_ui::Form::new(&frame, "Elevator Traffic");
        let usage = f.choice("Use", &["Office".to_owned(), "Residential".to_owned(), "Hotel".to_owned(), "Hospital".to_owned(), "Outpatient clinic".to_owned()], 0);
        let population = f.text("People above the lobby", "", "Empty: estimated from the floors served (offices only)");
        let area = f.text("Floor area per person (m²)", "", "Empty: the rule pack's (10 m² for offices)");
        let ok = f.show("Each group is the passenger elevators serving the same levels at the same speed with the same car. The up-peak is a planning check; its coefficients are flagged for expert review.", "Calculate");
        let building_use = uses[usage.get_selection().unwrap_or(0) as usize];
        let mut args = json!({ "building_use": building_use });
        if let Ok(n) = population.get_value().trim().parse::<f64>() {
            args["population"] = json!(n);
        }
        if let Ok(a) = area.get_value().trim().parse::<f64>() {
            args["area_per_person"] = json!(a);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        let groups = match self.run("elevator.traffic", args) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The traffic was not calculated:\n{e}")),
        };
        if groups.is_empty() {
            return error(&frame, "There are no passenger elevators serving two levels or more.");
        }
        let n = |v: &Value| v.as_f64().unwrap_or(0.0);
        let text: Vec<String> = groups
            .iter()
            .enumerate()
            .map(|(k, g)| {
                format!(
                    "Group {}: {} car(s), {} floors above the lobby, {:.1} m/s\n  {:.0} people{}; {:.1} passengers a trip ({} by load, {} by area)\n  round trip {:.0} s, interval {:.1} s (target {:.0} s), waiting about {:.0}–{:.0} s\n  {:.0} people in 5 minutes = {:.1} % (target {:.1} %): {}{}",
                    k + 1,
                    g["cars"],
                    g["floors"],
                    n(&g["speed"]),
                    n(&g["population"]),
                    if g["estimated"] == true { " (estimated)" } else { "" },
                    n(&g["passengers"]),
                    g["persons_by_mass"],
                    g["persons_by_area"],
                    n(&g["rtt"]),
                    n(&g["interval"]),
                    n(&g["target_interval"]),
                    n(&g["waiting"][0]),
                    n(&g["waiting"][1]),
                    n(&g["hc5"]),
                    n(&g["handling"]) * 100.0,
                    n(&g["target_handling"]) * 100.0,
                    if g["meets"] == true { "meets the targets" } else { "does not meet the targets" },
                    g["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(|s| format!("\n  note: {s}")).collect::<String>()).unwrap_or_default(),
                )
            })
            .collect();
        let d = MessageDialog::builder(&frame, &text.join("\n\n"), "Elevator Traffic").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build();
        d.show_modal();
    }
}
