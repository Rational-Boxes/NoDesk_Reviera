// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › MEP Settings › Mechanical Settings (MEP-SET-001): hidden lines, duct and pipe
//! annotation, air properties, angles, conversion, slopes and calculation in one form; OK writes
//! each changed branch through `mechanical_settings.set`, checked first. Duct Sizes edits a
//! shape's size table (each size in the size lists, in sizing, or both); Pipe Segment and Fluid
//! make a segment or a fluid.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::reinforcement_ui::Form;
use super::{Gui, error};

/// Angles as typed: "Any", "every 15" (an increment) or "45, 90" (a list).
fn angles_text(v: &Value) -> String {
    match v["mode"].as_str() {
        Some("increment") => format!("every {}", v["step"].as_f64().unwrap_or(0.0)),
        Some("list") => v["angles"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|a| a.to_string()).collect::<Vec<_>>().join(", "),
        _ => "Any".into(),
    }
}

fn parse_angles(t: &str) -> Result<Value, String> {
    let t = t.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("any") {
        return Ok(json!({ "mode": "any" }));
    }
    let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().map_err(|_| format!("{s:?} is not an angle"));
    if let Some(rest) = t.strip_prefix("every") {
        return Ok(json!({ "mode": "increment", "step": num(rest.trim_end_matches('°'))? }));
    }
    let angles = t.split([',', ';']).map(|s| num(s.trim().trim_end_matches('°'))).collect::<Result<Vec<_>, _>>()?;
    Ok(json!({ "mode": "list", "angles": angles }))
}

fn num(t: &TextCtrl) -> Result<f64, String> {
    t.get_value().trim().replace(',', ".").parse::<f64>().map_err(|_| format!("{:?} is not a number", t.get_value()))
}

const EQUATIONS: [(&str, &str); 3] = [("Colebrook", "colebrook"), ("Haaland", "haaland"), ("Altshul–Tsal", "altshul_tsal")];

impl Gui {
    /// Manage › MEP Settings › Mechanical Settings.
    pub(crate) fn mechanical_settings(&mut self) {
        let frame = self.frame;
        let Ok(mut t) = self.run("mechanical_settings.get", json!({})) else {
            return frame.set_status_text("Open a model first", 0);
        };
        let before = t.clone();
        let f = Form::new(&frame, "Mechanical Settings");
        let h = &t["hidden_line"];
        let hidden = f.check("Hidden lines", "Draw MEP hidden lines", h["draw"].as_bool().unwrap_or(true));
        let gap = |label: &str, v: &Value, tip: &str| f.text(label, &v.as_f64().unwrap_or(0.0).to_string(), tip);
        let inside = gap("Inside gap (mm)", &h["inside_gap"], "On paper, inside a crossing");
        let outside = gap("Outside gap (mm)", &h["outside_gap"], "On paper, outside a crossing");
        let single = gap("Single-line gap (mm)", &h["single_line_gap"], "On paper, for single lines");
        let d = &t["duct"];
        let density = gap("Air density (kg/m³)", &d["air_density"], "Used by duct sizing and pressure drop");
        let viscosity = gap("Air viscosity (Pa·s)", &d["air_viscosity"], "Dynamic viscosity");
        let fitting = gap("Duct fitting annotation (mm)", &d["fitting_annotation_size"], "Single-line fittings' size on paper");
        let sep = f.text("Duct size separator", d["size_separator"].as_str().unwrap_or(""), "Between width and height in size labels");
        let fot = f.text("Flat on top / bottom", &format!("{}, {}", d["flat_on_top"].as_str().unwrap_or(""), d["flat_on_bottom"].as_str().unwrap_or("")), "The two suffixes, comma separated");
        let duct_angles = f.text("Duct angles", &angles_text(&d["angles"]), "Any, \"every 15\" (an increment), or a list such as \"45, 90\"");
        let eq_at = |v: &Value| EQUATIONS.iter().position(|e| Some(e.1) == v.as_str()).unwrap_or(0);
        let labels: Vec<String> = EQUATIONS.iter().map(|e| e.0.to_owned()).collect();
        let duct_eq = f.choice("Duct friction", &labels, eq_at(&d["calculation"]["friction_equation"]));
        let flex = f.check("Conversion", "Ducts end in flex duct", d["conversion"]["flex"].as_bool().unwrap_or(false));
        let flex_len = gap("Flex duct up to (m)", &d["conversion"]["flex_max_length"], "The longest flex duct conversion uses");
        let p = &t["pipe"];
        let pipe_angles = f.text("Pipe angles", &angles_text(&p["angles"]), "Any, \"every 15\" (an increment), or a list such as \"45, 90\"");
        let slopes: Vec<String> = p["slopes"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|s| format!("{}", (s * 100_000.0).round() / 1000.0)).collect();
        let slopes = f.text("Pipe slopes (%)", &slopes.join(", "), "The allowed slopes; zero cannot be removed, and over 100 % (45°) is warned about");
        let pipe_eq = f.choice("Pipe pressure drop", &labels[..2], eq_at(&p["calculation"]["pressure_drop"]));
        let hydronic = f.check("Hydronic analysis", "Analyse hydronic networks", p["calculation"]["hydronic_analysis"].as_bool().unwrap_or(false));
        if !f.show("Duct size tables, pipe segments and fluids have their own buttons in MEP Settings.", "OK") {
            f.dialog.destroy();
            return;
        }
        let mut read = || -> Result<(), String> {
            t["hidden_line"]["draw"] = json!(hidden.get_value());
            t["hidden_line"]["inside_gap"] = json!(num(&inside)?);
            t["hidden_line"]["outside_gap"] = json!(num(&outside)?);
            t["hidden_line"]["single_line_gap"] = json!(num(&single)?);
            t["duct"]["air_density"] = json!(num(&density)?);
            t["duct"]["air_viscosity"] = json!(num(&viscosity)?);
            t["duct"]["fitting_annotation_size"] = json!(num(&fitting)?);
            t["duct"]["size_separator"] = json!(sep.get_value());
            let v = fot.get_value();
            let mut parts = v.split(',').map(str::trim);
            t["duct"]["flat_on_top"] = json!(parts.next().unwrap_or(""));
            t["duct"]["flat_on_bottom"] = json!(parts.next().unwrap_or(""));
            t["duct"]["angles"] = parse_angles(&duct_angles.get_value())?;
            t["duct"]["calculation"]["friction_equation"] = json!(EQUATIONS[duct_eq.get_selection().map_or(0, |i| i as usize)].1);
            t["duct"]["conversion"]["flex"] = json!(flex.get_value());
            t["duct"]["conversion"]["flex_max_length"] = json!(num(&flex_len)?);
            t["pipe"]["angles"] = parse_angles(&pipe_angles.get_value())?;
            let s = slopes
                .get_value()
                .split([',', ';'])
                .filter(|x| !x.trim().is_empty())
                .map(|x| x.trim().trim_end_matches('%').replace(',', ".").parse::<f64>().map(|v| v / 100.0).map_err(|_| format!("{x:?} is not a slope")))
                .collect::<Result<Vec<_>, _>>()?;
            t["pipe"]["slopes"] = json!(s);
            t["pipe"]["calculation"]["pressure_drop"] = json!(EQUATIONS[pipe_eq.get_selection().map_or(0, |i| i as usize)].1);
            t["pipe"]["calculation"]["hydronic_analysis"] = json!(hydronic.get_value());
            Ok(())
        };
        let parsed = read();
        f.dialog.destroy();
        if let Err(e) = parsed {
            return error(&frame, &format!("The settings were not changed:\n{e}"));
        }
        let mut changed = 0;
        for branch in ["hidden_line", "duct", "pipe"] {
            if t[branch] == before[branch] {
                continue;
            }
            match self.app.execute("mechanical_settings.set", json!({ "path": branch, "value": t[branch] }), &reviera_core::Caller::user()) {
                Ok(o) => {
                    changed += 1;
                    if let Some(w) = o.warnings.first() {
                        frame.set_status_text(&w.message, 0);
                    }
                }
                Err(e) => return error(&frame, &format!("The {} settings were not changed:\n{e}", branch.replace('_', " "))),
            }
        }
        if changed > 0 {
            self.refresh_scene(&frame);
            frame.set_status_text("Mechanical settings changed", 0);
        }
    }

    /// Manage › MEP Settings › Duct Sizes: a shape's size table, one size a line, marked L (in
    /// the size lists) and S (in sizing).
    pub(crate) fn duct_sizes(&mut self) {
        let frame = self.frame;
        let shapes = ["Rectangular", "Oval", "Round"];
        let d = SingleChoiceDialog::builder(&frame, "Shape:", "Duct Sizes", &shapes).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let shape = shapes[usize::try_from(d.get_selection()).unwrap_or(0).min(2)].to_lowercase();
        let Ok(rows) = self.run("duct_size_table.get", json!({ "shape": shape })) else { return };
        let text: Vec<String> = rows
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let mut line = format!("{}", (r["size"].as_f64().unwrap_or(0.0) * 10_000.0).round() / 10.0);
                if r["in_lists"].as_bool().unwrap_or(true) {
                    line.push_str(" L");
                }
                if r["in_sizing"].as_bool().unwrap_or(true) {
                    line.push_str(" S");
                }
                line
            })
            .collect();
        let f = Form::new(&frame, "Duct Sizes");
        let lines = f.lines("Sizes (mm)", &text.join("\n"), "One size a line; L: in the size lists, S: used in sizing");
        if !f.show("Taking a size off the lists leaves ducts of that size as they are.", "OK") {
            f.dialog.destroy();
            return;
        }
        let v = lines.get_value();
        f.dialog.destroy();
        let mut sizes = Vec::new();
        for l in v.lines().filter(|l| !l.trim().is_empty()) {
            let mut parts = l.split_whitespace();
            let Some(mm) = parts.next().and_then(|n| n.replace(',', ".").parse::<f64>().ok()) else {
                return error(&frame, &format!("{l:?} does not start with a size in mm"));
            };
            let flags: String = parts.collect::<String>().to_uppercase();
            sizes.push(json!({ "size": mm / 1000.0, "in_lists": flags.contains('L'), "in_sizing": flags.contains('S') }));
        }
        match self.run("duct_size_table.set", json!({ "shape": shape, "sizes": sizes })) {
            Ok(_) => frame.set_status_text(&format!("{} {shape} sizes", sizes.len()), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Manage › MEP Settings › Pipe Segment: a material and schedule with its roughness and sizes.
    pub(crate) fn new_pipe_segment(&mut self) {
        let frame = self.frame;
        let f = Form::new(&frame, "Pipe Segment");
        let material = f.text("Material", "", "Copper, Steel, PVC-U…");
        let schedule = f.text("Schedule or class", "", "Schedule 40, Type L, PN 16…");
        let rough = f.text("Roughness (mm)", "0.045", "Absolute roughness");
        let sizes = f.lines("Sizes (mm)", "", "One a line: nominal, inner and outer diameter");
        if !f.show("Pipe sizing chooses from the segments' sizes; each is exported as an IfcPipeSegmentType.", "Create") {
            f.dialog.destroy();
            return;
        }
        let (m, s, r, z) = (material.get_value(), schedule.get_value(), rough.get_value(), sizes.get_value());
        f.dialog.destroy();
        let Ok(r) = r.trim().replace(',', ".").parse::<f64>() else { return error(&frame, "The roughness is a number of mm") };
        let mut rows = Vec::new();
        for l in z.lines().filter(|l| !l.trim().is_empty()) {
            let v: Vec<f64> = l.split([' ', '\t', ';', '/']).filter(|x| !x.is_empty()).filter_map(|x| x.replace(',', ".").parse().ok()).collect();
            let [n, i, o] = v[..] else { return error(&frame, &format!("{l:?}: give nominal, inner and outer diameter in mm")) };
            rows.push(json!({ "nominal": n / 1000.0, "inner": i / 1000.0, "outer": o / 1000.0 }));
        }
        match self.run("pipe_segment.create", json!({ "material_id": m.trim(), "schedule": s.trim(), "roughness": r / 1000.0, "sizes": rows })) {
            Ok(_) => frame.set_status_text("Pipe segment made", 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Manage › MEP Settings › Fluid: a fluid's property table.
    pub(crate) fn new_fluid(&mut self) {
        let frame = self.frame;
        let f = Form::new(&frame, "Fluid");
        let name = f.text("Name", "", "As system types name it");
        let table = f.lines("Table", "", "One row a line: temperature (°C), density (kg/m³), viscosity (mPa·s)");
        if !f.show("Systems whose type names the fluid use it at their temperature.", "Create") {
            f.dialog.destroy();
            return;
        }
        let (n, t) = (name.get_value(), table.get_value());
        f.dialog.destroy();
        let mut rows = Vec::new();
        for l in t.lines().filter(|l| !l.trim().is_empty()) {
            let v: Vec<f64> = l.split([' ', '\t', ';']).filter(|x| !x.is_empty()).filter_map(|x| x.replace(',', ".").parse().ok()).collect();
            let [temp, rho, mu] = v[..] else { return error(&frame, &format!("{l:?}: give temperature, density and viscosity")) };
            rows.push(json!({ "temperature": temp, "density": rho, "viscosity": mu / 1000.0 }));
        }
        match self.run("fluid.create", json!({ "name": n.trim(), "table": rows })) {
            Ok(_) => frame.set_status_text("Fluid made", 0),
            Err(e) => error(&frame, &e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_round_trip() {
        for t in ["Any", "every 15", "45, 90"] {
            assert_eq!(angles_text(&parse_angles(t).unwrap()), t);
        }
        assert!(parse_angles("every x").is_err());
    }
}
