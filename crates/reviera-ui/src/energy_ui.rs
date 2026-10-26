// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Energy › **Energy Model** (ANA-EM-003, ANA-ENV-003): the analytical spaces derived
//! from the MEP spaces with their exterior wall, window, roof and ground areas and UA, the whole
//! model's summary (window-to-wall ratio, surfaces still on default U-values), and *Thermal
//! Values…* to give a wall, window, door, floor or roof type its U-value (and a window its
//! SHGC).

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const THERMAL: i32 = 7202;

impl Gui {
    pub(crate) fn energy_model_dialog(&mut self) {
        let frame = self.frame;
        loop {
            let (m, s) = match (self.run("energy_model.get", json!({})), self.run("energy_model.summary", json!({}))) {
                (Ok(m), Ok(s)) => (m, s),
                (Err(e), _) | (_, Err(e)) => return error(&frame, &e),
            };
            let area = |sp: &Value, class: &str| sp["surfaces"].as_array().into_iter().flatten().filter(|f| f["class"] == class).map(|f| f["area"].as_f64().unwrap_or(0.0)).sum::<f64>() + 0.0;
            let rows: Vec<Vec<String>> = m["spaces"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|sp| {
                    let ua: f64 = sp["surfaces"].as_array().into_iter().flatten().filter(|f| matches!(f["class"].as_str(), Some("exterior_wall" | "roof" | "slab_on_grade" | "window" | "door"))).map(|f| f["u"].as_f64().unwrap_or(0.0) * f["area"].as_f64().unwrap_or(0.0)).sum();
                    vec![
                        sp["number"].as_str().unwrap_or("").to_owned(),
                        sp["name"].as_str().unwrap_or("").to_owned(),
                        format!("{:.1}", sp["area"].as_f64().unwrap_or(0.0)),
                        format!("{:.1}", area(sp, "exterior_wall")),
                        format!("{:.1}", area(sp, "window")),
                        format!("{:.1}", area(sp, "roof")),
                        format!("{:.1}", area(sp, "slab_on_grade")),
                        format!("{ua:.1}"),
                    ]
                })
                .collect();
            let intro = format!(
                "{} analytical spaces from the MEP spaces ({} skipped). UA to the outside {:.0} W/K; window-to-wall ratio {:.0} %. {} surfaces use default U-values (set them with Thermal Values).",
                s["spaces"],
                s["skipped"],
                s["ua_exterior"].as_f64().unwrap_or(0.0),
                s["window_to_wall"].as_f64().unwrap_or(0.0) * 100.0,
                s["default_u_surfaces"]
            );
            let (code, _) = super::mep_loads_ui::list_dialog(
                &frame,
                "Energy Model",
                &intro,
                &[("Space", 60), ("Name", 110), ("m²", 60), ("Ext. wall m²", 80), ("Window m²", 75), ("Roof m²", 65), ("Ground m²", 70), ("UA W/K", 70)],
                &rows,
                &[(THERMAL, "Thermal Values…", "A type's U-value (and a window's SHGC)")],
                None,
            );
            if code != THERMAL {
                return;
            }
            self.thermal_values();
        }
    }

    fn thermal_values(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let classes = ["IfcWallType", "IfcWindowType", "IfcWindowStyle", "IfcDoorType", "IfcDoorStyle", "IfcSlabType", "IfcRoofType", "IfcCoveringType"];
        let types: Vec<(reviera_core::ElementId, String, Option<f64>)> = store
            .iter()
            .filter(|e| classes.contains(&e.class.as_str()))
            .map(|e| (e.id(), format!("{} ({})", e.name.clone().unwrap_or_default(), e.class.trim_start_matches("Ifc")), reviera_core::energy_model::u_of(&store, e)))
            .collect();
        if types.is_empty() {
            return error(&frame, "The model has no wall, window, door, floor or roof types");
        }
        let labels: Vec<String> = types.iter().map(|t| format!("{}  —  U {}", t.1, t.2.map_or("default".to_owned(), |u| format!("{u} W/m²K")))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Type:", "Thermal Values", &refs).build();
        let k = (d.show_modal() == ID_OK).then(|| usize::try_from(d.get_selection()).ok()).flatten();
        d.destroy();
        let Some((id, name, u)) = k.and_then(|k| types.get(k)).cloned() else { return };
        let window = name.contains("Window");
        let f = super::reinforcement_ui::Form::new(&frame, &name);
        let ut = f.text("U-value (W/m²K)", &u.map_or(String::new(), |u| format!("{u}")), "Blank: the class default");
        let gt = window.then(|| f.text("SHGC", "", "Solar heat gain coefficient, 0–1; blank: 0.6"));
        if !f.show("Written to the type's common property set (ThermalTransmittance), so it goes to IFC.", "OK") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let mut args = json!({ "type_id": id, "u_value": num(&ut) });
        if let Some(g) = &gt {
            args["shgc"] = json!(num(g));
        }
        f.dialog.destroy();
        if let Err(e) = self.run("energy_model.set_thermal", args) {
            error(&frame, &e);
        }
    }
}
