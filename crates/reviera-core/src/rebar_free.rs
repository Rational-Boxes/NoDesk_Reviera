// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Free-form rebar (STR-RBR-013): bars in hosts with curved or varying geometry.
//!
//! - **Surface distribution** (concrete roofs, such as domes and vaults): the roof's underside
//!   is offset into the shell by the cover and half a bar (from the underside for the bottom
//!   layer, from the top for the top layer) along its smoothed normals, and cut by planes
//!   interpolated between a start plane and an end plane. Each cut is a bar following the
//!   surface, trimmed by the edge cover at both ends.
//! - **Aligned distribution** (any host): planes square to a distribution path cut the host's
//!   body; each section's outline, inset by its own faces' covers and half a bar, is a closed
//!   link, so links follow a varying section.
//!
//! Every bar is worked out on its own, so a free-form set is always varying (STR-RBR-005): each
//! rounded length is its own rebar number. A bar matching no shape is code 99 and reports its
//! total length.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::rebar::{self, Bar, BarType, HostKind, RebarRecipe, SetGeometry, V3, add, dot, len, mul, sub, unit};
use crate::store::Store;
use crate::NewElement;

/// A plane by a point on it and its normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlaneRef {
    pub origin: V3,
    pub normal: V3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "distribution", rename_all = "snake_case")]
pub enum FreeFormPlacement {
    /// Bars on a roof's surface between a start and an end plane.
    Surface {
        /// `bottom` or `top`.
        face: String,
        start: PlaneRef,
        end: PlaneRef,
    },
    /// Closed links square to a path (world points).
    Aligned { path: Vec<V3> },
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

// ---- Meshes ----

/// A mesh with its corners shared, so normals can be smoothed.
struct Welded {
    points: Vec<V3>,
    tris: Vec<[usize; 3]>,
}

fn weld(mesh: &Mesh) -> Welded {
    let mut points: Vec<V3> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let key = |p: V3| [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64, (p[2] * 1e6).round() as i64];
    let mut at = |p: V3| {
        *index.entry(key(p)).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    let tris = mesh
        .1
        .chunks(3)
        .filter(|t| t.len() == 3)
        .map(|t| [at(mesh.0[t[0] as usize]), at(mesh.0[t[1] as usize]), at(mesh.0[t[2] as usize])])
        .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
        .collect();
    Welded { points, tris }
}

/// Where a plane cuts a mesh: segments.
fn slice(points: &[V3], tris: &[[usize; 3]], o: V3, n: V3) -> Vec<[V3; 2]> {
    let d: Vec<f64> = points.iter().map(|p| dot(sub(*p, o), n)).collect();
    let mut out = Vec::new();
    for t in tris {
        let mut cut = Vec::new();
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let (da, db) = (d[a], d[b]);
            if (da < 0.0) != (db < 0.0) {
                let s = da / (da - db);
                cut.push(add(points[a], mul(sub(points[b], points[a]), s)));
            }
        }
        if cut.len() == 2 {
            out.push([cut[0], cut[1]]);
        }
    }
    out
}

/// Segments joined end to end into polylines (closed ones end where they start).
fn chain(segments: &[[V3; 2]]) -> Vec<Vec<V3>> {
    let key = |p: V3| [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64, (p[2] * 1e6).round() as i64];
    let mut ends: std::collections::HashMap<[i64; 3], Vec<usize>> = std::collections::HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        ends.entry(key(s[0])).or_default().push(i);
        ends.entry(key(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = vec![segments[start][0], segments[start][1]];
        // Forward from the end, then backward from the start.
        for forward in [true, false] {
            loop {
                let tip = if forward { *line.last().expect("points") } else { line[0] };
                let next = ends.get(&key(tip)).and_then(|v| v.iter().copied().find(|i| !used[*i]));
                let Some(i) = next else { break };
                used[i] = true;
                let s = segments[i];
                let other = if key(s[0]) == key(tip) { s[1] } else { s[0] };
                if forward {
                    line.push(other);
                } else {
                    line.insert(0, other);
                }
            }
        }
        out.push(line);
    }
    out
}

fn polyline_length(p: &[V3]) -> f64 {
    p.windows(2).map(|w| len(sub(w[1], w[0]))).sum()
}

/// A polyline shortened by `cut` at both ends.
fn trimmed(p: &[V3], cut: f64) -> Option<Vec<V3>> {
    let total = polyline_length(p);
    if total <= 2.0 * cut {
        return None;
    }
    let at = |s: f64| -> (usize, V3) {
        let mut run = 0.0;
        for (i, w) in p.windows(2).enumerate() {
            let l = len(sub(w[1], w[0]));
            if run + l >= s {
                let t = if l > 0.0 { (s - run) / l } else { 0.0 };
                return (i, add(w[0], mul(sub(w[1], w[0]), t)));
            }
            run += l;
        }
        (p.len() - 2, p[p.len() - 1])
    };
    let (i0, a) = at(cut);
    let (i1, b) = at(total - cut);
    let mut out = vec![a];
    out.extend_from_slice(&p[i0 + 1..=i1]);
    out.push(b);
    out.dedup_by(|x, y| len(sub(*x, *y)) < 1e-9);
    Some(out)
}

// ---- Hosts ----

/// A host's body as one mesh.
pub fn host_mesh(store: &Store, host: ElementId) -> Option<Mesh> {
    let body = crate::bodies::all_but_reinforcement(store).into_iter().find(|b| b.element == host)?;
    let mut points = Vec::new();
    let mut idx = Vec::new();
    for p in body.parts {
        let base = points.len() as u32;
        points.extend(p.mesh.0);
        idx.extend(p.mesh.1.iter().map(|i| i + base));
    }
    Some((points, idx))
}

/// A roof's underside as triangles, its upward normals smoothed, and its thickness.
fn roof_surface(store: &Store, host: &Element) -> Result<(Welded, Vec<V3>, f64)> {
    let g = crate::roofs::regenerate(store, host)?;
    let mut points = Vec::new();
    let mut idx = Vec::new();
    for f in &g.faces {
        let base = points.len() as u32;
        points.extend(f.bottom.iter().copied());
        for k in 1..f.bottom.len().saturating_sub(1) {
            idx.extend([base, base + k as u32, base + k as u32 + 1]);
        }
    }
    let w = weld(&(points, idx));
    let mut normals = vec![[0.0; 3]; w.points.len()];
    for t in &w.tris {
        let n = cross(sub(w.points[t[1]], w.points[t[0]]), sub(w.points[t[2]], w.points[t[0]]));
        // Upward, weighted by area.
        let n = if n[2] < 0.0 { mul(n, -1.0) } else { n };
        for k in t {
            normals[*k] = add(normals[*k], n);
        }
    }
    let normals = normals.into_iter().map(|n| if len(n) > 0.0 { unit(n) } else { [0.0, 0.0, 1.0] }).collect();
    Ok((w, normals, g.thickness))
}

/// The paths a host offers for aligned links: a beam's or column's axis; a wall's location
/// line at mid-height (`along`) or its middle upright (`up`).
pub fn named_path(store: &Store, host: &Element, name: &str) -> Result<Vec<V3>> {
    let kind = rebar::host_kind(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    match (kind, name) {
        (HostKind::Beam | HostKind::Column, "axis") => {
            let f = crate::steel_detail::member_frame(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host does not regenerate"))?;
            Ok(vec![f.start, f.end])
        }
        (HostKind::Wall, "along" | "up") => {
            let g = crate::walls::regenerate(store, host)?;
            if name == "along" {
                let z = g.base_z + g.height / 2.0;
                Ok(vec![[g.start[0], g.start[1], z], [g.end[0], g.end[1], z]])
            } else {
                let m = [(g.start[0] + g.end[0]) / 2.0, (g.start[1] + g.end[1]) / 2.0];
                Ok(vec![[m[0], m[1], g.base_z], [m[0], m[1], g.base_z + g.height]])
            }
        }
        _ => Err(Error::new(
            "rebar.path",
            format!("no path {name} for a {kind:?}: beams and columns have axis, walls along and up; or give the path's points"),
        )),
    }
}

/// The cover of a host face facing `outward` (the nearest planar face's), else the host's
/// largest.
fn cover_facing(store: &Store, host: &Element, kind: HostKind, faces: &[crate::rebar_sketch::Face], outward: V3) -> f64 {
    faces
        .iter()
        .max_by(|a, b| dot(a.outward, outward).total_cmp(&dot(b.outward, outward)))
        .map(|f| rebar::cover(store, host, kind, crate::rebar_sketch::face_cover(f)))
        .unwrap_or_else(|| ["top", "bottom", "other"].iter().map(|f| rebar::cover(store, host, kind, f)).fold(0.0, f64::max))
}

/// A closed outline (in a plane's u, v) with nearly straight corners merged.
fn merged(ring: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut r = ring;
    loop {
        let n = r.len();
        if n < 4 {
            return r;
        }
        let drop = (0..n).find(|&i| {
            let (a, b, c) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
            let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
            lu < 1e-6 || lv < 1e-6 || ((u[0] * v[1] - u[1] * v[0]) / (lu * lv)).abs() < 0.0175 && (u[0] * v[0] + u[1] * v[1]) > 0.0
        });
        match drop {
            Some(i) => {
                r.remove(i);
            }
            None => return r,
        }
    }
}

fn contains(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

// ---- Geometry ----

pub(crate) fn geometry(store: &Store, r: &RebarRecipe, host: &Element, kind: HostKind, bt: &BarType, f: &FreeFormPlacement) -> Result<SetGeometry> {
    let d = bt.diameter;
    match f {
        FreeFormPlacement::Surface { face, start, end } => {
            if kind != HostKind::Roof {
                return Err(Error::new("rebar.free_form", "a surface distribution follows a roof's shell; other hosts take aligned distributions"));
            }
            let (w, normals, thickness) = roof_surface(store, host)?;
            let off = match face.as_str() {
                "bottom" => rebar::cover(store, host, kind, "bottom") + d / 2.0,
                "top" => thickness - rebar::cover(store, host, kind, "top") - d / 2.0,
                o => return Err(Error::new("rebar.face", format!("a roof's layers are top or bottom, not {o}"))),
            };
            if off <= d / 2.0 || off >= thickness - d / 2.0 {
                return Err(Error::new("rebar.no_fit", format!("the bars do not fit: the roof is {:.0} mm thick", thickness * 1000.0)));
            }
            let moved: Vec<V3> = w.points.iter().zip(&normals).map(|(p, n)| add(*p, mul(*n, off))).collect();
            let span = len(sub(end.origin, start.origin));
            let pos = rebar::positions(&r.layout, span, d)?;
            let edge = rebar::cover(store, host, kind, "other");
            let mut bars = Vec::new();
            for t in pos {
                let s = if span > 0.0 { t / span } else { 0.0 };
                let o = add(start.origin, mul(sub(end.origin, start.origin), s));
                let n = unit(add(mul(unit(start.normal), 1.0 - s), mul(unit(end.normal), s)));
                let cut = chain(&slice(&moved, &w.tris, o, n));
                if let Some(line) = cut.into_iter().max_by(|a, b| polyline_length(a).total_cmp(&polyline_length(b)))
                    && let Some(line) = trimmed(&line, edge)
                {
                    bars.push(Bar { points: line });
                }
            }
            if bars.is_empty() {
                return Err(Error::new("rebar.no_fit", "the planes do not cut the roof's shell"));
            }
            let longest = bars.iter().map(Bar::length).fold(0.0, f64::max);
            Ok(SetGeometry { bars, dimensions: vec![("A".into(), longest)], role: "main".into(), array_length: span, indices: Vec::new() })
        }
        FreeFormPlacement::Aligned { path } => {
            if path.len() < 2 {
                return Err(Error::new("rebar.path", "the path needs at least two points"));
            }
            let mesh = host_mesh(store, host.id()).ok_or_else(|| Error::new("rebar.no_host", "the host has no body"))?;
            let w = weld(&mesh);
            let faces = crate::rebar_sketch::host_faces(store, host, kind).unwrap_or_default();
            let total = polyline_length(path);
            let ends = rebar::cover(store, host, kind, "other");
            let spread = total - 2.0 * ends - d;
            if spread < 0.0 {
                return Err(Error::new("rebar.no_fit", format!("the path is {:.0} mm, less than its end covers", total * 1000.0)));
            }
            let pos = rebar::positions(&r.layout, spread, d)?;
            let rad = bt.stirrup_bend / 2.0 + d / 2.0;
            let ext = rebar::extension(&rebar::hook_type(store, "stirrup_135")?, d);
            let mut bars = Vec::new();
            let mut first_dims = Vec::new();
            let mut square = false;
            for t in pos {
                // The point and tangent at this distance along the path.
                let s = ends + d / 2.0 + t;
                let (mut run, mut at, mut tan) = (0.0, path[0], unit(sub(path[1], path[0])));
                for wv in path.windows(2) {
                    let l = len(sub(wv[1], wv[0]));
                    tan = unit(sub(wv[1], wv[0]));
                    if run + l >= s {
                        at = add(wv[0], mul(tan, s - run));
                        break;
                    }
                    run += l;
                    at = wv[1];
                }
                // The plane's u, v.
                let helper = if tan[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
                let u = unit(cross(helper, tan));
                let v = cross(tan, u);
                let to2 = |p: V3| [dot(sub(p, at), u), dot(sub(p, at), v)];
                let to3 = |q: [f64; 2]| add(at, add(mul(u, q[0]), mul(v, q[1])));
                let loops: Vec<Vec<[f64; 2]>> = chain(&slice(&w.points, &w.tris, at, tan))
                    .into_iter()
                    .filter(|l| l.len() >= 4 && len(sub(l[0], l[l.len() - 1])) < 1e-6)
                    .map(|l| l[..l.len() - 1].iter().map(|p| to2(*p)).collect())
                    .collect();
                let Some(ring) = loops.into_iter().filter(|l| contains(l, [0.0, 0.0])).max_by(|a, b| crate::polygons::area(a).abs().total_cmp(&crate::polygons::area(b).abs())) else {
                    return Err(Error::new("rebar.no_fit", format!("the path leaves the host {:.0} mm along it", s * 1000.0)));
                };
                let mut ring = merged(ring);
                if crate::polygons::area(&ring) < 0.0 {
                    ring.reverse();
                }
                let n = ring.len();
                // Each edge in by its face's cover and half a bar; corners where neighbours meet.
                let lines: Vec<([f64; 2], [f64; 2])> = (0..n)
                    .map(|i| {
                        let (a, b) = (ring[i], ring[(i + 1) % n]);
                        let e = [b[0] - a[0], b[1] - a[1]];
                        let l = e[0].hypot(e[1]);
                        let out2 = [e[1] / l, -e[0] / l];
                        let out3 = add(mul(u, out2[0]), mul(v, out2[1]));
                        let c = cover_facing(store, host, kind, &faces, out3) + d / 2.0;
                        ([a[0] - out2[0] * c, a[1] - out2[1] * c], [e[0] / l, e[1] / l])
                    })
                    .collect();
                let corner = |i: usize| -> Option<[f64; 2]> {
                    let (p, dp) = lines[(i + n - 1) % n];
                    let (q, dq) = lines[i];
                    let den = dp[0] * dq[1] - dp[1] * dq[0];
                    if den.abs() < 1e-12 {
                        return None;
                    }
                    let w = [q[0] - p[0], q[1] - p[1]];
                    let k = (w[0] * dq[1] - w[1] * dq[0]) / den;
                    Some([p[0] + dp[0] * k, p[1] + dp[1] * k])
                };
                let inset: Option<Vec<[f64; 2]>> = (0..n).map(corner).collect();
                let inset = inset.filter(|r| crate::polygons::area(r) > d * d).ok_or_else(|| {
                    Error::new("rebar.no_fit", format!("the link does not fit inside the cover {:.0} mm along the path", s * 1000.0))
                })?;
                let corners: Vec<V3> = inset.iter().map(|q| to3(*q)).collect();
                let m = corners.len();
                // From the first corner round and back to it, a bar apart along the path, both
                // 135° hooks turned into the core.
                let mid = mul(corners.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / m as f64);
                let into = unit(sub(mid, corners[0]));
                let (a0, a1) = (mul(tan, -d / 2.0), mul(tan, d / 2.0));
                let mut pts = vec![add(add(corners[0], mul(into, ext)), a0), add(corners[0], a0)];
                pts.extend(corners.iter().skip(1).copied());
                pts.push(add(corners[0], a1));
                pts.push(add(add(corners[0], mul(into, ext)), a1));
                if first_dims.is_empty() {
                    first_dims = (0..m)
                        .map(|i| (crate::rebar_docs::SEGMENTS.chars().nth(i).map_or(format!("S{i}"), |c| c.to_string()), len(sub(corners[(i + 1) % m], corners[i])) + d))
                        .collect();
                    first_dims.push((crate::rebar_docs::SEGMENTS.chars().nth(m).map_or(format!("S{m}"), |c| c.to_string()), ext));
                    first_dims.push((crate::rebar_docs::SEGMENTS.chars().nth(m + 1).map_or(format!("S{}", m + 1), |c| c.to_string()), ext));
                    square = m == 4 && (0..4).all(|i| dot(unit(sub(corners[(i + 1) % 4], corners[i])), unit(sub(corners[(i + 2) % 4], corners[(i + 1) % 4]))).abs() < 1e-3);
                }
                bars.push(Bar { points: rebar::rounded(&pts, rad) });
            }
            // A rectangular link is shape 51: its sides A, B and hooks C, D.
            if square {
                first_dims = vec![first_dims[0].clone(), first_dims[1].clone(), ("C".into(), ext), ("D".into(), ext)];
            }
            Ok(SetGeometry { bars, dimensions: first_dims, role: "ligature".into(), array_length: spread, indices: Vec::new() })
        }
    }
}

// ---- Commands ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlaceParams {
    pub host_id: ElementRef,
    /// `surface` (a roof's shell) or `aligned` (closed links square to a path).
    pub kind: String,
    pub bar_type: String,
    /// Surface: `bottom` or `top`.
    #[serde(default)]
    pub face: Option<String>,
    /// Surface: the first and last bars' planes; or `direction` `x` or `y` for planes square to
    /// it across the whole roof, less the edge cover.
    #[serde(default)]
    pub start_plane: Option<PlaneRef>,
    #[serde(default)]
    pub end_plane: Option<PlaneRef>,
    #[serde(default)]
    pub direction: Option<String>,
    /// Aligned: the path's points, or a named path (`axis` for beams and columns, `along` or
    /// `up` for walls).
    #[serde(default)]
    pub path: Option<Vec<V3>>,
    #[serde(default)]
    pub path_ref: Option<String>,
    #[serde(default)]
    pub layout: rebar::Layout,
}

/// Places a free-form set (`rebar.place_free_form`).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = store.resolve(&p.host_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.host_id)))?;
    let host = store.get(id).expect("resolved");
    let kind = rebar::host_check(&store, host).map_err(|why| Error::new("rebar.not_a_host", format!("element {id} cannot host rebar: {why}")))?;
    let bt = rebar::bar_type(&store, &p.bar_type)?;
    let placement = match p.kind.as_str() {
        "surface" => {
            let (start, end) = match (p.start_plane, p.end_plane) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    let axis = match p.direction.as_deref().unwrap_or("x") {
                        "x" => 0,
                        "y" => 1,
                        o => return Err(Error::new("rebar.direction", format!("the direction is x or y, not {o}"))),
                    };
                    if kind != HostKind::Roof {
                        return Err(Error::new("rebar.free_form", "a surface distribution follows a roof's shell"));
                    }
                    let (w, _, _) = roof_surface(&store, host)?;
                    let (lo, hi) = w.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q[axis]), h.max(q[axis])));
                    let c = rebar::cover(&store, host, kind, "other") + bt.diameter / 2.0;
                    let mut n = [0.0; 3];
                    n[axis] = 1.0;
                    let mut a = [0.0; 3];
                    let mut b = [0.0; 3];
                    a[axis] = lo + c;
                    b[axis] = hi - c;
                    (PlaneRef { origin: a, normal: n }, PlaneRef { origin: b, normal: n })
                }
            };
            FreeFormPlacement::Surface { face: p.face.clone().unwrap_or_else(|| "bottom".into()), start, end }
        }
        "aligned" => {
            let path = match (&p.path, &p.path_ref) {
                (Some(pts), _) => pts.clone(),
                (None, Some(n)) => named_path(&store, host, n)?,
                (None, None) => return Err(Error::new("rebar.path", "give the path's points or a named path")),
            };
            FreeFormPlacement::Aligned { path }
        }
        k => return Err(Error::new("rebar.free_form", format!("the distribution is surface or aligned, not {k}"))),
    };
    let mut r = RebarRecipe {
        kind: rebar::REBAR_RECIPE.into(),
        host: host.guid().to_string(),
        shape: "99".into(),
        bar_type: bt.id.clone(),
        placement: rebar::Placement::FreeForm(placement),
        layout: p.layout.clone(),
        edits: rebar::BarEdits::default(),
        splice: None,
        varying: true,
    };
    let g = rebar::geometry(&store, &r)?;
    // The shape, if one fits: a rectangular link 51 (its dimensions A, B, C, D), a straight bar
    // 00; otherwise 99, its total length only.
    r.shape = match &r.placement {
        rebar::Placement::FreeForm(FreeFormPlacement::Aligned { .. }) if g.dimensions.len() == 4 && g.dimensions[0].0 == "A" && g.dimensions[2].0 == "C" => "51".into(),
        rebar::Placement::FreeForm(FreeFormPlacement::Surface { .. }) if g.bars.iter().all(|b| b.points.len() == 2) => "00".into(),
        _ => "99".into(),
    };
    let predefined = if g.role == "ligature" { "LIGATURE" } else { "MAIN" };
    let id = txn.create(
        NewElement::new("IfcReinforcingBar")
            .param("@PredefinedType", predefined)
            .param(crate::walls::RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}
