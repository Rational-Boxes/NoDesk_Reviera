// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Panoramic cars and glass hoistways (ARC-VT-016, ARC-VT-017): an elevator's car glazed at the
//! rear or the rear and sides, and its hoistway glazed on the sides and rear as curtain walls of
//! laminated glass on steel posts, sized from the vertical-transport rule packs
//! ([`crate::elevators`]): the car glass's thickness for its panels (EN 81-20 Table 9), its mass,
//! the wider hoistway a side counterweight needs, and the glass checks.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::elevators::{Confidence, Drive, Pack};
use crate::error::Result;
use crate::id::ElementId;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// Where a car is glazed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CarGlass {
    #[default]
    None,
    /// The back wall.
    Rear,
    /// The back wall and both side walls.
    RearSides,
}

impl CarGlass {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }
}

/// What encloses the hoistway.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Hoistway {
    /// Walls on all four sides.
    #[default]
    Walls,
    /// A wall with the entrances at the front; glass on steel posts at the sides and rear.
    Glass,
}

// ---- Pack blocks ----

/// Glass rules for cars and hoistways.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rules {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The least laminated car-wall glass by the panel's inscribed circle: [circle, thickness]
    /// (m), rising.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub car_glass: Vec<V2>,
    /// A handrail band (m above the car floor) where car glass reaches below its top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handrail: Option<V2>,
    /// A glass hoistway serving more levels than this is noted as needing fire-rated glazing
    /// unless the authority accepts it unenclosed (an atrium).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rated_hoistway_levels: Option<u32>,
    pub source: String,
    pub confidence: Confidence,
}

/// Generic, maker-style panoramic sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sizing {
    /// Added to a traction hoistway's width when the car's rear is glazed and the counterweight
    /// moves to the side.
    pub counterweight_side: f64,
    /// Car glass where no pack's table asks more (m).
    pub car_glass: f64,
    /// Hoistway glass (m).
    pub shaft_glass: f64,
    /// The steel posts' width along the wall and depth through it.
    pub post: V2,
    /// The most between posts (wall fixings).
    pub post_spacing: f64,
    /// The largest pane for the hoistway glass, width × height.
    pub max_pane: V2,
    /// Glass mass per square metre per millimetre (kg).
    pub glass_density: f64,
    pub source: String,
    pub confidence: Confidence,
}

pub(crate) fn sizing(packs: &[Pack]) -> Option<(&Pack, &Sizing)> {
    packs.iter().find_map(|p| p.glass_sizing.as_ref().map(|s| (p, s)))
}

/// A car's glazing as sized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CarGlazing {
    pub glass: CarGlass,
    /// Laminated glass thickness (m).
    pub thickness: f64,
    /// Glazed area (m²) and its mass (kg).
    pub area: f64,
    pub mass: f64,
    /// The largest panel's inscribed circle (m).
    pub circle: f64,
}

/// The least thickness a pack's table asks for a panel's inscribed circle; beyond the table,
/// its last row (pendulum tests then apply).
pub(crate) fn table_thickness(rows: &[V2], circle: f64) -> Option<f64> {
    rows.iter().find(|r| circle <= r[0] + 1e-9).or(rows.last()).map(|r| r[1])
}

/// The panels of a car `[width, depth, height]` glazed as `glass`: each panel's width.
fn panels(glass: CarGlass, car: [f64; 3]) -> Vec<f64> {
    match glass {
        CarGlass::None => Vec::new(),
        CarGlass::Rear => vec![car[0]],
        CarGlass::RearSides => vec![car[0], car[1], car[1]],
    }
}

/// Sizes a car's glazing under the packs in use; `None` for an unglazed car.
pub fn car_glazing(packs: &[Pack], glass: CarGlass, car: [f64; 3], notes: &mut Vec<String>) -> Option<CarGlazing> {
    let widths = panels(glass, car);
    if widths.is_empty() {
        return None;
    }
    let default = sizing(packs).map(|(_, s)| s);
    let circle = widths.iter().map(|w| w.min(car[2])).fold(0.0, f64::max);
    let mut thickness = default.map_or(0.01076, |s| s.car_glass);
    for r in packs.iter().filter_map(|p| p.glass.as_ref()) {
        if let Some(t) = table_thickness(&r.car_glass, circle) {
            thickness = thickness.max(t);
            if r.car_glass.last().is_some_and(|l| circle > l[0] + 1e-9) {
                notes.push(format!("the car's glass panels ({circle:.2} m inscribed circle) are larger than {}'s table; pendulum tests decide their glass", r.id));
            }
        }
        if let Some([lo, hi]) = r.handrail {
            notes.push(format!("the glazed car needs a handrail {:.0}–{:.0} mm above its floor, fixed independently of the glass ({})", lo * 1000.0, hi * 1000.0, r.id));
        }
    }
    let area: f64 = widths.iter().map(|w| w * car[2]).sum();
    let density = default.map_or(2.5, |s| s.glass_density);
    Some(CarGlazing { glass, thickness, area, mass: area * thickness * 1000.0 * density, circle })
}

/// The width a glazed traction car's side counterweight adds to its hoistway.
pub fn hoistway_add(packs: &[Pack], glass: CarGlass, drive: Drive) -> f64 {
    if glass.is_none() || !matches!(drive, Drive::MrlTraction | Drive::Traction) {
        return 0.0;
    }
    sizing(packs).map_or(0.3, |(_, s)| s.counterweight_side)
}

/// The curtain wall type of glass hoistways for these sizes, made the first time: posts at most
/// the post spacing and the largest pane apart, transoms at most the pane height apart.
pub(crate) fn hoistway_type(txn: &mut Transaction<'_>, s: &Sizing) -> Result<ElementId> {
    let name = format!("Glass Hoistway {:.1} mm laminated, posts {:.0} × {:.0}", s.shaft_glass * 1000.0, s.post[0] * 1000.0, s.post[1] * 1000.0);
    if let Some(e) = txn.store().iter().find(|e| e.name.as_deref() == Some(name.as_str()) && crate::curtain::curtain_type(e).is_some()) {
        return Ok(e.id());
    }
    let post = crate::curtain::Mullion { width: s.post[0], depth: s.post[1] };
    let recipe = crate::curtain::CurtainType {
        kind: crate::curtain::CURTAIN_TYPE_RECIPE.into(),
        u: crate::curtain::Rule { layout: crate::curtain::Layout::MaximumSpacing, spacing: s.post_spacing.min(s.max_pane[0]), number: 0 },
        v: crate::curtain::Rule { layout: crate::curtain::Layout::MaximumSpacing, spacing: s.max_pane[1], number: 0 },
        interior: Some(post),
        border: Some(post),
        panel: crate::curtain::Panel::Glazed,
        panel_thickness: s.shaft_glass,
    };
    txn.create(
        crate::element::NewElement::new("IfcCurtainWallType")
            .name(&name)
            .param("@PredefinedType", "NOTDEFINED")
            .param(crate::walls::RECIPE_KEY, serde_json::to_value(recipe).expect("serialises")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packs(ids: &[&str]) -> Vec<Pack> {
        crate::elevators::shipped().iter().filter(|p| ids.contains(&p.id.as_str())).cloned().collect()
    }

    #[test]
    fn car_glass_follows_en_81_20_table_9() {
        let en = packs(&["en81", "practice_en"]);
        let mut notes = Vec::new();
        // A 1100 mm rear panel 2200 mm high: a 1.1 m inscribed circle, so 10 mm (5 + 5 + 0.76).
        let g = car_glazing(&en, CarGlass::Rear, [1.1, 1.4, 2.2], &mut notes).unwrap();
        assert_eq!((g.thickness, g.circle), (0.01076, 1.1));
        assert!((g.mass - 1.1 * 2.2 * 10.76 * 2.5).abs() < 1e-9);
        assert!(notes.iter().any(|n| n.contains("handrail")), "{notes:?}");
        // Wider than the table: its last row, and a note that pendulum tests decide.
        let mut notes = Vec::new();
        let wide = car_glazing(&en, CarGlass::RearSides, [2.1, 1.6, 2.3], &mut notes).unwrap();
        assert_eq!(wide.thickness, 0.01076);
        assert!(notes.iter().any(|n| n.contains("pendulum")), "{notes:?}");
        assert!(car_glazing(&en, CarGlass::None, [1.1, 1.4, 2.2], &mut notes).is_none());
        // Small panels need only 8.76 mm under the table; the generic 10.76 mm is kept.
        assert_eq!(table_thickness(&en[0].glass.as_ref().unwrap().car_glass, 0.9), Some(0.00876));
    }

    #[test]
    fn only_a_glazed_traction_car_moves_its_counterweight_aside() {
        let us = packs(&["practice_us"]);
        assert_eq!(hoistway_add(&us, CarGlass::Rear, Drive::MrlTraction), 0.3);
        assert_eq!(hoistway_add(&us, CarGlass::Rear, Drive::Hydraulic), 0.0);
        assert_eq!(hoistway_add(&us, CarGlass::None, Drive::Traction), 0.0);
    }
}
