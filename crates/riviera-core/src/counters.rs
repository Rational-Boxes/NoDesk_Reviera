// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Counters with height zones (ARC-INT-004, `INTERIORS_PLAN.md` §4.4): a transaction, service,
//! bar or work counter along a back line, with an accessible portion lowered to 36 in where the
//! 2010 ADA Standards ask one (§904.4).
//!
//! A counter is built from segments along the line. Each has a casework body (`IfcFurniture`
//! USERDEFINED "Casework", built in) and a top (`IfcSystemFurnitureElement` WORKSURFACE). The
//! accessible portion's top carries its approach as its type's ObjectType
//! (AccessibleCounterParallel or AccessibleCounterForward), which the interiors rule packs select:
//! a 30 × 48 in clear floor space in front of it (§305, §904.4), its length and height (the
//! surfaces rule kind), and, for a forward approach, knee clearance under it (§306), where no
//! body is built.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bath_components::Shape;
use crate::cabinets::{CARCASS, WORKTOP, line_frame, unit_type};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::Category;
use crate::txn::Transaction;

/// The tag on a counter's parts: `{counter, part}`.
pub const COUNTER_KEY: &str = "rxd:counter";
const IN: f64 = 0.0254;
/// A counter's top thickness, and how far it passes the body's front.
const TOP: f64 = 1.5 * IN;
const OVERHANG: f64 = 1.0 * IN;
const TOE: (f64, f64) = (4.0 * IN, 3.0 * IN);

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What a counter is for, which sets its height (research/15 C1–C3, `INTERIORS_PLAN.md` §4.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A transaction counter with its ledge at 42 in.
    Transaction,
    /// A service counter at 36 in.
    #[default]
    Service,
    /// A bar counter at 42 in.
    Bar,
    /// A work surface at 29 in, open below for knees.
    Work,
}

impl Kind {
    fn height(self) -> f64 {
        match self {
            Kind::Transaction | Kind::Bar => 42.0 * IN,
            Kind::Service => 36.0 * IN,
            Kind::Work => 29.0 * IN,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Approach {
    /// Alongside the portion: 36 in long, a 48 × 30 in space along it (§904.4.1).
    #[default]
    Parallel,
    /// Facing it: 30 in long, a 30 × 48 in space in front and knee clearance under it (§904.4.2).
    Forward,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum At {
    Start,
    #[default]
    End,
}

/// The counter's accessible portion.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Portion {
    /// At the line's start or end (default the end) …
    pub at: Option<At>,
    /// … or this far along it from the start to the portion's start.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub offset: Option<f64>,
    pub approach: Option<Approach>,
    /// Default 36 in (parallel) or 30 in (forward), the ADA minimums.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub length: Option<f64>,
    /// Its surface above the floor; default 36 in, the ADA maximum, or the counter's height if
    /// that is lower.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub height: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CounterParams {
    /// The counter's back line: from here …
    pub start: [f64; 2],
    /// … to here.
    pub end: [f64; 2],
    pub level: ElementRef,
    #[serde(default)]
    pub kind: Kind,
    /// Its surface above the floor (default from the kind).
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// The body's depth (default 24 in); the top passes it by 1 in.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub depth: Option<f64>,
    /// A point in front of it; default away from the wall behind it.
    pub toward: Option<[f64; 2]>,
    /// A portion lowered for accessibility; none when absent.
    pub accessible: Option<Portion>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PortionOut {
    /// Its top, which the rule packs check.
    pub top: ElementId,
    /// Along the line from its start.
    pub from: f64,
    pub to: f64,
    pub height: f64,
    pub approach: Approach,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Counter {
    /// The counter's id (its parts' tag).
    pub counter: String,
    pub length: f64,
    pub height: f64,
    /// Its bodies and tops, along the line.
    pub elements: Vec<ElementId>,
    pub portion: Option<PortionOut>,
}

fn mm(v: f64) -> String {
    format!("{:.0}", v * 1000.0)
}

/// Lays out a counter (`counter.run`).
pub(crate) fn run(txn: &mut Transaction<'_>, p: CounterParams) -> Result<Counter> {
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let z0 = crate::levels::elevation(level);
    let (length, d, front, rotation) = line_frame(&store, p.start, p.end, z0, p.toward)?;
    let height = p.height.unwrap_or(p.kind.height());
    let depth = p.depth.unwrap_or(24.0 * IN);
    if height <= TOP + TOE.0 || depth <= 0.0 {
        return Err(bad("counter.bad_size", "a counter needs a height over its toe kick and top, and a depth"));
    }
    // The segments along the line: (from, to, the portion's approach and height if it is one).
    let mut segments: Vec<(f64, f64, Option<(Approach, f64)>)> = Vec::new();
    match &p.accessible {
        None => segments.push((0.0, length, None)),
        Some(a) => {
            let approach = a.approach.unwrap_or_default();
            let len = a.length.unwrap_or(match approach {
                Approach::Parallel => 36.0 * IN,
                Approach::Forward => 30.0 * IN,
            });
            let ph = a.height.unwrap_or(36.0 * IN).min(height);
            let from = match (a.offset, a.at.unwrap_or_default()) {
                (Some(o), _) => o,
                (None, At::Start) => 0.0,
                (None, At::End) => length - len,
            };
            if len <= 0.0 || from < -1e-9 || from + len > length + 1e-9 {
                return Err(bad("counter.portion_outside", format!("a {:.0} in portion at {from:.3} m does not fit the {length:.3} m counter", len / IN)));
            }
            for (a, b, portion) in [(0.0, from, None), (from, from + len, Some((approach, ph))), (from + len, length, None)] {
                if b - a > 0.003 || portion.is_some() {
                    segments.push((a, b, portion));
                }
            }
        }
    }
    let counter = crate::id::GlobalId::new().as_str().to_owned();
    let mut elements = Vec::new();
    let mut portion_out = None;
    let at = |c: f64, out: f64| [p.start[0] + d[0] * c + front[0] * out, p.start[1] + d[1] * c + front[1] * out];
    let top_depth = depth + OVERHANG;
    for (a, b, portion) in segments {
        let w = b - a;
        let c = (a + b) / 2.0;
        let h = portion.map_or(height, |(_, ph)| ph);
        let open = p.kind == Kind::Work || matches!(portion, Some((Approach::Forward, _)));
        if !open {
            let body_h = h - TOP;
            let sh = Shape::default().cuboid([-w / 2.0, w / 2.0], [-depth / 2.0 + TOE.1, depth / 2.0], [0.0, TOE.0]).cuboid([-w / 2.0, w / 2.0], [-depth / 2.0, depth / 2.0], [TOE.0, body_h]);
            let ty = unit_type(txn, &format!("Counter body {} × {} × {}", mm(w), mm(depth), mm(body_h)), Category::Furniture, "USERDEFINED", "Casework", &sh, [w, depth, body_h], CARCASS)?;
            let id = crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: at(c, depth / 2.0), level: p.level.clone(), offset: 0.0, rotation, mirror: false, scale: 1.0 })?;
            txn.modify(id, |e| {
                e.params.insert(COUNTER_KEY.into(), json!({ "counter": counter, "part": "body" }));
            })?;
            elements.push(id);
        }
        let (name, object_type) = match portion {
            Some((Approach::Parallel, _)) => ("Accessible counter top, parallel approach", Some("AccessibleCounterParallel")),
            Some((Approach::Forward, _)) => ("Accessible counter top, forward approach", Some("AccessibleCounterForward")),
            None => ("Counter top", None),
        };
        let sh = Shape::default().cuboid([-w / 2.0, w / 2.0], [-top_depth / 2.0, top_depth / 2.0], [0.0, TOP]);
        let ty = unit_type(txn, &format!("{name} {} × {}", mm(w), mm(top_depth)), Category::SystemFurniture, "WORKSURFACE", "", &sh, [w, top_depth, TOP], WORKTOP)?;
        if let Some(ot) = object_type {
            txn.modify(ty, |e| {
                e.params.insert("@ElementType".into(), ot.into());
            })?;
        }
        let id = crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: at(c, top_depth / 2.0), level: p.level.clone(), offset: h - TOP, rotation, mirror: false, scale: 1.0 })?;
        txn.modify(id, |e| {
            e.params.insert(COUNTER_KEY.into(), json!({ "counter": counter, "part": if portion.is_some() { "accessible_top" } else { "top" } }));
        })?;
        elements.push(id);
        if let Some((approach, ph)) = portion {
            portion_out = Some(PortionOut { top: id, from: a, to: b, height: ph, approach });
        }
    }
    Ok(Counter { counter, length, height, elements, portion: portion_out })
}
