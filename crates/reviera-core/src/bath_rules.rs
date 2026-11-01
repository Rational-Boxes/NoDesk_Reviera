// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Bathroom rule packs and fixture clearance zones (ARC-BATH-001, ARC-BATH-002).
//!
//! A rule pack is JSON following `schemas/bath/reviera.bath-rules-1.schema.json` (MIT): zones
//! kept clear in front of, beside or around fixtures, and distances such as a WC's centreline
//! from its side wall, each with its clause, source and confidence. Four ship with Reviera (the
//! US plumbing codes, NKBA guidance, the 2010 ADA Standards and England's Approved Document M);
//! a project may add its own or replace a shipped one (`rxd:bath_rule_packs`). The packs in use
//! are the project's choice (`rxd:bath_rules`), or chosen from its building code and
//! accessibility rule set.
//!
//! A fixture's zones are virtual volumes ([`crate::clearances`], kind *fixture space*), placed in
//! the fixture's own frame: *across* from its centreline, an edge or its side wall (positive
//! toward the open side), and *out* from the wall behind it or its front.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bodies::{Mesh, extrude};
use crate::clearances::{Clearance, ClearanceKind};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::{self, Placement};
use crate::store::Store;
use crate::txn::Transaction;

/// The packs a project has chosen (absent: chosen from its code and accessibility rule set).
pub const RULES_KEY: &str = "rxd:bath_rules";
/// The project's own packs.
pub const PACKS_KEY: &str = "rxd:bath_rule_packs";
/// A fixture's side wall: `left` or `right` as you face it (absent: found).
pub const HAND_KEY: &str = "rxd:fixture_hand";
/// Whether a fixture is one the accessibility packs apply to (absent: it is). A toilet core's
/// standard compartments, for example, are not (ARC-BATH-008).
pub const ACCESSIBLE_KEY: &str = "rxd:bath_accessible";

/// The published pack schema (MIT).
pub const SCHEMA: &str = include_str!("../../../schemas/bath/reviera.bath-rules-1.schema.json");
const SHIPPED: &[&str] = &[
    include_str!("../../../schemas/bath/packs/us_plumbing.json"),
    include_str!("../../../schemas/bath/packs/us_nkba.json"),
    include_str!("../../../schemas/bath/packs/ada_2010.json"),
    include_str!("../../../schemas/bath/packs/england_adm.json"),
    include_str!("../../../schemas/bath/packs/uk_workplace.json"),
    include_str!("../../../schemas/bath/packs/uk_bs7671.json"),
    include_str!("../../../schemas/bath/packs/us_spa.json"),
    include_str!("../../../schemas/bath/packs/us_nec.json"),
];

/// How far either side of a fixture a side wall is looked for.
pub const SIDE_SEARCH: f64 = 1.2;
/// A zone's height when its rule gives none.
pub const DEFAULT_HEIGHT: f64 = 1.0;
/// Where no side wall is found and the packs give no distance, it is taken this far away.
pub const ASSUMED_SIDE: f64 = 0.45;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// A code's minimum.
    Minimum,
    /// An accessibility standard.
    Accessible,
    /// Guidance: its findings are notes.
    Recommended,
}

/// A bathroom fixture, by its sanitary terminal's IFC predefined type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FixtureKind {
    WaterCloset,
    Urinal,
    Lavatory,
    Bidet,
    Bath,
    Shower,
    /// Locker-room furniture (ARC-BATH-009), by its ElementType.
    Locker,
    Bench,
    /// A hot tub or portable spa (ARC-BATH-013), by its ObjectType.
    Spa,
    /// A sauna heater (ARC-BATH-014), by its ObjectType.
    SaunaHeater,
    /// A steam room's steam head (ARC-BATH-015), by its ObjectType.
    SteamHead,
}

impl FixtureKind {
    /// From a furniture copy's ObjectType: lockers and benches.
    pub fn from_object_type(o: &str) -> Option<Self> {
        Some(match o {
            "Locker" => Self::Locker,
            "Bench" => Self::Bench,
            "Hot Tub" => Self::Spa,
            "Sauna Heater" => Self::SaunaHeater,
            "Steam Head" => Self::SteamHead,
            _ => return None,
        })
    }

    /// From an `IfcSanitaryTerminal` predefined type; a kitchen sink is not a bathroom fixture.
    pub fn from_predefined(p: &str) -> Option<Self> {
        Some(match p {
            "TOILETPAN" => Self::WaterCloset,
            "URINAL" => Self::Urinal,
            "WASHHANDBASIN" => Self::Lavatory,
            "BIDET" => Self::Bidet,
            "BATH" => Self::Bath,
            "SHOWER" => Self::Shower,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WaterCloset => "water closet",
            Self::Urinal => "urinal",
            Self::Lavatory => "lavatory",
            Self::Bidet => "bidet",
            Self::Bath => "bath",
            Self::Shower => "shower",
            Self::Locker => "locker",
            Self::Bench => "bench",
            Self::Spa => "hot tub",
            Self::SaunaHeater => "sauna heater",
            Self::SteamHead => "steam head",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleOccupancy {
    #[default]
    Any,
    Dwelling,
    Other,
}

impl RuleOccupancy {
    pub fn applies(self, o: crate::codes::Occupancy) -> bool {
        use crate::codes::Occupancy as O;
        matches!((self, o), (Self::Any, _) | (Self::Dwelling, O::Dwelling) | (Self::Other, O::Other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Read in the official text.
    Verified,
    /// From a reliable summary or an adoption's text.
    Secondary,
    /// A reading that needs checking: its findings are notes.
    Advisory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcrossFrom {
    Centre,
    NearEdge,
    FarEdge,
    SideWall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutFrom {
    Wall,
    Front,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoorSwing {
    Allowed,
    Forbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    CentrelineToSideWall,
}

/// A zone kept clear at a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Zone {
    pub id: String,
    pub description: String,
    pub fixtures: Vec<FixtureKind>,
    #[serde(default)]
    pub occupancy: RuleOccupancy,
    pub source: String,
    pub confidence: Confidence,
    /// From and to, across the fixture (metres).
    pub across: [(AcrossFrom, f64); 2],
    /// From and to, out into the room (metres).
    pub out: [(OutFrom, f64); 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_swing: Option<DoorSwing>,
}

/// A measured rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Distance {
    pub id: String,
    pub description: String,
    pub fixtures: Vec<FixtureKind>,
    #[serde(default)]
    pub occupancy: RuleOccupancy,
    pub source: String,
    pub confidence: Confidence,
    pub measure: Measure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// Which wall a grab bar goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BarWall {
    /// The fixture's side wall.
    Side,
    /// The wall behind it.
    Rear,
}

/// Where a bar's stretch is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoverFrom {
    /// Out from the rear wall's face (a side-wall bar).
    RearWall,
    /// Across from the fixture's centreline, positive toward the open side (a rear-wall bar).
    Centre,
}

/// A range with the value used when placing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Range {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub default: f64,
}

impl Range {
    /// Whether `v` is in range, within `tolerance`.
    pub fn holds(&self, v: f64, tolerance: f64) -> bool {
        self.min.is_none_or(|m| v >= m - tolerance) && self.max.is_none_or(|m| v <= m + tolerance)
    }
}

/// A grab bar a fixture needs (ARC-BATH-003).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bar {
    pub id: String,
    pub description: String,
    pub fixtures: Vec<FixtureKind>,
    #[serde(default)]
    pub occupancy: RuleOccupancy,
    pub source: String,
    pub confidence: Confidence,
    pub wall: BarWall,
    /// The stretch it must cover at least (metres), and is placed over.
    pub covers: [(CoverFrom, f64); 2],
    /// The top of the bar above the floor.
    pub top: Range,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<Range>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Range>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_min: Option<f64>,
}

/// The diameter and gap a bar is placed with when its rule gives none (1½ in each).
pub const DEFAULT_BAR: f64 = 0.038;

impl Bar {
    pub fn diameter_default(&self) -> f64 {
        self.diameter.map_or(DEFAULT_BAR, |r| r.default)
    }

    pub fn gap_default(&self) -> f64 {
        self.gap.map_or(DEFAULT_BAR, |r| r.default)
    }
}

/// A rule pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pack {
    pub format: String,
    pub id: String,
    pub title: String,
    pub jurisdiction: String,
    pub edition: String,
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub distances: Vec<Distance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bars: Vec<Bar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<Electrical>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falls: Option<Falls>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waterproofing: Vec<Waterproofing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spa: Option<SpaRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sauna: Option<SaunaRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam: Option<SteamRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spa_access: Option<SpaAccess>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whirlpool: Option<Whirlpool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receptacles: Option<Receptacles>,
}

fn validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA).expect("the published schema is JSON");
        jsonschema::validator_for(&schema).expect("the published schema compiles")
    })
}

/// Reads and checks a pack.
pub fn parse(v: &Value) -> Result<Pack> {
    let errors: Vec<String> = validator().iter_errors(v).take(5).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if !errors.is_empty() {
        return Err(bad("bath.pack_schema", format!("the pack does not follow reviera.bath-rules-1: {}", errors.join("; "))));
    }
    let pack: Pack = serde_json::from_value(v.clone()).map_err(|e| bad("bath.pack_schema", e.to_string()))?;
    for d in &pack.distances {
        if let (Some(min), Some(max)) = (d.min, d.max)
            && min > max
        {
            return Err(bad("bath.pack_range", format!("{}: its minimum is above its maximum", d.id)));
        }
    }
    for b in &pack.bars {
        let ranges = [Some(b.top), b.diameter, b.gap];
        if ranges.iter().flatten().any(|r| !r.holds(r.default, 0.0) || r.min.zip(r.max).is_some_and(|(a, z)| a > z)) {
            return Err(bad("bath.pack_range", format!("{}: a range's default is outside it, or its minimum is above its maximum", b.id)));
        }
        let fits = match b.wall {
            BarWall::Side => b.covers.iter().all(|c| c.0 == CoverFrom::RearWall),
            BarWall::Rear => b.covers.iter().all(|c| c.0 == CoverFrom::Centre),
        };
        if !fits {
            return Err(bad("bath.pack_range", format!("{}: a side-wall bar is measured from the rear wall, a rear-wall bar from the centreline", b.id)));
        }
    }
    Ok(pack)
}

/// The packs that ship with Reviera.
pub fn shipped() -> &'static [Pack] {
    static P: OnceLock<Vec<Pack>> = OnceLock::new();
    P.get_or_init(|| SHIPPED.iter().map(|t| parse(&serde_json::from_str(t).expect("a shipped pack is JSON")).expect("a shipped pack is valid")).collect())
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The project's own packs.
pub fn project_packs(store: &Store) -> Vec<Pack> {
    project(store).and_then(|e| e.param(PACKS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Every pack the project can use: the shipped ones, each replaced by a project pack of its id,
/// then the project's others.
pub fn available(store: &Store) -> Vec<Pack> {
    let own = project_packs(store);
    let mut out: Vec<Pack> = shipped().iter().map(|s| own.iter().find(|p| p.id == s.id).unwrap_or(s).clone()).collect();
    out.extend(own.into_iter().filter(|p| !shipped().iter().any(|s| s.id == p.id)));
    out
}

/// The packs the project names, if it names any.
pub fn chosen(store: &Store) -> Option<Vec<String>> {
    project(store).and_then(|e| e.param(RULES_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The packs chosen for a project that names none: from its building code and its
/// accessibility rule set.
pub fn automatic(store: &Store) -> Vec<String> {
    use crate::codes::Code;
    let mut out = Vec::new();
    match crate::codes::settings(store).code {
        Code::Ibc | Code::Irc => {
            out.push("us_plumbing".to_owned());
            out.push("us_spa".to_owned());
            out.push("us_nec".to_owned());
        }
        Code::UkAdk => {
            out.push("england_adm".to_owned());
            out.push("uk_bs7671".to_owned());
        }
        _ => {}
    }
    if crate::clearances::rule_set(store) == crate::clearances::RuleSet::Ada2010 {
        out.push("ada_2010".to_owned());
    }
    out
}

/// The packs in use.
pub fn active(store: &Store) -> Vec<Pack> {
    let ids = chosen(store).unwrap_or_else(|| automatic(store));
    let all = available(store);
    ids.iter().filter_map(|id| all.iter().find(|p| &p.id == id).cloned()).collect()
}

/// Which side a fixture's side wall is on, as you face the fixture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    /// The nearer wall within reach, else the left.
    #[default]
    Auto,
    Left,
    Right,
}

/// A bathroom fixture as placed: its frame and extents (metres, in its own frame: x across,
/// y toward the wall behind it), and its side wall.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub element: ElementId,
    pub kind: FixtureKind,
    /// Its frame without scale: extents are already scaled.
    pub frame: Placement,
    pub x: [f64; 2],
    /// Its front (toward the room) and back (against the wall).
    pub front: f64,
    pub back: f64,
    /// The floor it stands on.
    pub floor: f64,
    /// −1 when the side wall is on its left (−x), 1 on its right.
    pub side: f64,
    /// The distance from its centreline to the side wall, and the wall, when one was found.
    pub side_distance: Option<f64>,
    pub side_wall: Option<ElementId>,
    /// The wall behind it, when there is one.
    pub rear_wall: Option<ElementId>,
    /// Its level (GlobalId).
    pub level: String,
    pub hand: Hand,
    /// Whether the accessibility packs' rules apply to it.
    pub accessible: bool,
}

impl Fixture {
    /// The across position of a reference, `assumed` standing in for a side wall not found.
    fn across(&self, (from, d): (AcrossFrom, f64), assumed: f64) -> f64 {
        let open = -self.side;
        let near = if self.side < 0.0 { self.x[0] } else { self.x[1] };
        let far = if self.side < 0.0 { self.x[1] } else { self.x[0] };
        match from {
            AcrossFrom::Centre => open * d,
            AcrossFrom::NearEdge => near + open * d,
            AcrossFrom::FarEdge => far + open * d,
            AcrossFrom::SideWall => open * (d - self.side_distance.unwrap_or(assumed)),
        }
    }

    fn out(&self, (from, d): (OutFrom, f64)) -> f64 {
        // Out is −y; nothing behind a wall face, but a free-standing fixture's zone may go round
        // it.
        let y = match from {
            OutFrom::Wall => self.back - d,
            OutFrom::Front => self.front - d,
        };
        if self.rear_wall.is_some() { y.min(self.back) } else { y }
    }

    /// A zone's volume, in project coordinates.
    pub fn zone_mesh(&self, z: &Zone, assumed: f64) -> Mesh {
        let xs = [self.across(z.across[0], assumed), self.across(z.across[1], assumed)];
        let ys = [self.out(z.out[0]), self.out(z.out[1])];
        let (x0, x1) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
        let (y0, y1) = (ys[0].min(ys[1]), ys[0].max(ys[1]));
        let world: Vec<[f64; 2]> = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .iter()
            .map(|[x, y]| {
                let w = self.frame.apply([*x, *y, 0.0]);
                [w[0], w[1]]
            })
            .collect();
        // Just above the floor, so the floor itself is not an intrusion.
        extrude(&world, &[], self.floor + 0.005, self.floor + z.height.unwrap_or(DEFAULT_HEIGHT))
    }
}

/// Where a ray from `p` along `d` first meets a wall's plan outline within `reach`, and the wall.
pub(crate) fn ray_to_walls(store: &Store, p: [f64; 2], d: [f64; 2], z: f64, reach: f64) -> Option<(f64, ElementId)> {
    let mut best: Option<(f64, ElementId)> = None;
    for w in store.iter().filter(|e| crate::walls::wall_recipe(e).is_some()) {
        let Ok(g) = crate::walls::regenerate(store, w) else { continue };
        if z < g.base_z || z > g.base_z + g.height {
            continue;
        }
        let f = g.footprint();
        for k in 0..4 {
            let (a, b) = (f[k], f[(k + 1) % 4]);
            if let Some(t) = ray_segment(p, d, a, b)
                && t <= reach
                && best.is_none_or(|(x, _)| t < x)
            {
                best = Some((t, w.id()));
            }
        }
    }
    best
}

/// The distance along a ray (unit `d`) to a segment, if it meets it ahead.
fn ray_segment(p: [f64; 2], d: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let e = [b[0] - a[0], b[1] - a[1]];
    let den = d[0] * e[1] - d[1] * e[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let ap = [a[0] - p[0], a[1] - p[1]];
    let t = (ap[0] * e[1] - ap[1] * e[0]) / den;
    let u = (ap[0] * d[1] - ap[1] * d[0]) / den;
    (t >= -1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u)).then_some(t.max(0.0))
}

/// A sanitary fixture's frame, extents and side wall, if it is a bathroom fixture.
pub fn fixture(store: &Store, e: &crate::element::Element) -> Option<Fixture> {
    let kind = match e.class.as_str() {
        "IfcSanitaryTerminal" => e.param("@ObjectType").and_then(|v| v.as_str()).and_then(FixtureKind::from_object_type),
        "IfcFurniture" | "IfcSpaceHeater" => Some(FixtureKind::from_object_type(e.param("@ObjectType")?.as_str()?)?),
        _ => return None,
    };
    let g = mesh_objects::regenerate(store, e).ok()?;
    let kind = match kind {
        Some(k) => k,
        None => FixtureKind::from_predefined(g.ty.predefined_type())?,
    };
    let s = g.placement.scale;
    let (mut x, mut y) = ([f64::MAX, f64::MIN], [f64::MAX, f64::MIN]);
    for p in &g.mesh.positions {
        x = [x[0].min(f64::from(p[0]) * s), x[1].max(f64::from(p[0]) * s)];
        y = [y[0].min(f64::from(p[1]) * s), y[1].max(f64::from(p[1]) * s)];
    }
    if x[0] > x[1] {
        return None;
    }
    let frame = Placement { scale: 1.0, ..g.placement };
    let r = mesh_objects::mesh_object_recipe(e)?;
    let floor = crate::levels::elevation_of(store, &r.level).unwrap_or(g.placement.origin[2]);
    let hand: Hand = e.param(HAND_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    // Look sideways from the centreline at mid-depth, 600 mm up: above the gap under toilet
    // partitions (305 mm, ARC-BATH-008).
    let mid = frame.apply([0.0, (y[0] + y[1]) / 2.0, 0.0]);
    let look = |sign: f64| {
        let d = frame.apply_direction([sign, 0.0, 0.0]);
        ray_to_walls(store, [mid[0], mid[1]], [d[0], d[1]], floor + 0.6, SIDE_SEARCH)
    };
    let (left, right) = (look(-1.0), look(1.0));
    let (side, found) = match hand {
        Hand::Left => (-1.0, left),
        Hand::Right => (1.0, right),
        Hand::Auto => match (left, right) {
            (Some(l), Some(r)) if r.0 < l.0 => (1.0, Some(r)),
            (Some(l), _) => (-1.0, Some(l)),
            (None, Some(r)) => (1.0, Some(r)),
            (None, None) => (-1.0, None),
        },
    };
    // Behind it: the wall it is mounted on, else the nearest wall just behind its back.
    let rear_wall = r.face.as_ref().and_then(|f| crate::id::GlobalId::parse(&f.wall)).and_then(|g| store.find_by_guid(&g)).or_else(|| {
        let d = frame.apply_direction([0.0, 1.0, 0.0]);
        ray_to_walls(store, [mid[0], mid[1]], [d[0], d[1]], floor + 0.3, (y[1] - y[0]) / 2.0 + 0.3).map(|(_, w)| w)
    });
    Some(Fixture {
        element: e.id(),
        kind,
        frame,
        x,
        front: y[0],
        back: y[1],
        floor,
        side,
        side_distance: found.map(|f| f.0),
        side_wall: found.map(|f| f.1),
        rear_wall,
        level: r.level.clone(),
        hand,
        accessible: e.param(ACCESSIBLE_KEY).and_then(serde_json::Value::as_bool) != Some(false),
    })
}

/// Every bathroom fixture.
pub fn fixtures(store: &Store) -> Vec<Fixture> {
    store.iter().filter_map(|e| fixture(store, e)).collect()
}

/// Where a side wall not found is taken to be, for a fixture under the packs: the largest
/// distance their rules allow it.
pub fn assumed_side(packs: &[Pack], kind: FixtureKind) -> f64 {
    packs
        .iter()
        .flat_map(|p| &p.distances)
        .filter(|d| d.fixtures.contains(&kind) && d.measure == Measure::CentrelineToSideWall)
        .filter_map(|d| d.max.or(d.min))
        .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))))
        .unwrap_or(ASSUMED_SIDE)
}

/// A fixture space and the rule it comes from.
#[derive(Debug, Clone)]
pub struct Space {
    pub clearance: Clearance,
    pub pack: String,
    pub zone: Zone,
}

/// Whether a pack's rules apply to a fixture: an accessibility pack's only to fixtures not marked
/// otherwise.
fn applies_to(p: &Pack, f: &Fixture) -> bool {
    p.layer != Layer::Accessible || f.accessible
}

/// The zones of the packs in use at one fixture.
pub fn spaces_of(store: &Store, f: &Fixture, packs: &[Pack]) -> Vec<Space> {
    let occupancy = crate::codes::settings(store).occupancy;
    let assumed = assumed_side(packs, f.kind);
    let mut out = Vec::new();
    for p in packs.iter().filter(|p| applies_to(p, f)) {
        for z in p.zones.iter().filter(|z| z.fixtures.contains(&f.kind) && z.occupancy.applies(occupancy)) {
            out.push(Space {
                clearance: Clearance {
                    element: f.element,
                    kind: ClearanceKind::FixtureSpace,
                    rule: z.id.clone(),
                    mesh: f.zone_mesh(z, assumed),
                    door_swing_forbidden: z.door_swing == Some(DoorSwing::Forbidden),
                },
                pack: p.id.clone(),
                zone: z.clone(),
            });
        }
    }
    out.extend(heater_space(store, f));
    out.extend(pump_access_space(store, f, packs));
    out
}

/// Every fixture space under the packs in use.
pub fn spaces(store: &Store) -> Vec<Space> {
    let packs = active(store);
    if packs.is_empty() {
        return Vec::new();
    }
    fixtures(store).iter().flat_map(|f| spaces_of(store, f, &packs)).collect()
}

/// The distance rules that apply to a fixture.
pub fn distances_of<'a>(store: &Store, f: &Fixture, packs: &'a [Pack]) -> Vec<(&'a Pack, &'a Distance)> {
    let occupancy = crate::codes::settings(store).occupancy;
    packs
        .iter()
        .filter(|p| applies_to(p, f))
        .flat_map(|p| p.distances.iter().map(move |d| (p, d)))
        .filter(|(_, d)| d.fixtures.contains(&f.kind) && d.occupancy.applies(occupancy))
        .collect()
}

/// The bar rules that apply to a fixture.
pub fn bars_of<'a>(store: &Store, f: &Fixture, packs: &'a [Pack]) -> Vec<(&'a Pack, &'a Bar)> {
    let occupancy = crate::codes::settings(store).occupancy;
    packs
        .iter()
        .filter(|p| applies_to(p, f))
        .flat_map(|p| p.bars.iter().map(move |b| (p, b)))
        .filter(|(_, b)| b.fixtures.contains(&f.kind) && b.occupancy.applies(occupancy))
        .collect()
}

/// Where a bar rule puts its bar at a fixture: the wall, its face (1 the exterior, −1 the
/// interior), and the stretch along the wall's location line from its start, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stretch {
    pub wall: ElementId,
    pub side: f64,
    pub along: [f64; 2],
}

impl Fixture {
    /// The stretch a bar rule asks for, or why there is none.
    pub fn bar_stretch(&self, store: &Store, bar: &Bar) -> std::result::Result<Stretch, String> {
        let (wall, points) = match bar.wall {
            BarWall::Side => {
                let (Some(wall), Some(dist)) = (self.side_wall, self.side_distance) else {
                    return Err(format!("no side wall within {:.0} mm", SIDE_SEARCH * 1000.0));
                };
                let x = self.side * dist;
                (wall, bar.covers.map(|(_, d)| [x, self.back - d]))
            }
            BarWall::Rear => {
                let Some(wall) = self.rear_wall else { return Err("no wall behind it".into()) };
                let open = -self.side;
                (wall, bar.covers.map(|(_, d)| [open * d, self.back]))
            }
        };
        let w = store.get(wall).ok_or("its wall is gone")?;
        let g = crate::walls::regenerate(store, w).map_err(|e| e.message)?;
        let (d, n) = (g.direction(), g.normal());
        let u = |p: [f64; 2]| {
            let q = self.frame.apply([p[0], p[1], 0.0]);
            (q[0] - g.start[0]) * d[0] + (q[1] - g.start[1]) * d[1]
        };
        let (a, b) = (u(points[0]), u(points[1]));
        let along = [a.min(b), a.max(b)];
        if along[0] < -0.005 || along[1] > g.length() + 0.005 {
            return Err("its wall is too short".into());
        }
        let mid = self.frame.apply([0.0, (self.front + self.back) / 2.0, 0.0]);
        let v = (mid[0] - g.start[0]) * n[0] + (mid[1] - g.start[1]) * n[1];
        Ok(Stretch { wall, side: if v >= 0.0 { 1.0 } else { -1.0 }, along })
    }
}


// ---- Fixture counts (ARC-BATH-007) ----

/// Whose number a count rule counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Male,
    Female,
    /// Each sex: the rule applies to the males and to the females, and the two are added.
    EachSex,
    Total,
    /// Dwelling or sleeping units.
    Unit,
}

/// One band of a ratio: one fixture per `per` people, up to `to` people (the last band has none).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Band {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    pub per: f64,
}

/// How many of a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Requirement {
    pub basis: Basis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<Vec<Band>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Vec<(f64, u32)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then_per: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<u32>,
}

impl Requirement {
    /// The fixtures needed for `n` people (or units).
    pub fn count(&self, n: f64) -> u32 {
        if n <= 0.0 {
            return 0;
        }
        let mut c = if let Some(bands) = &self.ratio {
            let (mut done, mut sum) = (0.0f64, 0.0f64);
            for b in bands {
                let end = b.to.unwrap_or(f64::INFINITY);
                let here = (n.min(end) - done).max(0.0);
                sum += here / b.per;
                done = end;
                if n <= end {
                    break;
                }
            }
            // A hair below a whole number stays it: 2.0000000001 is 2.
            (sum - 1e-9).ceil().max(0.0) as u32
        } else if let Some(t) = &self.table {
            match t.iter().find(|(to, _)| n <= *to) {
                Some((_, k)) => *k,
                None => {
                    let (last_to, last_k) = *t.last().expect("non-empty");
                    last_k + self.then_per.map_or(0, |p| ((n - last_to) / p - 1e-9).ceil().max(0.0) as u32)
                }
            }
        } else {
            0
        };
        if let Some(m) = self.minimum {
            c = c.max(m);
        }
        c
    }
}

/// A row of a count table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Count {
    pub id: String,
    pub occupancy: String,
    pub description: String,
    pub fixtures: BTreeMap<String, Requirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urinal_substitution: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CountParams {
    /// The occupancy, as a count row names it (`B`, `A-3`, `workplace`).
    pub occupancy: String,
    pub occupant_load: f64,
    /// The share of males (default 0.5).
    pub male_share: Option<f64>,
    /// Dwelling or sleeping units, for rows counted per unit.
    pub units: Option<f64>,
    /// IPC 2021 403.1.1: all-gender rooms, counted on the whole load.
    #[serde(default)]
    pub all_gender: bool,
    /// The pack to count by (default the first in use with the occupancy).
    pub pack: Option<String>,
    /// Count the fixtures modelled on this level (default the whole model).
    pub level_id: Option<ElementRef>,
}

/// One fixture's requirement against the model.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Need {
    pub fixture: String,
    pub required: u32,
    /// For WCs: of which for males and for females, when counted separately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub male: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub female: Option<u32>,
    pub modelled: u32,
    pub short: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Counted {
    pub pack: String,
    pub row: String,
    pub description: String,
    pub source: String,
    pub needs: Vec<Need>,
    /// How many of the WCs required for males urinals may replace.
    pub urinals_may_replace: u32,
}

/// The fixtures modelled, by the count's fixture names.
fn modelled(store: &Store, level: Option<&str>) -> BTreeMap<&'static str, u32> {
    let mut out = BTreeMap::new();
    for e in store.iter().filter(|e| e.class == "IfcSanitaryTerminal" && crate::mesh_objects::mesh_object_recipe(e).is_some()) {
        let r = crate::mesh_objects::mesh_object_recipe(e).expect("a copy");
        if level.is_some_and(|l| l != r.level) {
            continue;
        }
        let pre = crate::mesh_objects::type_of(store, e).map(|(_, t)| t.predefined_type().to_owned()).unwrap_or_default();
        let key = match pre.as_str() {
            "TOILETPAN" => "water_closet",
            "URINAL" => "urinal",
            "WASHHANDBASIN" => "lavatory",
            "SHOWER" | "BATH" => "shower",
            "SANITARYFOUNTAIN" => "drinking_fountain",
            "SINK" => "service_sink",
            _ => continue,
        };
        *out.entry(key).or_default() += 1;
    }
    out
}

/// `bath.fixture_count`.
pub(crate) fn fixture_count(store: &Store, p: CountParams) -> Result<Counted> {
    if !(p.occupant_load >= 0.0 && p.occupant_load.is_finite()) {
        return Err(bad("bath.invalid_input", "the occupant load is a number of people"));
    }
    let share = p.male_share.unwrap_or(0.5);
    if !(0.0..=1.0).contains(&share) {
        return Err(bad("bath.invalid_input", "the share of males is between 0 and 1"));
    }
    let packs: Vec<Pack> = match &p.pack {
        Some(id) => available(store).into_iter().filter(|x| &x.id == id).collect(),
        None => active(store),
    };
    let wanted = p.occupancy.trim().to_lowercase();
    let (pack, row) = packs
        .iter()
        .find_map(|x| x.counts.iter().find(|c| c.occupancy.to_lowercase() == wanted).map(|c| (x, c)))
        .ok_or_else(|| {
            let known: Vec<String> = packs.iter().flat_map(|x| x.counts.iter().map(|c| c.occupancy.clone())).collect();
            bad("bath.no_occupancy", format!("no count rule for occupancy {:?} in the packs; there are: {}", p.occupancy, known.join(", ")))
        })?;
    let n = p.occupant_load;
    let (males, females) = (n * share, n * (1.0 - share));
    let level = p
        .level_id
        .as_ref()
        .map(|r| store.resolve(r).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).map(|l| l.guid().to_string()).ok_or_else(|| bad("bath.no_level", "no such level")))
        .transpose()?;
    let have = modelled(store, level.as_deref());
    let mut by: BTreeMap<&str, (u32, Option<u32>, Option<u32>)> = BTreeMap::new();
    let mut add = |name: &str, k: u32, m: Option<u32>, f: Option<u32>| {
        let key: &'static str = match name {
            n if n.starts_with("water_closet") => "water_closet",
            n if n.starts_with("lavatory") => "lavatory",
            "urinal" => "urinal",
            "drinking_fountain" => "drinking_fountain",
            "shower" => "shower",
            "service_sink" => "service_sink",
            _ => return,
        };
        let e = by.entry(key).or_insert((0, None, None));
        e.0 += k;
        if let Some(m) = m {
            e.1 = Some(e.1.unwrap_or(0) + m);
        }
        if let Some(f) = f {
            e.2 = Some(e.2.unwrap_or(0) + f);
        }
    };
    let mut male_wcs = 0;
    // The fixture a rule is for: `water_closet_male` → `water_closet`.
    let family = |name: &str| name.trim_end_matches("_male").trim_end_matches("_female").to_owned();
    let mut done: Vec<String> = Vec::new();
    for (name, r) in &row.fixtures {
        let wc = name.starts_with("water_closet");
        match r.basis {
            Basis::Unit => add(name, r.count(p.units.unwrap_or(0.0)), None, None),
            Basis::Total => add(name, r.count(n), None, None),
            // All-gender: the whole load, by the fixture's rule that asks for more.
            _ if p.all_gender => {
                let f = family(name);
                if done.contains(&f) {
                    continue;
                }
                let k = row.fixtures.iter().filter(|(k, q)| family(k) == f && !matches!(q.basis, Basis::Unit | Basis::Total)).map(|(_, q)| q.count(n)).max().unwrap_or(0);
                add(&f, k, None, None);
                done.push(f);
            }
            Basis::Male => {
                let k = r.count(males);
                if wc {
                    male_wcs += k;
                }
                add(name, k, Some(k), None);
            }
            Basis::Female => {
                let k = r.count(females);
                add(name, k, None, Some(k));
            }
            Basis::EachSex => {
                let (m, f) = (r.count(males), r.count(females));
                if wc {
                    male_wcs += m;
                }
                add(name, m + f, Some(m), Some(f));
            }
        }
    }
    let needs = by
        .into_iter()
        .map(|(fixture, (required, male, female))| {
            let m = have.get(fixture).copied().unwrap_or(0);
            Need { fixture: fixture.to_owned(), required, male, female, modelled: m, short: required.saturating_sub(m) }
        })
        .collect();
    let urinals_may_replace = row.urinal_substitution.map_or(0, |s| (f64::from(male_wcs) * s + 1e-9).floor() as u32);
    Ok(Counted { pack: pack.id.clone(), row: row.id.clone(), description: row.description.clone(), source: row.source.clone(), needs, urinals_may_replace })
}


// ---- Electrical zones (ARC-BATH-010) ----

/// IP ratings each zone needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ZoneIp {
    pub zone0: String,
    pub zone1: String,
    pub zone2: String,
}

/// A pack's bathroom electrical zones (BS 7671 Section 701 / IEC 60364-7-701).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Electrical {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone0_trayless_height: Option<f64>,
    pub zone1_height: f64,
    pub trayless_radius: f64,
    pub zone2_width: f64,
    pub socket_distance: f64,
    pub ip: ZoneIp,
    pub source: String,
    pub confidence: Confidence,
}

/// One zone round one fixture: its plan outline and height.
#[derive(Debug, Clone)]
pub struct ElectricalZone {
    pub element: ElementId,
    /// 0, 1 or 2.
    pub zone: u8,
    pub outline: Vec<[f64; 2]>,
    pub z0: f64,
    pub z1: f64,
    pub pack: String,
}

impl ElectricalZone {
    pub fn contains(&self, p: [f64; 3]) -> bool {
        p[2] >= self.z0 - 1e-6 && p[2] <= self.z1 + 1e-6 && inside(&self.outline, [p[0], p[1]])
    }

    pub fn mesh(&self) -> Mesh {
        extrude(&self.outline, &[], self.z0, self.z1)
    }
}

/// Point in polygon (even-odd).
fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// The distance in plan from a point to a polygon's boundary (0 inside).
pub fn plan_distance(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    if inside(poly, p) {
        return 0.0;
    }
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-18);
            let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
            (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
        })
        .fold(f64::MAX, f64::min)
}

/// The point of a polygon's boundary nearest `p` (itself when inside).
pub fn nearest_on(poly: &[[f64; 2]], p: [f64; 2]) -> [f64; 2] {
    if inside(poly, p) {
        return p;
    }
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-18)).clamp(0.0, 1.0);
            [a[0] + d[0] * t, a[1] + d[1] * t]
        })
        .min_by(|a, b| (a[0] - p[0]).hypot(a[1] - p[1]).total_cmp(&(b[0] - p[0]).hypot(b[1] - p[1])))
        .unwrap_or(p)
}

/// Whether a wall stands between a point and a fixture's outline in plan, at height `z`: the
/// zones BS 7671 701 and NEC 406.9(C) draw stop at walls.
pub fn wall_between(store: &Store, outline: &[[f64; 2]], p: [f64; 2], z: f64) -> bool {
    let q = nearest_on(outline, p);
    let d = [q[0] - p[0], q[1] - p[1]];
    let l = d[0].hypot(d[1]);
    if l < 0.02 {
        return false;
    }
    let u = [d[0] / l, d[1] / l];
    // From just off the point, so a device on a wall's face does not see its own wall.
    let start = [p[0] + u[0] * 0.01, p[1] + u[1] * 0.01];
    ray_to_walls(store, start, u, z, l - 0.02).is_some()
}

/// A rectangle in a fixture's frame, `grow` larger each way, in plan.
fn rect(f: &Fixture, x: [f64; 2], y: [f64; 2], grow: f64) -> Vec<[f64; 2]> {
    [[x[0] - grow, y[0] - grow], [x[1] + grow, y[0] - grow], [x[1] + grow, y[1] + grow], [x[0] - grow, y[1] + grow]]
        .iter()
        .map(|q| {
            let w = f.frame.apply([q[0], q[1], 0.0]);
            [w[0], w[1]]
        })
        .collect()
}

fn circle(c: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    (0..32).map(|k| {
        let a = std::f64::consts::TAU * f64::from(k) / 32.0;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    }).collect()
}

/// The zones round one bath or shower under a pack's rules.
pub fn electrical_zones_of(store: &Store, f: &Fixture, pack: &Pack) -> Vec<ElectricalZone> {
    let Some(e) = &pack.electrical else { return Vec::new() };
    if !matches!(f.kind, FixtureKind::Bath | FixtureKind::Shower) {
        return Vec::new();
    }
    let Some(el) = store.get(f.element) else { return Vec::new() };
    let Ok(g) = crate::mesh_objects::regenerate(store, el) else { return Vec::new() };
    let s = g.placement.scale;
    let (zmin, zmax) = g.mesh.positions.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(f64::from(q[2]) * s), b.max(f64::from(q[2]) * s)));
    let base = g.placement.origin[2];
    let (lo, hi) = (base + zmin, base + zmax);
    let top = f.floor + e.zone1_height.max(hi - f.floor);
    let zone = |zone: u8, outline: Vec<[f64; 2]>, z0: f64, z1: f64| ElectricalZone { element: f.element, zone, outline, z0, z1, pack: pack.id.clone() };
    // A shower with no tray: a wall-mounted head or a level former under 20 mm.
    let trayless = f.kind == FixtureKind::Shower && (lo > f.floor + 0.3 || hi - f.floor < 0.02);
    if trayless {
        let head = f.frame.apply([0.0, f.back, 0.0]);
        let r = e.trayless_radius;
        vec![
            zone(0, circle([head[0], head[1]], r), f.floor, f.floor + e.zone0_trayless_height.unwrap_or(0.1)),
            zone(1, circle([head[0], head[1]], r), f.floor, top),
        ]
    } else {
        let (x, y) = (f.x, [f.front, f.back]);
        vec![
            zone(0, rect(f, x, y, 0.0), f.floor, hi),
            zone(1, rect(f, x, y, 0.0), f.floor, top),
            zone(2, rect(f, x, y, e.zone2_width), f.floor, f.floor + e.zone1_height),
        ]
    }
}

/// Every electrical zone under the packs in use.
pub fn electrical_zones(store: &Store) -> Vec<ElectricalZone> {
    let packs: Vec<Pack> = active(store).into_iter().filter(|p| p.electrical.is_some()).collect();
    if packs.is_empty() {
        return Vec::new();
    }
    fixtures(store).iter().flat_map(|f| packs.iter().flat_map(|p| electrical_zones_of(store, f, p))).collect()
}

/// An IP code's water figure (IPX4 → 4), if it has one.
pub fn ip_water(code: &str) -> Option<u8> {
    let c = code.trim().to_ascii_uppercase();
    let rest = c.strip_prefix("IP")?;
    rest.chars().nth(1)?.to_digit(10).map(|d| d as u8)
}

/// A device's IP code: `Pset_ElectricalDeviceCommon.IP_Code` on it or its type.
pub fn device_ip(store: &Store, e: &crate::element::Element) -> Option<String> {
    let read = |x: &crate::element::Element| {
        let v = x.param("Pset_ElectricalDeviceCommon.IP_Code")?;
        v.as_str().map(str::to_owned).or_else(|| v.get("value")?.as_str().map(str::to_owned))
    };
    read(e).or_else(|| crate::mesh_objects::type_of(store, e).and_then(|(t, _)| read(store.get(t)?)))
}

// ---- Commands ----

fn project_id(txn: &Transaction<'_>) -> Result<ElementId> {
    project(txn.store()).map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))
}

/// A pack for listing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PackInfo {
    #[serde(flatten)]
    pub pack: Pack,
    pub in_use: bool,
    /// It ships with Reviera (and is not replaced by the project's own).
    pub shipped: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// The packs available (`bath.rule_packs`).
pub fn rule_packs(store: &Store) -> Vec<PackInfo> {
    let in_use: Vec<String> = active(store).into_iter().map(|p| p.id).collect();
    let own = project_packs(store);
    available(store)
        .into_iter()
        .map(|p| PackInfo { in_use: in_use.contains(&p.id), shipped: !own.iter().any(|o| o.id == p.id), pack: p })
        .collect()
}

/// `bath.rule_packs`.
pub(crate) fn list_rule_packs(store: &Store, _: Option<NoParams>) -> Result<Vec<PackInfo>> {
    Ok(rule_packs(store))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPacksParams {
    /// The packs to use, by id; none to choose them from the building code and accessibility
    /// rule set.
    pub packs: Option<Vec<String>>,
}

/// Chooses the packs in use (`bath.set_rule_packs`).
pub(crate) fn set_rule_packs(txn: &mut Transaction<'_>, p: SetPacksParams) -> Result<()> {
    let all = available(txn.store());
    if let Some(ids) = &p.packs {
        for id in ids {
            if !all.iter().any(|a| &a.id == id) {
                let known: Vec<&str> = all.iter().map(|a| a.id.as_str()).collect();
                return Err(bad("bath.no_pack", format!("no rule pack {id:?}; there are: {}", known.join(", "))));
            }
        }
    }
    let project = project_id(txn)?;
    txn.modify(project, |e| match &p.packs {
        Some(ids) => {
            e.params.insert(RULES_KEY.into(), serde_json::to_value(ids).expect("serialises"));
        }
        None => {
            e.params.remove(RULES_KEY);
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPackParams {
    /// A pack following `reviera.bath-rules-1`.
    pub pack: Value,
}

/// Adds a pack to the project, or replaces its pack (or a shipped one) of that id
/// (`bath.add_rule_pack`).
pub(crate) fn add_rule_pack(txn: &mut Transaction<'_>, p: AddPackParams) -> Result<String> {
    let pack = parse(&p.pack)?;
    let mut own = project_packs(txn.store());
    own.retain(|o| o.id != pack.id);
    let id = pack.id.clone();
    own.push(pack);
    let project = project_id(txn)?;
    let value = serde_json::to_value(&own).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(PACKS_KEY.into(), value);
    })?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePackParams {
    pub id: String,
}

/// Removes one of the project's own packs (`bath.remove_rule_pack`); a shipped pack it replaced
/// comes back.
pub(crate) fn remove_rule_pack(txn: &mut Transaction<'_>, p: RemovePackParams) -> Result<()> {
    let mut own = project_packs(txn.store());
    let before = own.len();
    own.retain(|o| o.id != p.id);
    if own.len() == before {
        return Err(bad("bath.no_pack", format!("the project has no pack {:?} of its own", p.id)));
    }
    let project = project_id(txn)?;
    txn.modify(project, |e| {
        if own.is_empty() {
            e.params.remove(PACKS_KEY);
        } else {
            e.params.insert(PACKS_KEY.into(), serde_json::to_value(&own).expect("serialises"));
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetHandParams {
    pub element_ids: Vec<ElementRef>,
    pub hand: Hand,
}

/// Fixes which side fixtures' side walls are on (`bath.set_hand`).
pub(crate) fn set_hand(txn: &mut Transaction<'_>, p: SetHandParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        if fixture(&store, store.get(id).expect("resolved")).is_none() {
            return Err(bad("bath.not_a_fixture", format!("element {} is not a bathroom fixture", id.0)));
        }
        txn.modify(id, |e| match p.hand {
            Hand::Auto => {
                e.params.remove(HAND_KEY);
            }
            h => {
                e.params.insert(HAND_KEY.into(), serde_json::to_value(h).expect("serialises"));
            }
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAccessibleParams {
    pub element_ids: Vec<ElementRef>,
    /// False: the accessibility packs' rules do not apply to them; true: they do.
    pub accessible: bool,
}

/// Marks fixtures as ones the accessibility packs apply to, or not (`bath.set_accessible`).
pub(crate) fn set_accessible(txn: &mut Transaction<'_>, p: SetAccessibleParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        // A door: one not marked accessible has no maneuvering clearances (a standard toilet
        // compartment's, ADA 604.8.1.2 asking them only of the wheelchair compartment's).
        if fixture(&store, e).is_none() && e.class != "IfcDoor" {
            return Err(bad("bath.not_a_fixture", format!("element {} is not a bathroom fixture or a door", id.0)));
        }
        txn.modify(id, |e| {
            if p.accessible {
                e.params.remove(ACCESSIBLE_KEY);
            } else {
                e.params.insert(ACCESSIBLE_KEY.into(), false.into());
            }
        })?;
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceBarsParams {
    /// The fixtures (default every bathroom fixture).
    pub element_ids: Option<Vec<ElementRef>>,
}

/// A bar not placed, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Skipped {
    pub fixture: ElementId,
    pub rule: String,
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Placed {
    pub placed: Vec<ElementId>,
    pub skipped: Vec<Skipped>,
}

/// Places the grab bars the packs in use ask for at fixtures, replacing those placed before for
/// the same fixture and rule (`bath.place_grab_bars`).
pub(crate) fn place_grab_bars(txn: &mut Transaction<'_>, p: PlaceBarsParams) -> Result<Placed> {
    let store = txn.store().clone();
    let only: Option<Vec<ElementId>> = p
        .element_ids
        .map(|refs| refs.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))).collect::<Result<Vec<_>>>())
        .transpose()?;
    let packs = active(&store);
    let mut out = Placed { placed: Vec::new(), skipped: Vec::new() };
    for f in fixtures(&store).iter().filter(|f| only.as_ref().is_none_or(|ids| ids.contains(&f.element))) {
        let guid = store.get(f.element).expect("listed").guid().to_string();
        for (_, bar) in bars_of(&store, f, &packs) {
            for (old, r) in crate::grab_bars::of_fixture(&store, f.element) {
                if r.rule.as_deref() == Some(bar.id.as_str()) && txn.store().get(old).is_some() {
                    txn.delete(old)?;
                }
            }
            match f.bar_stretch(&store, bar) {
                Ok(st) => {
                    let wall = store.get(st.wall).expect("found");
                    let d = bar.diameter_default();
                    let z = bar.top.default - d / 2.0;
                    let recipe = crate::grab_bars::GrabBarRecipe {
                        kind: crate::grab_bars::KIND.into(),
                        wall: wall.guid().to_string(),
                        side: st.side,
                        level: f.level.clone(),
                        points: vec![[st.along[0], z], [st.along[1], z]],
                        diameter: d,
                        gap: bar.gap_default(),
                        fixture: Some(guid.clone()),
                        rule: Some(bar.id.clone()),
                    };
                    out.placed.push(crate::grab_bars::insert(txn, recipe)?);
                }
                Err(reason) => out.skipped.push(Skipped { fixture: f.element, rule: bar.id.clone(), reason }),
            }
        }
    }
    Ok(out)
}

// ---- Shower falls (ARC-BATH-011) ----

/// A pack's falls for level-access showers, as gradients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Falls {
    pub id: String,
    pub description: String,
    pub min: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

/// Whether a shower has no raised tray: a wall-mounted head, or a level former under 20 mm.
pub fn level_access(store: &Store, f: &Fixture) -> bool {
    if f.kind != FixtureKind::Shower {
        return false;
    }
    let Some(e) = store.get(f.element) else { return false };
    let Ok(g) = crate::mesh_objects::regenerate(store, e) else { return false };
    let s = g.placement.scale;
    let (zmin, zmax) = g.mesh.positions.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(f64::from(q[2]) * s), b.max(f64::from(q[2]) * s)));
    let (lo, hi) = (g.placement.origin[2] + zmin, g.placement.origin[2] + zmax);
    lo > f.floor + 0.3 || hi - f.floor < 0.02
}

/// A level-access shower's wet area in plan: its former's footprint, or 900 × 900 mm in front of
/// a wall-mounted head.
pub fn shower_area(store: &Store, f: &Fixture) -> Vec<[f64; 2]> {
    let wall_head = store
        .get(f.element)
        .and_then(|e| crate::mesh_objects::regenerate(store, e).ok())
        .is_some_and(|g| g.mesh.positions.iter().map(|q| f64::from(q[2]) * g.placement.scale).fold(f64::MAX, f64::min) > 0.3);
    let (x, y) = if wall_head { ([-0.45, 0.45], [f.back - 0.9, f.back]) } else { (f.x, [f.front, f.back]) };
    [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]]
        .iter()
        .map(|q| {
            let w = f.frame.apply([q[0], q[1], 0.0]);
            [w[0], w[1]]
        })
        .collect()
}

/// The distance from a point to the line through an edge.
fn edge_distance(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / d[0].hypot(d[1]).max(1e-12)
}

/// The Reviera floor on a level whose outline holds a point.
pub fn floor_under(store: &Store, level: &str, p: [f64; 2]) -> Option<ElementId> {
    store.iter().filter(|e| crate::floors::floor_recipe(e).is_some_and(|r| r.level == level)).find_map(|e| {
        let g = crate::floors::regenerate(store, e).ok()?;
        g.profiles.first().filter(|pr| inside(&pr.outer, p)).map(|_| e.id())
    })
}

/// The least and greatest slope of a floor's top within an area (of the triangles whose middle
/// lies in it); none when the floor is not warped.
pub fn falls_in(store: &Store, floor: ElementId, area: &[[f64; 2]]) -> Option<(f64, f64)> {
    let e = store.get(floor)?;
    let shape = crate::shape_edit::shape_of(e)?;
    let g = crate::floors::regenerate(store, e).ok()?;
    let profile = g.profiles.first()?;
    let tris = crate::shape_edit::top_triangles(profile, &shape).ok()?;
    let mut range: Option<(f64, f64)> = None;
    for t in tris {
        let c = [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0];
        if !inside(area, c) {
            continue;
        }
        // The gradient of the plane through the three corners.
        let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        if n[2].abs() < 1e-12 {
            continue;
        }
        let slope = (n[0] * n[0] + n[1] * n[1]).sqrt() / n[2].abs();
        range = Some(range.map_or((slope, slope), |(a, b)| (a.min(slope), b.max(slope))));
    }
    range
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DrainAt {
    /// A point drain in the middle of the shower area.
    #[default]
    Centre,
    /// A linear drain along the back wall.
    Linear,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowerFallsParams {
    pub element_ids: Vec<ElementRef>,
    /// As a gradient (0.02 is 2 %); default the pack's.
    pub fall: Option<f64>,
    #[serde(default)]
    pub drain: DrainAt,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShowerFallsOut {
    pub drains: Vec<ElementId>,
    pub floors: Vec<ElementId>,
}

/// On a drain `bath.shower_falls` placed: the floor it sits in and the shower it serves
/// (GlobalIds), which it may overlap.
pub const DRAIN_KEY: &str = "rxd:bath_drain";

/// Lays the falls of level-access showers to new drains (`bath.shower_falls`).
pub(crate) fn shower_falls(txn: &mut Transaction<'_>, p: ShowerFallsParams) -> Result<ShowerFallsOut> {
    let store = txn.store().clone();
    let packs = active(&store);
    let fall = p
        .fall
        .or_else(|| packs.iter().filter_map(|x| x.falls.as_ref()).filter_map(|f| f.default.or(Some(f.min))).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v)))))
        .unwrap_or(0.02);
    let steepest = packs.iter().filter_map(|x| x.falls.as_ref()?.max).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.min(v))));
    if !(0.001..=0.2).contains(&fall) {
        return Err(bad("bath.invalid_input", "a fall is a gradient between 0.001 and 0.2 (2 % is 0.02)"));
    }
    let mut out = ShowerFallsOut { drains: Vec::new(), floors: Vec::new() };
    let drain_type = crate::mep::load_starters(txn, crate::mep::LoadStartersParams { names: vec!["Floor Drain".into()] })?["Floor Drain"];
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        let f = store.get(id).and_then(|e| fixture(&store, e)).ok_or_else(|| bad("bath.not_a_fixture", format!("element {} is not a bathroom fixture", id.0)))?;
        if !level_access(&store, &f) {
            return Err(bad("bath.not_level_access", format!("element {} is not a level-access shower (it has a raised tray)", id.0)));
        }
        let area = shower_area(&store, &f);
        let n = area.len() as f64;
        let centre = [area.iter().map(|q| q[0]).sum::<f64>() / n, area.iter().map(|q| q[1]).sum::<f64>() / n];
        let floor = floor_under(&store, &f.level, centre).ok_or_else(|| bad("bath.no_floor", format!("no Reviera floor under shower {}", id.0)))?;
        let level_ref = ElementRef::Guid(crate::id::GlobalId::parse(&f.level).ok_or_else(|| bad("bath.no_level", "the shower's level is gone"))?);
        let serves = store.get(f.element).map(|e| e.guid().to_string());
        let drain = lay_falls(txn, floor, &area, p.drain, fall, if p.fall.is_none() { steepest } else { None }, level_ref, drain_type, serves)?;
        out.drains.push(drain);
        if !out.floors.contains(&floor) {
            out.floors.push(floor);
        }
    }
    Ok(out)
}

/// Lays falls over an area of a floor to a new drain: the area's edge stays where it is and
/// the drain (at its centre, or along its back for a linear one: the area is front-left,
/// front-right, back-right, back-left) sinks by the fall over the longest run to it, no deeper
/// than `steepest` allows over the shortest. Earlier drains and falls within the area go first.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_falls(txn: &mut Transaction<'_>, floor: ElementId, area: &[[f64; 2]], drain_kind: DrainAt, fall: f64, steepest: Option<f64>, level_ref: ElementRef, drain_type: ElementId, serves: Option<String>) -> Result<ElementId> {
    let store = txn.store().clone();
    // The drain: the middle, or along the back (the area is front-left, front-right,
    // back-right, back-left) for a linear drain.
    let deep = (area[3][0] - area[0][0]).hypot(area[3][1] - area[0][1]);
    let drain_at = match drain_kind {
        DrainAt::Centre => [area.iter().map(|q| q[0]).sum::<f64>() / area.len() as f64, area.iter().map(|q| q[1]).sum::<f64>() / area.len() as f64],
        DrainAt::Linear => {
            // 100 mm in, clear of the wall behind.
            let t = 0.1 / deep.max(0.2);
            let b = [(area[2][0] + area[3][0]) / 2.0, (area[2][1] + area[3][1]) / 2.0];
            let fr = [(area[0][0] + area[1][0]) / 2.0, (area[0][1] + area[1][1]) / 2.0];
            [b[0] + (fr[0] - b[0]) * t, b[1] + (fr[1] - b[1]) * t]
        }
    };
    // How deep the drain sits: the fall over the longest run to it, square to an edge of
    // the area, so every face falls at least that much; by default no steeper than the
    // pack's steepest over the shortest run.
    let runs: Vec<f64> = match drain_kind {
        DrainAt::Centre => (0..area.len()).map(|i| edge_distance(area[i], area[(i + 1) % area.len()], drain_at)).collect(),
        DrainAt::Linear => vec![deep],
    };
    let (shortest, longest) = runs.iter().fold((f64::MAX, 0.0f64), |(a, b), r| (a.min(*r), b.max(*r)));
    let mut depth = fall * longest;
    if let Some(max) = steepest {
        depth = depth.min(max * shortest);
    }
    // An earlier run's drains and falls go first.
    for e in store.iter().filter(|e| e.class == "IfcWasteTerminal") {
        let floor_trap = crate::mesh_objects::type_of(&store, e).is_some_and(|(_, t)| t.predefined_type() == "FLOORTRAP");
        let at = crate::mesh_objects::mesh_object_recipe(e).map(|r| r.point);
        if floor_trap && at.is_some_and(|o| inside(&area, o)) {
            txn.delete(e.id())?;
        }
    }
    crate::shape_edit::clear_within(txn, floor, &area)?;
    // The drain's top flush with the fallen floor.
    let drain_height = txn.store().get(drain_type).and_then(mesh_objects::mesh_object_type).map_or(0.08, |t| t.size[2] * t.scale);
    let drain = crate::mep_controls::place_fixture(
        txn,
        crate::mep_controls::PlaceParams {
            type_id: ElementRef::Id(drain_type),
            point: drain_at,
            level: level_ref,
            offset: Some(-depth - drain_height),
            rotation: 0.0,
            host_wall: None,
            side: None,
            work_plane: true,
            controlled_ids: Vec::new(),
        },
    )?;
    let floor_guid = store.get(floor).map(|e| e.guid().to_string());
    txn.modify(drain, |e| {
        e.params.insert(DRAIN_KEY.into(), serde_json::json!({ "floor": floor_guid, "shower": serves }));
    })?;
    match drain_kind {
        // The area's edge stays where it is; the drain point drops.
        DrainAt::Centre => {
            let mut ring = area.to_vec();
            ring.push(area[0]);
            let n = ring.len();
            crate::shape_edit::add_offsets(txn, floor, ring, &vec![0.0; n])?;
            crate::shape_edit::add_offsets(txn, floor, vec![drain_at], &[-depth])?;
        }
        // The front and sides fall to a line along the back.
        DrainAt::Linear => {
            let edge = vec![area[3], area[0], area[1], area[2]];
            crate::shape_edit::add_offsets(txn, floor, edge, &[-depth, 0.0, 0.0, -depth])?;
            crate::shape_edit::add_offsets(txn, floor, vec![area[3], area[2]], &[-depth, -depth])?;
        }
    }
    Ok(drain)
}

// ---- Waterproofing extents (ARC-BATH-012) ----

/// Where walls and floors round some fixtures are waterproof or nonabsorbent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Waterproofing {
    pub id: String,
    pub description: String,
    pub fixtures: Vec<FixtureKind>,
    /// Only for level-access showers (no raised tray).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub level_access_only: bool,
    /// Up the walls bounding the fixture, from its floor.
    pub wall_height: f64,
    /// How far the wall strip runs past the fixture's sides.
    #[serde(default)]
    pub wall_beyond: f64,
    /// The floor: the fixture's footprint, grown in front and to each side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    #[serde(default)]
    pub floor_front: f64,
    #[serde(default)]
    pub floor_sides: f64,
    /// The whole floor of the room it stands in (a wet room).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole_room: bool,
    /// A water-exposure class (DIN 18534 W0-I to W3-I), as data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Wall,
    Floor,
}

/// One waterproofed area: a thin volume on a wall face or the floor.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Extent {
    /// The fixture it is for.
    pub element: ElementId,
    pub fixture: FixtureKind,
    pub surface: Surface,
    /// The wall or floor it is on, when one was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<ElementId>,
    /// Along the wall (or across the floor) and up it (or deep), in metres; the area in m².
    pub width: f64,
    pub height: f64,
    pub area: f64,
    pub pack: String,
    pub rule: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip)]
    pub outline: Vec<[f64; 2]>,
    #[serde(skip)]
    pub z: [f64; 2],
}

impl Extent {
    pub fn mesh(&self) -> Mesh {
        extrude(&self.outline, &[], self.z[0], self.z[1])
    }
}

/// How thick an extent's volume is drawn.
const SKIN: f64 = 0.005;
/// How far a wall face may stand from a fixture's edge and still bound it.
const FACE_GAP: f64 = 0.05;

fn polygon_area(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1]).sum::<f64>().abs() / 2.0
}

/// The extents round one fixture under a pack's rules.
pub fn waterproofing_of(store: &Store, f: &Fixture, pack: &Pack) -> Vec<Extent> {
    let mut out = Vec::new();
    let world = |q: [f64; 2]| {
        let w = f.frame.apply([q[0], q[1], 0.0]);
        [w[0], w[1]]
    };
    let o = world([0.0, 0.0]);
    let dir = |q: [f64; 2]| {
        let w = world(q);
        [w[0] - o[0], w[1] - o[1]]
    };
    let (x, y) = (f.x, [f.front, f.back]);
    let mid = [(x[0] + x[1]) / 2.0, (y[0] + y[1]) / 2.0];
    let c = world(mid);
    let z = f.floor + 0.6;
    for rule in pack.waterproofing.iter().filter(|r| r.fixtures.contains(&f.kind)) {
        if rule.level_access_only && !level_access(store, f) {
            continue;
        }
        let extent = |surface, host, width: f64, height: f64, area: f64, outline: Vec<[f64; 2]>, z: [f64; 2]| Extent {
            element: f.element,
            fixture: f.kind,
            surface,
            host,
            width,
            height,
            area,
            pack: pack.id.clone(),
            rule: rule.id.clone(),
            class: rule.class.clone(),
            outline,
            z,
        };
        // The walls along its back and sides: each edge's direction out, its half-depth from
        // the middle, and the edge's two ends.
        let edges = [
            ([0.0, 1.0], (y[1] - y[0]) / 2.0, [x[0], y[1]], [x[1], y[1]]),
            ([-1.0, 0.0], (x[1] - x[0]) / 2.0, [x[0], y[0]], [x[0], y[1]]),
            ([1.0, 0.0], (x[1] - x[0]) / 2.0, [x[1], y[0]], [x[1], y[1]]),
        ];
        for (d, half, a, b) in edges {
            let dw = dir(d);
            let Some((t, wall)) = ray_to_walls(store, c, dw, z, half + FACE_GAP) else { continue };
            if t < half - FACE_GAP {
                continue;
            }
            // The strip on the face, `wall_beyond` past each end.
            let along = [b[0] - a[0], b[1] - a[1]];
            let len = along[0].hypot(along[1]);
            let u = [along[0] / len, along[1] / len];
            let shift = t - half;
            let end = |q: [f64; 2], s: f64, back: f64| world([q[0] + u[0] * s + d[0] * (shift - back), q[1] + u[1] * s + d[1] * (shift - back)]);
            let wb = rule.wall_beyond;
            let outline = vec![end(a, -wb, 0.0), end(b, wb, 0.0), end(b, wb, SKIN), end(a, -wb, SKIN)];
            let width = len + 2.0 * wb;
            out.push(extent(Surface::Wall, Some(wall), width, rule.wall_height, width * rule.wall_height, outline, [f.floor, f.floor + rule.wall_height]));
        }
        // The floor: the room's, or the fixture's grown.
        let room = rule.whole_room.then(|| {
            let hit = |d: [f64; 2]| ray_to_walls(store, c, dir(d), z, 30.0).map(|h| h.0);
            Some((hit([-1.0, 0.0])?, hit([1.0, 0.0])?, hit([0.0, -1.0])?, hit([0.0, 1.0])?))
        });
        let rect = match room {
            Some(Some((l, r, fr, bk))) => Some(([mid[0] - l, mid[0] + r], [mid[1] - fr, mid[1] + bk])),
            _ if rule.floor || rule.whole_room => Some(([x[0] - rule.floor_sides, x[1] + rule.floor_sides], [y[0] - rule.floor_front, y[1]])),
            _ => None,
        };
        if let Some((rx, ry)) = rect {
            let outline: Vec<[f64; 2]> = [[rx[0], ry[0]], [rx[1], ry[0]], [rx[1], ry[1]], [rx[0], ry[1]]].into_iter().map(world).collect();
            let host = floor_under(store, &f.level, c);
            let (w, h) = (rx[1] - rx[0], ry[1] - ry[0]);
            out.push(extent(Surface::Floor, host, w, h, polygon_area(&outline), outline, [f.floor, f.floor + SKIN]));
        }
    }
    out
}

/// Every extent under the packs in use.
pub fn waterproofing(store: &Store) -> Vec<Extent> {
    let packs: Vec<Pack> = active(store).into_iter().filter(|p| !p.waterproofing.is_empty()).collect();
    if packs.is_empty() {
        return Vec::new();
    }
    fixtures(store).iter().flat_map(|f| packs.iter().flat_map(|p| waterproofing_of(store, f, p))).collect()
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaterproofingParams {
    /// These fixtures only (default all).
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WaterproofingSchedule {
    pub extents: Vec<Extent>,
    /// m².
    pub wall_area: f64,
    pub floor_area: f64,
}

/// The waterproofing schedule (`bath.waterproofing`).
pub(crate) fn waterproofing_schedule(store: &Store, p: WaterproofingParams) -> Result<WaterproofingSchedule> {
    let only = p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))).collect::<Result<Vec<_>>>()?;
    let extents: Vec<Extent> = waterproofing(store).into_iter().filter(|e| only.is_empty() || only.contains(&e.element)).collect();
    let sum = |s: Surface| extents.iter().filter(|e| e.surface == s).map(|e| e.area).sum();
    Ok(WaterproofingSchedule { wall_area: sum(Surface::Wall), floor_area: sum(Surface::Floor), extents })
}

// ---- Hot tubs and spas (ARC-BATH-013) ----

/// A pack's rules for hot tubs and spas: electrical distances and the floor load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaRules {
    pub id: String,
    pub description: String,
    /// In plan from the tub's outline: receptacles and wall switches at least this far.
    pub receptacle_distance: f64,
    pub switch_distance: f64,
    /// Luminaires and fans over the tub (within `receptacle_distance` of it) at least this far
    /// above the water.
    pub luminaire_height: f64,
    /// The water this far below the rim.
    #[serde(default = "default_water_below_rim")]
    pub water_below_rim: f64,
    /// For the floor load: each seat's occupant (kg), and the live load (kPa) to compare with
    /// on a suspended floor.
    pub occupant_mass: f64,
    pub floor_live_load: f64,
    /// The pad asked for on the lowest level (m of reinforced concrete).
    pub pad_thickness: f64,
    pub source: String,
    pub confidence: Confidence,
}

fn default_water_below_rim() -> f64 {
    0.1
}

/// The type properties holding a hot tub's filled weight (kg) and seats.
pub const SPA_WEIGHT: &str = "SpaProperties.FilledWeight";
pub const SPA_SEATS: &str = "SpaProperties.Seats";

/// A spa's filled weight and seats: on the copy, else its type.
pub fn spa_weight(store: &Store, e: &crate::element::Element) -> Option<(f64, f64)> {
    let num = |x: &crate::element::Element, k: &str| {
        let v = x.param(k)?;
        v.as_f64().or_else(|| v.get("value")?.as_f64()).or_else(|| v.as_str()?.trim().parse().ok())
    };
    let ty = mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t));
    let get = |k: &str| num(e, k).or_else(|| ty.and_then(|t| num(t, k)));
    Some((get(SPA_WEIGHT)?, get(SPA_SEATS).unwrap_or(0.0)))
}

/// A fixture's plan outline (its extents in its frame).
pub fn outline(f: &Fixture) -> Vec<[f64; 2]> {
    [[f.x[0], f.front], [f.x[1], f.front], [f.x[1], f.back], [f.x[0], f.back]]
        .iter()
        .map(|q| {
            let w = f.frame.apply([q[0], q[1], 0.0]);
            [w[0], w[1]]
        })
        .collect()
}

// ---- Saunas (ARC-BATH-014) ----

/// A pack's sauna rules: heater sizing, room height, the door and the thermostat's sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SaunaRules {
    pub id: String,
    pub description: String,
    /// Heaters by power: (kW, least and most room volume in m³), smallest first.
    pub heaters: Vec<[f64; 3]>,
    /// m³ added to the design volume for each m² of uninsulated surface (glass, tile, brick).
    pub uninsulated_volume: f64,
    /// The design volume is multiplied by this for log walls.
    pub log_factor: f64,
    /// The room's clear height.
    pub ceiling_min: f64,
    pub ceiling_max: f64,
    /// From the upper bench to the ceiling.
    pub upper_bench_headroom: f64,
    /// A non-integral thermostat sensor within this of the ceiling (IRC M1902).
    pub sensor_below_ceiling: f64,
    pub source: String,
    pub confidence: Confidence,
}

/// The sizing of a sauna's heater.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HeaterSizing {
    /// m³: the room, and the design volume with uninsulated surfaces and log walls counted.
    pub volume: f64,
    pub design_volume: f64,
    /// The smallest heater in the table for the design volume, and the volumes it suits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kw: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
}

pub fn size_heater(r: &SaunaRules, volume: f64, uninsulated: f64, log: bool) -> HeaterSizing {
    let design_volume = (volume + uninsulated * r.uninsulated_volume) * if log { r.log_factor } else { 1.0 };
    let pick = r.heaters.iter().find(|h| design_volume >= h[1] - 1e-9 && design_volume <= h[2] + 1e-9);
    HeaterSizing { volume, design_volume, kw: pick.map(|h| h[0]), range: pick.map(|h| [h[1], h[2]]) }
}

/// The sauna rules in use, else the shipped ones.
pub fn sauna_rules(store: &Store) -> SaunaRules {
    active(store).into_iter().find_map(|p| p.sauna).or_else(|| shipped().iter().find_map(|p| p.sauna.clone())).expect("a shipped pack has sauna rules")
}

/// A heater's properties: on the copy, else its type.
pub const HEATER_POWER: &str = "SaunaHeater.Power";
pub const HEATER_SIDE: &str = "SaunaHeater.SideClearance";
pub const HEATER_FRONT: &str = "SaunaHeater.FrontClearance";
pub const HEATER_CEILING: &str = "SaunaHeater.CeilingClearance";
/// What the Sauna tool sized the room with: `{uninsulated, log}`.
pub const SAUNA_KEY: &str = "rxd:sauna";

pub fn product_number(store: &Store, e: &crate::element::Element, key: &str) -> Option<f64> {
    let num = |x: &crate::element::Element| {
        let v = x.param(key)?;
        v.as_f64().or_else(|| v.get("value")?.as_f64()).or_else(|| v.as_str()?.trim().parse().ok())
    };
    num(e).or_else(|| mesh_objects::type_of(store, e).and_then(|(t, _)| num(store.get(t)?)))
}

/// The room round a fixture standing against a wall: rays along its frame from 0.5 m in front
/// of it, at 0.6 m up.
#[derive(Debug, Clone)]
pub struct RoomAround {
    pub outline: Vec<[f64; 2]>,
    pub area: f64,
    /// The walls' lowest top above the fixture's floor.
    pub height: f64,
    /// The walls found, toward the fixture's left, right, front and back.
    pub walls: Vec<ElementId>,
}

pub fn room_around(store: &Store, f: &Fixture) -> Option<RoomAround> {
    let local = |q: [f64; 2]| {
        let w = f.frame.apply([q[0], q[1], 0.0]);
        [w[0], w[1]]
    };
    let o = local([0.0, 0.0]);
    let dir = |q: [f64; 2]| {
        let w = local(q);
        [w[0] - o[0], w[1] - o[1]]
    };
    let c = local([(f.x[0] + f.x[1]) / 2.0, f.front - 0.5]);
    let z = f.floor + 0.6;
    let mut hits = Vec::new();
    for d in [[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]] {
        hits.push(ray_to_walls(store, c, dir(d), z, 30.0)?);
    }
    let [l, r, fr, bk] = [hits[0].0, hits[1].0, hits[2].0, hits[3].0];
    let mid = [(f.x[0] + f.x[1]) / 2.0, f.front - 0.5];
    let outline: Vec<[f64; 2]> = [[mid[0] - l, mid[1] - fr], [mid[0] + r, mid[1] - fr], [mid[0] + r, mid[1] + bk], [mid[0] - l, mid[1] + bk]].into_iter().map(local).collect();
    let height = hits
        .iter()
        .filter_map(|(_, w)| crate::walls::regenerate(store, store.get(*w)?).ok())
        .map(|g| g.base_z + g.height - f.floor)
        .fold(f64::MAX, f64::min);
    Some(RoomAround { outline, area: (l + r) * (fr + bk), height, walls: hits.iter().map(|h| h.1).collect() })
}

/// The pack standing for a product's own rules (its installation manual).
pub fn product_pack() -> Pack {
    Pack {
        format: "reviera.bath-rules-1".into(),
        id: "product".into(),
        title: "The product's installation manual".into(),
        jurisdiction: String::new(),
        edition: "the product's data".into(),
        layer: Layer::Minimum,
        notes: None,
        zones: Vec::new(),
        distances: Vec::new(),
        bars: Vec::new(),
        counts: Vec::new(),
        electrical: None,
        falls: None,
        waterproofing: Vec::new(),
        spa: None,
        sauna: None,
        steam: None,
        spa_access: None,
        whirlpool: None,
        receptacles: None,
    }
}

/// A sauna heater's safety distances, from the product, as a zone kept clear.
fn heater_space(store: &Store, f: &Fixture) -> Option<Space> {
    if f.kind != FixtureKind::SaunaHeater {
        return None;
    }
    let e = store.get(f.element)?;
    let side = product_number(store, e, HEATER_SIDE)?;
    let front = product_number(store, e, HEATER_FRONT).unwrap_or(side);
    let g = mesh_objects::regenerate(store, e).ok()?;
    let top = g.placement.origin[2] + g.mesh.positions.iter().map(|q| f64::from(q[2]) * g.placement.scale).fold(f64::MIN, f64::max);
    let zone = Zone {
        id: "Heater safety distances".into(),
        description: format!("The heater's own distances to combustible material: {:.0} mm to the sides and {:.0} mm in front", side * 1000.0, front * 1000.0),
        fixtures: vec![FixtureKind::SaunaHeater],
        occupancy: RuleOccupancy::default(),
        source: String::new(),
        confidence: Confidence::Verified,
        across: [(AcrossFrom::NearEdge, -side), (AcrossFrom::FarEdge, side)],
        out: [(OutFrom::Wall, 0.0), (OutFrom::Front, front)],
        height: Some((top - f.floor).max(0.1)),
        door_swing: None,
    };
    Some(Space {
        clearance: Clearance { element: f.element, kind: ClearanceKind::FixtureSpace, rule: zone.id.clone(), mesh: f.zone_mesh(&zone, 0.0), door_swing_forbidden: false },
        pack: "product".into(),
        zone,
    })
}

// ---- Steam rooms (ARC-BATH-015) ----

/// A pack's steam-room rules: the ceiling's slope and height, the floor's fall, the steam head's
/// distance from seats, and one maker's factors for the generator's design volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SteamRules {
    pub id: String,
    pub description: String,
    /// The ceiling's least slope (a gradient: 2 in per ft is 0.1667).
    pub ceiling_slope: f64,
    /// The ceiling's height: the ideal (its high side by default) and the most.
    pub ceiling_ideal: f64,
    pub ceiling_max: f64,
    /// The floor's fall to its drain.
    pub floor_fall: f64,
    /// A steam head at least this far (in plan) from a bench.
    pub head_clearance: f64,
    /// The generator's design volume: the room's volume times the walls' material factor,
    /// raised by `height_step` for each `height_unit` above `height_base`, and by
    /// `exterior_wall` for an outside wall.
    pub materials: BTreeMap<String, f64>,
    pub height_base: f64,
    pub height_unit: f64,
    pub height_step: f64,
    pub exterior_wall: f64,
    pub source: String,
    pub confidence: Confidence,
}

/// The steam rules in use, else the shipped ones.
pub fn steam_rules(store: &Store) -> SteamRules {
    active(store).into_iter().find_map(|p| p.steam).or_else(|| shipped().iter().find_map(|p| p.steam.clone())).expect("a shipped pack has steam rules")
}

/// A steam generator's design volume (m³), by one maker's factors.
pub fn steam_design_volume(r: &SteamRules, volume: f64, high: f64, material: &str, exterior: bool) -> Result<f64> {
    let m = r.materials.get(material).copied().ok_or_else(|| bad("bath.invalid_input", format!("no factor for walls of {material:?}; there are: {}", r.materials.keys().cloned().collect::<Vec<_>>().join(", "))))?;
    let steps = ((high - r.height_base) / r.height_unit).max(0.0);
    Ok(volume * m * (1.0 + r.height_step * steps) * if exterior { 1.0 + r.exterior_wall } else { 1.0 })
}

/// The ceiling (a Reviera ceiling) over a plan point on a level, and its slope there: the
/// steepest of its underside's facets over the point's room, 0 when it is flat.
pub fn ceiling_over(store: &Store, level: &str, p: [f64; 2]) -> Option<(ElementId, f64)> {
    store.iter().find_map(|e| {
        let (kind, r) = crate::floors::slab_recipe(e)?;
        if kind != crate::floors::SlabKind::Ceiling || r.level != level {
            return None;
        }
        let g = crate::floors::regenerate(store, e).ok()?;
        let profile = g.profiles.first().filter(|pr| inside(&pr.outer, p))?;
        let slope = crate::shape_edit::shape_of(e)
            .and_then(|s| crate::shape_edit::top_triangles(profile, &s).ok())
            .map_or(0.0, |tris| {
                tris.iter()
                    .filter_map(|t| {
                        let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                        (n[2].abs() > 1e-12).then(|| (n[0] * n[0] + n[1] * n[1]).sqrt() / n[2].abs())
                    })
                    .fold(f64::MAX, f64::min)
            });
        Some((e.id(), if slope == f64::MAX { 0.0 } else { slope }))
    })
}

// ---- Spa accessibility (ARC-BATH-016) ----

/// An accessibility pack's rules for saunas, steam rooms and spas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaAccess {
    pub id: String,
    pub description: String,
    /// A sauna's or steam room's turning space (metres).
    pub turning_diameter: f64,
    /// An accessible bench: its top's least and most height, its depth's, and its least length.
    pub bench_height: [f64; 2],
    pub bench_depth: [f64; 2],
    pub bench_length: f64,
    /// The ObjectTypes that are a spa's means of entry, and how near its outline they stand.
    pub entries: Vec<String>,
    pub entry_reach: f64,
    /// A transfer wall's height, depth and least length.
    pub transfer_wall_height: [f64; 2],
    pub transfer_wall_depth: [f64; 2],
    pub transfer_wall_length: f64,
    pub source: String,
    pub confidence: Confidence,
}

/// A copy's plan box and top: in its own frame's axes, its length along local x and depth along
/// y (metres), its outline, and its top's height above `floor`.
#[derive(Debug, Clone)]
pub struct Piece {
    pub element: ElementId,
    pub outline: Vec<[f64; 2]>,
    pub length: f64,
    pub depth: f64,
    pub top: f64,
    /// Its top at its two ends: a transfer wall's, below the grab bars across it.
    pub end_top: f64,
}

pub fn piece(store: &Store, e: &crate::element::Element, floor: f64) -> Option<Piece> {
    let g = mesh_objects::regenerate(store, e).ok()?;
    let s = g.placement.scale;
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &g.mesh.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(f64::from(q[k]) * s);
            hi[k] = hi[k].max(f64::from(q[k]) * s);
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let end_top = g.mesh.positions.iter().filter(|q| (f64::from(q[0]) * s - lo[0]).abs() < 1e-3 || (f64::from(q[0]) * s - hi[0]).abs() < 1e-3).map(|q| f64::from(q[2]) * s).fold(f64::MIN, f64::max);
    let frame = Placement { scale: 1.0, ..g.placement };
    let outline = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]]
        .iter()
        .map(|q| {
            let w = frame.apply([q[0], q[1], 0.0]);
            [w[0], w[1]]
        })
        .collect();
    Some(Piece { element: e.id(), outline, length: hi[0] - lo[0], depth: hi[1] - lo[1], top: g.placement.origin[2] + hi[2] - floor, end_top: g.placement.origin[2] + end_top - floor })
}

// ---- US bathroom receptacles (ARC-BATH-021) ----

/// A pack's US-style receptacle rules: none near a tub or shower, one near each basin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Receptacles {
    pub id: String,
    pub description: String,
    /// No receptacle within this of a tub's or shower's outside edge in plan, up to
    /// `tub_zone_height` above its rim or threshold.
    pub tub_zone: f64,
    pub tub_zone_height: f64,
    /// In a dwelling, a receptacle within this of each basin's outside edge.
    pub basin_reach: f64,
    pub source: String,
    pub confidence: Confidence,
}

// ---- Whirlpool baths (ARC-BATH-017) ----

/// A pack's rule for a whirlpool bath's pump access.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Whirlpool {
    pub id: String,
    pub description: String,
    /// The access opening's least width and height, in the bath's apron at the pump.
    pub opening: f64,
    /// Kept clear in front of the opening, to take the panel off and reach the pump.
    pub clear_depth: f64,
    pub source: String,
    pub confidence: Confidence,
}

/// A whirlpool bath's pump: along its length from its middle (metres), a property of its type
/// or product; a bath without it has no pump.
pub const PUMP_POSITION: &str = "WhirlpoolBath.PumpPosition";

/// The space kept clear in front of a whirlpool bath's pump access, under the packs' rules.
fn pump_access_space(store: &Store, f: &Fixture, packs: &[Pack]) -> Option<Space> {
    if f.kind != FixtureKind::Bath {
        return None;
    }
    let e = store.get(f.element)?;
    let pump = product_number(store, e, PUMP_POSITION)?;
    let (pack, r) = packs.iter().find_map(|p| p.whirlpool.as_ref().map(|w| (p, w)))?;
    let half = r.opening / 2.0;
    let (x0, x1) = ((pump - half).max(f.x[0]), (pump + half).min(f.x[1]));
    let outline: Vec<[f64; 2]> = [[x0, f.front - r.clear_depth], [x1, f.front - r.clear_depth], [x1, f.front], [x0, f.front]]
        .iter()
        .map(|q| {
            let w = f.frame.apply([q[0], q[1], 0.0]);
            [w[0], w[1]]
        })
        .collect();
    let zone = Zone {
        id: r.id.clone(),
        description: r.description.clone(),
        fixtures: vec![FixtureKind::Bath],
        occupancy: RuleOccupancy::default(),
        source: r.source.clone(),
        confidence: r.confidence,
        across: [(AcrossFrom::Centre, x0), (AcrossFrom::Centre, x1)],
        out: [(OutFrom::Front, 0.0), (OutFrom::Front, r.clear_depth)],
        height: Some(r.opening),
        door_swing: None,
    };
    Some(Space {
        clearance: Clearance { element: f.element, kind: ClearanceKind::FixtureSpace, rule: zone.id.clone(), mesh: extrude(&outline, &[], f.floor + 0.005, f.floor + r.opening), door_swing_forbidden: false },
        pack: pack.id.clone(),
        zone,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_packs_are_valid_and_cite_sources() {
        let register = include_str!("../../../design_documents/SOURCE_REGISTER.md");
        assert_eq!(shipped().len(), 8);
        for p in shipped() {
            let sources = p.zones.iter().map(|z| &z.source).chain(p.distances.iter().map(|d| &d.source)).chain(p.bars.iter().map(|b| &b.source)).chain(p.counts.iter().map(|c| &c.source)).chain(p.electrical.iter().map(|e| &e.source));
            for s in sources {
                for id in s.split(' ') {
                    assert!(register.contains(&format!("| {id} |")), "{}: {id} is not in the source register", p.id);
                }
            }
        }
    }

    #[test]
    fn a_pack_breaking_the_schema_is_refused() {
        let mut v: Value = serde_json::from_str(SHIPPED[0]).unwrap();
        v["zones"][0]["across"][0][0] = "middle".into();
        assert_eq!(parse(&v).unwrap_err().code, "bath.pack_schema");
        let mut v: Value = serde_json::from_str(SHIPPED[2]).unwrap();
        v["distances"][0]["min"] = 0.6.into();
        assert_eq!(parse(&v).unwrap_err().code, "bath.pack_range");
    }

    #[test]
    fn a_ray_meets_a_segment_ahead_only() {
        assert_eq!(ray_segment([0.0, 0.0], [1.0, 0.0], [2.0, -1.0], [2.0, 1.0]), Some(2.0));
        assert_eq!(ray_segment([0.0, 0.0], [-1.0, 0.0], [2.0, -1.0], [2.0, 1.0]), None);
        assert_eq!(ray_segment([0.0, 0.0], [1.0, 0.0], [2.0, 1.0], [2.0, 3.0]), None);
    }

    #[test]
    fn counts_by_ratio_bands_and_step_tables() {
        let b = Requirement { basis: Basis::EachSex, ratio: Some(vec![Band { to: Some(50.0), per: 25.0 }, Band { to: None, per: 50.0 }]), table: None, then_per: None, minimum: None };
        assert_eq!((b.count(180.0), b.count(50.0), b.count(51.0), b.count(0.0)), (5, 2, 3, 0));
        let t = Requirement { basis: Basis::Total, ratio: None, table: Some(vec![(5.0, 1), (25.0, 2), (50.0, 3), (75.0, 4), (100.0, 5)]), then_per: Some(25.0), minimum: None };
        assert_eq!((t.count(60.0), t.count(100.0), t.count(101.0), t.count(130.0)), (4, 5, 6, 7));
    }
}

