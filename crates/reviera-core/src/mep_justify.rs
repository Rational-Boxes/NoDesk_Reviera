// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The justification editor (MEP-DUCT-006): `mep_run.justify` aligns a run whose segments
//! change size on one face or edge.
//!
//! - The run is a single chain of segments and fittings without branches (no fitting of more
//!   than two legs, no taps); anything else is refused ("Justify needs a single run without
//!   branches").
//! - The control segment (the run's `start` or `end` segment) stays where it is. Walking away
//!   from it, each segment is moved square to its axis so that its chosen alignment point
//!   (top, middle or bottom × left, centre or right of its section, left and right as seen
//!   travelling away from the control) lies on the line of the control's, where it continues
//!   straight on through a transition, coupling or accessory, or directly; across an elbow it
//!   moves with what it follows. Fittings move with the segment before them; a transition
//!   whose two sides no longer share an axis becomes eccentric (its legs are offset).
//!   Size-locked segments keep their size and sloped ones their slope (they are only moved).
//! - The start of the run is the end whose first segment's free connector is its start
//!   connector (the end it was drawn from), else the end of the lower element id.
//! - Each segment keeps its justification, so a later size change (`duct.set_size`) moves it
//!   to keep the aligned face or edge in place, and the fittings beside it follow.
//! - A run end joined to something outside the run (equipment, a terminal, another run) that
//!   would have to move is refused, naming that element: make that end the control.
//!
//! Not yet: alignment lines picked in a 3D view, the contextual tab with the control-point
//! glyph (the tool is a dialog, so there is no pending choice to journal), stretching or moving
//! branches to keep them joined (a run with branches is refused).

use std::collections::{BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{self, Shape};
use crate::mep_runs::{self, SegmentRecipe, Size};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}

const BRANCHES: &str = "Justify needs a single run without branches";

/// One of the nine alignment points of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Alignment {
    TL,
    TC,
    TR,
    ML,
    MC,
    MR,
    BL,
    BC,
    BR,
}

impl Alignment {
    pub const ALL: [Alignment; 9] = [Alignment::TL, Alignment::TC, Alignment::TR, Alignment::ML, Alignment::MC, Alignment::MR, Alignment::BL, Alignment::BC, Alignment::BR];

    pub fn label(self) -> &'static str {
        match self {
            Alignment::TL => "Top Left",
            Alignment::TC => "Top Centre",
            Alignment::TR => "Top Right",
            Alignment::ML => "Middle Left",
            Alignment::MC => "Middle Centre",
            Alignment::MR => "Middle Right",
            Alignment::BL => "Bottom Left",
            Alignment::BC => "Bottom Centre",
            Alignment::BR => "Bottom Right",
        }
    }

    /// Across (+1 left, −1 right) and up (+1 top, −1 bottom).
    fn signs(self) -> (f64, f64) {
        use Alignment::*;
        let across = match self {
            TL | ML | BL => 1.0,
            TC | MC | BC => 0.0,
            TR | MR | BR => -1.0,
        };
        let up = match self {
            TL | TC | TR => 1.0,
            ML | MC | MR => 0.0,
            BL | BC | BR => -1.0,
        };
        (across, up)
    }
}

/// A segment's justification: the alignment point, and whether the run travels against the
/// segment's own direction (end to start), which sets its left and right.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Justification {
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reversed: bool,
}

/// The direction the run travels along a segment.
fn travel(s: &SegmentRecipe, reversed: bool) -> Option<V3> {
    let u = unit(sub(s.end, s.start))?;
    Some(if reversed { mul(u, -1.0) } else { u })
}

/// The alignment point's offset from the centreline for a size, travelling along `u`.
fn offset(s: &SegmentRecipe, size: &Size, u: V3, a: Alignment) -> V3 {
    let (w, h) = mep_runs::axes(u, s.across);
    let (width, height) = match size.shape {
        Shape::Round => (size.diameter, size.diameter),
        _ => (size.width, size.height),
    };
    let (sa, su) = a.signs();
    add(mul(w, sa * width / 2.0), mul(h, su * height / 2.0))
}

/// How far a justified segment moves when its size changes from `before` to its own, to keep
/// its aligned face or edge in place.
pub(crate) fn resize_shift(s: &SegmentRecipe, before: &Size, j: &Justification) -> V3 {
    let Some(u) = travel(s, j.reversed) else { return [0.0; 3] };
    sub(offset(s, before, u, j.alignment), offset(s, &s.size, u, j.alignment))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlEnd {
    #[default]
    Start,
    End,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JustifyParams {
    /// The run's segments and fittings (one chain without branches).
    pub element_ids: Vec<ElementRef>,
    /// The segment that stays put: the run's `start` (default) or `end` segment.
    #[serde(default)]
    pub control_end: ControlEnd,
    /// TL, TC, TR, ML, MC, MR, BL, BC or BR (top, middle, bottom × left, centre, right).
    pub alignment: Alignment,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct JustifyReport {
    /// The control segment.
    pub control: Option<ElementId>,
    /// The segments and fittings moved.
    pub moved: Vec<ElementId>,
}

/// The chain in order from one end: each element with the port it is entered by (none for the
/// first) and the port it leaves by (none for the last).
type Chain = Vec<(ElementId, Option<usize>, Option<usize>)>;

fn chain(store: &Store, ids: &[ElementId]) -> Result<Chain> {
    let set: BTreeSet<ElementId> = ids.iter().copied().collect();
    for id in &set {
        let e = store.get(*id).ok_or_else(|| bad("element.not_found", "no such element"))?;
        if let Some(s) = mep_runs::segment_recipe(e) {
            if s.placeholder {
                return Err(bad("mep_run.justify_placeholder", "a placeholder is always on its centreline; convert it first"));
            }
            if !s.taps.is_empty() {
                return Err(bad("mep_run.justify_branches", BRANCHES));
            }
            if s.flex.is_some() || s.corner.is_some() {
                return Err(bad("mep_run.justify", format!("element {id} is curved; justify straight runs")));
            }
        } else if let Some(f) = mep_runs::fitting_recipe(e) {
            if f.legs.len() > 2 {
                return Err(bad("mep_run.justify_branches", BRANCHES));
            }
        } else {
            return Err(bad("mep_run.not_a_run", format!("element {id} is not a segment or fitting")));
        }
    }
    let partners = mep::partners(store);
    // Each element's links within the run: (its port, the other, the other's port).
    let mut inner: HashMap<ElementId, Vec<(usize, ElementId, usize)>> = HashMap::new();
    for ((a, ka), list) in &partners {
        if !set.contains(a) {
            continue;
        }
        for (b, kb) in list {
            if set.contains(b) {
                inner.entry(*a).or_default().push((*ka, *b, *kb));
            }
        }
    }
    if inner.values().any(|v| v.len() > 2) {
        return Err(bad("mep_run.justify_branches", BRANCHES));
    }
    let ends: Vec<ElementId> = set.iter().copied().filter(|id| inner.get(id).map_or(0, Vec::len) <= 1).collect();
    if ends.len() != 2 && set.len() > 1 || ends.is_empty() {
        return Err(bad("mep_run.justify_branches", BRANCHES));
    }
    let walk = |first: ElementId| -> Chain {
        let mut out: Chain = Vec::new();
        let mut at = first;
        let mut entered: Option<usize> = None;
        let mut prev: Option<ElementId> = None;
        loop {
            let next = inner.get(&at).into_iter().flatten().find(|(k, b, _)| Some(*b) != prev && Some(*k) != entered).copied();
            out.push((at, entered, next.map(|n| n.0)));
            match next {
                Some((_, b, kb)) if out.len() <= set.len() => {
                    prev = Some(at);
                    at = b;
                    entered = Some(kb);
                }
                _ => break,
            }
        }
        out
    };
    let c = walk(ends[0]);
    if c.len() != set.len() {
        return Err(bad("mep_run.justify_branches", BRANCHES));
    }
    Ok(c)
}

/// Whether the first segment from a chain's end faces that end with its start connector (the
/// end the run was drawn from).
fn starts_at_drawn_start(store: &Store, c: &Chain) -> Option<bool> {
    let (_, entered, left) = c.iter().find(|x| store.get(x.0).and_then(mep_runs::segment_recipe).is_some())?;
    let away = entered.unwrap_or_else(|| 1 - left.unwrap_or(1).min(1));
    Some(away == 0)
}

/// `mep_run.justify`.
pub(crate) fn justify(txn: &mut Transaction<'_>, p: JustifyParams) -> Result<JustifyReport> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p
        .element_ids
        .iter()
        .map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element")))
        .collect::<Result<_>>()?;
    if ids.is_empty() {
        return Err(bad("mep_run.justify", "select a run"));
    }
    let mut c = chain(&store, &ids)?;
    if !c.iter().any(|x| store.get(x.0).and_then(mep_runs::segment_recipe).is_some()) {
        return Err(bad("mep_run.justify", "the run has no segments"));
    }
    // Order from the start: the end it was drawn from.
    let reversed: Chain = c.iter().rev().map(|(id, e, l)| (*id, *l, *e)).collect();
    let from_a = starts_at_drawn_start(&store, &c);
    let from_b = starts_at_drawn_start(&store, &reversed);
    let a_is_start = match (from_a, from_b) {
        (Some(true), Some(false)) => true,
        (Some(false), Some(true)) => false,
        _ => c.first().map(|x| x.0) <= reversed.first().map(|x| x.0),
    };
    if !a_is_start {
        c = reversed;
    }
    if p.control_end == ControlEnd::End {
        c = c.iter().rev().map(|(id, e, l)| (*id, *l, *e)).collect();
    }
    // Walk away from the control segment; elements before it (end fittings) stay.
    let first_seg = c.iter().position(|x| store.get(x.0).and_then(mep_runs::segment_recipe).is_some()).expect("a segment");
    let control = c[first_seg].0;
    let mut moves: HashMap<ElementId, V3> = c[..=first_seg].iter().map(|x| (x.0, [0.0; 3])).collect();
    let mut justs: HashMap<ElementId, Justification> = HashMap::new();
    // The last segment placed: its id, recipe, the run's direction along it and its move.
    let mut last: Option<(SegmentRecipe, usize, V3, V3)> = None;
    let mut carry = [0.0; 3];
    for (k, (id, entered, left)) in c.iter().enumerate() {
        let e = store.get(*id).expect("listed");
        if k < first_seg {
            continue;
        }
        if let Some(s) = mep_runs::segment_recipe(e) {
            // The run enters by `entered` (0: start, 1: end) and leaves by `left`.
            let rev = matches!((entered, left), (Some(1), _) | (None, Some(0)));
            let u = travel(&s, rev).ok_or_else(|| bad("mep_run.justify", format!("element {id} has no length")))?;
            let mut mv = carry;
            if let Some((ps, pleave, pu, pmv)) = &last
                && dot(*pu, u) > 0.9999
            {
                // In line with the segment before: its aligned point's line continues.
                let pend = add(if *pleave == 1 { ps.end } else { ps.start }, *pmv);
                let near = if rev { s.end } else { s.start };
                let want = add(pend, offset(ps, &ps.size, *pu, p.alignment));
                let have = add(near, offset(&s, &s.size, u, p.alignment));
                let d = sub(want, have);
                mv = sub(d, mul(u, dot(d, u)));
            }
            if *id == control {
                mv = [0.0; 3];
            }
            moves.insert(*id, mv);
            justs.insert(*id, Justification { alignment: p.alignment, reversed: rev });
            carry = mv;
            last = Some((s, if rev { 0 } else { 1 }, u, mv));
        } else {
            // A fitting moves with what it follows.
            moves.insert(*id, carry);
        }
    }
    // Run ends joined to something outside the run must not move.
    let partners = mep::partners(&store);
    let inside: BTreeSet<ElementId> = c.iter().map(|x| x.0).collect();
    for (id, mv) in &moves {
        if len(*mv) < 1e-9 {
            continue;
        }
        let n = mep::ports(&store, store.get(*id).expect("listed")).len();
        for k in 0..n {
            for (o, _) in partners.get(&(*id, k)).into_iter().flatten() {
                if !inside.contains(o) {
                    return Err(bad(
                        "mep_run.justify_joined",
                        format!("justifying would move the run off element {o}, which it is joined to; make that end the control point"),
                    ));
                }
            }
        }
    }
    // Apply: segments and fitting centres move; each fitting leg ends at its partner's port.
    let mut report = JustifyReport { control: Some(control), moved: Vec::new() };
    let new_port = |el: ElementId, port: usize| -> Option<V3> {
        let p = mep::port(&store, el, port)?;
        Some(add(p.position, moves.get(&el).copied().unwrap_or([0.0; 3])))
    };
    for (id, ..) in &c {
        let e = store.get(*id).expect("listed");
        let mv = moves.get(id).copied().unwrap_or([0.0; 3]);
        let value = if let Some(mut s) = mep_runs::segment_recipe(e) {
            s.start = add(s.start, mv);
            s.end = add(s.end, mv);
            s.justification = justs.get(id).copied();
            serde_json::to_value(&s).expect("serialises")
        } else if let Some(mut f) = mep_runs::fitting_recipe(e) {
            let centre = add(f.centre, mv);
            let mut offsets = Vec::new();
            for k in 0..f.legs.len() {
                let l = f.legs[k];
                let own = add(add(centre, mul(l.direction, l.length)), f.offsets.get(k).copied().unwrap_or([0.0; 3]));
                let target = partners
                    .get(&(*id, k))
                    .into_iter()
                    .flatten()
                    .find(|(o, _)| inside.contains(o))
                    .and_then(|(o, ko)| new_port(*o, *ko))
                    .unwrap_or(own);
                offsets.push(sub(target, add(centre, mul(l.direction, l.length))));
            }
            f.centre = centre;
            f.offsets = if offsets.iter().all(|o| len(*o) < 1e-9) { Vec::new() } else { offsets };
            serde_json::to_value(&f).expect("serialises")
        } else {
            continue;
        };
        if len(mv) > 1e-9 && !report.moved.contains(id) {
            report.moved.push(*id);
        }
        if store.get(*id).and_then(|e| e.param(RECIPE_KEY)) != Some(&value) {
            txn.modify(*id, |e| {
                e.params.insert(RECIPE_KEY.into(), value);
            })?;
        }
    }
    Ok(report)
}
