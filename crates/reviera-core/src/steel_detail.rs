// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel detailing elements placed by hand (STR-STL-005, STR-STL-006, STR-STL-007): plates
//! sketched on a work plane or a steel face, fastener patterns (bolts, anchors, holes and shear
//! studs) through the elements they join, and welds along an edge where elements touch.
//!
//! **Faces.** A column, beam or brace names its faces in its own frame: `flange_1` and
//! `flange_2` (the outer faces across its depth: a beam's top and bottom, a column's faces along
//! its depth direction), `web_1` and `web_2` (an I, channel or tee section's web faces; the outer
//! sides of other sections), `start` and `end`. A plate names `top` and `bottom`. A face's plane
//! has its origin at the member's start (x along the member) or at the plate's origin, and its
//! normal pointing out; work planes on faces follow their element.
//!
//! **Plates** keep their contour in their plane's coordinates, a thickness (1–200 mm) and a
//! justification: *Bottom* puts the plate on the normal's side of its plane (on a face, outside
//! the element), *Top* on the other side, *Centre* across it. A plate on a face moves with its
//! element; if the element is deleted the plate stays where it was, unhosted.
//!
//! **Patterns** lay out positions in their plane (a rectangular grid or a circle) and run each
//! fastener along the plane's normal through the joined elements. The grip (from where the first
//! element is entered to where the last is left), the bolt length (grip, two washers, a nut and
//! thread beyond, rounded up) and the holes made in each element are found when the pattern is
//! made or edited, from the elements' bodies, and kept; positions follow the plane.
//!
//! **Welds** lie on an edge of one of the welded elements that touches every other within 1 mm.
//! They keep their place when the elements move, and are flagged for review until accepted. A
//! weld or a pattern goes when one of its elements is deleted.
//!
//! Not yet: arcs kept as arcs (they are flattened to segments), coatings and grades from data
//! tables, bolt standards beyond the generic proportional tables, slotted holes (holes are round, cut in
//! the bodies by `reviera-geom` and written to IFC as openings), assembly marks
//! from shop welds, `IfcRelConnectsWithRealizingElements`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::steel_fab::Part;
use crate::store::Store;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{Plane, PlaneArgs, WorkPlaneRef};

pub const PLATE_RECIPE: &str = "recipe.steel_plate";
pub const PATTERN_RECIPE: &str = "recipe.fastener_pattern";
pub const WELD_RECIPE: &str = "recipe.weld";

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
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-12 { a } else { mul(a, 1.0 / l) }
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
}

// ---- Faces ----

/// The faces an element names, if it has any.
pub fn faces_of(store: &Store, e: &Element) -> Vec<&'static str> {
    if plate_recipe(e).is_some() {
        return vec!["top", "bottom"];
    }
    let framing = crate::columns::column_recipe(e).is_some() || crate::beams::beam_recipe(e).is_some();
    if framing && member_frame(store, e).is_some() {
        vec!["flange_1", "flange_2", "web_1", "web_2", "start", "end"]
    } else {
        Vec::new()
    }
}

/// A framing member's frame: its start and end on the section's centre, the axis, the section's
/// width direction and depth direction, and its section.
pub(crate) struct Frame {
    pub start: V3,
    pub end: V3,
    pub axis: V3,
    pub across: V3,
    pub deep: V3,
    pub ty: crate::columns::ColumnType,
}

pub(crate) fn member_frame(store: &Store, e: &Element) -> Option<Frame> {
    if let Some(r) = crate::columns::column_recipe(e) {
        let g = crate::columns::regenerate(store, e).ok()?;
        let (base, top) = g.axis();
        let axis = unit(sub(top, base));
        let (c, s) = (r.rotation.cos(), r.rotation.sin());
        let across = unit(sub([c, s, 0.0], mul(axis, dot([c, s, 0.0], axis))));
        let deep = cross(axis, across);
        return Some(Frame { start: base, end: top, axis, across, deep, ty: g.ty });
    }
    crate::beams::beam_recipe(e)?;
    let g = crate::beams::regenerate(store, e).ok()?;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &g.outline {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let (cl, cu) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let off = |p: V3| add(p, add(mul(g.left, cl), mul(g.up, cu)));
    Some(Frame { start: off(g.start), end: off(g.end), axis: g.u, across: g.left, deep: g.up, ty: g.ty })
}

/// A face's plane: origin, x direction and outward normal.
pub fn face_plane(store: &Store, e: &Element, face: &str) -> Result<Plane> {
    let bad = || {
        Error::new(
            "face.unknown",
            format!("element {} has no face `{face}` (it has: {})", e.id(), faces_of(store, e).join(", ")),
        )
    };
    if let Some(r) = plate_recipe(e) {
        let p = plate_plane(store, &r);
        let (lo, hi) = r.justification.span(r.thickness);
        return match face {
            "top" => Plane::new(add(p.origin, mul(p.normal, hi)), p.x, p.normal),
            "bottom" => Plane::new(add(p.origin, mul(p.normal, lo)), p.x, mul(p.normal, -1.0)),
            _ => Err(bad()),
        };
    }
    let f = member_frame(store, e).ok_or_else(bad)?;
    let half_web = match f.ty.section {
        crate::columns::Section::IShape | crate::columns::Section::Channel | crate::columns::Section::Tee => {
            f.ty.web_thickness / 2.0
        }
        _ => f.ty.width / 2.0,
    };
    let (d, w) = (f.ty.depth / 2.0, half_web);
    match face {
        "flange_1" => Plane::new(add(f.start, mul(f.deep, d)), f.axis, f.deep),
        "flange_2" => Plane::new(add(f.start, mul(f.deep, -d)), f.axis, mul(f.deep, -1.0)),
        "web_1" => Plane::new(add(f.start, mul(f.across, w)), f.axis, f.across),
        "web_2" => Plane::new(add(f.start, mul(f.across, -w)), f.axis, mul(f.across, -1.0)),
        "start" => Plane::new(f.start, f.across, mul(f.axis, -1.0)),
        "end" => Plane::new(f.end, f.across, f.axis),
        _ => Err(bad()),
    }
}

/// The planes stored with a hosted element: its reference, and where it was when last resolved
/// (kept when its host goes).
fn plane_of(store: &Store, r: &WorkPlaneRef, fallback: &[V3; 3]) -> Plane {
    crate::work_planes::resolve(store, r)
        .or_else(|_| Plane::new(fallback[0], fallback[1], fallback[2]))
        .unwrap_or(Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0] })
}

fn frame_of(p: &Plane) -> [V3; 3] {
    [p.origin, p.x, p.normal]
}

// ---- Plates ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Justification {
    /// The plate's top face on the plane.
    Top,
    Centre,
    /// The plate's bottom face on the plane (on a face, the plate sits outside the element).
    #[default]
    Bottom,
}

impl Justification {
    /// The plate's extent along its plane's normal.
    pub fn span(self, t: f64) -> (f64, f64) {
        match self {
            Justification::Top => (-t, 0.0),
            Justification::Centre => (-t / 2.0, t / 2.0),
            Justification::Bottom => (0.0, t),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlateRecipe {
    pub kind: String,
    pub plane: WorkPlaneRef,
    /// Where the plane was when last resolved: origin, x, normal.
    pub fallback: [V3; 3],
    /// The contour in the plane's coordinates (metres), one closed loop.
    pub contour: Vec<[f64; 2]>,
    pub thickness: f64,
    #[serde(default)]
    pub justification: Justification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coating: Option<String>,
    /// What the plate is for (stiffener, cap plate, …), for numbering and schedules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Its host went: it stays where it was.
    #[serde(default)]
    pub unhosted: bool,
    /// Corner cuts (STR-STL-008), applied to the contour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corner_cuts: Vec<crate::steel_cuts::CornerCut>,
}

pub fn plate_recipe(e: &Element) -> Option<PlateRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == PLATE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn plate_plane(store: &Store, r: &PlateRecipe) -> Plane {
    plane_of(store, &r.plane, &r.fallback)
}

/// A contour segment: a line, or an arc through a middle point (flattened).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Segment {
    Line { start: [f64; 2], end: [f64; 2] },
    Arc { start: [f64; 2], mid: [f64; 2], end: [f64; 2] },
}

/// A plate's contour: its corners in order, or its segments (which must join up).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Contour {
    Points(Vec<[f64; 2]>),
    Segments { segments: Vec<Segment> },
}

fn arc_points(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    // The circle through three points.
    let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
    if d.abs() < 1e-12 {
        return vec![a];
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let cx = (sq(a) * (m[1] - b[1]) + sq(m) * (b[1] - a[1]) + sq(b) * (a[1] - m[1])) / d;
    let cy = (sq(a) * (b[0] - m[0]) + sq(m) * (a[0] - b[0]) + sq(b) * (m[0] - a[0])) / d;
    let r = ((a[0] - cx).powi(2) + (a[1] - cy).powi(2)).sqrt();
    let ang = |p: [f64; 2]| (p[1] - cy).atan2(p[0] - cx);
    let (t0, tm, t1) = (ang(a), ang(m), ang(b));
    let tau = std::f64::consts::TAU;
    let norm = |t: f64| t.rem_euclid(tau);
    // Sweep from a to b through m.
    let ccw = norm(tm - t0) < norm(t1 - t0);
    let sweep = if ccw { norm(t1 - t0) } else { -norm(t0 - t1) };
    let n = ((sweep.abs() / (tau / 32.0)).ceil() as usize).max(2);
    (0..n).map(|k| {
        let t = t0 + sweep * k as f64 / n as f64;
        [cx + r * t.cos(), cy + r * t.sin()]
    })
    .collect()
}

fn seg_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 1e-12 && d2 < -1e-12) || (d1 < -1e-12 && d2 > 1e-12))
        && ((d3 > 1e-12 && d4 < -1e-12) || (d3 < -1e-12 && d4 > 1e-12))
}

/// Checks a contour and returns its corners: one closed, simple loop with an area.
pub fn contour_points(c: &Contour) -> Result<Vec<[f64; 2]>> {
    let mut pts = match c {
        Contour::Points(p) => p.clone(),
        Contour::Segments { segments } => {
            let ends = |s: &Segment| match s {
                Segment::Line { start, end } | Segment::Arc { start, end, .. } => (*start, *end),
            };
            let mut out = Vec::new();
            for (i, s) in segments.iter().enumerate() {
                let (_, e) = ends(s);
                let (next, _) = ends(&segments[(i + 1) % segments.len()]);
                let gap = ((e[0] - next[0]).powi(2) + (e[1] - next[1]).powi(2)).sqrt();
                if gap > 1e-4 {
                    return Err(Error::new(
                        "plate.open_contour",
                        format!(
                            "the contour is not closed: segment {} ends {:.1} mm from where segment {} starts",
                            i + 1,
                            gap * 1000.0,
                            (i + 1) % segments.len() + 1
                        ),
                    )
                    .with_details(json!({ "segment": i, "point": e })));
                }
                match s {
                    Segment::Line { start, .. } => out.push(*start),
                    Segment::Arc { start, mid, end } => out.extend(arc_points(*start, *mid, *end)),
                }
            }
            out
        }
    };
    // Drop repeated corners and a closing repeat.
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if pts.len() > 1 {
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        if (f[0] - l[0]).abs() < 1e-9 && (f[1] - l[1]).abs() < 1e-9 {
            pts.pop();
        }
    }
    if pts.len() < 3 {
        return Err(Error::new("plate.contour", "a plate's contour needs at least three corners"));
    }
    let n = pts.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if seg_intersect(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]) {
                return Err(Error::new(
                    "plate.self_intersecting",
                    format!("the contour crosses itself: edges {} and {}", i + 1, j + 1),
                )
                .with_details(json!({ "segments": [i, j] })));
            }
        }
    }
    let area: f64 = (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum::<f64>() / 2.0;
    if area.abs() < 1e-8 {
        return Err(Error::new("plate.contour", "the contour has no area"));
    }
    Ok(pts)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlateCreateParams {
    #[serde(flatten)]
    pub placement: PlaneArgs,
    pub contour: Contour,
    /// Metres; default 10 mm (1–200 mm).
    #[serde(default)]
    pub thickness: Option<f64>,
    #[serde(default)]
    pub justification: Justification,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub coating: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

fn check_thickness(t: f64) -> Result<f64> {
    if !(0.001 - 1e-9..=0.2 + 1e-9).contains(&t) {
        return Err(Error::new("plate.thickness", "a plate is 1 to 200 mm thick"));
    }
    Ok(t)
}

/// Places a plate (`plate.create`).
pub(crate) fn plate_create(txn: &mut crate::txn::Transaction<'_>, p: PlateCreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let plane = p.placement.to_ref(&store)?;
    let contour = contour_points(&p.contour)?;
    let thickness = check_thickness(p.thickness.unwrap_or(0.01))?;
    let resolved = crate::work_planes::resolve(&store, &plane)?;
    let r = PlateRecipe {
        kind: PLATE_RECIPE.into(),
        plane,
        fallback: frame_of(&resolved),
        contour,
        thickness,
        justification: p.justification,
        material: p.material.clone(),
        coating: p.coating.clone(),
        role: p.role.clone(),
        unhosted: false,
        corner_cuts: Vec::new(),
    };
    let mut ne = NewElement::new("IfcPlate")
        .param("@PredefinedType", "SHEET")
        .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"));
    ne = plate_psets(ne, &r);
    txn.create(ne)
}

fn plate_psets(ne: NewElement, r: &PlateRecipe) -> NewElement {
    let mut ne = ne.param("Pset_PlateCommon.LoadBearing", json!({ "type": "IfcBoolean", "value": true }));
    if let Some(role) = &r.role {
        ne = ne.param("Pset_PlateCommon.Reference", json!({ "type": "IfcIdentifier", "value": role }));
    }
    ne
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlateSetParams {
    pub plate_ids: Vec<ElementRef>,
    #[serde(default)]
    pub contour: Option<Contour>,
    #[serde(default)]
    pub thickness: Option<f64>,
    #[serde(default)]
    pub justification: Option<Justification>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub coating: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

/// Changes plates' contour (Edit Contour), thickness, justification, material, coating or role
/// (`plate.set`).
pub(crate) fn plate_set(txn: &mut crate::txn::Transaction<'_>, p: PlateSetParams) -> Result<()> {
    let contour = p.contour.as_ref().map(contour_points).transpose()?;
    let thickness = p.thickness.map(check_thickness).transpose()?;
    for r in &p.plate_ids {
        let store = txn.store().clone();
        let id = resolve(&store, r)?;
        let mut rec = store
            .get(id)
            .and_then(plate_recipe)
            .ok_or_else(|| Error::new("plate.not_a_plate", format!("element {id} is not a plate")))?;
        if let Some(c) = &contour {
            rec.contour = c.clone();
        }
        if let Some(t) = thickness {
            rec.thickness = t;
        }
        if let Some(j) = p.justification {
            rec.justification = j;
        }
        if p.material.is_some() {
            rec.material = p.material.clone();
        }
        if p.coating.is_some() {
            rec.coating = p.coating.clone();
        }
        if p.role.is_some() {
            rec.role = p.role.clone();
        }
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        if let Some(role) = &rec.role {
            txn.set_param(id, "Pset_PlateCommon.Reference", json!({ "type": "IfcIdentifier", "value": role }))?;
        }
    }
    Ok(())
}

/// A plate's body: its contour extruded across its thickness.
pub fn plate_mesh(store: &Store, r: &PlateRecipe) -> crate::bodies::Mesh {
    let p = plate_plane(store, r);
    let (lo, hi) = r.justification.span(r.thickness);
    let outline = crate::steel_cuts::cut_contour(&r.contour, &r.corner_cuts);
    let (pts, idx) = crate::bodies::extrude(&outline, &[], lo, hi);
    let pts = pts
        .into_iter()
        .map(|q| add(p.origin, add(mul(p.x, q[0]), add(mul(p.y, q[1]), mul(p.normal, q[2])))))
        .collect();
    (pts, idx)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlateInfo {
    pub id: ElementId,
    pub plane: String,
    pub hosted: bool,
    pub contour: Vec<[f64; 2]>,
    /// The contour in the world.
    pub outline: Vec<V3>,
    pub normal: V3,
    pub thickness: f64,
    pub justification: Justification,
    pub area: f64,
    pub material: Option<String>,
    pub coating: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IdsParams {
    pub element_ids: Vec<ElementRef>,
}

pub fn plate_info(store: &Store, p: IdsParams) -> Result<Vec<PlateInfo>> {
    p.element_ids
        .iter()
        .map(|r| {
            let id = resolve(store, r)?;
            let rec = store
                .get(id)
                .and_then(plate_recipe)
                .ok_or_else(|| Error::new("plate.not_a_plate", format!("element {id} is not a plate")))?;
            let pl = plate_plane(store, &rec);
            let cut = crate::steel_cuts::cut_contour(&rec.contour, &rec.corner_cuts);
            let n = cut.len();
            let area = (0..n)
                .map(|i| cut[i][0] * cut[(i + 1) % n][1] - cut[(i + 1) % n][0] * cut[i][1])
                .sum::<f64>()
                .abs()
                / 2.0;
            Ok(PlateInfo {
                id,
                plane: crate::work_planes::label(store, &rec.plane),
                hosted: !rec.unhosted && crate::work_planes::resolve(store, &rec.plane).is_ok(),
                outline: cut.iter().map(|c| pl.to_world(*c)).collect(),
                contour: rec.contour.clone(),
                normal: pl.normal,
                thickness: rec.thickness,
                justification: rec.justification,
                area,
                material: rec.material.clone(),
                coating: rec.coating.clone(),
                role: rec.role.clone(),
            })
        })
        .collect()
}

// ---- Fastener patterns ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    #[default]
    Bolt,
    Anchor,
    Hole,
    ShearStud,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    #[default]
    Shop,
    Site,
}

/// Where a pattern's fasteners go in its plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "shape")]
pub enum Layout {
    /// `rows` along y by `columns` along x, `spacing_x` and `spacing_y` apart, centred on `centre`.
    Rect {
        rows: usize,
        columns: usize,
        spacing_x: f64,
        spacing_y: f64,
        #[serde(default)]
        centre: [f64; 2],
    },
    /// `count` round a circle of `radius` about `centre`, the first at `start_angle` (degrees).
    Circle {
        count: usize,
        radius: f64,
        #[serde(default)]
        centre: [f64; 2],
        #[serde(default)]
        start_angle: f64,
    },
}

impl Layout {
    pub fn positions(&self) -> Vec<[f64; 2]> {
        match self {
            Layout::Rect { rows, columns, spacing_x, spacing_y, centre } => {
                let mut out = Vec::new();
                for r in 0..*rows {
                    for c in 0..*columns {
                        out.push([
                            centre[0] + (c as f64 - (*columns as f64 - 1.0) / 2.0) * spacing_x,
                            centre[1] + (r as f64 - (*rows as f64 - 1.0) / 2.0) * spacing_y,
                        ]);
                    }
                }
                out
            }
            Layout::Circle { count, radius, centre, start_angle } => (0..*count)
                .map(|k| {
                    let t = start_angle.to_radians() + std::f64::consts::TAU * k as f64 / *count as f64;
                    [centre[0] + radius * t.cos(), centre[1] + radius * t.sin()]
                })
                .collect(),
        }
    }
}

/// What a pattern found through its elements, kept with it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Grip {
    /// Along the normal from the plane into the elements, where the first is entered (metres).
    pub entry: f64,
    /// From the first entry to the last exit.
    pub grip: f64,
    /// The bolt's length under its head (bolts), rounded up.
    pub length: f64,
    /// Holes made in each element (GlobalId → count).
    pub holes: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PatternRecipe {
    pub kind: String,
    pub pattern: PatternKind,
    /// The elements joined (GlobalIds).
    pub elements: Vec<String>,
    pub plane: WorkPlaneRef,
    pub fallback: [V3; 3],
    pub layout: Layout,
    pub diameter: f64,
    /// Data table of bolt proportions: `generic_metric` or `generic_inch`.
    pub standard: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// Added to the grip (metres).
    #[serde(default)]
    pub grip_increase: f64,
    /// Holes are this much bigger than the fastener (metres).
    pub hole_tolerance: f64,
    /// A slotted hole's length, if slotted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<f64>,
    /// Bolts entered from the far side.
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub location: Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coating: Option<String>,
    /// An anchor's or stud's length (metres).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default)]
    pub found: Grip,
}

pub fn pattern_recipe(e: &Element) -> Option<PatternRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == PATTERN_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A bolt table's proportions: nut height, washer thickness and the thread beyond the nut, as
/// fractions of the diameter, and the length step. Generic proportions, not a standard's table.
fn proportions(standard: &str) -> (f64, f64, f64, f64) {
    match standard {
        "generic_inch" => (0.875, 0.16, 0.3, 0.25 * 0.0254),
        _ => (0.8, 0.15, 0.3, 0.005),
    }
}

/// A bolt's length under its head for a grip: two washers, a nut and thread beyond, rounded up.
pub fn bolt_length(standard: &str, diameter: f64, grip: f64) -> f64 {
    let (nut, washer, thread, step) = proportions(standard);
    let l = grip + diameter * (2.0 * washer + nut + thread);
    (l / step - 1e-9).ceil() * step
}

fn pattern_plane(store: &Store, r: &PatternRecipe) -> Plane {
    plane_of(store, &r.plane, &r.fallback)
}

/// The parts of a pattern: a fastener at each position (none for holes).
pub fn pattern_parts(store: &Store, r: &PatternRecipe) -> Result<Vec<Part>> {
    let pl = pattern_plane(store, r);
    let n = pl.normal;
    let d = r.diameter;
    let (nut, _, _, _) = proportions(&r.standard);
    let into = mul(n, -1.0);
    let mut out = Vec::new();
    for q in r.layout.positions() {
        let p = pl.to_world(q);
        let entry = add(p, mul(into, r.found.entry));
        match r.pattern {
            PatternKind::Hole => {}
            PatternKind::ShearStud => {
                let l = r.length.unwrap_or(0.1);
                out.push(Part::Stud { start: p, end: add(p, mul(n, l)), diameter: d });
            }
            PatternKind::Bolt => {
                let l = r.found.length;
                if r.inverted {
                    let exit = add(entry, mul(into, r.found.grip));
                    out.push(Part::Bolt { start: exit, end: add(exit, mul(n, l)), diameter: d, anchor: false });
                } else {
                    out.push(Part::Bolt { start: entry, end: add(entry, mul(into, l)), diameter: d, anchor: false });
                }
            }
            PatternKind::Anchor => {
                // From a nut above the steel down into the concrete.
                let l = r.length.unwrap_or(0.4);
                let top = add(entry, mul(n, d * (nut + 0.5)));
                out.push(Part::Bolt { start: top, end: add(top, mul(into, l)), diameter: d, anchor: true });
            }
        }
    }
    Ok(out)
}

// -- Mesh queries --

/// Where a line `o + t·d` crosses a mesh's triangles (the `t` values).
fn ray_hits(mesh: &crate::bodies::Mesh, o: V3, d: V3) -> Vec<f64> {
    let (pts, idx) = mesh;
    let mut out = Vec::new();
    for t in idx.chunks(3) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let (e1, e2) = (sub(b, a), sub(c, a));
        let h = cross(d, e2);
        let det = dot(e1, h);
        if det.abs() < 1e-14 {
            continue;
        }
        let f = 1.0 / det;
        let s = sub(o, a);
        let u = f * dot(s, h);
        if !(-1e-9..=1.0 + 1e-9).contains(&u) {
            continue;
        }
        let q = cross(s, e1);
        let v = f * dot(d, q);
        if v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        out.push(f * dot(e2, q));
    }
    out
}

fn point_triangle(p: V3, a: V3, b: V3, c: V3) -> f64 {
    // The closest point on a triangle (Ericson, Real-Time Collision Detection, 5.1.5).
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return len(ap);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return len(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return len(sub(p, add(a, mul(ab, v))));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return len(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return len(sub(p, add(a, mul(ac, w))));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return len(sub(p, add(b, mul(sub(c, b), w))));
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    len(sub(p, add(a, add(mul(ab, v), mul(ac, w)))))
}

/// Where a fastener's line enters and leaves an element first (the first solid run along it;
/// an I-section's far flange is not part of the grip).
fn first_run(mesh: &crate::bodies::Mesh, o: V3, d: V3) -> Option<(f64, f64)> {
    let mut hits = ray_hits(mesh, o, d);
    hits.sort_by(|a, b| a.total_cmp(b));
    hits.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    (hits.len() >= 2).then(|| (hits[0], hits[1]))
}

fn mesh_distance(mesh: &crate::bodies::Mesh, p: V3) -> f64 {
    let (pts, idx) = mesh;
    idx.chunks(3)
        .map(|t| point_triangle(p, pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]))
        .fold(f64::INFINITY, f64::min)
}

/// The meshes of the given elements, one merged mesh each.
fn meshes(store: &Store, ids: &[ElementId]) -> BTreeMap<ElementId, crate::bodies::Mesh> {
    let mut out: BTreeMap<ElementId, crate::bodies::Mesh> = BTreeMap::new();
    for b in crate::bodies::all(store) {
        if !ids.contains(&b.element) {
            continue;
        }
        let m = out.entry(b.element).or_default();
        for part in b.parts {
            let base = m.0.len() as u32;
            m.0.extend(part.mesh.0);
            m.1.extend(part.mesh.1.iter().map(|i| i + base));
        }
    }
    out
}

fn element_ids(store: &Store, guids: &[String]) -> Vec<ElementId> {
    guids.iter().filter_map(|g| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))).collect()
}

/// Finds a pattern's grip and holes from its elements' bodies.
fn find_grip(store: &Store, r: &PatternRecipe) -> Result<Grip> {
    let pl = pattern_plane(store, r);
    let ids = element_ids(store, &r.elements);
    let ms = meshes(store, &ids);
    let into = mul(pl.normal, -1.0);
    let positions = r.layout.positions();
    let mut holes: BTreeMap<String, usize> = BTreeMap::new();
    let (mut entry, mut exit) = (f64::INFINITY, f64::NEG_INFINITY);
    for q in &positions {
        // From well outside the plane, along the normal into the elements.
        let o = add(pl.to_world(*q), mul(pl.normal, 1.0));
        for id in &ids {
            let Some(m) = ms.get(id) else { continue };
            let Some((lo, hi)) = first_run(m, o, into) else { continue };
            entry = entry.min(lo - 1.0);
            exit = exit.max(hi - 1.0);
            let guid = store.get(*id).expect("listed").guid().to_string();
            *holes.entry(guid).or_default() += 1;
        }
    }
    if r.pattern == PatternKind::ShearStud {
        return Ok(Grip { entry: 0.0, grip: 0.0, length: r.length.unwrap_or(0.1), holes: BTreeMap::new() });
    }
    if !entry.is_finite() {
        return Err(Error::new(
            "pattern.misses",
            "the pattern's fasteners pass through none of its elements: pick the face they cross",
        ));
    }
    let grip = (exit - entry) + r.grip_increase;
    let length = match r.pattern {
        PatternKind::Bolt => bolt_length(&r.standard, r.diameter, grip),
        _ => r.length.unwrap_or(0.0),
    };
    if matches!(r.pattern, PatternKind::Hole) {
        return Ok(Grip { entry, grip, length: 0.0, holes });
    }
    Ok(Grip { entry, grip, length, holes })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PatternCreateParams {
    /// The elements joined (a single element for shear studs).
    pub element_ids: Vec<ElementRef>,
    /// The plane the fasteners are square to: a work plane or a face.
    #[serde(flatten)]
    pub placement: PlaneArgs,
    pub layout: Layout,
    /// Metres; default M20 (or 3/4" under US codes); 19 mm for studs.
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub standard: Option<String>,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub grip_increase: f64,
    #[serde(default)]
    pub hole_tolerance: Option<f64>,
    #[serde(default)]
    pub slot: Option<f64>,
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub coating: Option<String>,
    /// An anchor's or stud's length.
    #[serde(default)]
    pub length: Option<f64>,
}

fn us(store: &Store) -> bool {
    crate::steel_fab::us_region(store)
}

fn create_pattern(
    txn: &mut crate::txn::Transaction<'_>,
    p: PatternCreateParams,
    kind: PatternKind,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    match kind {
        PatternKind::ShearStud if ids.len() != 1 => {
            return Err(Error::new("pattern.elements", "shear studs go on one steel element"));
        }
        PatternKind::Bolt | PatternKind::Anchor if ids.len() < 2 => {
            return Err(Error::new("pattern.elements", "a bolt or anchor pattern joins two or more elements"));
        }
        _ if ids.is_empty() => return Err(Error::new("pattern.elements", "select the elements first")),
        _ => {}
    }
    let positions = p.layout.positions();
    if positions.is_empty() {
        return Err(Error::new("pattern.layout", "the layout has no positions"));
    }
    let plane = p.placement.to_ref(&store)?;
    let resolved = crate::work_planes::resolve(&store, &plane)?;
    let us = us(&store);
    let diameter = p.diameter.unwrap_or(match kind {
        PatternKind::ShearStud => 0.019,
        _ if us => 0.75 * 0.0254,
        _ => 0.020,
    });
    if !(0.003..=0.1).contains(&diameter) {
        return Err(Error::new("pattern.diameter", "a fastener is 3 to 100 mm in diameter"));
    }
    let standard = p.standard.clone().unwrap_or_else(|| if us { "generic_inch" } else { "generic_metric" }.into());
    if !matches!(standard.as_str(), "generic_metric" | "generic_inch") {
        return Err(Error::new("pattern.standard", format!("no bolt table {standard} (generic_metric, generic_inch)")));
    }
    let mut r = PatternRecipe {
        kind: PATTERN_RECIPE.into(),
        pattern: kind,
        elements: ids.iter().map(|i| store.get(*i).expect("resolved").guid().to_string()).collect(),
        plane,
        fallback: frame_of(&resolved),
        layout: p.layout.clone(),
        diameter,
        standard,
        grade: p.grade.clone(),
        grip_increase: p.grip_increase,
        hole_tolerance: p.hole_tolerance.unwrap_or(if us { 0.0625 * 0.0254 } else { 0.002 }),
        slot: p.slot,
        inverted: p.inverted,
        location: p.location,
        coating: p.coating.clone(),
        length: p.length.or(match kind {
            PatternKind::ShearStud => Some(0.1),
            PatternKind::Anchor => Some(0.4),
            _ => None,
        }),
        found: Grip::default(),
    };
    r.found = find_grip(&store, &r)?;
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "FASTENER_GROUP")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::steel_fab::relayout(txn, id)?;
    Ok(id)
}

pub(crate) fn bolt_pattern_create(txn: &mut crate::txn::Transaction<'_>, p: PatternCreateParams) -> Result<ElementId> {
    create_pattern(txn, p, PatternKind::Bolt)
}
pub(crate) fn anchor_pattern_create(txn: &mut crate::txn::Transaction<'_>, p: PatternCreateParams) -> Result<ElementId> {
    create_pattern(txn, p, PatternKind::Anchor)
}
pub(crate) fn hole_pattern_create(txn: &mut crate::txn::Transaction<'_>, p: PatternCreateParams) -> Result<ElementId> {
    create_pattern(txn, p, PatternKind::Hole)
}
pub(crate) fn stud_pattern_create(txn: &mut crate::txn::Transaction<'_>, p: PatternCreateParams) -> Result<ElementId> {
    create_pattern(txn, p, PatternKind::ShearStud)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PatternSetParams {
    pub pattern_ids: Vec<ElementRef>,
    #[serde(default)]
    pub layout: Option<Layout>,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub grip_increase: Option<f64>,
    #[serde(default)]
    pub hole_tolerance: Option<f64>,
    #[serde(default)]
    pub inverted: Option<bool>,
    #[serde(default)]
    pub location: Option<Location>,
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub grade: Option<String>,
}

/// Edits patterns (`fastener_pattern.set`); with no changes, finds their grips again (after the
/// elements changed).
pub(crate) fn pattern_set(txn: &mut crate::txn::Transaction<'_>, p: PatternSetParams) -> Result<()> {
    for r in &p.pattern_ids {
        let store = txn.store().clone();
        let id = resolve(&store, r)?;
        let mut rec = store
            .get(id)
            .and_then(pattern_recipe)
            .ok_or_else(|| Error::new("pattern.not_a_pattern", format!("element {id} is not a fastener pattern")))?;
        if let Some(l) = &p.layout {
            rec.layout = l.clone();
        }
        if let Some(d) = p.diameter {
            rec.diameter = d;
        }
        if let Some(g) = p.grip_increase {
            rec.grip_increase = g;
        }
        if let Some(t) = p.hole_tolerance {
            rec.hole_tolerance = t;
        }
        if let Some(i) = p.inverted {
            rec.inverted = i;
        }
        if let Some(l) = p.location {
            rec.location = l;
        }
        if p.length.is_some() {
            rec.length = p.length;
        }
        if p.grade.is_some() {
            rec.grade = p.grade.clone();
        }
        rec.found = find_grip(&store, &rec)?;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        crate::steel_fab::relayout(txn, id)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PatternInfo {
    pub id: ElementId,
    pub pattern: PatternKind,
    pub elements: Vec<ElementId>,
    pub plane: String,
    pub positions: Vec<V3>,
    pub count: usize,
    pub diameter: f64,
    pub standard: String,
    pub grade: Option<String>,
    pub grip: f64,
    /// A bolt's length under its head, an anchor's or stud's length.
    pub length: f64,
    pub hole_diameter: f64,
    pub slot: Option<f64>,
    /// Holes in each element (element id → count).
    pub holes: BTreeMap<String, usize>,
    pub location: Location,
    pub inverted: bool,
    /// The fastener elements.
    pub fasteners: Vec<ElementId>,
}

pub fn pattern_info(store: &Store, p: IdsParams) -> Result<Vec<PatternInfo>> {
    p.element_ids
        .iter()
        .map(|r| {
            let id = resolve(store, r)?;
            let e = store.get(id).expect("resolved");
            let rec = pattern_recipe(e)
                .ok_or_else(|| Error::new("pattern.not_a_pattern", format!("element {id} is not a fastener pattern")))?;
            let pl = pattern_plane(store, &rec);
            let guid = e.guid().to_string();
            let holes = rec
                .found
                .holes
                .iter()
                .filter_map(|(g, n)| {
                    let id = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))?;
                    Some((id.0.to_string(), *n))
                })
                .collect();
            Ok(PatternInfo {
                id,
                pattern: rec.pattern,
                elements: element_ids(store, &rec.elements),
                plane: crate::work_planes::label(store, &rec.plane),
                positions: rec.layout.positions().iter().map(|q| pl.to_world(*q)).collect(),
                count: rec.layout.positions().len(),
                diameter: rec.diameter,
                standard: rec.standard.clone(),
                grade: rec.grade.clone(),
                grip: rec.found.grip,
                length: rec.found.length,
                hole_diameter: rec.diameter + rec.hole_tolerance,
                slot: rec.slot,
                holes,
                location: rec.location,
                inverted: rec.inverted,
                fasteners: store
                    .iter()
                    .filter(|f| crate::steel_fab::fab_recipe(f).is_some_and(|r| r.connection == guid))
                    .map(|f| f.id())
                    .collect(),
            })
        })
        .collect()
}

/// The holes a pattern makes, for IFC openings: in each element, a cylinder from where the
/// fastener enters to where it leaves (start, end, diameter).
pub fn holes(store: &Store, e: &Element) -> Vec<(ElementId, V3, V3, f64)> {
    let Some(r) = pattern_recipe(e) else { return Vec::new() };
    let ms = meshes(store, &element_ids(store, &r.elements));
    holes_in(store, e, &|id| ms.get(&id).cloned())
}

/// The holes a pattern makes, given the elements' bodies (each merged into one mesh).
pub fn holes_in(
    store: &Store,
    e: &Element,
    body_of: &dyn Fn(ElementId) -> Option<crate::bodies::Mesh>,
) -> Vec<(ElementId, V3, V3, f64)> {
    let Some(r) = pattern_recipe(e) else { return Vec::new() };
    if r.pattern == PatternKind::ShearStud {
        return Vec::new();
    }
    let pl = pattern_plane(store, &r);
    let into = mul(pl.normal, -1.0);
    let ids = element_ids(store, &r.elements);
    let ms: BTreeMap<ElementId, crate::bodies::Mesh> = ids.iter().filter_map(|i| Some((*i, body_of(*i)?))).collect();
    let mut out = Vec::new();
    for q in r.layout.positions() {
        let o = add(pl.to_world(q), mul(pl.normal, 1.0));
        for id in &ids {
            let Some(m) = ms.get(id) else { continue };
            let Some((lo, hi)) = first_run(m, o, into) else { continue };
            out.push((*id, add(o, mul(into, lo)), add(o, mul(into, hi)), r.diameter + r.hole_tolerance));
        }
    }
    out
}

// ---- Welds ----

/// The weld types and surface shapes welds take their values from (an open data file).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WeldTables {
    pub types: Vec<String>,
    pub surfaces: Vec<String>,
    pub preparations: Vec<String>,
}

pub fn weld_tables() -> WeldTables {
    serde_json::from_str(include_str!("../data/steel/weld_types.json")).expect("the weld table parses")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Intermittent {
    /// Each weld segment's length and the distance from one segment's start to the next.
    pub length: f64,
    pub pitch: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeldRecipe {
    pub kind: String,
    /// The welded elements (GlobalIds).
    pub elements: Vec<String>,
    /// The picked edge.
    pub edge: [V3; 2],
    /// The weld's legs: along the faces it fills between (unit vectors).
    pub legs: [V3; 2],
    pub weld_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weld_type_2: Option<String>,
    /// The throat thickness (metres).
    pub a: f64,
    /// Shorter than the edge, if given (from the edge's start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default = "flat")]
    pub surface: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<String>,
    #[serde(default)]
    pub location: Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intermittent: Option<Intermittent>,
    /// Each element's shape when the weld was placed or last accepted.
    pub signatures: Vec<String>,
}

fn flat() -> String {
    "flat".into()
}

pub fn weld_recipe(e: &Element) -> Option<WeldRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == WELD_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// An element's shape, to notice when it changes: its body's box, to 0.1 mm.
fn signature(mesh: Option<&crate::bodies::Mesh>) -> String {
    let Some((pts, _)) = mesh else { return "gone".into() };
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let r = |v: f64| (v * 10_000.0).round() as i64;
    format!("{}:{}:{}:{}:{}:{}:{}", pts.len(), r(lo[0]), r(lo[1]), r(lo[2]), r(hi[0]), r(hi[1]), r(hi[2]))
}

type EdgeKey = ([i64; 3], [i64; 3]);
type FeatureEdge = ([V3; 2], Vec<V3>);

/// A mesh's edges where its faces fold (or its border): pairs of points, with the normals of the
/// faces on either side.
fn feature_edges(mesh: &crate::bodies::Mesh) -> Vec<([V3; 2], Vec<V3>)> {
    let (pts, idx) = mesh;
    let key = |p: V3| [(p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64];
    let mut edges: BTreeMap<EdgeKey, FeatureEdge> = BTreeMap::new();
    for t in idx.chunks(3) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let n = cross(sub(b, a), sub(c, a));
        if len(n) < 1e-14 {
            continue;
        }
        let n = unit(n);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (kp, kq) = (key(p), key(q));
            let k = if kp <= kq { (kp, kq) } else { (kq, kp) };
            let entry = edges.entry(k).or_insert(([p, q], Vec::new()));
            if !entry.1.iter().any(|m| dot(*m, n) > 1.0 - 1e-6) {
                entry.1.push(n);
            }
        }
    }
    edges.into_values().filter(|(_, ns)| ns.len() != 1).collect()
}

const TOUCH: f64 = 0.001;

fn touches(mesh: &crate::bodies::Mesh, e: [V3; 2]) -> bool {
    (0..=4).all(|k| {
        let t = k as f64 / 4.0;
        mesh_distance(mesh, add(e[0], mul(sub(e[1], e[0]), t))) <= TOUCH
    })
}

/// The normals of a mesh's faces along an edge (those within 1 mm of its middle and square to it).
fn normals_at(mesh: &crate::bodies::Mesh, e: [V3; 2]) -> Vec<V3> {
    let (pts, idx) = mesh;
    let mid = mul(add(e[0], e[1]), 0.5);
    let along = unit(sub(e[1], e[0]));
    let mut out: Vec<V3> = Vec::new();
    for t in idx.chunks(3) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let n = cross(sub(b, a), sub(c, a));
        if len(n) < 1e-14 {
            continue;
        }
        let n = unit(n);
        if dot(n, along).abs() > 1e-3 || point_triangle(mid, a, b, c) > TOUCH {
            continue;
        }
        if !out.iter().any(|m| dot(*m, n) > 1.0 - 1e-6) {
            out.push(n);
        }
    }
    out
}

/// The edges a weld can be placed on (`weld.edges`): edges of any of the elements that touch every
/// other within 1 mm.
pub fn weld_edges(store: &Store, p: IdsParams) -> Result<Vec<[V3; 2]>> {
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| resolve(store, r)).collect::<Result<_>>()?;
    if ids.len() < 2 {
        return Err(Error::new("weld.elements", "a weld joins two or more elements"));
    }
    let ms = meshes(store, &ids);
    let mut out: Vec<[V3; 2]> = Vec::new();
    for id in &ids {
        let Some(m) = ms.get(id) else { continue };
        for (e, _) in feature_edges(m) {
            if len(sub(e[1], e[0])) < 1e-4 {
                continue;
            }
            let others = ids.iter().filter(|o| *o != id);
            if others.clone().all(|o| ms.get(o).is_some_and(|om| touches(om, e))) {
                let dup = out.iter().any(|f| {
                    (len(sub(f[0], e[0])) < 1e-4 && len(sub(f[1], e[1])) < 1e-4)
                        || (len(sub(f[0], e[1])) < 1e-4 && len(sub(f[1], e[0])) < 1e-4)
                });
                if !dup {
                    out.push(e);
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WeldCreateParams {
    pub element_ids: Vec<ElementRef>,
    /// The edge: two points on an edge where the elements touch (from `weld.edges`).
    pub edge: [V3; 2],
    /// From the weld table (fillet by default).
    #[serde(default)]
    pub weld_type: Option<String>,
    #[serde(default)]
    pub weld_type_2: Option<String>,
    /// The throat (metres; default 6 mm).
    #[serde(default)]
    pub a: Option<f64>,
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub preparation: Option<String>,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub intermittent: Option<Intermittent>,
}

fn check_weld_values(t: &WeldTables, ty: &str, ty2: Option<&str>, surface: &str, prep: Option<&str>, a: f64) -> Result<()> {
    let bad = |what: &str, v: &str, list: &[String]| {
        Error::new("weld.value", format!("no {what} `{v}` in the weld table ({})", list.join(", ")))
    };
    if !t.types.iter().any(|x| x == ty) {
        return Err(bad("weld type", ty, &t.types));
    }
    if let Some(t2) = ty2
        && !t.types.iter().any(|x| x == t2)
    {
        return Err(bad("weld type", t2, &t.types));
    }
    if !t.surfaces.iter().any(|x| x == surface) {
        return Err(bad("surface", surface, &t.surfaces));
    }
    if let Some(pr) = prep
        && !t.preparations.iter().any(|x| x == pr)
    {
        return Err(bad("preparation", pr, &t.preparations));
    }
    if a <= 0.0 {
        return Err(Error::new("weld.value", "a weld's thickness is more than 0 mm"));
    }
    Ok(())
}

/// Places a weld (`weld.create`).
pub(crate) fn weld_create(txn: &mut crate::txn::Transaction<'_>, p: WeldCreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    if ids.len() < 2 {
        return Err(Error::new("weld.elements", "a weld joins two or more elements"));
    }
    if len(sub(p.edge[1], p.edge[0])) < 1e-4 {
        return Err(Error::new("weld.edge", "the edge has no length"));
    }
    let ms = meshes(&store, &ids);
    for id in &ids {
        let ok = ms.get(id).is_some_and(|m| touches(m, p.edge));
        if !ok {
            return Err(Error::new(
                "weld.not_touching",
                format!("element {id} does not touch the edge within 1 mm: pick an edge from weld.edges"),
            ));
        }
    }
    // The legs: along each of the first two elements' faces at the edge, leaving out the faces
    // where they bear on each other.
    let n0 = normals_at(&ms[&ids[0]], p.edge);
    let n1 = normals_at(&ms[&ids[1]], p.edge);
    let free = |a: &[V3], b: &[V3]| -> V3 {
        a.iter()
            .copied()
            .filter(|n| !b.iter().any(|m| dot(*n, *m) < -1.0 + 1e-3))
            .max_by(|x, y| dot(*x, b.first().copied().unwrap_or([0.0; 3])).total_cmp(&dot(*y, b.first().copied().unwrap_or([0.0; 3]))))
            .or_else(|| a.first().copied())
            .unwrap_or([0.0, 0.0, 1.0])
    };
    let legs = [free(&n1, &n0), free(&n0, &n1)];
    let t = weld_tables();
    let weld_type = p.weld_type.clone().unwrap_or_else(|| "fillet".into());
    let surface = p.surface.clone().unwrap_or_else(flat);
    let a = p.a.unwrap_or(0.006);
    check_weld_values(&t, &weld_type, p.weld_type_2.as_deref(), &surface, p.preparation.as_deref(), a)?;
    let r = WeldRecipe {
        kind: WELD_RECIPE.into(),
        elements: ids.iter().map(|i| store.get(*i).expect("resolved").guid().to_string()).collect(),
        edge: p.edge,
        legs,
        weld_type,
        weld_type_2: p.weld_type_2.clone(),
        a,
        length: p.length,
        surface,
        preparation: p.preparation.clone(),
        location: p.location,
        intermittent: p.intermittent.clone(),
        signatures: ids.iter().map(|i| signature(ms.get(i))).collect(),
    };
    let ne = weld_psets(
        NewElement::new("IfcFastener")
            .param("@PredefinedType", "WELD")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
        &r,
    );
    txn.create(ne)
}

fn weld_length(r: &WeldRecipe) -> f64 {
    let full = len(sub(r.edge[1], r.edge[0]));
    r.length.map_or(full, |l| l.min(full))
}

fn weld_pset_values(r: &WeldRecipe) -> Vec<(&'static str, Value)> {
    let mut v = vec![
        ("Pset_FastenerWeld.Type1", json!({ "type": "IfcLabel", "value": r.weld_type })),
        ("Pset_FastenerWeld.Surface1", json!({ "type": "IfcLabel", "value": r.surface })),
        ("Pset_FastenerWeld.a", json!({ "type": "IfcPositiveLengthMeasure", "value": r.a })),
        ("Pset_FastenerWeld.l", json!({ "type": "IfcPositiveLengthMeasure", "value": weld_length(r) })),
        ("Pset_FastenerWeld.Intermittent", json!({ "type": "IfcBoolean", "value": r.intermittent.is_some() })),
        (
            "Pset_FastenerWeld.Process",
            json!({ "type": "IfcLabel", "value": match r.location { Location::Shop => "shop", Location::Site => "site" } }),
        ),
    ];
    if let Some(t2) = &r.weld_type_2 {
        v.push(("Pset_FastenerWeld.Type2", json!({ "type": "IfcLabel", "value": t2 })));
    }
    v
}

fn weld_psets(mut ne: NewElement, r: &WeldRecipe) -> NewElement {
    for (k, v) in weld_pset_values(r) {
        ne = ne.param(k, v);
    }
    ne
}

/// A weld's body: a triangular bead along its edge (in segments when intermittent).
pub fn weld_mesh(r: &WeldRecipe) -> crate::bodies::Mesh {
    let along = unit(sub(r.edge[1], r.edge[0]));
    let total = weld_length(r);
    let leg = r.a * std::f64::consts::SQRT_2;
    let segments: Vec<(f64, f64)> = match &r.intermittent {
        Some(i) if i.pitch > 0.0 && i.length > 0.0 => {
            let mut s = Vec::new();
            let mut t = 0.0;
            while t < total - 1e-9 {
                s.push((t, (t + i.length).min(total)));
                t += i.pitch;
            }
            s
        }
        _ => vec![(0.0, total)],
    };
    let parts = segments
        .into_iter()
        .map(|(t0, t1)| {
            let p0 = add(r.edge[0], mul(along, t0));
            let p1 = add(r.edge[0], mul(along, t1));
            crate::steel_fab::Part::Weld { path: vec![p0, p1], a: r.legs[0], b: r.legs[1], size: leg }.mesh()
        })
        .collect();
    crate::steel_fab::join(parts)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeldInfo {
    pub id: ElementId,
    pub elements: Vec<ElementId>,
    pub edge: [V3; 2],
    pub weld_type: String,
    pub weld_type_2: Option<String>,
    pub a: f64,
    pub length: f64,
    pub surface: String,
    pub preparation: Option<String>,
    pub location: Location,
    pub intermittent: Option<Intermittent>,
    /// A welded element changed since the weld was placed or accepted.
    pub needs_review: bool,
}

pub fn weld_info(store: &Store, p: IdsParams) -> Result<Vec<WeldInfo>> {
    let welds: Vec<(ElementId, WeldRecipe)> = p
        .element_ids
        .iter()
        .map(|r| {
            let id = resolve(store, r)?;
            let rec = store
                .get(id)
                .and_then(weld_recipe)
                .ok_or_else(|| Error::new("weld.not_a_weld", format!("element {id} is not a weld")))?;
            Ok((id, rec))
        })
        .collect::<Result<_>>()?;
    let all: Vec<ElementId> = welds.iter().flat_map(|(_, r)| element_ids(store, &r.elements)).collect();
    let ms = meshes(store, &all);
    Ok(welds
        .into_iter()
        .map(|(id, r)| {
            let ids = element_ids(store, &r.elements);
            let now: Vec<String> = ids.iter().map(|i| signature(ms.get(i))).collect();
            WeldInfo {
                id,
                elements: ids,
                edge: r.edge,
                weld_type: r.weld_type.clone(),
                weld_type_2: r.weld_type_2.clone(),
                a: r.a,
                length: weld_length(&r),
                surface: r.surface.clone(),
                preparation: r.preparation.clone(),
                location: r.location,
                intermittent: r.intermittent.clone(),
                needs_review: now != r.signatures,
            }
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WeldSetParams {
    pub weld_ids: Vec<ElementRef>,
    #[serde(default)]
    pub weld_type: Option<String>,
    #[serde(default)]
    pub weld_type_2: Option<String>,
    #[serde(default)]
    pub a: Option<f64>,
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub preparation: Option<String>,
    #[serde(default)]
    pub location: Option<Location>,
    #[serde(default)]
    pub intermittent: Option<Intermittent>,
    /// Accept the weld as it is after its elements changed (clears *needs review*).
    #[serde(default)]
    pub accept: bool,
}

/// Changes welds or accepts them after review (`weld.set`).
pub(crate) fn weld_set(txn: &mut crate::txn::Transaction<'_>, p: WeldSetParams) -> Result<()> {
    let t = weld_tables();
    for r in &p.weld_ids {
        let store = txn.store().clone();
        let id = resolve(&store, r)?;
        let mut rec = store
            .get(id)
            .and_then(weld_recipe)
            .ok_or_else(|| Error::new("weld.not_a_weld", format!("element {id} is not a weld")))?;
        if let Some(v) = &p.weld_type {
            rec.weld_type = v.clone();
        }
        if p.weld_type_2.is_some() {
            rec.weld_type_2 = p.weld_type_2.clone();
        }
        if let Some(a) = p.a {
            rec.a = a;
        }
        if p.length.is_some() {
            rec.length = p.length;
        }
        if let Some(s) = &p.surface {
            rec.surface = s.clone();
        }
        if p.preparation.is_some() {
            rec.preparation = p.preparation.clone();
        }
        if let Some(l) = p.location {
            rec.location = l;
        }
        if p.intermittent.is_some() {
            rec.intermittent = p.intermittent.clone();
        }
        check_weld_values(&t, &rec.weld_type, rec.weld_type_2.as_deref(), &rec.surface, rec.preparation.as_deref(), rec.a)?;
        if p.accept {
            let ids = element_ids(&store, &rec.elements);
            let ms = meshes(&store, &ids);
            rec.signatures = ids.iter().map(|i| signature(ms.get(i))).collect();
        }
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        for (k, v) in weld_pset_values(&rec) {
            txn.set_param(id, k, v)?;
        }
    }
    Ok(())
}

/// The welds needing review (`weld.review`).
pub fn weld_review(store: &Store) -> Vec<ElementId> {
    let ids: Vec<ElementRef> = store.iter().filter(|e| weld_recipe(e).is_some()).map(|e| ElementRef::Id(e.id())).collect();
    weld_info(store, IdsParams { element_ids: ids })
        .map(|w| w.into_iter().filter(|w| w.needs_review).map(|w| w.id).collect())
        .unwrap_or_default()
}

// ---- Shared ----

/// The work plane of a plate or a fastener pattern.
pub fn plane_ref(e: &Element) -> Option<WorkPlaneRef> {
    plate_recipe(e).map(|r| r.plane).or_else(|| pattern_recipe(e).map(|r| r.plane))
}

/// Moves, turns or mirrors a plate or a pattern in plan (and by `dz`): its plane becomes a plane
/// fixed in space.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("an element");
    if let Some(mut r) = plate_recipe(e) {
        let cur = WorkPlaneRef::Explicit { origin: plate_plane(&store, &r).origin, x_dir: plate_plane(&store, &r).x, normal: plate_plane(&store, &r).normal };
        r.plane = crate::work_planes::transform_frame(&store, &cur, point, dz)?;
        r.fallback = frame_of(&crate::work_planes::resolve(&store, &r.plane)?);
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
    } else if let Some(mut r) = pattern_recipe(e) {
        let p = pattern_plane(&store, &r);
        let cur = WorkPlaneRef::Explicit { origin: p.origin, x_dir: p.x, normal: p.normal };
        r.plane = crate::work_planes::transform_frame(&store, &cur, point, dz)?;
        r.fallback = frame_of(&crate::work_planes::resolve(&store, &r.plane)?);
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
        crate::steel_fab::relayout(txn, id)?;
    }
    Ok(())
}

/// Whether an element is a detailing element (plate, pattern, weld): drawn at Fine detail.
pub fn is_detail(e: &Element) -> bool {
    plate_recipe(e).is_some() || pattern_recipe(e).is_some() || weld_recipe(e).is_some()
}

/// Whether an element goes with `guid`: a pattern or weld on it.
pub fn uses(e: &Element, guid: &str) -> bool {
    pattern_recipe(e).is_some_and(|r| r.elements.iter().any(|g| g == guid))
        || weld_recipe(e).is_some_and(|r| r.elements.iter().any(|g| g == guid))
}

/// Before elements are deleted: plates on their faces stay where they are, unhosted.
pub(crate) fn release(
    txn: &mut crate::txn::Transaction<'_>,
    deleted: &std::collections::BTreeSet<ElementId>,
) -> Result<()> {
    let store = txn.store().clone();
    let gone: Vec<String> = deleted.iter().filter_map(|i| store.get(*i).map(|e| e.guid().to_string())).collect();
    for e in store.iter() {
        let Some(mut r) = plate_recipe(e) else { continue };
        if deleted.contains(&e.id()) {
            continue;
        }
        let host = crate::work_planes::host_of(&r.plane).map(str::to_owned);
        if host.is_some_and(|h| gone.contains(&h)) {
            let p = plate_plane(&store, &r);
            r.plane = WorkPlaneRef::Explicit { origin: p.origin, x_dir: p.x, normal: p.normal };
            r.fallback = frame_of(&p);
            r.unhosted = true;
            txn.set_param(e.id(), RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
        }
    }
    Ok(())
}

/// The faces of elements (`face.list`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FaceInfo {
    pub element: ElementId,
    pub face: String,
    pub origin: V3,
    pub x: V3,
    pub normal: V3,
}

pub fn face_list(store: &Store, p: IdsParams) -> Result<Vec<FaceInfo>> {
    let mut out = Vec::new();
    for r in &p.element_ids {
        let id = resolve(store, r)?;
        let e = store.get(id).expect("resolved");
        for f in faces_of(store, e) {
            let pl = face_plane(store, e, f)?;
            out.push(FaceInfo { element: id, face: f.into(), origin: pl.origin, x: pl.x, normal: pl.normal });
        }
    }
    Ok(out)
}
