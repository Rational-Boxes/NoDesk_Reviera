// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel connection types and their fabrication elements (STR-STL-001, STR-STL-002): a built-in
//! library of standard connections authored from first principles, each with an input signature
//! (its members and their roles), a parameter schema and a generator that makes plates, bolts,
//! anchor rods and welds from the members' sections and the parameters.
//!
//! A typed connection keeps its fabrication elements as elements of their own (`IfcPlate`,
//! `IfcMechanicalFastener` bolts and anchor bolts, `IfcFastener` welds), each with a
//! `recipe.fabrication` naming its connection and its slot in the generator's output. Their
//! geometry follows the connection's members; changing the type or a parameter makes them afresh.
//! Parameters come from the type's defaults (by region: US or metric sizes), then the project's
//! values for the type (`rxd:connection_types`), then the connection's own overrides. Fabrication
//! elements are drawn only in views at Fine detail.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::connections::{self, ConnectionRecipe};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const FAB_RECIPE: &str = "recipe.fabrication";
/// The project's values for connection types' parameters.
pub const TYPES_KEY: &str = "rxd:connection_types";

type V3 = [f64; 3];

/// A parameter of a connection type.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ParamDef {
    pub key: &'static str,
    pub label: &'static str,
    /// `length` (metres) or `count`.
    pub kind: &'static str,
    /// The tab it is shown on (Plates, Bolts, Welds).
    pub tab: &'static str,
    pub metric: f64,
    pub us: f64,
}

/// A connection type: its id, name, group, input signature and parameters.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConnectionType {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    /// Its members, in order: `column`, `beam` (a beam or girder), `brace`, `main` (a column or a
    /// beam).
    pub inputs: &'static [&'static str],
    pub params: Vec<ParamDef>,
    pub description: &'static str,
}

fn p(key: &'static str, label: &'static str, kind: &'static str, tab: &'static str, metric: f64, us: f64) -> ParamDef {
    ParamDef { key, label, kind, tab, metric, us }
}

const IN: f64 = 0.0254;

/// The built-in library.
pub fn library() -> Vec<ConnectionType> {
    vec![
        ConnectionType {
            id: "shear_plate",
            name: "Shear plate (fin plate)",
            group: "Beam–column",
            inputs: &["main", "beam"],
            description: "A plate welded to the main member's face and bolted through the beam's web: a simple shear connection.",
            params: vec![
                p("plate_thickness", "Plate thickness", "length", "Plates", 0.010, 0.375 * IN),
                p("plate_width", "Plate width", "length", "Plates", 0.100, 4.0 * IN),
                p("rows", "Bolt rows", "count", "Bolts", 3.0, 3.0),
                p("bolt_diameter", "Bolt diameter", "length", "Bolts", 0.020, 0.75 * IN),
                p("pitch", "Bolt pitch", "length", "Bolts", 0.070, 3.0 * IN),
                p("edge", "Edge distance", "length", "Bolts", 0.040, 1.5 * IN),
                p("weld_size", "Weld size", "length", "Welds", 0.006, 0.25 * IN),
            ],
        },
        ConnectionType {
            id: "end_plate",
            name: "Extended end plate",
            group: "Beam–column",
            inputs: &["main", "beam"],
            description: "A plate welded across the beam's end, extended above and below its flanges, bolted to the main member: a moment connection.",
            params: vec![
                p("plate_thickness", "Plate thickness", "length", "Plates", 0.020, 0.75 * IN),
                p("extension", "Extension beyond the flanges", "length", "Plates", 0.080, 3.0 * IN),
                p("side", "Plate beyond the flanges' edges", "length", "Plates", 0.025, 1.0 * IN),
                p("bolt_diameter", "Bolt diameter", "length", "Bolts", 0.020, 0.75 * IN),
                p("gauge", "Bolt gauge", "length", "Bolts", 0.100, 5.5 * IN),
                p("inner_offset", "Inner bolts from the flanges", "length", "Bolts", 0.050, 2.0 * IN),
                p("weld_size", "Weld size", "length", "Welds", 0.008, 0.3125 * IN),
            ],
        },
        ConnectionType {
            id: "base_plate",
            name: "Pinned base plate",
            group: "Base plates",
            inputs: &["column"],
            description: "A plate under the column's foot with four anchor rods, the column welded all round.",
            params: vec![
                p("plate_thickness", "Plate thickness", "length", "Plates", 0.025, 1.0 * IN),
                p("projection", "Plate beyond the column", "length", "Plates", 0.075, 3.0 * IN),
                p("anchor_diameter", "Anchor rod diameter", "length", "Bolts", 0.020, 0.75 * IN),
                p("anchor_edge", "Anchor edge distance", "length", "Bolts", 0.040, 1.5 * IN),
                p("anchor_embedment", "Anchor embedment", "length", "Bolts", 0.300, 12.0 * IN),
                p("weld_size", "Weld size", "length", "Welds", 0.008, 0.3125 * IN),
            ],
        },
        ConnectionType {
            id: "gusset",
            name: "Brace gusset plate",
            group: "Bracing",
            inputs: &["main", "brace"],
            description: "A plate in the brace's plane, welded to the main member and bolted through the brace.",
            params: vec![
                p("plate_thickness", "Plate thickness", "length", "Plates", 0.012, 0.5 * IN),
                p("length", "Plate length along the brace", "length", "Plates", 0.400, 16.0 * IN),
                p("height", "Plate height", "length", "Plates", 0.300, 12.0 * IN),
                p("bolts", "Bolts", "count", "Bolts", 3.0, 3.0),
                p("bolt_diameter", "Bolt diameter", "length", "Bolts", 0.020, 0.75 * IN),
                p("pitch", "Bolt pitch", "length", "Bolts", 0.070, 3.0 * IN),
                p("edge", "Edge distance", "length", "Bolts", 0.040, 1.5 * IN),
                p("weld_size", "Weld size", "length", "Welds", 0.006, 0.25 * IN),
            ],
        },
    ]
}

pub fn connection_type(id: &str) -> Option<ConnectionType> {
    library().into_iter().find(|t| t.id == id)
}

pub(crate) fn us_region(store: &Store) -> bool {
    matches!(
        crate::codes::settings(store).code,
        crate::codes::Code::Ibc | crate::codes::Code::Irc
    )
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The project's values for each type's parameters.
pub fn project_values(store: &Store) -> BTreeMap<String, Map<String, Value>> {
    project(store)
        .and_then(|p| p.param(TYPES_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// A connection's parameters: the type's defaults for the region, the project's values, its own
/// overrides.
pub fn params(store: &Store, c: &ConnectionRecipe) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let Some(t) = connection_type(&c.connection_type) else {
        return out;
    };
    let us = us_region(store);
    for d in &t.params {
        out.insert(d.key.to_owned(), if us { d.us } else { d.metric });
    }
    let layers = [
        project_values(store).remove(t.id).unwrap_or_default(),
        c.params.clone(),
    ];
    for m in layers {
        for (k, v) in m {
            if let (Some(slot), Some(x)) = (out.get_mut(&k), v.as_f64()) {
                *slot = x;
            }
        }
    }
    out
}

// ---- Parts ----

/// A fabrication element as generated.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// A flat plate: its outline (convex, in its mid-plane), unit normal and thickness.
    Plate { outline: Vec<V3>, normal: V3, thickness: f64 },
    /// A bolt (or anchor rod) from `start` (its head) to `end`.
    Bolt { start: V3, end: V3, diameter: f64, anchor: bool },
    /// A fillet weld along `path`, its legs along `a` and `b` (unit vectors) of length `size`.
    Weld { path: Vec<V3>, a: V3, b: V3, size: f64 },
    /// A headed shear stud standing from `start` (on the steel) to `end` (its head).
    Stud { start: V3, end: V3, diameter: f64 },
}

impl Part {
    pub fn class(&self) -> (&'static str, &'static str) {
        match self {
            Part::Plate { .. } => ("IfcPlate", "SHEET"),
            Part::Bolt { anchor: false, .. } => ("IfcMechanicalFastener", "BOLT"),
            Part::Bolt { anchor: true, .. } => ("IfcMechanicalFastener", "ANCHORBOLT"),
            Part::Weld { .. } => ("IfcFastener", "WELD"),
            Part::Stud { .. } => ("IfcMechanicalFastener", "STUDSHEARCONNECTOR"),
        }
    }
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    mul(a, 1.0 / l)
}
/// p + Σ vᵢ·sᵢ
fn at(p: V3, terms: &[(V3, f64)]) -> V3 {
    terms.iter().fold(p, |q, (v, s)| add(q, mul(*v, *s)))
}

/// A beam's frame at the end it meets the join with: the end point on its location line, the
/// unit vector into the beam, its left and up, the section centre's offsets (left, up) from the
/// location line, and its section.
struct BeamEnd {
    end: V3,
    into: V3,
    left: V3,
    up: V3,
    centre: V3,
    ty: crate::columns::ColumnType,
}

fn beam_end(store: &Store, id: ElementId, join: V3) -> Option<BeamEnd> {
    let e = store.get(id)?;
    crate::beams::beam_recipe(e)?;
    let g = crate::beams::regenerate(store, e).ok()?;
    let d = |p: V3| dot(sub(p, join), sub(p, join));
    let (end, other) = if d(g.a) <= d(g.b) { (g.a, g.b) } else { (g.b, g.a) };
    let into = unit(sub(other, end));
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &g.outline {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let (cl, cu) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    // The left and up the outline is measured in, made to agree with `into`.
    let (left, up) = if dot(g.u, into) >= 0.0 {
        (g.left, g.up)
    } else {
        (mul(g.left, -1.0), g.up)
    };
    let centre = at(end, &[(g.left, cl), (g.up, cu)]);
    Some(BeamEnd { end, into, left, up, centre, ty: g.ty })
}

/// How far from `p` (on a column's axis) its section's face is, along plan direction `d`.
fn column_face(store: &Store, id: ElementId, d: V3) -> Option<f64> {
    let e = store.get(id)?;
    let r = crate::columns::column_recipe(e)?;
    let g = crate::columns::regenerate(store, e).ok()?;
    let (c, s) = (r.rotation.cos(), r.rotation.sin());
    let (ex, ey) = ([c, s, 0.0], [-s, c, 0.0]);
    let dp = unit([d[0], d[1], 0.0]);
    let (hx, hy) = (g.ty.width / 2.0, g.ty.depth / 2.0);
    let k = (dot(dp, ex).abs() / hx.max(1e-6)).max(dot(dp, ey).abs() / hy.max(1e-6));
    Some(if k > 1e-9 { 1.0 / k } else { 0.0 })
}

/// How far from the join the main member's face is, along `into` (the connected member's
/// direction): a column's section boundary, or half a beam's web.
fn main_face(store: &Store, main: ElementId, into: V3) -> f64 {
    if let Some(f) = column_face(store, main, into) {
        return f;
    }
    store
        .get(main)
        .and_then(|e| crate::beams::regenerate(store, e).ok())
        .map_or(0.0, |g| g.ty.web_thickness / 2.0)
}

/// The fabrication elements a connection's type makes, in order (their slots).
pub fn parts(store: &Store, e: &Element) -> Result<Vec<Part>> {
    if let Some(p) = crate::steel_detail::pattern_recipe(e) {
        return crate::steel_detail::pattern_parts(store, &p);
    }
    let c = connections::recipe(e)
        .ok_or_else(|| Error::new("connection.not_a_connection", "not a connection"))?;
    let Some(t) = connection_type(&c.connection_type) else {
        return Ok(Vec::new());
    };
    let v = params(store, &c);
    let g = |k: &str| v.get(k).copied().unwrap_or(0.0);
    let members: Vec<ElementId> = c
        .members
        .iter()
        .filter_map(|m| GlobalId::parse(m).and_then(|g| store.find_by_guid(&g)))
        .collect();
    let bad = |m: &str| Error::new("connection.inputs", format!("{}: {m}", t.name));
    let mut out = Vec::new();
    match t.id {
        "base_plate" => {
            let col = *members.first().ok_or_else(|| bad("needs a column"))?;
            let ce = store.get(col).ok_or_else(|| bad("its column is gone"))?;
            let r = crate::columns::column_recipe(ce).ok_or_else(|| bad("its member is not a column"))?;
            let cg = crate::columns::regenerate(store, ce)?;
            let (a, b) = cg.axis();
            let foot = if a[2] <= b[2] { a } else { b };
            let (cs, sn) = (r.rotation.cos(), r.rotation.sin());
            let (ex, ey) = ([cs, sn, 0.0], [-sn, cs, 0.0]);
            let (hx, hy) = (cg.ty.width / 2.0, cg.ty.depth / 2.0);
            let (tp, pr) = (g("plate_thickness"), g("projection"));
            let (px, py) = (hx + pr, hy + pr);
            // On the footing, under the column's foot: the column stands on it.
            let mid = at(foot, &[([0.0, 0.0, 1.0], tp / 2.0)]);
            out.push(Part::Plate {
                outline: [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .iter()
                    .map(|(sx, sy)| at(mid, &[(ex, sx * px), (ey, sy * py)]))
                    .collect(),
                normal: [0.0, 0.0, 1.0],
                thickness: tp,
            });
            let ed = g("anchor_edge");
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let q = at(foot, &[(ex, sx * (px - ed)), (ey, sy * (py - ed))]);
                out.push(Part::Bolt {
                    start: at(q, &[([0.0, 0.0, 1.0], tp + g("anchor_diameter") * 1.5)]),
                    end: at(q, &[([0.0, 0.0, 1.0], -g("anchor_embedment"))]),
                    diameter: g("anchor_diameter"),
                    anchor: true,
                });
            }
            // All round the column's section, on the plate.
            let ring: Vec<V3> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
                .iter()
                .map(|(sx, sy)| at(foot, &[(ex, sx * hx), (ey, sy * hy), ([0.0, 0.0, 1.0], tp)]))
                .collect();
            for w in ring.windows(2) {
                let along = unit(sub(w[1], w[0]));
                let out_dir = cross(along, [0.0, 0.0, 1.0]);
                out.push(Part::Weld {
                    path: vec![w[0], w[1]],
                    a: out_dir,
                    b: [0.0, 0.0, 1.0],
                    size: g("weld_size"),
                });
            }
        }
        "shear_plate" | "end_plate" => {
            let (main, beam) = match members.as_slice() {
                [m, b, ..] => (*m, *b),
                _ => return Err(bad("needs a main member and a beam")),
            };
            let join = connections::regenerate(store, e)?.point;
            let b = beam_end(store, beam, join).ok_or_else(|| bad("its second member is not a beam"))?;
            let s0 = main_face(store, main, b.into) - dot(sub(b.end, join), b.into);
            let (d, bf, tw, tf) = (b.ty.depth, b.ty.width, b.ty.web_thickness, b.ty.flange_thickness);
            let base = at(b.centre, &[(b.into, -dot(sub(b.end, join), b.into))]);
            if t.id == "shear_plate" {
                let (tp, pw, rows) = (g("plate_thickness"), g("plate_width"), g("rows").round().max(1.0) as usize);
                let (pitch, edge) = (g("pitch"), g("edge"));
                let h = (rows - 1) as f64 * pitch + 2.0 * edge;
                let off = tw / 2.0 + tp / 2.0;
                let mid = at(base, &[(b.left, off)]);
                out.push(Part::Plate {
                    outline: [(0.0, -0.5), (1.0, -0.5), (1.0, 0.5), (0.0, 0.5)]
                        .iter()
                        .map(|(x, z)| at(mid, &[(b.into, s0 + x * pw), (b.up, z * h)]))
                        .collect(),
                    normal: b.left,
                    thickness: tp,
                });
                let xb = s0 + pw - edge.min(pw / 2.0);
                let db = g("bolt_diameter");
                for i in 0..rows {
                    let z = -h / 2.0 + edge + i as f64 * pitch;
                    let q = at(base, &[(b.into, xb), (b.up, z)]);
                    out.push(Part::Bolt {
                        start: at(q, &[(b.left, tw / 2.0 + tp + db * 0.6)]),
                        end: at(q, &[(b.left, -(tw / 2.0 + db * 0.9))]),
                        diameter: db,
                        anchor: false,
                    });
                }
                for side in [-1.0, 1.0] {
                    let corner = at(base, &[(b.into, s0), (b.left, off + side * tp / 2.0)]);
                    out.push(Part::Weld {
                        path: vec![at(corner, &[(b.up, -h / 2.0)]), at(corner, &[(b.up, h / 2.0)])],
                        a: b.into,
                        b: mul(b.left, side),
                        size: g("weld_size"),
                    });
                }
            } else {
                let (tp, ext, side, gauge) = (g("plate_thickness"), g("extension"), g("side"), g("gauge"));
                let (w, hh) = (bf + 2.0 * side, d + 2.0 * ext);
                let mid = at(base, &[(b.into, s0 + tp / 2.0)]);
                out.push(Part::Plate {
                    outline: [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
                        .iter()
                        .map(|(y, z)| at(mid, &[(b.left, y * w), (b.up, z * hh)]))
                        .collect(),
                    normal: b.into,
                    thickness: tp,
                });
                let db = g("bolt_diameter");
                let rows = [d / 2.0 + ext / 2.0, d / 2.0 - tf - g("inner_offset"), -(d / 2.0 - tf - g("inner_offset")), -(d / 2.0 + ext / 2.0)];
                for z in rows {
                    for y in [-gauge / 2.0, gauge / 2.0] {
                        let q = at(base, &[(b.into, s0), (b.left, y), (b.up, z)]);
                        out.push(Part::Bolt {
                            start: at(q, &[(b.into, tp + db * 0.6)]),
                            end: at(q, &[(b.into, -(0.03 + db * 0.9))]),
                            diameter: db,
                            anchor: false,
                        });
                    }
                }
                // The beam's flanges and web welded to the plate's face.
                let face = s0 + tp;
                let ws = g("weld_size");
                for (zf, dir) in [(d / 2.0, 1.0), (-d / 2.0, -1.0)] {
                    let q = at(base, &[(b.into, face), (b.up, zf)]);
                    out.push(Part::Weld {
                        path: vec![at(q, &[(b.left, -bf / 2.0)]), at(q, &[(b.left, bf / 2.0)])],
                        a: b.into,
                        b: mul(b.up, dir),
                        size: ws,
                    });
                }
                for s in [-1.0, 1.0] {
                    let q = at(base, &[(b.into, face), (b.left, s * tw / 2.0)]);
                    out.push(Part::Weld {
                        path: vec![at(q, &[(b.up, -(d / 2.0 - tf))]), at(q, &[(b.up, d / 2.0 - tf)])],
                        a: b.into,
                        b: mul(b.left, s),
                        size: ws,
                    });
                }
            }
        }
        "gusset" => {
            let (main, brace) = match members.as_slice() {
                [m, b, ..] => (*m, *b),
                _ => return Err(bad("needs a main member and a brace")),
            };
            let join = connections::regenerate(store, e)?.point;
            let be = store.get(brace).ok_or_else(|| bad("its brace is gone"))?;
            let bg = crate::beams::regenerate(store, be)?;
            let d2 = |p: V3| dot(sub(p, join), sub(p, join));
            let (end, other) = if d2(bg.a) <= d2(bg.b) { (bg.a, bg.b) } else { (bg.b, bg.a) };
            let into = unit(sub(other, end));
            let mut n = cross(into, [0.0, 0.0, 1.0]);
            if dot(n, n) < 1e-9 {
                n = [1.0, 0.0, 0.0];
            }
            let n = unit(n);
            let v = cross(n, into);
            let s0 = main_face(store, main, into).min(0.5);
            let (tp, l, h) = (g("plate_thickness"), g("length"), g("height"));
            // At a column's foot the gusset stops at its base, above the base plate.
            let floor = store
                .get(main)
                .filter(|m| crate::columns::column_recipe(m).is_some())
                .and_then(|m| crate::columns::regenerate(store, m).ok())
                .map(|cg| cg.z0 + base_plate_thickness(store, main));
            let outline: Vec<V3> = [(0.0, -0.5), (1.0, -0.5), (1.0, 0.5), (0.0, 0.5)]
                .iter()
                .map(|(x, y)| at(join, &[(into, s0 + x * l), (v, y * h)]))
                .collect();
            // Beams framing into the column at this joint: the gusset keeps to the brace's side
            // of them (below for a brace running down from the joint, above for one running up).
            let mut floor = floor;
            let mut ceiling: Option<f64> = None;
            if floor.is_some() && into[2].abs() > 0.05 {
                for (b, bg) in crate::beams::all(store) {
                    let framed = bg.supports.iter().any(|s| matches!(s, Some(crate::beams::Support::Column(c)) if *c == main));
                    if b == brace || !framed {
                        continue;
                    }
                    let Some(be) = beam_end(store, b, join) else { continue };
                    let plan = ((be.end[0] - join[0]).powi(2) + (be.end[1] - join[1]).powi(2)).sqrt();
                    let (bottom, top) = (be.centre[2] - be.ty.depth / 2.0, be.centre[2] + be.ty.depth / 2.0);
                    if plan > 1.0 || bottom > join[2] + h || top < join[2] - h {
                        continue;
                    }
                    if into[2] < 0.0 {
                        ceiling = Some(ceiling.map_or(bottom, |c| c.min(bottom)));
                    } else {
                        floor = Some(floor.map_or(top, |f| f.max(top)));
                    }
                }
            }
            let outline = match floor {
                Some(z) => clip_above(&outline, z),
                None => outline,
            };
            let outline = match ceiling {
                Some(z) => clip_below(&outline, z),
                None => outline,
            };
            if outline.len() >= 3 {
                out.push(Part::Plate { outline, normal: n, thickness: tp });
            }
            let (db, pitch, edge) = (g("bolt_diameter"), g("pitch"), g("edge"));
            let half_brace = bg.ty.width / 2.0;
            for i in 0..(g("bolts").round().max(1.0) as usize) {
                let x = s0 + l - edge - i as f64 * pitch;
                if x < s0 + edge {
                    break;
                }
                let q = at(join, &[(into, x)]);
                // Not where the plate was cut back.
                if floor.is_some_and(|z| q[2] < z + db) || ceiling.is_some_and(|z| q[2] > z - db) {
                    continue;
                }
                out.push(Part::Bolt {
                    start: at(q, &[(n, half_brace + db * 0.6)]),
                    end: at(q, &[(n, -(half_brace + db * 0.9))]),
                    diameter: db,
                    anchor: false,
                });
            }
            for s in [-1.0, 1.0] {
                let q = at(join, &[(into, s0), (n, s * tp / 2.0)]);
                let path = vec![at(q, &[(v, -h / 2.0)]), at(q, &[(v, h / 2.0)])];
                let path = match floor {
                    Some(z) => clip_above(&path, z),
                    None => path,
                };
                let path = match ceiling {
                    Some(z) => clip_below(&path, z),
                    None => path,
                };
                if path.len() < 2 {
                    continue;
                }
                out.push(Part::Weld {
                    path: path[..2].to_vec(),
                    a: into,
                    b: mul(n, s),
                    size: g("weld_size"),
                });
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The thickness of the base plate under a column, or zero.
fn base_plate_thickness(store: &Store, column: ElementId) -> f64 {
    let Some(guid) = store.get(column).map(|c| c.guid().to_string()) else {
        return 0.0;
    };
    store
        .iter()
        .filter_map(connections::recipe)
        .find(|c| c.connection_type == "base_plate" && c.members.first() == Some(&guid))
        .and_then(|c| params(store, &c).get("plate_thickness").copied())
        .unwrap_or(0.0)
}

/// The part of a convex outline (or a two-point line) at or below height `z`.
fn clip_below(points: &[V3], z: f64) -> Vec<V3> {
    let flip = |p: &V3| [p[0], p[1], -p[2]];
    let flipped: Vec<V3> = points.iter().map(flip).collect();
    clip_above(&flipped, -z).iter().map(flip).collect()
}

/// The part of a convex outline (or a two-point line) at or above height `z`.
fn clip_above(points: &[V3], z: f64) -> Vec<V3> {
    if points.iter().all(|p| p[2] >= z) {
        return points.to_vec();
    }
    let n = points.len();
    let closed = n > 2;
    let mut out = Vec::new();
    for i in 0..n {
        let (p, q) = (points[i], points[(i + 1) % n]);
        if p[2] >= z {
            out.push(p);
        }
        if (closed || i + 1 < n) && (p[2] >= z) != (q[2] >= z) {
            let t = (z - p[2]) / (q[2] - p[2]);
            out.push(add(p, mul(sub(q, p), t)));
        }
    }
    out
}

// ---- Bodies ----

/// A convex outline extruded symmetrically about its plane.
fn prism(outline: &[V3], normal: V3, thickness: f64) -> crate::bodies::Mesh {
    let h = thickness / 2.0;
    let n = outline.len();
    let mut pts: Vec<V3> = outline.iter().map(|q| at(*q, &[(normal, -h)])).collect();
    pts.extend(outline.iter().map(|q| at(*q, &[(normal, h)])));
    let mut idx = Vec::new();
    for i in 1..n - 1 {
        idx.extend([0, i as u32 + 1, i as u32]);
        idx.extend([n as u32, (n + i) as u32, (n + i + 1) as u32]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, c, d) = (i as u32, j as u32, (n + j) as u32, (n + i) as u32);
        idx.extend([a, b, c, a, c, d]);
    }
    (pts, idx)
}

pub fn cylinder(a: V3, b: V3, r: f64, sides: usize) -> crate::bodies::Mesh {
    let axis = unit(sub(b, a));
    let helper = if axis[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let u = unit(cross(axis, helper));
    let v = cross(axis, u);
    let ring: Vec<V3> = (0..sides)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / sides as f64;
            at(a, &[(u, r * t.cos()), (v, r * t.sin())])
        })
        .collect();
    let len = dot(sub(b, a), axis);
    prism(&ring.iter().map(|q| at(*q, &[(axis, len / 2.0)])).collect::<Vec<_>>(), axis, len)
}

pub(crate) fn join(meshes: Vec<crate::bodies::Mesh>) -> crate::bodies::Mesh {
    let mut pts = Vec::new();
    let mut idx = Vec::new();
    for (p, i) in meshes {
        let base = pts.len() as u32;
        pts.extend(p);
        idx.extend(i.into_iter().map(|x| x + base));
    }
    (pts, idx)
}

impl Part {
    pub fn mesh(&self) -> crate::bodies::Mesh {
        match self {
            Part::Plate { outline, normal, thickness } => prism(outline, *normal, *thickness),
            Part::Bolt { start, end, diameter, anchor } => {
                let r = diameter / 2.0;
                let axis = unit(sub(*end, *start));
                let mut parts = vec![cylinder(*start, *end, r, 8)];
                // A hexagonal head (or an anchor's nut) at the start.
                let head = at(*start, &[(axis, diameter * 0.6)]);
                parts.push(cylinder(*start, head, r * 1.8, 6));
                if *anchor {
                    let hook = at(*end, &[(axis, -diameter * 2.0)]);
                    parts.push(cylinder(hook, *end, r * 1.6, 6));
                }
                join(parts)
            }
            Part::Weld { path, a, b, size } => {
                let mut parts = Vec::new();
                for w in path.windows(2) {
                    let along = unit(sub(w[1], w[0]));
                    let tri = [w[0], at(w[0], &[(*a, *size)]), at(w[0], &[(*b, *size)])];
                    let len = dot(sub(w[1], w[0]), along);
                    let mid: Vec<V3> = tri.iter().map(|q| at(*q, &[(along, len / 2.0)])).collect();
                    parts.push(prism(&mid, along, len));
                }
                join(parts)
            }
            Part::Stud { start, end, diameter } => {
                let r = diameter / 2.0;
                let axis = unit(sub(*end, *start));
                let head = at(*end, &[(axis, -diameter * 0.5)]);
                join(vec![cylinder(*start, head, r, 8), cylinder(head, *end, r * 1.6, 8)])
            }
        }
    }
}

// ---- Fabrication elements ----

/// The stored recipe of a fabrication element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FabRecipe {
    pub kind: String,
    /// Its connection (GlobalId) and its place in the connection's parts.
    pub connection: String,
    pub slot: usize,
}

pub fn fab_recipe(e: &Element) -> Option<FabRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == FAB_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Every fabrication element and its part as it is now.
pub fn all(store: &Store) -> Vec<(ElementId, Part)> {
    let mut by_connection: BTreeMap<String, Vec<(ElementId, usize)>> = BTreeMap::new();
    for e in store.iter() {
        if let Some(r) = fab_recipe(e) {
            by_connection.entry(r.connection).or_default().push((e.id(), r.slot));
        }
    }
    let mut out = Vec::new();
    for (guid, elements) in by_connection {
        let Some(c) = GlobalId::parse(&guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i)) else {
            continue;
        };
        let Ok(ps) = parts(store, c) else { continue };
        for (id, slot) in elements {
            if let Some(p) = ps.get(slot) {
                out.push((id, p.clone()));
            }
        }
    }
    out
}

/// Whether an element depends on `guid`: a fabrication element on its connection.
pub fn uses(e: &Element, guid: &str) -> bool {
    fab_recipe(e).is_some_and(|r| r.connection == guid)
}

/// Makes a connection's fabrication elements afresh.
pub(crate) fn relayout(txn: &mut crate::txn::Transaction<'_>, id: ElementId) -> Result<usize> {
    let store = txn.store().clone();
    let e = store.get(id).expect("a connection");
    let guid = e.guid().to_string();
    // The elements made before, by slot: kept (with their GlobalIds) where the slot's part is
    // still of the same kind, so selections and IFC identities survive an edit.
    let mut old: BTreeMap<usize, ElementId> = BTreeMap::new();
    for f in store.iter() {
        if let Some(r) = fab_recipe(f).filter(|r| r.connection == guid)
            && let Some(dup) = old.insert(r.slot, f.id())
        {
            txn.delete(dup)?;
        }
    }
    let ps = parts(&store, e)?;
    for (slot, p) in ps.iter().enumerate() {
        let (class, predefined) = p.class();
        let bolt = match p {
            Part::Bolt { diameter, start, end, .. } | Part::Stud { diameter, start, end } => {
                Some((*diameter, dot(sub(*end, *start), sub(*end, *start)).sqrt()))
            }
            _ => None,
        };
        let prev = old.remove(&slot);
        let kept = prev.filter(|f| {
            store.get(*f).is_some_and(|f| {
                f.class == class && f.param("@PredefinedType").and_then(Value::as_str) == Some(predefined)
            })
        });
        match kept {
            Some(f) => {
                if let Some((d, len)) = bolt {
                    txn.set_param(f, "rxd:nominal_diameter", d)?;
                    txn.set_param(f, "rxd:nominal_length", len)?;
                }
            }
            None => {
                if let Some(f) = prev {
                    txn.delete(f)?;
                }
                let r = FabRecipe {
                    kind: FAB_RECIPE.into(),
                    connection: guid.clone(),
                    slot,
                };
                let mut ne = NewElement::new(class)
                    .param("@PredefinedType", predefined)
                    .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"));
                if let Some((d, len)) = bolt {
                    ne = ne.param("rxd:nominal_diameter", d).param("rxd:nominal_length", len);
                }
                txn.create(ne)?;
            }
        }
    }
    // Slots the connection no longer has.
    for f in old.into_values() {
        txn.delete(f)?;
    }
    Ok(ps.len())
}

/// Checks that a type fits a connection's members (their count and kinds).
pub(crate) fn check_inputs(store: &Store, t: &ConnectionType, members: &[String]) -> Result<()> {
    if members.len() != t.inputs.len() {
        return Err(Error::new(
            "connection.inputs",
            format!("{} takes {} member(s): {}", t.name, t.inputs.len(), t.inputs.join(", ")),
        ));
    }
    for (k, (role, g)) in t.inputs.iter().zip(members).enumerate() {
        let Some(e) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i)) else {
            continue;
        };
        let column = crate::columns::column_recipe(e).is_some();
        let beam = crate::beams::beam_recipe(e);
        let brace = beam.as_ref().is_some_and(|b| b.brace);
        let ok = match *role {
            "column" => column,
            "beam" => beam.is_some() && !brace,
            "brace" => brace,
            _ => column || beam.is_some(),
        };
        if !ok {
            return Err(Error::new(
                "connection.inputs",
                format!("{}: member {} must be a {role}", t.name, k + 1),
            ));
        }
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    #[serde(flatten)]
    pub def: ConnectionType,
    /// Each parameter's value in this project (the region's default unless changed).
    pub values: BTreeMap<String, f64>,
    /// How many connections in the model have this type.
    pub used: usize,
}

/// The connection library with the project's values (`connection_type.list`).
pub fn list(store: &Store) -> Vec<TypeInfo> {
    library()
        .into_iter()
        .map(|t| {
            let probe = ConnectionRecipe {
                connection_type: t.id.to_owned(),
                ..connections::blank()
            };
            TypeInfo {
                values: params(store, &probe),
                used: store
                    .iter()
                    .filter(|e| connections::recipe(e).is_some_and(|c| c.connection_type == t.id))
                    .count(),
                def: t,
            }
        })
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditTypeParams {
    pub type_id: String,
    /// Parameter values (metres or counts); null returns one to the region's default.
    pub params: Map<String, Value>,
}

fn check_values(t: &ConnectionType, values: &Map<String, Value>) -> Result<()> {
    for (k, v) in values {
        let Some(d) = t.params.iter().find(|d| d.key == k) else {
            return Err(Error::new(
                "connection.unknown_param",
                format!("{} has no parameter `{k}`", t.name),
            ));
        };
        if v.is_null() {
            continue;
        }
        let x = v.as_f64().ok_or_else(|| {
            Error::new("connection.bad_value", format!("`{k}` is a number"))
        })?;
        let ok = if d.kind == "count" { (1.0..=12.0).contains(&x) && x.fract() == 0.0 } else { x > 0.0 && x <= 2.0 };
        if !ok {
            return Err(Error::new(
                "connection.bad_value",
                format!("`{k}` is {}", if d.kind == "count" { "a whole number from 1 to 12" } else { "above 0 and at most 2 m" }),
            ));
        }
    }
    Ok(())
}

/// Changes a connection type's parameters for the project (`connection_type.edit_parameters`):
/// every connection of the type is made afresh.
pub(crate) fn edit_type(txn: &mut crate::txn::Transaction<'_>, p: EditTypeParams) -> Result<usize> {
    let store = txn.store().clone();
    let t = connection_type(&p.type_id).ok_or_else(|| {
        Error::new("connection.unknown_type", format!("no connection type {}", p.type_id))
    })?;
    check_values(&t, &p.params)?;
    let mut all = project_values(&store);
    let mine = all.entry(t.id.to_owned()).or_default();
    for (k, v) in p.params {
        if v.is_null() {
            mine.remove(&k);
        } else {
            mine.insert(k, v);
        }
    }
    let pid = project(&store)
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&all).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(TYPES_KEY.into(), v);
    })?;
    let store = txn.store().clone();
    let ids: Vec<ElementId> = store
        .iter()
        .filter(|e| connections::recipe(e).is_some_and(|c| c.connection_type == t.id))
        .map(Element::id)
        .collect();
    for id in &ids {
        relayout(txn, *id)?;
    }
    Ok(ids.len())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub connection_ids: Vec<ElementRef>,
    /// A type from `connection_type.list`, or `generic`.
    pub connection_type: String,
}

/// Gives connections a type (`structural_connection.set_type`): their members must fit its input
/// signature; their fabrication elements are made.
pub(crate) fn set_type(txn: &mut crate::txn::Transaction<'_>, p: SetTypeParams) -> Result<Vec<ElementId>> {
    let t = if p.connection_type == "generic" {
        None
    } else {
        Some(connection_type(&p.connection_type).ok_or_else(|| {
            Error::new("connection.unknown_type", format!("no connection type {}", p.connection_type))
        })?)
    };
    let mut out = Vec::new();
    for r in &p.connection_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut c = store.get(id).and_then(connections::recipe).ok_or_else(|| {
            Error::new("connection.not_a_connection", format!("element {id} is not a connection"))
        })?;
        if let Some(t) = &t {
            check_inputs(&store, t, &c.members)?;
        }
        c.connection_type = p.connection_type.clone();
        c.params.clear();
        let v = serde_json::to_value(&c).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        relayout(txn, id)?;
        out.push(id);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideParams {
    pub connection_id: ElementRef,
    /// This connection's own values; null returns one to the type's.
    pub params: Map<String, Value>,
}

/// A connection's own parameter values (`structural_connection.override`); its fabrication
/// elements are made afresh and it is flagged as overridden.
pub(crate) fn override_params(txn: &mut crate::txn::Transaction<'_>, p: OverrideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.connection_id)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.connection_id)))?;
    let mut c = store.get(id).and_then(connections::recipe).ok_or_else(|| {
        Error::new("connection.not_a_connection", format!("element {id} is not a connection"))
    })?;
    let t = connection_type(&c.connection_type).ok_or_else(|| {
        Error::new("connection.generic", "a generic connection has no parameters: give it a type first")
    })?;
    check_values(&t, &p.params)?;
    for (k, v) in p.params {
        if v.is_null() {
            c.params.remove(&k);
        } else {
            c.params.insert(k, v);
        }
    }
    c.override_by_instance = !c.params.is_empty();
    let v = serde_json::to_value(&c).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    relayout(txn, id)?;
    Ok(())
}
