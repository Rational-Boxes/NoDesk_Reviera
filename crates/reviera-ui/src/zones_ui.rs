// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Spaces & Zones › System-Zone (MEP-ZONE-001): *By Sketch* draws a boundary in the
//! plan (walls picked, lines, rectangles, or typed coordinates); *By Level* covers the plan's
//! whole level; *System-Zones* lists them with their spaces and what serves them (Edit, Assign,
//! Boundary to sketch it again, Select, Delete); *Convert Zones* turns legacy zones from
//! imported models into system-zones. Plans draw each zone's outline with its name.

use reviera_core::ElementId;
use reviera_core::system_zones as zones;
use reviera_render::annotation::{label_strokes, room_tag_labels};
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};
use crate::sketch::Curve;

/// Same as the list dialog's double-click code.
const EDIT: i32 = 7202;
const ASSIGN: i32 = 7501;
const BOUNDARY: i32 = 7502;
const SELECT: i32 = 7503;
const DELETE: i32 = 7504;
const ZONE_COLOR: [f32; 4] = [0.55, 0.2, 0.75, 1.0];

fn num(v: &Value) -> String {
    v.as_f64().map_or(String::new(), |x| format!("{}", (x * 1000.0).round() / 1000.0))
}

impl Gui {
    /// System-Zone › By Sketch: a boundary on the plan's level.
    pub(crate) fn start_system_zone_sketch(&mut self) {
        self.start_floor_sketch();
        if let Some(s) = &mut self.sketch {
            s.system_zone = true;
            self.sketch_prompt();
        }
    }

    /// System-Zone › By Level: one zone covering the plan's whole level.
    pub(crate) fn system_zone_by_level(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return frame.set_status_text("System-zones are placed in plan views", 0);
        };
        match self.run("system_zone.create", json!({ "level_id": plan.level, "whole_level": true })) {
            Ok(id) => {
                let _ = self.run("selection.set", json!({ "ids": [id] }));
                self.selection_changed();
                frame.set_status_text("System-zone made for the whole level", 0);
                self.after_zone_change();
            }
            Err(e) => error(&frame, &format!("The system-zone was not made:\n{e}")),
        }
    }

    /// Convert Zones: every legacy zone becomes a system-zone.
    pub(crate) fn convert_legacy_zones(&mut self) {
        let frame = self.frame;
        match self.run("system_zone.convert_legacy", json!({})) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} legacy zone{} converted into system-zones", if n == 1 { "" } else { "s" }), 0);
                self.after_zone_change();
            }
            Err(e) => error(&frame, &e),
        }
    }

    fn after_zone_change(&mut self) {
        let frame = self.frame;
        self.refresh_scene(&frame);
        if let Some(doc) = self.doc {
            self.plans_redraw(doc);
        }
    }

    /// System-Zones: each zone's level, area, spaces and what serves it.
    pub(crate) fn system_zones_dialog(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("system_zone.info", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|z| {
                    let spaces: Vec<&str> = z["space_numbers"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    let served = match (z["served_by_name"].as_str(), z["served_by_kind"].as_str()) {
                        (Some(n), Some(k)) => format!("{n} ({k})"),
                        _ => "—".into(),
                    };
                    let p = &z["props"];
                    let set = match (p["heating_set_point"].as_f64(), p["cooling_set_point"].as_f64()) {
                        (None, None) => String::new(),
                        (h, c) => format!("{} / {}", h.map_or("–".into(), |v| format!("{v}")), c.map_or("–".into(), |v| format!("{v}"))),
                    };
                    vec![
                        z["name"].as_str().unwrap_or("").to_owned(),
                        z["level"].as_str().unwrap_or("").to_owned() + if z["whole_level"] == true { " (whole)" } else { "" },
                        format!("{:.1}", z["area"].as_f64().unwrap_or(0.0)),
                        format!("{}: {}", spaces.len(), spaces.join(", ")),
                        served,
                        set,
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "System-Zones",
                "The areas served by air systems, water loops and zone equipment. Each takes the MEP spaces on its level inside or touching it.",
                &[("Name", 120), ("Level", 110), ("Area m²", 70), ("Spaces", 180), ("Served by", 150), ("Heat / cool °C", 90)],
                &rows,
                &[
                    (EDIT, "Edit…", "Its name, height, set points, humidity and outdoor air"),
                    (ASSIGN, "Assign…", "The equipment or system that serves it"),
                    (BOUNDARY, "Boundary", "Sketch its boundary again in the plan"),
                    (SELECT, "Select", "Select it and its spaces"),
                    (DELETE, "Delete", "Delete it (its spaces stay)"),
                ],
                selected,
            );
            selected = at;
            let Some(z) = at.and_then(|k| list.get(k)).cloned() else {
                if matches!(code, EDIT | ASSIGN | BOUNDARY | SELECT | DELETE) {
                    continue;
                }
                break;
            };
            let id = z["id"].clone();
            let result = match code {
                EDIT => match self.zone_form(&z) {
                    Some(mut args) => {
                        args["zone_ids"] = json!([id]);
                        self.run("system_zone.set", args).map(|_| ())
                    }
                    None => continue,
                },
                ASSIGN => match self.choose_zone_server(&z) {
                    Some(target) => self.run("system_zone.assign", json!({ "zone_id": id, "equipment_or_system_id": target })).map(|_| ()),
                    None => continue,
                },
                BOUNDARY => {
                    let (Some(zone), Some(level)) = (id.as_u64().map(ElementId), z["level_id"].as_u64().map(ElementId)) else { continue };
                    let outline: Vec<[f64; 2]> = serde_json::from_value(z["outline"].clone()).unwrap_or_default();
                    self.edit_zone_boundary(zone, level, &outline);
                    return;
                }
                SELECT => {
                    let mut ids = vec![id];
                    ids.extend(z["spaces"].as_array().cloned().unwrap_or_default());
                    let r = self.run("selection.set", json!({ "ids": ids })).map(|_| ());
                    self.selection_changed();
                    self.refresh_active();
                    r
                }
                DELETE => self.run("element.delete", json!({ "element_ids": [id] })).map(|_| ()),
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
            self.after_zone_change();
        }
    }

    /// Starts a sketch of a zone's boundary from its outline now, in a plan of its level.
    fn edit_zone_boundary(&mut self, zone: ElementId, level: ElementId, outline: &[[f64; 2]]) {
        let frame = self.frame;
        if self.active_view().and_then(|v| v.plan.as_ref()).is_none_or(|p| p.level != level) {
            return frame.set_status_text("Open a plan of the system-zone's level to sketch its boundary", 0);
        }
        let curves: Vec<Curve> = (0..outline.len())
            .map(|i| Curve::new(reviera_core::walls::Line { start: outline[i], end: outline[(i + 1) % outline.len()] }, None))
            .collect();
        self.begin_sketch(crate::sketch::SketchKind::Slab(reviera_core::floors::SlabKind::Floor), None, level, curves);
        if let Some(s) = &mut self.sketch {
            s.system_zone = true;
            s.zone_edit = Some(zone);
            self.sketch_prompt();
        }
    }

    /// A zone's values in a form: the `system_zone.set` arguments, or none.
    fn zone_form(&mut self, z: &Value) -> Option<Value> {
        let p = &z["props"];
        let f = super::reinforcement_ui::Form::new(&self.frame, z["name"].as_str().unwrap_or("System-Zone"));
        let name = f.text("Name", z["name"].as_str().unwrap_or(""), "");
        let height = f.text("Height (m)", &num(&z["height"]), "Blank: up to the next level");
        let heat = f.text("Heating set point (°C)", &num(&p["heating_set_point"]), "Blank: none");
        let cool = f.text("Cooling set point (°C)", &num(&p["cooling_set_point"]), "Blank: none; above the heating set point");
        let hmin = f.text("Humidity min (%)", &num(&p["humidity_min"]), "Relative humidity");
        let hmax = f.text("Humidity max (%)", &num(&p["humidity_max"]), "Relative humidity");
        let oa_p = f.text("Outdoor air per person (L/s)", &p["outside_air_per_person"].as_f64().map_or(String::new(), |v| format!("{}", v * 1000.0)), "");
        let oa_a = f.text("Outdoor air per area (L/s·m²)", &p["outside_air_per_area"].as_f64().map_or(String::new(), |v| format!("{}", v * 1000.0)), "");
        if !f.show("Set points are written to each assigned space in IFC (Pset_SpaceThermalRequirements).", "OK") {
            f.dialog.destroy();
            return None;
        }
        let val = |t: &TextCtrl| -> Value { t.get_value().trim().replace(',', ".").parse::<f64>().map_or(Value::Null, |x| json!(x)) };
        let per_k = |t: &TextCtrl| -> Value { t.get_value().trim().replace(',', ".").parse::<f64>().map_or(Value::Null, |x| json!(x / 1000.0)) };
        let args = json!({
            "name": name.get_value().trim(),
            "height": val(&height),
            "heating_set_point": val(&heat),
            "cooling_set_point": val(&cool),
            "humidity_min": val(&hmin),
            "humidity_max": val(&hmax),
            "outside_air_per_person": per_k(&oa_p),
            "outside_air_per_area": per_k(&oa_a),
        });
        f.dialog.destroy();
        Some(args)
    }

    /// The equipment or system to serve a zone (null: none), or none when cancelled.
    fn choose_zone_server(&mut self, z: &Value) -> Option<Value> {
        let store = self.store_now()?;
        let mut options: Vec<(String, Value)> = vec![("(none)".into(), Value::Null)];
        for s in reviera_core::mep_systems::systems(&store) {
            options.push((format!("System: {} ({})", s.name, s.type_name), json!(s.id)));
        }
        for e in store.iter() {
            let Some((tid, t)) = reviera_core::mesh_objects::type_of(&store, e) else { continue };
            if t.category == reviera_core::mesh_objects::Category::MechanicalEquipment && reviera_core::mesh_objects::mesh_object_recipe(e).is_some() {
                let name = e.name.clone().filter(|n| !n.is_empty()).or_else(|| store.get(tid)?.name.clone()).unwrap_or_default();
                options.push((format!("Equipment: {name} {}", e.id().0), json!(e.id())));
            }
        }
        let labels: Vec<&str> = options.iter().map(|o| o.0.as_str()).collect();
        let d = SingleChoiceDialog::builder(&self.frame, "Served by:", z["name"].as_str().unwrap_or("System-Zone"), &labels).build();
        let at = options.iter().position(|o| o.1 == z["served_by"]).unwrap_or(0);
        d.set_selection(at as i32);
        let chosen = (d.show_modal() == ID_OK).then(|| usize::try_from(d.get_selection()).ok()).flatten();
        d.destroy();
        chosen.and_then(|k| options.get(k)).map(|o| o.1.clone())
    }

    /// The outlines and names of the system-zones on a plan's level; selected ones lit.
    pub(crate) fn zone_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone() else { return Vec::new() };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let z = plan.range.cut as f32 + 0.002;
        let mut normal = Lines { color: ZONE_COLOR, width: 0.25, segments: Vec::new() };
        let mut lit = Lines { color: reviera_render::HIGHLIGHT, width: 0.4, segments: Vec::new() };
        for id in zones::all(&store) {
            let Some(e) = store.get(id) else { continue };
            let Some(g) = zones::geometry(&store, e).filter(|g| g.level == plan.level) else { continue };
            let target = if selected.contains(&id) { &mut lit } else { &mut normal };
            let n = g.outline.len();
            // Dashed: 600 mm on, 300 mm off at 1:100, scaled with the plan.
            let (on, off) = (6.0 * plan.scale as f64 / 1000.0, 3.0 * plan.scale as f64 / 1000.0);
            for i in 0..n {
                let (a, b) = (g.outline[i], g.outline[(i + 1) % n]);
                let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                let mut t = 0.0;
                while t < len {
                    let t1 = (t + on).min(len);
                    let at = |s: f64| [(a[0] + (b[0] - a[0]) * s / len) as f32, (a[1] + (b[1] - a[1]) * s / len) as f32, z];
                    target.segments.push([at(t), at(t1)]);
                    t = t1 + off;
                }
            }
            // The name near the first corner, inside.
            let c = g.outline.iter().fold([0.0, 0.0], |s, p| [s[0] + p[0] / n as f64, s[1] + p[1] / n as f64]);
            let p0 = g.outline[0];
            let at = [p0[0] + (c[0] - p0[0]) * 0.15, p0[1] + (c[1] - p0[1]) * 0.15];
            let labels = room_tag_labels(at, &[e.name.clone().unwrap_or_default().to_uppercase()], plan.scale, z);
            target.segments.extend(label_strokes(&labels));
        }
        vec![normal, lit]
    }
}
