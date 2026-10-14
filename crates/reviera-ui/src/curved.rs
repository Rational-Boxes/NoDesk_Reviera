// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curved roofs in the window (ARC-ROOF-008/009):
//! - **Arched Roof**: the roof footprint sketch (four sides: Pick Walls, Line or Rectangle), and
//!   Finish makes an arched roof springing from the longer pair of sides (`roof.create_arched`).
//! - **Dome**: choose the plan (circle or a polygon) and profile, then in a plan click the
//!   centre and a point on the springing (`roof.create_dome`). Esc ends.
//! - A selected arched roof or dome shows its own rows in Properties (`roof.set_arch`,
//!   `roof.set_dome`).

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

/// The Dome tool: the plan's sides (0 a circle), the profile, the level, and the centre once
/// clicked.
#[derive(Debug, Clone)]
pub struct DomeTool {
    sides: u32,
    profile: &'static str,
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
            level,
            center: None,
            cursor: None,
        });
        frame.set_status_text("Dome: click the centre", 0);
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
            return frame.set_status_text("Dome: click a point on the springing (the radius)", 0);
        };
        let r = (p[0] - c[0]).hypot(p[1] - c[1]);
        let rotation = (p[1] - c[1]).atan2(p[0] - c[0]);
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
            let (rise, oculus) = (self.format_length(d.rise), self.format_length(d.oculus));
            grid.append(Property::string("Rise", "do_rise", rise));
            grid.append(Property::string("Oculus", "do_oculus", oculus));
            grid.append(Property::boolean("Faceted", "do_faceted", d.faceted));
            grid.set_help_string(
                "do_oculus",
                "The opening's diameter at the crown; 0 for none",
            );
            return true;
        }
        false
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
}
