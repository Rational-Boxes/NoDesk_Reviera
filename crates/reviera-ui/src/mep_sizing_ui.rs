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
