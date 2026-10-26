// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sustainability › Energy › **Energy Model** (ANA-EM-003, ANA-ENV-003): the analytical spaces derived
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

impl Gui {
    /// Sustainability › Energy › Heating & Cooling Loads (ANA-HCL-001, ANA-ENV-004): the infiltration
    /// class and load credits, then a compute job; the report opens when it is collected.
    pub(crate) fn hc_loads_dialog(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Heating & Cooling Loads");
        let classes = vec!["Loose (0.39 L/s·m²)".to_owned(), "Medium (0.19 L/s·m²)".to_owned(), "Tight (0.10 L/s·m²)".to_owned(), "None".to_owned()];
        let class = f.choice("Infiltration", &classes, 1);
        let credits = f.check("Load credits", "Allow negative load components", false);
        if !f.show("Design-day loads on the energy model's spaces from the weather's design conditions, the space types' loads and set points; runs out of process.", "Run") {
            f.dialog.destroy();
            return;
        }
        let key = ["loose", "medium", "tight", "none"][class.get_selection().map_or(1, |i| i as usize).min(3)];
        let args = json!({ "infiltration": key, "credits": credits.get_value() });
        f.dialog.destroy();
        match self.run("hc_loads.run", args) {
            Ok(v) => self.watch_job(v["job_id"].as_str().unwrap_or_default().to_owned(), "hc_loads.collect", "Heating and cooling loads"),
            Err(e) => error(&frame, &format!("The loads run did not start:\n{e}")),
        }
    }

    /// The latest loads report.
    pub(crate) fn hc_loads_report(&mut self) {
        let frame = self.frame;
        let r = match self.run("hc_loads.report", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let stale = self.run("hc_loads.status", json!({})).ok().is_some_and(|s| s["stale"] == true);
        let kw = |v: &Value| format!("{:.2}", v.as_f64().unwrap_or(0.0) / 1000.0);
        let mut rows: Vec<Vec<String>> = r["spaces"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let l = &s["load"];
                vec![
                    s["number"].as_str().unwrap_or("").to_owned(),
                    s["name"].as_str().unwrap_or("").to_owned(),
                    kw(&l["heating"]),
                    kw(&l["cooling_sensible"]),
                    kw(&l["cooling_total"]),
                    format!("{}", l["peak_hour"]),
                    format!("{:.0}", l["cooling_density"].as_f64().unwrap_or(0.0)),
                    format!("{:.0} / {:.0}", l["cooling_airflow"].as_f64().unwrap_or(0.0), l["heating_airflow"].as_f64().unwrap_or(0.0)),
                ]
            })
            .collect();
        for z in r["zones"].as_array().into_iter().flatten() {
            let name = z[0].as_u64().and_then(|id| self.store_now()?.get(reviera_core::ElementId(id))?.name.clone()).unwrap_or_else(|| "Zone".into());
            rows.push(vec!["Zone".into(), name, kw(&z[1]), String::new(), kw(&z[2]), format!("{}", z[3]), String::new(), String::new()]);
        }
        let mut intro = format!(
            "Infiltration {}; design {:.1} °C heating, {:.1} °C cooling. Heating {} kW, cooling {} kW (sum of space peaks), coincident {} kW at {} h. Peaks are written to the spaces (Pset_SpaceThermalDesign).",
            r["infiltration"].as_str().unwrap_or(""),
            r["heating_dry_bulb"].as_f64().unwrap_or(0.0),
            r["cooling_dry_bulb"].as_f64().unwrap_or(0.0),
            kw(&r["heating_total"]),
            kw(&r["cooling_total"]),
            kw(&r["cooling_coincident"]),
            r["coincident_hour"]
        );
        if stale {
            intro.push_str(" Stale: the model or the weather changed since; run it again.");
        }
        super::mep_loads_ui::list_dialog(&frame, "Heating & Cooling Loads", &intro, &[("Space", 60), ("Name", 110), ("Heat kW", 65), ("Sens. kW", 65), ("Cool kW", 65), ("Peak h", 55), ("W/m²", 55), ("Air L/s c/h", 90)], &rows, &[], None);
    }
}

impl Gui {
    /// Sustainability › Energy › Annual Energy (ANA-ENV-005): the infiltration, mass, heating
    /// source, efficiencies and emission factors, then a compute job; the results open when it is
    /// collected.
    pub(crate) fn annual_energy_dialog(&mut self) {
        let frame = self.frame;
        let last = self.run("energy.results", json!({})).ok().map(|r| r["options"].clone()).unwrap_or(Value::Null);
        let pick = |key: &str, items: &[&str], default: usize| last[key].as_str().and_then(|v| items.iter().position(|i| *i == v)).unwrap_or(default);
        let num = |key: &str, default: &str| last[key].as_f64().filter(|v| *v > 0.0).map_or_else(|| default.to_owned(), |v| format!("{v}"));
        let f = super::reinforcement_ui::Form::new(&frame, "Annual Energy");
        let infil_keys = ["loose", "medium", "tight", "none"];
        let infil = f.choice("Infiltration", &["Loose (0.39 L/s·m²)".to_owned(), "Medium (0.19 L/s·m²)".to_owned(), "Tight (0.10 L/s·m²)".to_owned(), "None".to_owned()], pick("infiltration", &infil_keys, 1));
        let mass_keys = ["light", "medium", "heavy"];
        let mass = f.choice("Thermal mass", &["Light (timber, steel)".to_owned(), "Medium".to_owned(), "Heavy (masonry, concrete)".to_owned()], pick("mass", &mass_keys, 1));
        let source_keys = ["gas", "electric", "heat_pump"];
        let source = f.choice("Heating", &["Gas boiler".to_owned(), "Electric resistance".to_owned(), "Heat pump".to_owned()], pick("heating", &source_keys, 0));
        let eff = f.text("Heating efficiency or COP", &num("heating_efficiency", ""), "Blank for the source's default: 0.9 gas, 1.0 electric, COP 3.0 heat pump");
        let cop = f.text("Cooling COP", &num("cooling_cop", "3.0"), "Seasonal coefficient of performance of the cooling");
        let grid = f.text("Grid factor (kg CO₂e/kWh)", &last["grid_factor"].as_f64().map_or(String::new(), |v| format!("{v}")), "Your grid's emission factor; electricity carbon is left out when blank");
        let gas = f.text("Gas factor (kg CO₂e/kWh)", &num("gas_factor", "0.202"), "Natural gas combustion");
        if !f.show("A year of hourly simulation of the energy model's spaces through the weather file, with the space types' loads, set points and opening hours; runs out of process.", "Run") {
            f.dialog.destroy();
            return;
        }
        let parse = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        fn sel(c: &Choice, keys: &[&'static str], d: usize) -> &'static str {
            keys[c.get_selection().map_or(d, |i| i as usize).min(keys.len() - 1)]
        }
        let mut o = json!({
            "infiltration": sel(&infil, &infil_keys, 1),
            "mass": sel(&mass, &mass_keys, 1),
            "heating": sel(&source, &source_keys, 0),
            "heating_efficiency": parse(&eff).unwrap_or(0.0),
            "cooling_cop": parse(&cop).unwrap_or(3.0),
            "gas_factor": parse(&gas).unwrap_or(0.202),
        });
        if let Some(g) = parse(&grid) {
            o["grid_factor"] = json!(g);
        }
        f.dialog.destroy();
        match self.run("energy.run", json!({ "options": o })) {
            Ok(v) => self.watch_job(v["job_id"].as_str().unwrap_or_default().to_owned(), "energy.collect", "Annual energy"),
            Err(e) => error(&frame, &format!("The energy run did not start:\n{e}")),
        }
    }

    /// The latest annual energy results: monthly end uses, the year, and each space's loads.
    pub(crate) fn annual_energy_results(&mut self) {
        let frame = self.frame;
        let r = match self.run("energy.results", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let stale = self.run("energy.status", json!({})).ok().is_some_and(|s| s["stale"] == true);
        let n = |v: &Value| format!("{:.0}", v.as_f64().unwrap_or(0.0));
        let carbon = |v: &Value| v.as_f64().map_or("—".to_owned(), |c| format!("{c:.0}"));
        let line = |label: String, m: &Value| vec![label, n(&m["heating_load"]), n(&m["cooling_load"]), n(&m["heating"]), n(&m["cooling"]), n(&m["lighting"]), n(&m["equipment"]), n(&m["electricity"]), n(&m["gas"]), carbon(&m["carbon"])];
        let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let mut rows: Vec<Vec<String>> = r["months"].as_array().into_iter().flatten().enumerate().map(|(i, m)| line(names[i.min(11)].to_owned(), m)).collect();
        rows.push(line("Year".into(), &r["year"]));
        for s in r["spaces"].as_array().into_iter().flatten() {
            let e = &s["energy"];
            let label = format!("{} {}", s["number"].as_str().unwrap_or(""), s["name"].as_str().unwrap_or("")).trim().to_owned();
            rows.push(vec![label, n(&e["heating_load"]), n(&e["cooling_load"]), String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), String::new()]);
        }
        let o = &r["options"];
        let year = &r["year"];
        let mut intro = format!(
            "Floor {:.0} m²; delivered {:.0} kWh a year, EUI {:.0} kWh/m²; peaks {:.1} kW heating, {:.1} kW cooling. Heating by {} (efficiency {}), cooling COP {}, infiltration {}, {} mass. Loads and energy in kWh, carbon in kg CO₂e{}. Rows after the year are the spaces' loads.",
            r["floor_area"].as_f64().unwrap_or(0.0),
            year["electricity"].as_f64().unwrap_or(0.0) + year["gas"].as_f64().unwrap_or(0.0),
            r["eui"].as_f64().unwrap_or(0.0),
            r["peak_heating"].as_f64().unwrap_or(0.0) / 1000.0,
            r["peak_cooling"].as_f64().unwrap_or(0.0) / 1000.0,
            o["heating"].as_str().unwrap_or("gas").replace('_', " "),
            o["heating_efficiency"].as_f64().filter(|v| *v > 0.0).map_or("default".to_owned(), |v| format!("{v}")),
            o["cooling_cop"],
            o["infiltration"].as_str().unwrap_or(""),
            o["mass"].as_str().unwrap_or(""),
            if year["carbon"].is_null() { " (no grid factor given, so no carbon)" } else { "" }
        );
        if stale {
            intro.push_str(" Stale: the model or the weather changed since; run it again.");
        }
        super::mep_loads_ui::list_dialog(&frame, "Annual Energy", &intro, &[("Month / space", 110), ("Heat load", 70), ("Cool load", 70), ("Heating", 65), ("Cooling", 65), ("Lighting", 65), ("Equip.", 65), ("Electric", 70), ("Gas", 60), ("CO₂e", 60)], &rows, &[], None);
    }
}

impl Gui {
    /// File › Export gbXML… (ANA-EM-004): the energy model as gbXML 8.01.
    pub(crate) fn gbxml_export(&mut self) {
        let frame = self.frame;
        let fd = FileDialog::builder(&frame).with_message("Export gbXML").with_wildcard("gbXML (*.xml)|*.xml").with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt).build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        match self.run("gbxml.export", json!({ "path": path })) {
            Ok(r) => {
                let mut msg = format!("Wrote {} spaces, {} surfaces and {} openings to {path}.", r["spaces"], r["surfaces"], r["openings"]);
                for w in r["warnings"].as_array().into_iter().flatten() {
                    msg.push_str(&format!("\n• {}", w.as_str().unwrap_or("")));
                }
                MessageDialog::builder(&frame, &msg, "Export gbXML").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// File › Import gbXML Loads… (ANA-EM-004): design loads from another tool onto the spaces.
    pub(crate) fn gbxml_import_loads(&mut self) {
        let frame = self.frame;
        let fd = FileDialog::builder(&frame).with_message("Import Loads from gbXML").with_wildcard("gbXML (*.xml;*.gbxml)|*.xml;*.gbxml").with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist).build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        match self.run("gbxml.import_loads", json!({ "path": path })) {
            Ok(r) => {
                let list = |k: &str| r[k].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                let mut msg = format!("Loads written to {} spaces.", r["matched"].as_array().map_or(0, Vec::len));
                if !list("unmatched").is_empty() {
                    msg.push_str(&format!("\nNot matched: {}", list("unmatched")));
                }
                if !list("without_results").is_empty() {
                    msg.push_str(&format!("\nNo values in the file for: {}", list("without_results")));
                }
                MessageDialog::builder(&frame, &msg, "Import gbXML Loads").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }
}
