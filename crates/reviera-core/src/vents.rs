// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Vents of glazed enclosures (spec Ch 3 §3.6a.4, ARC-GLZ-004): their settings, free area and
//! the `Reviera_Vent` property set.
//!
//! A vent is a pane of a curtain wall or a sloped-glazing roof swapped for an opening one
//! (`Panel::Vent`), hung from its upper edge. Its **free area** is the opening it leaves when
//! open at its design angle θ: for a sash `w` along the hinge and `h` from it, the slot under
//! its lower edge, `w · 2h·sin(θ/2)`, and the two side triangles, `h²·sin θ` together, at most
//! the pane itself (`w · h`). The **effective area** is the free area times the discharge
//! coefficient. The settings are provisional (EXPERT_REVIEW E-54).
//!
//! `Reviera_Vent` goes on each vent `IfcWindow` a curtain wall writes (that vent's values), and
//! on every roof or curtain wall that has vents (their count and totals; a glazed roof's vents
//! are part of its one body in IFC).
//!
//! Not yet: opening and closing vents in the view (they are drawn 15° open), the actuator as an
//! `IfcActuator`, and roof vents as their own `IfcWindow`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::curtain::Panel;
use crate::element::Element;
use crate::error::Result;
use crate::id::ElementId;
use crate::store::Store;
use crate::txn::Transaction;

pub const PSET: &str = "Reviera_Vent";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Actuator {
    Manual,
    #[default]
    RackAndPinion,
    Chain,
    Linear,
}

/// How an enclosure's vents open.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VentSettings {
    /// The design opening angle (radians).
    #[serde(default = "opening_angle")]
    pub opening_angle: f64,
    /// Effective over free area.
    #[serde(default = "discharge")]
    pub discharge: f64,
    #[serde(default)]
    pub actuator: Actuator,
}

fn opening_angle() -> f64 {
    30f64.to_radians()
}

fn discharge() -> f64 {
    0.6
}

impl Default for VentSettings {
    fn default() -> Self {
        Self { opening_angle: opening_angle(), discharge: discharge(), actuator: Actuator::default() }
    }
}

/// A top-hung sash's free area, `w` along its hinge and `h` from it, open at `angle`.
pub fn free_area(w: f64, h: f64, angle: f64) -> f64 {
    let a = angle.clamp(0.0, std::f64::consts::FRAC_PI_2);
    (w * 2.0 * h * (a / 2.0).sin() + h * h * a.sin()).min(w * h)
}

/// An element's vents: their count, pane area, free and effective area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, JsonSchema)]
pub struct VentTotals {
    pub count: usize,
    pub pane_area: f64,
    pub free_area: f64,
    pub effective_area: f64,
}

impl VentTotals {
    fn add(&mut self, w: f64, h: f64, s: &VentSettings) {
        let free = free_area(w, h, s.opening_angle);
        self.count += 1;
        self.pane_area += w * h;
        self.free_area += free;
        self.effective_area += free * s.discharge;
    }
}

/// The vents of a curtain wall or a sloped-glazing roof (sashes: a curtain pane's width and
/// height; a roof pane's cell along the eave and up the slope).
pub fn totals(store: &Store, e: &Element, s: &VentSettings) -> VentTotals {
    let mut t = VentTotals::default();
    if crate::curtain::curtain_recipe(e).is_some() {
        if let Ok(g) = crate::curtain::regenerate(store, e) {
            for p in g.parts.iter().filter(|p| !p.mullion && p.panel == Panel::Vent) {
                t.add(p.x[1] - p.x[0], p.z[1] - p.z[0], s);
            }
        }
    } else if let Some(g) = crate::roofs::glazing::of(store, e) {
        for p in g.parts.iter().filter(|p| !p.mullion && p.panel == Panel::Vent) {
            if let Some([w, h]) = p.size {
                t.add(w, h, s);
            }
        }
    }
    t
}

fn typed(t: &str, v: Value) -> Value {
    json!({ "type": t, "value": v })
}

/// The settings' values, under `Reviera_Vent`.
fn setting_params(s: &VentSettings) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{PSET}.{p}");
    vec![
        (k("OpeningAngle"), typed("IfcPlaneAngleMeasure", json!(s.opening_angle))),
        (k("DischargeCoefficient"), typed("IfcNormalisedRatioMeasure", json!(s.discharge))),
        (k("Actuator"), typed("IfcLabel", serde_json::to_value(s.actuator).expect("serialises"))),
    ]
}

/// One vent's `Reviera_Vent` values (for a curtain wall's vent windows in IFC).
pub fn vent_params(w: f64, h: f64, s: &VentSettings) -> Vec<(String, Value)> {
    let free = free_area(w, h, s.opening_angle);
    let k = |p: &str| format!("{PSET}.{p}");
    let mut out = setting_params(s);
    out.push((k("FreeArea"), typed("IfcAreaMeasure", json!(free))));
    out.push((k("EffectiveArea"), typed("IfcAreaMeasure", json!(free * s.discharge))));
    out
}

/// An element's vent settings, if it has vents.
pub fn settings_of(e: &Element) -> Option<VentSettings> {
    let num = |p: &str| e.param(&format!("{PSET}.{p}")).and_then(|v| v.get("value")?.as_f64());
    let actuator = e
        .param(&format!("{PSET}.Actuator"))
        .and_then(|v| v.get("value")?.as_str().map(str::to_owned))
        .and_then(|a| serde_json::from_value(json!(a)).ok())
        .unwrap_or_default();
    Some(VentSettings { opening_angle: num("OpeningAngle")?, discharge: num("DischargeCoefficient").unwrap_or(discharge()), actuator })
}

/// Writes the settings and the vents' totals onto `id` when it has vents, and clears them when
/// it has none.
pub(crate) fn write_host(txn: &mut Transaction<'_>, id: ElementId, s: &VentSettings) -> Result<()> {
    let store = txn.store().clone();
    let t = totals(&store, store.get(id).expect("exists"), s);
    let k = |p: &str| format!("{PSET}.{p}");
    let mut values = setting_params(s);
    values.push((k("VentCount"), typed("IfcCountMeasure", json!(t.count))));
    values.push((k("FreeArea"), typed("IfcAreaMeasure", json!(t.free_area))));
    values.push((k("EffectiveArea"), typed("IfcAreaMeasure", json!(t.effective_area))));
    txn.modify(id, |e| {
        e.params.retain(|key, _| !key.starts_with(&format!("{PSET}.")));
        if t.count > 0 {
            for (key, v) in values {
                e.params.insert(key, v);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_top_hung_sash_opens_a_slot_and_two_triangles() {
        // A 1.125 × 1.0 m sash at 30°: 1.125 × 2 sin 15° + sin 30° = 0.5823 + 0.5.
        let a = free_area(1.125, 1.0, 30f64.to_radians());
        assert!((a - (1.125 * 2.0 * 15f64.to_radians().sin() + 0.5)).abs() < 1e-12);
        // Shut, nothing; flung wide, no more than the pane.
        assert_eq!(free_area(1.0, 1.0, 0.0), 0.0);
        assert_eq!(free_area(1.0, 1.0, 90f64.to_radians()), 1.0);
    }

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(crate::glazing_library::DEFINITIONS).unwrap();
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == PSET).expect("Reviera_Vent");
        let props = set["properties"].as_array().unwrap();
        let mut written = vent_params(1.0, 1.0, &VentSettings::default());
        written.push((format!("{PSET}.VentCount"), typed("IfcCountMeasure", json!(1))));
        for (key, value) in written {
            let name = key.strip_prefix(&format!("{PSET}.")).unwrap();
            let def = props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }
}
