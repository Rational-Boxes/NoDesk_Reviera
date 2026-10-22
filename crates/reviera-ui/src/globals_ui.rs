// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Settings › Global Parameters (DM-GPAR-001): the project's global parameters with
//! New, Edit, Duplicate, Delete and Select Elements; Label with Global on a selected dimension;
//! Associate Global on selected elements (an instance parameter of the same data type, or doors'
//! and windows' sill height).

use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::reinforcement_ui::Form;
use super::{Gui, error};

const NEW: i32 = 7501;
const EDIT: i32 = 7502;
const DUPLICATE: i32 = 7503;
const DELETE: i32 = 7504;
const SELECT: i32 = 7505;

const TYPES: [&str; 9] = ["length", "area", "volume", "angle", "number", "integer", "yes_no", "text", "material"];

impl Gui {
    /// A global's value as text: lengths in project units, angles in degrees.
    fn global_text(&mut self, g: &Value) -> String {
        let v = &g["value"];
        match g["data_type"].as_str().unwrap_or("") {
            "length" => self.format_length(v.as_f64().unwrap_or(0.0)),
            "area" => format!("{:.3} m²", v.as_f64().unwrap_or(0.0)),
            "volume" => format!("{:.3} m³", v.as_f64().unwrap_or(0.0)),
            "angle" => format!("{:.2}°", v.as_f64().unwrap_or(0.0).to_degrees()),
            "yes_no" => if v.as_bool() == Some(true) { "Yes".into() } else { "No".into() },
            "text" | "material" => v.as_str().unwrap_or("").to_owned(),
            _ => format!("{}", v.as_f64().unwrap_or(0.0)),
        }
    }

    /// Manage › Settings › Global Parameters (`GL`).
    pub(crate) fn global_parameters(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("global.list", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let mut sorted = list.clone();
            sorted.sort_by(|a, b| a["group"].as_str().cmp(&b["group"].as_str()).then(a["name"].as_str().cmp(&b["name"].as_str())));
            let rows: Vec<Vec<String>> = sorted
                .iter()
                .map(|g| {
                    vec![
                        g["group"].as_str().unwrap_or("").to_owned(),
                        g["name"].as_str().unwrap_or("").to_owned(),
                        self.global_text(g),
                        g["formula"].as_str().unwrap_or("").to_owned(),
                        if g["reporting"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
                        g["uses"].as_u64().unwrap_or(0).to_string(),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Global Parameters",
                "Project-wide values that drive dimensions (Label with Global) and element parameters (Associate Global). Formulas use other globals by name, with units: CorridorWidth * 2 + 300 mm.",
                &[("Group", 90), ("Name", 150), ("Value", 100), ("Formula", 170), ("Reporting", 70), ("Uses", 50)],
                &rows,
                &[
                    (NEW, "New…", "Add a global parameter"),
                    (EDIT, "Edit…", "Change its value, formula, name, group or tooltip"),
                    (DUPLICATE, "Duplicate", "Copy it under a new name"),
                    (DELETE, "Delete", "Delete it: driven parameters keep their values, dimensions are unlabelled"),
                    (SELECT, "Select Elements", "Select what it drives"),
                ],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| sorted.get(k)).cloned();
            let result = match (code, chosen) {
                (NEW, _) => match self.global_form(None) {
                    Some(args) => self.run("global.create", args).map(|_| ()),
                    None => continue,
                },
                (EDIT, Some(g)) => match self.global_form(Some(&g)) {
                    Some(args) => self.run("global.set", args).map(|_| ()),
                    None => continue,
                },
                (DUPLICATE, Some(g)) => {
                    let mut args = json!({ "name": format!("{} 2", g["name"].as_str().unwrap_or("")), "data_type": g["data_type"], "group": g["group"], "reporting": g["reporting"] });
                    if g["reporting"].as_bool() != Some(true) {
                        match g["formula"].as_str() {
                            Some(f) => args["formula"] = json!(f),
                            None => args["value"] = g["value"].clone(),
                        }
                    }
                    self.run("global.create", args).map(|_| ())
                }
                (DELETE, Some(g)) => {
                    let uses = g["uses"].as_u64().unwrap_or(0);
                    if uses > 0 {
                        let d = MessageDialog::builder(&frame, &format!("{} has {uses} use(s). Driven parameters keep their values and dimensions are unlabelled. Delete it?", g["name"].as_str().unwrap_or("")), "Delete Global Parameter")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                            .build();
                        if d.show_modal() != ID_YES {
                            continue;
                        }
                    }
                    self.run("global.delete", json!({ "global_id": g["id"] })).map(|_| ())
                }
                (SELECT, Some(g)) => {
                    match self.run("global.select_driven", json!({ "global_id": g["id"] })) {
                        Ok(v) => {
                            let ids: Vec<ElementId> = serde_json::from_value(v).unwrap_or_default();
                            self.select_ids(&ids, super::select::Combine::Replace);
                            frame.set_status_text(&format!("{} element(s) driven by {}", ids.len(), g["name"].as_str().unwrap_or("")), 0);
                            break;
                        }
                        Err(e) => Err(e),
                    }
                }
                (EDIT | DUPLICATE | DELETE | SELECT, None) => continue,
                _ => break,
            };
            match result {
                Ok(()) => self.refresh_scene(&frame),
                Err(e) => error(&frame, &e),
            }
        }
    }

    /// The Global Parameter Properties form: `global.create` arguments (new) or `global.set`
    /// ones (existing); none when cancelled.
    fn global_form(&mut self, g: Option<&Value>) -> Option<Value> {
        let value_text = g.map(|g| match g["data_type"].as_str() {
            Some("length") => self.format_length(g["value"].as_f64().unwrap_or(0.0)),
            Some("angle") => format!("{}", g["value"].as_f64().unwrap_or(0.0).to_degrees()),
            _ => g["value"].as_str().map_or_else(|| g["value"].to_string(), str::to_owned),
        });
        let f = Form::new(&self.frame, "Global Parameter Properties");
        let name = f.text("Name", g.and_then(|g| g["name"].as_str()).unwrap_or(""), "Unique; no [ ] { } | ; < > @");
        let types: Vec<String> = TYPES.iter().map(|t| (*t).to_owned()).collect();
        let ty = g.is_none().then(|| f.choice("Data type", &types, 0));
        let group = f.text("Group", g.and_then(|g| g["group"].as_str()).unwrap_or("Dimensions"), "");
        let reporting = g.is_none().then(|| f.check("Reporting", "Read from the dimension it labels", false));
        let value = f.text("Value", value_text.as_deref().unwrap_or(""), "Lengths with units (1800 mm); angles in degrees; yes/no as true or false");
        let formula = f.text("Formula", g.and_then(|g| g["formula"].as_str()).unwrap_or(""), "Over other globals, with units; blank: none");
        let tooltip = f.text("Tooltip", g.and_then(|g| g["tooltip"].as_str()).unwrap_or(""), "");
        let ok = f.show("", "OK");
        let data_type = match (&ty, g) {
            (Some(c), _) => TYPES[c.get_selection().map_or(0, |i| i as usize)].to_owned(),
            (None, Some(g)) => g["data_type"].as_str().unwrap_or("number").to_owned(),
            _ => "number".to_owned(),
        };
        let (n, gr, vt, ft, tt) = (name.get_value(), group.get_value(), value.get_value(), formula.get_value(), tooltip.get_value());
        let rep = reporting.as_ref().is_some_and(CheckBox::get_value) || g.is_some_and(|g| g["reporting"].as_bool() == Some(true));
        f.dialog.destroy();
        if !ok {
            return None;
        }
        let parsed: Option<Value> = if vt.trim().is_empty() || rep || !ft.trim().is_empty() {
            None
        } else {
            match data_type.as_str() {
                "length" => self.parse_length_text(&vt).map(|v| json!(v)),
                "angle" => vt.trim().trim_end_matches('°').parse::<f64>().ok().map(|d| json!(d.to_radians())),
                "yes_no" => Some(json!(matches!(vt.trim().to_lowercase().as_str(), "true" | "yes" | "1"))),
                "text" | "material" => Some(json!(vt)),
                _ => vt.trim().replace(',', ".").parse::<f64>().ok().map(|v| json!(v)),
            }
        };
        let mut args = match g {
            None => json!({ "name": n.trim(), "data_type": data_type, "group": gr, "reporting": rep, "tooltip": tt }),
            Some(g) => json!({ "global_id": g["id"], "name": n.trim(), "group": gr, "tooltip": tt }),
        };
        if let Some(v) = parsed {
            args["value"] = v;
        }
        if !rep && (g.is_some() || !ft.trim().is_empty()) {
            args["formula"] = json!(ft.trim());
        }
        Some(args)
    }

    /// Modify | Dimensions › Label with Global: the selected dimension takes a length global.
    pub(crate) fn label_dimension_with_global(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let Some(dim) = self.selection().into_iter().find(|id| store.get(*id).and_then(reviera_core::annotations::dimension_data).is_some()) else {
            return frame.set_status_text("Label with Global: select a dimension", 0);
        };
        let Some(g) = self.pick_global(Some("length"), true) else { return };
        match self.run("global.label_dimension", json!({ "global_id": g, "dimension_id": dim })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Dimension labelled", 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Associate Global: the selected elements' parameter follows a global.
    pub(crate) fn associate_global(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let Some(first) = sel.first().copied() else {
            return frame.set_status_text("Associate Global: select the elements first", 0);
        };
        // Doors and windows: their sill height; and every writable instance parameter.
        let mut params: Vec<(String, String, String)> = Vec::new();
        if store.get(first).is_some_and(|e| reviera_core::inserts::resolve(&store, e).is_ok()) {
            params.push((reviera_core::globals::SILL.into(), "Sill Height".into(), "length".into()));
        }
        if let Ok(v) = self.run("param.list", json!({ "element_id": first })) {
            for d in v.as_array().into_iter().flatten() {
                if d["read_only"].as_bool() == Some(true) || d["binding"].as_str() == Some("type") {
                    continue;
                }
                params.push((d["ref"].as_str().unwrap_or("").into(), d["name"].as_str().unwrap_or("").into(), d["data_type"].as_str().unwrap_or("").into()));
            }
        }
        if params.is_empty() {
            return frame.set_status_text("Associate Global: the selection has no parameter a global can drive", 0);
        }
        let labels: Vec<String> = params.iter().map(|p| format!("{} ({})", p.1, p.2)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Parameter:", "Associate Global Parameter", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((pref, _, ty)) = usize::try_from(d.get_selection()).ok().and_then(|k| params.get(k)).cloned() else { return };
        let Some(g) = self.pick_global(Some(&ty), false) else { return };
        match self.run("global.associate", json!({ "global_id": g, "element_ids": sel, "param_ref": pref })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                frame.set_status_text(&format!("{} element(s) follow the global", sel.len()), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// A global of a data type (reporting ones too when `reporting`), chosen from a list (none
    /// when cancelled or none fits).
    fn pick_global(&mut self, data_type: Option<&str>, reporting: bool) -> Option<String> {
        let frame = self.frame;
        let list = self.run("global.list", json!({})).ok()?.as_array().cloned().unwrap_or_default();
        let fits: Vec<&Value> = list.iter().filter(|g| data_type.is_none_or(|t| g["data_type"].as_str() == Some(t)) && (reporting || g["reporting"].as_bool() != Some(true))).collect();
        if fits.is_empty() {
            frame.set_status_text("No global parameter of that type: make one in Manage › Global Parameters", 0);
            return None;
        }
        let labels: Vec<String> = fits.iter().map(|g| format!("{}{}", g["name"].as_str().unwrap_or(""), if g["reporting"].as_bool() == Some(true) { " (reporting)" } else { "" })).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Global parameter:", "Global Parameters", &refs).build();
        if d.show_modal() != ID_OK {
            return None;
        }
        usize::try_from(d.get_selection()).ok().and_then(|k| fits.get(k)).and_then(|g| g["id"].as_str()).map(str::to_owned)
    }
}
