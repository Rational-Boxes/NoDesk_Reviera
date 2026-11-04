// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The generic bathroom components (ARC-BATH-004): placeholder fixtures and accessories with the
//! sizes, mounting heights, IFC classes and connectors of research/13's catalogue (E3), to stand
//! in until a product is chosen (then a manufacturer's type takes their place, keeping each
//! copy's identity). Each is a generated custom-object type ([`crate::mesh_objects`]) shaped from
//! simple prisms: its back (+y) goes against the wall, its geometry stands at its mounting height
//! above the floor, and it is centred on its insertion point in plan.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{Classification as K, Connector, FlowDirection as D};
use crate::mesh_objects::Category as C;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// Marks a type as one of these components, for the Place Bathroom Component tool.
pub const MARK: &str = "rxd:bath_component";

/// Whether a type is one of these components.
pub fn is_component(e: &crate::element::Element) -> bool {
    e.param(MARK).and_then(serde_json::Value::as_bool) == Some(true)
}

/// Triangles built from prisms.
#[derive(Debug, Default)]
struct Shape {
    p: Vec<[f64; 3]>,
    t: Vec<[u32; 3]>,
}

impl Shape {
    /// A convex polygon (counter-clockwise from above) extruded from `z0` to `z1`.
    fn prism(mut self, poly: &[V2], z0: f64, z1: f64) -> Self {
        let n = poly.len() as u32;
        let base = self.p.len() as u32;
        for z in [z0, z1] {
            for q in poly {
                self.p.push([q[0], q[1], z]);
            }
        }
        for k in 1..n - 1 {
            self.t.push([base, base + k + 1, base + k]);
            self.t.push([base + n, base + n + k, base + n + k + 1]);
        }
        for k in 0..n {
            let j = (k + 1) % n;
            self.t.push([base + k, base + j, base + n + j]);
            self.t.push([base + k, base + n + j, base + n + k]);
        }
        self
    }

    fn cuboid(self, x: V2, y: V2, z: V2) -> Self {
        self.prism(&[[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]], z[0], z[1])
    }
}

/// A rectangle with rounded corners, centred on `c`.
fn rounded(c: V2, w: f64, d: f64, r: f64) -> Vec<V2> {
    let r = r.min(w / 2.0).min(d / 2.0);
    let (hx, hy) = (w / 2.0 - r, d / 2.0 - r);
    let mut out = Vec::new();
    for (k, (sx, sy)) in [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)].iter().enumerate() {
        for s in 0..=4 {
            let a = std::f64::consts::FRAC_PI_2 * (k as f64 - 1.0 + f64::from(s) / 4.0);
            out.push([c[0] + sx * hx + r * a.cos(), c[1] + sy * hy + r * a.sin()]);
        }
    }
    out
}

/// A bowl in plan: straight along the back at `back`, round to the front `depth` ahead of it,
/// `w` wide.
fn bowl(back: f64, w: f64, depth: f64) -> Vec<V2> {
    let mut out: Vec<V2> = (0..=16)
        .map(|k| {
            let a = std::f64::consts::PI * (1.0 + f64::from(k) / 16.0);
            [w / 2.0 * a.cos(), back + depth * a.sin()]
        })
        .collect();
    out.push([-w / 2.0, back]);
    out.dedup();
    out
}

fn pipe(name: &str, origin: [f64; 3], direction: [f64; 3], cl: K, d: f64, fu: f64) -> Connector {
    let flow = if cl == K::Sanitary { D::Out } else { D::In };
    crate::mep::pipe(name, origin, direction, cl, flow, d, fu)
}

const WHITE: [f32; 4] = [0.96, 0.96, 0.97, 1.0];
const MIRROR: [f32; 4] = [0.78, 0.86, 0.92, 1.0];
const STEEL: [f32; 4] = [0.75, 0.76, 0.78, 1.0];
const GREY: [f32; 4] = [0.55, 0.56, 0.58, 1.0];
const WOOD: [f32; 4] = [0.74, 0.6, 0.42, 1.0];

/// One generic component.
#[derive(Debug)]
pub struct Component {
    pub name: &'static str,
    pub category: C,
    /// Its IFC predefined type, and for USERDEFINED its element type (Mirror, …).
    pub predefined: &'static str,
    pub element_type: Option<&'static str>,
    pub color: [f32; 4],
    shape: fn() -> Shape,
    connectors: fn() -> Vec<Connector>,
}

fn none() -> Vec<Connector> {
    Vec::new()
}

/// The catalogue.
pub fn catalogue() -> Vec<Component> {
    vec![
        // Sanitary ware (IfcSanitaryTerminal).
        Component {
            name: "WC, close-coupled",
            category: C::PlumbingFixture,
            predefined: "TOILETPAN",
            element_type: None,
            color: WHITE,
            // 380 × 700, seat 430 mm (17 in), cistern to 780 mm; the outlet 305 mm (12 in) off the wall.
            shape: || Shape::default().cuboid([-0.19, 0.19], [0.17, 0.35], [0.4, 0.78]).cuboid([-0.12, 0.12], [-0.2, 0.2], [0.0, 0.38]).prism(&bowl(0.19, 0.36, 0.54), 0.38, 0.43),
            connectors: || vec![pipe("Cold", [0.12, 0.35, 0.2], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 2.5), pipe("Waste", [0.0, 0.045, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.1, 4.0)],
        },
        Component {
            name: "WC, wall-hung",
            category: C::PlumbingFixture,
            predefined: "TOILETPAN",
            element_type: None,
            color: WHITE,
            // A 360 × 540 pan, seat 430 mm; its frame and cistern are in the wall.
            shape: || Shape::default().cuboid([-0.17, 0.17], [0.15, 0.27], [0.25, 0.42]).prism(&bowl(0.17, 0.36, 0.44), 0.28, 0.43),
            connectors: || vec![pipe("Cold", [0.1, 0.27, 0.35], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 2.5), pipe("Waste", [0.0, 0.27, 0.23], [0.0, 1.0, 0.0], K::Sanitary, 0.1, 4.0)],
        },
        Component {
            name: "WC, accessible height",
            category: C::PlumbingFixture,
            predefined: "TOILETPAN",
            element_type: None,
            color: WHITE,
            // An elongated bowl, 380 × 740, seat 460 mm (18 in, ADA 17–19 in).
            shape: || Shape::default().cuboid([-0.19, 0.19], [0.19, 0.37], [0.43, 0.81]).cuboid([-0.12, 0.12], [-0.2, 0.21], [0.0, 0.41]).prism(&bowl(0.21, 0.37, 0.58), 0.41, 0.46),
            connectors: || vec![pipe("Cold", [0.12, 0.37, 0.2], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 2.5), pipe("Waste", [0.0, 0.065, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.1, 4.0)],
        },
        Component {
            name: "Urinal, wall-hung",
            category: C::PlumbingFixture,
            predefined: "URINAL",
            element_type: None,
            color: WHITE,
            // 360 × 350, its rim 430 mm up (ADA at most 17 in), to 950 mm.
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 0.36, 0.35, 0.12), 0.43, 0.95),
            connectors: || vec![pipe("Cold", [0.0, 0.175, 0.9], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 4.0), pipe("Waste", [0.0, 0.175, 0.5], [0.0, 1.0, 0.0], K::Sanitary, 0.05, 2.0)],
        },
        Component {
            name: "Lavatory, wall-hung",
            category: C::PlumbingFixture,
            predefined: "WASHHANDBASIN",
            element_type: None,
            color: WHITE,
            // 550 × 450, rim 850 mm (ADA at most 34 in), knee space under it.
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 0.55, 0.45, 0.08), 0.69, 0.85),
            connectors: || vec![pipe("Cold", [-0.08, 0.225, 0.55], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 1.0), pipe("Hot", [0.08, 0.225, 0.55], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 1.0), pipe("Waste", [0.0, 0.225, 0.45], [0.0, 1.0, 0.0], K::Sanitary, 0.04, 1.0)],
        },
        Component {
            name: "Lavatory, pedestal",
            category: C::PlumbingFixture,
            predefined: "WASHHANDBASIN",
            element_type: None,
            color: WHITE,
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 0.55, 0.45, 0.08), 0.69, 0.85).prism(&rounded([0.0, 0.08], 0.18, 0.16, 0.05), 0.0, 0.69),
            connectors: || vec![pipe("Cold", [-0.08, 0.225, 0.55], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 1.0), pipe("Hot", [0.08, 0.225, 0.55], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 1.0), pipe("Waste", [0.0, 0.225, 0.45], [0.0, 1.0, 0.0], K::Sanitary, 0.04, 1.0)],
        },
        Component {
            name: "Hand-rinse basin",
            category: C::PlumbingFixture,
            predefined: "WASHHANDBASIN",
            element_type: None,
            color: WHITE,
            // 350 × 200 (Approved Document M's planning size), rim 800 mm.
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 0.35, 0.2, 0.05), 0.68, 0.8),
            connectors: || vec![pipe("Cold", [0.0, 0.1, 0.55], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 0.5), pipe("Waste", [0.0, 0.1, 0.45], [0.0, 1.0, 0.0], K::Sanitary, 0.032, 0.5)],
        },
        Component {
            name: "Vanity basin, 900",
            category: C::PlumbingFixture,
            predefined: "WASHHANDBASIN",
            element_type: None,
            color: WOOD,
            // A 900 × 530 cabinet on a plinth, its top 850 mm up.
            shape: || Shape::default().cuboid([-0.43, 0.43], [-0.24, 0.265], [0.0, 0.1]).cuboid([-0.45, 0.45], [-0.265, 0.265], [0.1, 0.82]).cuboid([-0.45, 0.45], [-0.265, 0.265], [0.82, 0.85]),
            connectors: || vec![pipe("Cold", [-0.08, 0.265, 0.55], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 1.0), pipe("Hot", [0.08, 0.265, 0.55], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 1.0), pipe("Waste", [0.0, 0.265, 0.45], [0.0, 1.0, 0.0], K::Sanitary, 0.04, 1.0)],
        },
        Component {
            name: "Bidet",
            category: C::PlumbingFixture,
            predefined: "BIDET",
            element_type: None,
            color: WHITE,
            shape: || Shape::default().cuboid([-0.12, 0.12], [-0.18, 0.28], [0.0, 0.34]).prism(&bowl(0.28, 0.36, 0.56), 0.34, 0.4),
            connectors: || vec![pipe("Cold", [-0.08, 0.28, 0.2], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 1.0), pipe("Hot", [0.08, 0.28, 0.2], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 1.0), pipe("Waste", [0.0, 0.1, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.04, 1.0)],
        },
        Component {
            name: "Bath, 1700 × 700",
            category: C::PlumbingFixture,
            predefined: "BATH",
            element_type: None,
            color: WHITE,
            // Its long side against the wall; rim 550 mm, 60 mm sides.
            shape: || bath_shape(1.7, 0.7, 0.55),
            connectors: || bath_connectors(1.7, 0.7),
        },
        Component {
            name: "Whirlpool bath, 1700 × 750",
            category: C::PlumbingFixture,
            predefined: "BATH",
            element_type: None,
            color: WHITE,
            // As the 1700 bath, 750 wide; its pump under the foot end, 600 mm from the middle.
            shape: || bath_shape(1.7, 0.75, 0.55),
            connectors: || bath_connectors(1.7, 0.75),
        },
        Component {
            name: "Bath, 60 × 30 in",
            category: C::PlumbingFixture,
            predefined: "BATH",
            element_type: None,
            color: WHITE,
            // An alcove tub, 1525 × 760, rim 380 mm (15 in).
            shape: || bath_shape(1.525, 0.76, 0.38),
            connectors: || bath_connectors(1.525, 0.76),
        },
        Component {
            name: "Shower tray, 900 × 900",
            category: C::PlumbingFixture,
            predefined: "SHOWER",
            element_type: None,
            color: WHITE,
            shape: || Shape::default().cuboid([-0.45, 0.45], [-0.45, 0.45], [0.0, 0.04]),
            connectors: || shower_connectors(0.9),
        },
        Component {
            name: "Shower tray, 1200 × 800",
            category: C::PlumbingFixture,
            predefined: "SHOWER",
            element_type: None,
            color: WHITE,
            shape: || Shape::default().cuboid([-0.6, 0.6], [-0.4, 0.4], [0.0, 0.04]),
            connectors: || shower_connectors(0.8),
        },
        Component {
            name: "Roll-in shower, 60 × 30 in",
            category: C::PlumbingFixture,
            predefined: "SHOWER",
            element_type: None,
            color: WHITE,
            // A level-access former, 1525 × 760, 13 mm up (ADA thresholds at most ½ in).
            shape: || Shape::default().cuboid([-0.7625, 0.7625], [-0.38, 0.38], [0.0, 0.013]),
            connectors: || shower_connectors(0.76),
        },
        // Hot tubs (ARC-BATH-013): portable, plugged in, filled weights from spa_props.
        Component {
            name: "Hot tub, 2.0 × 2.0 m",
            category: C::PlumbingFixture,
            predefined: "USERDEFINED",
            element_type: Some("Hot Tub"),
            color: WOOD,
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 2.0, 2.0, 0.2), 0.0, 0.9),
            connectors: none,
        },
        Component {
            name: "Hot tub, 2.3 × 2.3 m",
            category: C::PlumbingFixture,
            predefined: "USERDEFINED",
            element_type: Some("Hot Tub"),
            color: WOOD,
            shape: || Shape::default().prism(&rounded([0.0, 0.0], 2.3, 2.3, 0.25), 0.0, 0.95),
            connectors: none,
        },
        // Saunas (ARC-BATH-014): a wall heater and bench modules, 600 mm along the wall.
        Component {
            name: "Sauna heater, wall",
            category: C::SpaceHeater,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Heater"),
            color: STEEL,
            // A 400 × 300 mm heater 150–750 mm up, its stones heaped on top.
            shape: || Shape::default().cuboid([-0.2, 0.2], [-0.15, 0.15], [0.15, 0.65]).cuboid([-0.17, 0.17], [-0.12, 0.12], [0.65, 0.75]),
            connectors: none,
        },
        Component {
            name: "Sauna vent 100",
            category: C::AirTerminal,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Vent"),
            color: WOOD,
            // A 100 mm round wooden vent with a slide, its centre at its insertion height.
            shape: || Shape::default().prism(&rounded([0.0, -0.01], 0.14, 0.02, 0.0), -0.07, 0.07),
            connectors: none,
        },
        Component {
            name: "Sauna vent 200",
            category: C::AirTerminal,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Vent"),
            color: WOOD,
            shape: || Shape::default().prism(&rounded([0.0, -0.01], 0.24, 0.02, 0.0), -0.12, 0.12),
            connectors: none,
        },
        Component {
            name: "Sauna thermostat sensor",
            category: C::Sensor,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Sensor"),
            color: GREY,
            // A 60 × 30 × 40 mm sensor, its top at its insertion height.
            shape: || Shape::default().cuboid([-0.03, 0.03], [-0.04, 0.0], [-0.03, 0.0]),
            connectors: none,
        },
        // The door's vent and the notice outside it (IMC 914).
        Component {
            name: "Sauna door vent 4 × 8 in",
            category: C::AirTerminal,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Door Vent"),
            color: WOOD,
            // A 203 × 102 mm grille through a 40 mm door leaf, its centre at its insertion height.
            shape: || Shape::default().cuboid([-0.1015, 0.1015], [-0.02, 0.02], [-0.051, 0.051]),
            connectors: none,
        },
        Component {
            name: "Sauna notice",
            category: C::GenericModel,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Notice"),
            color: WHITE,
            // A 300 × 200 mm plate on the wall, its centre at its insertion height.
            shape: || Shape::default().cuboid([-0.15, 0.15], [-0.005, 0.0], [-0.1, 0.1]),
            connectors: none,
        },
        Component {
            name: "Sauna bench, upper 600",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Bench"),
            color: WOOD,
            // A 600 × 600 mm board, its top 450 mm up, skirted in front; raised by its offset.
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.3, 0.3], [0.41, 0.45]).cuboid([-0.3, 0.3], [-0.3, -0.28], [0.0, 0.41]),
            connectors: none,
        },
        Component {
            name: "Sauna bench, lower 600",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Bench"),
            color: WOOD,
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.225, 0.225], [0.41, 0.45]).cuboid([-0.3, 0.3], [-0.225, -0.205], [0.0, 0.41]),
            connectors: none,
        },
        // Steam rooms (ARC-BATH-015): the steam head low on a wall, its control and a tiled bench.
        Component {
            name: "Steam head",
            category: C::PlumbingFixture,
            predefined: "USERDEFINED",
            element_type: Some("Steam Head"),
            color: STEEL,
            // A 120 mm head 300 mm up, 60 mm off the wall.
            shape: || Shape::default().cuboid([-0.06, 0.06], [-0.06, 0.0], [0.25, 0.35]),
            connectors: || vec![pipe("Steam", [0.0, 0.0, 0.3], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 0.0)],
        },
        Component {
            name: "Steam control",
            category: C::Controller,
            predefined: "USERDEFINED",
            element_type: Some("Steam Control"),
            color: GREY,
            // A 120 × 80 mm control pad 1.5 m up.
            shape: || Shape::default().cuboid([-0.06, 0.06], [-0.02, 0.0], [1.46, 1.54]),
            connectors: none,
        },
        Component {
            name: "Steam generator",
            category: C::Humidifier,
            predefined: "STEAMINJECTION",
            element_type: None,
            color: GREY,
            // A 450 × 250 × 400 mm generator on the floor outside the room, its back to the wall; the
            // steam line and the water supply leave its top, the drain its base.
            shape: || Shape::default().cuboid([-0.225, 0.225], [-0.25, 0.0], [0.0, 0.4]),
            connectors: || {
                vec![
                    pipe("Steam", [0.15, -0.125, 0.4], [0.0, 0.0, 1.0], K::DomesticHotWater, 0.015, 0.0),
                    pipe("Cold water", [-0.15, -0.125, 0.4], [0.0, 0.0, 1.0], K::DomesticColdWater, 0.015, 0.0),
                    pipe("Drain", [0.0, -0.125, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.02, 0.0),
                ]
            },
        },
        Component {
            name: "Steam room bench 600",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Steam Bench"),
            color: WHITE,
            // A tiled masonry bench 600 long, 450 deep, 450 high.
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.225, 0.225], [0.0, 0.45]),
            connectors: none,
        },
        // Accessible spa rooms and spas (ARC-BATH-016): a §903 bench module for saunas and steam
        // rooms, a transfer wall and a pool lift.
        Component {
            name: "Sauna bench, accessible 600",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Sauna Bench"),
            color: WOOD,
            // 600 long, 610 (24 in) deep, its top 460 mm up (17–19 in).
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.305, 0.305], [0.42, 0.46]).cuboid([-0.3, 0.3], [-0.305, -0.285], [0.0, 0.42]),
            connectors: none,
        },
        Component {
            name: "Steam room bench, accessible 600",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Steam Bench"),
            color: WHITE,
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.305, 0.305], [0.0, 0.46]),
            connectors: none,
        },
        Component {
            name: "Spa transfer wall",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Spa Transfer Wall"),
            color: WHITE,
            // 1525 long, 355 deep, 455 high (ADA 1009.4), with two grab bars across it 4–6 in
            // above, 610 mm apart.
            shape: || {
                Shape::default()
                    .cuboid([-0.7625, 0.7625], [-0.1775, 0.1775], [0.0, 0.455])
                    .cuboid([-0.325, -0.285], [-0.1775, 0.1775], [0.455, 0.585])
                    .cuboid([0.285, 0.325], [-0.1775, 0.1775], [0.455, 0.585])
            },
            connectors: none,
        },
        Component {
            name: "Pool lift",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Pool Lift"),
            color: GREY,
            // A post on the deck with a seat 460 mm up swung over the water.
            shape: || Shape::default().cuboid([-0.08, 0.08], [0.2, 0.36], [0.0, 1.4]).cuboid([-0.23, 0.23], [-0.25, 0.2], [0.42, 0.46]),
            connectors: none,
        },
        // Accessories (IfcFurniture USERDEFINED, as research/13 E3 maps them).
        Component {
            name: "Mirror, 600 × 800",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Mirror"),
            color: MIRROR,
            // Its bottom 1000 mm up (ADA at most 40 in over a lavatory).
            shape: || Shape::default().cuboid([-0.3, 0.3], [-0.01, 0.01], [1.0, 1.8]),
            connectors: none,
        },
        Component {
            name: "Toilet roll holder",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Toilet Roll Holder"),
            color: STEEL,
            // Outlet about 600 mm up (ADA 15–48 in).
            shape: || Shape::default().cuboid([-0.075, 0.075], [-0.05, 0.05], [0.6, 0.7]),
            connectors: none,
        },
        Component {
            name: "Soap dispenser",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Soap Dispenser"),
            color: STEEL,
            shape: || Shape::default().cuboid([-0.06, 0.06], [-0.055, 0.055], [0.95, 1.2]),
            connectors: none,
        },
        Component {
            name: "Paper towel dispenser",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Paper Towel Dispenser"),
            color: STEEL,
            shape: || Shape::default().cuboid([-0.14, 0.14], [-0.06, 0.06], [0.9, 1.28]),
            connectors: none,
        },
        Component {
            name: "Hand dryer",
            category: C::ElectricAppliance,
            predefined: "HANDDRYER",
            element_type: None,
            color: GREY,
            // Its outlet 950 mm up, within reach (ADA 48 in, UK 800–1000 mm).
            shape: || Shape::default().cuboid([-0.15, 0.15], [-0.1, 0.1], [0.95, 1.6]),
            connectors: || vec![crate::mep::power("Power", [0.0, 0.1, 1.3], [0.0, 1.0, 0.0], D::In, 230.0, 1, 1800.0, "Power")],
        },
        Component {
            name: "Baby changing unit",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Baby Changing Unit"),
            color: GREY,
            // Closed, 895 × 100 × 565, its top about 1070 mm up.
            shape: || Shape::default().cuboid([-0.4475, 0.4475], [-0.05, 0.05], [0.505, 1.07]),
            connectors: none,
        },
        Component {
            name: "Sanitary bin",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Sanitary Bin"),
            color: GREY,
            shape: || Shape::default().cuboid([-0.125, 0.125], [-0.1, 0.1], [0.0, 0.45]),
            connectors: none,
        },
        Component {
            name: "Coat hook",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Coat Hook"),
            color: STEEL,
            // 1200 mm up, within reach (ADA 48 in; UK 1050 and 1400 mm).
            shape: || Shape::default().cuboid([-0.015, 0.015], [-0.03, 0.03], [1.17, 1.23]),
            connectors: none,
        },
        Component {
            name: "Shower seat, folding",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Shower Seat"),
            color: WHITE,
            // 400 deep (ADA 15–16 in from the wall), its top 460 mm up (ADA 17–19 in), down.
            shape: || Shape::default().cuboid([-0.2, 0.2], [-0.2, 0.2], [0.42, 0.46]).cuboid([-0.2, 0.2], [0.17, 0.2], [0.46, 0.6]),
            connectors: none,
        },
        // Locker rooms (ARC-BATH-009).
        Component {
            name: "Locker, full height",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Locker"),
            color: GREY,
            // 15 × 18 in, 72 in tall on a 6 in base (US federal modules).
            shape: || Shape::default().cuboid([-0.18, 0.18], [-0.21, 0.2285], [0.0, 0.152]).cuboid([-0.1905, 0.1905], [-0.2285, 0.2285], [0.152, 1.981]),
            connectors: none,
        },
        Component {
            name: "Locker, two tier",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Locker"),
            color: GREY,
            shape: || Shape::default().cuboid([-0.18, 0.18], [-0.21, 0.2285], [0.0, 0.152]).cuboid([-0.1905, 0.1905], [-0.2285, 0.2285], [0.152, 1.06]).cuboid([-0.1905, 0.1905], [-0.2285, 0.2285], [1.073, 1.981]),
            connectors: none,
        },
        Component {
            name: "Bench, locker room",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Bench"),
            color: WOOD,
            // 1830 × 305 mm top at 445 mm (17½ in), on two pedestals.
            shape: || Shape::default().cuboid([-0.915, 0.915], [-0.1525, 0.1525], [0.415, 0.445]).cuboid([-0.75, -0.69], [-0.06, 0.06], [0.0, 0.415]).cuboid([0.69, 0.75], [-0.06, 0.06], [0.0, 0.415]),
            connectors: none,
        },
        Component {
            name: "Bench, accessible",
            category: C::Furniture,
            predefined: "USERDEFINED",
            element_type: Some("Bench"),
            color: WOOD,
            // 42 × 24 in seat at 18 in, fixed to the wall that backs it (ADA 903).
            shape: || Shape::default().cuboid([-0.5325, 0.5325], [-0.305, 0.305], [0.42, 0.46]).cuboid([-0.5325, 0.5325], [0.28, 0.305], [0.51, 0.9]),
            connectors: none,
        },
        Component {
            name: "Shower head, gang",
            category: C::PlumbingFixture,
            predefined: "SHOWER",
            element_type: None,
            color: STEEL,
            // A wall-mounted head and valve, the head at 2.0 m.
            shape: || Shape::default().cuboid([-0.06, 0.06], [-0.04, 0.04], [1.0, 1.15]).cuboid([-0.05, 0.05], [-0.15, 0.04], [1.95, 2.0]),
            connectors: || vec![pipe("Cold", [-0.03, 0.04, 1.1], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 2.0), pipe("Hot", [0.03, 0.04, 1.1], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 2.0)],
        },
        Component {
            name: "Heated towel rail, 500 × 800",
            category: C::SpaceHeater,
            predefined: "RADIATOR",
            element_type: None,
            color: STEEL,
            // A ladder of rails from 300 mm to 1100 mm up.
            shape: || {
                let mut s = Shape::default().cuboid([-0.25, -0.22], [-0.03, 0.0], [0.3, 1.1]).cuboid([0.22, 0.25], [-0.03, 0.0], [0.3, 1.1]);
                for k in 0..8 {
                    let z = 0.36 + 0.1 * f64::from(k);
                    s = s.cuboid([-0.22, 0.22], [-0.025, -0.005], [z, z + 0.02]);
                }
                s
            },
            connectors: || vec![crate::mep::power("Power", [0.2, 0.0, 0.3], [0.0, 1.0, 0.0], D::In, 230.0, 1, 300.0, "Power")],
        },
    ]
}

fn bath_shape(l: f64, w: f64, h: f64) -> Shape {
    let (x, y, t) = (l / 2.0, w / 2.0, 0.06);
    Shape::default()
        .cuboid([-x, x], [-y, y], [0.0, 0.1])
        .cuboid([-x, x], [-y, -y + t], [0.1, h])
        .cuboid([-x, x], [y - t, y], [0.1, h])
        .cuboid([-x, -x + t], [-y + t, y - t], [0.1, h])
        .cuboid([x - t, x], [-y + t, y - t], [0.1, h])
}

fn bath_connectors(l: f64, w: f64) -> Vec<Connector> {
    let (x, y) = (l / 2.0, w / 2.0);
    vec![
        pipe("Cold", [-x + 0.15, y, 0.6], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.02, 2.0),
        pipe("Hot", [-x + 0.3, y, 0.6], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.02, 2.0),
        pipe("Waste", [-x + 0.2, 0.0, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.04, 2.0),
    ]
}

/// A shower's valve on the wall behind it, 1100 mm up, and its waste in the middle of the tray.
fn shower_connectors(w: f64) -> Vec<Connector> {
    let y = w / 2.0;
    vec![
        pipe("Cold", [-0.08, y, 1.1], [0.0, 1.0, 0.0], K::DomesticColdWater, 0.015, 2.0),
        pipe("Hot", [0.08, y, 1.1], [0.0, 1.0, 0.0], K::DomesticHotWater, 0.015, 2.0),
        pipe("Waste", [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], K::Sanitary, 0.05, 2.0),
    ]
}

/// A hot tub's filled weight (kg) and seats: about 450 kg/m² of footprint, the middle of one
/// maker's 400–490 (research/13 D1.6).
fn spa_props(name: &str) -> Option<(f64, u32)> {
    match name {
        "Hot tub, 2.0 × 2.0 m" => Some((1800.0, 5)),
        "Hot tub, 2.3 × 2.3 m" => Some((2400.0, 7)),
        _ => None,
    }
}

/// The components' names.
pub fn names() -> Vec<&'static str> {
    catalogue().iter().map(|c| c.name).collect()
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadParams {
    /// Which (by name); all when empty.
    #[serde(default)]
    pub names: Vec<String>,
}

/// Loads generic bathroom components into the project (`bath.load_components`); a type of the
/// same name already there is reused. Returns the types by name.
pub(crate) fn load(txn: &mut Transaction<'_>, p: LoadParams) -> Result<BTreeMap<String, ElementId>> {
    let all = catalogue();
    for n in &p.names {
        if !all.iter().any(|c| c.name == n) {
            return Err(Error::new("bath.no_component", format!("no bathroom component {n:?}; there are: {}", names().join(", "))));
        }
    }
    let mut out = BTreeMap::new();
    for c in all.into_iter().filter(|c| p.names.is_empty() || p.names.iter().any(|n| n == c.name)) {
        let store = txn.store().clone();
        let existing = store.iter().find(|e| e.name.as_deref() == Some(c.name) && crate::mesh_objects::mesh_object_type(e).is_some()).map(crate::element::Element::id);
        let id = match existing {
            Some(id) => id,
            None => {
                let s = (c.shape)();
                let connectors = (c.connectors)().into_iter().map(crate::mep::validate).collect::<Result<Vec<_>>>()?;
                let id = crate::mesh_objects::create_generated_type(txn, c.name, c.category, &s.p, &s.t, c.color, connectors, Some(c.predefined))?;
                let element_type = c.element_type;
                txn.modify(id, |e| {
                    e.params.insert(MARK.into(), true.into());
                    if let Some(t) = element_type {
                        e.params.insert("@ElementType".into(), t.into());
                    }
                    if c.name == "Heated towel rail, 500 × 800" {
                        // Rated for BS 7671's zone 1 when fixed (research/13 E3); a product's own
                        // rating replaces it.
                        e.params.insert("Pset_ElectricalDeviceCommon.IP_Code".into(), serde_json::json!({ "type": "IfcLabel", "value": "IPX4" }));
                    }
                    if c.name == "Whirlpool bath, 1700 × 750" {
                        // IFC's bath type for a whirlpool (PEnum_BathType SPA), and its pump.
                        e.params.insert("Pset_SanitaryTerminalTypeBath.BathType".into(), serde_json::json!({ "type": "IfcLabel", "value": "SPA" }));
                        e.params.insert(crate::bath_rules::PUMP_POSITION.into(), serde_json::json!({ "type": "IfcLengthMeasure", "value": 0.6 }));
                    }
                    if c.name == "Sauna heater, wall" {
                        // A 6 kW wall heater's typical distances (research/13 D3.3); a product's
                        // replace them.
                        e.params.insert(crate::bath_rules::HEATER_POWER.into(), serde_json::json!({ "type": "IfcPowerMeasure", "value": 6000.0 }));
                        e.params.insert(crate::bath_rules::HEATER_SIDE.into(), serde_json::json!({ "type": "IfcPositiveLengthMeasure", "value": 0.08 }));
                        e.params.insert(crate::bath_rules::HEATER_FRONT.into(), serde_json::json!({ "type": "IfcPositiveLengthMeasure", "value": 0.08 }));
                        e.params.insert(crate::bath_rules::HEATER_CEILING.into(), serde_json::json!({ "type": "IfcPositiveLengthMeasure", "value": 1.1 }));
                    }
                    if let Some((kg, seats)) = spa_props(c.name) {
                        e.params.insert(crate::bath_rules::SPA_WEIGHT.into(), serde_json::json!({ "type": "IfcMassMeasure", "value": kg }));
                        e.params.insert(crate::bath_rules::SPA_SEATS.into(), serde_json::json!({ "type": "IfcCountMeasure", "value": seats }));
                    }
                })?;
                id
            }
        };
        out.insert(c.name.to_owned(), id);
    }
    Ok(out)
}

/// Places a bathroom component on the nearest wall face, or the one given, its back to the wall
/// (`bath.place_component`). Its mounting height is in its geometry, so it stands on the level
/// unless `offset` says otherwise.
pub(crate) fn place(txn: &mut Transaction<'_>, p: crate::mep_controls::PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let ty = store.resolve(&p.type_id).and_then(|t| store.get(t)).ok_or_else(|| Error::new("element.not_found", "no such type"))?;
    if !is_component(ty) {
        return Err(Error::new("bath.not_a_component", format!("{} is not a bathroom component (bath.load_components)", ty.name.clone().unwrap_or_default())));
    }
    crate::mep_controls::place_hosted(txn, p, |_| true, "bath.not_a_component", "a bathroom component", 0.0)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MakeProductParams {
    /// The generic component it stands for (a type from `bath.load_components`).
    pub generic: ElementRef,
    /// The manufacturer's mesh (OBJ, STL, PLY, glTF, GLB).
    pub path: String,
    /// The product type's name; the file's name when omitted.
    pub name: Option<String>,
    /// A turn about the vertical, degrees anticlockwise, to put its back to +y (toward the wall).
    #[serde(default)]
    pub turn: f64,
    #[serde(default)]
    pub units: crate::mesh_objects::Units,
    #[serde(default)]
    pub up_axis: crate::mesh_objects::UpAxis,
    pub manufacturer: Option<String>,
    pub model_reference: Option<String>,
    pub model_label: Option<String>,
    pub article_number: Option<String>,
    pub gtin: Option<String>,
}

/// Makes a product type from a manufacturer's mesh, of a generic component's kind
/// (`bath.make_product`): its category, IFC predefined type and element type, its connectors at
/// the same places relative to its back and centre, and its geometry raised to the generic's
/// lowest point (its mounting height). Manufacturer data go in `Pset_ManufacturerTypeInformation`.
pub(crate) fn make_product(txn: &mut Transaction<'_>, p: MakeProductParams) -> Result<ElementId> {
    use crate::mesh_objects::{self as mo, ImportOptions, Insertion};
    let store = txn.store().clone();
    let g = store.resolve(&p.generic).and_then(|t| store.get(t)).filter(|e| is_component(e)).ok_or_else(|| Error::new("bath.not_a_component", "the generic is not a bathroom component (bath.load_components)"))?;
    let gt = mo::mesh_object_type(g).expect("a component type");
    let gm = gt.mesh.decode()?;
    let g_min_z = gm.positions.iter().map(|q| f64::from(q[2])).fold(f64::MAX, f64::min) * gt.scale;
    let options = ImportOptions { units: p.units, up_axis: p.up_axis, insertion: Insertion::BottomCentre, clean: true };
    let id = mo::import(txn, mo::ImportParams { path: p.path.clone(), name: p.name.clone(), category: gt.category, options, allow_large: false })?;
    let mut t = mo::mesh_object_type(txn.store().get(id).expect("imported")).expect("a type");
    let m = t.mesh.decode()?;
    // Turned, then centred again in plan and raised to the generic's lowest point.
    let (s, c) = p.turn.to_radians().sin_cos();
    let turned: Vec<[f64; 3]> = m.positions.iter().map(|q| {
        let (x, y) = (f64::from(q[0]), f64::from(q[1]));
        [x * c - y * s, x * s + y * c, f64::from(q[2])]
    }).collect();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &turned {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let dz = g_min_z / t.scale - lo[2];
    let positions: Vec<[f64; 3]> = turned.iter().map(|q| [q[0] - cx, q[1] - cy, q[2] + dz]).collect();
    let slots: Vec<u32> = m.tri_slot.iter().map(|s| u32::from(*s)).collect();
    t.mesh = mo::StoredMesh::encode(&positions, &m.triangles, &slots);
    t.size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    // The generic's connectors, kept on the product's back plane.
    if t.connectors.is_empty() {
        let back_g = gt.size[1] * gt.scale / 2.0;
        let back_p = t.size[1] * t.scale / 2.0;
        t.connectors = gt.connectors.iter().cloned().map(|mut k| {
            if (k.origin[1] - back_g).abs() < 1e-6 {
                k.origin[1] = back_p;
            }
            k
        }).collect();
    }
    t.predefined = gt.predefined.clone();
    let element_type = g.param("@ElementType").cloned();
    let predefined = gt.predefined_type().to_owned();
    let recipe = serde_json::to_value(&t).expect("serialises");
    // With their IFC types (IFC4 Pset_ManufacturerTypeInformation).
    let data: Vec<(&str, &str, Option<String>)> = vec![
        ("Manufacturer", "IfcLabel", p.manufacturer),
        ("ModelReference", "IfcLabel", p.model_reference),
        ("ModelLabel", "IfcLabel", p.model_label),
        ("ArticleNumber", "IfcIdentifier", p.article_number),
        ("GlobalTradeItemNumber", "IfcIdentifier", p.gtin),
    ];
    txn.modify(id, |e| {
        e.params.insert(crate::walls::RECIPE_KEY.into(), recipe);
        e.params.insert("@PredefinedType".into(), predefined.into());
        e.params.insert(MARK.into(), true.into());
        if let Some(v) = element_type {
            e.params.insert("@ElementType".into(), v);
        }
        for (k, ty, v) in data {
            if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
                e.params.insert(format!("Pset_ManufacturerTypeInformation.{k}"), serde_json::json!({ "type": ty, "value": v }));
            }
        }
    })?;
    Ok(id)
}
