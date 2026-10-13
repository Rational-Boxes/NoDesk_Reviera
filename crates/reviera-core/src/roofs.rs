// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roofs by footprint (spec Ch 3 §3.6.4, ARC-ROOF-001).
//!
//! A roof is an `IfcRoof` with a `recipe.roof` (`rxd:recipe`):
//! - its base level (GlobalId) and base offset, which give the plate height;
//! - a default slope;
//! - per footprint curve, whether it defines slope, its own slope and its overhang;
//! - an optional cutoff (level and offset).
//!
//! The footprint is a sketch (`rxd:sketch`) with one outer loop. Its type (`IfcRoofType` with a
//! `recipe.roof_type`) gives the thickness.
//!
//! Generation (ARC-ROOF-001):
//! - Gable edges are moved out by their overhang.
//! - The weighted straight skeleton of the footprint ([`skeleton`]) gives one face per
//!   slope-defining edge on that edge's plane, plus vertical gable faces. Each edge moves inward
//!   at `1 / tan(slope)` metres per metre of rise.
//! - Each sloped face is extended outward to its eave, down its own plane, along the corner
//!   trajectories (so edges with different slopes reach different eave heights).
//! - Each face then becomes a slab of the roof's thickness, perpendicular to the face. The slab
//!   is mitred against its neighbours (a plane through each hip, valley or ridge, halfway
//!   between the two faces) and cut plumb at the eaves and rakes.
//!
//! Built so far: straight footprint curves, one outer loop, plate offsets of 0, plumb eave cuts.
//! Slope arrows, openings in the footprint, Align Eaves, other rafter cuts and attached walls
//! come later.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::sketch::{self, SKETCH_KEY};
use crate::store::Store;
use crate::walls::{Line, RECIPE_KEY};

pub mod skeleton;

use skeleton::{Across, SkelEdge};

pub const ROOF_RECIPE: &str = "recipe.roof";
pub const ROOF_TYPE_RECIPE: &str = "recipe.roof_type";
pub const CLASS: &str = "IfcRoof";
pub const TYPE_CLASS: &str = "IfcRoofType";
pub const DEFAULT_TYPE_NAME: &str = "Generic Roof - 300mm";
/// The slope of new roofs: 30°.
pub const DEFAULT_SLOPE: f64 = std::f64::consts::PI / 6.0;
/// The steepest slope a roof edge may have (ARC-ROOF-001: 89.9°).
pub const MAX_SLOPE: f64 = 89.9 * std::f64::consts::PI / 180.0;
/// Slopes below this are flat.
const FLAT: f64 = 1e-6;

fn yes() -> bool {
    true
}

/// What one footprint curve does to the roof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoofEdge {
    /// Whether the edge slopes (otherwise it is a gable).
    #[serde(default = "yes")]
    pub defines_slope: bool,
    /// The edge's own slope (radians); the roof's slope when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<f64>,
    /// How far the roof reaches past the edge, metres.
    #[serde(default)]
    pub overhang: f64,
}

impl Default for RoofEdge {
    fn default() -> Self {
        Self {
            defines_slope: true,
            slope: None,
            overhang: 0.0,
        }
    }
}

/// Where a cutoff stops the roof: a level and an offset above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Cutoff {
    pub level: String,
    #[serde(default)]
    pub offset: f64,
}

/// The stored recipe of a roof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoofRecipe {
    pub kind: String,
    /// The base level (GlobalId).
    pub level: String,
    /// Height of the plate above the level, metres.
    #[serde(default)]
    pub base_offset: f64,
    /// The slope of every slope-defining edge without its own, radians.
    pub slope: f64,
    /// Footprint curve data by curve key; curves without an entry slope at `slope` with no
    /// overhang.
    #[serde(default)]
    pub edges: BTreeMap<String, RoofEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutoff: Option<Cutoff>,
}

impl RoofRecipe {
    /// The data of curve `key` (defaults if it has none).
    pub fn edge(&self, key: u32) -> RoofEdge {
        self.edges
            .get(&key.to_string())
            .cloned()
            .unwrap_or_default()
    }
}

/// The recipe of a Reviera roof.
pub fn roof_recipe(e: &Element) -> Option<RoofRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == ROOF_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Whether an element is a roof type.
pub fn is_roof_type(t: &Element) -> bool {
    t.param(RECIPE_KEY)
        .and_then(|v| v.get("kind"))
        .and_then(Value::as_str)
        == Some(ROOF_TYPE_RECIPE)
}

/// The thickness of a roof type.
pub fn type_thickness(t: &Element) -> Option<f64> {
    if !is_roof_type(t) {
        return None;
    }
    t.param(RECIPE_KEY)?.get("thickness")?.as_f64()
}

/// Triangle mesh: positions and index triples.
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// One sloped (or flat) face of a roof.
#[derive(Debug, Clone, PartialEq)]
pub struct RoofFace {
    /// The footprint curve the face rises from (`None` for a flat top at the cutoff).
    pub key: Option<u32>,
    /// The underside of the face (the roof's reference surface), counter-clockwise from above.
    pub bottom: Vec<[f64; 3]>,
    /// The face's unit normal, pointing up.
    pub normal: [f64; 3],
    /// The slab of the roof's thickness on this face, as a closed mesh.
    pub slab: Mesh,
    /// Courtyards through a flat roof (clockwise), on the underside's plane.
    pub holes: Vec<Vec<[f64; 3]>>,
}

/// A roof as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct RoofGeometry {
    pub faces: Vec<RoofFace>,
    /// The plate elevation (absolute).
    pub plate: f64,
    pub thickness: f64,
    /// The footprint corners (plate lines) and their curve keys.
    pub footprint: Vec<[f64; 2]>,
    pub keys: Vec<u32>,
    /// `IfcRoof.PredefinedType` classified from the faces.
    pub predefined: &'static str,
}

impl RoofGeometry {
    /// The sloped area of the roof's underside.
    pub fn area(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| face_area(&f.bottom) - f.holes.iter().map(|h| face_area(h)).sum::<f64>())
            .sum()
    }

    /// The area seen from above.
    pub fn projected_area(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| {
                let flat = |q: &[[f64; 3]]| {
                    let ring: Vec<[f64; 2]> = q.iter().map(|p| [p[0], p[1]]).collect();
                    sketch::signed_area(&ring).abs()
                };
                flat(&f.bottom) - f.holes.iter().map(|h| flat(h)).sum::<f64>()
            })
            .sum()
    }

    pub fn volume(&self) -> f64 {
        self.area() * self.thickness
    }

    /// The highest point of the roof's top.
    pub fn max_height(&self) -> f64 {
        self.faces
            .iter()
            .flat_map(|f| f.slab.0.iter())
            .map(|p| p[2])
            .fold(self.plate, f64::max)
    }

    /// The height of the roof's underside above plan point `p`, if the roof covers it.
    pub fn underside(&self, p: [f64; 2]) -> Option<f64> {
        self.faces
            .iter()
            .filter(|f| {
                let ring: Vec<[f64; 2]> = f.bottom.iter().map(|q| [q[0], q[1]]).collect();
                // A point on a hip or valley line belongs to the faces on both sides of it.
                let in_hole = f.holes.iter().any(|h| {
                    let h: Vec<[f64; 2]> = h.iter().map(|q| [q[0], q[1]]).collect();
                    inside(p, &h)
                });
                (inside(p, &ring) || on_edge(p, &ring)) && !in_hole
            })
            .map(|f| {
                let n = f.normal;
                let a = f.bottom[0];
                a[2] - (n[0] * (p[0] - a[0]) + n[1] * (p[1] - a[1])) / n[2]
            })
            .reduce(f64::min)
    }
}

/// Whether `p` lies on an edge of `ring` (within a micrometre).
fn on_edge(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    (0..ring.len()).any(|i| {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len2 = d[0] * d[0] + d[1] * d[1];
        if len2 < 1e-18 {
            return false;
        }
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
        (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t) < 1e-6
    })
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

// ---- Vector helpers ----

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit3(a: [f64; 3]) -> [f64; 3] {
    let l = dot3(a, a).sqrt();
    if l < 1e-15 { a } else { scale3(a, 1.0 / l) }
}
fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The true (sloped) area of a planar polygon.
fn face_area(points: &[[f64; 3]]) -> f64 {
    let mut n = [0.0; 3];
    for i in 0..points.len() {
        n = add3(n, cross3(points[i], points[(i + 1) % points.len()]));
    }
    dot3(n, n).sqrt() / 2.0
}

/// Where two plan lines (point, direction) meet.
fn meet(p: [f64; 2], d: [f64; 2], q: [f64; 2], e: [f64; 2]) -> Option<[f64; 2]> {
    let den = cross2(d, e);
    if den.abs() < 1e-12 * (d[0].hypot(d[1]) * e[0].hypot(e[1])).max(1e-30) {
        return None;
    }
    let w = [q[0] - p[0], q[1] - p[1]];
    let t = cross2(w, e) / den;
    Some([p[0] + d[0] * t, p[1] + d[1] * t])
}

/// The point where three planes `n · x = d` meet.
fn three_planes(planes: [([f64; 3], f64); 3]) -> Option<[f64; 3]> {
    let [(a, da), (b, db), (c, dc)] = planes;
    let det = dot3(a, cross3(b, c));
    if det.abs() < 1e-9 {
        return None;
    }
    let p = add3(
        add3(scale3(cross3(b, c), da), scale3(cross3(c, a), db)),
        scale3(cross3(a, b), dc),
    );
    Some(scale3(p, 1.0 / det))
}

/// A closed slab on the planar polygon `bottom` (counter-clockwise from above, normal `normal`
/// up), `thickness` thick along the normal, its sides on the planes through each edge with the
/// given outward normals.
fn slab(bottom: &[[f64; 3]], normal: [f64; 3], cuts: &[[f64; 3]], thickness: f64) -> Mesh {
    let n = bottom.len();
    let top_d = dot3(normal, bottom[0]) + thickness;
    let top: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let prev = (i + n - 1) % n;
            three_planes([
                (normal, top_d),
                (cuts[prev], dot3(cuts[prev], bottom[i])),
                (cuts[i], dot3(cuts[i], bottom[i])),
            ])
            .unwrap_or_else(|| add3(bottom[i], scale3(normal, thickness)))
        })
        .collect();
    let flat: Vec<[f64; 2]> = bottom.iter().map(|p| [p[0], p[1]]).collect();
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(flat.iter().copied(), &[] as &[u32], &mut tris);
    let mut positions = bottom.to_vec();
    positions.extend(&top);
    let nn = n as u32;
    let mut indices = Vec::new();
    for t in tris.chunks(3) {
        let ccw = cross2(
            [
                flat[t[1] as usize][0] - flat[t[0] as usize][0],
                flat[t[1] as usize][1] - flat[t[0] as usize][1],
            ],
            [
                flat[t[2] as usize][0] - flat[t[0] as usize][0],
                flat[t[2] as usize][1] - flat[t[0] as usize][1],
            ],
        ) > 0.0;
        let (a, b, c) = if ccw {
            (t[0], t[1], t[2])
        } else {
            (t[0], t[2], t[1])
        };
        // Bottom faces down, top faces up.
        indices.extend([a, c, b]);
        indices.extend([nn + a, nn + b, nn + c]);
    }
    for i in 0..nn {
        let j = (i + 1) % nn;
        indices.extend([i, j, nn + j, i, nn + j, nn + i]);
    }
    (positions, indices)
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// Regenerates a roof from its recipe, footprint, levels and type.
pub fn regenerate(store: &Store, e: &Element) -> Result<RoofGeometry> {
    let r = roof_recipe(e).ok_or_else(|| Error::new("roof.not_reviera", "not a Reviera roof"))?;
    let level = find(store, &r.level)
        .and_then(|l| store.get(l))
        .ok_or_else(|| {
            Error::new(
                "roof.level_missing",
                "the roof's base level no longer exists",
            )
        })?;
    let thickness = e
        .param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(type_thickness)
        .ok_or_else(|| {
            Error::new(
                "roof.no_type_thickness",
                "the roof type defines no thickness",
            )
        })?;
    let sketch = sketch::sketch_of(e)
        .ok_or_else(|| Error::new("roof.no_sketch", "the roof has no footprint"))?;
    let plate = crate::levels::elevation(level) + r.base_offset;
    let cutoff = match &r.cutoff {
        Some(c) => Some(
            find(store, &c.level)
                .and_then(|l| store.get(l))
                .map(|l| crate::levels::elevation(l) + c.offset)
                .ok_or_else(|| {
                    Error::new(
                        "roof.level_missing",
                        "the roof's cutoff level no longer exists",
                    )
                })?,
        ),
        None => None,
    };
    // The outer loop (the largest), then the courtyards inside it.
    let mut rings: Vec<(Vec<[f64; 2]>, Vec<u32>)> = sketch::rings(store, &sketch)
        .into_iter()
        .filter(|(r, _)| r.len() >= 3)
        .collect();
    rings.sort_by(|a, b| {
        sketch::signed_area(&b.0)
            .abs()
            .total_cmp(&sketch::signed_area(&a.0).abs())
    });
    let Some((outer, _)) = rings.first().cloned() else {
        return Err(Error::new("roof.no_sketch", "the roof has no footprint"));
    };
    let mut loops = Vec::new();
    let (mut ring, mut keys) = (Vec::new(), Vec::new());
    for (k, (r, ks)) in rings.into_iter().enumerate() {
        if k > 0 && !inside(r[0], &outer) {
            continue;
        }
        // The outer loop counter-clockwise, courtyards clockwise.
        let (r, ks) = oriented(r, ks, k == 0);
        loops.push(r.len());
        ring.extend(r);
        keys.extend(ks);
    }
    let edges: Vec<RoofEdge> = keys.iter().map(|k| r.edge(*k)).collect();
    generate_loops(
        &loops, &ring, &keys, &edges, r.slope, plate, cutoff, thickness,
    )
}

/// A loop turned counter-clockwise (or clockwise when `ccw` is false), each corner still
/// starting the curve after it.
fn oriented(ring: Vec<[f64; 2]>, keys: Vec<u32>, ccw: bool) -> (Vec<[f64; 2]>, Vec<u32>) {
    if (sketch::signed_area(&ring) > 0.0) == ccw {
        return (ring, keys);
    }
    let n = ring.len();
    // Reversed, corner j is old corner −j, and the curve after it is old curve −j−1.
    let r = (0..n).map(|j| ring[(n - j) % n]).collect();
    let k = (0..n).map(|j| keys[(2 * n - j - 1) % n]).collect();
    (r, k)
}

/// The roof over the counter-clockwise footprint `ring` (corner `i` starts curve `keys[i]`, with
/// `edges[i]`), from plate height `plate`, cut off at `cutoff` (an elevation).
pub fn generate(
    ring: &[[f64; 2]],
    keys: &[u32],
    edges: &[RoofEdge],
    default_slope: f64,
    plate: f64,
    cutoff: Option<f64>,
    thickness: f64,
) -> Result<RoofGeometry> {
    generate_loops(
        &[ring.len()],
        ring,
        keys,
        edges,
        default_slope,
        plate,
        cutoff,
        thickness,
    )
}

/// The roof over a footprint with courtyards: `ring` holds the loops one after another (their
/// lengths in `loops`), the outer one counter-clockwise first, then each courtyard clockwise.
/// A courtyard's edges slope down toward it like the outer eaves.
#[allow(clippy::too_many_arguments)]
pub fn generate_loops(
    loops: &[usize],
    ring: &[[f64; 2]],
    keys: &[u32],
    edges: &[RoofEdge],
    default_slope: f64,
    plate: f64,
    cutoff: Option<f64>,
    thickness: f64,
) -> Result<RoofGeometry> {
    let n = ring.len();
    // The previous and next corner of each corner, within its loop.
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
    let outer_len = loops.first().copied().unwrap_or(n);
    let fail = |msg: String| Error::new("roof.cannot_generate", msg);
    // Slopes: the edge's own, else the roof's; gables have none.
    let mut slopes: Vec<Option<f64>> = Vec::with_capacity(n);
    for (i, e) in edges.iter().enumerate() {
        if !e.defines_slope {
            slopes.push(None);
            continue;
        }
        let s = e.slope.unwrap_or(default_slope);
        if !(s.is_finite() && (0.0..=MAX_SLOPE).contains(&s)) {
            return Err(Error::new(
                "roof.bad_slope",
                format!("edge {} slope must be between 0° and 89.9°", i + 1),
            ));
        }
        if !(e.overhang.is_finite() && e.overhang >= 0.0) {
            return Err(Error::new(
                "roof.bad_overhang",
                format!("edge {}: the overhang cannot be negative", i + 1),
            ));
        }
        slopes.push(Some(s));
    }
    let dir = |i: usize| {
        let (a, b) = (ring[i], ring[next_of[i]]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = d[0].hypot(d[1]).max(1e-30);
        [d[0] / l, d[1] / l]
    };
    let inward = |i: usize| {
        let d = dir(i);
        [-d[1], d[0]]
    };
    let flat = slopes.iter().all(|s| s.is_none_or(|s| s < FLAT));
    let is_gable = |i: usize| slopes[i].is_none();
    // The roof's outline lines: gables (and every edge of a flat roof) moved out by their
    // overhang; sloped edges on the plate line.
    let shift = |i: usize| {
        if flat || is_gable(i) {
            edges[i].overhang
        } else {
            0.0
        }
    };
    let line = |i: usize| {
        let m = inward(i);
        let o = shift(i);
        ([ring[i][0] - m[0] * o, ring[i][1] - m[1] * o], dir(i))
    };
    let corners: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let prev = prev_of[i];
            let (p, d) = line(prev);
            let (q, e) = line(i);
            meet(p, d, q, e).unwrap_or(q)
        })
        .collect();
    if flat {
        let normal = [0.0, 0.0, 1.0];
        let mut holes = Vec::new();
        let mut first = outer_len;
        for &len in &loops[1..] {
            holes.push(
                corners[first..first + len]
                    .iter()
                    .map(|c| [c[0], c[1], plate])
                    .collect(),
            );
            first += len;
        }
        let (bottom, slab) = if loops.len() > 1 {
            // A flat roof around courtyards: one slab with holes.
            let bottom: Vec<[f64; 3]> = corners[..outer_len]
                .iter()
                .map(|c| [c[0], c[1], plate])
                .collect();
            (bottom, flat_slab(loops, &corners, plate, thickness))
        } else {
            let bottom: Vec<[f64; 3]> = corners.iter().map(|c| [c[0], c[1], plate]).collect();
            let cuts: Vec<[f64; 3]> = (0..n)
                .map(|i| {
                    let d = dir(i);
                    [d[1], -d[0], 0.0]
                })
                .collect();
            let slab = slab(&bottom, normal, &cuts, thickness);
            (bottom, slab)
        };
        return Ok(RoofGeometry {
            faces: vec![RoofFace {
                key: None,
                bottom,
                normal,
                slab,
                holes,
            }],
            plate,
            thickness,
            footprint: ring[..outer_len].to_vec(),
            keys: keys[..outer_len].to_vec(),
            predefined: "FLAT_ROOF",
        });
    }
    // Speeds: horizontal metres per metre of rise. A very shallow edge moves very fast.
    let speed = |i: usize| match slopes[i] {
        None => 0.0,
        Some(s) => 1.0 / s.max(1e-4).tan(),
    };
    let skel_edges: Vec<SkelEdge> = (0..n)
        .map(|i| SkelEdge {
            a: corners[i],
            b: corners[next_of[i]],
            speed: speed(i),
        })
        .collect();
    let skel = skeleton::skeleton_loops(
        loops,
        &skel_edges,
        cutoff.map(|c| c - plate).filter(|t| *t > 0.0),
    )
    .map_err(|e| fail(format!("Roof cannot be generated from this footprint: {e}")))?;
    // Plane normals (up) of each sloped face: the face rises at tan(slope) inward.
    let normal = |i: usize| -> [f64; 3] {
        match slopes[i] {
            None => {
                let m = inward(i);
                [-m[0], -m[1], 0.0]
            }
            Some(s) => {
                let m = inward(i);
                let t = s.max(1e-4).tan();
                unit3([-m[0] * t, -m[1] * t, 1.0])
            }
        }
    };
    // The trajectory of corner i (between edges i-1 and i) per unit of time (rise).
    let velocity = |i: usize| -> Option<[f64; 2]> {
        let prev = prev_of[i];
        let (na, nb) = (inward(prev), inward(i));
        let det = cross2(na, nb);
        if det.abs() < 1e-12 {
            return None;
        }
        let (wa, wb) = (speed(prev), speed(i));
        Some([
            (wa * nb[1] - wb * na[1]) / det,
            (na[0] * wb - nb[0] * wa) / det,
        ])
    };
    // Eave time (height below the plate, negative) of a sloped edge; gables have none.
    let eave_t = |i: usize| -> f64 {
        match slopes[i] {
            None => f64::NEG_INFINITY,
            Some(s) => -edges[i].overhang * s.max(1e-4).tan(),
        }
    };
    let eave_line = |i: usize| {
        let m = inward(i);
        let o = edges[i].overhang;
        ([ring[i][0] - m[0] * o, ring[i][1] - m[1] * o], dir(i))
    };
    let mut faces = Vec::new();
    for (i, f) in skel.faces.iter().enumerate() {
        if is_gable(i) || f.points.len() < 3 {
            continue;
        }
        // The skeleton face: corner i, corner i+1, then around. Extend it below the plate to the
        // eave: [c_e, ..., c_s, Q_s, (E_s), (E_e), Q_e].
        let (prev, next) = (prev_of[i], next_of[i]);
        let ta = eave_t(i);
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut across: Vec<Across> = Vec::new();
        for k in 1..f.points.len() {
            pts.push(f.points[k]);
            across.push(f.across[k]);
        }
        pts.push(f.points[0]);
        // Segment from c_s outward: along the hip with the previous face (or the rake).
        across.push(Across::Face(prev));
        let c_s = f.points[0];
        let c_e = f.points[1];
        let start_ext: Vec<([f64; 3], Across)> = match velocity(i) {
            Some(u) if ta < 0.0 => {
                let tp = eave_t(prev);
                let t = ta.max(tp);
                let q = [c_s[0] + u[0] * t, c_s[1] + u[1] * t, t];
                if ta >= tp {
                    vec![(q, Across::Base)]
                } else {
                    let (p1, d1) = eave_line(prev);
                    let (p2, d2) = eave_line(i);
                    let e = meet(p1, d1, p2, d2).unwrap_or([q[0], q[1]]);
                    vec![(q, Across::Base), ([e[0], e[1], ta], Across::Base)]
                }
            }
            _ => Vec::new(),
        };
        let end_ext: Vec<([f64; 3], Across)> = match velocity(next) {
            Some(u) if ta < 0.0 => {
                let tn = eave_t(next);
                let t = ta.max(tn);
                let q = [c_e[0] + u[0] * t, c_e[1] + u[1] * t, t];
                if ta >= tn {
                    vec![(q, Across::Face(next))]
                } else {
                    let (p1, d1) = eave_line(i);
                    let (p2, d2) = eave_line(next);
                    let e = meet(p1, d1, p2, d2).unwrap_or([q[0], q[1]]);
                    vec![([e[0], e[1], ta], Across::Base), (q, Across::Face(next))]
                }
            }
            _ => Vec::new(),
        };
        if start_ext.is_empty() && end_ext.is_empty() {
            // No overhang: the base segment closes the face.
            *across.last_mut().expect("points") = Across::Base;
        } else {
            for (p, x) in start_ext.into_iter().chain(end_ext) {
                pts.push(p);
                across.push(x);
            }
            if pts.len() == across.len() + 1 {
                across.push(Across::Face(next));
            }
            // The last segment (back to c_e) runs along the next hip.
            *across.last_mut().expect("points") = Across::Face(next);
        }
        // Drop repeated points.
        let mut ring3: Vec<([f64; 3], Across)> = Vec::new();
        for (p, x) in pts.into_iter().zip(across) {
            if let Some(last) = ring3.last()
                && dot3(sub3(last.0, p), sub3(last.0, p)) < 1e-14
            {
                ring3.last_mut().expect("checked").1 = x;
                continue;
            }
            ring3.push((p, x));
        }
        while ring3.len() > 1 {
            let (a, b) = (ring3[0].0, ring3[ring3.len() - 1].0);
            if dot3(sub3(a, b), sub3(a, b)) < 1e-14 {
                ring3.pop();
            } else {
                break;
            }
        }
        if ring3.len() < 3 {
            continue;
        }
        let up = normal(i);
        let bottom: Vec<[f64; 3]> = ring3
            .iter()
            .map(|(p, _)| [p[0], p[1], plate + p[2]])
            .collect();
        let cuts = cut_planes(
            &bottom,
            &ring3.iter().map(|r| r.1).collect::<Vec<_>>(),
            up,
            &|j| {
                if is_gable(j) { None } else { Some(normal(j)) }
            },
        );
        let slab = slab(&bottom, up, &cuts, thickness);
        faces.push(RoofFace {
            key: Some(keys[i]),
            bottom,
            normal: up,
            slab,
            holes: Vec::new(),
        });
    }
    for top in &skel.tops {
        let bottom: Vec<[f64; 3]> = top
            .points
            .iter()
            .map(|p| [p[0], p[1], plate + p[2]])
            .collect();
        let up = [0.0, 0.0, 1.0];
        let cuts = cut_planes(&bottom, &top.across, up, &|j| {
            if is_gable(j) { None } else { Some(normal(j)) }
        });
        let slab = slab(&bottom, up, &cuts, thickness);
        faces.push(RoofFace {
            key: None,
            bottom,
            normal: up,
            slab,
            holes: Vec::new(),
        });
    }
    let predefined = classify(&slopes, &faces, cutoff.is_some() && !skel.tops.is_empty());
    Ok(RoofGeometry {
        faces,
        plate,
        thickness,
        footprint: ring[..outer_len].to_vec(),
        keys: keys[..outer_len].to_vec(),
        predefined,
    })
}

/// A flat slab over loops (the outer counter-clockwise, holes clockwise), `thickness` thick
/// from `z` up: earcut caps and plumb sides around every loop.
fn flat_slab(loops: &[usize], corners: &[[f64; 2]], z: f64, thickness: f64) -> Mesh {
    let n = corners.len();
    let mut holes: Vec<u32> = Vec::new();
    let mut first = 0;
    for (k, len) in loops.iter().enumerate() {
        if k > 0 {
            holes.push(first as u32);
        }
        first += len;
    }
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(corners.iter().copied(), &holes, &mut tris);
    let mut positions: Vec<[f64; 3]> = corners.iter().map(|c| [c[0], c[1], z]).collect();
    positions.extend(corners.iter().map(|c| [c[0], c[1], z + thickness]));
    let nn = n as u32;
    let mut indices = Vec::new();
    for t in tris.chunks(3) {
        let (a, b, c) = (
            corners[t[0] as usize],
            corners[t[1] as usize],
            corners[t[2] as usize],
        );
        let ccw = cross2([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]) > 0.0;
        let (a, b, c) = if ccw {
            (t[0], t[1], t[2])
        } else {
            (t[0], t[2], t[1])
        };
        indices.extend([a, c, b]);
        indices.extend([nn + a, nn + b, nn + c]);
    }
    let mut first = 0u32;
    for &len in loops {
        let len = len as u32;
        for k in 0..len {
            let (i, j) = (first + k, first + (k + 1) % len);
            indices.extend([i, j, nn + j, i, nn + j, nn + i]);
        }
        first += len;
    }
    (positions, indices)
}

/// The outward normal of the side plane through each edge of a face: a mitre halfway to a
/// sloped neighbour, plumb at eaves, rakes and gables.
fn cut_planes(
    bottom: &[[f64; 3]],
    across: &[Across],
    up: [f64; 3],
    neighbour: &dyn Fn(usize) -> Option<[f64; 3]>,
) -> Vec<[f64; 3]> {
    let n = bottom.len();
    (0..n)
        .map(|j| {
            let d = unit3(sub3(bottom[(j + 1) % n], bottom[j]));
            let plumb = unit3([d[1], -d[0], 0.0]);
            let other = match across.get(j) {
                Some(Across::Face(f)) => neighbour(*f),
                Some(Across::Top) => Some([0.0, 0.0, 1.0]),
                _ => None,
            };
            match other {
                Some(m) => {
                    let c = unit3(cross3(d, add3(up, m)));
                    if dot3(c, c) < 0.5 {
                        plumb
                    } else if dot3(c, plumb) < 0.0 {
                        scale3(c, -1.0)
                    } else {
                        c
                    }
                }
                None => plumb,
            }
        })
        .collect()
}

/// `IfcRoof.PredefinedType` from the resolved faces (ARC-ROOF-001).
fn classify(slopes: &[Option<f64>], faces: &[RoofFace], cut: bool) -> &'static str {
    let sloped = faces.iter().filter(|f| f.key.is_some()).count();
    let gables = slopes.iter().filter(|s| s.is_none()).count();
    if sloped == 0 {
        return "FLAT_ROOF";
    }
    if sloped == 1 {
        return "SHED_ROOF";
    }
    if cut {
        return if gables == 0 {
            "MANSARD_ROOF"
        } else {
            "FREEFORM"
        };
    }
    if gables > 0 {
        return if sloped == 2 {
            "GABLE_ROOF"
        } else {
            "HIPPED_GABLE_ROOF"
        };
    }
    // All faces meeting at one apex: every face has the highest point.
    let top = faces
        .iter()
        .flat_map(|f| f.bottom.iter())
        .map(|p| p[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let apex: Vec<[f64; 3]> = faces
        .iter()
        .flat_map(|f| f.bottom.iter().copied())
        .filter(|p| (p[2] - top).abs() < 1e-6)
        .collect();
    let one_apex = apex
        .iter()
        .all(|p| dot3(sub3(*p, apex[0]), sub3(*p, apex[0])) < 1e-10);
    if one_apex && faces.len() >= 3 {
        "PAVILION_ROOF"
    } else if sloped >= 4 {
        "HIP_ROOF"
    } else {
        "FREEFORM"
    }
}

/// The Reviera roofs of a model that regenerate.
pub fn all(store: &Store) -> Vec<(ElementId, RoofGeometry)> {
    store
        .iter()
        .filter(|e| roof_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The project's default roof type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| is_roof_type(e))
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new(TYPE_CLASS)
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "NOTDEFINED")
            .param(
                RECIPE_KEY,
                json!({ "kind": ROOF_TYPE_RECIPE, "thickness": 0.3 }),
            ),
    )
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<String> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != crate::levels::CLASS {
        return Err(Error::new(
            "roof.not_a_level",
            format!("element {id} is not a level"),
        ));
    }
    Ok(e.guid().to_string())
}

fn resolve_roof(store: &Store, r: &ElementRef) -> Result<(ElementId, RoofRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
    let recipe = roof_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "roof.not_reviera",
            format!("element {id} is not a Reviera roof"),
        )
    })?;
    Ok((id, recipe))
}

fn write_recipe(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    r: &RoofRecipe,
) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// Checks that a roof regenerates; on failure the edit is refused with the reason.
fn check(txn: &crate::txn::Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store();
    regenerate(store, store.get(id).expect("exists")).map(|_| ())
}

// ---- Commands ----

/// One footprint curve.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoofCurveArg {
    pub line: Line,
    /// Lock to a wall face or location line: `{element, sub}`.
    pub lock: Option<sketch::LockArg>,
    /// Whether the curve defines slope (default true); a curve that does not is a gable.
    pub defines_slope: Option<bool>,
    /// The curve's own slope (radians, or text with units such as "35°"); default the roof's.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Value>,
    /// How far the roof reaches past the curve (metres or text with units); default 0.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
}

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "roof.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

/// The sketch and per-curve data of footprint curves (keys follow the curves' order).
fn footprint(
    store: &Store,
    curves: &[RoofCurveArg],
) -> Result<(sketch::Sketch, BTreeMap<String, RoofEdge>)> {
    let args: Vec<sketch::CurveArg> = curves
        .iter()
        .map(|c| sketch::CurveArg {
            line: c.line,
            lock: c.lock.clone(),
        })
        .collect();
    let sketch = sketch::from_args(store, &args)?;
    // One outline, with any courtyards inside it.
    if sketch::profiles(store, &sketch).len() != 1 {
        return Err(Error::new(
            "roof.one_loop",
            "a roof footprint is one closed outline, with any courtyards inside it",
        ));
    }
    let mut edges = BTreeMap::new();
    for (i, c) in curves.iter().enumerate() {
        let e = RoofEdge {
            defines_slope: c.defines_slope.unwrap_or(true),
            slope: c
                .slope
                .as_ref()
                .map(|v| number(store, v, "angle", "a slope"))
                .transpose()?,
            overhang: c
                .overhang
                .as_ref()
                .map(|v| number(store, v, "length", "an overhang"))
                .transpose()?
                .unwrap_or(0.0),
        };
        if e != RoofEdge::default() {
            edges.insert((i + 1).to_string(), e);
        }
    }
    Ok((sketch, edges))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A roof type; omitted, the project's default roof type (created on first use).
    pub type_id: Option<ElementRef>,
    /// The footprint: one closed loop of curves (in any order).
    pub curves: Vec<RoofCurveArg>,
    pub level_id: ElementRef,
    /// Height of the plate above the level (metres or text with units); default 0.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    /// The slope of slope-defining curves without their own (radians or "30°"); default 30°.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Value>,
    /// A level where the roof is cut off flat.
    pub cutoff_level_id: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub cutoff_offset: Option<Value>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (sketch, edges) = footprint(&store, &p.curves)?;
    let level = resolve_level(&store, &p.level_id)?;
    let type_id = match &p.type_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such roof type"))?;
            if !store.get(id).is_some_and(is_roof_type) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} is not a roof type"),
                ));
            }
            id
        }
        None => default_type(txn)?,
    };
    let slope = p
        .slope
        .as_ref()
        .map(|v| number(&store, v, "angle", "the slope"))
        .transpose()?
        .unwrap_or(DEFAULT_SLOPE);
    let cutoff = match &p.cutoff_level_id {
        Some(l) => Some(Cutoff {
            level: resolve_level(&store, l)?,
            offset: p
                .cutoff_offset
                .as_ref()
                .map(|v| number(&store, v, "length", "the cutoff offset"))
                .transpose()?
                .unwrap_or(0.0),
        }),
        None => None,
    };
    let recipe = RoofRecipe {
        kind: ROOF_RECIPE.into(),
        level,
        base_offset: p
            .base_offset
            .as_ref()
            .map(|v| number(&store, v, "length", "the base offset"))
            .transpose()?
            .unwrap_or(0.0),
        slope,
        edges,
        cutoff,
    };
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let id = txn.create(
        NewElement::new(CLASS)
            .param("@PredefinedType", "NOTDEFINED")
            .param("@type", type_guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            )
            .param(
                SKETCH_KEY,
                serde_json::to_value(&sketch).expect("serialises"),
            ),
    )?;
    check(txn, id)?;
    classify_into(txn, id)?;
    Ok(id)
}

/// Keeps `@PredefinedType` in step with the geometry.
fn classify_into(txn: &mut crate::txn::Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store();
    let Ok(g) = regenerate(store, store.get(id).expect("exists")) else {
        return Ok(());
    };
    let current = store
        .get(id)
        .and_then(|e| e.param("@PredefinedType"))
        .and_then(Value::as_str);
    if current != Some(g.predefined) {
        txn.modify(id, |e| {
            e.params
                .insert("@PredefinedType".into(), g.predefined.into());
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditFootprintParams {
    pub roof_id: ElementRef,
    pub curves: Vec<RoofCurveArg>,
}

/// Replaces a roof's footprint and its per-curve data (Edit Footprint › Finish).
pub(crate) fn edit_footprint(
    txn: &mut crate::txn::Transaction<'_>,
    p: EditFootprintParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let (sketch, edges) = footprint(&store, &p.curves)?;
    recipe.edges = edges;
    write_recipe(txn, id, &recipe)?;
    let value = serde_json::to_value(&sketch).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SKETCH_KEY.into(), value);
    })?;
    check(txn, id)?;
    classify_into(txn, id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub roof_ids: Vec<ElementRef>,
    /// A new base level.
    pub level_id: Option<ElementRef>,
    /// Plate height above the base level (metres or text with units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    /// The slope of every slope-defining curve (radians or "30°"); clears the curves' own.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Value>,
    /// The cutoff level; `null` removes the cutoff.
    #[serde(default, deserialize_with = "some")]
    pub cutoff_level_id: Option<Option<ElementRef>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub cutoff_offset: Option<Value>,
}

fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// Sets roofs' base level and offset, slope and cutoff (their instance parameters).
pub(crate) fn set_params(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let level = p
        .level_id
        .as_ref()
        .map(|l| resolve_level(&store, l))
        .transpose()?;
    let num = |v: &Option<Value>, t: &str, what: &str| {
        v.as_ref().map(|v| number(&store, v, t, what)).transpose()
    };
    let base_offset = num(&p.base_offset, "length", "the base offset")?;
    let slope = num(&p.slope, "angle", "the slope")?;
    let cutoff_offset = num(&p.cutoff_offset, "length", "the cutoff offset")?;
    let cutoff_level = match &p.cutoff_level_id {
        Some(Some(l)) => Some(Some(resolve_level(&store, l)?)),
        Some(None) => Some(None),
        None => None,
    };
    for r in &p.roof_ids {
        let (id, mut recipe) = resolve_roof(&store, r)?;
        crate::modify::check_unpinned(&store, id)?;
        if let Some(l) = &level {
            recipe.level = l.clone();
        }
        if let Some(o) = base_offset {
            recipe.base_offset = o;
        }
        if let Some(s) = slope {
            recipe.slope = s;
            for e in recipe.edges.values_mut() {
                e.slope = None;
            }
        }
        match &cutoff_level {
            Some(Some(l)) => {
                recipe.cutoff = Some(Cutoff {
                    level: l.clone(),
                    offset: cutoff_offset
                        .or(recipe.cutoff.as_ref().map(|c| c.offset))
                        .unwrap_or(0.0),
                });
            }
            Some(None) => recipe.cutoff = None,
            None => {
                if let (Some(c), Some(o)) = (&mut recipe.cutoff, cutoff_offset) {
                    c.offset = o;
                }
            }
        }
        write_recipe(txn, id, &recipe)?;
        check(txn, id)?;
        classify_into(txn, id)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEdgeParams {
    pub roof_id: ElementRef,
    /// Footprint curves by key (1-based, in the order they were given).
    pub keys: Vec<u32>,
    pub defines_slope: Option<bool>,
    /// The curves' own slope (radians or "35°"); `null` returns them to the roof's slope.
    #[serde(default, deserialize_with = "some")]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Option<Value>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
}

/// Sets footprint curves' Defines Slope, Slope and Overhang.
pub(crate) fn set_edge(txn: &mut crate::txn::Transaction<'_>, p: SetEdgeParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let sketch = sketch::sketch_of(store.get(id).expect("resolved")).unwrap_or_default();
    let slope = match &p.slope {
        Some(Some(v)) => Some(Some(number(&store, v, "angle", "the slope")?)),
        Some(None) => Some(None),
        None => None,
    };
    let overhang = p
        .overhang
        .as_ref()
        .map(|v| number(&store, v, "length", "the overhang"))
        .transpose()?;
    for k in &p.keys {
        if !sketch.curves.iter().any(|c| c.key == *k) {
            return Err(Error::new(
                "roof.no_such_curve",
                format!("the footprint has no curve {k}"),
            ));
        }
        let mut e = recipe.edge(*k);
        if let Some(d) = p.defines_slope {
            e.defines_slope = d;
        }
        if let Some(s) = slope {
            e.slope = s;
        }
        if let Some(o) = overhang {
            e.overhang = o;
        }
        if e == RoofEdge::default() {
            recipe.edges.remove(&k.to_string());
        } else {
            recipe.edges.insert(k.to_string(), e);
        }
    }
    write_recipe(txn, id, &recipe)?;
    check(txn, id)?;
    classify_into(txn, id)
}

/// A roof shape: which footprint curves slope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoofShape {
    /// Every curve slopes.
    Hip,
    /// The curves along the longest one slope; the ends are gables.
    Gable,
    /// Turned a quarter: the curves across the longest one slope.
    GableRotated,
    /// One slope rising from the longest curve; the rest are gables.
    Shed,
    /// One slope rising from the curve opposite the longest one.
    ShedReversed,
    /// No curve slopes.
    Flat,
}

pub const ROOF_SHAPES: &[(RoofShape, &str)] = &[
    (RoofShape::Hip, "Hip"),
    (RoofShape::Gable, "Gable"),
    (RoofShape::GableRotated, "Gable (rotated)"),
    (RoofShape::Shed, "Shed"),
    (RoofShape::ShedReversed, "Shed (reversed)"),
    (RoofShape::Flat, "Flat"),
];

/// Which outer footprint curves (by key) slope for `shape`; `None` when the footprint has no
/// curve opposite its longest one (Shed reversed).
pub fn shape_slopes(sketch: &sketch::Sketch, shape: RoofShape) -> Option<Vec<(u32, bool)>> {
    // Courtyards' curves take the same rule as the outline's (the longest curve is the
    // outline's longest, as every loop's curves are among those compared).
    let outer: Vec<&sketch::SketchCurve> = sketch
        .loops
        .iter()
        .flatten()
        .filter_map(|k| sketch.curves.iter().find(|c| c.key == *k))
        .collect();
    let dir = |c: &sketch::SketchCurve| {
        let (s, e) = (c.line.start, c.line.end);
        let len = (e[0] - s[0]).hypot(e[1] - s[1]);
        (
            [
                (e[0] - s[0]) / len.max(1e-12),
                (e[1] - s[1]) / len.max(1e-12),
            ],
            len,
        )
    };
    // The first of the longest curves (ties within a millimetre).
    let max = outer.iter().map(|c| dir(c).1).fold(0.0, f64::max);
    let longest = outer.iter().copied().find(|c| dir(c).1 > max - 1e-3)?;
    let d = dir(longest).0;
    // Within 10° of the longest curve's direction.
    let parallel = |c: &sketch::SketchCurve| {
        let u = dir(c).0;
        (u[0] * d[1] - u[1] * d[0]).abs() < 10f64.to_radians().sin()
    };
    // The parallel curve farthest from the longest one.
    let opposite = || {
        let n = [-d[1], d[0]];
        let at = |c: &sketch::SketchCurve| {
            let m = c.line.start;
            ((m[0] - longest.line.start[0]) * n[0] + (m[1] - longest.line.start[1]) * n[1]).abs()
        };
        outer
            .iter()
            .copied()
            .filter(|c| c.key != longest.key && parallel(c))
            .max_by(|a, b| at(a).total_cmp(&at(b)))
            .filter(|c| at(c) > 1e-6)
            .map(|c| c.key)
    };
    let slopes = |f: &dyn Fn(&sketch::SketchCurve) -> bool| {
        outer.iter().map(|c| (c.key, f(c))).collect::<Vec<_>>()
    };
    Some(match shape {
        RoofShape::Hip => slopes(&|_| true),
        RoofShape::Flat => slopes(&|_| false),
        RoofShape::Gable => slopes(&|c| parallel(c)),
        RoofShape::GableRotated => slopes(&|c| !parallel(c)),
        RoofShape::Shed => slopes(&|c| c.key == longest.key),
        RoofShape::ShedReversed => {
            let o = opposite()?;
            slopes(&|c| c.key == o)
        }
    })
}

/// The shape a roof has now, if its curves match one (`None` is a custom roof).
pub fn shape_of(e: &Element) -> Option<RoofShape> {
    let recipe = roof_recipe(e)?;
    let sketch = sketch::sketch_of(e)?;
    ROOF_SHAPES.iter().map(|s| s.0).find(|shape| {
        shape_slopes(&sketch, *shape).is_some_and(|v| {
            v.iter()
                .all(|(k, slopes)| recipe.edge(*k).defines_slope == *slopes)
        })
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetShapeParams {
    pub roof_id: ElementRef,
    /// `hip`, `gable`, `gable_rotated`, `shed`, `shed_reversed` or `flat`.
    pub shape: RoofShape,
    /// The overhang of every footprint curve (metres or text with units); default unchanged.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
}

/// Sets which footprint curves slope from a roof shape, keeping their slopes and overhangs.
pub(crate) fn set_shape(txn: &mut crate::txn::Transaction<'_>, p: SetShapeParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = resolve_roof(&store, &p.roof_id)?;
    let sketch = sketch::sketch_of(store.get(id).expect("resolved")).unwrap_or_default();
    let slopes = shape_slopes(&sketch, p.shape).ok_or_else(|| {
        Error::new(
            "roof.shape_unsuitable",
            "the footprint has no curve opposite its longest one for a reversed shed",
        )
    })?;
    let (_, mut recipe) = resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let overhang = p
        .overhang
        .as_ref()
        .map(|v| number(&store, v, "length", "the overhang"))
        .transpose()?;
    for (k, defines) in slopes {
        let mut e = recipe.edge(k);
        e.defines_slope = defines;
        if let Some(o) = overhang {
            e.overhang = o;
        }
        if e == RoofEdge::default() {
            recipe.edges.remove(&k.to_string());
        } else {
            recipe.edges.insert(k.to_string(), e);
        }
    }
    write_recipe(txn, id, &recipe)?;
    check(txn, id)?;
    classify_into(txn, id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSlopeParams {
    pub roof_id: ElementRef,
    /// Radians or text with units ("35°").
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Value,
    /// Footprint curves by key; omitted, every slope-defining curve.
    pub edge_keys: Option<Vec<u32>>,
}

/// `roof.set_slope` (ARC-ROOF-001): the slope of some curves, or of the whole roof.
pub(crate) fn set_slope(txn: &mut crate::txn::Transaction<'_>, p: SetSlopeParams) -> Result<()> {
    match p.edge_keys {
        Some(keys) => set_edge(
            txn,
            SetEdgeParams {
                roof_id: p.roof_id,
                keys,
                defines_slope: None,
                slope: Some(Some(p.slope)),
                overhang: None,
            },
        ),
        None => set_params(
            txn,
            SetParams {
                roof_ids: vec![p.roof_id],
                level_id: None,
                base_offset: None,
                slope: Some(p.slope),
                cutoff_level_id: None,
                cutoff_offset: None,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
    }

    fn edges(n: usize, f: impl Fn(usize) -> RoofEdge) -> Vec<RoofEdge> {
        (0..n).map(f).collect()
    }

    /// Signed volume of a closed mesh.
    fn volume(m: &Mesh) -> f64 {
        m.1.chunks(3)
            .map(|t| {
                let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
                dot3(a, cross3(b, c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn the_specified_rectangles_at_30_degrees() {
        // ARC-ROOF-001 acceptance: a 10 × 6 hip at 30° has a 4 m ridge at 3·tan 30° above the
        // plate; with gable ends the ridge runs the full 10 m at the same height.
        let ring = rect(10.0, 6.0);
        let h = 3.0 * DEFAULT_SLOPE.tan();
        let hip = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |_| RoofEdge::default()),
            DEFAULT_SLOPE,
            0.0,
            None,
            0.2,
        )
        .unwrap();
        let ridge: Vec<[f64; 3]> = hip
            .faces
            .iter()
            .flat_map(|f| f.bottom.iter().copied())
            .filter(|p| (p[2] - h).abs() < 1e-9)
            .collect();
        let xs = ridge.iter().map(|p| p[0]);
        let (lo, hi) = xs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!((hi - lo - 4.0).abs() < 1e-9, "{lo}..{hi}");
        let gable = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |i| RoofEdge {
                defines_slope: i % 2 == 0,
                ..RoofEdge::default()
            }),
            DEFAULT_SLOPE,
            0.0,
            None,
            0.2,
        )
        .unwrap();
        assert!((gable.underside([0.01, 3.0]).unwrap() - h).abs() < 1e-6);
        assert!((gable.underside([9.99, 3.0]).unwrap() - h).abs() < 1e-6);
    }

    #[test]
    fn a_hip_roof_at_45_degrees() {
        let ring = rect(10.0, 6.0);
        let g = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |_| RoofEdge {
                slope: Some(std::f64::consts::FRAC_PI_4),
                ..RoofEdge::default()
            }),
            DEFAULT_SLOPE,
            3.0,
            None,
            0.2,
        )
        .unwrap();
        assert_eq!(g.faces.len(), 4);
        assert_eq!(g.predefined, "HIP_ROOF");
        // The ridge is 3 m above the plate; the underside covers the footprint.
        assert!((g.underside([5.0, 3.0]).unwrap() - 6.0).abs() < 1e-6);
        assert!((g.projected_area() - 60.0).abs() < 1e-6);
        assert!((g.area() - 60.0 * 2f64.sqrt()).abs() < 1e-6);
        // Each slab is a closed solid with the face's area times the thickness (within the
        // mitres' small differences).
        for f in &g.faces {
            let v = volume(&f.slab);
            assert!(v > 0.0, "slabs face out: {v}");
        }
        let total: f64 = g.faces.iter().map(|f| volume(&f.slab)).sum();
        assert!(
            (total - g.volume()).abs() / g.volume() < 0.05,
            "{total} vs {}",
            g.volume()
        );
    }

    #[test]
    fn gables_and_overhangs() {
        // Gable ends east and west, 0.5 m overhang on the eaves and 0.3 m on the rakes.
        let ring = rect(10.0, 6.0);
        let g = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |i| RoofEdge {
                defines_slope: i % 2 == 0,
                slope: None,
                overhang: if i % 2 == 0 { 0.5 } else { 0.3 },
            }),
            std::f64::consts::FRAC_PI_4,
            3.0,
            None,
            0.2,
        )
        .unwrap();
        assert_eq!(g.predefined, "GABLE_ROOF");
        assert_eq!(g.faces.len(), 2);
        // The eaves are 0.5 m out and 0.5 m below the plate; the rakes 0.3 m out.
        let south = &g.faces[0];
        let ys: Vec<f64> = south.bottom.iter().map(|p| p[1]).collect();
        assert!(
            ys.iter().any(|y| (y + 0.5).abs() < 1e-9),
            "{:?}",
            south.bottom
        );
        let low = south
            .bottom
            .iter()
            .map(|p| p[2])
            .fold(f64::INFINITY, f64::min);
        assert!((low - 2.5).abs() < 1e-9, "{low}");
        let xs: Vec<f64> = south.bottom.iter().map(|p| p[0]).collect();
        assert!(
            xs.iter().any(|x| (x + 0.3).abs() < 1e-9) && xs.iter().any(|x| (x - 10.3).abs() < 1e-9)
        );
        assert!(
            (g.projected_area() - 10.6 * 7.0).abs() < 1e-6,
            "{}",
            g.projected_area()
        );
    }

    #[test]
    fn different_eave_slopes_meet_on_a_hip_and_a_lower_eave() {
        // The south slope is steeper: with the same overhang its eave is lower; the faces still
        // cover the eave outline exactly.
        let ring = rect(10.0, 6.0);
        let g = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |i| RoofEdge {
                slope: Some(if i == 0 { 1.0 } else { 0.5 }),
                overhang: 0.6,
                ..RoofEdge::default()
            }),
            DEFAULT_SLOPE,
            0.0,
            None,
            0.25,
        )
        .unwrap();
        assert!(
            (g.projected_area() - 11.2 * 7.2).abs() < 1e-6,
            "{}",
            g.projected_area()
        );
        for f in &g.faces {
            assert!(volume(&f.slab) > 0.0);
        }
    }

    #[test]
    fn flat_and_cutoff_roofs() {
        let ring = rect(8.0, 8.0);
        let flat = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |_| RoofEdge {
                defines_slope: false,
                overhang: 0.5,
                ..RoofEdge::default()
            }),
            DEFAULT_SLOPE,
            3.0,
            None,
            0.3,
        )
        .unwrap();
        assert_eq!(flat.predefined, "FLAT_ROOF");
        assert!((flat.projected_area() - 81.0).abs() < 1e-9);
        assert!((volume(&flat.faces[0].slab) - 81.0 * 0.3).abs() < 1e-9);
        let cut = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |_| RoofEdge::default()),
            std::f64::consts::FRAC_PI_4,
            3.0,
            Some(4.0),
            0.2,
        )
        .unwrap();
        assert_eq!(cut.predefined, "MANSARD_ROOF");
        assert_eq!(cut.faces.len(), 5);
        assert!((cut.underside([4.0, 4.0]).unwrap() - 4.0).abs() < 1e-9);
        let pyramid = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges(4, |_| RoofEdge::default()),
            std::f64::consts::FRAC_PI_4,
            3.0,
            None,
            0.2,
        )
        .unwrap();
        assert_eq!(pyramid.predefined, "PAVILION_ROOF");
    }
}
