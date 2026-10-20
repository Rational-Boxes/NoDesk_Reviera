// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Systems › Electrical › Circuit Properties (MEP-ELEC-004, 005): a circuit's calculated values
//! (load, power factor, current, phases, path length, cable, voltage drop, ampacity, warnings)
//! and its editable ones (load name, rating, cable type, conductor size or Auto, runs, path
//! mode and offset, voltage drop limit). Manage › MEP Settings › Wiring (MEP-SET-002, 003): the
//! default cable type, voltage drop limit, correction factor, path offset and the ampacity
//! table, entered one row per line.

use reviera_core::mep_electrical::{self, CircuitInfo};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

fn num(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The calculated values as text.
fn summary(p: &Value) -> String {
    let f = |k: &str| p[k].as_f64().unwrap_or(0.0);
    let mut out = vec![
        format!("{} — {}", p["name"].as_str().unwrap_or(""), p["load_name"].as_str().unwrap_or("")),
        format!("Phases {}; {} element(s)", p["phases"].as_str().unwrap_or("–"), p["elements"]),
        format!("Load {:.0} VA at power factor {:.2}; design current {:.2} A", f("apparent_load"), f("power_factor"), f("current")),
        format!("Path length {:.2} m", f("length")),
        format!(
            "Cable {} {}{} × {}",
            p["cable_type"].as_str().unwrap_or(""),
            p["cable_size"].as_str().unwrap_or("(none)"),
            if p["cable_size_auto"].as_bool() == Some(true) { " (sized)" } else { " (given)" },
            p["runs"]
        ),
        format!("Voltage drop {:.2} V, {:.2} % (limit {} %)", f("voltage_drop"), f("voltage_drop_percent"), f("max_voltage_drop_percent")),
    ];
    if let Some(a) = p["ampacity"].as_f64() {
        out.push(format!("Ampacity {a:.1} A after correction"));
    }
    for w in p["warnings"].as_array().into_iter().flatten() {
        out.push(format!("⚠ {}", w.as_str().unwrap_or("")));
    }
    out.join("\n")
}

impl Gui {
    /// Systems › Electrical › Circuit Properties for a circuit of the selection.
    pub(crate) fn circuit_properties(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let keep = |c: &CircuitInfo| sel.is_empty() || sel.iter().any(|i| c.elements.contains(i) || c.id == *i || c.panel == Some(*i));
        let Some(c) = self.choose_circuit("Circuit Properties", &keep) else { return };
        let p = match self.run("circuit.properties", json!({ "circuit_id": c.id })) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let Some(store) = self.store_now() else { return };
        let s = mep_electrical::settings(&store);
        let rec = store.get(c.id).and_then(mep_electrical::recipe);
        let f = super::reinforcement_ui::Form::new(&frame, "Circuit Properties");
        let info = f.lines("Calculated", &summary(&p), "Recalculated whenever the circuit, its devices or the settings change");
        info.set_min_size(Size::new(420, 170));
        let name = f.text("Load name", &c.load_name, "Empty: made from the loads");
        let rating = f.text("Rating (A)", &c.rating.map_or_else(String::new, |r| format!("{r}")), "The protective device; empty for none");
        let types: Vec<(String, String)> = s.conductors.cable_types.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
        let current_type = rec.as_ref().and_then(|r| r.cable_type.clone()).unwrap_or_else(|| s.wiring.default_cable_type.clone());
        let mut type_labels = vec![format!("Project default ({})", types.iter().find(|t| t.0 == s.wiring.default_cable_type).map_or("", |t| t.1.as_str()))];
        type_labels.extend(types.iter().map(|t| t.1.clone()));
        let type_at = rec.as_ref().and_then(|r| r.cable_type.as_ref()).and_then(|id| types.iter().position(|t| &t.0 == id)).map_or(0, |k| k + 1);
        let cable_type = f.choice("Cable type", &type_labels, type_at);
        let system = s.conductors.cable_types.iter().find(|t| t.id == current_type).map(|t| t.system).unwrap_or_default();
        let sizes: Vec<(String, String)> = s.conductors.sizes.iter().filter(|z| z.system == system).map(|z| (z.id.clone(), z.name.clone())).collect();
        let mut size_labels = vec!["Auto (sized)".to_owned()];
        size_labels.extend(sizes.iter().map(|z| z.1.clone()));
        let size_at = rec.as_ref().and_then(|r| r.cable_size.as_ref()).and_then(|id| sizes.iter().position(|z| &z.0 == id)).map_or(0, |k| k + 1);
        let size = f.choice("Conductor size", &size_labels, size_at);
        let runs = f.text("Parallel sets", &p["runs"].to_string(), "1 to 20");
        let modes = [("farthest_device", "Farthest Device"), ("all_devices", "All Devices"), ("custom", "Custom (as set by command)")];
        let mode_at = modes.iter().position(|m| p["path_mode"] == m.0).unwrap_or(0);
        let mode = f.choice("Path", &modes.iter().map(|m| m.1.to_owned()).collect::<Vec<_>>(), mode_at);
        let offset = f.text("Path offset (m)", &format!("{}", p["path_offset"].as_f64().unwrap_or(0.0)), "Horizontal runs this far above the higher end (negative: below the lower)");
        let max_vd = f.text("Voltage drop limit (%)", &format!("{}", p["max_voltage_drop_percent"].as_f64().unwrap_or(3.0)), "For this circuit");
        let ok = f.show("Sizing picks the smallest conductor within the voltage drop limit that carries the rating.", "OK");
        let type_sel = cable_type.get_selection().map_or(0, |k| k as usize);
        let size_sel = size.get_selection().map_or(0, |k| k as usize);
        let args = json!({
            "circuit_id": c.id,
            "load_name": name.get_value(),
            "rating": num(&rating).unwrap_or(0.0),
            "cable_type": type_sel.checked_sub(1).and_then(|k| types.get(k)).map_or(String::new(), |t| t.0.clone()),
            "cable_size": size_sel.checked_sub(1).and_then(|k| sizes.get(k)).map_or(String::new(), |z| z.0.clone()),
            "runs": num(&runs).map_or(1, |v| v.round().max(1.0) as u32),
        });
        let new_mode = modes[mode.get_selection().map_or(0, |k| k as usize).min(2)].0;
        let new_offset = num(&offset);
        let new_vd = num(&max_vd);
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut result = self.run("circuit.set", args);
        if result.is_ok() && c.panel.is_some() && (new_mode != p["path_mode"].as_str().unwrap_or("") || new_offset != p["path_offset"].as_f64()) && new_mode != "custom" {
            result = self.run("circuit.set_path", json!({ "circuit_id": c.id, "mode": new_mode, "offset": new_offset }));
        }
        if result.is_ok() && new_vd.is_some() && new_vd != p["max_voltage_drop_percent"].as_f64() && size_sel == 0 {
            result = self.run("circuit.size_cable", json!({ "circuit_id": c.id, "max_vd_percent": new_vd }));
        }
        match result {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                if let Ok(p) = self.run("circuit.properties", json!({ "circuit_id": c.id })) {
                    frame.set_status_text(
                        &format!(
                            "{}: {} {}, {:.2} % voltage drop",
                            p["name"].as_str().unwrap_or(""),
                            p["cable_type"].as_str().unwrap_or(""),
                            p["cable_size"].as_str().unwrap_or(""),
                            p["voltage_drop_percent"].as_f64().unwrap_or(0.0)
                        ),
                        0,
                    );
                }
            }
            Err(e) => error(&frame, &format!("The circuit was not changed:\n{e}")),
        }
    }

    /// Manage › MEP Settings › Wiring.
    pub(crate) fn wiring_settings(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let s = mep_electrical::settings(&store);
        let f = super::reinforcement_ui::Form::new(&frame, "Wiring Settings");
        let types: Vec<(String, String)> = s.conductors.cable_types.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
        let at = types.iter().position(|t| t.0 == s.wiring.default_cable_type).unwrap_or(0);
        let ct = f.choice("Default cable type", &types.iter().map(|t| t.1.clone()).collect::<Vec<_>>(), at);
        let vd = f.text("Voltage drop limit (%)", &format!("{}", s.wiring.max_voltage_drop_percent), "Circuits are sized to stay within it");
        let cf = f.text("Correction factor", &format!("{}", s.wiring.correction_factor), "Applied to ampacities: ambient temperature and grouping together");
        let off = f.text("Path offset (m)", &format!("{}", s.wiring.path_offset), "Circuit paths' horizontal runs above the higher end");
        let table: Vec<String> = s.conductors.ampacities.iter().map(|a| format!("{}, {}, {}, {}", a.material, a.insulation, a.size, a.amperes)).collect();
        let amp = f.lines(
            "Ampacities",
            &table.join("\n"),
            "One row per line: material, insulation, size, amperes (for example copper, pvc, 2_5mm2, 24). Reviera ships none: enter your code's values. Clear a line to remove it.",
        );
        let ok = f.show("Sizes: 1mm2, 1_5mm2, 2_5mm2 … 300mm2; 14_awg … 4_0_awg, 250_kcmil … 500_kcmil.", "OK");
        let (ct_v, vd_v, cf_v, off_v, amp_v) = (ct.get_selection().map_or(0, |k| k as usize), num(&vd), num(&cf), num(&off), amp.get_value());
        f.dialog.destroy();
        if !ok {
            return;
        }
        let mut rows = Vec::new();
        for line in amp_v.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            match (parts.as_slice(), parts.get(3).and_then(|a| a.parse::<f64>().ok())) {
                ([m, i, z, _], Some(a)) => rows.push(json!({ "material": m, "insulation": i, "size": z, "amperes": a })),
                _ => return error(&frame, &format!("{line:?}: write material, insulation, size, amperes")),
            }
        }
        // Rows no longer listed are removed.
        for a in &s.conductors.ampacities {
            if !rows.iter().any(|r| r["material"] == a.material.as_str() && r["insulation"] == a.insulation.as_str() && r["size"] == a.size.as_str()) {
                rows.push(json!({ "material": a.material, "insulation": a.insulation, "size": a.size, "amperes": 0.0 }));
            }
        }
        let mut w = json!({ "default_cable_type": types.get(ct_v).map(|t| t.0.clone()) });
        if let Some(v) = vd_v {
            w["max_voltage_drop_percent"] = json!(v);
        }
        if let Some(v) = cf_v {
            w["correction_factor"] = json!(v);
        }
        if let Some(v) = off_v {
            w["path_offset"] = json!(v);
        }
        let r = self.run("wiring.set", w).and_then(|_| self.run("conductor_ampacity.set", json!({ "rows": rows })));
        match r {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text("Wiring settings saved; circuits are sized again", 0);
            }
            Err(e) => error(&frame, &format!("The wiring settings were not saved:\n{e}")),
        }
    }
}
