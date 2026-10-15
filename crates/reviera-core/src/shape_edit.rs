// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Shape editing (spec Ch 3 §3.6.10, ARC-SHAPE-001): warping a floor's top with points and split
//! lines, for falls to drains and similar.
//!
//! The modifiers are the floor's `rxd:shape`: height offsets of its outline's corners (`v1`,
//! `v2`, … in outline order), added points and split lines, each height relative to the floor's
//! top, so the floor keeps them as its level moves. The top is a constrained Delaunay
//! triangulation (the `spade` crate) of the corners, points and split-line vertices, with the
//! outline, holes and split lines as constraints, each triangle lifted to its corners' heights.
//! The slab follows the warped top: each underside vertex is moved down along its averaged
//! normal, scaled so the thickness square to the facets stays constant.
//!
//! Not built yet: roofs, toposolids and structural floors; variable layers (floors have no
//! layers yet); Pick Supports; copy and paste; arcs discretised to 1 mm (sampled as the sketch
//! samples them).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use crate::bodies::Mesh;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::sketch::Profile;
use crate::store::Store;
use crate::txn::Transaction;

pub const SHAPE_KEY: &str = "rxd:shape";

/// An added point: plan position and height offset from the top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapePoint {
    pub key: String,
    pub xy: [f64; 2],
    pub z: f64,
}

/// A split line: plan points and their height offsets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SplitLine {
    pub key: String,
    pub points: Vec<[f64; 2]>,
    pub heights: Vec<f64>,
}

/// A floor's shape modifiers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shape {
    /// Outline corner offsets by key (`v1` …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vertices: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<ShapePoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<SplitLine>,
    /// The next key's number.
    #[serde(default)]
    pub next: u32,
}

impl Shape {
    pub fn is_empty(&self) -> bool {
        self.vertices.values().all(|v| v.abs() < 1e-12)
            && self.points.is_empty()
            && self.lines.is_empty()
    }
}

pub fn shape_of(e: &Element) -> Option<Shape> {
    let s: Shape = serde_json::from_value(e.param(SHAPE_KEY)?.clone()).ok()?;
    (!s.is_empty()).then_some(s)
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The warped top: triangles of `(x, y, offset)` corners over a profile.
pub fn top_triangles(profile: &Profile, shape: &Shape) -> Result<Vec<[[f64; 3]; 3]>> {
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> =
        ConstrainedDelaunayTriangulation::new();
    let mut heights: Vec<(Point2<f64>, f64)> = Vec::new();
    let bad = |what: &str| {
        Error::new(
            "shape.triangulation",
            format!("the shape cannot be triangulated: {what}"),
        )
    };
    let add_ring = |cdt: &mut ConstrainedDelaunayTriangulation<Point2<f64>>,
                    ring: &[[f64; 2]],
                    offset: &dyn Fn(usize) -> f64,
                    heights: &mut Vec<(Point2<f64>, f64)>|
     -> Result<()> {
        let mut handles = Vec::new();
        for (k, p) in ring.iter().enumerate() {
            let q = Point2::new(p[0], p[1]);
            handles.push(cdt.insert(q).map_err(|_| bad("a corner"))?);
            heights.push((q, offset(k)));
        }
        for k in 0..handles.len() {
            let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
            if a != b && cdt.can_add_constraint(a, b) {
                cdt.add_constraint(a, b);
            }
        }
        Ok(())
    };
    let outer_offset = |k: usize| {
        shape
            .vertices
            .get(&format!("v{}", k + 1))
            .copied()
            .unwrap_or(0.0)
    };
    add_ring(&mut cdt, &profile.outer, &outer_offset, &mut heights)?;
    for h in &profile.holes {
        add_ring(&mut cdt, h, &|_| 0.0, &mut heights)?;
    }
    for p in &shape.points {
        let q = Point2::new(p.xy[0], p.xy[1]);
        cdt.insert(q).map_err(|_| bad("a point"))?;
        heights.push((q, p.z));
    }
    for l in &shape.lines {
        let mut prev = None;
        for (p, h) in l.points.iter().zip(&l.heights) {
            let q = Point2::new(p[0], p[1]);
            let v = cdt.insert(q).map_err(|_| bad("a split line"))?;
            heights.push((q, *h));
            if let Some(a) = prev
                && cdt.can_add_constraint(a, v)
            {
                cdt.add_constraint(a, v);
            }
            prev = Some(v);
        }
    }
    let height = |q: Point2<f64>| {
        heights
            .iter()
            .filter(|(p, _)| (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9)
            .map(|(_, h)| *h)
            .next_back()
            .unwrap_or(0.0)
    };
    let mut out = Vec::new();
    for f in cdt.inner_faces() {
        let vs = f.vertices().map(|v| v.position());
        let c = [
            (vs[0].x + vs[1].x + vs[2].x) / 3.0,
            (vs[0].y + vs[1].y + vs[2].y) / 3.0,
        ];
        if !inside(c, &profile.outer) || profile.holes.iter().any(|h| inside(c, h)) {
            continue;
        }
        let mut t = vs.map(|v| [v.x, v.y, height(v)]);
        // Counter-clockwise from above.
        let cross =
            (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
        if cross < 0.0 {
            t.swap(1, 2);
        }
        out.push(t);
    }
    Ok(out)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-15);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// The warped slab over a profile: the top lifted from `top`, the underside `thickness` below
/// it square to the facets, and the outline's sides.
pub fn slab_mesh(profile: &Profile, shape: &Shape, top: f64, thickness: f64) -> Result<Mesh> {
    let tris = top_triangles(profile, shape)?;
    let key = |p: [f64; 2]| [(p[0] * 1e9).round() as i64, (p[1] * 1e9).round() as i64];
    let mut index: BTreeMap<[i64; 2], u32> = BTreeMap::new();
    let mut tops: Vec<[f64; 3]> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    for t in &tris {
        let mut f = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let id = *index.entry(key([p[0], p[1]])).or_insert_with(|| {
                tops.push([p[0], p[1], top + p[2]]);
                (tops.len() - 1) as u32
            });
            f[k] = id;
        }
        faces.push(f);
    }
    // Per vertex: the facets' unit normals around it.
    let mut around: Vec<Vec<[f64; 3]>> = vec![Vec::new(); tops.len()];
    for f in &faces {
        let n = unit(cross(
            sub(tops[f[1] as usize], tops[f[0] as usize]),
            sub(tops[f[2] as usize], tops[f[0] as usize]),
        ));
        for v in f {
            around[*v as usize].push(n);
        }
    }
    let n = tops.len() as u32;
    let mut positions = tops.clone();
    for (k, p) in tops.iter().enumerate() {
        let ns = &around[k];
        let sum = ns
            .iter()
            .fold([0.0; 3], |s, m| [s[0] + m[0], s[1] + m[1], s[2] + m[2]]);
        let avg = if ns.is_empty() {
            [0.0, 0.0, 1.0]
        } else {
            unit(sum)
        };
        let cos = ns
            .iter()
            .map(|m| avg[0] * m[0] + avg[1] * m[1] + avg[2] * m[2])
            .fold(1.0f64, f64::min)
            .max(0.2);
        let d = thickness / cos;
        positions.push([p[0] - avg[0] * d, p[1] - avg[1] * d, p[2] - avg[2] * d]);
    }
    let mut indices: Vec<u32> = Vec::new();
    for f in &faces {
        indices.extend([f[0], f[1], f[2]]);
        indices.extend([n + f[0], n + f[2], n + f[1]]);
    }
    // The sides: every boundary edge (used by one triangle), closed by a quad.
    let mut uses: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    for f in &faces {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    for f in &faces {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            if uses[&(a.min(b), a.max(b))] == 1 {
                // The face uses a → b counter-clockwise: the outside is on the right.
                indices.extend([b, a, n + a, b, n + a, n + b]);
            }
        }
    }
    Ok((positions, indices))
}

/// The warped top's height offset at a plan point, or 0 outside it.
fn surface_offset(profile: &Profile, shape: &Shape, p: [f64; 2]) -> f64 {
    let Ok(tris) = top_triangles(profile, shape) else {
        return 0.0;
    };
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if d.abs() < 1e-15 {
            continue;
        }
        let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
        let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
        let l3 = 1.0 - l1 - l2;
        if l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9 {
            return l1 * a[2] + l2 * b[2] + l3 * c[2];
        }
    }
    0.0
}

fn floor_of(store: &Store, r: &ElementRef) -> Result<(ElementId, Profile, f64)> {
    let id = store
        .resolve(r)
        .filter(|id| {
            store
                .get(*id)
                .and_then(crate::floors::floor_recipe)
                .is_some()
        })
        .ok_or_else(|| {
            Error::new(
                "shape.host",
                "shape editing works on Reviera floors for now",
            )
        })?;
    let e = store.get(id).expect("resolved");
    let g = crate::floors::regenerate(store, e)?;
    let profile = g
        .profiles
        .into_iter()
        .next()
        .ok_or_else(|| Error::new("shape.host", "the floor has no outline"))?;
    Ok((id, profile, g.top))
}

fn shape_now(store: &Store, id: ElementId) -> Shape {
    store
        .get(id)
        .and_then(|e| e.param(SHAPE_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn write(txn: &mut Transaction<'_>, id: ElementId, s: &Shape) -> Result<()> {
    let value = serde_json::to_value(s).expect("serialises");
    let empty = s.is_empty();
    txn.modify(id, |e| {
        if empty {
            e.params.remove(SHAPE_KEY);
        } else {
            e.params.insert(SHAPE_KEY.into(), value);
        }
    })?;
    let store = txn.store().clone();
    let (_, profile, top) = floor_of(&store, &ElementRef::Id(id))?;
    let t = store
        .get(id)
        .and_then(|e| crate::floors::regenerate(&store, e).ok())
        .map_or(0.2, |g| g.thickness);
    slab_mesh(&profile, s, top, t).map(|_| ())
}

/// A height mode: along the surface (an offset from it there) or absolute.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    AlongSurface {
        #[serde(default)]
        offset: f64,
    },
    Absolute {
        z: f64,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPointParams {
    pub element_id: ElementRef,
    pub xy: [f64; 2],
    /// `{"along_surface": {"offset": -0.05}}` or `{"absolute": {"z": 3.0}}`.
    pub mode: Mode,
}

/// `shape.add_point`.
pub(crate) fn add_point(txn: &mut Transaction<'_>, p: AddPointParams) -> Result<String> {
    let store = txn.store().clone();
    let (id, profile, top) = floor_of(&store, &p.element_id)?;
    if !inside(p.xy, &profile.outer) || profile.holes.iter().any(|h| inside(p.xy, h)) {
        return Err(Error::new(
            "shape.outside",
            "the point is outside the floor",
        ));
    }
    let mut s = shape_now(&store, id);
    let z = match p.mode {
        Mode::AlongSurface { offset } => surface_offset(&profile, &s, p.xy) + offset,
        Mode::Absolute { z } => z - top,
    };
    s.next += 1;
    let key = format!("p{}", s.next);
    s.points.push(ShapePoint {
        key: key.clone(),
        xy: p.xy,
        z,
    });
    write(txn, id, &s)?;
    Ok(key)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLineParams {
    pub element_id: ElementRef,
    pub points: Vec<[f64; 2]>,
    pub mode: Mode,
}

/// `shape.add_split_line`.
pub(crate) fn add_split_line(txn: &mut Transaction<'_>, p: AddLineParams) -> Result<String> {
    let store = txn.store().clone();
    let (id, profile, top) = floor_of(&store, &p.element_id)?;
    if p.points.len() < 2 {
        return Err(Error::new(
            "shape.line",
            "a split line has at least two points",
        ));
    }
    let mut s = shape_now(&store, id);
    let heights = p
        .points
        .iter()
        .map(|q| match &p.mode {
            Mode::AlongSurface { offset } => surface_offset(&profile, &s, *q) + offset,
            Mode::Absolute { z } => z - top,
        })
        .collect();
    s.next += 1;
    let key = format!("l{}", s.next);
    s.lines.push(SplitLine {
        key: key.clone(),
        points: p.points,
        heights,
    });
    write(txn, id, &s)?;
    Ok(key)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    pub element_id: ElementRef,
    /// Points (`p…`), split lines (`l…`, all their vertices) and outline corners (`v1`…).
    pub keys: Vec<String>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub dz: f64,
}

/// `shape.move`: raises or lowers modifiers.
pub(crate) fn move_keys(txn: &mut Transaction<'_>, p: MoveParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, profile, _) = floor_of(&store, &p.element_id)?;
    let mut s = shape_now(&store, id);
    for k in &p.keys {
        if let Some(pt) = s.points.iter_mut().find(|q| &q.key == k) {
            pt.z += p.dz;
        } else if let Some(l) = s.lines.iter_mut().find(|l| &l.key == k) {
            l.heights.iter_mut().for_each(|h| *h += p.dz);
        } else if let Some(i) = k.strip_prefix('v').and_then(|i| i.parse::<usize>().ok())
            && (1..=profile.outer.len()).contains(&i)
        {
            *s.vertices.entry(k.clone()).or_insert(0.0) += p.dz;
        } else {
            return Err(Error::new("shape.key", format!("no shape modifier {k:?}")));
        }
    }
    write(txn, id, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElevationParams {
    pub element_id: ElementRef,
    pub keys: Vec<String>,
    /// The absolute elevation (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub z: f64,
}

/// `shape.set_elevation`.
pub(crate) fn set_elevation(txn: &mut Transaction<'_>, p: ElevationParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, profile, top) = floor_of(&store, &p.element_id)?;
    let mut s = shape_now(&store, id);
    let off = p.z - top;
    for k in &p.keys {
        if let Some(pt) = s.points.iter_mut().find(|q| &q.key == k) {
            pt.z = off;
        } else if let Some(l) = s.lines.iter_mut().find(|l| &l.key == k) {
            l.heights.iter_mut().for_each(|h| *h = off);
        } else if let Some(i) = k.strip_prefix('v').and_then(|i| i.parse::<usize>().ok())
            && (1..=profile.outer.len()).contains(&i)
        {
            s.vertices.insert(k.clone(), off);
        } else {
            return Err(Error::new("shape.key", format!("no shape modifier {k:?}")));
        }
    }
    write(txn, id, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementParams {
    pub element_id: ElementRef,
}

/// `shape.reset`: removes every modifier.
pub(crate) fn reset(txn: &mut Transaction<'_>, p: ElementParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, _, _) = floor_of(&store, &p.element_id)?;
    write(txn, id, &Shape::default())
}

/// `shape.get`: the modifiers.
pub(crate) fn get(store: &Store, p: ElementParams) -> Result<Shape> {
    let (id, _, _) = floor_of(store, &p.element_id)?;
    Ok(shape_now(store, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drain_point_gives_four_facets() {
        let profile = Profile {
            outer: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            holes: Vec::new(),
        };
        let s = Shape {
            points: vec![ShapePoint {
                key: "p1".into(),
                xy: [5.0, 5.0],
                z: -0.05,
            }],
            ..Default::default()
        };
        let tris = top_triangles(&profile, &s).unwrap();
        assert_eq!(tris.len(), 4);
        assert!((surface_offset(&profile, &s, [5.0, 5.0]) + 0.05).abs() < 1e-12);
        let (pts, idx) = slab_mesh(&profile, &s, 0.0, 0.3).unwrap();
        assert_eq!(idx.len() % 3, 0);
        // The underside under the drain is 0.3 below the top square to the facets.
        let drain_bottom = pts[pts.len() / 2..]
            .iter()
            .find(|p| (p[0] - 5.0).abs() < 1e-9 && (p[1] - 5.0).abs() < 1e-9)
            .unwrap();
        assert!(drain_bottom[2] < -0.05 - 0.3 + 1e-9);
    }
}
