// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mounting parts for precast pieces (STR-PRE-005): lifting anchors from rules.
//!
//! Each kind's `connections` rules give weight bands (up to a weight, so many anchors), the part
//! and its size, and the distances to keep from the piece's ends and from openings. A piece's
//! weight is its parts' volume (openings removed) × the configured density; a piece heavier than
//! the last band gets no anchors and is reported. Anchors go on a wall piece's top edge (in the
//! middle of its thickness) or a slab piece's top face, symmetric about the piece's centre of
//! gravity; when no symmetric places keep the distances, the nearest valid places are used and a
//! warning given.
//!
//! An anchor is an `IfcMechanicalFastener` (USERDEFINED `LIFTING_ANCHOR`) with
//! `recipe.mounting_part` (its assembly, point and size), belonging to the piece's assembly. It
//! moves like any element; running again replaces anchors as placed and keeps, and reports, those
//! moved since and those placed by hand.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::precast::{self, PieceKind};
use crate::store::Store;
use crate::txn::Transaction;
use crate::NewElement;

pub const RECIPE: &str = "recipe.mounting_part";

type V3 = [f64; 3];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeightBand {
    /// Up to this weight (tonnes) …
    pub max_weight: f64,
    /// … so many anchors.
    pub count: usize,
}

/// A kind's mounting-part rules (the configuration's `connections` node).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MountingRules {
    /// The part's name (an anchor of the supplier's range).
    pub part: String,
    /// Ascending, contiguous from 0.
    pub bands: Vec<WeightBand>,
    /// From the piece's ends (and a slab's edges), and from openings (metres).
    pub edge_distance: f64,
    pub opening_distance: f64,
    /// The anchor's body: diameter and depth into the concrete.
    pub diameter: f64,
    pub depth: f64,
}

impl Default for MountingRules {
    /// Provisional starting rules (EXPERT_REVIEW.md E-15).
    fn default() -> Self {
        Self {
            part: "Lifting anchor".into(),
            bands: vec![WeightBand { max_weight: 10.0, count: 2 }, WeightBand { max_weight: 16.0, count: 4 }],
            edge_distance: 0.3,
            opening_distance: 0.3,
            diameter: 0.03,
            depth: 0.2,
        }
    }
}

/// What is wrong with a kind's rules.
pub fn rule_problems(kind: &str, r: &MountingRules) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let path = format!("{kind}.connections");
    let mut last = 0.0;
    for b in &r.bands {
        if b.max_weight <= last {
            out.push((format!("{path}.bands"), "weight bands must be contiguous and ascending, each heavier than the one before".into()));
            break;
        }
        if b.count == 0 {
            out.push((format!("{path}.bands"), "each weight band places at least one part".into()));
        }
        last = b.max_weight;
    }
    for (k, v) in [("edge_distance", r.edge_distance), ("opening_distance", r.opening_distance)] {
        if v < 0.0 {
            out.push((format!("{path}.{k}"), "distances cannot be negative".into()));
        }
    }
    if r.diameter <= 0.0 || r.depth <= 0.0 {
        out.push((format!("{path}.diameter"), "the part's diameter and depth must be more than 0".into()));
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MountingPartRecipe {
    pub kind: String,
    /// The piece's assembly (GlobalId).
    pub assembly: String,
    /// `lifting_anchor`.
    pub part: String,
    pub name: String,
    /// The anchor's head (world) and its size; it runs down by `depth`.
    pub point: V3,
    pub diameter: f64,
    pub depth: f64,
}

pub fn recipe(e: &Element) -> Option<MountingPartRecipe> {
    let v = e.param(crate::walls::RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Whether an element goes with `guid`: a part of that assembly.
pub fn uses(e: &Element, guid: &str) -> bool {
    recipe(e).is_some_and(|r| r.assembly == guid)
}

/// A part's body: a rod down from its head.
pub fn mesh(e: &Element) -> Option<crate::bodies::Mesh> {
    let r = recipe(e)?;
    let p = r.point;
    Some(crate::steel_fab::cylinder([p[0], p[1], p[2] - r.depth], p, r.diameter / 2.0, 12))
}

/// Moves a part (with element.move, .rotate, .mirror): `f` maps plan points; `dz` lifts.
pub(crate) fn transform(txn: &mut Transaction<'_>, id: ElementId, f: &dyn Fn([f64; 2]) -> [f64; 2], dz: f64) -> Result<()> {
    let mut r = txn.store().get(id).and_then(recipe).ok_or_else(|| Error::new("element.not_found", "not a mounting part"))?;
    let q = f([r.point[0], r.point[1]]);
    r.point = [q[0], q[1], r.point[2] + dz];
    txn.set_param(id, crate::walls::RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

fn rules(c: &precast::Config, k: PieceKind) -> &MountingRules {
    match k {
        PieceKind::SolidWall => &c.wall.solid.connections,
        PieceKind::DoubleWall => &c.wall.double.connections,
        PieceKind::SolidSlab => &c.slab.solid.connections,
        PieceKind::HollowCore => &c.slab.hollow_core.connections,
        PieceKind::LatticeGirder => &c.slab.lattice_girder.connections,
    }
}

/// A closed mesh's volume and centroid.
pub(crate) fn volume_centroid(meshes: &[crate::bodies::Mesh]) -> (f64, V3) {
    let (mut v, mut c) = (0.0, [0.0; 3]);
    for (pts, idx) in meshes {
        for t in idx.chunks(3) {
            let (a, b, d) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let vol = (a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0]) + a[2] * (b[0] * d[1] - b[1] * d[0])) / 6.0;
            v += vol;
            for k in 0..3 {
                c[k] += vol * (a[k] + b[k] + d[k]) / 4.0;
            }
        }
    }
    if v.abs() < 1e-12 {
        return (0.0, [0.0; 3]);
    }
    (v.abs(), c.map(|x| x / v))
}

/// Places along a run from `lo` to `hi` (one axis): `n` symmetric about `cg`, spread over the run,
/// drawn in until every place is allowed; else each the nearest allowed place.
fn places(n: usize, cg: f64, lo: f64, hi: f64, allowed: &dyn Fn(f64) -> bool) -> (Vec<f64>, bool) {
    let span = hi - lo;
    let ideal = |s: f64| (0..n).map(|k| cg + (k as f64 - (n as f64 - 1.0) / 2.0) * span / n as f64 * s).collect::<Vec<_>>();
    for step in 0..=18 {
        let s = 1.0 - step as f64 * 0.05;
        let xs = ideal(s);
        if xs.iter().all(|x| allowed(*x)) {
            return (xs, true);
        }
    }
    // The nearest allowed place for each, searching outward in 10 mm steps.
    let xs = ideal(1.0)
        .into_iter()
        .map(|x| {
            (0..2000)
                .flat_map(|k| [x + k as f64 * 0.01, x - k as f64 * 0.01])
                .find(|c| allowed(*c))
                .unwrap_or(x)
        })
        .collect();
    (xs, false)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlaceParams {
    pub assembly_ids: Vec<ElementRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PartsReport {
    pub assembly: ElementId,
    pub mark: String,
    /// The piece's weight (t) and centre of gravity.
    pub weight: f64,
    pub centre: V3,
    pub created: Vec<ElementId>,
    pub removed: usize,
    pub kept: Vec<crate::precast_rebar::Kept>,
    pub warnings: Vec<String>,
}

fn hash(e: &Element) -> String {
    let s = e.param(crate::walls::RECIPE_KEY).map(Value::to_string).unwrap_or_default();
    format!("{:016x}", s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)))
}

/// Places mounting parts on precast pieces (`precast.place_mounting_parts`).
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<Vec<PartsReport>> {
    let mut out = Vec::new();
    for r in &p.assembly_ids {
        let store = txn.store().clone();
        let aid = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let a = store.get(aid).and_then(precast::assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {aid} is not a precast assembly")))?;
        let c = precast::config(&store);
        let rule = rules(&c, a.piece).clone();
        let aguid = store.get(aid).expect("resolved").guid().to_string();
        let bodies = crate::bodies::all_but_reinforcement(&store);
        let meshes: Vec<crate::bodies::Mesh> = a.parts.iter().filter_map(|g| by_guid(&store, g)).flat_map(|e| crate::parts::geometry(&store, e, &bodies)).collect();
        let (volume, cg) = volume_centroid(&meshes);
        let weight = volume * c.density;
        let mut rep = PartsReport { assembly: aid, mark: a.mark.clone(), weight, centre: cg, created: Vec::new(), removed: 0, kept: Vec::new(), warnings: Vec::new() };
        // The last run's parts: removed if as placed, kept if moved; parts placed by hand stay.
        let mut held: Vec<String> = Vec::new();
        for e in store.iter().filter(|e| recipe(e).is_some_and(|x| x.assembly == aguid)) {
            let made = e.param(crate::precast_rebar::GENERATED_KEY).and_then(|g| g.get("hash")).and_then(Value::as_str);
            match made {
                Some(h) if h == hash(e) => {
                    txn.delete(e.id())?;
                    rep.removed += 1;
                }
                Some(_) => {
                    rep.kept.push(crate::precast_rebar::Kept { id: e.id(), why: "placed by the rules, then moved: kept".into() });
                    held.push(e.guid().to_string());
                }
                None => {
                    rep.kept.push(crate::precast_rebar::Kept { id: e.id(), why: "placed by hand: kept".into() });
                    held.push(e.guid().to_string());
                }
            }
        }
        let band = rule.bands.iter().find(|b| weight <= b.max_weight + 1e-9);
        let Some(band) = band else {
            rep.warnings.push(format!(
                "{:.1} t is heavier than the last weight band ({:.1} t): no parts placed",
                weight,
                rule.bands.last().map_or(0.0, |b| b.max_weight)
            ));
            set_held(txn, aid, held)?;
            out.push(rep);
            continue;
        };
        let src = by_guid(&store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
        let pts: Vec<V3> = meshes.iter().flat_map(|m| m.0.iter().copied()).collect();
        let top = pts.iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max);
        let mut heads: Vec<V3> = Vec::new();
        let mut symmetric = true;
        if matches!(a.piece, PieceKind::SolidWall | PieceKind::DoubleWall) {
            let g = crate::walls::regenerate(&store, src)?;
            let dir = g.direction();
            let along = |q: &V3| (q[0] - g.start[0]) * dir[0] + (q[1] - g.start[1]) * dir[1];
            let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(along(q)), h.max(along(q))));
            // Openings reaching up near the top edge keep anchors away.
            let near_top: Vec<[f64; 2]> = crate::inserts::cuts_by_host(&store)
                .remove(&src.id())
                .unwrap_or_default()
                .iter()
                .filter(|c| c.z[1] >= top - rule.depth - rule.opening_distance)
                .map(|c| [c.x[0] - rule.opening_distance, c.x[1] + rule.opening_distance])
                .collect();
            let allowed = |x: f64| x >= x0 + rule.edge_distance - 1e-9 && x <= x1 - rule.edge_distance + 1e-9 && !near_top.iter().any(|o| x > o[0] + 1e-9 && x < o[1] - 1e-9);
            let (xs, sym) = places(band.count, along(&cg), x0, x1, &allowed);
            symmetric = sym;
            // On the wall's location line, the middle of its thickness.
            heads = xs.iter().map(|x| [g.start[0] + dir[0] * x, g.start[1] + dir[1] * x, top]).collect();
        } else {
            let (lo, hi) = pts.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1])], [h[0].max(q[0]), h[1].max(q[1])]));
            let along_x = hi[0] - lo[0] >= hi[1] - lo[1];
            let (ax, bx) = if along_x { (0, 1) } else { (1, 0) };
            let fl = crate::floors::regenerate(&store, src)?;
            let holes: Vec<[f64; 4]> = crate::slab_openings::cut_profiles(&store, src, &fl.profiles)
                .iter()
                .flat_map(|p| p.holes.iter())
                .map(|h| h.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].min(q[0]), b[1].max(q[0]), b[2].min(q[1]), b[3].max(q[1])]))
                .collect();
            let clear = |x: f64, y: f64| !holes.iter().any(|h| x > h[0] - rule.opening_distance && x < h[1] + rule.opening_distance && y > h[2] - rule.opening_distance && y < h[3] + rule.opening_distance);
            // Rows of anchors along the long side: one row for two, two rows for more.
            let rows = if band.count > 2 { 2 } else { 1 };
            let per = band.count.div_ceil(rows);
            let (cy_lo, cy_hi) = (lo[bx] + rule.edge_distance, hi[bx] - rule.edge_distance);
            let row_at: Vec<f64> = if rows == 1 { vec![cg[bx]] } else { let q = (hi[bx] - lo[bx]) / 4.0; vec![cg[bx] - q, cg[bx] + q] };
            for y in row_at {
                let y = y.clamp(cy_lo, cy_hi);
                let allowed = |x: f64| {
                    x >= lo[ax] + rule.edge_distance - 1e-9 && x <= hi[ax] - rule.edge_distance + 1e-9 && if along_x { clear(x, y) } else { clear(y, x) }
                };
                let (xs, sym) = places(per, cg[ax], lo[ax], hi[ax], &allowed);
                symmetric &= sym;
                for x in xs {
                    let mut h = [0.0, 0.0, top];
                    h[ax] = x;
                    h[bx] = y;
                    heads.push(h);
                }
            }
        }
        if !symmetric {
            rep.warnings.push("no places symmetric about the centre of gravity keep the distances: the nearest valid places are used".into());
        }
        for (k, h) in heads.into_iter().enumerate() {
            let rec = MountingPartRecipe { kind: RECIPE.into(), assembly: aguid.clone(), part: "lifting_anchor".into(), name: rule.part.clone(), point: h, diameter: rule.diameter, depth: rule.depth };
            let id = txn.create(
                NewElement::new("IfcMechanicalFastener")
                    .name(format!("{} {} {}", a.mark, rule.part, k + 1))
                    .param("@PredefinedType", "USERDEFINED")
                    .param("@ObjectType", "LIFTING_ANCHOR")
                    .param(crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises")),
            )?;
            let e = txn.store().get(id).expect("made");
            let (h, g) = (hash(e), e.guid().to_string());
            txn.set_param(id, crate::precast_rebar::GENERATED_KEY, json!({ "assembly": aguid, "type": "mounting", "hash": h }))?;
            rep.created.push(id);
            held.push(g);
        }
        // The piece's weight on its assembly (kg).
        let qto = if matches!(a.piece, PieceKind::SolidWall | PieceKind::DoubleWall) { "Qto_WallBaseQuantities.NetWeight" } else { "Qto_SlabBaseQuantities.NetWeight" };
        txn.set_param(aid, qto, json!({ "type": "IfcMassMeasure", "value": (weight * 1000.0 * 10.0).round() / 10.0 }))?;
        set_held(txn, aid, held)?;
        out.push(rep);
    }
    Ok(out)
}

fn set_held(txn: &mut Transaction<'_>, aid: ElementId, held: Vec<String>) -> Result<()> {
    let mut rec = txn.store().get(aid).and_then(precast::assembly_recipe).expect("an assembly");
    rec.mounting = held;
    txn.set_param(aid, crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct HandParams {
    pub assembly_id: ElementRef,
    /// The part's head (world).
    pub point: V3,
}

/// Places a mounting part by hand (`mounting_part.place`): kept by later rule runs.
pub(crate) fn place_by_hand(txn: &mut Transaction<'_>, p: HandParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let aid = store.resolve(&p.assembly_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.assembly_id)))?;
    let a = store.get(aid).and_then(precast::assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {aid} is not a precast assembly")))?;
    let rule = rules(&precast::config(&store), a.piece).clone();
    let aguid = store.get(aid).expect("resolved").guid().to_string();
    let rec = MountingPartRecipe { kind: RECIPE.into(), assembly: aguid, part: "lifting_anchor".into(), name: rule.part.clone(), point: p.point, diameter: rule.diameter, depth: rule.depth };
    let id = txn.create(
        NewElement::new("IfcMechanicalFastener")
            .name(format!("{} {}", a.mark, rule.part))
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "LIFTING_ANCHOR")
            .param(crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises")),
    )?;
    let g = txn.store().get(id).expect("made").guid().to_string();
    let mut held = a.mounting.clone();
    held.push(g);
    set_held(txn, aid, held)?;
    Ok(id)
}
