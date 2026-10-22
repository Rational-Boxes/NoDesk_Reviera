// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe placeholders (MEP-DUCT-007): single-line segments without fittings for fast
//! early layout, converted later into fitted runs without redrawing.
//!
//! - A placeholder is a segment recipe with `placeholder` set. Its ports are its two ends; it has
//!   no body (IFC gets its `Axis` alone), so sizing, pressure drop, insulation, justification and
//!   interference checks pass it by, while system flow and *Check Systems* see it.
//! - Each picked leg is one placeholder. Ends that meet are connected to every other end there,
//!   of any direction or size (no fittings exist to face), and to an equipment connector there.
//!   An end on another placeholder's side splits that one, so a network is a graph of segments
//!   meeting at points.
//! - Conversion works point by point over the selected placeholders' ends: two legs in line
//!   join directly, or by a transition when their sizes differ; two legs at an angle by an
//!   elbow; three by a tee (two in line, the branch square to them); four by a cross (two
//!   square pairs), each by the type's routing preferences, the segments shortened by the
//!   fittings' legs. A point the rules cannot fit (more than four legs, a branch off square, a
//!   size change at a bend, a missing routing row, a placeholder left out) keeps its ends where
//!   they are, open, and the report lists each with the reason.
//! - A converted placeholder stays the same element: its GlobalId, type, size, slope and system
//!   membership are kept. Fittings are new and join the system.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{self, Domain, Shape};
use crate::mep_runs::{self, CreateParams, Ctx, Leg, Part, Role, SegmentRecipe, Size, segment_recipe};
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
    (l > 1e-12).then(|| mul(a, 1.0 / l))
}

/// Ends closer than this are one point.
const NEAR: f64 = mep::POSITION_TOLERANCE;

/// What placeholders made.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct PlaceholderReport {
    /// The placeholders drawn, then the second halves of those split by an end on their side.
    pub segments: Vec<ElementId>,
    /// Elements that were there before and are now connected.
    pub connected_to: Vec<ElementId>,
    /// The system it joined.
    pub system: Option<ElementId>,
}

/// Whether an element is a placeholder.
pub fn is_placeholder(store: &Store, id: ElementId) -> bool {
    store.get(id).and_then(segment_recipe).is_some_and(|s| s.placeholder)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, s: &SegmentRecipe) -> Result<()> {
    let v = serde_json::to_value(s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

/// A placeholder of `domain` whose side (away from its ends) `q` lies on.
fn side_at(store: &Store, domain: Domain, q: V3) -> Option<(ElementId, SegmentRecipe)> {
    store.iter().find_map(|e| {
        let s = segment_recipe(e)?;
        if !s.placeholder || s.domain != domain {
            return None;
        }
        let u = unit(sub(s.end, s.start))?;
        let t = dot(sub(q, s.start), u);
        let off = len(sub(q, add(s.start, mul(u, t))));
        (t > NEAR && t < len(sub(s.end, s.start)) - NEAR && off <= NEAR).then(|| (e.id(), s))
    })
}

/// Draws placeholders (`duct_placeholder.create`, `pipe_placeholder.create`): one per leg of the
/// picked points, at the given type, size, elevation and system type.
pub(crate) fn create(txn: &mut Transaction<'_>, domain: Domain, p: CreateParams) -> Result<PlaceholderReport> {
    if p.bend_radius.is_some() {
        return Err(bad("placeholder.bend_radius", "placeholders have no bends"));
    }
    let slope = p.slope;
    let (cx, t, size, pts) = mep_runs::prepare(txn, domain, &p)?;
    if t.radius_bends() {
        return Err(bad("placeholder.domain", "placeholders are ducts or pipes"));
    }
    let mut report = PlaceholderReport::default();
    // An end on another placeholder's side splits it there.
    for q in [pts[0], *pts.last().expect("two")] {
        let store = txn.store().clone();
        let Some((host, hs)) = side_at(&store, domain, q) else { continue };
        let mut first = hs.clone();
        first.end = q;
        let mut second = hs.clone();
        second.start = q;
        write(txn, host, &first)?;
        let e = store.get(host).expect("host");
        let mut params = e.params.clone();
        params.insert(RECIPE_KEY.into(), serde_json::to_value(&second).expect("serialises"));
        let half = txn.create(NewElement { class: e.class.clone(), name: e.name.clone(), params, guid: None })?;
        mep_runs::retarget_end(txn, host, half)?;
        report.segments.push(half);
    }
    let mut made = Vec::new();
    for w in pts.windows(2) {
        let id = mep_runs::make_segment(txn, &cx, w[0], w[1], size, if domain == Domain::Pipe { slope } else { 0.0 }, None)?;
        let mut s = segment_recipe(txn.store().get(id).expect("made")).expect("a segment");
        s.placeholder = true;
        write(txn, id, &s)?;
        made.push(id);
    }
    made.append(&mut report.segments);
    report.segments = made;
    let before: Vec<ElementId> = txn.store().iter().map(|e| e.id()).collect();
    for c in mep::connect_coincident(txn, &report.segments)? {
        for e in [c.a.0, c.b.0] {
            if before.contains(&e) && !report.segments.contains(&e) && !report.connected_to.contains(&e) {
                report.connected_to.push(e);
            }
        }
    }
    report.system = mep_runs::join_system(txn, &report.segments, cx.classification)?;
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertParams {
    /// The placeholders to convert (other elements are passed by).
    pub ids: Vec<ElementRef>,
}

/// An end conversion left open.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenEnd {
    pub element: ElementId,
    pub port: usize,
    pub at: V3,
    pub reason: String,
}

/// What a conversion made.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ConvertReport {
    /// The converted segments, then the fittings made.
    pub element_ids: Vec<ElementId>,
    pub segments: Vec<ElementId>,
    pub fittings: Vec<ElementId>,
    /// Ends left open, with why.
    pub open: Vec<OpenEnd>,
}

/// A placeholder's end at a point: the placeholder, its port (0 start, 1 end), the direction
/// from the point along it and its size.
#[derive(Debug, Clone, Copy)]
struct End {
    id: ElementId,
    port: usize,
    away: V3,
    size: Size,
}

/// How much one size differs from another: the larger change of its two sides.
fn size_change(a: &Size, b: &Size) -> f64 {
    match (a.shape, b.shape) {
        (Shape::Round, Shape::Round) => (a.diameter - b.diameter).abs(),
        _ => (a.width.max(a.diameter) - b.width.max(b.diameter)).abs().max((a.height.max(a.diameter) - b.height.max(b.diameter)).abs()),
    }
}

fn same_size(a: &Size, b: &Size) -> bool {
    a.shape == b.shape && size_change(a, b) < 1e-6
}

/// In line, pointing opposite ways.
fn opposite(a: V3, b: V3) -> bool {
    dot(a, b) < -(0.5f64).to_radians().cos()
}

/// Square to each other.
fn square(a: V3, b: V3) -> bool {
    dot(a, b).abs() < (1.0f64).to_radians().sin()
}

/// One point's fitting: the part, its legs (in the order of `ends`), and each end's trim.
struct Plan {
    part: Part,
    legs: Vec<Leg>,
    trims: Vec<f64>,
}

/// The routing preferences and context of the placeholder an end belongs to.
fn context(store: &Store, s: &SegmentRecipe) -> Ctx {
    Ctx {
        domain: s.domain,
        predefined: "",
        type_guid: s.segment_type.clone(),
        routing: mep_runs::type_of(store, s).map(|t| t.routing).unwrap_or_default(),
        level: s.level.clone(),
        classification: s.classification,
    }
}

/// The fitting a point of `ends` takes, or why it takes none.
fn plan(cx: &Ctx, ends: &[End]) -> std::result::Result<Option<Plan>, String> {
    let leg = |e: &End, length: f64, size: Size| Leg { direction: e.away, length, size, across: None };
    let row = |role: Role, part: Option<Part>, size: f64| -> std::result::Result<Part, String> {
        let found = match part {
            Some(want) => cx.routing.rows(role).iter().find(|r| r.part == want && r.fits(size)).map(|r| r.part),
            None => cx.routing.choose(role, size),
        };
        found.ok_or_else(|| {
            let what = match (role, part) {
                (Role::Junction, _) => "tee",
                (Role::Cross, _) => "cross",
                (Role::Elbow, _) => "elbow",
                _ => "transition",
            };
            format!("no {what} in the routing preferences fits {:.0} mm", size * 1000.0)
        })
    };
    match ends {
        [_] | [] => Ok(None),
        [a, b] if opposite(a.away, b.away) => {
            if same_size(&a.size, &b.size) {
                return Ok(None);
            }
            let part = row(Role::Transition, None, a.size.nominal().max(b.size.nominal()))?;
            let l = (2.5 * size_change(&a.size, &b.size)).max(0.15) / 2.0;
            Ok(Some(Plan { part, legs: vec![leg(a, l, a.size), leg(b, l, b.size)], trims: vec![l, l] }))
        }
        [a, b] => {
            if !same_size(&a.size, &b.size) {
                return Err("the size changes at a bend".into());
            }
            let part = row(Role::Elbow, None, a.size.nominal())?;
            // A vertical leg keeps the level leg's across direction.
            let vertical = |d: V3| d[2].abs() >= 0.999;
            let level = if vertical(a.away) { b.away } else { a.away };
            let across = (!vertical(level)).then(|| mep_runs::frame(level).0);
            let l = mep_runs::elbow_leg(&a.size, mul(a.away, -1.0), b.away, across);
            let mut legs = vec![leg(a, l, a.size), leg(b, l, b.size)];
            for g in &mut legs {
                g.across = across;
            }
            Ok(Some(Plan { part, legs, trims: vec![l, l] }))
        }
        [_, _, _] | [_, _, _, _] => {
            // The main: an in-line pair (the larger when there are two); the rest branch off
            // square to it, in line with each other when there are two.
            let n = ends.len();
            let mut pairs: Vec<(usize, usize)> = Vec::new();
            for i in 0..n {
                for j in i + 1..n {
                    if opposite(ends[i].away, ends[j].away) {
                        pairs.push((i, j));
                    }
                }
            }
            let nominal = |k: usize| ends[k].size.nominal();
            pairs.sort_by(|x, y| nominal(y.0).max(nominal(y.1)).total_cmp(&nominal(x.0).max(nominal(x.1))));
            let Some(&(i, j)) = pairs.first() else {
                return Err(format!("no two of its {n} legs are in line"));
            };
            let rest: Vec<usize> = (0..n).filter(|k| *k != i && *k != j).collect();
            if rest.iter().any(|k| !square(ends[*k].away, ends[i].away)) || (rest.len() == 2 && !opposite(ends[rest[0]].away, ends[rest[1]].away)) {
                return Err("a branch does not meet the main square to it".into());
            }
            let main = nominal(i).max(nominal(j));
            let part = if n == 3 { row(Role::Junction, Some(Part::Tee), main)? } else { row(Role::Cross, None, main)? };
            let hl = mep_runs::leg_length(if nominal(i) >= nominal(j) { &ends[i].size } else { &ends[j].size });
            let mut legs = Vec::new();
            let mut trims = Vec::new();
            for (k, e) in ends.iter().enumerate() {
                let l = if k == i || k == j { hl } else { main / 2.0 + mep_runs::leg_length(&e.size) };
                legs.push(leg(e, l, e.size));
                trims.push(l);
            }
            Ok(Some(Plan { part, legs, trims }))
        }
        _ => Err(format!("a junction of {} legs; fittings join at most four", ends.len())),
    }
}

/// Draws duct placeholders (`duct_placeholder.create`).
pub(crate) fn create_duct(txn: &mut Transaction<'_>, p: CreateParams) -> Result<PlaceholderReport> {
    create(txn, Domain::Duct, p)
}

/// Draws pipe placeholders (`pipe_placeholder.create`).
pub(crate) fn create_pipe(txn: &mut Transaction<'_>, p: CreateParams) -> Result<PlaceholderReport> {
    create(txn, Domain::Pipe, p)
}

/// Converts placeholders into ducts or pipes with fittings (`placeholder.convert`).
pub(crate) fn convert(txn: &mut Transaction<'_>, p: ConvertParams) -> Result<ConvertReport> {
    let store = txn.store().clone();
    let mut ids: Vec<ElementId> = Vec::new();
    for r in &p.ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        if is_placeholder(&store, id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(bad("placeholder.none", "select duct or pipe placeholders to convert"));
    }
    let recipes: BTreeMap<ElementId, SegmentRecipe> = ids.iter().map(|id| (*id, segment_recipe(store.get(*id).expect("listed")).expect("a placeholder"))).collect();
    // Every selected end, gathered into points.
    let mut points: Vec<(V3, Vec<End>)> = Vec::new();
    for (id, s) in &recipes {
        let Some(u) = unit(sub(s.end, s.start)) else { continue };
        for (port, at, away) in [(0, s.start, u), (1, s.end, mul(u, -1.0))] {
            let end = End { id: *id, port, away, size: s.size };
            match points.iter_mut().find(|(q, _)| len(sub(*q, at)) <= 2.0 * NEAR) {
                Some((_, list)) => list.push(end),
                None => points.push((at, vec![end])),
            }
        }
    }
    // Ends joined to elements other than placeholders before, to report those left open.
    let partners = mep::partners(&store);
    let joined_out = |e: &End| partners.get(&(e.id, e.port)).is_some_and(|v| v.iter().any(|(o, _)| !is_placeholder(&store, *o)));
    let mut report = ConvertReport::default();
    let mut trims: BTreeMap<(ElementId, usize), f64> = BTreeMap::new();
    let mut open_ports: Vec<(ElementId, usize)> = Vec::new();
    let mut fittings: Vec<(Ctx, V3, Plan)> = Vec::new();
    for (at, ends) in &points {
        let left_out = store.iter().any(|e| {
            !ids.contains(&e.id()) && segment_recipe(e).is_some_and(|s| s.placeholder && (len(sub(s.start, *at)) <= 2.0 * NEAR || len(sub(s.end, *at)) <= 2.0 * NEAR))
        });
        let cx = context(&store, &recipes[&ends[0].id]);
        let outcome = if left_out { Err("it meets a placeholder not being converted".to_string()) } else { plan(&cx, ends) };
        match outcome {
            Ok(Some(pl)) => {
                for (e, t) in ends.iter().zip(&pl.trims) {
                    trims.insert((e.id, e.port), *t);
                }
                fittings.push((cx, *at, pl));
            }
            Ok(None) => {}
            Err(reason) => {
                for e in ends {
                    open_ports.push((e.id, e.port));
                    report.open.push(OpenEnd { element: e.id, port: e.port, at: *at, reason: reason.clone() });
                }
            }
        }
    }
    // The placeholders' links go; conversion connects what meets.
    let guids: Vec<String> = ids.iter().map(|id| store.get(*id).expect("listed").guid().to_string()).collect();
    let links: Vec<mep::Link> = mep::stored_links(&store).into_iter().filter(|l| !guids.contains(&l.a.element) && !guids.contains(&l.b.element)).collect();
    mep::save_links(txn, links)?;
    for (id, s) in &recipes {
        let u = unit(sub(s.end, s.start)).expect("a segment");
        let (t0, t1) = (trims.get(&(*id, 0)).copied().unwrap_or(0.0), trims.get(&(*id, 1)).copied().unwrap_or(0.0));
        if t0 + t1 > len(sub(s.end, s.start)) - 0.01 {
            return Err(bad("mep_run.too_short", format!("placeholder {id} is too short for the fittings at its ends")));
        }
        let mut r = s.clone();
        r.placeholder = false;
        r.start = add(s.start, mul(u, t0));
        r.end = sub(s.end, mul(u, t1));
        write(txn, *id, &r)?;
        report.segments.push(*id);
    }
    let mut classes = Vec::new();
    for (cx, at, pl) in fittings {
        report.fittings.push(mep_runs::make_fitting(txn, &cx, pl.part, at, pl.legs)?);
        classes.push(cx.classification);
    }
    let made: Vec<ElementId> = report.segments.iter().chain(&report.fittings).copied().collect();
    mep::connect_coincident(txn, &made)?;
    // The ends left open stay open, even where two happen to face.
    if !open_ports.is_empty() {
        let now = txn.store().clone();
        let open: Vec<mep::StoredPort> = open_ports.iter().map(|(id, k)| mep::stored(&now, *id, *k)).collect();
        let links: Vec<mep::Link> = mep::stored_links(&now).into_iter().filter(|l| !open.contains(&l.a) && !open.contains(&l.b)).collect();
        mep::save_links(txn, links)?;
    }
    // The fittings join the system of what they now connect.
    for (id, c) in report.fittings.clone().into_iter().zip(classes) {
        mep_runs::join_system(txn, &[id], c)?;
    }
    // Ends that met equipment and no longer do.
    let partners = mep::partners(txn.store());
    for (at, ends) in &points {
        for e in ends {
            if joined_out(e) && !open_ports.contains(&(e.id, e.port)) && !partners.contains_key(&(e.id, e.port)) {
                open_ports.push((e.id, e.port));
                report.open.push(OpenEnd {
                    element: e.id,
                    port: e.port,
                    at: *at,
                    reason: "the connector it met does not face it at the same size".into(),
                });
            }
        }
    }
    for o in &report.open {
        mep_runs::warn(
            txn,
            "placeholder.open_end",
            format!("placeholder {} is left open at ({:.3}, {:.3}, {:.3}): {}", o.element, o.at[0], o.at[1], o.at[2], o.reason),
            vec![o.element],
        );
    }
    report.element_ids = made;
    Ok(report)
}
