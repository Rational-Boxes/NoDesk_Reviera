// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

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
use crate::id::ElementId;
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
