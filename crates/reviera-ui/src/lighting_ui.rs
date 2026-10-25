// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Average estimated illumination (ANA-LIGHT-001): Analyze › Spaces & Zones › **Illumination**
//! lists every space's lumen-method average with its target, RCR, fixtures counted and those
//! left out (Target… sets a space's target and light loss factor, Select selects it);
//! Systems › Electrical › **Lighting Data** sets the selected fixture's type's lumens, light loss
//! factor and CU table, the table pasted as text from the manufacturer's data:
//!
//! ```text
//! RCR, 80/50/20, 80/30/20, 50/50/20
//! 0,   1.19,     1.19,     1.11
//! 1,   1.02,     0.98,     0.96
//! ```
//!
//! (columns are ceiling/wall/floor reflectance in per cent).

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const TARGET: i32 = 7202;
const SELECT: i32 = 7601;

/// A CU table as text, one row per RCR.
fn table_text(t: &Value) -> String {
    let Some(cols) = t["columns"].as_array() else { return String::new() };
    let pct = |v: &Value| format!("{:.0}", v.as_f64().unwrap_or(0.0) * 100.0);
    let mut out = String::from("RCR");
    for c in cols {
        out.push_str(&format!(", {}/{}/{}", pct(&c[0]), pct(&c[1]), pct(&c[2])));
    }
    for (rcr, row) in t["rcr"].as_array().into_iter().flatten().zip(t["values"].as_array().into_iter().flatten()) {
        out.push_str(&format!("\n{}", rcr.as_f64().unwrap_or(0.0)));
        for v in row.as_array().into_iter().flatten() {
            out.push_str(&format!(", {}", v.as_f64().unwrap_or(0.0)));
        }
    }
    out
}

/// Parses a CU table from text (commas, semicolons, tabs or spaces between values).
pub(crate) fn parse_table(text: &str) -> Result<Option<Value>, String> {
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(|l| l.split([',', ';', '\t', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>())
        .filter(|r| !r.is_empty())
        .collect();
    if rows.is_empty() {
        return Ok(None);
    }
    let header = &rows[0];
    let mut columns = Vec::new();
    for h in header.iter().skip(1) {
        let p: Vec<f64> = h.split('/').filter_map(|x| x.trim().parse::<f64>().ok()).collect();
        if p.len() != 3 {
            return Err(format!("column {h:?} should be ceiling/wall/floor reflectance in per cent, as 80/50/20"));
        }
        columns.push([p[0] / 100.0, p[1] / 100.0, p[2] / 100.0]);
    }
    let mut rcr = Vec::new();
    let mut values = Vec::new();
    for (k, r) in rows.iter().enumerate().skip(1) {
        let nums: Result<Vec<f64>, _> = r.iter().map(|x| x.parse::<f64>()).collect();
        let nums = nums.map_err(|_| format!("row {} has a value that is not a number", k + 1))?;
        if nums.len() != columns.len() + 1 {
            return Err(format!("row {} has {} values; it needs the RCR and {}", k + 1, nums.len(), columns.len()));
        }
        rcr.push(nums[0]);
        values.push(nums[1..].to_vec());
    }
    Ok(Some(json!({ "rcr": rcr, "columns": columns, "values": values })))
}

impl Gui {
    /// Systems › Electrical › Lighting Data for the selected fixture's type.
    pub(crate) fn fixture_lighting_data(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let ty = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            let (t, mt) = reviera_core::mesh_objects::type_of(&store, e)?;
            (mt.category == reviera_core::mesh_objects::Category::LightingFixture).then_some((t, mt))
        });
        let Some((ty, mt)) = ty else {
            return frame.set_status_text("Lighting Data: select a lighting fixture", 0);
        };
        let data = store.get(ty).map(reviera_core::mep_illumination::lighting_data).unwrap_or_default();
        let dv = serde_json::to_value(&data).unwrap_or_default();
        let name = store.get(ty).and_then(|t| t.name.clone()).unwrap_or_default();
        let f = super::reinforcement_ui::Form::new(&frame, &format!("Lighting Data: {name}"));
        let from_file = mt.photometry.as_ref().and_then(|p| p.lumens);
        let lumens = f.text(
            "Lumens per fixture",
            &data.lumens.map_or(String::new(), |l| format!("{l}")),
            &match from_file {
                Some(l) => format!("The photometric file's {l:.0} lm is used"),
                None => "Rated lumens (no photometric file states them)".into(),
            },
        );
        let llf = f.text("Light loss factor", &data.light_loss_factor.map_or(String::new(), |v| format!("{v}")), "0–1; blank: 0.8");
        let table = f.lines("CU table", &table_text(&dv["cu_table"]), "From the manufacturer: a header of ceiling/wall/floor reflectances in per cent, then a row per RCR");
        if !f.show("Average estimated illumination uses these (Analyze › Spaces & Zones › Illumination).", "OK") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl| -> Value { t.get_value().trim().replace(',', ".").parse::<f64>().map_or(Value::Null, |x| json!(x)) };
        let (l, k, txt) = (num(&lumens), num(&llf), table.get_value());
        f.dialog.destroy();
        let cu = match parse_table(&txt) {
            Ok(v) => v.unwrap_or(Value::Null),
            Err(e) => return error(&frame, &format!("The CU table was not read: {e}")),
        };
        match self.run("lighting_fixture_type.set_lighting", json!({ "type_id": ty, "lumens": l, "light_loss_factor": k, "cu_table": cu })) {
            Ok(_) => frame.set_status_text(&format!("Lighting data set for {name}"), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Analyze › Spaces & Zones › Illumination: each space's average estimated illumination.
    pub(crate) fn illumination_dialog(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("lighting.calc_average", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|s| {
                    let lx = match (s["illuminance"].as_f64(), s["skipped"].as_str()) {
                        (Some(v), _) => format!("{v:.0}"),
                        (None, Some(why)) => format!("({why})"),
                        _ => String::new(),
                    };
                    let excluded: Vec<String> = s["excluded"].as_array().into_iter().flatten().map(|x| format!("{}: {}", x["fixture"], x["reason"].as_str().unwrap_or(""))).collect();
                    vec![
                        s["number"].as_str().unwrap_or("").to_owned(),
                        s["name"].as_str().unwrap_or("").to_owned(),
                        lx,
                        s["target"].as_f64().map_or(String::new(), |t| format!("{t:.0}")),
                        match s["meets_target"].as_bool() {
                            Some(true) => "Yes".into(),
                            Some(false) => "No".into(),
                            None => String::new(),
                        },
                        format!("{:.2}", s["rcr"].as_f64().unwrap_or(0.0)),
                        s["fixtures"].to_string(),
                        excluded.join("; "),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Average Estimated Illumination",
                "The lumen method for each space: the fixtures whose calculation point is in it, with their type's lumens, CU table and light loss factor. It follows the model as it changes.",
                &[("Number", 60), ("Name", 120), ("Lux", 70), ("Target", 60), ("Meets", 50), ("RCR", 50), ("Fixtures", 60), ("Left out", 200)],
                &rows,
                &[(TARGET, "Target…", "The space's target illuminance and light loss factor"), (SELECT, "Select", "Select the space")],
                selected,
            );
            selected = at;
            let Some(s) = at.and_then(|k| list.get(k)).cloned() else {
                if matches!(code, TARGET | SELECT) {
                    continue;
                }
                break;
            };
            let result = match code {
                TARGET => {
                    let info = self.run("space.info", json!({ "ids": [s["space"]] })).ok().and_then(|v| v.get(0).cloned()).unwrap_or_default();
                    let f = super::reinforcement_ui::Form::new(&frame, &format!("Space {}", s["number"].as_str().unwrap_or("")));
                    let target = f.text("Target illuminance (lx)", &s["target"].as_f64().map_or(String::new(), |v| format!("{v}")), "Pset_SpaceLightingRequirements.Illuminance; blank: none");
                    let llf = f.text("Light loss factor", &info["props"]["light_loss_factor"].as_f64().map_or(String::new(), |v| format!("{v}")), "Blank: each fixture type's (0.8 by default)");
                    if !f.show("", "OK") {
                        f.dialog.destroy();
                        continue;
                    }
                    let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(-1.0);
                    let args = json!({ "ids": [s["space"]], "target_illuminance": num(&target), "light_loss_factor": num(&llf) });
                    f.dialog.destroy();
                    self.run("space.set", args).map(|_| ())
                }
                SELECT => {
                    let r = self.run("selection.set", json!({ "ids": [s["space"]] })).map(|_| ());
                    self.selection_changed();
                    self.refresh_active();
                    r
                }
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cu_tables_read_from_text() {
        let t = parse_table("RCR, 80/50/20, 70/30/20\n0, 1.19, 1.1\n1; 1.02; 0.95\n").unwrap().unwrap();
        assert_eq!(t["columns"], json!([[0.8, 0.5, 0.2], [0.7, 0.3, 0.2]]));
        assert_eq!(t["values"], json!([[1.19, 1.1], [1.02, 0.95]]));
        assert_eq!(t["rcr"], json!([0.0, 1.0]));
        assert_eq!(parse_table(&table_text(&t)).unwrap().unwrap(), t);
        assert!(parse_table("RCR, 80/50\n0, 1").is_err());
        assert!(parse_table("RCR, 80/50/20\n0, 1, 2").is_err());
        assert_eq!(parse_table("  \n").unwrap(), None);
    }
}
