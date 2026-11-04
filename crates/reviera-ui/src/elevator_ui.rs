// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

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
        let speed = f.text("Speed (m/s)", "0.5", "");
        let flat = f.text("Flat steps", "", "Empty: the least the rule packs ask for the rise and speed");
        let duty = f.choice("Duty", &["Commercial".to_owned(), "Transit (heavy duty)".to_owned()], 0);
        let length = f.text("Walk length (m)", "", "A horizontal walk's length between its working points");
        let pit = f.choice("Horizontal walk", &["On the floor".to_owned(), "In a pit, flush".to_owned()], 0);
        let rotation = f.text("Rotation (°)", "0", "0: it rises toward the right of the plan; counter-clockwise");
        let at = f.text("Lower end point (x, y)", "", "Empty: click in the plan at the middle of its lower end");
        let ok = f.show("Sized under the vertical-transport rule packs in use, with floor openings for its pits and headroom. Then click the middle of its lower end.", "Place");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let step = [0.6, 0.8, 1.0, 1.2, 1.4][width.get_selection().unwrap_or(2) as usize];
        let mut args = json!({
            "command": "escalator.create",
            "kind": if kind.get_selection() == Some(1) { "moving_walk" } else { "escalator" },
            "step_width": step,
            "rotation": num(&rotation).unwrap_or(0.0).to_radians(),
            "duty": if duty.get_selection() == Some(1) { "transit" } else { "commercial" },
            "pit": pit.get_selection() == Some(1),
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
            selected.into_iter().find(|id| store.get(*id).is_some_and(|e| reviera_core::stairs::stair_recipe(e).is_some()))
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
