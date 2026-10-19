// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Properties for analytical elements (STR-AM-001, 002, 003, 008): a member's role, section,
//! material, end releases and Analyze As; a panel's role, thickness, material, Analyze As and
//! cladding; a node's point, support and manually adjusted flag. They are selected through the
//! Analytical Checks pane, Insights or a script. Analyze › Analytical › Reset puts the selected
//! ones back where their physical elements put them.

use reviera_core::{ElementId, analytical, analytical_panels};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const ROLES: [(&str, &str); 5] = [
    ("Beam", "beam"),
    ("Column", "column"),
    ("Brace", "brace"),
    ("Truss member", "member"),
    ("Tie", "tie"),
];
const ANALYZE: [(&str, &str); 3] = [
    ("By releases", "releases"),
    ("Tension only", "tension_only"),
    ("Compression only", "compression_only"),
];
const RELEASES: [&str; 3] = ["Fixed", "Pinned", "Custom"];
const PANEL_ROLES: [(&str, &str); 3] = [("Floor", "floor"), ("Wall", "wall"), ("Roof", "roof")];
const PANEL_ANALYZE: [(&str, &str); 3] = [
    ("Shell", "shell"),
    ("Membrane", "membrane"),
    ("Bending plate", "bending_plate"),
];

fn choices(list: &[&str]) -> Vec<(String, i32)> {
    list.iter()
        .enumerate()
        .map(|(i, n)| ((*n).to_owned(), i as i32))
        .collect()
}

fn labelled(list: &[(&str, &str)]) -> Vec<(String, i32)> {
    list.iter()
        .enumerate()
        .map(|(i, (n, _))| ((*n).to_owned(), i as i32))
        .collect()
}

fn release_kind(flags: &[bool; 6]) -> usize {
    if flags.iter().all(|f| *f) {
        0
    } else if flags[..3].iter().all(|f| *f) && flags[3..].iter().all(|f| !*f) {
        1
    } else {
        2
    }
}

impl Gui {
    /// The rows of an analytical member, panel or node; `false` if `id` is none of them.
    pub(crate) fn analytical_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let pick = |list: &[(&str, &str)], v: serde_json::Value| {
            list.iter()
                .position(|(_, k)| v == *k)
                .map_or(-1, |i| i as i32)
        };
        if let Some(m) = analytical::member_recipe(e) {
            grid.append(Property::category("Analytical Member", "c_am"));
            grid.append(Property::enumeration(
                "Role",
                "am_role",
                labelled(&ROLES),
                pick(&ROLES, serde_json::to_value(m.role).unwrap_or_default()),
            ));
            let section = m
                .section_type
                .as_deref()
                .and_then(reviera_core::GlobalId::parse)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|t| store.get(t))
                .and_then(|t| t.name.clone())
                .unwrap_or_else(|| "—".into());
            grid.append(Property::string("Section Type", "am_section", section));
            grid.set_property_read_only("am_section", true);
            grid.append(Property::string(
                "Material",
                "am_material",
                m.material.clone().unwrap_or_default(),
            ));
            for (k, label) in [(0usize, "Start Release"), (1, "End Release")] {
                let name = if k == 0 { "am_start" } else { "am_end" };
                grid.append(Property::enumeration(
                    label,
                    name,
                    choices(&RELEASES),
                    release_kind(&m.releases[k]) as i32,
                ));
            }
            grid.append(Property::enumeration(
                "Analyze As",
                "am_analyze",
                labelled(&ANALYZE),
                pick(&ANALYZE, serde_json::to_value(m.analyze_as).unwrap_or_default()),
            ));
            let physical = m
                .physical
                .iter()
                .filter_map(|g| reviera_core::GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)))
                .map(|i| i.0.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            grid.append(Property::string(
                "Physical Elements",
                "am_physical",
                if physical.is_empty() { "None (analytical only)".into() } else { physical },
            ));
            grid.set_property_read_only("am_physical", true);
            return true;
        }
        if let Some(p) = analytical_panels::panel_recipe(e) {
            grid.append(Property::category("Analytical Panel", "c_ap"));
            grid.append(Property::enumeration(
                "Role",
                "ap_role",
                labelled(&PANEL_ROLES),
                pick(&PANEL_ROLES, serde_json::to_value(p.role).unwrap_or_default()),
            ));
            let t = self.format_length(p.thickness);
            grid.append(Property::string("Thickness", "ap_thickness", t));
            grid.append(Property::string(
                "Material",
                "ap_material",
                p.material.clone().unwrap_or_default(),
            ));
            grid.append(Property::enumeration(
                "Analyze As",
                "ap_analyze",
                labelled(&PANEL_ANALYZE),
                pick(&PANEL_ANALYZE, serde_json::to_value(p.analyze_as).unwrap_or_default()),
            ));
            grid.append(Property::enumeration(
                "Cladding",
                "ap_cladding",
                choices(&["No", "Yes"]),
                i32::from(p.cladding),
            ));
            grid.append(Property::string(
                "Openings",
                "ap_openings",
                p.openings.len().to_string(),
            ));
            grid.set_property_read_only("ap_openings", true);
            if let Some(g) = analytical_panels::geometry(&store, e) {
                grid.append(Property::string("Area", "ap_area", format!("{:.2} m²", g.area())));
                grid.set_property_read_only("ap_area", true);
            }
            return true;
        }
        if let Some(n) = analytical::node_recipe(e) {
            grid.append(Property::category("Analytical Node", "c_an"));
            let p = analytical::node_point(&store, e).unwrap_or_default();
            let text = format!(
                "{}, {}, {}",
                self.format_length(p[0]),
                self.format_length(p[1]),
                self.format_length(p[2])
            );
            grid.append(Property::string("Point", "an_point", text));
            grid.set_property_read_only("an_point", true);
            let states = ["None", "Pinned", "Fixed", "Roller", "User"];
            let at = match n.support.as_ref().map(|s| s.state) {
                None => 0,
                Some(reviera_core::loads::SupportState::Pinned) => 1,
                Some(reviera_core::loads::SupportState::Fixed) => 2,
                Some(reviera_core::loads::SupportState::Roller) => 3,
                Some(reviera_core::loads::SupportState::User) => 4,
            };
            grid.append(Property::enumeration("Support", "an_support", choices(&states), at));
            grid.append(Property::string(
                "Manually Adjusted",
                "an_adjusted",
                if n.adjusted { "Yes" } else { "No" },
            ));
            grid.set_property_read_only("an_adjusted", true);
            return true;
        }
        false
    }

    /// An edit of an analytical row; `false` if `property` is not one.
    pub(crate) fn analytical_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let prefix = &property[..property.len().min(3)];
        if !matches!(prefix, "am_" | "ap_" | "an_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let ids = self.selection();
        let lookup = |list: &[(&'static str, &'static str)]| -> Option<&'static str> {
            list.iter().find(|(n, _)| *n == text).map(|(_, k)| *k)
        };
        for id in ids {
            let result = match property {
                "am_role" => lookup(&ROLES).map(|k| self.run("analytical_member.set", json!({ "member_ids": [id], "role": k }))),
                "am_material" => Some(self.run("analytical_member.set", json!({ "member_ids": [id], "material": text }))),
                "am_analyze" => lookup(&ANALYZE).map(|k| self.run("analytical_member.set", json!({ "member_ids": [id], "analyze_as": k }))),
                "am_start" | "am_end" => {
                    let end = if property == "am_start" { "start" } else { "end" };
                    match text.as_str() {
                        "Fixed" | "Pinned" => Some(self.run(
                            "analytical_member.set_releases",
                            json!({ "member_ids": [id], "end": end, "preset": text.to_lowercase() }),
                        )),
                        _ => None,
                    }
                }
                "ap_role" => lookup(&PANEL_ROLES).map(|k| self.run("analytical_panel.set", json!({ "panel_id": id, "role": k }))),
                "ap_thickness" => Some(self.run("analytical_panel.set", json!({ "panel_id": id, "thickness": text }))),
                "ap_material" => Some(self.run("analytical_panel.set", json!({ "panel_id": id, "material": text }))),
                "ap_analyze" => lookup(&PANEL_ANALYZE).map(|k| self.run("analytical_panel.set", json!({ "panel_id": id, "analyze_as": k }))),
                "ap_cladding" => Some(self.run("analytical_panel.set_cladding", json!({ "panel_id": id, "cladding": text == "Yes" }))),
                "an_support" => Some(match text.as_str() {
                    "None" => self.run("boundary_condition.remove", json!({ "node_ids": [id] })),
                    s => self.run(
                        "boundary_condition.create",
                        json!({ "kind": "point", "host_id": id, "state": s.to_lowercase() }),
                    ),
                }),
                _ => None,
            };
            if let Some(Err(e)) = result {
                error(&frame, &format!("The change was not made:\n{e}"));
                break;
            }
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// Analyze › Analytical › Reset: the selected analytical elements (or those of the selected
    /// physical elements) back where their physical elements put them.
    pub(crate) fn reset_analytical(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select analytical elements, or physical elements with analytical ones", 0);
        }
        match self.run("analytical_model.reset", json!({ "element_ids": ids })) {
            Ok(r) => {
                let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
                frame.set_status_text(
                    &format!(
                        "{} reset; {} without a physical element left as they are; {} shared node(s) left behind (run the checks)",
                        n("reset"),
                        n("refused"),
                        n("disconnected")
                    ),
                    0,
                );
                self.refresh_scene(&frame);
                for i in 0..self.views.len() {
                    self.update_inspection(i);
                }
            }
            Err(e) => error(&frame, &e),
        }
    }
}

impl Gui {
    /// With Show Analytical on, the analytical element under pixel (`x`, `y`) of a 3D view: a
    /// node within 7 pixels, else a member within 5, else a panel's edge within 5.
    pub(crate) fn analytical_pick(&self, x: i32, y: i32, width: f32, height: f32) -> Option<ElementId> {
        if !self.show_analytical || self.active_view().is_none_or(|v| v.is_2d()) {
            return None;
        }
        let store = self.store_now()?;
        let cam = self.camera();
        let (px, py) = (x as f32, y as f32);
        let screen = |p: [f64; 3]| cam.project(glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), width, height);
        let seg = |a: (f32, f32), b: (f32, f32)| -> f32 {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 < 1e-6 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0) };
            ((a.0 + dx * t - px).powi(2) + (a.1 + dy * t - py).powi(2)).sqrt()
        };
        let node = analytical::nodes(&store)
            .into_iter()
            .filter_map(|(id, p)| {
                let s = screen(p)?;
                let d = ((s.0 - px).powi(2) + (s.1 - py).powi(2)).sqrt();
                (d <= 7.0).then_some((id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = node {
            return Some(id);
        }
        let member = analytical::members(&store)
            .into_iter()
            .filter_map(|(id, m)| {
                let d = seg(screen(m.start)?, screen(m.end)?);
                (d <= 5.0).then_some((id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = member {
            return Some(id);
        }
        analytical_panels::panels(&store)
            .into_iter()
            .filter_map(|(id, g)| {
                let n = g.outline.len();
                let d = (0..n)
                    .filter_map(|i| Some(seg(screen(g.outline[i])?, screen(g.outline[(i + 1) % n])?)))
                    .fold(f32::INFINITY, f32::min);
                (d <= 5.0).then_some((id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}
