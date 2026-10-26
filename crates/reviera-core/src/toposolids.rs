// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Toposolids (spec Ch 3 §3.11.4 …, ARC-TOPO-001 … 005): solid terrain.
//!
//! A toposolid is an `IfcGeographicElement` (`TERRAIN`) with `recipe.toposolid`: a level and
//! offset, a boundary in plan, points (x, y and a height above the level) and a thickness. Its
//! top is a triangulated surface (a constrained Delaunay triangulation of the boundary and the
//! points, the spade crate); boundary corners without a point take the height of the points'
//! surface there (or the nearest point's, or the level's). The solid has upright sides and a
//! flat bottom `thickness` below the lowest point of the top.
//!
//! - **Sub-divisions** (a region of a toposolid raised or lowered by an offset) lie on their
//!   parent's surface; a lowered one excavates its parent (through the boolean relations, ARC-
//!   MOD-028) and reports the excavated volume.
//! - **Graded regions** are copies (with all points or the perimeter's) whose cut and fill
//!   against the original are exact for the two triangulated surfaces.
//! - **Simplify** removes interior points that change the surface least until a point count or
//!   a vertical tolerance is reached; the deviation reported is measured against the original
//!   points. Locked points stay.
//! - **Contours** follow the site settings (an interval and an elevation they pass through).
//!
//! Not built yet: layers and types, Split and Merge, excavation by floors and pads, imports from
//! DWG and the point cap (the window reads points files), smooth shading, contour rows per type and labels, phases (a graded
//! region's original is not demolished), `IfcSite` body.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use spade::{ConstrainedDelaunayTriangulation, DelaunayTriangulation, Point2, Triangulation};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::polygons;
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.toposolid";
pub const CUT_RECIPE: &str = "recipe.topo_cut";
pub const SETTINGS_KEY: &str = "rxd:site_settings";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TopoPoint {
    pub x: f64,
    pub y: f64,
    /// Above the toposolid's level and offset.
    pub z: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToposolidRecipe {
    pub kind: String,
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    pub boundary: Vec<[f64; 2]>,
    #[serde(default)]
    pub points: Vec<TopoPoint>,
    pub thickness: f64,
    /// A sub-division: its parent (GlobalId) and how far it is raised (negative: excavated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub sub_offset: f64,
    /// A graded region: the toposolid it was copied from (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graded_from: Option<String>,
}

pub fn recipe(e: &Element) -> Option<ToposolidRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The excavation a lowered sub-division makes in its parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CutRecipe {
    pub kind: String,
    /// The sub-division (GlobalId).
    pub of: String,
}

pub fn cut_recipe(e: &Element) -> Option<CutRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == CUT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Site settings (ARC-TOPO-004).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SiteSettings {
    /// Contours every so many metres (none when 0).
    pub contour_interval: f64,
    /// An elevation the contours pass through.
    #[serde(default)]
    pub passing_elevation: f64,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            contour_interval: 1.0,
            passing_elevation: 0.0,
        }
    }
}

pub fn settings(store: &Store) -> SiteSettings {
    store
        .iter()
        .find(|e| e.class == "IfcProject")
        .and_then(|p| p.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// A triangle's height at `p` (barycentric), if `p` is in it (with a little slack).
fn on_triangle(t: &[[f64; 3]; 3], p: [f64; 2]) -> Option<f64> {
    let (l, z) = barycentric(t, p)?;
    (l >= -1e-9).then_some(z)
}

/// The smallest barycentric coordinate of `p` in a triangle and the height of its plane there.
fn barycentric(t: &[[f64; 3]; 3], p: [f64; 2]) -> Option<(f64, f64)> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-15 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    Some((l1.min(l2).min(l3), l1 * a[2] + l2 * b[2] + l3 * c[2]))
}

/// The height of a plane through a triangle at `p` (inside it or not: clipped pieces' corners
/// can sit a rounding error outside).
fn plane_z(t: &[[f64; 3]; 3], p: [f64; 2]) -> f64 {
    barycentric(t, p).map_or((t[0][2] + t[1][2] + t[2][2]) / 3.0, |b| b.1)
}

/// The height of a surface at `p`: the triangle it is in, or the nearest one by a rounding
/// error.
fn surface_z(surface: &[[[f64; 3]; 3]], p: [f64; 2]) -> Option<f64> {
    surface
        .iter()
        .filter_map(|t| barycentric(t, p))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|b| b.0 > -1e-4)
        .map(|b| b.1)
}

/// The triangulated top (heights above the level) of a boundary and points.
pub fn tin(boundary: &[[f64; 2]], points: &[[f64; 3]]) -> Result<Vec<[[f64; 3]; 3]>> {
    let bad = |w: &str| {
        Error::new(
            "toposolid.triangulation",
            format!("the toposolid cannot be triangulated: {w}"),
        )
    };
    if boundary.len() < 3 {
        return Err(bad("the boundary needs three corners"));
    }
    let inner: Vec<[f64; 3]> = points
        .iter()
        .copied()
        .filter(|p| {
            inside([p[0], p[1]], boundary)
                || boundary
                    .iter()
                    .any(|b| (b[0] - p[0]).hypot(b[1] - p[1]) < 1e-9)
        })
        .collect();
    // The points' own surface, for the corners' heights.
    let mut dt: DelaunayTriangulation<Point2<f64>> = DelaunayTriangulation::new();
    for p in &inner {
        let _ = dt.insert(Point2::new(p[0], p[1]));
    }
    let point_z = |x: f64, y: f64| -> Option<f64> {
        inner
            .iter()
            .find(|p| (p[0] - x).abs() < 1e-9 && (p[1] - y).abs() < 1e-9)
            .map(|p| p[2])
    };
    let surface_z = |q: [f64; 2]| -> f64 {
        if let Some(z) = point_z(q[0], q[1]) {
            return z;
        }
        for f in dt.inner_faces() {
            let vs = f.vertices().map(|v| v.position());
            let t = vs.map(|v| [v.x, v.y, point_z(v.x, v.y).unwrap_or(0.0)]);
            if let Some(z) = on_triangle(&t, q) {
                return z;
            }
        }
        // Outside the points' hull: the nearest point's height.
        inner
            .iter()
            .min_by(|a, b| {
                (a[0] - q[0])
                    .hypot(a[1] - q[1])
                    .total_cmp(&(b[0] - q[0]).hypot(b[1] - q[1]))
            })
            .map_or(0.0, |p| p[2])
    };
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> =
        ConstrainedDelaunayTriangulation::new();
    let mut heights: Vec<([f64; 2], f64)> = Vec::new();
    let mut handles = Vec::new();
    for b in boundary {
        handles.push(
            cdt.insert(Point2::new(b[0], b[1]))
                .map_err(|_| bad("a corner"))?,
        );
        heights.push((*b, surface_z(*b)));
    }
    for k in 0..handles.len() {
        let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
        if a != b && cdt.can_add_constraint(a, b) {
            cdt.add_constraint(a, b);
        }
    }
    for p in &inner {
        cdt.insert(Point2::new(p[0], p[1]))
            .map_err(|_| bad("a point"))?;
        heights.push(([p[0], p[1]], p[2]));
    }
    let height = |x: f64, y: f64| {
        heights
            .iter()
            .rev()
            .find(|(p, _)| (p[0] - x).abs() < 1e-9 && (p[1] - y).abs() < 1e-9)
            .map_or(0.0, |(_, z)| *z)
    };
    let mut out = Vec::new();
    for f in cdt.inner_faces() {
        let vs = f.vertices().map(|v| v.position());
        let c = [
            (vs[0].x + vs[1].x + vs[2].x) / 3.0,
            (vs[0].y + vs[1].y + vs[2].y) / 3.0,
        ];
        if !inside(c, boundary) {
            continue;
        }
        let mut t = vs.map(|v| [v.x, v.y, height(v.x, v.y)]);
        let cross =
            (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
        if cross < 0.0 {
            t.swap(1, 2);
        }
        out.push(t);
    }
    Ok(out)
}

/// A toposolid's top in the model (absolute heights).
pub fn top(store: &Store, e: &Element) -> Result<Vec<[[f64; 3]; 3]>> {
    let r = recipe(e).ok_or_else(|| Error::new("toposolid.not", "not a toposolid"))?;
    let base = crate::levels::elevation_of(store, &r.level)? + r.offset;
    if let Some(parent) = &r.parent {
        // A sub-division: its parent's surface over its region, raised by its offset.
        let p = GlobalId::parse(parent)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|id| store.get(id))
            .ok_or_else(|| {
                Error::new(
                    "toposolid.no_parent",
                    "the sub-division's toposolid is gone",
                )
            })?;
        let surface = top(store, p)?;
        return Ok(clip(&surface, &r.boundary)
            .into_iter()
            .map(|t| t.map(|v| [v[0], v[1], v[2] + r.sub_offset]))
            .collect());
    }
    let pts: Vec<[f64; 3]> = r.points.iter().map(|p| [p.x, p.y, p.z]).collect();
    Ok(tin(&r.boundary, &pts)?
        .into_iter()
        .map(|t| t.map(|v| [v[0], v[1], v[2] + base]))
        .collect())
}

/// A surface's triangles clipped to a region, re-triangulated (heights kept linear).
fn clip(surface: &[[[f64; 3]; 3]], region: &[[f64; 2]]) -> Vec<[[f64; 3]; 3]> {
    let reg = polygons::union(&[region.to_vec()]);
    let mut out = Vec::new();
    for t in surface {
        let tri = vec![[t[0][0], t[0][1]], [t[1][0], t[1][1]], [t[2][0], t[2][1]]];
        for piece in polygons::intersection(&polygons::union(&[tri]), &reg) {
            let mut idx: Vec<u32> = Vec::new();
            let mut pts = piece.outer.clone();
            let mut starts = Vec::new();
            for h in &piece.holes {
                starts.push(pts.len() as u32);
                pts.extend(h.iter().copied());
            }
            earcut::Earcut::new().earcut(pts.iter().copied(), &starts, &mut idx);
            let z = |p: [f64; 2]| plane_z(t, p);
            for c in idx.chunks(3) {
                let mut tri = [0, 1, 2].map(|k| {
                    let p = pts[c[k] as usize];
                    [p[0], p[1], z(p)]
                });
                let cross = (tri[1][0] - tri[0][0]) * (tri[2][1] - tri[0][1])
                    - (tri[1][1] - tri[0][1]) * (tri[2][0] - tri[0][0]);
                if cross < 0.0 {
                    tri.swap(1, 2);
                }
                out.push(tri);
            }
        }
    }
    out
}

/// The closed solid under a top: the top triangles, the same triangles dropped to `bottom`
/// (facing down), and walls along the top's outer edges only, so every edge has two faces.
fn solid(top: &[[[f64; 3]; 3]], bottom: &dyn Fn([f64; 2]) -> f64) -> Mesh {
    use std::collections::{HashMap, HashSet};
    let key = |v: [f64; 3]| ((v[0] * 1e6).round() as i64, (v[1] * 1e6).round() as i64);
    // Shared vertices, so the faces meet exactly.
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut at: HashMap<(i64, i64), u32> = HashMap::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for t in top {
        let ids = t.map(|v| {
            *at.entry(key(v)).or_insert_with(|| {
                pts.push(v);
                pts.len() as u32 - 1
            })
        });
        if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
            tris.push(ids);
        }
    }
    let n = pts.len() as u32;
    for k in 0..n as usize {
        let v = pts[k];
        pts.push([v[0], v[1], bottom([v[0], v[1]])]);
    }
    let edges: HashSet<(u32, u32)> = tris
        .iter()
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .collect();
    let mut idx = Vec::new();
    for t in &tris {
        idx.extend([t[0], t[1], t[2]]);
        idx.extend([t[0] + n, t[2] + n, t[1] + n]);
    }
    for t in &tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if !edges.contains(&(b, a)) {
                // An outer edge (counter-clockwise from above): its wall faces out.
                idx.extend([a, a + n, b + n, a, b + n, b]);
            }
        }
    }
    (pts, idx)
}

/// A toposolid's body (sub-divisions included).
pub fn regenerate(store: &Store, e: &Element) -> Result<Mesh> {
    let r = recipe(e).ok_or_else(|| Error::new("toposolid.not", "not a toposolid"))?;
    let t = top(store, e)?;
    if r.parent.is_some() && r.sub_offset > 0.0 {
        // A raised sub-division sits on its parent's surface.
        let lift = r.sub_offset;
        let lower = t.clone();
        return Ok(solid(&t, &|p| {
            surface_z(&lower, p).map_or(0.0, |z| z - lift)
        }));
    }
    let lowest = t
        .iter()
        .flat_map(|tri| tri.iter().map(|v| v[2]))
        .fold(f64::MAX, f64::min);
    let bottom = lowest - r.thickness;
    Ok(solid(&t, &|_| bottom))
}

/// The excavation tool of a lowered sub-division: its region from its top up to its parent's.
pub fn cut_mesh(store: &Store, e: &Element) -> Result<Mesh> {
    let c = cut_recipe(e).ok_or_else(|| Error::new("toposolid.not", "not an excavation"))?;
    let sub = GlobalId::parse(&c.of)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("toposolid.no_parent", "the sub-division is gone"))?;
    let r = recipe(sub).expect("a sub-division");
    let t = top(store, sub)?;
    let depth = -r.sub_offset;
    // Up to the parent's surface and a touch above, so the cut is clean.
    let raised: Vec<[[f64; 3]; 3]> = t
        .iter()
        .map(|tri| tri.map(|v| [v[0], v[1], v[2] + depth + 0.001]))
        .collect();
    Ok(solid(&raised, &|p| surface_z(&t, p).unwrap_or(0.0)))
}

/// Every toposolid and excavation that regenerates, with whether it is an excavation.
pub fn all(store: &Store) -> Vec<(ElementId, bool, Mesh)> {
    store
        .iter()
        .filter_map(|e| {
            if recipe(e).is_some() {
                return Some((e.id(), false, regenerate(store, e).ok()?));
            }
            if cut_recipe(e).is_some() {
                return Some((e.id(), true, cut_mesh(store, e).ok()?));
            }
            None
        })
        .collect()
}

/// Whether a graded region has replaced this toposolid: the original is kept (for cut and
/// fill, and in the IFC) but hidden, so views and overlap checks see only the graded copy.
pub fn superseded(store: &Store, e: &Element) -> bool {
    let guid = e.guid().to_string();
    store.iter().any(|g| recipe(g).is_some_and(|r| r.graded_from.as_deref() == Some(guid.as_str())))
}

/// The surface height at a plan point, if a toposolid's top is there.
pub fn height_at(store: &Store, e: &Element, p: [f64; 2]) -> Option<f64> {
    surface_z(&top(store, e).ok()?, p)
}

/// Contours on a top: (height, polyline) for every height of the site settings.
pub fn contours(store: &Store, e: &Element) -> Vec<(f64, Vec<[f64; 3]>)> {
    let s = settings(store);
    let Ok(t) = top(store, e) else {
        return Vec::new();
    };
    if s.contour_interval <= 0.0 || t.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = t
        .iter()
        .flat_map(|tri| tri.iter().map(|v| v[2]))
        .fold((f64::MAX, f64::MIN), |a, z| (a.0.min(z), a.1.max(z)));
    let i = s.contour_interval;
    let first = ((lo - s.passing_elevation) / i).ceil() as i64;
    let last = ((hi - s.passing_elevation) / i).floor() as i64;
    let mut out = Vec::new();
    for k in first..=last.min(first + 10_000) {
        let z = s.passing_elevation + k as f64 * i;
        let mut segs: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for tri in &t {
            let mut hits = Vec::new();
            for e in 0..3 {
                let (a, b) = (tri[e], tri[(e + 1) % 3]);
                let (da, db) = (a[2] - z, b[2] - z);
                if (da > 0.0) != (db > 0.0) {
                    let s = da / (da - db);
                    hits.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, z]);
                }
            }
            if hits.len() == 2 {
                segs.push((hits[0], hits[1]));
            }
        }
        // Chain the pieces into polylines.
        let near = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7;
        while let Some((a, b)) = segs.pop() {
            let mut line = vec![a, b];
            loop {
                let end = *line.last().expect("points");
                let start = line[0];
                if let Some(k) = segs.iter().position(|s| near(s.0, end) || near(s.1, end)) {
                    let (p, q) = segs.remove(k);
                    line.push(if near(p, end) { q } else { p });
                } else if let Some(k) = segs
                    .iter()
                    .position(|s| near(s.0, start) || near(s.1, start))
                {
                    let (p, q) = segs.remove(k);
                    line.insert(0, if near(p, start) { q } else { p });
                } else {
                    break;
                }
            }
            out.push((z, line));
        }
    }
    out
}

/// The cut and fill between two surfaces over their overlap: (cut, fill), m³.
pub fn cut_fill(new: &[[[f64; 3]; 3]], old: &[[[f64; 3]; 3]]) -> (f64, f64) {
    let (mut cut, mut fill) = (0.0, 0.0);
    let bbox = |t: &[[f64; 3]; 3]| {
        let xs = t.map(|v| v[0]);
        let ys = t.map(|v| v[1]);
        (
            xs.iter().copied().fold(f64::MAX, f64::min),
            xs.iter().copied().fold(f64::MIN, f64::max),
            ys.iter().copied().fold(f64::MAX, f64::min),
            ys.iter().copied().fold(f64::MIN, f64::max),
        )
    };
    for a in new {
        let ba = bbox(a);
        for b in old {
            let bb = bbox(b);
            if ba.1 < bb.0 || bb.1 < ba.0 || ba.3 < bb.2 || bb.3 < ba.2 {
                continue;
            }
            let ta = polygons::union(&[vec![
                [a[0][0], a[0][1]],
                [a[1][0], a[1][1]],
                [a[2][0], a[2][1]],
            ]]);
            let tb = vec![[b[0][0], b[0][1]], [b[1][0], b[1][1]], [b[2][0], b[2][1]]];
            for piece in polygons::intersection(&ta, &polygons::union(&[tb])) {
                let ring = piece.outer;
                // Δz is linear over the piece: split where it changes sign and integrate.
                let dz = |p: [f64; 2]| plane_z(a, p) - plane_z(b, p);
                let vals: Vec<f64> = ring.iter().map(|p| dz(*p)).collect();
                for sign in [1.0, -1.0] {
                    // The part where sign · Δz ≥ 0.
                    let mut part: Vec<([f64; 2], f64)> = Vec::new();
                    for k in 0..ring.len() {
                        let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
                        let (vp, vq) = (vals[k] * sign, vals[(k + 1) % ring.len()] * sign);
                        if vp >= 0.0 {
                            part.push((p, vp));
                        }
                        if (vp >= 0.0) != (vq >= 0.0) {
                            let s = vp / (vp - vq);
                            part.push(([p[0] + (q[0] - p[0]) * s, p[1] + (q[1] - p[1]) * s], 0.0));
                        }
                    }
                    if part.len() < 3 {
                        continue;
                    }
                    // ∫ over a polygon of a linear function: fan triangles, area × mean value.
                    let mut v = 0.0;
                    for k in 1..part.len() - 1 {
                        let (p0, p1, p2) = (part[0], part[k], part[k + 1]);
                        let ar = ((p1.0[0] - p0.0[0]) * (p2.0[1] - p0.0[1])
                            - (p2.0[0] - p0.0[0]) * (p1.0[1] - p0.0[1]))
                            .abs()
                            / 2.0;
                        v += ar * (p0.1 + p1.1 + p2.1) / 3.0;
                    }
                    if sign > 0.0 {
                        fill += v;
                    } else {
                        cut += v;
                    }
                }
            }
        }
    }
    (cut, fill)
}

// ---- Commands ----

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &ToposolidRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    // Check it still triangulates.
    regenerate(txn.store(), txn.store().get(id).expect("exists")).map(|_| ())
}

fn level_guid(store: &Store, r: &ElementRef) -> Result<String> {
    store
        .resolve(r)
        .and_then(|id| store.get(id))
        .filter(|e| e.class == crate::levels::CLASS)
        .map(|e| e.guid().to_string())
        .ok_or_else(|| Error::new("element.not_found", "no such level"))
}

fn topo(store: &Store, r: &ElementRef) -> Result<(ElementId, ToposolidRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such toposolid"))?;
    let t = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("toposolid.not", "not a toposolid"))?;
    Ok((id, t))
}

/// The convex hull of points (for a boundary from points alone).
fn hull(points: &[[f64; 3]]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = points.iter().map(|q| [q[0], q[1]]).collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *q) <= 0.0 {
            lower.pop();
        }
        lower.push(*q);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *q) <= 0.0 {
            upper.pop();
        }
        upper.push(*q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level_id: ElementRef,
    /// The boundary in plan; omitted, the points' outline.
    #[serde(default)]
    pub boundary: Vec<[f64; 2]>,
    /// Points: x, y and a height above the level.
    #[serde(default)]
    pub points: Vec<[f64; 3]>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Below the lowest point of the top (default 1 m).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
}

/// `toposolid.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = level_guid(&store, &p.level_id)?;
    let boundary = if p.boundary.is_empty() {
        hull(&p.points)
    } else {
        p.boundary
    };
    if boundary.len() < 3 {
        return Err(Error::new(
            "toposolid.boundary",
            "a toposolid needs a boundary (or three points not in a line)",
        ));
    }
    let r = ToposolidRecipe {
        kind: RECIPE.into(),
        level,
        offset: p.offset,
        boundary,
        points: p
            .points
            .iter()
            .map(|q| TopoPoint {
                x: q[0],
                y: q[1],
                z: q[2],
                locked: false,
            })
            .collect(),
        thickness: p.thickness.unwrap_or(1.0),
        parent: None,
        sub_offset: 0.0,
        graded_from: None,
    };
    let id = txn.create(
        NewElement::new("IfcGeographicElement")
            .name("Toposolid")
            .param("@PredefinedType", "TERRAIN")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    regenerate(txn.store(), txn.store().get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPointsParams {
    pub toposolid_id: ElementRef,
    /// Points to add or move (a point at the same x and y takes the new height).
    #[serde(default)]
    pub points: Vec<[f64; 3]>,
    /// Points to remove (by x and y).
    #[serde(default)]
    pub remove: Vec<[f64; 2]>,
    /// Lock (survey points) or unlock these (by x and y).
    #[serde(default)]
    pub lock: Vec<[f64; 2]>,
}

/// `toposolid.set_points`.
pub(crate) fn set_points(txn: &mut Transaction<'_>, p: SetPointsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = topo(&store, &p.toposolid_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let same = |a: &TopoPoint, x: f64, y: f64| (a.x - x).abs() < 1e-9 && (a.y - y).abs() < 1e-9;
    for q in &p.points {
        match r.points.iter_mut().find(|a| same(a, q[0], q[1])) {
            Some(a) => a.z = q[2],
            None => r.points.push(TopoPoint {
                x: q[0],
                y: q[1],
                z: q[2],
                locked: false,
            }),
        }
    }
    r.points
        .retain(|a| !p.remove.iter().any(|q| same(a, q[0], q[1])));
    for q in &p.lock {
        if let Some(a) = r.points.iter_mut().find(|a| same(a, q[0], q[1])) {
            a.locked = !a.locked;
        }
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubdivideParams {
    pub toposolid_id: ElementRef,
    /// The region in plan.
    pub region: Vec<[f64; 2]>,
    /// Up from the parent's surface (negative: excavated into it).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
}

/// `toposolid.subdivide`.
pub(crate) fn subdivide(txn: &mut Transaction<'_>, p: SubdivideParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (pid, pr) = topo(&store, &p.toposolid_id)?;
    if pr.parent.is_some() {
        return Err(Error::new(
            "toposolid.nested",
            "sub-divide the toposolid, not a sub-division",
        ));
    }
    let parent_guid = store.get(pid).expect("resolved").guid().to_string();
    let r = ToposolidRecipe {
        kind: RECIPE.into(),
        level: pr.level.clone(),
        offset: pr.offset,
        boundary: p.region,
        points: Vec::new(),
        thickness: p.thickness.unwrap_or(0.15),
        parent: Some(parent_guid.clone()),
        sub_offset: p.offset,
        graded_from: None,
    };
    let id = txn.create(
        NewElement::new("IfcGeographicElement")
            .name("Sub-Division")
            .param("@PredefinedType", "TERRAIN")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    let sub = txn.store().get(id).expect("created");
    if top(txn.store(), sub)?.is_empty() {
        return Err(Error::new(
            "toposolid.region",
            "the region is not on the toposolid",
        ));
    }
    if p.offset < 0.0 {
        let guid = sub.guid().to_string();
        let c = CutRecipe {
            kind: CUT_RECIPE.into(),
            of: guid,
        };
        let cid = txn.create(
            NewElement::new("IfcOpeningElement")
                .name("Excavation")
                .param(RECIPE_KEY, serde_json::to_value(&c).expect("serialises")),
        )?;
        let cg = txn.store().get(cid).expect("created").guid().to_string();
        crate::csg::set_tool_targets(txn, &cg, &[parent_guid], crate::csg::CsgOp::Subtract, true)?;
    }
    Ok(id)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradedMode {
    #[default]
    Copy,
    PerimeterOnly,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradedParams {
    pub toposolid_id: ElementRef,
    #[serde(default)]
    pub mode: GradedMode,
}

/// `toposolid.create_graded_region`: a copy to grade, linked to the original.
pub(crate) fn create_graded(txn: &mut Transaction<'_>, p: GradedParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, mut r) = topo(&store, &p.toposolid_id)?;
    let e = store.get(id).expect("resolved");
    if p.mode == GradedMode::PerimeterOnly {
        // The boundary corners at their heights, as points.
        let base = crate::levels::elevation_of(&store, &r.level)? + r.offset;
        let t = top(&store, e)?;
        r.points = r
            .boundary
            .iter()
            .map(|b| {
                let z = surface_z(&t, *b).unwrap_or(base);
                TopoPoint {
                    x: b[0],
                    y: b[1],
                    z: z - base,
                    locked: false,
                }
            })
            .collect();
    }
    r.graded_from = Some(e.guid().to_string());
    let new = txn.create(
        NewElement::new("IfcGeographicElement")
            .name("Graded Region")
            .param("@PredefinedType", "TERRAIN")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    Ok(new)
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CutFill {
    pub cut: f64,
    pub fill: f64,
    pub net: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub toposolid_id: ElementRef,
}

/// `toposolid.cut_fill`: a graded region against its original.
pub(crate) fn cut_fill_of(store: &Store, p: IdParams) -> Result<CutFill> {
    let (id, r) = topo(store, &p.toposolid_id)?;
    let orig = r
        .graded_from
        .as_deref()
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|i| store.get(i))
        .ok_or_else(|| {
            Error::new(
                "toposolid.not_graded",
                "not a graded region (or its original is gone)",
            )
        })?;
    let (cut, fill) = cut_fill(
        &top(store, store.get(id).expect("resolved"))?,
        &top(store, orig)?,
    );
    Ok(CutFill {
        cut,
        fill,
        net: fill - cut,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TopoInfo {
    pub area: f64,
    pub volume: f64,
    pub points: usize,
    /// A lowered sub-division's excavated volume.
    pub excavation: f64,
}

/// `toposolid.info`.
pub(crate) fn info(store: &Store, p: IdParams) -> Result<TopoInfo> {
    let (id, r) = topo(store, &p.toposolid_id)?;
    let e = store.get(id).expect("resolved");
    let t = top(store, e)?;
    let area: f64 = t
        .iter()
        .map(|tri| {
            ((tri[1][0] - tri[0][0]) * (tri[2][1] - tri[0][1])
                - (tri[2][0] - tri[0][0]) * (tri[1][1] - tri[0][1]))
                .abs()
                / 2.0
        })
        .sum();
    let m = regenerate(store, e)?;
    let volume: f64 =
        m.1.chunks(3)
            .map(|c| {
                let [a, b, d] = [0, 1, 2].map(|k| m.0[c[k] as usize]);
                (a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0])
                    + a[2] * (b[0] * d[1] - b[1] * d[0]))
                    / 6.0
            })
            .sum();
    Ok(TopoInfo {
        area,
        volume: volume.abs(),
        points: r.points.len(),
        excavation: if r.sub_offset < 0.0 {
            area * -r.sub_offset
        } else {
            0.0
        },
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimplifyParams {
    pub toposolid_id: ElementRef,
    /// Keep this many points …
    pub target_points: Option<usize>,
    /// … or remove points while the surface moves at most this much (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub max_vertical_error: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Simplified {
    pub points: usize,
    /// The largest vertical distance between the new surface and the original points.
    pub max_deviation: f64,
}

/// `toposolid.simplify` (ARC-TOPO-005).
pub(crate) fn simplify(txn: &mut Transaction<'_>, p: SimplifyParams) -> Result<Simplified> {
    let store = txn.store().clone();
    let (id, mut r) = topo(&store, &p.toposolid_id)?;
    if p.target_points.is_some() == p.max_vertical_error.is_some() {
        return Err(Error::new(
            "toposolid.simplify_args",
            "Specify a target or a tolerance, not both",
        ));
    }
    let on_boundary = |q: &TopoPoint| {
        r.boundary
            .iter()
            .any(|b| (b[0] - q.x).abs() < 1e-9 && (b[1] - q.y).abs() < 1e-9)
    };
    let original = r.points.clone();
    let free = original
        .iter()
        .filter(|q| !q.locked && !on_boundary(q))
        .count();
    if free < 1 {
        return Err(Error::new("toposolid.nothing", "Nothing to simplify"));
    }
    let protected = original.len() - free;
    let target = p.target_points.map(|t| t.max(protected + 1));
    let tol = p.max_vertical_error.unwrap_or(f64::INFINITY);
    let surface = |pts: &[TopoPoint]| -> Result<Vec<[[f64; 3]; 3]>> {
        tin(
            &r.boundary,
            &pts.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>(),
        )
    };
    let deviation = |pts: &[TopoPoint]| -> Result<f64> {
        let s = surface(pts)?;
        Ok(original
            .iter()
            .map(|q| surface_z(&s, [q.x, q.y]).map_or(0.0, |z| (z - q.z).abs()))
            .fold(0.0, f64::max))
    };
    let mut pts = original.clone();
    loop {
        if target.is_some_and(|t| pts.len() <= t) {
            break;
        }
        // Each removable point's error: its height against the surface without it, near it.
        let candidates: Vec<usize> = (0..pts.len())
            .filter(|&k| !pts[k].locked && !on_boundary(&pts[k]))
            .collect();
        if candidates.is_empty() {
            break;
        }
        let s = surface(&pts)?;
        let mut scored: Vec<(f64, usize)> = candidates
            .iter()
            .map(|&k| {
                let q = pts[k];
                // The plane of the point's neighbours: the triangles at it, without it.
                let ring: Vec<[f64; 3]> = s
                    .iter()
                    .filter(|t| {
                        t.iter()
                            .any(|v| (v[0] - q.x).abs() < 1e-9 && (v[1] - q.y).abs() < 1e-9)
                    })
                    .flat_map(|t| t.iter().copied())
                    .filter(|v| !((v[0] - q.x).abs() < 1e-9 && (v[1] - q.y).abs() < 1e-9))
                    .collect();
                let n = ring.len().max(1) as f64;
                // Inverse-distance weighted neighbours' height at the point.
                let (mut wsum, mut zsum) = (0.0, 0.0);
                for v in &ring {
                    let w = 1.0 / ((v[0] - q.x).hypot(v[1] - q.y).max(1e-6));
                    wsum += w;
                    zsum += w * v[2];
                }
                let est = if wsum > 0.0 {
                    zsum / wsum
                } else {
                    ring.iter().map(|v| v[2]).sum::<f64>() / n
                };
                ((q.z - est).abs(), k)
            })
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Remove a batch of the least important points, none next to another removed now.
        let want = match target {
            Some(t) => (pts.len() - t).min((candidates.len() / 10).max(1)),
            None => (candidates.len() / 10).max(1),
        };
        let mut removed: Vec<usize> = Vec::new();
        let mut used: Vec<[f64; 2]> = Vec::new();
        for (err, k) in &scored {
            if removed.len() >= want {
                break;
            }
            if target.is_none() && *err > tol {
                break;
            }
            let q = pts[*k];
            if used.iter().any(|u| (u[0] - q.x).hypot(u[1] - q.y) < 1e-9) {
                continue;
            }
            // Its neighbours are not removed in this batch.
            for t in s.iter().filter(|t| {
                t.iter()
                    .any(|v| (v[0] - q.x).abs() < 1e-9 && (v[1] - q.y).abs() < 1e-9)
            }) {
                for v in t {
                    used.push([v[0], v[1]]);
                }
            }
            removed.push(*k);
        }
        if removed.is_empty() {
            break;
        }
        let trial: Vec<TopoPoint> = pts
            .iter()
            .enumerate()
            .filter(|(k, _)| !removed.contains(k))
            .map(|(_, q)| *q)
            .collect();
        // With a tolerance, a batch that goes past it is not taken.
        if target.is_none() && deviation(&trial)? > tol + 1e-12 {
            if removed.len() == 1 {
                break;
            }
            // Try the single least important one.
            let one: Vec<TopoPoint> = pts
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != removed[0])
                .map(|(_, q)| *q)
                .collect();
            if deviation(&one)? > tol + 1e-12 {
                break;
            }
            pts = one;
            continue;
        }
        pts = trial;
    }
    let max_deviation = deviation(&pts)?;
    r.points = pts;
    let n = r.points.len();
    write(txn, id, &r)?;
    Ok(Simplified {
        points: n,
        max_deviation,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsParams {
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub contour_interval: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub passing_elevation: Option<f64>,
}

/// `site.set_settings`.
pub(crate) fn set_settings(txn: &mut Transaction<'_>, p: SettingsParams) -> Result<()> {
    let mut s = settings(txn.store());
    if let Some(v) = p.contour_interval {
        if v < 0.0 {
            return Err(Error::new(
                "site.interval",
                "the interval must not be negative",
            ));
        }
        s.contour_interval = v;
    }
    if let Some(v) = p.passing_elevation {
        s.passing_elevation = v;
    }
    let pid = txn
        .store()
        .iter()
        .find(|e| e.class == "IfcProject")
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(s).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(SETTINGS_KEY.into(), v);
    })
}

/// Moves, turns or mirrors a toposolid's boundary and points.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a toposolid");
    r.boundary = r.boundary.iter().map(|p| point(*p)).collect();
    for q in &mut r.points {
        let p = point([q.x, q.y]);
        q.x = p[0];
        q.y = p[1];
    }
    r.offset += dz;
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteComponentParams {
    /// A component type (`mesh_object` type): a tree, bench, car, …
    pub type_id: ElementRef,
    pub point: [f64; 2],
    /// The toposolid it stands on (it then follows the surface); omitted, the level.
    pub host_id: Option<ElementRef>,
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// `sitecomponent.place` (ARC-SITE-001): a component on a toposolid's surface.
pub(crate) fn place_component(
    txn: &mut Transaction<'_>,
    p: SiteComponentParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = match &p.host_id {
        Some(h) => {
            let (id, _) = topo(&store, h)?;
            Some(store.get(id).expect("resolved").guid().to_string())
        }
        None => None,
    };
    let args: crate::mesh_objects::PlaceParams = serde_json::from_value(serde_json::json!({
        "type_id": p.type_id, "point": p.point, "level": p.level, "rotation": p.rotation,
    }))
    .map_err(|e| Error::new("args.invalid", e.to_string()))?;
    let id = crate::mesh_objects::place(txn, args)?;
    if host.is_some() {
        let mut r = crate::mesh_objects::mesh_object_recipe(txn.store().get(id).expect("placed"))
            .expect("a component");
        r.host = host;
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    Ok(id)
}
