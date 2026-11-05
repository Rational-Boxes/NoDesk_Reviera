// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Fare lines (ARC-VT-022): a row of fare gates across a station concourse, as one object of
//! cabinets with aisles between them. Each aisle is a tripod turnstile, a paddle gate, a wide
//! (accessible) gate or an emergency gate, and passes people at its rate in an evacuation
//! (SCDF CPFPRTS Table 2.2.3i), when the gates stand open. Paths of travel walk the aisles
//! between the cabinets, and platform clearing (ANA-ROUTE-005) adds the wait at a line its route
//! crosses.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::elevators::measure;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// The fare line's data, kept on its object.
pub const DATA_KEY: &str = "rxd:fare_line";

/// A cabinet's width across the line, its length along the way through, and its height (m).
pub const CABINET_WIDTH: f64 = 0.3;
pub const CABINET_LENGTH: f64 = 1.6;
pub const CABINET_HEIGHT: f64 = 1.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What stands in an aisle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// A tripod turnstile, 0.5 m clear.
    Tripod,
    /// A paddle or swing gate narrower than 850 mm, 0.6 m clear.
    #[default]
    Paddle,
    /// A wide (accessible) gate, 0.9 m clear.
    Wide,
    /// An emergency (swing) gate, 1.0 m clear.
    Emergency,
}

impl GateKind {
    /// The clear width of its aisle (m).
    pub fn aisle(self) -> f64 {
        match self {
            GateKind::Tripod => 0.5,
            GateKind::Paddle => 0.6,
            GateKind::Wide => 0.9,
            GateKind::Emergency => 1.0,
        }
    }

    /// Persons a minute in an evacuation (SCDF CPFPRTS Table 2.2.3i): 25 a turnstile, 50 a gate
    /// under 850 mm, and 80 a minute per metre of a wider one.
    pub fn capacity(self) -> f64 {
        match self {
            GateKind::Tripod => 25.0,
            GateKind::Paddle => 50.0,
            GateKind::Wide | GateKind::Emergency => 80.0 * self.aisle(),
        }
    }
}

/// A fare line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FareLine {
    /// Its level (GlobalId).
    pub level: String,
    /// Where it starts, and the unit direction it runs.
    pub start: V2,
    pub direction: V2,
    /// Its aisles in order from the start.
    pub aisles: Vec<GateKind>,
    /// Its length (m) and its capacity (persons a minute).
    pub length: f64,
    pub capacity: f64,
}

impl FareLine {
    fn at(&self, along: f64, across: f64) -> V2 {
        let (u, n) = (self.direction, [-self.direction[1], self.direction[0]]);
        [self.start[0] + u[0] * along + n[0] * across, self.start[1] + u[1] * along + n[1] * across]
    }

    /// Where it ends.
    pub fn end(&self) -> V2 {
        self.at(self.length, 0.0)
    }

    /// Its cabinets' outlines in plan (counter-clockwise), one either side of each aisle.
    pub fn cabinets(&self) -> Vec<Vec<V2>> {
        let mut out = Vec::new();
        let mut x = 0.0;
        let h = CABINET_LENGTH / 2.0;
        for k in 0..=self.aisles.len() {
            out.push(vec![self.at(x, -h), self.at(x + CABINET_WIDTH, -h), self.at(x + CABINET_WIDTH, h), self.at(x, h)]);
            x += CABINET_WIDTH + self.aisles.get(k).map_or(0.0, |g| g.aisle());
        }
        out
    }
}

fn length(aisles: &[GateKind]) -> f64 {
    aisles.iter().map(|g| g.aisle()).sum::<f64>() + CABINET_WIDTH * (aisles.len() + 1) as f64
}

/// A fare line's data, if the element is one.
pub fn fare_line(e: &crate::element::Element) -> Option<FareLine> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

/// Every fare line.
pub fn fare_lines(store: &Store) -> Vec<(ElementId, FareLine)> {
    store.iter().filter_map(|e| Some((e.id(), fare_line(e)?))).collect()
}

fn crosses(a: V2, b: V2, c: V2, d: V2) -> bool {
    let cr = |o: V2, p: V2, q: V2| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    let (d1, d2, d3, d4) = (cr(c, d, a), cr(c, d, b), cr(a, b, c), cr(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The fare lines on `level` that a walk along `points` passes through, in the order met.
pub fn crossed(store: &Store, level: ElementId, points: &[V2]) -> Vec<(ElementId, FareLine)> {
    let Some(guid) = store.get(level).map(|e| e.guid().to_string()) else { return Vec::new() };
    let mut out: Vec<(usize, ElementId, FareLine)> = Vec::new();
    for (id, f) in fare_lines(store).into_iter().filter(|(_, f)| f.level == guid) {
        let (c, d) = (f.start, f.end());
        if let Some(k) = points.windows(2).position(|w| crosses(w[0], w[1], c, d)) {
            out.push((k, id, f));
        }
    }
    out.sort_by_key(|(k, _, _)| *k);
    out.into_iter().map(|(_, id, f)| (id, f)).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level: ElementRef,
    /// Where the line starts; it runs toward `toward` for as long as its gates need.
    pub start: V2,
    pub toward: V2,
    /// The kind of most of its gates, and how many (default paddle).
    #[serde(default)]
    pub kind: GateKind,
    pub count: u32,
    /// Wide (accessible) gates first (default 1) and emergency gates last (default 0).
    #[serde(default = "one")]
    pub wide: u32,
    #[serde(default)]
    pub emergency: u32,
}

fn one() -> u32 {
    1
}

/// What `fare_line.create` made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    pub fare_line: ElementId,
    #[serde(flatten)]
    pub data: FareLine,
}

/// `fare_line.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store();
    let level = store.resolve(&p.level).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let info = crate::levels::list(store).into_iter().find(|l| l.id == level).ok_or_else(|| bad("vt.not_a_level", format!("element {level} is not a level")))?;
    let n = p.count + p.wide + p.emergency;
    if n == 0 || n > 200 {
        return Err(bad("vt.invalid", "a fare line has from 1 to 200 gates"));
    }
    let (dx, dy) = (p.toward[0] - p.start[0], p.toward[1] - p.start[1]);
    let l = dx.hypot(dy);
    if !(l.is_finite() && l > 1e-6) {
        return Err(bad("vt.invalid", "the line needs a direction: `toward` apart from `start`"));
    }
    let aisles: Vec<GateKind> = std::iter::repeat_n(GateKind::Wide, p.wide as usize)
        .chain(std::iter::repeat_n(p.kind, p.count as usize))
        .chain(std::iter::repeat_n(GateKind::Emergency, p.emergency as usize))
        .collect();
    let line = FareLine {
        level: info.guid.clone(),
        start: p.start,
        direction: [dx / l, dy / l],
        length: length(&aisles),
        capacity: aisles.iter().map(|g| g.capacity()).sum(),
        aisles,
    };
    // The cabinets, from the level up.
    let (mut pts, mut tris) = (Vec::new(), Vec::new());
    for c in line.cabinets() {
        let (q, idx) = crate::bodies::extrude(&c, &[], 0.0, CABINET_HEIGHT);
        let base = pts.len() as u32;
        pts.extend(q);
        tris.extend(idx.chunks(3).map(|t| [base + t[0], base + t[1], base + t[2]]));
    }
    let name = format!("Fare Line, {} gates, {:.0} a minute", line.aisles.len(), line.capacity);
    let ty = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::GenericModel, &pts, &tris, [0.62, 0.64, 0.68, 1.0], Vec::new(), Some("USERDEFINED"))?;
    txn.modify(ty, |e| {
        e.params.insert("@ElementType".into(), "FAREGATE".into());
    })?;
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(level), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )?;
    let data = serde_json::to_value(&line).expect("serialises");
    let props = properties(&line);
    txn.modify(id, |e| {
        e.name = Some("Fare Line".into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { fare_line: id, data: line })
}

/// Its property set: `Riviera_FareLine`.
pub fn properties(f: &FareLine) -> Vec<(String, Value)> {
    let count = |k: GateKind| f.aisles.iter().filter(|g| **g == k).count();
    vec![
        ("Riviera_FareLine.Gates".into(), measure("IfcCountMeasure", f.aisles.len())),
        ("Riviera_FareLine.Turnstiles".into(), measure("IfcCountMeasure", count(GateKind::Tripod))),
        ("Riviera_FareLine.PaddleGates".into(), measure("IfcCountMeasure", count(GateKind::Paddle))),
        ("Riviera_FareLine.WideGates".into(), measure("IfcCountMeasure", count(GateKind::Wide))),
        ("Riviera_FareLine.EmergencyGates".into(), measure("IfcCountMeasure", count(GateKind::Emergency))),
        ("Riviera_FareLine.Length".into(), measure("IfcPositiveLengthMeasure", f.length)),
        ("Riviera_FareLine.EvacuationCapacity".into(), measure("IfcReal", f.capacity)),
    ]
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// A fare line as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub fare_line: ElementId,
    /// Its level's name.
    pub level_name: String,
    #[serde(flatten)]
    pub data: FareLine,
}

/// `fare_line.list`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<Info>> {
    let levels = crate::levels::list(store);
    Ok(fare_lines(store)
        .into_iter()
        .map(|(id, data)| Info { fare_line: id, level_name: levels.iter().find(|l| l.guid == data.level).map_or_else(String::new, |l| l.name.clone()), data })
        .collect())
}
