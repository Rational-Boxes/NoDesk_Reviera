// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel modifiers and parametric cuts (STR-STL-008).
//!
//! A **cut** is an element of its own (`IfcOpeningElement`, USERDEFINED with the cut's kind as
//! ObjectType) on a target member or plate. Its cutter solid is worked out from the members
//! whenever bodies are built, so it follows them, and the solid kernel subtracts it from the
//! target's body (`reviera-geom`); IFC gets the opening voiding the target. The cuts one command
//! makes are grouped in a **sub-connection** (`IfcElementAssembly`, USERDEFINED SUB_CONNECTION).
//!
//! - **Cope**: the secondary member's end loses what lies in the main member's flange zones
//!   (their thickness plus a clearance), across the main member's flange width plus the clearance;
//!   it follows the main member's depth. The member cut is the one whose end is at the joint
//!   (or the one named).
//! - **Skewed cope**: a notch `length` long and `depth` deep at the top and/or bottom of a member's
//!   end, its inner face turned by `angle`.
//! - **Miter**: two members meeting at their ends are each cut on the plane halving the angle
//!   between them.
//! - **Saw cut**: a member's end is cut flush with the other member's nearer flange or web face.
//! - **Shorten**: a beam or brace end moves back by `length` (forward when negative) through its
//!   end extension; `angle_h` and `angle_w` tilt the cut across its height and width.
//! - **Contour cut**: a closed contour on a work plane or face, cut `depth` deep (through by
//!   default).
//! - **Cut through** and **cut by**: the target loses the other element's body (grown by `gap`).
//! - **Corner cuts** change a plate's contour: straight (a chamfer), convex (rounded) or concave
//!   (a notch arc), `size` from the corner along each edge.
//!
//! **Uncope** removes the copes on members; a sub-connection left without cuts goes with them;
//! copes belonging to a connection are skipped.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{PlaneArgs, WorkPlaneRef};

pub const CUT_RECIPE: &str = "recipe.steel_cut";
pub const SUB_RECIPE: &str = "recipe.sub_connection";

type V3 = [f64; 3];

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
    let l = dot(a, a).sqrt();
    if l < 1e-12 { a } else { mul(a, 1.0 / l) }
}
/// `v` turned by `angle` (radians) about unit axis `k`.
fn rotate(v: V3, k: V3, angle: f64) -> V3 {
    let (c, s) = (angle.cos(), angle.sin());
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SawPart {
    Flange,
    Web,
}

/// What a cut does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CutKind {
    Cope { by: String, clearance: f64 },
    CopeSkewed { end: End, depth: f64, length: f64, angle: f64, top: bool, bottom: bool },
    Miter { with: String },
    Saw { with: String, part: SawPart },
    Shorten { end: End, length: f64, angle_h: f64, angle_w: f64 },
    Contour { plane: WorkPlaneRef, fallback: [V3; 3], contour: Vec<[f64; 2]>, depth: Option<f64> },
    Through { cutter: String, gap: f64 },
    By { cutter: String },
}

impl CutKind {
    pub fn name(&self) -> &'static str {
        match self {
            CutKind::Cope { .. } => "COPE",
            CutKind::CopeSkewed { .. } => "COPE_SKEWED",
            CutKind::Miter { .. } => "MITER",
            CutKind::Saw { .. } => "SAW_CUT",
            CutKind::Shorten { .. } => "SHORTEN",
            CutKind::Contour { .. } => "CONTOUR_CUT",
            CutKind::Through { .. } => "CUT_THROUGH",
            CutKind::By { .. } => "CUT_BY",
        }
    }

    fn is_cope(&self) -> bool {
        matches!(self, CutKind::Cope { .. } | CutKind::CopeSkewed { .. })
    }

    /// The other element it is made from, if any (GlobalId).
    fn other(&self) -> Option<&str> {
        match self {
            CutKind::Cope { by, .. } => Some(by),
            CutKind::Miter { with } | CutKind::Saw { with, .. } => Some(with),
            CutKind::Through { cutter, .. } | CutKind::By { cutter } => Some(cutter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CutRecipe {
    pub kind: String,
    /// The element cut (GlobalId).
    pub target: String,
    pub cut: CutKind,
    /// Its sub-connection (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_connection: Option<String>,
    /// A detailed connection it belongs to (GlobalId): not removed by Uncope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

pub fn cut_recipe(e: &Element) -> Option<CutRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == CUT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn is_sub_connection(e: &Element) -> bool {
    e.param(RECIPE_KEY).and_then(|v| v.get("kind")).and_then(|k| k.as_str()) == Some(SUB_RECIPE)
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
}

// ---- Cutter solids ----

/// A box in a frame: `o + x·ex + y·ey + z·ez` for x, y, z in their ranges.
fn cuboid(o: V3, ex: V3, ey: V3, ez: V3, xr: [f64; 2], yr: [f64; 2], zr: [f64; 2]) -> Mesh {
    let p = |x: f64, y: f64, z: f64| add(o, add(mul(ex, x), add(mul(ey, y), mul(ez, z))));
    let (pts, idx) = crate::bodies::extrude(
        &[[xr[0], yr[0]], [xr[1], yr[0]], [xr[1], yr[1]], [xr[0], yr[1]]],
        &[],
        zr[0],
        zr[1],
    );
    let pts = pts.into_iter().map(|q| p(q[0], q[1], q[2])).collect();
    // A left-handed frame turns the faces inside out.
    if dot(cross(ex, ey), ez) < 0.0 {
        let idx = idx.chunks(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        return (pts, idx);
    }
    (pts, idx)
}

/// A large box on the far side of a plane (where `dot(x - o, n) > 0`), around `near`.
fn half_space(o: V3, n: V3, near: V3, size: f64) -> Mesh {
    let n = unit(n);
    let t = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let ex = unit(cross(t, n));
    let ey = cross(n, ex);
    // Centred on the point of the plane nearest `near`.
    let c = sub(near, mul(n, dot(sub(near, o), n)));
    cuboid(c, ex, ey, n, [-size, size], [-size, size], [0.0, 2.0 * size])
}

fn frame_of(store: &Store, guid: &str) -> Option<crate::steel_detail::Frame> {
    crate::steel_detail::member_frame(store, by_guid(store, guid)?)
}

fn end_point(f: &crate::steel_detail::Frame, end: End) -> (V3, V3) {
    // The end and the direction out of the member there.
    match end {
        End::Start => (f.start, mul(f.axis, -1.0)),
        End::End => (f.end, f.axis),
    }
}

/// The end of member `f` nearest a point.
fn nearest_end(f: &crate::steel_detail::Frame, p: V3) -> End {
    let d = |q: V3| dot(sub(q, p), sub(q, p));
    if d(f.start) <= d(f.end) { End::Start } else { End::End }
}

fn dist_to_axis(f: &crate::steel_detail::Frame, p: V3) -> f64 {
    let t = dot(sub(p, f.start), f.axis).clamp(0.0, dot(sub(f.end, f.start), f.axis));
    let q = add(f.start, mul(f.axis, t));
    dot(sub(p, q), sub(p, q)).sqrt()
}

const CUT_SIZE: f64 = 2.0;

/// The solids a cut subtracts from its target, given the bodies of other elements (for cut
/// through and cut by). `None` when its members are gone or do not fit.
pub fn cutter(store: &Store, r: &CutRecipe, body_of: &dyn Fn(ElementId) -> Option<Vec<Mesh>>) -> Option<Vec<Mesh>> {
    match &r.cut {
        CutKind::Cope { by, clearance } => {
            let s = frame_of(store, &r.target)?;
            let g = frame_of(store, by)?;
            let end = if dist_to_axis(&g, s.start) <= dist_to_axis(&g, s.end) { End::Start } else { End::End };
            let (p, _) = end_point(&s, end);
            // In the main member's frame, centred where the secondary's end is.
            let along = dot(sub(p, g.start), g.axis);
            let o = add(g.start, mul(g.axis, along));
            let (d, bf, tf, c) = (g.ty.depth, g.ty.width, g.ty.flange_thickness, *clearance);
            let half_s = s.ty.width.max(s.ty.depth) / 2.0 + 0.05;
            let reach = bf / 2.0 + c;
            let mut out = Vec::new();
            // The secondary's extent up and down the main member's depth.
            let (s_lo, s_hi) = {
                let mid = dot(sub(add(s.start, mul(sub(s.end, s.start), 0.5)), o), g.deep);
                let h = s.ty.depth / 2.0;
                (mid - h, mid + h)
            };
            if s_hi > d / 2.0 - tf - c {
                out.push(cuboid(o, g.axis, g.across, g.deep, [-half_s, half_s], [-reach, reach], [d / 2.0 - tf - c, s_hi + 1.0]));
            }
            if s_lo < -d / 2.0 + tf + c {
                out.push(cuboid(o, g.axis, g.across, g.deep, [-half_s, half_s], [-reach, reach], [s_lo - 1.0, -d / 2.0 + tf + c]));
            }
            Some(out)
        }
        CutKind::CopeSkewed { end, depth, length, angle, top, bottom } => {
            let s = frame_of(store, &r.target)?;
            let (p, out_dir) = end_point(&s, *end);
            // Inward along the member, turned about its depth direction.
            let inward = rotate(mul(out_dir, -1.0), s.deep, angle.to_radians());
            let side = unit(cross(s.deep, inward));
            let (h, w) = (s.ty.depth / 2.0, s.ty.width / 2.0 + 0.05);
            let mut out = Vec::new();
            if *top {
                out.push(cuboid(p, inward, side, s.deep, [-0.5, *length], [-w - 0.5, w + 0.5], [h - depth, h + 0.5]));
            }
            if *bottom {
                out.push(cuboid(p, inward, side, s.deep, [-0.5, *length], [-w - 0.5, w + 0.5], [-h - 0.5, -h + depth]));
            }
            Some(out)
        }
        CutKind::Miter { with } => {
            let a = frame_of(store, &r.target)?;
            let b = frame_of(store, with)?;
            // The joint: the nearest points of the two axes.
            let (pa, pb) = closest(a.start, a.axis, b.start, b.axis)?;
            let j = mul(add(pa, pb), 0.5);
            // Into each member from the joint.
            let into = |f: &crate::steel_detail::Frame| {
                let e = nearest_end(f, j);
                mul(end_point(f, e).1, -1.0)
            };
            let (da, db) = (into(&a), into(&b));
            let n = unit(sub(db, da));
            // Member a keeps the side toward da: cut where dot(x - j, n) > 0.
            Some(vec![half_space(j, n, j, CUT_SIZE)])
        }
        CutKind::Saw { with, part } => {
            let a = frame_of(store, &r.target)?;
            let b_el = by_guid(store, with)?;
            let mid = mul(add(a.start, a.end), 0.5);
            let faces: &[&str] = match part {
                SawPart::Flange => &["flange_1", "flange_2"],
                SawPart::Web => &["web_1", "web_2"],
            };
            // The face of b toward a's body.
            let face = faces
                .iter()
                .filter_map(|f| crate::steel_detail::face_plane(store, b_el, f).ok())
                .max_by(|x, y| dot(sub(mid, x.origin), x.normal).total_cmp(&dot(sub(mid, y.origin), y.normal)))?;
            let end = nearest_end(&a, face.origin);
            let (p, _) = end_point(&a, end);
            Some(vec![half_space(face.origin, mul(face.normal, -1.0), p, CUT_SIZE)])
        }
        CutKind::Shorten { end, angle_h, angle_w, .. } => {
            if *angle_h == 0.0 && *angle_w == 0.0 {
                return Some(Vec::new());
            }
            // The end, already moved by the extension: a cut through it, tilted.
            let s = frame_of(store, &r.target)?;
            let (p, out_dir) = end_point(&s, *end);
            let n = rotate(rotate(out_dir, s.across, angle_h.to_radians()), s.deep, angle_w.to_radians());
            Some(vec![half_space(p, n, p, CUT_SIZE)])
        }
        CutKind::Contour { plane, fallback, contour, depth } => {
            let pl = crate::work_planes::resolve(store, plane)
                .or_else(|_| crate::work_planes::Plane::new(fallback[0], fallback[1], fallback[2]))
                .ok()?;
            let (pts, idx) = crate::bodies::extrude(contour, &[], -depth.unwrap_or(CUT_SIZE), 0.002);
            let pts = pts
                .into_iter()
                .map(|q| add(pl.origin, add(mul(pl.x, q[0]), add(mul(pl.y, q[1]), mul(pl.normal, q[2])))))
                .collect();
            Some(vec![(pts, idx)])
        }
        CutKind::Through { cutter, gap } => {
            let id = GlobalId::parse(cutter).and_then(|g| store.find_by_guid(&g))?;
            Some(body_of(id)?.into_iter().map(|m| grow(m, *gap)).collect())
        }
        CutKind::By { cutter } => {
            let id = GlobalId::parse(cutter).and_then(|g| store.find_by_guid(&g))?;
            body_of(id)
        }
    }
}

/// The nearest points of two lines.
fn closest(p: V3, u: V3, q: V3, v: V3) -> Option<(V3, V3)> {
    let w = sub(p, q);
    let (a, b, c, d, e) = (dot(u, u), dot(u, v), dot(v, v), dot(u, w), dot(v, w));
    let den = a * c - b * b;
    if den.abs() < 1e-12 {
        return None;
    }
    let s = (b * e - c * d) / den;
    let t = (a * e - b * d) / den;
    Some((add(p, mul(u, s)), add(q, mul(v, t))))
}

/// A mesh grown outward by `gap` (each corner moved along its faces' summed normal).
fn grow(m: Mesh, gap: f64) -> Mesh {
    if gap == 0.0 {
        return m;
    }
    let (pts, idx) = m;
    let key = |p: V3| [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64, (p[2] * 1e6).round() as i64];
    let mut normals: std::collections::HashMap<[i64; 3], V3> = std::collections::HashMap::new();
    for t in idx.chunks(3) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let n = unit(cross(sub(b, a), sub(c, a)));
        for p in [a, b, c] {
            let e = normals.entry(key(p)).or_insert([0.0; 3]);
            if !(dot(*e, n) > 0.0 && dot(unit(*e), n) > 1.0 - 1e-6) {
                *e = add(*e, n);
            }
        }
    }
    let pts = pts.iter().map(|p| add(*p, mul(unit(normals[&key(*p)]), gap))).collect();
    (pts, idx)
}

/// Each element's shortening at its start and end (positive shortens), from Shorten modifiers.
pub fn shortenings(store: &Store) -> std::collections::HashMap<String, [f64; 2]> {
    let mut out: std::collections::HashMap<String, [f64; 2]> = std::collections::HashMap::new();
    for e in store.iter() {
        let Some(r) = cut_recipe(e) else { continue };
        if let CutKind::Shorten { end, length, .. } = r.cut {
            let s = out.entry(r.target).or_default();
            s[if end == End::Start { 0 } else { 1 }] += length;
        }
    }
    out
}

/// The cuts on elements: (cut, target) for every cut whose target exists.
pub fn all(store: &Store) -> Vec<(ElementId, ElementId, CutRecipe)> {
    store
        .iter()
        .filter_map(|e| {
            let r = cut_recipe(e)?;
            let t = GlobalId::parse(&r.target).and_then(|g| store.find_by_guid(&g))?;
            Some((e.id(), t, r))
        })
        .collect()
}

// ---- Plate corner cuts ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CornerKind {
    Straight,
    Convex,
    Concave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CornerCut {
    /// The contour's corner (from 0).
    pub corner: usize,
    pub kind: CornerKind,
    pub size: f64,
}

/// A contour with its corner cuts made.
pub fn cut_contour(contour: &[[f64; 2]], cuts: &[CornerCut]) -> Vec<[f64; 2]> {
    let n = contour.len();
    let mut out = Vec::new();
    for (i, c) in contour.iter().enumerate() {
        let Some(k) = cuts.iter().find(|k| k.corner == i) else {
            out.push(*c);
            continue;
        };
        let (prev, next) = (contour[(i + n - 1) % n], contour[(i + 1) % n]);
        let toward = |q: [f64; 2]| {
            let (dx, dy) = (q[0] - c[0], q[1] - c[1]);
            let l = dx.hypot(dy).max(1e-12);
            let s = k.size.min(l * 0.49);
            [c[0] + dx / l * s, c[1] + dy / l * s]
        };
        let (a, b) = (toward(prev), toward(next));
        match k.kind {
            CornerKind::Straight => out.extend([a, b]),
            CornerKind::Convex | CornerKind::Concave => {
                // A quarter-like arc from a to b: rounded about the inside point, or notched about
                // the corner itself.
                let centre = match k.kind {
                    CornerKind::Convex => [a[0] + b[0] - c[0], a[1] + b[1] - c[1]],
                    _ => *c,
                };
                let ang = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
                let (t0, mut t1) = (ang(a), ang(b));
                let tau = std::f64::consts::TAU;
                // The short way round.
                while t1 - t0 > std::f64::consts::PI {
                    t1 -= tau;
                }
                while t0 - t1 > std::f64::consts::PI {
                    t1 += tau;
                }
                let r = ((a[0] - centre[0]).powi(2) + (a[1] - centre[1]).powi(2)).sqrt();
                for s in 0..=8 {
                    let t = t0 + (t1 - t0) * s as f64 / 8.0;
                    out.push([centre[0] + r * t.cos(), centre[1] + r * t.sin()]);
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CornerCutParams {
    pub plate_id: ElementRef,
    /// The contour's corner, from 0.
    pub corner: usize,
    pub kind: CornerKind,
    /// Along each edge from the corner (metres); 0 removes the cut.
    pub size: f64,
}

/// Cuts a plate's corner (`steel_modifier.corner_cut`).
pub(crate) fn corner_cut(txn: &mut crate::txn::Transaction<'_>, p: CornerCutParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.plate_id)?;
    let mut r = store
        .get(id)
        .and_then(crate::steel_detail::plate_recipe)
        .ok_or_else(|| Error::new("plate.not_a_plate", format!("element {id} is not a plate")))?;
    if p.corner >= r.contour.len() {
        return Err(Error::new(
            "corner_cut.corner",
            format!("the plate has corners 0 to {}", r.contour.len() - 1),
        ));
    }
    if p.size < 0.0 {
        return Err(Error::new("corner_cut.size", "a corner cut's size is not negative"));
    }
    r.corner_cuts.retain(|c| c.corner != p.corner);
    if p.size > 0.0 {
        r.corner_cuts.push(CornerCut { corner: p.corner, kind: p.kind, size: p.size });
    }
    txn.set_param(id, RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))
}

// ---- Commands ----

/// Makes a sub-connection grouping cuts on `members`.
fn sub_connection(txn: &mut crate::txn::Transaction<'_>, members: &[String]) -> Result<String> {
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "SUB_CONNECTION")
            .param(RECIPE_KEY, json!({ "kind": SUB_RECIPE, "members": members })),
    )?;
    Ok(txn.store().get(id).expect("made").guid().to_string())
}

fn make_cut(txn: &mut crate::txn::Transaction<'_>, target: &str, cut: CutKind, sub: Option<String>) -> Result<ElementId> {
    let r = CutRecipe { kind: CUT_RECIPE.into(), target: target.into(), cut, sub_connection: sub, connection: None };
    txn.create(
        NewElement::new("IfcOpeningElement")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", r.cut.name())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

fn guid_of(store: &Store, id: ElementId) -> String {
    store.get(id).expect("resolved").guid().to_string()
}

fn framing(store: &Store, id: ElementId, what: &str) -> Result<crate::steel_detail::Frame> {
    store
        .get(id)
        .and_then(|e| crate::steel_detail::member_frame(store, e))
        .ok_or_else(|| Error::new("cut.not_framing", format!("element {id} is not a column, beam or brace ({what})")))
}

/// The cuts made, and their sub-connection.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Made {
    pub cuts: Vec<ElementId>,
    pub sub_connection: Option<ElementId>,
}

fn made(store: &Store, cuts: Vec<ElementId>, sub: Option<String>) -> Made {
    Made { cuts, sub_connection: sub.and_then(|g| GlobalId::parse(&g)).and_then(|g| store.find_by_guid(&g)) }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CopeParams {
    /// Two members that meet.
    pub member_ids: Vec<ElementRef>,
    /// The member to cut (default: the one whose end is at the joint).
    #[serde(default)]
    pub cut_member_id: Option<ElementRef>,
    /// Between the cope and the main member's flange (metres; default 13 mm or 1/2").
    #[serde(default)]
    pub clearance: Option<f64>,
}

/// Copes a member at a joint (`steel_cut.cope`).
pub(crate) fn cope(txn: &mut crate::txn::Transaction<'_>, p: CopeParams) -> Result<Made> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p.member_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    let [a, b] = ids[..] else {
        return Err(Error::new("cut.members", "a cope is between two members"));
    };
    let (fa, fb) = (framing(&store, a, "to cope")?, framing(&store, b, "to cope")?);
    let cut = match &p.cut_member_id {
        Some(r) => {
            let c = resolve(&store, r)?;
            if c != a && c != b {
                return Err(Error::new("cut.members", "the member to cut is one of the two"));
            }
            c
        }
        // The one whose end is nearest the other's axis.
        None => {
            let ea = dist_to_axis(&fb, fa.start).min(dist_to_axis(&fb, fa.end));
            let eb = dist_to_axis(&fa, fb.start).min(dist_to_axis(&fa, fb.end));
            if ea <= eb { a } else { b }
        }
    };
    let main = if cut == a { b } else { a };
    let clearance = p.clearance.unwrap_or(if crate::steel_fab::us_region(&store) { 0.5 * 0.0254 } else { 0.013 });
    let (tg, mg) = (guid_of(&store, cut), guid_of(&store, main));
    let sub = sub_connection(txn, &[tg.clone(), mg.clone()])?;
    let c = make_cut(txn, &tg, CutKind::Cope { by: mg, clearance }, Some(sub.clone()))?;
    Ok(made(txn.store(), vec![c], Some(sub)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PairParams {
    pub a: ElementRef,
    pub b: ElementRef,
    /// For saw cuts: `flange` or `web` of b.
    #[serde(default)]
    pub part: Option<SawPart>,
}

/// Miters two members' ends (`steel_cut.miter`).
pub(crate) fn miter(txn: &mut crate::txn::Transaction<'_>, p: PairParams) -> Result<Made> {
    let store = txn.store().clone();
    let (a, b) = (resolve(&store, &p.a)?, resolve(&store, &p.b)?);
    let (fa, fb) = (framing(&store, a, "to miter")?, framing(&store, b, "to miter")?);
    if closest(fa.start, fa.axis, fb.start, fb.axis).is_none() {
        return Err(Error::new("cut.parallel", "parallel members cannot be mitered"));
    }
    let (ga, gb) = (guid_of(&store, a), guid_of(&store, b));
    let sub = sub_connection(txn, &[ga.clone(), gb.clone()])?;
    let c1 = make_cut(txn, &ga, CutKind::Miter { with: gb.clone() }, Some(sub.clone()))?;
    let c2 = make_cut(txn, &gb, CutKind::Miter { with: ga }, Some(sub.clone()))?;
    Ok(made(txn.store(), vec![c1, c2], Some(sub)))
}

/// Cuts a's end flush with b's flange or web (`steel_cut.saw`).
pub(crate) fn saw(txn: &mut crate::txn::Transaction<'_>, p: PairParams) -> Result<Made> {
    let store = txn.store().clone();
    let (a, b) = (resolve(&store, &p.a)?, resolve(&store, &p.b)?);
    framing(&store, a, "to cut")?;
    framing(&store, b, "to cut against")?;
    let (ga, gb) = (guid_of(&store, a), guid_of(&store, b));
    let sub = sub_connection(txn, &[ga.clone(), gb.clone()])?;
    let c = make_cut(txn, &ga, CutKind::Saw { with: gb, part: p.part.unwrap_or(SawPart::Flange) }, Some(sub.clone()))?;
    Ok(made(txn.store(), vec![c], Some(sub)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CutParams {
    /// The element cut.
    pub cut_id: ElementRef,
    /// The element whose body cuts it.
    pub cutter_id: ElementRef,
    /// Around the cutter (metres), for cut through.
    #[serde(default)]
    pub gap: f64,
}

/// Cuts an element by another's body grown by a gap (`steel_cut.cut_through`).
pub(crate) fn cut_through(txn: &mut crate::txn::Transaction<'_>, p: CutParams) -> Result<Made> {
    cut_with(txn, p, true)
}

/// Cuts an element by another's body (`steel_cut.cut_by`).
pub(crate) fn cut_by(txn: &mut crate::txn::Transaction<'_>, p: CutParams) -> Result<Made> {
    cut_with(txn, p, false)
}

fn cut_with(txn: &mut crate::txn::Transaction<'_>, p: CutParams, through: bool) -> Result<Made> {
    let store = txn.store().clone();
    let (a, b) = (resolve(&store, &p.cut_id)?, resolve(&store, &p.cutter_id)?);
    if a == b {
        return Err(Error::new("cut.members", "an element cannot cut itself"));
    }
    if p.gap < 0.0 {
        return Err(Error::new("cut.gap", "the gap is not negative"));
    }
    let (ga, gb) = (guid_of(&store, a), guid_of(&store, b));
    let sub = sub_connection(txn, &[ga.clone(), gb.clone()])?;
    let cut = if through { CutKind::Through { cutter: gb, gap: p.gap } } else { CutKind::By { cutter: gb } };
    let c = make_cut(txn, &ga, cut, Some(sub.clone()))?;
    Ok(made(txn.store(), vec![c], Some(sub)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShortenParams {
    pub member_id: ElementRef,
    pub end: End,
    /// Metres: positive shortens, negative extends.
    pub length: f64,
    /// Degrees: tilts the cut across the member's height and width.
    #[serde(default)]
    pub angle_h: f64,
    #[serde(default)]
    pub angle_w: f64,
}

/// Shortens (or extends) a beam or brace end (`steel_modifier.shorten`).
pub(crate) fn shorten(txn: &mut crate::txn::Transaction<'_>, p: ShortenParams) -> Result<Made> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.member_id)?;
    if store.get(id).and_then(crate::beams::beam_recipe).is_none() {
        return Err(Error::new("cut.not_framing", format!("element {id} is not a beam or brace")));
    }
    if p.angle_h.abs() >= 80.0 || p.angle_w.abs() >= 80.0 {
        return Err(Error::new("cut.angle", "a shorten cut's angles are under 80°"));
    }
    let g = guid_of(&store, id);
    let c = make_cut(
        txn,
        &g,
        CutKind::Shorten { end: p.end, length: p.length, angle_h: p.angle_h, angle_w: p.angle_w },
        None,
    )?;
    Ok(made(txn.store(), vec![c], None))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CopeSkewedParams {
    pub member_id: ElementRef,
    pub end: End,
    /// Down from the top (or up from the bottom), metres.
    pub depth: f64,
    /// Back from the end, metres.
    pub length: f64,
    /// Degrees the cope's inner face is turned.
    #[serde(default)]
    pub angle: f64,
    #[serde(default = "yes")]
    pub top: bool,
    #[serde(default)]
    pub bottom: bool,
}

fn yes() -> bool {
    true
}

/// Copes a member's end by given sizes (`steel_modifier.cope_skewed`).
pub(crate) fn cope_skewed(txn: &mut crate::txn::Transaction<'_>, p: CopeSkewedParams) -> Result<Made> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.member_id)?;
    let f = framing(&store, id, "to cope")?;
    if p.depth <= 0.0 || p.length <= 0.0 || p.depth >= f.ty.depth {
        return Err(Error::new("cut.size", "a cope's depth and length are more than 0, the depth less than the member's"));
    }
    if !p.top && !p.bottom {
        return Err(Error::new("cut.size", "cope the top, the bottom or both"));
    }
    let g = guid_of(&store, id);
    let sub = sub_connection(txn, std::slice::from_ref(&g))?;
    let c = make_cut(
        txn,
        &g,
        CutKind::CopeSkewed { end: p.end, depth: p.depth, length: p.length, angle: p.angle, top: p.top, bottom: p.bottom },
        Some(sub.clone()),
    )?;
    Ok(made(txn.store(), vec![c], Some(sub)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ContourCutParams {
    pub element_id: ElementRef,
    #[serde(flatten)]
    pub placement: PlaneArgs,
    pub contour: crate::steel_detail::Contour,
    /// Into the element from the plane (metres); through by default.
    #[serde(default)]
    pub depth: Option<f64>,
}

/// Cuts a sketched hole or notch (`steel_modifier.contour_cut`).
pub(crate) fn contour_cut(txn: &mut crate::txn::Transaction<'_>, p: ContourCutParams) -> Result<Made> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.element_id)?;
    let plane = p.placement.to_ref(&store)?;
    let pl = crate::work_planes::resolve(&store, &plane)?;
    let contour = crate::steel_detail::contour_points(&p.contour)?;
    let g = guid_of(&store, id);
    let c = make_cut(
        txn,
        &g,
        CutKind::Contour { plane, fallback: [pl.origin, pl.x, pl.normal], contour, depth: p.depth },
        None,
    )?;
    Ok(made(txn.store(), vec![c], None))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UncopeParams {
    pub member_ids: Vec<ElementRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UncopeReport {
    pub removed: Vec<ElementId>,
    /// Copes belonging to connections, left.
    pub skipped: Vec<ElementId>,
    pub sub_connections_deleted: Vec<ElementId>,
}

/// Removes members' copes (`steel_cut.uncope`).
pub(crate) fn uncope(txn: &mut crate::txn::Transaction<'_>, p: UncopeParams) -> Result<UncopeReport> {
    let store = txn.store().clone();
    let guids: Vec<String> = p
        .member_ids
        .iter()
        .map(|r| resolve(&store, r).map(|i| guid_of(&store, i)))
        .collect::<Result<_>>()?;
    let mut report = UncopeReport { removed: Vec::new(), skipped: Vec::new(), sub_connections_deleted: Vec::new() };
    let mut subs = Vec::new();
    for e in store.iter() {
        let Some(r) = cut_recipe(e) else { continue };
        if !r.cut.is_cope() || !guids.contains(&r.target) {
            continue;
        }
        if r.connection.is_some() {
            report.skipped.push(e.id());
            continue;
        }
        report.removed.push(e.id());
        subs.extend(r.sub_connection.clone());
    }
    if report.removed.is_empty() {
        let msg = if report.skipped.is_empty() {
            "nothing to remove: the members have no copes".to_owned()
        } else {
            format!("nothing to remove: the copes belong to connections ({} skipped)", report.skipped.len())
        };
        return Err(Error::new("uncope.nothing", msg));
    }
    for id in &report.removed {
        txn.delete(*id)?;
    }
    // Sub-connections left with no cuts go too.
    let store = txn.store().clone();
    for g in subs {
        let Some(s) = by_guid(&store, &g) else { continue };
        let left = store.iter().any(|e| cut_recipe(e).is_some_and(|r| r.sub_connection.as_deref() == Some(g.as_str())));
        if !left && !report.sub_connections_deleted.contains(&s.id()) {
            report.sub_connections_deleted.push(s.id());
            txn.delete(s.id())?;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CutInfo {
    pub id: ElementId,
    pub target: Option<ElementId>,
    pub cut: CutKind,
    pub sub_connection: Option<ElementId>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Elements whose cuts to list (every cut when omitted).
    #[serde(default)]
    pub element_ids: Option<Vec<ElementRef>>,
}

/// The cuts on elements (`steel_cut.list`).
pub fn list(store: &Store, p: ListParams) -> Result<Vec<CutInfo>> {
    let only: Option<Vec<String>> = p
        .element_ids
        .map(|ids| ids.iter().map(|r| resolve(store, r).map(|i| guid_of(store, i))).collect::<Result<_>>())
        .transpose()?;
    let find = |g: &str| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g));
    Ok(store
        .iter()
        .filter_map(|e| {
            let r = cut_recipe(e)?;
            if only.as_ref().is_some_and(|o| !o.contains(&r.target)) {
                return None;
            }
            Some(CutInfo {
                id: e.id(),
                target: find(&r.target),
                sub_connection: r.sub_connection.as_deref().and_then(find),
                cut: r.cut,
            })
        })
        .collect())
}

/// Whether an element goes with `guid`: a cut on it or made from it.
pub fn uses(e: &Element, guid: &str) -> bool {
    cut_recipe(e).is_some_and(|r| r.target == guid || r.cut.other() == Some(guid) || r.sub_connection.as_deref() == Some(guid))
}
