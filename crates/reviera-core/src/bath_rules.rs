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

/// The published pack schema (MIT).
pub const SCHEMA: &str = include_str!("../../../schemas/bath/reviera.bath-rules-1.schema.json");
const SHIPPED: &[&str] = &[
    include_str!("../../../schemas/bath/packs/us_plumbing.json"),
    include_str!("../../../schemas/bath/packs/us_nkba.json"),
    include_str!("../../../schemas/bath/packs/ada_2010.json"),
    include_str!("../../../schemas/bath/packs/england_adm.json"),
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
}

impl FixtureKind {
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
    fn applies(self, o: crate::codes::Occupancy) -> bool {
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
        Code::Ibc | Code::Irc => out.push("us_plumbing".to_owned()),
        Code::UkAdk => out.push("england_adm".to_owned()),
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
    /// The distance from its centreline to the side wall, when one was found.
    pub side_distance: Option<f64>,
    pub hand: Hand,
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
        // Out is −y; nothing behind the wall face.
        let y = match from {
            OutFrom::Wall => self.back - d,
            OutFrom::Front => self.front - d,
        };
        y.min(self.back)
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

/// Where a ray from `p` along `d` first meets a wall's plan outline within `reach`.
fn ray_to_walls(store: &Store, p: [f64; 2], d: [f64; 2], z: f64, reach: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
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
            {
                best = Some(best.map_or(t, |x: f64| x.min(t)));
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
    if e.class != "IfcSanitaryTerminal" {
        return None;
    }
    let g = mesh_objects::regenerate(store, e).ok()?;
    let kind = FixtureKind::from_predefined(g.ty.predefined_type())?;
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
    // Look sideways from the centreline at mid-depth, a little above the floor.
    let mid = frame.apply([0.0, (y[0] + y[1]) / 2.0, 0.0]);
    let look = |sign: f64| {
        let d = frame.apply_direction([sign, 0.0, 0.0]);
        ray_to_walls(store, [mid[0], mid[1]], [d[0], d[1]], floor + 0.3, SIDE_SEARCH)
    };
    let (left, right) = (look(-1.0), look(1.0));
    let (side, side_distance) = match hand {
        Hand::Left => (-1.0, left),
        Hand::Right => (1.0, right),
        Hand::Auto => match (left, right) {
            (Some(l), Some(r)) if r < l => (1.0, Some(r)),
            (Some(l), _) => (-1.0, Some(l)),
            (None, Some(r)) => (1.0, Some(r)),
            (None, None) => (-1.0, None),
        },
    };
    Some(Fixture { element: e.id(), kind, frame, x, front: y[0], back: y[1], floor, side, side_distance, hand })
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

/// The zones of the packs in use at one fixture.
pub fn spaces_of(store: &Store, f: &Fixture, packs: &[Pack]) -> Vec<Space> {
    let occupancy = crate::codes::settings(store).occupancy;
    let assumed = assumed_side(packs, f.kind);
    let mut out = Vec::new();
    for p in packs {
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
        .flat_map(|p| p.distances.iter().map(move |d| (p, d)))
        .filter(|(_, d)| d.fixtures.contains(&f.kind) && d.occupancy.applies(occupancy))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_packs_are_valid_and_cite_sources() {
        let register = include_str!("../../../design_documents/SOURCE_REGISTER.md");
        assert_eq!(shipped().len(), 4);
        for p in shipped() {
            let sources = p.zones.iter().map(|z| &z.source).chain(p.distances.iter().map(|d| &d.source));
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
}
