// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Generic furniture and equipment (ARC-FURN-001): the catalogue the bundled Component Libraries
//! are made from (`libraries/`, DM-LIB-006). Each item is a placeholder at the size research/15
//! gives (its sources named), with its IFC class and predefined type or kind, so the shared rule
//! packs select it (ARC-RULE-002), and its Uniclass code for the libraries' tags. Shapes are simple prisms: an item's front, the
//! side a person uses, is its −y side; it is centred on its insertion point in plan and stands on
//! the floor.
//!
//! `furnishing.load` loads items into a project as types. The bundled libraries hold them as
//! model fragments (DM-LIB-003), one IFC file an item, written by Riviera from this catalogue.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bath_components::{Shape, rounded};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::mep::{Connector, FlowDirection as D};
use crate::mesh_objects::Category as C;
use crate::txn::Transaction;

/// Marks a type as one of these items.
pub const MARK: &str = "rxd:furnishing";

const IN: f64 = 0.0254;

/// Which bundled library an item goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Library {
    Interiors,
    Gym,
    Retail,
}

impl Library {
    pub const ALL: [Library; 3] = [Library::Interiors, Library::Gym, Library::Retail];

    /// Its folder under `libraries/`.
    pub fn dir(self) -> &'static str {
        match self {
            Library::Interiors => "interiors",
            Library::Gym => "gym",
            Library::Retail => "retail",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Library::Interiors => "Riviera Interiors",
            Library::Gym => "Riviera Gym Equipment",
            Library::Retail => "Riviera Retail and Checkout Fixtures",
        }
    }
}

/// One item.
#[derive(Debug)]
pub struct Item {
    pub library: Library,
    pub folder: &'static str,
    pub name: &'static str,
    pub category: C,
    /// Its IFC predefined type; for USERDEFINED, `kind` is its ElementType.
    pub predefined: &'static str,
    pub kind: Option<&'static str>,
    pub uniclass: Option<&'static str>,
    pub color: [f32; 4],
    /// What it is, and where its size comes from.
    pub description: &'static str,
    /// Source IDs in SOURCE_REGISTER.md; empty when its size is judgement (EXPERT_REVIEW E-103).
    pub source: &'static str,
    shape: fn() -> Shape,
    connectors: fn() -> Vec<Connector>,
    /// Properties on its type (a light fixture's mounting and placing types and wattage).
    params: fn() -> Vec<(&'static str, serde_json::Value)>,
}

impl Item {
    pub(crate) fn shape(&self) -> Shape {
        (self.shape)()
    }
}

fn none() -> Vec<Connector> {
    Vec::new()
}

fn no_params() -> Vec<(&'static str, serde_json::Value)> {
    Vec::new()
}

/// A light fixture's `Pset_LightFixtureTypeCommon`: mounting, placing and wattage
/// (research/15 D4).
fn fixture(mounting: &str, placing: &str, watts: f64) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("Pset_LightFixtureTypeCommon.LightFixtureMountingType", serde_json::json!({ "type": "IfcLabel", "value": mounting })),
        ("Pset_LightFixtureTypeCommon.LightFixturePlacingType", serde_json::json!({ "type": "IfcLabel", "value": placing })),
        ("Pset_LightFixtureTypeCommon.TotalWattage", serde_json::json!({ "type": "IfcPowerMeasure", "value": watts })),
    ]
}

const WOOD: [f32; 4] = [0.62, 0.47, 0.32, 1.0];
const LINEN: [f32; 4] = [0.88, 0.86, 0.80, 1.0];
const FABRIC: [f32; 4] = [0.42, 0.48, 0.55, 1.0];
const BLACK: [f32; 4] = [0.18, 0.18, 0.20, 1.0];
const STEEL: [f32; 4] = [0.66, 0.68, 0.70, 1.0];
const WHITE: [f32; 4] = [0.93, 0.93, 0.92, 1.0];
const GLASS: [f32; 4] = [0.75, 0.85, 0.90, 0.6];
const RUBBER: [f32; 4] = [0.25, 0.25, 0.27, 1.0];
const BRASS: [f32; 4] = [0.78, 0.64, 0.32, 1.0];

// ---- Shapes ----

/// Four legs `t` square under a top `w` × `d` from `z0` to `z1`.
fn legs(s: Shape, w: f64, d: f64, t: f64, z1: f64) -> Shape {
    let (x, y) = (w / 2.0 - t, d / 2.0 - t);
    let mut s = s;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (cx, cy) = (sx * x, sy * y);
        s = s.cuboid([cx - t / 2.0, cx + t / 2.0], [cy - t / 2.0, cy + t / 2.0], [0.0, z1]);
    }
    s
}

/// A table: a top 30 mm thick at `h`, on four legs.
fn table(w: f64, d: f64, h: f64) -> Shape {
    legs(Shape::default().cuboid([-w / 2.0, w / 2.0], [-d / 2.0, d / 2.0], [h - 0.03, h]), w, d, 0.05, h - 0.03)
}

/// A round table on a pedestal.
fn round_table(dia: f64, h: f64) -> Shape {
    Shape::default()
        .prism(&rounded([0.0, 0.0], dia, dia, dia / 2.0), h - 0.03, h)
        .prism(&rounded([0.0, 0.0], 0.1, 0.1, 0.05), 0.04, h - 0.03)
        .prism(&rounded([0.0, 0.0], dia * 0.55, dia * 0.55, dia * 0.275), 0.0, 0.04)
}

/// A chair: a seat at `seat`, legs, and a back at +y up to `top`.
fn chair(w: f64, d: f64, seat: f64, top: f64) -> Shape {
    legs(Shape::default().cuboid([-w / 2.0, w / 2.0], [-d / 2.0, d / 2.0], [seat - 0.04, seat]), w, d, 0.035, seat - 0.04)
        .cuboid([-w / 2.0, w / 2.0], [d / 2.0 - 0.04, d / 2.0], [seat, top])
}

/// An upholstered seat (sofa, armchair): a base to the seat, a back at +y, an arm each side.
fn sofa(w: f64, d: f64, seat: f64, top: f64, arm: f64) -> Shape {
    let back = 0.2;
    Shape::default()
        .cuboid([-w / 2.0 + arm, w / 2.0 - arm], [-d / 2.0, d / 2.0 - back], [0.05, seat])
        .cuboid([-w / 2.0, w / 2.0], [d / 2.0 - back, d / 2.0], [0.05, top])
        .cuboid([-w / 2.0, -w / 2.0 + arm], [-d / 2.0, d / 2.0 - back], [0.05, seat + 0.2])
        .cuboid([w / 2.0 - arm, w / 2.0], [-d / 2.0, d / 2.0 - back], [0.05, seat + 0.2])
}

/// A bed: a frame 50 mm larger than the mattress all round to 300 mm, the mattress to `top`, and
/// a headboard at +y (its foot is its front, −y).
fn bed(mw: f64, ml: f64, top: f64) -> Shape {
    let (w, l) = (mw + 0.1, ml + 0.1);
    Shape::default()
        .cuboid([-w / 2.0, w / 2.0], [-l / 2.0, l / 2.0 - 0.05], [0.0, 0.3])
        .cuboid([-mw / 2.0, mw / 2.0], [-ml / 2.0, ml / 2.0 - 0.05], [0.3, top])
        .cuboid([-w / 2.0, w / 2.0], [l / 2.0 - 0.05, l / 2.0], [0.0, top + 0.45])
}

/// A cabinet on a 75 mm plinth set back 50 mm at the front.
fn cabinet(w: f64, d: f64, h: f64) -> Shape {
    Shape::default().cuboid([-w / 2.0, w / 2.0], [-d / 2.0 + 0.05, d / 2.0], [0.0, 0.075]).cuboid([-w / 2.0, w / 2.0], [-d / 2.0, d / 2.0], [0.075, h])
}

/// A plain block.
fn block(w: f64, d: f64, h: f64) -> Shape {
    Shape::default().cuboid([-w / 2.0, w / 2.0], [-d / 2.0, d / 2.0], [0.0, h])
}

/// Open shelving: two end panels, a base and `n` shelves, a back at +y.
fn shelves(w: f64, d: f64, h: f64, n: usize) -> Shape {
    let t = 0.02;
    let mut s = Shape::default()
        .cuboid([-w / 2.0, -w / 2.0 + t], [-d / 2.0, d / 2.0], [0.0, h])
        .cuboid([w / 2.0 - t, w / 2.0], [-d / 2.0, d / 2.0], [0.0, h])
        .cuboid([-w / 2.0 + t, w / 2.0 - t], [d / 2.0 - t, d / 2.0], [0.0, h]);
    for k in 0..n {
        let z = 0.08 + (h - 0.1) * k as f64 / (n.max(2) - 1) as f64;
        s = s.cuboid([-w / 2.0 + t, w / 2.0 - t], [-d / 2.0, d / 2.0 - t], [z, z + t]);
    }
    s
}

/// A gondola section `w` wide (centre to centre of uprights), its base decks `deck` deep each
/// side of a 2½ in upright (one side for a wall section), `h` high, with `n` shelves a side.
fn gondola(w: f64, deck: f64, h: f64, n: usize, island: bool) -> Shape {
    let up = 2.5 * IN;
    let (y0, y1) = if island { (-(deck + up / 2.0), deck + up / 2.0) } else { (-(deck + up / 2.0), up / 2.0) };
    let base = 8.0 * IN;
    let mut s = Shape::default()
        // The back (pegboard) between the uprights, and the uprights at each end.
        .cuboid([-w / 2.0, w / 2.0], [-up / 2.0, up / 2.0], [0.0, h])
        .cuboid([-w / 2.0 - 0.02, -w / 2.0], [-up, up], [0.0, h])
        .cuboid([w / 2.0, w / 2.0 + 0.02], [-up, up], [0.0, h])
        // The base decks.
        .cuboid([-w / 2.0, w / 2.0], [y0, -up / 2.0], [0.0, base]);
    if island {
        s = s.cuboid([-w / 2.0, w / 2.0], [up / 2.0, y1], [0.0, base]);
    }
    for k in 1..=n {
        let z = base + (h - base - 0.15) * k as f64 / n as f64;
        let shelf = deck - 2.0 * IN;
        s = s.cuboid([-w / 2.0, w / 2.0], [-up / 2.0 - shelf, -up / 2.0], [z - 0.02, z]);
        if island {
            s = s.cuboid([-w / 2.0, w / 2.0], [up / 2.0, up / 2.0 + shelf], [z - 0.02, z]);
        }
    }
    s
}

/// A ring of rail at `z` on a post and round base: a garment rounder.
fn rounder(dia: f64, z: f64) -> Shape {
    let ring = rounded([0.0, 0.0], dia, dia, dia / 2.0);
    let mut s = Shape::default().prism(&rounded([0.0, 0.0], 0.5, 0.5, 0.25), 0.0, 0.05).prism(&rounded([0.0, 0.0], 0.05, 0.05, 0.025), 0.05, z);
    // The rail as short straight pieces round the ring.
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        let (x0, x1) = (a[0].min(b[0]) - 0.012, a[0].max(b[0]) + 0.012);
        let (y0, y1) = (a[1].min(b[1]) - 0.012, a[1].max(b[1]) + 0.012);
        s = s.cuboid([x0, x1], [y0, y1], [z - 0.025, z]);
    }
    s.cuboid([-dia / 2.0, dia / 2.0], [-0.012, 0.012], [z - 0.025, z]).cuboid([-0.012, 0.012], [-dia / 2.0, dia / 2.0], [z - 0.025, z])
}

/// A machine: a frame `l` long (along y, its user side at −y) and `w` wide, a body over the
/// back half to `h`, a deck or seat over the front half to `deck`.
fn machine(w: f64, l: f64, h: f64, deck: f64) -> Shape {
    Shape::default()
        .cuboid([-w / 2.0, w / 2.0], [-l / 2.0, l / 2.0], [0.0, deck])
        .cuboid([-w / 2.0, w / 2.0], [l / 2.0 - l * 0.3, l / 2.0], [deck, h])
}

/// A rack: four uprights to `h` round a `w` × `d` base, and two crossbars on top.
fn rack(w: f64, d: f64, h: f64) -> Shape {
    let t = 0.075;
    legs(Shape::default(), w, d, t, h)
        .cuboid([-w / 2.0, w / 2.0], [-d / 2.0 + t / 2.0, -d / 2.0 + 1.5 * t], [h - t, h])
        .cuboid([-w / 2.0, w / 2.0], [d / 2.0 - 1.5 * t, d / 2.0 - t / 2.0], [h - t, h])
        .cuboid([-w / 2.0, w / 2.0], [-d / 2.0, -d / 2.0 + t], [0.0, 0.05])
        .cuboid([-w / 2.0, w / 2.0], [d / 2.0 - t, d / 2.0], [0.0, 0.05])
}

/// A checkstand: a cabinet `l` long (along x) and `w` across, its deck at `deck`, the cashier's
/// side at +y; a lane light pole at its +x end.
fn checkstand(l: f64, w: f64, deck: f64) -> Shape {
    Shape::default()
        .cuboid([-l / 2.0, l / 2.0], [-w / 2.0, w / 2.0 - 0.5], [0.0, deck])
        .cuboid([l / 2.0 - 0.6, l / 2.0], [w / 2.0 - 0.5, w / 2.0], [0.0, deck])
        .cuboid([l / 2.0 - 0.1, l / 2.0 - 0.05], [w / 2.0 - 0.1, w / 2.0 - 0.05], [deck, 2.38])
}

/// A chandelier, its bottom at 0: a bowl, a ring of arms and candles, a canopy stem up to `h`.
fn chandelier(dia: f64, h: f64) -> Shape {
    let mut s = Shape::default()
        .prism(&rounded([0.0, 0.0], dia * 0.3, dia * 0.3, dia * 0.15), 0.0, h * 0.2)
        .prism(&rounded([0.0, 0.0], 0.03, 0.03, 0.015), h * 0.2, h);
    for k in 0..6 {
        let a = std::f64::consts::PI / 3.0 * f64::from(k);
        let (x, y) = (a.cos() * (dia / 2.0 - 0.03), a.sin() * (dia / 2.0 - 0.03));
        s = s.prism(&rounded([x / 2.0, y / 2.0], (x.abs() + 0.02).max(0.02), (y.abs() + 0.02).max(0.02), 0.01), h * 0.25, h * 0.3).prism(&rounded([x, y], 0.04, 0.04, 0.02), h * 0.3, h * 0.55);
    }
    s
}

/// A pendant shade, its bottom at 0, on a cord to `h`.
fn pendant(dia: f64, h: f64) -> Shape {
    Shape::default()
        .prism(&rounded([0.0, 0.0], dia, dia, dia / 2.0), 0.0, h * 0.25)
        .prism(&rounded([0.0, 0.0], dia * 0.6, dia * 0.6, dia * 0.3), h * 0.25, h * 0.55)
        .prism(&rounded([0.0, 0.0], 0.01, 0.01, 0.005), h * 0.55, h)
}

/// A wall sconce, its bottom at 0, its back plate at +y against the wall.
fn sconce(w: f64, d: f64, h: f64) -> Shape {
    Shape::default()
        .cuboid([-w / 4.0, w / 4.0], [d / 2.0 - 0.015, d / 2.0], [h * 0.2, h * 0.8])
        .prism(&rounded([0.0, -0.01], w, d - 0.02, (w.min(d) - 0.02) / 2.0), 0.0, h)
}

/// A lamp: a base, a stem, and a shade `dia` across and `shade` tall at the top of `h`.
fn lamp(dia: f64, h: f64, shade: f64) -> Shape {
    Shape::default()
        .prism(&rounded([0.0, 0.0], dia * 0.6, dia * 0.6, dia * 0.3), 0.0, 0.03)
        .prism(&rounded([0.0, 0.0], 0.03, 0.03, 0.015), 0.03, h - shade)
        .prism(&rounded([0.0, 0.0], dia, dia, dia / 2.0), h - shade, h)
}

fn power(va: f64, voltage: f64, z: f64, back: f64) -> Connector {
    crate::mep::power("Power", [0.0, back, z], [0.0, 1.0, 0.0], D::In, voltage, 1, va, "Power")
}

// ---- The catalogue ----

macro_rules! item {
    ($lib:ident, $folder:expr, $name:expr, $cat:ident, $pre:expr, $kind:expr, $uni:expr, $color:expr, $desc:expr, $src:expr, $shape:expr) => {
        item!($lib, $folder, $name, $cat, $pre, $kind, $uni, $color, $desc, $src, $shape, none)
    };
    ($lib:ident, $folder:expr, $name:expr, $cat:ident, $pre:expr, $kind:expr, $uni:expr, $color:expr, $desc:expr, $src:expr, $shape:expr, $conn:expr) => {
        item!($lib, $folder, $name, $cat, $pre, $kind, $uni, $color, $desc, $src, $shape, $conn, no_params)
    };
    ($lib:ident, $folder:expr, $name:expr, $cat:ident, $pre:expr, $kind:expr, $uni:expr, $color:expr, $desc:expr, $src:expr, $shape:expr, $conn:expr, $params:expr) => {
        Item { library: Library::$lib, folder: $folder, name: $name, category: C::$cat, predefined: $pre, kind: $kind, uniclass: $uni, color: $color, description: $desc, source: $src, shape: $shape, connectors: $conn, params: $params }
    };
}

/// The catalogue.
pub fn catalogue() -> Vec<Item> {
    vec![
        // ---- Interiors: bedroom (research/15 D1) ----
        item!(Interiors, "Bedroom", "Bed, twin 38 × 75 in", Furniture, "BED", None, Some("Pr_40_50_06_77"), LINEN, "A twin mattress 38 × 75 in on a frame 2 in larger each side; mattress top 24 in.", "S1476", || bed(38.0 * IN, 75.0 * IN, 0.61)),
        item!(Interiors, "Bedroom", "Bed, full 54 × 75 in", Furniture, "BED", None, Some("Pr_40_50_06_24"), LINEN, "A full (double) mattress 54 × 75 in on its frame; mattress top 24 in.", "S1476", || bed(54.0 * IN, 75.0 * IN, 0.61)),
        item!(Interiors, "Bedroom", "Bed, queen 60 × 80 in", Furniture, "BED", None, Some("Pr_40_50_06_24"), LINEN, "A queen mattress 60 × 80 in on its frame; mattress top 24 in.", "S1476", || bed(60.0 * IN, 80.0 * IN, 0.61)),
        item!(Interiors, "Bedroom", "Bed, king 76 × 80 in", Furniture, "BED", None, Some("Pr_40_50_06_24"), LINEN, "An Eastern king mattress 76 × 80 in on its frame; mattress top 24 in.", "S1476", || bed(76.0 * IN, 80.0 * IN, 0.61)),
        item!(Interiors, "Bedroom", "Bed, single 900 × 1900", Furniture, "BED", None, Some("Pr_40_50_06_77"), LINEN, "Approved Document M's single bed, 900 × 1900 mm.", "S1465", || bed(0.9, 1.9, 0.55)),
        item!(Interiors, "Bedroom", "Bed, double 1350 × 1900", Furniture, "BED", None, Some("Pr_40_50_06_24"), LINEN, "Approved Document M's other double bed, 1350 × 1900 mm.", "S1465", || bed(1.35, 1.9, 0.55)),
        item!(Interiors, "Bedroom", "Bed, principal double 1500 × 2000", Furniture, "BED", None, Some("Pr_40_50_06_24"), LINEN, "Approved Document M's principal double bed, 1500 × 2000 mm.", "S1465", || bed(1.5, 2.0, 0.55)),
        item!(Interiors, "Bedroom", "Nightstand", Furniture, "USERDEFINED", Some("Nightstand"), None, WOOD, "22 × 16 in, 26 in high: about level with a mattress top.", "S1503", || cabinet(22.0 * IN, 16.0 * IN, 26.0 * IN)),
        item!(Interiors, "Bedroom", "Dresser", Furniture, "USERDEFINED", Some("Dresser"), None, WOOD, "60 in wide and 30 in high; its 19 in depth is unverified.", "S1503", || cabinet(60.0 * IN, 19.0 * IN, 30.0 * IN)),
        item!(Interiors, "Bedroom", "Chest of drawers 750 × 450", Furniture, "USERDEFINED", Some("ChestOfDrawers"), None, WOOD, "Approved Document M's chest of drawers, 750 × 450 mm; its 1000 mm height is judgement.", "S1465", || cabinet(0.75, 0.45, 1.0)),
        item!(Interiors, "Bedroom", "Wardrobe, double 1200 × 600", Furniture, "USERDEFINED", Some("Wardrobe"), None, WOOD, "Approved Document M's double wardrobe, 1200 × 600 mm, 2010 mm high (a PAX frame's height).", "S1465 S1477", || cabinet(1.2, 0.6, 2.01)),
        item!(Interiors, "Bedroom", "Luggage rack", Furniture, "USERDEFINED", Some("LuggageRack"), None, WOOD, "A hotel luggage rack, 24 × 17½ in and 22 in high.", "S1499", || table(24.0 * IN, 17.5 * IN, 22.0 * IN)),
        // ---- Interiors: living ----
        item!(Interiors, "Living", "Sofa, 3-seat 1850 × 850", Furniture, "SOFA", None, Some("Pr_40_50_12_81"), FABRIC, "Approved Document M's 3-seat settee, 1850 × 850 mm; seat 430 mm, back 850 mm.", "S1465 S1490", || sofa(1.85, 0.85, 0.43, 0.85, 0.2)),
        item!(Interiors, "Living", "Sofa, 2-seat 1300 × 850", Furniture, "SOFA", None, Some("Pr_40_50_12_76"), FABRIC, "Approved Document M's 2-seat settee, 1300 × 850 mm.", "S1465 S1490", || sofa(1.3, 0.85, 0.43, 0.85, 0.2)),
        item!(Interiors, "Living", "Armchair 850 × 850", Furniture, "CHAIR", None, Some("Pr_40_50_12_48"), FABRIC, "Approved Document M's armchair, 850 × 850 mm.", "S1465 S1490", || sofa(0.85, 0.85, 0.43, 0.85, 0.18)),
        item!(Interiors, "Living", "Coffee table 1050 × 500", Furniture, "TABLE", Some("CoffeeTable"), Some("Pr_40_50_21_14"), WOOD, "Approved Document M's coffee table, 1050 × 500 mm; its 450 mm height is judgement.", "S1465", || table(1.05, 0.5, 0.45)),
        item!(Interiors, "Living", "Occasional table 450 × 450", Furniture, "TABLE", Some("SideTable"), None, WOOD, "Approved Document M's occasional table, 450 × 450 mm, 550 mm high.", "S1465", || table(0.45, 0.45, 0.55)),
        item!(Interiors, "Living", "Storage unit 1500 × 500", Furniture, "USERDEFINED", Some("StorageUnit"), None, WOOD, "Approved Document M's storage unit for four people, 1500 × 500 mm; its 750 mm height is judgement.", "S1465", || cabinet(1.5, 0.5, 0.75)),
        item!(Interiors, "Living", "Rug 8 × 10 ft", Furniture, "USERDEFINED", Some("Rug"), Some("Pr_40_50_81"), FABRIC, "A living-room rug, 8 × 10 ft.", "S1480", || block(10.0 * 12.0 * IN, 8.0 * 12.0 * IN, 0.01)),
        // ---- Interiors: dining ----
        item!(Interiors, "Dining", "Dining table, 4 seats 48 × 30 in", Furniture, "TABLE", Some("DiningTable"), Some("Pr_40_50_21_22"), WOOD, "A rectangular table for four, 48 × 30 in, 29 in high.", "S1479 S1503", || table(48.0 * IN, 30.0 * IN, 29.0 * IN)),
        item!(Interiors, "Dining", "Dining table, 6 seats 72 × 36 in", Furniture, "TABLE", Some("DiningTable"), Some("Pr_40_50_21_22"), WOOD, "A rectangular table for six, 72 × 36 in, 29 in high.", "S1479 S1503", || table(72.0 * IN, 36.0 * IN, 29.0 * IN)),
        item!(Interiors, "Dining", "Dining table, round 42 in", Furniture, "TABLE", Some("DiningTable"), Some("Pr_40_50_21_22"), WOOD, "A round table for two to four, 42 in across, 29 in high.", "S1479 S1503", || round_table(42.0 * IN, 29.0 * IN)),
        item!(Interiors, "Dining", "Dining chair", Furniture, "CHAIR", None, None, WOOD, "Seat 19 in wide at 18 in high; 20 in deep and 34 in to the top of the back by judgement.", "S1503", || chair(19.0 * IN, 20.0 * IN, 18.0 * IN, 34.0 * IN)),
        item!(Interiors, "Dining", "Counter stool", Furniture, "CHAIR", Some("Stool"), Some("Pr_40_50_12_85"), WOOD, "Seat 25 in high for a 36 in counter; 16 in square by judgement.", "S1491", || legs(Shape::default().cuboid([-8.0 * IN, 8.0 * IN], [-8.0 * IN, 8.0 * IN], [25.0 * IN - 0.04, 25.0 * IN]), 16.0 * IN, 16.0 * IN, 0.03, 25.0 * IN - 0.04)),
        // ---- Interiors: office ----
        item!(Interiors, "Office", "Desk 60 × 30 in", Furniture, "DESK", None, Some("Pr_40_50_21_96"), WOOD, "The commonest desk, 60 × 30 in and 29 in high.", "S1503", || table(60.0 * IN, 30.0 * IN, 29.0 * IN)),
        item!(Interiors, "Office", "Standing desk 60 × 30 in", Furniture, "DESK", Some("StandingDesk"), Some("Pr_40_50_21_83"), WOOD, "A sit-stand desk, 60 × 30 in, shown at 42 in (BIFMA G1 range 22.0–50.2 in).", "S1482 S1503", || table(60.0 * IN, 30.0 * IN, 42.0 * IN)),
        item!(Interiors, "Office", "Task chair", Furniture, "CHAIR", Some("TaskChair"), Some("Pr_40_50_12_57"), BLACK, "Seat 20 in wide at 18 in (adjustable about 15–20 in); 24 in square and 40 in high by judgement.", "S1482 S1483", || chair(24.0 * IN, 24.0 * IN, 18.0 * IN, 40.0 * IN)),
        item!(Interiors, "Office", "Conference table, 8 seats 96 × 42 in", Furniture, "TABLE", Some("ConferenceTable"), Some("Pr_40_50_21_15"), WOOD, "A table for eight, 96 × 42 in (24 in of edge a seat), 29 in high.", "S1479 S1485", || table(96.0 * IN, 42.0 * IN, 29.0 * IN)),
        item!(Interiors, "Office", "Bookcase 36 in", Furniture, "SHELF", None, None, WOOD, "Five shelves 36 in wide; its 12 in depth and 72 in height are unverified.", "", || shelves(36.0 * IN, 12.0 * IN, 72.0 * IN, 5)),
        item!(Interiors, "Office", "Reception desk 72 × 30 in", Furniture, "USERDEFINED", Some("ServiceCounter"), None, WOOD, "A work surface at 29 in behind a 42 in transaction ledge, 72 × 30 in.", "S1492", || block(72.0 * IN, 30.0 * IN, 29.0 * IN).cuboid([-36.0 * IN, 36.0 * IN], [-15.0 * IN, -9.0 * IN], [29.0 * IN, 42.0 * IN])),
        // ---- Interiors: decorative lighting (research/15 D4) ----
        item!(Interiors, "Lighting", "Chandelier 24 in", LightingFixture, "USERDEFINED", Some("Chandelier"), Some("Pr_70_70_48_13"), BRASS, "A chandelier 24 in across and 20 in tall (2.5 in a foot of an 8 ft ceiling), hung from the ceiling; its geometry starts at its bottom.", "S1471", || chandelier(24.0 * IN, 20.0 * IN), || vec![power(240.0, 120.0, 20.0 * IN, 0.0)], || fixture("SUSPENDED", "CEILING", 240.0)),
        item!(Interiors, "Lighting", "Chandelier 36 in", LightingFixture, "USERDEFINED", Some("Chandelier"), Some("Pr_70_70_48_13"), BRASS, "A chandelier 36 in across and 30 in tall, for a large room or a long table.", "S1471", || chandelier(36.0 * IN, 30.0 * IN), || vec![power(480.0, 120.0, 30.0 * IN, 0.0)], || fixture("SUSPENDED", "CEILING", 480.0)),
        item!(Interiors, "Lighting", "Pendant 12 in", LightingFixture, "USERDEFINED", Some("Pendant"), Some("Pr_70_70_48_62"), BLACK, "A pendant shade 12 in across and 14 in tall, for islands and tables (24–30 in apart).", "S1472", || pendant(12.0 * IN, 14.0 * IN), || vec![power(60.0, 120.0, 14.0 * IN, 0.0)], || fixture("SUSPENDED", "CEILING", 60.0)),
        item!(Interiors, "Lighting", "Wall sconce", LightingFixture, "USERDEFINED", Some("Sconce"), None, BRASS, "A wall sconce 6 in wide and 12 in tall, projecting 4 in (ADA §307.2's limit between 27 and 80 in); its back against the wall.", "S1473 S1354", || sconce(6.0 * IN, 4.0 * IN, 12.0 * IN), || vec![power(60.0, 120.0, 6.0 * IN, 2.0 * IN)], || fixture("SURFACE", "WALL", 60.0)),
        item!(Interiors, "Lighting", "Table lamp", LightingFixture, "USERDEFINED", Some("TableLamp"), Some("Pr_70_70_48_32"), LINEN, "A table lamp 16 in across its shade and 24 in tall: no source gave a size.", "", || lamp(16.0 * IN, 24.0 * IN, 9.0 * IN), || vec![power(60.0, 120.0, 0.1, 0.0)], || fixture("FREESTANDING", "FURNITURE", 60.0)),
        item!(Interiors, "Lighting", "Floor lamp", LightingFixture, "USERDEFINED", Some("FloorLamp"), Some("Pr_70_70_48_32"), LINEN, "A floor lamp 16 in across its shade and 63 in tall: no source gave a size.", "", || lamp(16.0 * IN, 63.0 * IN, 10.0 * IN), || vec![power(100.0, 120.0, 0.1, 0.0)], || fixture("FREESTANDING", "FLOOR", 100.0)),
        // ---- Gym: cardio (research/15 A1) ----
        item!(Gym, "Cardio", "Treadmill, commercial", Furniture, "USERDEFINED", Some("Treadmill"), Some("Pr_40_70_84_91"), BLACK, "211 × 89 cm and 173 cm high (Precor TRM 885); 120 V on its own 20 A circuit. Its user steps on at the rear, its front here.", "S1356", || machine(0.89, 2.11, 1.73, 0.25), || vec![power(1920.0, 120.0, 0.2, 1.055)]),
        item!(Gym, "Cardio", "Elliptical", Furniture, "USERDEFINED", Some("Elliptical"), Some("Pr_40_70_84_26"), BLACK, "203 × 76 cm, 170 cm high (Precor EFX 885); self-powered.", "S1368", || machine(0.76, 2.03, 1.7, 0.45)),
        item!(Gym, "Cardio", "Stair climber", Furniture, "USERDEFINED", Some("StairClimber"), Some("Pr_40_70_84_84"), BLACK, "147 × 86 cm, 200 cm high (StairMaster 8Gx); a 12 V supply from an ordinary outlet. Its height and the user's rise need a tall ceiling.", "S1367", || machine(0.86, 1.47, 2.0, 0.6), || vec![power(60.0, 120.0, 0.2, 0.735)]),
        item!(Gym, "Cardio", "Upright bike", Furniture, "USERDEFINED", Some("ExerciseBike"), Some("Pr_40_70_84_27"), BLACK, "111 × 54 cm, 152 cm high (Life Fitness 95C).", "S1369", || machine(0.54, 1.11, 1.52, 0.8)),
        item!(Gym, "Cardio", "Recumbent bike", Furniture, "USERDEFINED", Some("ExerciseBike"), Some("Pr_40_70_84_27"), BLACK, "166 × 69 cm, 135 cm high (Life Fitness 95R).", "S1369", || machine(0.69, 1.66, 1.35, 0.6)),
        item!(Gym, "Cardio", "Indoor cycle", Furniture, "USERDEFINED", Some("ExerciseBike"), Some("Pr_40_70_84_27"), BLACK, "114 × 66 cm, 124 cm high (Keiser M3i); no mains power.", "S1369", || machine(0.66, 1.14, 1.24, 0.9)),
        item!(Gym, "Cardio", "Rower", Furniture, "USERDEFINED", Some("Rower"), Some("Pr_40_70_84_72"), BLACK, "244 × 61 cm assembled (Concept2 RowErg); its use area is 274 × 122 cm. Its 0.5 m height is judgement.", "S1360", || machine(0.61, 2.44, 0.5, 0.35)),
        // ---- Gym: strength ----
        item!(Gym, "Strength", "Leg press, selectorised", Furniture, "USERDEFINED", Some("LegPress"), Some("Pr_40_70_84_46"), STEEL, "201 × 109 cm, 178 cm high (Life Fitness Signature seated leg press); 367 kg.", "S1365", || machine(1.09, 2.01, 1.78, 0.5)),
        item!(Gym, "Strength", "Cable crossover", Furniture, "USERDEFINED", Some("CableCrossover"), Some("Pr_40_70_84_11"), STEEL, "425 cm wide, 73 cm deep and 239 cm high (Life Fitness Signature CMACO): it must clear ceiling services.", "S1364", || rack(4.25, 0.73, 2.39)),
        item!(Gym, "Strength", "Power rack", Furniture, "USERDEFINED", Some("PowerRack"), Some("Pr_40_70_84_66"), STEEL, "135 × 86 cm and 230 cm high (Rogue R-3); to be bolted to the floor.", "S1361", || rack(1.35, 0.86, 2.3)),
        item!(Gym, "Strength", "Half rack", Furniture, "USERDEFINED", Some("HalfRack"), Some("Pr_40_70_84_66"), STEEL, "122 × 124 cm and 234 cm high (Rogue HR-2).", "S1362", || rack(1.22, 1.24, 2.34)),
        item!(Gym, "Strength", "Smith machine", Furniture, "USERDEFINED", Some("SmithMachine"), Some("Pr_40_70_84_80"), STEEL, "204 × 141 cm and 229 cm high (Matrix Varsity Smith).", "S1371", || rack(2.04, 1.41, 2.29)),
        // ---- Gym: free weights ----
        item!(Gym, "Free weights", "Dumbbell rack, two-tier", Furniture, "SHELF", Some("DumbbellRack"), None, STEEL, "229 × 64 cm, 84 cm high (Life Fitness SDR2); its live area is 229 × 155 cm.", "S1366", || shelves(2.29, 0.64, 0.84, 2)),
        item!(Gym, "Free weights", "Flat bench", Furniture, "USERDEFINED", Some("WeightBench"), None, BLACK, "About 1.3 × 0.6 m and 0.45 m high: no maker's sheet was found.", "", || table(0.6, 1.3, 0.45)),
        item!(Gym, "Free weights", "Plate tree", Furniture, "USERDEFINED", Some("PlateTree"), None, STEEL, "A 0.7 × 0.7 m base, 1.2 m high: no maker's sheet was found.", "", || block(0.7, 0.7, 0.05).cuboid([-0.04, 0.04], [-0.04, 0.04], [0.05, 1.2])),
        item!(Gym, "Free weights", "Lifting platform 8 × 8 ft", Furniture, "USERDEFINED", Some("LiftingPlatform"), None, RUBBER, "An 8 × 8 ft (2.44 m) lifting area in a 2.54 m square frame (Rogue 8 × 8 ft Oly), 50 mm high.", "S1363", || block(2.54, 2.54, 0.05)),
        // ---- Retail: shelving (research/15 B1–B7) ----
        item!(Retail, "Shelving", "Gondola, island 48 in × 72 in", Furniture, "SHELF", Some("Gondola"), Some("Pr_40_30_78_71"), WHITE, "A 48 in island section of two 16 in decks each side of a 2½ in upright (35¾ in overall with the 1¼ in allowance), 72 in high, four shelves a side.", "S1400", || gondola(48.0 * IN, 16.0 * IN, 72.0 * IN, 4, true)),
        item!(Retail, "Shelving", "Gondola, wall 48 in × 84 in", Furniture, "SHELF", Some("Gondola"), Some("Pr_40_30_78_71"), WHITE, "A 48 in wall section, a 16 in deck, 84 in high, five shelves.", "S1400", || gondola(48.0 * IN, 16.0 * IN, 84.0 * IN, 5, false)),
        item!(Retail, "Shelving", "Gondola end cap 36 in", Furniture, "SHELF", Some("EndCap"), Some("Pr_40_30_78_71"), WHITE, "An end unit as wide as a 16 + 16 in island (36 in), 72 in high.", "S1400", || gondola(36.0 * IN, 16.0 * IN, 72.0 * IN, 4, false)),
        item!(Retail, "Shelving", "Pallet rack bay 96 × 42 in", Furniture, "USERDEFINED", Some("PalletRack"), Some("Pr_40_30_78_70"), STEEL, "A selective rack bay 96 in wide and 42 in deep, 16 ft high, with beams at 6 and 11 ft; anchored, and restrained above 8 ft in public areas.", "S1399", || rack(96.0 * IN, 42.0 * IN, 4.88).cuboid([-48.0 * IN, 48.0 * IN], [-21.0 * IN, 21.0 * IN], [1.83, 1.93]).cuboid([-48.0 * IN, 48.0 * IN], [-21.0 * IN, 21.0 * IN], [3.35, 3.45])),
        item!(Retail, "Shelving", "Produce table 72 × 48 in", Furniture, "TABLE", Some("ProduceTable"), Some("Pr_40_30_25_71"), WOOD, "A display table 72 × 48 in, its deck at 34 in: no primary sheet was found.", "", || table(72.0 * IN, 48.0 * IN, 34.0 * IN)),
        // ---- Retail: apparel and showcases ----
        item!(Retail, "Apparel", "Garment rounder 42 in", Furniture, "USERDEFINED", Some("GarmentRack"), Some("Pr_40_30_78_16"), STEEL, "A round rack 42 in across, its rail at 54 in (adjustable 45–72 in).", "S1403", || rounder(42.0 * IN, 54.0 * IN)),
        item!(Retail, "Apparel", "Garment rack, 4-way", Furniture, "USERDEFINED", Some("GarmentRack"), Some("Pr_40_30_78_16"), STEEL, "A 42½ in square 4-way, its arms at 60 in (adjustable 48–72 in).", "S1403", || block(0.5, 0.5, 0.05).cuboid([-0.025, 0.025], [-0.025, 0.025], [0.05, 60.0 * IN]).cuboid([-21.25 * IN, 21.25 * IN], [-0.015, 0.015], [60.0 * IN - 0.03, 60.0 * IN]).cuboid([-0.015, 0.015], [-21.25 * IN, 21.25 * IN], [60.0 * IN - 0.03, 60.0 * IN])),
        item!(Retail, "Apparel", "Mannequin", Furniture, "USERDEFINED", Some("Mannequin"), None, WHITE, "About 20 in across its base and 74 in tall: no source was found.", "", || Shape::default().prism(&rounded([0.0, 0.0], 0.5, 0.5, 0.25), 0.0, 0.03).prism(&rounded([0.0, 0.0], 0.35, 0.22, 0.1), 0.85, 1.6).prism(&rounded([0.0, 0.0], 0.04, 0.04, 0.02), 0.03, 0.85).prism(&rounded([0.0, 0.0], 0.18, 0.2, 0.09), 1.6, 1.88)),
        item!(Retail, "Showcases", "Showcase, counter 70 in", Furniture, "USERDEFINED", Some("Showcase"), Some("Pr_40_30_78_24"), GLASS, "A counter-height showcase 70 × 20 in, 38 in high, glass above a base; served across, it is a sales counter (ADA §904.4).", "S1405", || block(70.0 * IN, 20.0 * IN, 0.5).cuboid([-35.0 * IN, 35.0 * IN], [-10.0 * IN, 10.0 * IN], [0.5, 38.0 * IN])),
        item!(Retail, "Showcases", "Cash wrap 72 × 22 in", Furniture, "USERDEFINED", Some("CashWrap"), Some("Pr_40_30_65_65"), WOOD, "A sales counter 72 × 22 × 42 in, with a 36 in long lowered section at 36 in for ADA §904.4.", "S1460 S1354", || Shape::default().cuboid([-36.0 * IN, 0.0], [-11.0 * IN, 11.0 * IN], [0.0, 42.0 * IN]).cuboid([0.0, 36.0 * IN], [-11.0 * IN, 11.0 * IN], [0.0, 36.0 * IN])),
        // ---- Retail: carts and security ----
        item!(Retail, "Carts and security", "Shopping cart", Furniture, "USERDEFINED", Some("ShoppingCart"), Some("Pr_40_50_90_76"), STEEL, "A wire cart 43¾ × 24½ in and 41 5/16 in high (Unarco M125); carts nest 12½ in.", "S1411", || block(24.5 * IN, 43.75 * IN, 0.2).cuboid([-12.25 * IN, 12.25 * IN], [-21.875 * IN, 21.875 * IN], [0.45, 0.85]).cuboid([-12.25 * IN, 12.25 * IN], [21.875 * IN - 0.03, 21.875 * IN], [0.85, 41.3125 * IN])),
        item!(Retail, "Carts and security", "EAS pedestal", Sensor, "USERDEFINED", Some("EASPedestal"), None, GLASS, "A security pedestal 0.42 m wide, 0.10 m deep and 1.55 m high; pairs at most about 1.8 m apart, on their own circuit.", "S1409", || block(0.42, 0.1, 1.55), || vec![power(50.0, 120.0, 0.1, 0.05)]),
        // ---- Retail: checkouts (research/15 C2, C7) ----
        item!(Retail, "Checkouts", "Checkstand, carousel rear 142 × 48 in", Furniture, "USERDEFINED", Some("Checkstand"), Some("Pr_40_30_65_65"), WHITE, "A convertible lane with a carousel rear, 142 × 48 in, its deck at 36 in (Pan-Oston); its lane light 2380 mm up.", "S1432 S1406", || checkstand(142.0 * IN, 48.0 * IN, 36.0 * IN), || vec![power(1800.0, 120.0, 0.2, 0.6)]),
        item!(Retail, "Checkouts", "Checkstand, belted take-away 150 × 48 in", Furniture, "USERDEFINED", Some("Checkstand"), Some("Pr_40_30_65_65"), WHITE, "A scan-and-pass lane with a belted take-away, 150 × 48 in, its deck at 36 in (Pan-Oston).", "S1432 S1406", || checkstand(150.0 * IN, 48.0 * IN, 36.0 * IN), || vec![power(1800.0, 120.0, 0.2, 0.6)]),
        item!(Retail, "Checkouts", "Cash desk, compact 2500 × 1250", Furniture, "USERDEFINED", Some("Checkstand"), Some("Pr_40_30_65_65"), WHITE, "A compact tandem cash desk for baskets, 2500 × 1250 mm and 970 mm high (ITAB CheckMate Tandem); 230 V, 3 A.", "S1436", || checkstand(2.5, 1.25, 0.97), || vec![power(690.0, 230.0, 0.2, 0.425)]),
        item!(Retail, "Checkouts", "Self-checkout, pedestal", Furniture, "USERDEFINED", Some("SelfCheckout"), Some("Pr_40_30_65_60"), BLACK, "A pedestal self-checkout 440 mm wide, 718 mm deep and 1490 mm high (Diebold Nixdorf EASY eXpress).", "S1439", || block(0.44, 0.718, 0.9).cuboid([-0.22, 0.22], [0.2, 0.359], [0.9, 1.49]), || vec![power(300.0, 120.0, 0.2, 0.359)]),
        item!(Retail, "Checkouts", "Self-checkout lane", Furniture, "USERDEFINED", Some("SelfCheckout"), Some("Pr_40_30_65_60"), BLACK, "A full scan-and-bag lane 3068 × 1359 mm (NCR SelfServ 7350, core with bumper, no bag well); its 915 mm deck height is judgement.", "S1408", || checkstand(3.068, 1.359, 0.915), || vec![power(600.0, 120.0, 0.2, 0.68)]),
        item!(Retail, "Checkouts", "Attendant station", Furniture, "USERDEFINED", Some("AttendantStation"), None, BLACK, "A remote attendant station about 1077 × 660 mm and 1038 mm high (NCR, crated size).", "S1408", || cabinet(1.077, 0.66, 1.038), || vec![power(300.0, 120.0, 0.2, 0.33)]),
    ]
}

/// The items' names.
pub fn names() -> Vec<&'static str> {
    catalogue().iter().map(|c| c.name).collect()
}

/// An item, for listing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemInfo {
    pub library: Library,
    pub folder: String,
    pub name: String,
    pub predefined_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniclass: Option<String>,
    pub description: String,
    pub source: String,
    /// Width, depth and height (metres).
    pub size: [f64; 3],
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogueParams {
    /// Only this library's items.
    pub library: Option<Library>,
}

/// `furnishing.catalogue`.
pub fn list(_: &crate::store::Store, p: Option<CatalogueParams>) -> Result<Vec<ItemInfo>> {
    let only = p.and_then(|p| p.library);
    Ok(catalogue()
        .into_iter()
        .filter(|i| only.is_none_or(|l| l == i.library))
        .map(|i| {
            let s = i.shape();
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            for q in &s.p {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            ItemInfo {
                library: i.library,
                folder: i.folder.into(),
                name: i.name.into(),
                predefined_type: i.predefined.into(),
                kind: i.kind.map(str::to_owned),
                uniclass: i.uniclass.map(str::to_owned),
                description: i.description.into(),
                source: i.source.into(),
                size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]],
            }
        })
        .collect())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadParams {
    /// Which (by name); all when empty.
    #[serde(default)]
    pub names: Vec<String>,
}

/// Loads items into the project as types (`furnishing.load`); a type of the same name already
/// there is reused. Returns the types by name.
pub(crate) fn load(txn: &mut Transaction<'_>, p: LoadParams) -> Result<BTreeMap<String, ElementId>> {
    let all = catalogue();
    for n in &p.names {
        if !all.iter().any(|c| c.name == n) {
            return Err(Error::new("furnishing.no_item", format!("no furnishing {n:?}; see furnishing.catalogue")));
        }
    }
    let mut out = BTreeMap::new();
    for c in all.into_iter().filter(|c| p.names.is_empty() || p.names.iter().any(|n| n == c.name)) {
        let store = txn.store().clone();
        let existing = store.iter().find(|e| e.name.as_deref() == Some(c.name) && crate::mesh_objects::mesh_object_type(e).is_some()).map(crate::element::Element::id);
        let id = match existing {
            Some(id) => id,
            None => {
                let s = c.shape();
                let connectors = (c.connectors)().into_iter().map(crate::mep::validate).collect::<Result<Vec<_>>>()?;
                let id = crate::mesh_objects::create_generated_type(txn, c.name, c.category, &s.p, &s.t, c.color, connectors, Some(c.predefined))?;
                let (kind, description, params) = (c.kind, c.description, (c.params)());
                txn.modify(id, |e| {
                    e.params.insert(MARK.into(), true.into());
                    e.params.insert("@Description".into(), description.into());
                    for (k, v) in params {
                        e.params.insert(k.into(), v);
                    }
                    if let Some(k) = kind {
                        e.params.insert("@ElementType".into(), k.into());
                    }
                })?;
                id
            }
        };
        out.insert(c.name.to_owned(), id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_unique_valid_and_sourced() {
        let register = include_str!("../../../design_documents/SOURCE_REGISTER.md");
        let items = catalogue();
        let mut seen = std::collections::BTreeSet::new();
        for i in &items {
            assert!(seen.insert(i.name), "{} twice", i.name);
            assert!(i.category.predefined_types().contains(&i.predefined), "{}: {} is not a {:?} type", i.name, i.predefined, i.category);
            if i.predefined == "USERDEFINED" {
                assert!(i.kind.is_some(), "{}: a USERDEFINED item needs its kind", i.name);
            }
            for s in i.source.split(' ').filter(|s| !s.is_empty()) {
                assert!(register.contains(&format!("| {s} |")), "{}: {s} is not in SOURCE_REGISTER.md", i.name);
            }
            let s = i.shape();
            assert!(!s.t.is_empty() && s.p.iter().all(|q| q[2] >= -1e-9), "{}: it stands on the floor", i.name);
        }
        for l in Library::ALL {
            assert!(items.iter().any(|i| i.library == l));
        }
    }
}
