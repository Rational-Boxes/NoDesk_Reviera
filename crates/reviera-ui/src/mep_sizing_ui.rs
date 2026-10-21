// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Duct/Pipe Sizing (MEP-CALC-001, MEP-PIPE-005): the method (Equal Friction,
//! Friction, Velocity, or both with And or Or), its targets, the branch rule, height, width and
//! diameter restrictions and the friction equation, for the systems of the selection; the report
//! (segments sized, skipped, warnings) afterwards. Import Fixture-Unit Table reads a CSV of
//! `fixture_units, flow_tank, flow_valve` rows.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

fn num(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Gui {
    /// Analyze › Duct/Pipe Sizing for the selection's systems (or its segments).
    pub(crate) fn size_runs(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        if sel.is_empty() {
            return frame.set_status_text("Duct/Pipe Sizing: select segments of a system, or the system", 0);
        }
        let duct = sel.iter().any(|i| {
            store.get(*i).and_then(reviera_core::mep_runs::segment_recipe).is_some_and(|s| s.domain == reviera_core::mep::Domain::Duct)
        });
        let systems: Vec<reviera_core::ElementId> =
            reviera_core::mep_systems::systems(&store).into_iter().filter(|s| s.elements.iter().any(|e| sel.contains(e)) || sel.contains(&s.id)).map(|s| s.id).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Duct/Pipe Sizing");
        let methods = [("equal_friction", "Equal Friction"), ("friction", "Friction"), ("velocity", "Velocity"), ("friction_velocity", "Friction and Velocity")];
        let method = f.choice("Method", &methods.iter().map(|m| m.1.to_owned()).collect::<Vec<_>>(), 0);
        let friction = f.text("Friction (Pa/m)", if duct { "1" } else { "300" }, "At or below");
        let velocity = f.text("Velocity (m/s)", if duct { "8" } else { "1.5" }, "At or below");
        let combine = f.choice("Combine", &["And".to_owned(), "Or".to_owned()], 0);
        let rules = [("calculated_only", "Calculated size only"), ("match_connector", "Match connector size"), ("larger_of", "Larger of the two")];
        let branch = f.choice("Branches to terminals", &rules.iter().map(|r| r.1.to_owned()).collect::<Vec<_>>(), 0);
        let height = f.text("Restrict height (mm)", "", "Rectangular ducts at most this high; empty for none");
        let width = f.text("Restrict width (mm)", "", "Rectangular ducts at most this wide; empty for none");
        let diameter = f.text("Restrict diameter (mm)", "", "Round ducts and pipes at most this size; empty for none");
        let eq = f.choice("Friction equation", &["Colebrook".to_owned(), "Haaland".to_owned()], 0);
        let valves = f.check("Domestic water", "Flush valves (the flush-valve curve)", false);
        let ok = f.show("The selection's systems are sized from their equipment; size-locked segments are left as they are.", "Size");
        let mm = |t: &TextCtrl| num(t).map(|v| v / 1000.0);
        let mut args = json!({
            "method": methods[method.get_selection().map_or(0, |k| k as usize).min(3)].0,
            "targets": { "friction": num(&friction), "velocity": num(&velocity) },
            "combine": if combine.get_selection() == Some(1) { "or" } else { "and" },
            "branch_rule": rules[branch.get_selection().map_or(0, |k| k as usize).min(2)].0,
            "restrict": { "height": mm(&height), "width": mm(&width), "inner_diameter": mm(&diameter) },
            "equation": if eq.get_selection() == Some(1) { "haaland" } else { "colebrook" },
            "flush_valves": valves.get_value(),
        });
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut sized = 0;
        let mut skipped = 0;
        let mut warnings: Vec<String> = Vec::new();
        let targets: Vec<Value> = if systems.is_empty() { vec![Value::Null] } else { systems.iter().map(|s| json!(s)).collect() };
        for s in targets {
            if s.is_null() {
                args["element_ids"] = json!(sel);
            } else {
                args["system_id"] = s;
            }
            match self.run("mep.size", args.clone()) {
                Ok(r) => {
                    sized += r["sized"].as_array().map_or(0, |v| v.iter().filter(|x| x["changed"] == true).count());
                    skipped += r["skipped"].as_array().map_or(0, Vec::len);
                    warnings.extend(r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_owned)));
                    if let Some(c) = r["conversion"].as_str() {
                        warnings.push(format!("Domestic flow: {c}"));
                    }
                }
                Err(e) => return error(&frame, &format!("Sizing stopped:\n{e}")),
            }
        }
        self.refresh_scene(&frame);
        let text = format!("{sized} segment(s) resized, {skipped} size-locked skipped");
        if warnings.is_empty() {
            frame.set_status_text(&text, 0);
        } else {
            error(&frame, &format!("{text}.\n\n{}", warnings.join("\n")));
        }
    }

    /// Manage › MEP Settings › Import Fixture-Unit Table.
    pub(crate) fn import_fixture_table(&mut self) {
        let frame = self.frame;
        let d = FileDialog::builder(&frame)
            .with_message("Import Fixture-Unit Table")
            .with_wildcard("CSV (*.csv;*.txt)|*.csv;*.txt|All files (*.*)|*.*")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        let csv = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return error(&frame, &format!("{path}: {e}")),
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Fixture-Unit Table");
        let stem = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("Fixture units").to_owned();
        let name = f.text("Name", &stem, "");
        let source = f.text("Source", "", "The code and edition the values come from (shown in sizing reports)");
        let unit = f.choice("Flow unit", &["l/s".to_owned(), "gpm".to_owned()], 0);
        let ok = f.show("Rows: fixture_units, flow_tank, flow_valve, ascending.", "Import");
        let args = json!({ "name": name.get_value(), "source": source.get_value(), "csv": csv, "unit": if unit.get_selection() == Some(1) { "gpm" } else { "l/s" } });
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("fixture_unit_method.set", args) {
            Ok(_) => frame.set_status_text("Fixture-unit table imported; domestic water is sized with it", 0),
            Err(e) => error(&frame, &format!("The table was not imported:\n{e}")),
        }
    }
}

const SELECT: i32 = 7301;
const RULES: i32 = 7302;

impl Gui {
    /// Systems › Penetrations: where runs pass through walls, floors, ceilings and beams; the
    /// rules.
    pub(crate) fn penetrations(&mut self) {
        let frame = self.frame;
        loop {
            let list = match self.run("mep.penetrations", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|c| {
                    let p = &c["point"];
                    vec![
                        c["run"].to_string(),
                        c["size"].as_str().unwrap_or("").to_owned(),
                        format!("{} {}", c["host_kind"].as_str().unwrap_or(""), c["host"]),
                        match c["mode"].as_str() {
                            Some("create") => "Opened".to_owned(),
                            Some("flag") => "Flagged (not opened)".to_owned(),
                            other => other.unwrap_or("").to_owned(),
                        },
                        format!("{:.2}, {:.2}, {:.2}", p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0), p[2].as_f64().unwrap_or(0.0)),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Penetrations",
                "Openings are made where runs pass through walls, floors and ceilings, and follow them; crossings of beams are flagged for an engineer, not opened.",
                &[("Run", 70), ("Size", 80), ("Host", 140), ("Treatment", 160), ("At (m)", 160)],
                &rows,
                &[(SELECT, "Select", "Select the run and its host"), (RULES, "Rules…", "Create, flag or never, and the clearance, per host")],
                None,
            );
            match (code, at.and_then(|k| list.get(k))) {
                (SELECT, Some(c)) => {
                    let ids: Vec<reviera_core::ElementId> =
                        [&c["run"], &c["host"]].iter().filter_map(|v| v.as_u64()).map(reviera_core::ElementId).collect();
                    self.select_ids(&ids, super::select::Combine::Replace);
                    return;
                }
                (RULES, _) => self.penetration_rules(),
                (SELECT, None) => {}
                _ => return,
            }
        }
    }

    fn penetration_rules(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Penetration Rules");
        let modes = ["Create an opening".to_owned(), "Flag only".to_owned(), "Never".to_owned()];
        let wall = f.choice("Walls", &modes, 0);
        let floor = f.choice("Floors", &modes, 0);
        let ceiling = f.choice("Ceilings", &modes, 0);
        let clearance = f.text("Clearance (mm)", "25", "Around the run's section, for every host");
        let ok = f.show("Beams are always flagged, not opened. Rules per system classification: mep.set_penetration_rule.", "OK");
        let pick = |c: &Choice| ["create", "flag", "never"][c.get_selection().map_or(0, |k| k as usize).min(2)];
        let (w, fl, ce, cl) = (pick(&wall), pick(&floor), pick(&ceiling), num(&clearance).map(|v| v / 1000.0));
        f.dialog.destroy();
        if !ok {
            return;
        }
        for (host, mode) in [("wall", w), ("floor", fl), ("ceiling", ce)] {
            if let Err(e) = self.run("mep.set_penetration_rule", json!({ "host_category": host, "mode": mode, "clearance": cl })) {
                return error(&frame, &e);
            }
        }
        self.refresh_scene(&frame);
    }
}
