// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curved roofs in the window (ARC-ROOF-008/009):
//! - **Arched Roof**: the roof footprint sketch (four sides: Pick Walls, Line or Rectangle), and
//!   Finish makes an arched roof springing from the longer pair of sides (`roof.create_arched`).
//! - **Dome**: choose the plan (circle or a polygon) and profile, then in a plan click the
//!   centre and a point on the springing (`roof.create_dome`). Esc ends.
//! - **Geodesic Dome** (ARC-GLZ-006): choose the frequency and truncation, then click the
//!   centre and a point on the base ring (`roof.create_geodesic`).
//! - A selected arched roof or dome shows its own rows in Properties (`roof.set_arch`,
//!   `roof.set_dome`), and a geodesic dome its sizes, glazing, frame and struts
//!   (`roof.set_geodesic`, `roof.geodesic`).

use reviera_core::ElementId;
use reviera_core::roofs;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.85, 0.0, 0.75, 1.0];

const PLANS: &[(&str, u32)] = &[
    ("Circle", 0),
    ("Square (cloister vault)", 4),
    ("Hexagon (cloister vault)", 6),
    ("Octagon (cloister vault)", 8),
];
const DOME_PROFILES: &[(&str, &str)] = &[
    ("Hemisphere", "hemisphere"),
    ("Segmental", "segmental"),
    ("Pointed", "pointed"),
    ("Onion", "onion"),
];
const ARCH_PROFILES: &[(&str, &str)] = &[
    ("Circular Segment", "segment"),
    ("Semicircle", "semicircle"),
    ("Elliptical", "elliptical"),
    ("Catenary", "catenary"),
];
const TAPERS: &[(&str, &str)] = &[
    ("Constant Rise", "constant_rise"),
    ("Constant Slope", "constant_slope"),
];

const TRIANGULATE: [(&str, roofs::curved::Triangulate); 3] = [
    ("Where not flat", roofs::curved::Triangulate::Auto),
    ("Always", roofs::curved::Triangulate::Always),
    ("Never", roofs::curved::Triangulate::Never),
];
const FREQUENCIES: &[(&str, u32)] = &[("2V", 2), ("3V", 3), ("4V", 4), ("5V", 5), ("6V", 6)];
const TRUNCATIONS: &[(&str, &str)] = &[("1/2 (hemisphere)", "1/2"), ("3/8", "3/8"), ("5/8", "5/8"), ("3/4", "3/4")];

/// The Dome tool: the plan's sides (0 a circle), the profile, the level, and the centre once
/// clicked; or a geodesic dome's frequency and truncation.
#[derive(Debug, Clone)]
pub struct DomeTool {
    sides: u32,
    profile: &'static str,
    geodesic: Option<(u32, &'static str)>,
    level: ElementId,
    center: Option<[f64; 2]>,
    cursor: Option<[f64; 2]>,
}

fn pick(frame: &Frame, title: &str, labels: &[&str]) -> Option<usize> {
    let d = SingleChoiceDialog::builder(frame, title, "Dome", labels).build();
    if d.show_modal() != ID_OK {
        return None;
    }
    usize::try_from(d.get_selection()).ok()
}

impl Gui {
    /// Architecture › Arched Roof: the footprint sketch, finished as an arched roof.
    pub(crate) fn start_arched_roof_sketch(&mut self) {
        self.start_roof_sketch();
        if let Some(s) = &mut self.sketch {
            s.arched = true;
            self.frame.set_status_text(
                "Arched Roof: sketch a four-sided footprint; it springs from the longer pair of sides",
                0,
            );
        }
    }

    /// Architecture › Dome.
    pub(crate) fn start_dome_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return;
        };
        let plans: Vec<&str> = PLANS.iter().map(|p| p.0).collect();
        let Some(plan) = pick(&frame, "Dome plan:", &plans) else {
            return;
        };
        let profiles: Vec<&str> = DOME_PROFILES.iter().map(|p| p.0).collect();
        let Some(profile) = pick(&frame, "Dome profile:", &profiles) else {
            return;
        };
        self.end_tool(&frame);
        self.dome_tool = Some(DomeTool {
            sides: PLANS[plan].1,
            profile: DOME_PROFILES[profile].1,
            geodesic: None,
            level,
            center: None,
            cursor: None,
        });
        frame.set_status_text("Dome: click the centre", 0);
    }

    /// Architecture › Geodesic Dome: the frequency and truncation, then the dome tool's clicks.
    pub(crate) fn start_geodesic_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else {
            return;
        };
        let labels: Vec<&str> = FREQUENCIES.iter().map(|f| f.0).collect();
        let Some(f) = pick(&frame, "Frequency (more, shorter struts and a rounder dome):", &labels) else {
            return;
        };
        let labels: Vec<&str> = TRUNCATIONS.iter().map(|t| t.0).collect();
        let Some(t) = pick(&frame, "Truncation (the share of the sphere kept):", &labels) else {
            return;
        };
        self.end_tool(&frame);
        self.dome_tool = Some(DomeTool { sides: 0, profile: "hemisphere", geodesic: Some((FREQUENCIES[f].1, TRUNCATIONS[t].1)), level, center: None, cursor: None });
        frame.set_status_text("Geodesic Dome: click the centre", 0);
    }

    pub(crate) fn end_dome_tool(&mut self) -> bool {
        if self.dome_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    pub(crate) fn dome_motion(&mut self, x: i32, y: i32) {
        let p = self.plan_xy(x, y);
        let Some(t) = &mut self.dome_tool else { return };
        t.cursor = p;
        let t = t.clone();
        let (Some(c), Some(p)) = (t.center, t.cursor) else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let r = (p[0] - c[0]).hypot(p[1] - c[1]);
        let z = plan.range.cut as f32 + 0.004;
        let n = if t.sides == 0 { 48 } else { t.sides as usize };
        let a0 = (p[1] - c[1]).atan2(p[0] - c[0]);
        let at = |k: usize| {
            let a = a0 + std::f64::consts::TAU * k as f64 / n as f64;
            [(c[0] + r * a.cos()) as f32, (c[1] + r * a.sin()) as f32, z]
        };
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: (0..n).map(|k| [at(k), at(k + 1)]).collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    pub(crate) fn dome_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(t) = self.dome_tool.clone() else {
            return;
        };
        let Some(c) = t.center else {
            if let Some(t) = &mut self.dome_tool {
                t.center = Some(p);
            }
            return frame.set_status_text(if t.geodesic.is_some() { "Geodesic Dome: click a point at the sphere's radius" } else { "Dome: click a point on the springing (the radius)" }, 0);
        };
        let r = (p[0] - c[0]).hypot(p[1] - c[1]);
        let rotation = (p[1] - c[1]).atan2(p[0] - c[0]);
        // A geodesic dome: the click is on its base ring, which for a cut above the equator is
        // inside the sphere; the radius is the sphere's.
        if let Some((frequency, truncation)) = t.geodesic {
            let args = json!({ "level_id": t.level, "center": c, "radius": r.max(0.5), "frequency": frequency, "truncation": truncation, "rotation": rotation });
            match self.run("roof.create_geodesic", args) {
                Ok(v) => {
                    self.end_dome_tool();
                    self.refresh_scene(&frame);
                    let _ = self.run("selection.set", json!({ "ids": [v] }));
                    self.selection_changed();
                    frame.set_status_text("Geodesic dome placed", 0);
                }
                Err(e) => error(&frame, &format!("The dome was not made:\n{e}")),
            }
            return;
        }
        let plan = if t.sides == 0 {
            json!({ "circle": { "radius": r } })
        } else {
            json!({ "polygon": { "radius": r, "sides": t.sides, "rotation": rotation } })
        };
        // On the walls it covers (a drum), else on the top level's walls, like a roof.
        let base = self
            .walls_top_within(t.level, c, r + 0.3)
            .or_else(|| self.top_level_wall_top(t.level))
            .unwrap_or(0.0);
        let mut args = json!({
            "level_id": t.level, "center": c, "plan": plan, "profile": t.profile,
            "base_offset": base,
        });
        if matches!(t.profile, "segmental" | "onion") {
            args["rise"] = json!(r * if t.profile == "onion" { 1.6 } else { 0.5 });
        }
        match self.run("roof.create_dome", args) {
            Ok(v) => {
                self.end_dome_tool();
                self.refresh_scene(&frame);
                let _ = self.run("selection.set", json!({ "ids": [v] }));
                self.selection_changed();
                frame.set_status_text("Dome placed", 0);
            }
            Err(e) => error(&frame, &format!("The dome was not made:\n{e}")),
        }
    }

    /// The highest top, above `level`, of the level's walls lying within `radius` of `c`.
    fn walls_top_within(&self, level: ElementId, c: [f64; 2], radius: f64) -> Option<f64> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let z = crate::levels_elevation(store, level)?;
        let guid = store.get(level)?.guid().to_string();
        let near = |p: [f64; 2]| (p[0] - c[0]).hypot(p[1] - c[1]) <= radius;
        store
            .iter()
            .filter(|e| reviera_core::walls::wall_recipe(e).is_some_and(|r| r.base_level == guid))
            .filter_map(|e| reviera_core::walls::regenerate(store, e).ok())
            .filter(|g| near(g.start) && near(g.end))
            .map(|g| g.base_z + g.height - z)
            .reduce(f64::max)
    }

    /// Rows of a selected arched roof or dome; `true` if it is one.
    pub(crate) fn curved_roof_rows(
        &mut self,
        grid: &PropertyGrid,
        recipe: &roofs::RoofRecipe,
        glazed: bool,
    ) -> bool {
        let enumeration = |label: &str, key: &str, list: &[(&str, &str)], value: &str| {
            let index = list
                .iter()
                .position(|c| c.1 == value)
                .map_or(-1, |i| i as i32);
            Property::enumeration(
                label,
                key,
                list.iter()
                    .enumerate()
                    .map(|(i, c)| (c.0.to_owned(), i as i32))
                    .collect::<Vec<(String, i32)>>(),
                index,
            )
        };
        let name = |v: serde_json::Value| v.as_str().unwrap_or_default().to_owned();
        if let Some(a) = &recipe.arch {
            grid.append(Property::category("Arch", "c_arch"));
            grid.append(enumeration(
                "Profile",
                "ar_profile",
                ARCH_PROFILES,
                &name(json!(a.profile)),
            ));
            grid.append(enumeration(
                "Taper",
                "ar_taper",
                TAPERS,
                &name(json!(a.taper)),
            ));
            let rows = [
                ("Rise", "ar_rise", a.rise),
                ("Overhang", "ar_overhang", a.overhang),
                ("End Overhang", "ar_end_overhang", a.end_overhang),
            ];
            for (label, key, v) in rows {
                grid.append(Property::string(label, key, self.format_length(v)));
            }
            grid.set_help_string(
                "ar_taper",
                "Over a tapering footprint: keep the rise, or scale it with the span",
            );
            if glazed {
                self.curved_glazing_rows(grid, recipe, true);
            }
            return true;
        }
        if let Some(d) = &recipe.dome {
            grid.append(Property::category("Dome", "c_dome"));
            grid.append(enumeration(
                "Profile",
                "do_profile",
                DOME_PROFILES,
                &name(json!(d.profile)),
            ));
            let (cx, cy) = (
                self.format_length(d.center[0]),
                self.format_length(d.center[1]),
            );
            grid.append(Property::string("Centre X", "do_cx", cx));
            grid.append(Property::string("Centre Y", "do_cy", cy));
            let (plan, rows): (&str, Vec<(&str, &str, String)>) = match &d.plan {
                roofs::curved::DomePlan::Circle { radius } => (
                    "Circle",
                    vec![("Radius", "do_radius", self.format_length(*radius))],
                ),
                roofs::curved::DomePlan::Ellipse { radii, rotation } => (
                    "Ellipse",
                    vec![
                        ("Radius 1", "do_r0", self.format_length(radii[0])),
                        ("Radius 2", "do_r1", self.format_length(radii[1])),
                        (
                            "Rotation",
                            "do_rot",
                            format!("{:.1}°", rotation.to_degrees()),
                        ),
                    ],
                ),
                roofs::curved::DomePlan::Polygon {
                    radius,
                    sides,
                    rotation,
                } => (
                    "Polygon",
                    vec![
                        ("Radius", "do_radius", self.format_length(*radius)),
                        ("Sides", "do_sides", sides.to_string()),
                        (
                            "Rotation",
                            "do_rot",
                            format!("{:.1}°", rotation.to_degrees()),
                        ),
                    ],
                ),
            };
            grid.append(Property::string("Plan", "do_plan", plan));
            grid.set_property_read_only("do_plan", true);
            for (label, key, v) in rows {
                grid.append(Property::string(label, key, v));
            }
            grid.set_help_string(
                "do_radius",
                "A circle's radius, or a polygon's to its corners; a hemisphere's rise follows",
            );
            if d.profile != roofs::curved::DomeProfile::Hemisphere {
                let ratio = d.ratio.map_or(String::new(), |r| format!("{r}"));
                grid.append(Property::string("Ratio", "do_ratio", ratio));
                grid.set_help_string(
                    "do_ratio",
                    "Pointed: the arcs' radius over the span (1 when empty); onion: the bulge (1.25)",
                );
            }
            let (rise, oculus) = (self.format_length(d.rise), self.format_length(d.oculus));
            grid.append(Property::string("Rise", "do_rise", rise));
            grid.append(Property::string("Oculus", "do_oculus", oculus));
            grid.append(Property::boolean("Faceted", "do_faceted", d.faceted));
            grid.set_help_string(
                "do_oculus",
                "The opening's diameter at the crown; 0 for none",
            );
            if glazed {
                self.curved_glazing_rows(grid, recipe, false);
            }
            return true;
        }
        false
    }

    /// A glazed vault's or dome's panel grid (ARC-GLZ-006).
    fn curved_glazing_rows(&mut self, grid: &PropertyGrid, recipe: &roofs::RoofRecipe, vault: bool) {
        let g = recipe.curved_glazing.unwrap_or_default();
        grid.append(Property::category("Glazing Grid", "c_curved_glazing"));
        let auto = || "(from the glazing type)".to_owned();
        let u = if vault { g.u.map_or_else(auto, |v| self.format_length(v)) } else { g.u.map_or_else(auto, |v| format!("{v}")) };
        grid.append(Property::string(if vault { "Bay Spacing" } else { "Meridians" }, "cg_u", u));
        grid.append(Property::string(if vault { "Divisions" } else { "Rings" }, "cg_v", g.v.map_or_else(auto, |v| format!("{v}"))));
        let at = TRIANGULATE.iter().position(|t| t.1 == g.triangulate).unwrap_or(0) as i32;
        grid.append(Property::enumeration("Triangulate", "cg_triangulate", TRIANGULATE.iter().enumerate().map(|(i, t)| (t.0.to_owned(), i as i32)).collect::<Vec<_>>(), at));
        grid.set_help_string("cg_triangulate", "Split four-cornered panels into two triangles: only where they cannot be flat, always, or never");
        grid.set_help_string("cg_u", if vault { "The bays along the vault, at most this far apart; empty for the glazing type's spacing" } else { "The meridians round the dome; empty for the glazing type's spacing" });
    }

    /// An edit of a glazed vault's or dome's grid row; `false` if `property` is not one.
    pub(crate) fn curved_glazing_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("cg_") {
            return false;
        }
        let frame = self.frame;
        let Some(roof) = self.selection().first().copied() else { return true };
        let text = text.unwrap_or_default();
        let blank = text.trim().is_empty() || text.starts_with('(');
        let mut args = json!({ "roof_id": roof });
        match property {
            "cg_u" => args["grid_u"] = if blank { serde_json::Value::Null } else { text.trim().parse::<f64>().map_or_else(|_| json!(text.trim()), |n| json!(n)) },
            "cg_v" => match text.trim().parse::<f64>() {
                Ok(n) if !blank => args["grid_v"] = json!(n),
                _ => return true,
            },
            "cg_triangulate" => match TRIANGULATE.iter().find(|t| t.0 == text) {
                Some(t) => args["triangulate"] = json!(t.1),
                None => return true,
            },
            _ => return true,
        }
        match self.run("roof.set_glazing", args) {
            Ok(v) => frame.set_status_text(&format!("Glazing: {} panels ({} triangles), {} bars", v["panels"], v["triangles"], v["bars"]), 0),
            Err(e) => error(&frame, &format!("The glazing was not changed:\n{e}")),
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// An edit of an arch or dome row; `false` if `property` is not one.
    pub(crate) fn curved_roof_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !(property.starts_with("ar_") || property.starts_with("do_")) {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let Some(roof) = self.selection().first().copied() else {
            return true;
        };
        let choice = |list: &'static [(&'static str, &'static str)]| {
            list.iter().find(|c| c.0 == text).map(|c| c.1)
        };
        if let Some(r) = self.dome_plan_edit(property, &text, roof) {
            if let Err(e) = r {
                error(&frame, &format!("The roof was not changed:\n{e}"));
            }
            self.properties_stale = true;
            self.refresh_scene(&frame);
            return true;
        }
        let (command, field, value) = match property {
            "ar_profile" => (
                "roof.set_arch",
                "profile",
                choice(ARCH_PROFILES).map(|v| json!(v)),
            ),
            "ar_taper" => ("roof.set_arch", "taper", choice(TAPERS).map(|v| json!(v))),
            "ar_rise" => ("roof.set_arch", "rise", Some(json!(text))),
            "ar_overhang" => ("roof.set_arch", "overhang", Some(json!(text))),
            "ar_end_overhang" => ("roof.set_arch", "end_overhang", Some(json!(text))),
            "do_profile" => (
                "roof.set_dome",
                "profile",
                choice(DOME_PROFILES).map(|v| json!(v)),
            ),
            "do_rise" => ("roof.set_dome", "rise", Some(json!(text))),
            "do_oculus" => ("roof.set_dome", "oculus", Some(json!(text))),
            "do_faceted" => (
                "roof.set_dome",
                "faceted",
                Some(json!(text == "true" || text == "1")),
            ),
            _ => return false,
        };
        let Some(value) = value else { return true };
        let mut args = json!({ "roof_id": roof });
        args[field] = value;
        if let Err(e) = self.run(command, args) {
            error(&frame, &format!("The roof was not changed:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// A geodesic dome's rows (ARC-GLZ-006): sizes, glazing and frame, and the fabrication report.
    pub(crate) fn geodesic_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some(info) = self.run("roof.geodesic", json!({ "roof_id": id })).ok() else { return };
        let Ok(r) = serde_json::from_value::<reviera_core::geodesic::DomeRecipe>(info["recipe"].clone()) else { return };
        grid.append(Property::category("Geodesic Dome", "c_geodesic"));
        grid.append(Property::string("Radius", "gd_radius", self.format_length(r.radius)));
        let freq = FREQUENCIES.iter().position(|f| f.1 == r.frequency);
        let mut freqs: Vec<(String, i32)> = FREQUENCIES.iter().enumerate().map(|(i, f)| (f.0.to_owned(), i as i32)).collect();
        if freq.is_none() {
            freqs.push((format!("{}V", r.frequency), freqs.len() as i32));
        }
        let at = freq.map_or(freqs.len() as i32 - 1, |i| i as i32);
        grid.append(Property::enumeration("Frequency", "gd_frequency", freqs, at));
        grid.append(Property::string("Truncation", "gd_truncation", format!("{}", r.truncation)));
        grid.set_help_string("gd_truncation", "The share of the sphere's diameter kept: 0.5 a hemisphere, 0.625 (5/8) taller; cut at the nearest ring of hubs");
        grid.append(Property::string("Base Offset", "gd_base_offset", self.format_length(r.base_offset)));
        let names = |list: Vec<&'static str>, current: &Option<String>| {
            let mut v = vec!["Default".to_owned()];
            v.extend(list.into_iter().map(str::to_owned));
            let at = current.as_ref().and_then(|c| v.iter().position(|n| n.eq_ignore_ascii_case(c))).unwrap_or(0) as i32;
            (v.into_iter().enumerate().map(|(i, n)| (n, i as i32)).collect::<Vec<_>>(), at)
        };
        let (g, gi) = names(reviera_core::glazing_library::library().iter().map(|e| e.name).collect(), &r.glazing);
        grid.append(Property::enumeration("Glazing", "gd_glazing", g, gi));
        let (f, fi) = names(reviera_core::frame_library::library().iter().map(|e| e.name).collect(), &r.frame_system);
        grid.append(Property::enumeration("Frame System", "gd_frame_system", f, fi));
        let read_only = [
            ("Height", "gd_height", self.format_length(info["height"].as_f64().unwrap_or(0.0))),
            ("Base Radius", "gd_base_radius", self.format_length(info["base_radius"].as_f64().unwrap_or(0.0))),
            ("Level Base", "gd_level_base", if info["level_base"].as_bool() == Some(true) { "Yes".to_owned() } else { "No (steps between hub rings)".to_owned() }),
            ("Hubs / Panels / Struts", "gd_counts", format!("{} / {} / {}", info["hubs"], info["panels"], info["struts"])),
            ("Glass Area", "gd_glass", format!("{:.1} m²", info["glass_area"].as_f64().unwrap_or(0.0))),
        ];
        for (label, key, value) in read_only {
            grid.append(Property::string(label, key, value));
            grid.set_property_read_only(key, true);
        }
        let classes: Vec<String> = info["strut_classes"]
            .as_array()
            .map(|a| a.iter().map(|c| format!("{} {} × {}", c["label"].as_str().unwrap_or(""), c["count"], self.format_length(c["length"].as_f64().unwrap_or(0.0)))).collect())
            .unwrap_or_default();
        grid.append(Property::string("Struts", "gd_struts", classes.join(", ")));
        grid.set_property_read_only("gd_struts", true);
        grid.set_help_string("gd_struts", "Strut length classes, shortest first: label, count, hub-to-hub length");
    }

    /// An edit of a geodesic dome row; `false` if `property` is not one.
    pub(crate) fn geodesic_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("gd_") {
            return false;
        }
        let frame = self.frame;
        let Some(roof) = self.selection().first().copied() else { return true };
        let text = text.unwrap_or_default();
        let default = |t: &str| if t == "Default" { String::new() } else { t.to_owned() };
        let mut args = json!({ "roof_id": roof });
        match property {
            "gd_radius" => args["radius"] = json!(text),
            "gd_frequency" => match FREQUENCIES.iter().find(|f| f.0 == text) {
                Some(f) => args["frequency"] = json!(f.1),
                None => return true,
            },
            "gd_truncation" => args["truncation"] = json!(text.trim()),
            "gd_base_offset" => args["base_offset"] = json!(text),
            "gd_glazing" => args["glazing"] = json!(default(&text)),
            "gd_frame_system" => args["frame_system"] = json!(default(&text)),
            _ => return true,
        }
        if let Err(e) = self.run("roof.set_geodesic", args) {
            error(&frame, &format!("The dome was not changed:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// An edit of a dome's centre or plan; `None` if `property` is not one.
    fn dome_plan_edit(
        &mut self,
        property: &str,
        text: &str,
        roof: reviera_core::ElementId,
    ) -> Option<Result<(), String>> {
        use roofs::curved::DomePlan;
        if !matches!(
            property,
            "do_cx"
                | "do_cy"
                | "do_radius"
                | "do_r0"
                | "do_r1"
                | "do_sides"
                | "do_rot"
                | "do_ratio"
        ) {
            return None;
        }
        let d = self
            .doc
            .and_then(|d| self.app.document(d))
            .and_then(|d| d.doc.store().get(roof).and_then(roofs::roof_recipe))
            .and_then(|r| r.dome)?;
        let length = |g: &mut Gui| -> Result<f64, String> {
            g.run(
                "units.parse",
                json!({ "text": text, "data_type": "length" }),
            )
            .map(|v| v.as_f64().unwrap_or_default())
        };
        let number = || -> Result<f64, String> {
            text.trim()
                .trim_end_matches('°')
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("`{text}` is not a number"))
        };
        let mut args = json!({ "roof_id": roof });
        let result = (|| -> Result<(), String> {
            match property {
                "do_cx" => args["center"] = json!([length(self)?, d.center[1]]),
                "do_cy" => args["center"] = json!([d.center[0], length(self)?]),
                "do_ratio" => args["ratio"] = json!(number()?),
                _ => {
                    let mut plan = d.plan.clone();
                    match (&mut plan, property) {
                        (
                            DomePlan::Circle { radius } | DomePlan::Polygon { radius, .. },
                            "do_radius",
                        ) => *radius = length(self)?,
                        (DomePlan::Ellipse { radii, .. }, "do_r0") => radii[0] = length(self)?,
                        (DomePlan::Ellipse { radii, .. }, "do_r1") => radii[1] = length(self)?,
                        (DomePlan::Polygon { sides, .. }, "do_sides") => {
                            let n = number()?;
                            if n < 3.0 || n.fract() != 0.0 {
                                return Err("a polygon has 3 or more whole sides".to_owned());
                            }
                            *sides = n as u32;
                        }
                        (
                            DomePlan::Ellipse { rotation, .. } | DomePlan::Polygon { rotation, .. },
                            "do_rot",
                        ) => *rotation = number()?.to_radians(),
                        _ => return Ok(()),
                    }
                    args["plan"] = serde_json::to_value(&plan).expect("serialises");
                }
            }
            self.run("roof.set_dome", args.clone()).map(|_| ())
        })();
        Some(result)
    }
}
