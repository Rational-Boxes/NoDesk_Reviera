// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architecture › Circulation › Elevator (ARC-VT-002): the elevator's capacity, drive and the
//! levels it serves, then a click in a plan at the middle of its entrance (or a typed point).
//! Check Elevators (ARC-VT-004) lists `elevator.check`'s findings and selects and frames the
//! elevator picked.

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
        let top = f.choice("Serves up to", &names, names.len() - 1);
        let load = f.text("Rated load (kg)", "", "Empty: the smallest car that meets the accessibility rule packs and, where needed, takes the stretcher");
        let drive = f.choice("Drive", &["Traction, no machine room".to_owned(), "Traction, machine room".to_owned(), "Hydraulic".to_owned(), "Holeless hydraulic".to_owned()], 0);
        let speed = f.text("Speed (m/s)", "1.0", "");
        let door = f.choice("Doors", &["As the car comes".to_owned(), "Side opening".to_owned(), "Centre opening".to_owned()], 0);
        let rotation = f.text("Rotation (°)", "0", "0: the landing is toward the bottom of the plan; counter-clockwise");
        let walls = f.choice("Hoistway walls", &["Yes".to_owned(), "No".to_owned()], 0);
        let at = f.text("Entrance point (x, y)", "", "Empty: click in the plan at the middle of the entrance");
        let ok = f.show(
            &format!("Sized under the vertical-transport rule packs in use, from {} up; the shaft cuts the floors between. Then click the middle of its entrance.", plan.level_name),
            "Place",
        );
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let drive = ["mrl_traction", "traction", "hydraulic", "holeless"][drive.get_selection().unwrap_or(0) as usize];
        let mut args = json!({
            "command": "elevator.create",
            "top_level": above[top.get_selection().unwrap_or(0) as usize]["id"],
            "drive": drive,
            "rotation": num(&rotation).unwrap_or(0.0).to_radians(),
            "walls": walls.get_selection() != Some(1),
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
                "Elevator: {} kg, {} persons; car {:.0} × {:.0} mm; hoistway {:.0} × {:.0} mm, pit {:.0} mm, overhead {:.0} mm ({}); Check Elevators finds {problems} error(s)",
                made["load"],
                made["persons"],
                mm(&made["car"][0]),
                mm(&made["car"][1]),
                mm(&made["hoistway"][0]),
                mm(&made["hoistway"][1]),
                mm(&made["pit"]),
                mm(&made["overhead"]),
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
}
