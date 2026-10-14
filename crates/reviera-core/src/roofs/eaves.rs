// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Eaves and wall bearing (ARC-ROOF-010): an eave is a footprint edge with construction
//! settings. Each one meets the wall beneath it:
//! - **Bearing:** the roof's underside meets the plate height on the wall's exterior face, its
//!   core centre or its interior face. A **heel** height deeper than the roof's own plumb depth
//!   raises the roof's plane by the difference.
//! - **Overhang:** measured from the wall's exterior face, whatever the bearing.
//! - **Rafter cut** at the eave: plumb, square to the roof, or two-cut (a plumb face of the
//!   fascia depth, then a level cut).
//! - **Fascia** and **soffit** (boxed, sloped or open, returning to the wall's exterior face),
//!   built along the eave and mitred where eaves meet at hips. Gutters belong to the plumbing
//!   subsystem (they hang on the fascia and drain to downspouts), not to the eave.
//!
//! Generation works by moving each eave's plate line before the roof is generated. A plane
//! raised by `h` over the bearing line is the same plane at the plate height on a line
//! `h / tan(slope)` further out. The overhang beyond that line then reaches the eave line.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{FLAT, Mesh, RoofEdge, RoofGeometry, meet};
use crate::error::{Error, Result};
use crate::id::ElementRef;
use crate::sketch::{Lock, Sketch};
use crate::store::Store;

/// Where the roof bears on the wall below an eave.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Bearing {
    #[default]
    Exterior,
    Core,
    Interior,
}

/// The cut at a rafter's eave end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RafterCut {
    #[default]
    Plumb,
    Square,
    TwoCut,
}

/// The closure under an eave's overhang.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Soffit {
    #[default]
    Boxed,
    Sloped,
    Open,
}

fn default_fascia_depth() -> f64 {
    0.2
}

/// An eave's construction (per footprint edge, in the roof's recipe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Eave {
    #[serde(default)]
    pub bearing: Bearing,
    /// The roof construction's vertical depth at the bearing line; deeper than the roof's own
    /// plumb depth raises the roof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heel: Option<f64>,
    #[serde(default)]
    pub cut: RafterCut,
    #[serde(default = "default_fascia_depth")]
    pub fascia_depth: f64,
    #[serde(default)]
    pub fascia: bool,
    #[serde(default)]
    pub soffit: Soffit,
}

impl Default for Eave {
    fn default() -> Self {
        Self {
            bearing: Bearing::Exterior,
            heel: None,
            cut: RafterCut::Plumb,
            fascia_depth: default_fascia_depth(),
            fascia: false,
            soffit: Soffit::Boxed,
        }
    }
}

/// Fascia board thickness.
pub const FASCIA: f64 = 0.025;
/// Soffit board thickness.
pub const SOFFIT: f64 = 0.012;

/// What a trim along an eave is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrimKind {
    Fascia,
    Soffit,
}

/// A fascia or soffit along one eave.
#[derive(Debug, Clone, PartialEq)]
pub struct RoofTrim {
    pub kind: TrimKind,
    /// The eave's footprint curve.
    pub key: u32,
    pub mesh: Mesh,
}

/// The wall under footprint line `a`→`b` (outward to its right): the outward distance from the
/// line to the wall's exterior face, and the wall's width. A locked curve's wall is used when it
/// is one; otherwise the nearest parallel wall the line lies within or on.
pub fn wall_under(
    store: &Store,
    a: [f64; 2],
    b: [f64; 2],
    lock: Option<&Lock>,
) -> Option<(f64, f64)> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len < 1e-9 {
        return None;
    }
    let d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    let out = [d[1], -d[0]];
    let locked = lock
        .and_then(|l| store.find_by_guid(&crate::id::GlobalId::parse(&l.element)?))
        .filter(|id| store.get(*id).and_then(crate::walls::wall_recipe).is_some());
    let mut best: Option<(f64, f64, f64)> = None;
    for e in store.iter() {
        if locked.is_some_and(|l| l != e.id()) || crate::walls::wall_recipe(e).is_none() {
            continue;
        }
        let Ok(w) = crate::walls::regenerate(store, e) else {
            continue;
        };
        let u = w.direction();
        if (u[0] * d[1] - u[1] * d[0]).abs() > 1e-3 {
            continue;
        }
        let axis = (w.start[0] - a[0]) * out[0] + (w.start[1] - a[1]) * out[1];
        if locked.is_none() && axis.abs() > w.width / 2.0 + 1e-3 {
            continue;
        }
        // Overlapping along the line.
        let along = |p: [f64; 2]| (p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1];
        let (s0, s1) = (along(w.start), along(w.end));
        if s0.max(s1) < 1e-6 || s0.min(s1) > len - 1e-6 {
            continue;
        }
        if best.is_none_or(|b| axis.abs() < b.0) {
            best = Some((axis.abs(), axis + w.width / 2.0, w.width));
        }
    }
    best.map(|b| (b.1, b.2))
}

/// An eave's lines after its bearing is applied.
#[derive(Debug, Clone, Copy)]
pub struct EaveLine {
    /// How far the plate line moved out from the footprint curve.
    pub shift: f64,
    /// The overhang the user gave: from the wall's exterior face to the eave line.
    pub overhang: f64,
}

/// Applies each eave's bearing: the plate lines moved out (the corners where they meet again),
/// and the overhangs measured from those lines.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    store: &Store,
    sketch: &Sketch,
    loops: &[usize],
    ring: &[[f64; 2]],
    keys: &[u32],
    edges: &[RoofEdge],
    default_slope: f64,
    thickness: f64,
) -> (Vec<[f64; 2]>, Vec<RoofEdge>, Vec<Option<EaveLine>>) {
    let n = ring.len();
    let (prev_of, next_of) = neighbours(loops, n);
    let slope = |i: usize| {
        edges[i]
            .defines_slope
            .then(|| edges[i].slope.unwrap_or(default_slope))
    };
    let flat = (0..n).all(|i| slope(i).is_none_or(|s| s < FLAT));
    let mut out_edges = edges.to_vec();
    let mut lines = vec![None; n];
    for i in 0..n {
        let Some(eave) = &edges[i].eave else {
            continue;
        };
        let (a, b) = (ring[i], ring[next_of[i]]);
        let lock = sketch
            .curves
            .iter()
            .find(|c| c.key == keys[i])
            .and_then(|c| c.lock.as_ref());
        let (ext, width) = wall_under(store, a, b, lock).unwrap_or((0.0, 0.0));
        let overhang = edges[i].overhang;
        let shift = match slope(i) {
            Some(s) if !flat => {
                let bearing = match eave.bearing {
                    Bearing::Exterior => ext,
                    Bearing::Core => ext - width / 2.0,
                    Bearing::Interior => ext - width,
                };
                let raise = eave
                    .heel
                    .map_or(0.0, |h| (h - thickness / s.cos()).max(0.0));
                let shift = bearing + raise / s.max(1e-4).tan();
                out_edges[i].overhang = (ext + overhang - shift).max(0.0);
                shift
            }
            // Gables and flat roofs: the edge on the wall's exterior face, the overhang beyond.
            _ => ext,
        };
        lines[i] = Some(EaveLine { shift, overhang });
    }
    if lines.iter().all(Option::is_none) {
        return (ring.to_vec(), out_edges, lines);
    }
    let line = |i: usize| {
        let (a, b) = (ring[i], ring[next_of[i]]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-30);
        let d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let s = lines[i].map_or(0.0, |l| l.shift);
        ([a[0] + d[1] * s, a[1] - d[0] * s], d)
    };
    let corners = (0..n)
        .map(|i| {
            let (p, d) = line(prev_of[i]);
            let (q, e) = line(i);
            meet(p, d, q, e).unwrap_or(q)
        })
        .collect();
    (corners, out_edges, lines)
}

/// The previous and next corner of each corner, within its loop.
fn neighbours(loops: &[usize], n: usize) -> (Vec<usize>, Vec<usize>) {
    let mut prev_of = vec![0; n];
    let mut next_of = vec![0; n];
    let mut first = 0;
    for &len in loops {
        for k in 0..len {
            prev_of[first + k] = first + (k + len - 1) % len;
            next_of[first + k] = first + (k + 1) % len;
        }
        first += len;
    }
    (prev_of, next_of)
}

/// A vertical prism over the plan quad `q` (`[start outer, end outer, end inner, start inner]`)
/// whose bottom and top heights may vary over plan.
fn prism(q: &[[f64; 2]; 4], z0: &dyn Fn([f64; 2]) -> f64, z1: &dyn Fn([f64; 2]) -> f64) -> Mesh {
    let (mut points, indices) = crate::walls::prism(q, 0.0, 1.0);
    for p in &mut points {
        let at = [p[0], p[1]];
        p[2] = if p[2] < 0.5 { z0(at) } else { z1(at) };
    }
    (points, indices)
}

/// The fascias and soffits of a generated roof's eaves.
#[allow(clippy::too_many_arguments)]
pub fn trims(
    g: &RoofGeometry,
    loops: &[usize],
    ring: &[[f64; 2]],
    keys: &[u32],
    edges: &[RoofEdge],
    lines: &[Option<EaveLine>],
    default_slope: f64,
) -> Vec<RoofTrim> {
    let n = ring.len();
    let (prev_of, next_of) = neighbours(loops, n);
    let t = g.thickness;
    let slope = |i: usize| {
        edges[i]
            .defines_slope
            .then(|| edges[i].slope.unwrap_or(default_slope))
    };
    let flat = (0..n).all(|i| slope(i).is_none_or(|s| s < FLAT));
    let sloped = |i: usize| !flat && slope(i).is_some();
    let dir = |i: usize| {
        let (a, b) = (ring[i], ring[next_of[i]]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-30);
        [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
    };
    let out = |i: usize| {
        let d = dir(i);
        [d[1], -d[0]]
    };
    // The eave line: the plate line moved out by the generated overhang.
    let eave_line = |i: usize, o: f64| {
        let (p, m) = (ring[i], out(i));
        let o = edges[i].overhang + o;
        ([p[0] + m[0] * o, p[1] + m[1] * o], dir(i))
    };
    // Heights at the eave line: the underside and the top (plumb).
    let heights = |i: usize| -> (f64, f64) {
        match slope(i) {
            Some(s) if sloped(i) => {
                let ze = g.plate - edges[i].overhang * s.tan();
                (ze, ze + t / s.cos())
            }
            _ => (g.plate, g.plate + t),
        }
    };
    // The eave's extent: the roof's underside points on its eave line, at its height.
    let extent = |i: usize| -> Option<(f64, f64)> {
        let (p, d) = eave_line(i, 0.0);
        let m = out(i);
        let ze = heights(i).0;
        let along: Vec<f64> = g
            .faces
            .iter()
            .flat_map(|f| f.bottom.iter())
            .filter(|q| {
                ((q[0] - p[0]) * m[0] + (q[1] - p[1]) * m[1]).abs() < 1e-6
                    && (q[2] - ze).abs() < 1e-6
            })
            .map(|q| (q[0] - p[0]) * d[0] + (q[1] - p[1]) * d[1])
            .collect();
        let lo = along.iter().copied().fold(f64::MAX, f64::min);
        let hi = along.iter().copied().fold(f64::MIN, f64::max);
        (hi - lo > 1e-6).then_some((lo, hi))
    };
    let eave = |i: usize| edges[i].eave.as_ref().filter(|_| lines[i].is_some());
    // A band along eave `i` between offsets `inner` and `outer` from its eave line, mitred
    // against a neighbouring eave that has the same band (`has`), else square at the eave's ends.
    let band = |i: usize,
                has: &dyn Fn(usize) -> bool,
                inner: &dyn Fn(usize) -> f64,
                outer: &dyn Fn(usize) -> f64|
     -> Option<[[f64; 2]; 4]> {
        let (lo, hi) = extent(i)?;
        let end = |o: &dyn Fn(usize) -> f64, j: usize, s: f64| {
            let (p, d) = eave_line(i, o(i));
            let own = [p[0] + d[0] * s, p[1] + d[1] * s];
            if has(j) {
                let (q, e) = eave_line(j, o(j));
                meet(p, d, q, e).unwrap_or(own)
            } else {
                own
            }
        };
        let (prev, next) = (prev_of[i], next_of[i]);
        Some([
            end(outer, prev, lo),
            end(outer, next, hi),
            end(inner, next, hi),
            end(inner, prev, lo),
        ])
    };
    let mut trims = Vec::new();
    for i in 0..n {
        let Some(e) = eave(i) else { continue };
        let (ze, top) = heights(i);
        let ov = lines[i].map_or(0.0, |l| l.overhang);
        let soffit_z = if e.fascia {
            (top - e.fascia_depth).min(ze - SOFFIT)
        } else {
            ze - SOFFIT
        };
        let fascia_bottom = if e.soffit == Soffit::Boxed {
            soffit_z
        } else {
            (top - e.fascia_depth).min(ze)
        };
        if e.fascia {
            let has = |j: usize| eave(j).is_some_and(|f| f.fascia);
            if let Some(q) = band(i, &has, &|_| 0.0, &|_| FASCIA) {
                trims.push(RoofTrim {
                    kind: TrimKind::Fascia,
                    key: keys[i],
                    mesh: prism(&q, &|_| fascia_bottom, &|_| top),
                });
            }
        }
        let soffit = if flat || slope(i).is_none() {
            // Under a flat roof or a rake a sloped soffit is a boxed one.
            match e.soffit {
                Soffit::Sloped => Soffit::Boxed,
                s => s,
            }
        } else {
            e.soffit
        };
        if soffit != Soffit::Open && ov > 1e-6 {
            let has = |j: usize| eave(j).is_some_and(|f| f.soffit != Soffit::Open);
            let back = |j: usize| -lines[j].map_or(0.0, |l| l.overhang);
            if let Some(q) = band(i, &has, &back, &|_| 0.0) {
                let mesh = match (soffit, slope(i)) {
                    (Soffit::Sloped, Some(s)) => {
                        // Under the roof's underside: rising inward from the eave line.
                        let (p, _) = eave_line(i, 0.0);
                        let m = out(i);
                        let under = move |a: [f64; 2]| {
                            let inward = -((a[0] - p[0]) * m[0] + (a[1] - p[1]) * m[1]);
                            ze + inward * s.tan()
                        };
                        let depth = SOFFIT / s.cos();
                        prism(&q, &move |a| under(a) - depth, &under)
                    }
                    _ => prism(&q, &|_| soffit_z, &|_| soffit_z + SOFFIT),
                };
                trims.push(RoofTrim {
                    kind: TrimKind::Soffit,
                    key: keys[i],
                    mesh,
                });
            }
        }
    }
    trims
}

/// The part of a closed mesh at or above height `z`, closed again by a level cap.
pub fn clip_above(mesh: &Mesh, z: f64) -> Mesh {
    let (points, indices) = mesh;
    let mut out_pts: Vec<[f64; 3]> = Vec::new();
    let mut out_idx: Vec<u32> = Vec::new();
    let mut index: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut add = |p: [f64; 3], pts: &mut Vec<[f64; 3]>| -> u32 {
        let key = [
            (p[0] * 1e9).round() as i64,
            (p[1] * 1e9).round() as i64,
            (p[2] * 1e9).round() as i64,
        ];
        *index.entry(key).or_insert_with(|| {
            pts.push(p);
            (pts.len() - 1) as u32
        })
    };
    // The crossing of an edge with the plane, computed the same way from either side.
    let cross = |a: [f64; 3], b: [f64; 3]| {
        let (a, b) = if (a[0], a[1], a[2]) <= (b[0], b[1], b[2]) {
            (a, b)
        } else {
            (b, a)
        };
        let t = (z - a[2]) / (b[2] - a[2]);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z]
    };
    let mut segments: Vec<(u32, u32)> = Vec::new();
    for tri in indices.chunks(3) {
        let v: Vec<[f64; 3]> = tri.iter().map(|i| points[*i as usize]).collect();
        let above: Vec<bool> = v.iter().map(|p| p[2] >= z - 1e-12).collect();
        let mut poly: Vec<[f64; 3]> = Vec::new();
        let mut cut: Vec<[f64; 3]> = Vec::new();
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            let (ia, ib) = (above[k], above[(k + 1) % 3]);
            if ia {
                poly.push(a);
            }
            if ia != ib {
                let c = cross(a, b);
                poly.push(c);
                cut.push(c);
            }
        }
        if poly.len() < 3 {
            continue;
        }
        let ids: Vec<u32> = poly.iter().map(|p| add(*p, &mut out_pts)).collect();
        for k in 1..ids.len() - 1 {
            out_idx.extend([ids[0], ids[k], ids[k + 1]]);
        }
        if cut.len() == 2 {
            let (a, b) = (add(cut[0], &mut out_pts), add(cut[1], &mut out_pts));
            if a != b {
                segments.push((a, b));
            }
        }
    }
    // The cap: chain the cut segments into loops and fill each, facing down.
    let mut loops: Vec<Vec<u32>> = Vec::new();
    while let Some((a, b)) = segments.pop() {
        let mut lp = vec![a, b];
        loop {
            let last = *lp.last().expect("two");
            let Some(k) = segments.iter().position(|s| s.0 == last || s.1 == last) else {
                break;
            };
            let s = segments.swap_remove(k);
            let next = if s.0 == last { s.1 } else { s.0 };
            if next == lp[0] {
                break;
            }
            lp.push(next);
        }
        if lp.len() >= 3 {
            loops.push(lp);
        }
    }
    for lp in loops {
        let flat: Vec<[f64; 2]> = lp
            .iter()
            .map(|i| [out_pts[*i as usize][0], out_pts[*i as usize][1]])
            .collect();
        let mut tris: Vec<u32> = Vec::new();
        earcut::Earcut::new().earcut(flat.iter().copied(), &[] as &[u32], &mut tris);
        for t in tris.chunks(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let up = (flat[b][0] - flat[a][0]) * (flat[c][1] - flat[a][1])
                - (flat[b][1] - flat[a][1]) * (flat[c][0] - flat[a][0]);
            // Facing down: clockwise seen from above.
            if up > 0.0 {
                out_idx.extend([lp[a], lp[c], lp[b]]);
            } else {
                out_idx.extend([lp[a], lp[b], lp[c]]);
            }
        }
    }
    (out_pts, out_idx)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEavesParams {
    pub roof_id: ElementRef,
    /// Footprint curve keys; omitted, every slope-defining edge (all edges of a flat roof).
    pub edges: Option<Vec<u32>>,
    pub bearing: Option<Bearing>,
    /// The heel height at the bearing line; `null` clears it.
    #[serde(default, deserialize_with = "super::some")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub heel: Option<Option<Value>>,
    /// From the wall's exterior face to the eave line.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
    pub cut: Option<RafterCut>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub fascia_depth: Option<Value>,
    pub fascia: Option<bool>,
    pub soffit: Option<Soffit>,
    /// Attach the perimeter walls under the roof to it (default true).
    pub attach_walls: Option<bool>,
    /// Removes the edges' eave settings (they become plain overhangs again).
    #[serde(default)]
    pub remove: bool,
}

/// Sets eaves on a roof's edges (`roof.set_eaves`).
pub(crate) fn set_eaves(txn: &mut crate::txn::Transaction<'_>, p: SetEavesParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = super::resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let sketch = crate::sketch::sketch_of(store.get(id).expect("resolved")).unwrap_or_default();
    let length = |v: &Value, what: &str| super::number(&store, v, "length", what);
    let heel = match &p.heel {
        Some(Some(v)) => Some(Some(length(v, "the heel height")?)),
        Some(None) => Some(None),
        None => None,
    };
    let overhang = p
        .overhang
        .as_ref()
        .map(|v| length(v, "the overhang"))
        .transpose()?;
    let fascia_depth = p
        .fascia_depth
        .as_ref()
        .map(|v| length(v, "the fascia depth"))
        .transpose()?;
    if let Some(o) = overhang
        && !(o.is_finite() && o >= 0.0)
    {
        return Err(Error::new(
            "roof.bad_overhang",
            "the overhang cannot be negative",
        ));
    }
    let keys: Vec<u32> = match &p.edges {
        Some(keys) => {
            for k in keys {
                if !sketch.curves.iter().any(|c| c.key == *k) {
                    return Err(Error::new(
                        "roof.no_such_curve",
                        format!("the footprint has no curve {k}"),
                    ));
                }
            }
            keys.clone()
        }
        None => {
            let all: Vec<u32> = sketch.curves.iter().map(|c| c.key).collect();
            let sloped: Vec<u32> = all
                .iter()
                .copied()
                .filter(|k| recipe.edge(*k).defines_slope)
                .collect();
            if sloped.is_empty() { all } else { sloped }
        }
    };
    for k in keys {
        let mut edge = recipe.edge(k);
        if p.remove {
            edge.eave = None;
        } else {
            let mut e = edge.eave.take().unwrap_or_default();
            if let Some(b) = p.bearing {
                e.bearing = b;
            }
            if let Some(h) = heel {
                e.heel = h;
            }
            if let Some(c) = p.cut {
                e.cut = c;
            }
            if let Some(d) = fascia_depth {
                e.fascia_depth = d;
            }
            if let Some(f) = p.fascia {
                e.fascia = f;
            }
            if let Some(s) = p.soffit {
                e.soffit = s;
            }
            edge.eave = Some(e);
            if let Some(o) = overhang {
                edge.overhang = o;
            }
        }
        if edge == RoofEdge::default() {
            recipe.edges.remove(&k.to_string());
        } else {
            recipe.edges.insert(k.to_string(), edge);
        }
    }
    super::write_recipe(txn, id, &recipe)?;
    super::check(txn, id)?;
    super::classify_into(txn, id)?;
    if !p.remove && p.attach_walls != Some(false) {
        crate::walls::attach::attach_below(
            txn,
            crate::walls::attach::AttachBelowParams {
                roof_id: ElementRef::Id(id),
            },
        )?;
    }
    Ok(())
}

/// One eave of a roof, as `roof.eaves` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct EaveRow {
    /// The footprint curve's key.
    pub key: u32,
    /// From the wall's exterior face to the eave line.
    pub overhang: f64,
    pub eave: Eave,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EavesParams {
    pub roof_id: ElementRef,
}

/// A roof's eaves (`roof.eaves`).
pub fn eaves_of(store: &Store, p: EavesParams) -> Result<Vec<EaveRow>> {
    let (_, r) = super::resolve_roof(store, &p.roof_id)?;
    let mut rows: Vec<EaveRow> = r
        .edges
        .iter()
        .filter_map(|(k, e)| {
            Some(EaveRow {
                key: k.parse().ok()?,
                overhang: e.overhang,
                eave: e.eave.clone()?,
            })
        })
        .collect();
    rows.sort_by_key(|r| r.key);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_a_box_closes_it_with_a_cap() {
        let q = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
        let b = crate::walls::prism(&q, 0.0, 1.0);
        let (pts, idx) = clip_above(&b, 0.25);
        assert!(pts.iter().all(|p| p[2] >= 0.25 - 1e-12));
        // Signed volume of the closed result: 2 × 1 × 0.75.
        let v: f64 = idx
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum();
        assert!((v - 1.5).abs() < 1e-9, "{v}");
    }
}
