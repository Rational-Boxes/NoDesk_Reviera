// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Automatic precast reinforcement (STR-PRE-003).
//!
//! The configuration's reinforcement types (per piece kind, each for a thickness range) give a
//! piece its bars:
//!
//! - **Area bars**: for a wall piece, vertical and horizontal bars at both faces (the vertical
//!   ones outside), clipped to the piece less the edge cover and cut round its openings with the
//!   cover; for a slab piece, bars both ways at the top and the bottom.
//! - **Edge U-bars** (walls): round each opening side and each free edge of the piece, the web
//!   across the thickness at the edge cover and both legs running into the piece by the type's
//!   leg, spaced along the edge.
//!
//! Generated sets are hosted by the element split (so they keep its cover) and listed on the
//! piece's assembly, which holds them in IFC; their partition is the piece mark (STR-RBR-016).
//! Each carries `rxd:precast_generated` (the assembly, the type, and a hash of its recipe as made).
//! Running again removes the generated sets that are as made and keeps those edited since, and
//! any added by hand, reporting them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::precast::{self, AssemblyRecipe, PieceKind};
use crate::store::Store;
use crate::txn::Transaction;

pub const GENERATED_KEY: &str = "rxd:precast_generated";

type V3 = [f64; 3];

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

fn fxhash(s: &str) -> String {
    format!("{:016x}", s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)))
}

/// A set's recipe as a hash, to tell whether it was edited since it was made.
fn recipe_hash(e: &Element) -> String {
    fxhash(&e.param(crate::walls::RECIPE_KEY).map(Value::to_string).unwrap_or_default())
}

/// The assembly a generated set belongs to (GlobalId).
pub fn generated_for(e: &Element) -> Option<String> {
    e.param(GENERATED_KEY)?.get("assembly")?.as_str().map(str::to_owned)
}

/// Whether an element goes with `guid`: generated reinforcement of that assembly.
pub fn uses(e: &Element, guid: &str) -> bool {
    generated_for(e).as_deref() == Some(guid)
}

fn is_wall(k: PieceKind) -> bool {
    matches!(k, PieceKind::SolidWall | PieceKind::DoubleWall)
}

fn kind_config(c: &precast::Config, k: PieceKind) -> &precast::ReinforcementConfig {
    match k {
        PieceKind::SolidWall => &c.wall.solid.reinforcement,
        PieceKind::DoubleWall => &c.wall.double.reinforcement,
        PieceKind::SolidSlab => &c.slab.solid.reinforcement,
        PieceKind::HollowCore => &c.slab.hollow_core.reinforcement,
        PieceKind::LatticeGirder => &c.slab.lattice_girder.reinforcement,
    }
}

/// A piece's thickness: its wall's width or its floor's thickness.
fn thickness(store: &Store, a: &AssemblyRecipe) -> Result<f64> {
    let src = by_guid(store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
    if is_wall(a.piece) {
        Ok(crate::walls::regenerate(store, src)?.width)
    } else {
        Ok(crate::floors::regenerate(store, src)?.thickness)
    }
}

/// A piece's points (its parts' bodies).
fn piece_points(store: &Store, a: &AssemblyRecipe, bodies: &[crate::bodies::Body]) -> Vec<V3> {
    a.parts
        .iter()
        .filter_map(|g| by_guid(store, g))
        .flat_map(|e| crate::parts::geometry(store, e, bodies).into_iter().flat_map(|m| m.0))
        .collect()
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TypesParams {
    pub assembly_ids: Vec<ElementRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OfferedTypes {
    /// The pieces' thickness range (metres).
    pub thickness: [f64; 2],
    pub area_types: Vec<precast::AreaType>,
    pub edge_types: Vec<precast::EdgeType>,
}

fn assemblies(store: &Store, ids: &[ElementRef]) -> Result<Vec<(ElementId, AssemblyRecipe)>> {
    let out: Vec<(ElementId, AssemblyRecipe)> = ids
        .iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let a = store.get(id).and_then(precast::assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {id} is not a precast assembly")))?;
            Ok((id, a))
        })
        .collect::<Result<_>>()?;
    let Some((_, first)) = out.first() else {
        return Err(Error::new("precast.not_assembly", "select precast assemblies"));
    };
    if let Some((id, a)) = out.iter().find(|(_, a)| a.piece != first.piece) {
        return Err(Error::new(
            "precast.mixed",
            format!("assembly {id} is a {} and the first a {}: reinforce one kind at a time", a.piece.name(), first.piece.name()),
        ));
    }
    Ok(out)
}

/// The reinforcement types the selected pieces' thickness allows (`precast.reinforcement_types`).
pub fn offered(store: &Store, p: TypesParams) -> Result<OfferedTypes> {
    let list = assemblies(store, &p.assembly_ids)?;
    let ts: Vec<f64> = list.iter().map(|(_, a)| thickness(store, a)).collect::<Result<_>>()?;
    let (lo, hi) = (ts.iter().copied().fold(f64::INFINITY, f64::min), ts.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let c = precast::config(store);
    let r = kind_config(&c, list[0].1.piece);
    let fits = |a: f64, b: f64| a <= lo + 1e-9 && hi <= b + 1e-9;
    Ok(OfferedTypes {
        thickness: [lo, hi],
        area_types: r.area_types.iter().filter(|t| fits(t.min_thickness, t.max_thickness)).cloned().collect(),
        edge_types: if is_wall(list[0].1.piece) { r.edge_types.iter().filter(|t| fits(t.min_thickness, t.max_thickness)).cloned().collect() } else { Vec::new() },
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReinforceParams {
    pub assembly_ids: Vec<ElementRef>,
    /// An area type's id from the configuration.
    pub area_type_id: String,
    /// Walls: an edge type's id.
    #[serde(default)]
    pub edge_type_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Kept {
    pub id: ElementId,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AssemblyReport {
    pub assembly: ElementId,
    pub mark: String,
    pub created: Vec<ElementId>,
    /// Generated sets removed because they were as made.
    pub removed: usize,
    pub kept: Vec<Kept>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Report {
    pub assemblies: Vec<AssemblyReport>,
}

/// Places a set and marks it generated for the assembly.
fn generated(txn: &mut Transaction<'_>, made: Result<ElementId>, assembly: &str, mark: &str, type_id: &str) -> Result<ElementId> {
    let id = made?;
    txn.set_param(id, crate::rebar_numbers::PARTITION_KEY, json!(mark))?;
    let e = txn.store().get(id).expect("made");
    let hash = recipe_hash(e);
    txn.set_param(id, GENERATED_KEY, json!({ "assembly": assembly, "type": type_id, "hash": hash }))?;
    Ok(id)
}

/// Reinforces precast pieces (`precast.reinforce`).
pub(crate) fn reinforce(txn: &mut Transaction<'_>, p: ReinforceParams) -> Result<Report> {
    let store = txn.store().clone();
    let list = assemblies(&store, &p.assembly_ids)?;
    let c = precast::config(&store);
    let piece = list[0].1.piece;
    let rc = kind_config(&c, piece).clone();
    let area = rc.area_types.iter().find(|t| t.id == p.area_type_id).cloned().ok_or_else(|| {
        Error::new("precast.type", format!("no area type {} for a {} (types: {})", p.area_type_id, piece.name(), rc.area_types.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")))
    })?;
    let edge = match &p.edge_type_id {
        Some(e) if is_wall(piece) => Some(rc.edge_types.iter().find(|t| t.id == *e).cloned().ok_or_else(|| Error::new("precast.type", format!("no edge type {e} for a {}", piece.name())))?),
        Some(_) => return Err(Error::new("precast.type", "edge U-bars are for wall pieces")),
        None => None,
    };
    for b in std::iter::once(&area.bar_type).chain(edge.as_ref().map(|e| &e.bar_type)) {
        crate::rebar::bar_type(&store, b)?;
    }
    let mut report = Report { assemblies: Vec::new() };
    for (aid, a) in list {
        let store = txn.store().clone();
        let t = thickness(&store, &a)?;
        let check = |name: &str, lo: f64, hi: f64| -> Result<()> {
            if t < lo - 1e-9 || t > hi + 1e-9 {
                return Err(Error::new(
                    "precast.thickness",
                    format!("type {name} covers {:.0}–{:.0} mm; piece {} is {:.0} mm", lo * 1000.0, hi * 1000.0, a.mark, t * 1000.0),
                ));
            }
            Ok(())
        };
        check(&area.id, area.min_thickness, area.max_thickness)?;
        if let Some(e) = &edge {
            check(&e.id, e.min_thickness, e.max_thickness)?;
        }
        let aguid = store.get(aid).expect("resolved").guid().to_string();
        let mut r = AssemblyReport { assembly: aid, mark: a.mark.clone(), created: Vec::new(), removed: 0, kept: Vec::new(), skipped: Vec::new() };
        // The last run's sets: removed if as made, kept if edited; sets added by hand stay.
        // Custom sheets (STR-PRE-004) stay listed; they have their own command.
        let mut still: Vec<String> = a.reinforcement.iter().filter(|g| by_guid(&store, g).is_some_and(|e| crate::precast_fabric::recipe(e).is_some())).cloned().collect();
        for e in store.iter().filter(|e| generated_for(e).as_deref() == Some(aguid.as_str())) {
            let made = e.param(GENERATED_KEY).and_then(|g| g.get("hash")).and_then(Value::as_str).unwrap_or("");
            if made == recipe_hash(e) {
                txn.delete(e.id())?;
                r.removed += 1;
            } else {
                r.kept.push(Kept { id: e.id(), why: "generated, then edited: kept".into() });
                still.push(e.guid().to_string());
            }
        }
        let src = by_guid(&store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
        let host = src.id();
        for e in store.iter().filter(|e| crate::rebar::rebar_recipe(e).is_some_and(|x| x.host == a.source) && generated_for(e).is_none() && crate::rebar_numbers::partition(e) == a.mark) {
            r.kept.push(Kept { id: e.id(), why: "added by hand: kept".into() });
        }
        let bodies = crate::bodies::all_but_reinforcement(&store);
        let pts = piece_points(&store, &a, &bodies);
        if pts.is_empty() {
            r.skipped.push("the piece has no body".into());
            report.assemblies.push(r);
            continue;
        }
        let place = |txn: &mut Transaction<'_>, placement: crate::rebar::Placement, bar: &str, spacing: f64| {
            crate::rebar::place(
                txn,
                crate::rebar::PlaceParams {
                    host_id: ElementRef::Id(host),
                    shape: "00".into(),
                    bar_type: bar.into(),
                    method: "expand".into(),
                    placement,
                    layout: crate::rebar::Layout { rule: crate::rebar::Rule::MaximumSpacing, spacing: Some(spacing), ..Default::default() },
                    varying: false,
                },
            )
        };
        let mut made: Vec<(String, Result<ElementId>)> = Vec::new();
        let area_d = crate::rebar::bar_type(&store, &area.bar_type)?.diameter;
        if is_wall(piece) {
            let g = crate::walls::regenerate(&store, src)?;
            let dir = g.direction();
            let along = |q: &V3| (q[0] - g.start[0]) * dir[0] + (q[1] - g.start[1]) * dir[1];
            let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(along(q)), h.max(along(q))));
            let (z0, z1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q[2] - g.base_z), h.max(q[2] - g.base_z)));
            let boundary = vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
            for face in ["exterior", "interior"] {
                for (vertical, offset) in [(true, 0.0), (false, area_d)] {
                    let pl = crate::rebar::Placement::Layer { face: face.into(), angle: 0.0, vertical, offset, boundary: Some(boundary.clone()), hook: None, hook_flip: false };
                    let what = format!("{face} {} bars", if vertical { "vertical" } else { "horizontal" });
                    made.push((what, place(txn, pl, &area.bar_type, area.spacing)));
                }
            }
            if let Some(e) = &edge {
                let d = crate::rebar::bar_type(&store, &e.bar_type)?.diameter;
                let cuts = crate::inserts::cuts_by_host(&store).remove(&host).unwrap_or_default();
                for (what, res) in edge_bars(txn, &store, src, &g, e, d, [x0, x1, z0, z1], &cuts) {
                    made.push((what, res));
                }
            }
        } else {
            let gf = crate::floors::regenerate(&store, src)?;
            let outer = &gf.profiles.first().ok_or_else(|| Error::new("precast.source", "the floor has no outline"))?.outer;
            // The strip's direction: the outline's longest edge, as the split takes it.
            let (mut axis, mut best) = ([1.0, 0.0], 0.0);
            for i in 0..outer.len() {
                let (p0, p1) = (outer[i], outer[(i + 1) % outer.len()]);
                let dd = [p1[0] - p0[0], p1[1] - p0[1]];
                let l = dd[0].hypot(dd[1]);
                if l > best {
                    best = l;
                    axis = [dd[0] / l, dd[1] / l];
                }
            }
            let perp = [-axis[1], axis[0]];
            let pu = |q: &V3| q[0] * axis[0] + q[1] * axis[1];
            let pv = |q: &V3| q[0] * perp[0] + q[1] * perp[1];
            let (u0, u1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(pu(q)), h.max(pu(q))));
            let (v0, v1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(pv(q)), h.max(pv(q))));
            let at = |u: f64, v: f64| [axis[0] * u + perp[0] * v, axis[1] * u + perp[1] * v];
            let boundary = vec![at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)];
            let angle = axis[1].atan2(axis[0]).to_degrees();
            for face in ["bottom", "top"] {
                for (a2, offset) in [(angle + 90.0, 0.0), (angle, area_d)] {
                    let pl = crate::rebar::Placement::Layer { face: face.into(), angle: a2, vertical: false, offset, boundary: Some(boundary.clone()), hook: None, hook_flip: false };
                    made.push((format!("{face} bars at {a2:.0}°"), place(txn, pl, &area.bar_type, area.spacing)));
                }
            }
        }
        for (what, res) in made {
            let type_id = if what.starts_with("U-bars") { edge.as_ref().map_or("", |e| e.id.as_str()).to_owned() } else { area.id.clone() };
            match generated(txn, res, &aguid, &a.mark, &type_id) {
                Ok(id) => {
                    r.created.push(id);
                    still.push(txn.store().get(id).expect("made").guid().to_string());
                }
                Err(e) => r.skipped.push(format!("{what}: {}", e.message)),
            }
        }
        // The assembly lists what it holds now.
        let mut rec = txn.store().get(aid).and_then(precast::assembly_recipe).expect("an assembly");
        rec.reinforcement = still;
        txn.set_param(aid, crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        report.assemblies.push(r);
    }
    crate::rebar_numbers::persist(txn)?;
    Ok(report)
}

/// An edge for U-bars: its name, start and end (along and up), and the way into the concrete.
type Edge = (String, [f64; 2], [f64; 2], [f64; 2]);

/// U-bars round a wall piece's free edges and its openings' sides, each sketched square to its
/// edge: the web across the thickness at the edge cover, the legs into the piece.
#[allow(clippy::too_many_arguments)]
fn edge_bars(
    txn: &mut Transaction<'_>,
    store: &Store,
    wall: &Element,
    g: &crate::walls::WallGeometry,
    t: &precast::EdgeType,
    d: f64,
    [x0, x1, z0, z1]: [f64; 4],
    cuts: &[crate::walls::openings::Cut],
) -> Vec<(String, Result<ElementId>)> {
    let cover = |f: &str| crate::rebar::cover(store, wall, crate::rebar::HostKind::Wall, f);
    let edge_c = cover("other") + d / 2.0;
    // Openings in the piece, as rectangles along and up (from the base).
    let holes: Vec<[f64; 4]> = cuts
        .iter()
        .map(|c| [c.x[0], c.x[1], c.z[0] - g.base_z, c.z[1] - g.base_z])
        .filter(|h| h[1] > x0 + 1e-6 && h[0] < x1 - 1e-6 && h[3] > z0 + 1e-6 && h[2] < z1 - 1e-6)
        .collect();
    // Edges: (start, end) along and up, and the way into the concrete.
    let mut edges: Vec<Edge> = Vec::new();
    // A horizontal piece edge, less where openings meet it.
    let horizontal = |name: &str, z: f64, inward: f64, edges: &mut Vec<Edge>| {
        let mut spans = vec![(x0, x1)];
        for h in &holes {
            if h[2] <= z + 1e-6 && h[3] >= z - 1e-6 {
                spans = spans.into_iter().flat_map(|(a, b)| [(a, b.min(h[0])), (a.max(h[1]), b)]).filter(|(a, b)| b - a > 2.0 * edge_c).collect();
            }
        }
        for (a, b) in spans {
            edges.push((name.to_owned(), [a, z], [b, z], [0.0, inward]));
        }
    };
    horizontal("U-bars at the piece's bottom", z0, 1.0, &mut edges);
    horizontal("U-bars at the piece's top", z1, -1.0, &mut edges);
    edges.push(("U-bars at the piece's start".into(), [x0, z0], [x0, z1], [1.0, 0.0]));
    edges.push(("U-bars at the piece's end".into(), [x1, z0], [x1, z1], [-1.0, 0.0]));
    for h in &holes {
        let (a, b) = (h[2].max(z0), h[3].min(z1));
        edges.push(("U-bars at an opening's side".into(), [h[0], a], [h[0], b], [-1.0, 0.0]));
        edges.push(("U-bars at an opening's side".into(), [h[1], a], [h[1], b], [1.0, 0.0]));
        if h[3] < z1 - 1e-6 {
            edges.push(("U-bars over an opening".into(), [h[0].max(x0), h[3]], [h[1].min(x1), h[3]], [0.0, 1.0]));
        }
        if h[2] > z0 + 1e-6 {
            edges.push(("U-bars under an opening".into(), [h[0].max(x0), h[2]], [h[1].min(x1), h[2]], [0.0, -1.0]));
        }
    }
    let (dir, nrm) = (g.direction(), g.normal());
    let world = |x: f64, z: f64| [g.start[0] + dir[0] * x, g.start[1] + dir[1] * x, g.base_z + z];
    let to3 = |v: [f64; 2]| [dir[0] * v[0], dir[1] * v[0], v[1]];
    let (s_ext, s_int) = (g.width / 2.0 - cover("exterior") - d / 2.0, -(g.width / 2.0 - cover("interior") - d / 2.0));
    let mut out = Vec::new();
    for (name, a, b, inward) in edges {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let usable = len - 2.0 * edge_c;
        if usable < 0.0 {
            continue;
        }
        let e = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let start = [a[0] + e[0] * edge_c, a[1] + e[1] * edge_c];
        let origin = world(start[0], start[1]);
        let (x, normal) = (to3(inward), to3(e));
        let Ok(plane) = crate::work_planes::Plane::new(origin, x, normal) else { continue };
        // The plane's v runs across the wall one way or the other.
        let sign = plane.y[0] * nrm[0] + plane.y[1] * nrm[1];
        let (ve, vi) = (s_ext * sign, s_int * sign);
        let (uw, leg) = (cover("other") + d / 2.0, t.leg.max(cover("other") + 2.0 * d));
        let line = |p: [f64; 2], q: [f64; 2]| json!({ "line": { "start": p, "end": q } });
        let n = (usable / t.spacing + 1e-9).floor() as usize + 1;
        let res = crate::rebar_sketch::sketch(
            txn,
            serde_json::from_value(json!({
                "host_id": wall.id().0,
                "plane": { "origin": origin, "x": x, "normal": normal },
                "curves": [line([leg, ve], [uw, ve]), line([uw, ve], [uw, vi]), line([uw, vi], [leg, vi])],
                "bar_type": t.bar_type,
                "layout": { "rule": "number_with_spacing", "n": n, "spacing": if n > 1 { usable / (n - 1) as f64 } else { t.spacing } },
            }))
            .expect("valid sketch parameters"),
        );
        out.push((name, res));
    }
    out
}
