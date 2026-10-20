// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ducts, pipes and their fittings (MEP-DUCT-001, MEP-DUCT-002, MEP-DUCT-003, MEP-PIPE-001).
//!
//! - A **segment type** (`IfcDuctSegmentType`, `IfcPipeSegmentType` with
//!   `recipe.mep_segment_type`) has a domain, a shape and **routing preferences**: per role
//!   (elbow, junction, cross, transition, cap) an ordered list of parts with a size range, tried
//!   from the top; and a preferred junction (tee or tap).
//! - A **segment** (`IfcDuctSegment`, `IfcPipeSegment`, RIGIDSEGMENT) runs straight from `start`
//!   to `end` with a size, and has a port at each end, plus one per tap on its side.
//! - A **fitting** (`IfcDuctFitting`, `IfcPipeFitting`) is one of Reviera's own parts: a mitred
//!   elbow, a tee, a tap, a cross, a transition or a cap. Its legs run from its centre; the end
//!   of each leg is a port.
//! - **Drawing** a run along points puts an elbow at each corner (a change of elevation is a
//!   riser between two elbows), segments between them, and a tee or tap where the run ends on
//!   an existing segment (chosen by the junction rows); where no row fits, nothing is inserted,
//!   the ends are left open and a warning names the role and size. Its ends connect to the
//!   ports they meet, and it joins the system of what it connects to. Pipes may slope.
//!
//! The parts' sizes are Reviera's own: an elbow's and a tee's legs are 0.6 of the size plus
//! 50 mm long; a transition is as long as the size change times 2.5 (at least 150 mm).
//!
//! Not built yet: fittings and segments from component types, radius elbows, the Angles
//! setting, offsets and justification other than the centre, size and elevation locks, moving
//! and stretching runs with their fittings, flex duct, the in-canvas fitting controls
//! ("+"/"−", flip, rotate).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Connector, Domain, FlowDirection, Port, Shape};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

pub const TYPE_RECIPE: &str = "recipe.mep_segment_type";
pub const SEGMENT: &str = "recipe.mep_segment";
pub const FITTING: &str = "recipe.mep_fitting";

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
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
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}

// ---- Sizes ----

/// A duct or pipe size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    pub shape: Shape,
    /// Round (metres).
    #[serde(default)]
    pub diameter: f64,
    /// Rectangular and oval: across (horizontal on level runs) and up (metres).
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

impl Size {
    /// The size routing preferences compare: the diameter, or the larger side.
    pub fn nominal(&self) -> f64 {
        match self.shape {
            Shape::Round => self.diameter,
            _ => self.width.max(self.height),
        }
    }

    fn check(&self, domain: Domain) -> Result<()> {
        let ok = match self.shape {
            Shape::Round => self.diameter > 0.0 && self.diameter.is_finite(),
            _ => self.width > 0.0 && self.height > 0.0 && self.width.is_finite() && self.height.is_finite(),
        };
        if !ok {
            return Err(bad("mep_run.size", "give a diameter, or a width and height, above zero"));
        }
        if domain == Domain::Pipe && self.shape != Shape::Round {
            return Err(bad("mep_run.shape", "pipes are round"));
        }
        Ok(())
    }

    pub fn text(&self) -> String {
        match self.shape {
            Shape::Round => format!("Ø{:.0}", self.diameter * 1000.0),
            _ => format!("{:.0}×{:.0}", self.width * 1000.0, self.height * 1000.0),
        }
    }

    fn connector(&self, domain: Domain) -> Connector {
        Connector {
            shape: self.shape,
            diameter: self.diameter,
            width: self.width,
            height: self.height,
            ..mep::new_connector(domain)
        }
    }
}

// ---- Types and routing preferences ----

/// The parts Reviera makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Elbow,
    Tee,
    Tap,
    Cross,
    Transition,
    Cap,
}

impl Part {
    pub fn label(self) -> &'static str {
        match self {
            Part::Elbow => "Elbow",
            Part::Tee => "Tee",
            Part::Tap => "Tap",
            Part::Cross => "Cross",
            Part::Transition => "Transition",
            Part::Cap => "Cap",
        }
    }

    /// `IfcDuctFittingTypeEnum` / `IfcPipeFittingTypeEnum`.
    pub fn ifc(self) -> &'static str {
        match self {
            Part::Elbow => "BEND",
            Part::Tee | Part::Tap | Part::Cross => "JUNCTION",
            Part::Transition => "TRANSITION",
            Part::Cap => "OBSTRUCTION",
        }
    }
}

/// One row of a role: a part for sizes from `min` to `max` (metres; `max` 0 for no limit).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    pub part: Part,
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
}

impl Row {
    fn fits(&self, size: f64) -> bool {
        size + 1e-9 >= self.min && (self.max <= 0.0 || size <= self.max + 1e-9)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Junction {
    #[default]
    Tee,
    Tap,
}

/// Routing preferences (MEP-DUCT-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Routing {
    pub elbow: Vec<Row>,
    pub junction: Vec<Row>,
    pub preferred_junction: Junction,
    pub cross: Vec<Row>,
    pub transition: Vec<Row>,
    pub cap: Vec<Row>,
}

impl Default for Routing {
    fn default() -> Self {
        let any = |part| vec![Row { part, min: 0.0, max: 0.0 }];
        Self {
            elbow: any(Part::Elbow),
            junction: any(Part::Tee),
            preferred_junction: Junction::Tee,
            cross: any(Part::Cross),
            transition: any(Part::Transition),
            cap: any(Part::Cap),
        }
    }
}

impl Routing {
    fn rows(&self, role: Role) -> &[Row] {
        match role {
            Role::Elbow => &self.elbow,
            Role::Junction => &self.junction,
            Role::Cross => &self.cross,
            Role::Transition => &self.transition,
            Role::Cap => &self.cap,
        }
    }

    /// The part for a role and size: the first row that fits, the preferred junction kind first
    /// among junction rows that fit.
    pub fn choose(&self, role: Role, size: f64) -> Option<Part> {
        let fitting: Vec<&Row> = self.rows(role).iter().filter(|r| r.fits(size)).collect();
        if role == Role::Junction {
            let want = match self.preferred_junction {
                Junction::Tee => Part::Tee,
                Junction::Tap => Part::Tap,
            };
            if let Some(r) = fitting.iter().find(|r| r.part == want) {
                return Some(r.part);
            }
        }
        fitting.first().map(|r| r.part)
    }

    fn check(&self) -> Result<()> {
        for (role, rows, allowed) in [
            ("elbow", &self.elbow, &[Part::Elbow][..]),
            ("junction", &self.junction, &[Part::Tee, Part::Tap][..]),
            ("cross", &self.cross, &[Part::Cross][..]),
            ("transition", &self.transition, &[Part::Transition][..]),
            ("cap", &self.cap, &[Part::Cap][..]),
        ] {
            for r in rows {
                if !allowed.contains(&r.part) {
                    return Err(bad("routing.part", format!("a {} cannot be a {role} row", r.part.label().to_lowercase())));
                }
                if r.min < 0.0 || (r.max > 0.0 && r.max < r.min) {
                    return Err(bad("routing.range", format!("a {role} row's size range runs from {} to {}", r.min, r.max)));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Elbow,
    Junction,
    Cross,
    Transition,
    Cap,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Elbow => "elbow",
            Role::Junction => "junction",
            Role::Cross => "cross",
            Role::Transition => "transition",
            Role::Cap => "cap",
        }
    }
}

/// The stored recipe of a segment type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentType {
    pub kind: String,
    pub domain: Domain,
    pub shape: Shape,
    #[serde(default)]
    pub routing: Routing,
    #[serde(default)]
    pub material: String,
    /// Absolute roughness (metres).
    #[serde(default)]
    pub roughness: f64,
}

pub fn segment_type(e: &Element) -> Option<SegmentType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn type_class(domain: Domain) -> &'static str {
    if domain == Domain::Pipe { "IfcPipeSegmentType" } else { "IfcDuctSegmentType" }
}

fn segment_class(domain: Domain) -> &'static str {
    if domain == Domain::Pipe { "IfcPipeSegment" } else { "IfcDuctSegment" }
}

fn fitting_class(domain: Domain) -> &'static str {
    if domain == Domain::Pipe { "IfcPipeFitting" } else { "IfcDuctFitting" }
}

/// The built-in type names, by domain and shape.
fn default_name(domain: Domain, shape: Shape) -> &'static str {
    match (domain, shape) {
        (Domain::Pipe, _) => "Standard Pipe",
        (_, Shape::Round) => "Round Duct",
        (_, Shape::Rectangular) => "Rectangular Duct",
        (_, Shape::Oval) => "Oval Duct",
    }
}

/// The type of a domain and shape: an existing one of the built-in name, or a new one.
fn default_type(txn: &mut Transaction<'_>, domain: Domain, shape: Shape) -> Result<ElementId> {
    let name = default_name(domain, shape);
    let store = txn.store().clone();
    if let Some(e) = store
        .iter()
        .find(|e| e.name.as_deref() == Some(name) && segment_type(e).is_some_and(|t| t.domain == domain && t.shape == shape))
    {
        return Ok(e.id());
    }
    let t = SegmentType {
        kind: TYPE_RECIPE.into(),
        domain,
        shape,
        routing: Routing::default(),
        material: if domain == Domain::Pipe { "Copper".into() } else { "Galvanised Steel".into() },
        roughness: if domain == Domain::Pipe { 1.5e-6 } else { 9.0e-5 },
    };
    txn.create(
        NewElement::new(type_class(domain))
            .name(name)
            .param("@PredefinedType", "RIGIDSEGMENT")
            .param(RECIPE_KEY, serde_json::to_value(&t).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub domain: Domain,
    pub name: String,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub routing: Option<Routing>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub roughness: Option<f64>,
}

/// Makes a duct or pipe type (`mep_segment_type.create`).
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    if !matches!(p.domain, Domain::Duct | Domain::Pipe) {
        return Err(bad("mep_segment_type.domain", "segment types are duct or pipe types (tray and conduit come later)"));
    }
    if p.domain == Domain::Pipe && p.shape != Shape::Round {
        return Err(bad("mep_run.shape", "pipes are round"));
    }
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("mep_segment_type.name", "a type needs a name"));
    }
    if txn.store().iter().any(|e| segment_type(e).is_some() && e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&name))) {
        return Err(bad("mep_segment_type.duplicate", format!("there is already a type named {name:?}")));
    }
    let routing = p.routing.unwrap_or_default();
    routing.check()?;
    let t = SegmentType {
        kind: TYPE_RECIPE.into(),
        domain: p.domain,
        shape: p.shape,
        routing,
        material: p.material.unwrap_or_default(),
        roughness: p.roughness.unwrap_or(0.0),
    };
    txn.create(
        NewElement::new(type_class(p.domain))
            .name(name)
            .param("@PredefinedType", "RIGIDSEGMENT")
            .param(RECIPE_KEY, serde_json::to_value(&t).expect("serialises")),
    )
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTypesParams {
    pub domain: Option<Domain>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    #[serde(flatten)]
    pub segment_type: SegmentType,
}

/// `mep_segment_type.list`.
pub(crate) fn list_types(store: &Store, p: ListTypesParams) -> Result<Vec<TypeInfo>> {
    Ok(store
        .iter()
        .filter_map(|e| {
            let t = segment_type(e)?;
            (p.domain.is_none_or(|d| d == t.domain)).then(|| TypeInfo { id: e.id(), name: e.name.clone().unwrap_or_default(), segment_type: t })
        })
        .collect())
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<(ElementId, SegmentType)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such duct or pipe type"))?;
    let t = segment_type(store.get(id).expect("resolved"))
        .ok_or_else(|| bad("mep_segment_type.not_a_type", format!("element {id} is not a duct or pipe type")))?;
    Ok((id, t))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutingGetParams {
    pub type_id: ElementRef,
}

/// `routing_preferences.get`.
pub(crate) fn routing_get(store: &Store, p: RoutingGetParams) -> Result<Routing> {
    Ok(resolve_type(store, &p.type_id)?.1.routing)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutingSetParams {
    pub type_id: ElementRef,
    pub routing: Routing,
}

/// `routing_preferences.set`: used for runs drawn from now on (`mep_run.reapply_type` refits
/// existing ones).
pub(crate) fn routing_set(txn: &mut Transaction<'_>, p: RoutingSetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_type(&store, &p.type_id)?;
    p.routing.check()?;
    t.routing = p.routing;
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

// ---- Segments and fittings ----

/// A tap on a segment's side: its point on the axis and outward direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapPort {
    pub at: V3,
    pub direction: V3,
    pub size: Size,
}

/// The stored recipe of a segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentRecipe {
    pub kind: String,
    pub domain: Domain,
    /// Its type (GlobalId).
    pub segment_type: String,
    pub start: V3,
    pub end: V3,
    pub size: Size,
    /// The level it is drawn from (GlobalId).
    pub level: String,
    /// Its system classification, when drawn with a system type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taps: Vec<TapPort>,
    /// Pipes: the slope it was drawn with (rise over run).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slope: f64,
    /// The section's across direction, where it is not the level one square to the run
    /// (vertical runs keep their neighbours').
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<V3>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A fitting's leg: from the centre along `direction` for `length`, of `size`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Leg {
    pub direction: V3,
    pub length: f64,
    pub size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<V3>,
}

/// The stored recipe of a fitting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FittingRecipe {
    pub kind: String,
    pub domain: Domain,
    pub part: Part,
    pub centre: V3,
    pub legs: Vec<Leg>,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
}

pub fn segment_recipe(e: &Element) -> Option<SegmentRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SEGMENT).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn fitting_recipe(e: &Element) -> Option<FittingRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == FITTING).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Whether an element is a segment or a fitting.
pub fn is_run_element(e: &Element) -> bool {
    matches!(
        e.param(RECIPE_KEY).and_then(|v| v.get("kind")).and_then(Value::as_str),
        Some(SEGMENT | FITTING)
    )
}

/// How many ports a segment or fitting has (0 for other elements).
pub fn port_count(e: &Element) -> usize {
    if let Some(s) = segment_recipe(e) {
        return 2 + s.taps.len();
    }
    fitting_recipe(e).map_or(0, |f| f.legs.len())
}

fn port_connector(domain: Domain, size: &Size, c: Option<Classification>, name: &str) -> Connector {
    Connector {
        name: name.into(),
        classification: Some(c.unwrap_or(Classification::Global)),
        flow_direction: FlowDirection::Bidirectional,
        ..size.connector(domain)
    }
}

/// A segment's or fitting's ports.
pub fn ports(_: &Store, e: &Element) -> Vec<Port> {
    if let Some(s) = segment_recipe(e) {
        let Some(axis) = unit(sub(s.end, s.start)) else { return Vec::new() };
        let mut out = vec![
            Port { element: e.id(), index: 0, connector: port_connector(s.domain, &s.size, s.classification, "Start"), position: s.start, direction: mul(axis, -1.0) },
            Port { element: e.id(), index: 1, connector: port_connector(s.domain, &s.size, s.classification, "End"), position: s.end, direction: axis },
        ];
        for (k, t) in s.taps.iter().enumerate() {
            out.push(Port {
                element: e.id(),
                index: 2 + k,
                connector: port_connector(s.domain, &t.size, s.classification, "Tap"),
                position: t.at,
                direction: t.direction,
            });
        }
        return out;
    }
    let Some(f) = fitting_recipe(e) else { return Vec::new() };
    f.legs
        .iter()
        .enumerate()
        .map(|(k, l)| Port {
            element: e.id(),
            index: k,
            connector: port_connector(f.domain, &l.size, f.classification, &format!("Leg {}", k + 1)),
            position: add(f.centre, mul(l.direction, l.length)),
            direction: l.direction,
        })
        .collect()
}

// ---- Geometry ----

/// A solid as a polyhedron: points and outward polygons.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub points: Vec<V3>,
    pub faces: Vec<Vec<usize>>,
}

impl Solid {
    /// Triangles, fanned from each face's first point.
    pub fn mesh(&self) -> (Vec<[f64; 3]>, Vec<u32>) {
        let mut idx = Vec::new();
        for f in &self.faces {
            for k in 1..f.len().saturating_sub(1) {
                idx.extend([f[0] as u32, f[k] as u32, f[k + 1] as u32]);
            }
        }
        (self.points.clone(), idx)
    }
}

/// The across (`w`) and up (`h`) directions of a section square to `u`: across is level for
/// level and sloped runs, X for vertical ones.
fn frame(u: V3) -> (V3, V3) {
    let w = if u[2].abs() < 0.999 { unit(cross([0.0, 0.0, 1.0], u)).expect("not vertical") } else { [1.0, 0.0, 0.0] };
    (w, cross(u, w))
}

/// The section's across and up directions for a run along `u`, `across` given or the default.
fn axes(u: V3, across: Option<V3>) -> (V3, V3) {
    match across.and_then(|a| unit(sub(a, mul(u, dot(a, u))))) {
        Some(w) => (w, cross(u, w)),
        None => frame(u),
    }
}

/// The section outline of a size at `c`, square to `u`.
fn section(c: V3, u: V3, s: &Size, across: Option<V3>) -> Vec<V3> {
    let (w, h) = axes(u, across);
    match s.shape {
        Shape::Round => (0..16)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 16.0;
                add(c, add(mul(w, t.cos() * s.diameter / 2.0), mul(h, t.sin() * s.diameter / 2.0)))
            })
            .collect(),
        Shape::Rectangular | Shape::Oval => {
            let (a, b) = (s.width / 2.0, s.height / 2.0);
            [(-a, -b), (a, -b), (a, b), (-a, b)].iter().map(|(x, y)| add(c, add(mul(w, *x), mul(h, *y)))).collect()
        }
    }
}

/// A prism (or a frustum, with two sizes of one shape) from `a` to `b`.
fn loft(a: V3, b: V3, sa: &Size, sb: &Size, across: Option<V3>) -> Option<Solid> {
    let u = unit(sub(b, a))?;
    let p = section(a, u, sa, across);
    let mut q = section(b, u, sb, across);
    if q.len() != p.len() {
        q = section(b, u, &Size { shape: sa.shape, ..*sb }, across);
    }
    let n = p.len();
    let mut points = p;
    points.extend(q);
    let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![k, j, n + j, n + k]);
    }
    Some(Solid { points, faces })
}

/// A segment's body.
pub fn segment_solid(s: &SegmentRecipe) -> Vec<Solid> {
    loft(s.start, s.end, &s.size, &s.size, s.across).into_iter().collect()
}

/// A fitting's body: each leg from just behind the centre to its end; a transition as one loft;
/// a cap a short plug.
pub fn fitting_solids(f: &FittingRecipe) -> Vec<Solid> {
    match f.part {
        Part::Transition if f.legs.len() == 2 => {
            let a = add(f.centre, mul(f.legs[0].direction, f.legs[0].length));
            let b = add(f.centre, mul(f.legs[1].direction, f.legs[1].length));
            loft(a, b, &f.legs[0].size, &f.legs[1].size, f.legs[1].across).into_iter().collect()
        }
        Part::Cap if !f.legs.is_empty() => {
            let l = f.legs[0];
            let grow = |s: Size| Size { diameter: s.diameter * 1.08, width: s.width * 1.08, height: s.height * 1.08, ..s };
            let a = add(f.centre, mul(l.direction, -0.02));
            let b = add(f.centre, mul(l.direction, l.length));
            loft(a, b, &grow(l.size), &grow(l.size), l.across).into_iter().collect()
        }
        _ => f
            .legs
            .iter()
            .filter_map(|l| {
                let back = l.size.nominal() / 2.0;
                let a = add(f.centre, mul(l.direction, -back.min(l.length * 0.9)));
                // A tap's leg starts at the host's surface, not its axis.
                let a = if f.part == Part::Tap { f.centre } else { a };
                loft(a, add(f.centre, mul(l.direction, l.length)), &l.size, &l.size, l.across)
            })
            .collect(),
    }
}

/// Every segment and fitting with its solids.
pub fn all(store: &Store) -> Vec<(ElementId, Domain, bool, Vec<Solid>)> {
    store
        .iter()
        .filter_map(|e| {
            if let Some(s) = segment_recipe(e) {
                return Some((e.id(), s.domain, true, segment_solid(&s)));
            }
            let f = fitting_recipe(e)?;
            Some((e.id(), f.domain, false, fitting_solids(&f)))
        })
        .collect()
}

// ---- Drawing runs ----

fn leg_length(size: &Size) -> f64 {
    size.nominal() * 0.6 + 0.05
}

/// An elbow's leg length: by the section's extent in the plane of the bend (a rectangular
/// duct turning up uses its height).
fn elbow_leg(size: &Size, incoming: V3, outgoing: V3, across: Option<V3>) -> f64 {
    let extent = match size.shape {
        Shape::Round => size.diameter,
        _ => {
            let (w, h) = axes(incoming, across);
            match unit(sub(outgoing, mul(incoming, dot(outgoing, incoming)))) {
                Some(d) => dot(d, w).abs() * size.width + dot(d, h).abs() * size.height,
                None => size.nominal(),
            }
        }
    };
    extent * 0.6 + 0.05
}

fn level_of(store: &Store, r: &ElementRef) -> Result<(String, f64)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != "IfcBuildingStorey" {
        return Err(bad("level.not_a_level", format!("element {id} is not a level")));
    }
    let g = e.guid().to_string();
    let z = crate::levels::elevation_of(store, &g)?;
    Ok((g, z))
}

/// What a run is drawn with.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The duct or pipe type; the built-in type of the shape when omitted.
    pub type_id: Option<ElementRef>,
    /// The run's points in plan with their height above the level (`[x, y, z]`, metres), or in
    /// plan (`[x, y]`) at `middle_elevation`.
    pub points: Vec<Vec<f64>>,
    pub level: ElementRef,
    /// Height of the centreline above the level for points given in plan (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub middle_elevation: Option<f64>,
    #[serde(default)]
    pub shape: Option<Shape>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// The system type the run carries (`supply_air`, `domestic_cold_water`, …).
    pub system_type_id: Option<String>,
    /// Pipes: the fall along level legs (rise over run; 0.01 is 1 % falling from the first
    /// point).
    #[serde(default)]
    pub slope: f64,
}

/// What a run made.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RunReport {
    pub segments: Vec<ElementId>,
    pub fittings: Vec<ElementId>,
    /// Connections made to elements that were there before.
    pub connected_to: Vec<ElementId>,
    /// The system it joined.
    pub system: Option<ElementId>,
}

struct Ctx {
    domain: Domain,
    type_guid: String,
    routing: Routing,
    level: String,
    classification: Option<Classification>,
}

#[allow(clippy::too_many_arguments)]
fn make_segment(txn: &mut Transaction<'_>, cx: &Ctx, a: V3, b: V3, size: Size, slope: f64, across: Option<V3>) -> Result<ElementId> {
    let r = SegmentRecipe {
        kind: SEGMENT.into(),
        domain: cx.domain,
        segment_type: cx.type_guid.clone(),
        start: a,
        end: b,
        size,
        level: cx.level.clone(),
        classification: cx.classification,
        taps: Vec::new(),
        slope,
        across,
    };
    let mut e = NewElement::new(segment_class(cx.domain))
        .param("@PredefinedType", "RIGIDSEGMENT")
        .param("@type", cx.type_guid.clone())
        .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"));
    if slope != 0.0 {
        e = e.param(
            "Pset_PipeSegmentOccurrence.Gradient",
            serde_json::json!({ "type": "IfcPositiveRatioMeasure", "value": slope.abs() }),
        );
    }
    txn.create(e)
}

fn make_fitting(txn: &mut Transaction<'_>, cx: &Ctx, part: Part, centre: V3, legs: Vec<Leg>) -> Result<ElementId> {
    let r = FittingRecipe {
        kind: FITTING.into(),
        domain: cx.domain,
        part,
        centre,
        legs,
        level: cx.level.clone(),
        classification: cx.classification,
    };
    txn.create(
        NewElement::new(fitting_class(cx.domain))
            .name(part.label())
            .param("@PredefinedType", part.ifc())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

/// An existing segment the point lies on, away from its ends: the segment, its recipe and the
/// point on its axis.
fn host_at(store: &Store, domain: Domain, p: V3) -> Option<(ElementId, SegmentRecipe, V3)> {
    store.iter().find_map(|e| {
        let s = segment_recipe(e)?;
        if s.domain != domain {
            return None;
        }
        let d = sub(s.end, s.start);
        let l = len(d);
        let u = unit(d)?;
        let t = dot(sub(p, s.start), u);
        let q = add(s.start, mul(u, t));
        let off = len(sub(p, q));
        let margin = leg_length(&s.size);
        (t > margin && t < l - margin && off <= s.size.nominal() / 2.0 + 1e-6).then(|| (e.id(), s, q))
    })
}

fn warn(txn: &mut Transaction<'_>, id: &str, msg: String, ids: Vec<ElementId>) {
    txn.warn(Warning { id: id.into(), message: msg, element_ids: ids });
}

fn draw(txn: &mut Transaction<'_>, domain: Domain, p: CreateParams) -> Result<RunReport> {
    let store = txn.store().clone();
    let (level, z0) = level_of(&store, &p.level)?;
    let given_type = p.type_id.as_ref().map(|t| resolve_type(&store, t)).transpose()?;
    if let Some((_, t)) = &given_type
        && t.domain != domain
    {
        return Err(bad("mep_segment_type.domain", format!("that is a {} type", t.domain.label().to_lowercase())));
    }
    let shape = p.shape.or(given_type.as_ref().map(|t| t.1.shape)).unwrap_or(if domain == Domain::Pipe { Shape::Round } else { Shape::Rectangular });
    let size = Size {
        shape,
        diameter: p.diameter.unwrap_or(0.0),
        width: p.width.unwrap_or(0.0),
        height: p.height.unwrap_or(0.0),
    };
    size.check(domain)?;
    let type_id = match given_type {
        Some((id, _)) => id,
        None => default_type(txn, domain, shape)?,
    };
    let store = txn.store().clone();
    let t = segment_type(store.get(type_id).expect("made")).expect("a type");
    let classification = p
        .system_type_id
        .as_deref()
        .map(|id| {
            crate::mep_systems::system_type(&store, id)
                .ok_or_else(|| bad("system_type.not_found", format!("no system type {id:?}")))
                .and_then(|st| {
                    if st.classification.fits(domain) {
                        Ok(st.classification)
                    } else {
                        Err(bad("mep_run.classification", format!("a {} cannot carry {}", domain.label().to_lowercase(), st.classification.label())))
                    }
                })
        })
        .transpose()?;
    let cx = Ctx {
        domain,
        type_guid: store.get(type_id).expect("made").guid().to_string(),
        routing: t.routing.clone(),
        level,
        classification,
    };
    // The points in project coordinates, collinear ones dropped.
    let mid = p.middle_elevation.unwrap_or(if domain == Domain::Pipe { 2.4 } else { 2.7 });
    let mut pts: Vec<V3> = Vec::new();
    for q in &p.points {
        let v = match q.as_slice() {
            [x, y] => [*x, *y, z0 + mid],
            [x, y, z] => [*x, *y, z0 + z],
            _ => return Err(bad("mep_run.point", "each point is [x, y] or [x, y, z]")),
        };
        if pts.last().is_some_and(|l| len(sub(v, *l)) < 1e-6) {
            continue;
        }
        pts.push(v);
    }
    if pts.len() < 2 {
        return Err(bad("mep_run.points", "a run needs two different points"));
    }
    // Pipes fall along level legs.
    if p.slope != 0.0 {
        if domain != Domain::Pipe {
            return Err(bad("mep_run.slope", "only pipes slope"));
        }
        let mut drop = 0.0;
        let original = pts.clone();
        for k in 1..pts.len() {
            let d = sub(original[k], original[k - 1]);
            if d[2].abs() < 1e-9 {
                drop += p.slope * (d[0] * d[0] + d[1] * d[1]).sqrt();
            }
            pts[k][2] = original[k][2] - drop;
        }
    }
    let mut i = 1;
    while i + 1 < pts.len() {
        let (a, b) = (unit(sub(pts[i], pts[i - 1])).expect("distinct"), unit(sub(pts[i + 1], pts[i])).expect("distinct"));
        if dot(a, b) > (0.5f64).to_radians().cos() {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    let mut report = RunReport::default();
    // A run ending on an existing segment's side: a junction there.
    let ends_on = host_at(&store, domain, *pts.last().expect("two"));
    let starts_on = host_at(&store, domain, pts[0]);
    // Where each segment starts and ends, after the fittings take their legs.
    let n = pts.len();
    let mut starts = pts[..n - 1].to_vec();
    let mut ends = pts[1..].to_vec();
    let leg = leg_length(&size);
    // Each leg's across direction: its own level one, or for a vertical leg its nearest level
    // neighbour's, so a rectangular section does not turn going up or down.
    let dirs: Vec<V3> = (0..n - 1).map(|k| unit(sub(pts[k + 1], pts[k])).expect("distinct")).collect();
    let vertical = |d: V3| d[2].abs() >= 0.999;
    let across: Vec<Option<V3>> = (0..n - 1)
        .map(|k| {
            if !vertical(dirs[k]) {
                return None;
            }
            let near = (0..n - 1)
                .filter(|j| !vertical(dirs[*j]))
                .min_by_key(|j| (*j as i64 - k as i64).abs())?;
            Some(frame(dirs[near]).0)
        })
        .collect();
    let across_of = |k: usize| across[k].unwrap_or_else(|| frame(dirs[k]).0);
    for k in 1..n - 1 {
        let (a, b) = (mul(dirs[k - 1], -1.0), dirs[k]);
        match cx.routing.choose(Role::Elbow, size.nominal()) {
            Some(part) => {
                let leg = elbow_leg(&size, dirs[k - 1], dirs[k], Some(across_of(k - 1)));
                let id = make_fitting(
                    txn,
                    &cx,
                    part,
                    pts[k],
                    vec![
                        Leg { direction: a, length: leg, size, across: Some(across_of(k - 1)) },
                        Leg { direction: b, length: leg, size, across: Some(across_of(k)) },
                    ],
                )?;
                report.fittings.push(id);
                ends[k - 1] = add(pts[k], mul(a, leg));
                starts[k] = add(pts[k], mul(b, leg));
            }
            None => warn(
                txn,
                "mep.routing.no_part",
                format!("no elbow in the routing preferences fits {}; the corner is left open", size.text()),
                Vec::new(),
            ),
        }
    }
    for (end_at, at_end) in [(ends_on, true), (starts_on, false)] {
        let Some((host, hs, q)) = end_at else { continue };
        let k = if at_end { n - 1 } else { 0 };
        let other = if at_end { pts[n - 2] } else { pts[1] };
        let branch = unit(sub(other, q)).expect("distinct");
        let axis = unit(sub(hs.end, hs.start)).expect("a segment");
        if dot(branch, axis).abs() > (1.0f64).to_radians().sin() {
            return Err(bad("mep_run.branch", "a branch must meet the duct or pipe square to it"));
        }
        let Some(part) = cx.routing.choose(Role::Junction, size.nominal()) else {
            warn(
                txn,
                "mep.routing.no_part",
                format!("no junction in the routing preferences fits {}; the branch is left open", size.text()),
                vec![host],
            );
            continue;
        };
        let _ = k;
        let surface = hs.size.nominal() / 2.0;
        match part {
            Part::Tap => {
                // The host stays whole and gets a tap port; the tap runs out from its side.
                let base = add(q, mul(branch, surface));
                let tap_len = leg.max(0.05);
                let id = make_fitting(
                    txn,
                    &cx,
                    Part::Tap,
                    base,
                    vec![Leg { direction: mul(branch, -1.0), length: 0.0, size, across: None }, Leg { direction: branch, length: tap_len, size, across: None }],
                )?;
                report.fittings.push(id);
                let mut h = hs.clone();
                h.taps.push(TapPort { at: base, direction: branch, size });
                let v = serde_json::to_value(&h).expect("serialises");
                txn.modify(host, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                })?;
                let tip = add(base, mul(branch, tap_len));
                if at_end { ends[n - 2] = tip } else { starts[0] = tip }
            }
            _ => {
                // The host is cut in two either side of a tee.
                let hl = leg_length(&hs.size);
                let tee = make_fitting(
                    txn,
                    &cx,
                    Part::Tee,
                    q,
                    vec![
                        Leg { direction: mul(axis, -1.0), length: hl, size: hs.size, across: hs.across },
                        Leg { direction: axis, length: hl, size: hs.size, across: hs.across },
                        Leg { direction: branch, length: surface + leg, size, across: None },
                    ],
                )?;
                report.fittings.push(tee);
                let mut first = hs.clone();
                first.end = add(q, mul(axis, -hl));
                let mut second = hs.clone();
                second.start = add(q, mul(axis, hl));
                // Taps stay with the half they are on.
                let along = |p: V3| dot(sub(p, hs.start), axis);
                let cut = along(q);
                first.taps.retain(|t| along(t.at) < cut);
                second.taps.retain(|t| along(t.at) > cut);
                let (v1, v2) = (serde_json::to_value(&first).expect("serialises"), serde_json::to_value(&second).expect("serialises"));
                txn.modify(host, |e| {
                    e.params.insert(RECIPE_KEY.into(), v1);
                })?;
                let mut el = store.get(host).expect("host").params.clone();
                el.insert(RECIPE_KEY.into(), v2);
                let host_class = store.get(host).expect("host").class.clone();
                let new_half = txn.create(NewElement { class: host_class, name: None, params: el, guid: None })?;
                report.segments.push(new_half);
                // The host's end connection moves to the new half.
                retarget_end(txn, host, new_half)?;
                let tip = add(q, mul(branch, surface + leg));
                if at_end { ends[n - 2] = tip } else { starts[0] = tip }
            }
        }
    }
    for k in 0..n - 1 {
        if len(sub(ends[k], starts[k])) < 0.01 || dot(sub(ends[k], starts[k]), sub(pts[k + 1], pts[k])) <= 0.0 {
            return Err(bad("mep_run.too_short", "a leg of the run is too short for its fittings"));
        }
        let slope = if domain == Domain::Pipe { p.slope } else { 0.0 };
        report.segments.push(make_segment(txn, &cx, starts[k], ends[k], size, slope, across[k])?);
    }
    // Connect everything made, then to what it meets.
    let made: Vec<ElementId> = report.segments.iter().chain(&report.fittings).copied().collect();
    let before: Vec<ElementId> = store.iter().map(|e| e.id()).collect();
    for c in mep::connect_coincident(txn, &made)? {
        for e in [c.a.0, c.b.0] {
            if before.contains(&e) && !made.contains(&e) && !report.connected_to.contains(&e) {
                report.connected_to.push(e);
            }
        }
    }
    report.system = join_system(txn, &made, classification)?;
    Ok(report)
}

/// Moves the connection on a split segment's end port (index 1) to the new second half.
fn retarget_end(txn: &mut Transaction<'_>, from: ElementId, to: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let (a, b) = (mep::stored(&store, from, 1), mep::stored(&store, to, 1));
    let links: Vec<mep::Link> = mep::stored_links(&store)
        .into_iter()
        .map(|mut l| {
            if l.a == a {
                l.a = b.clone();
            }
            if l.b == a {
                l.b = b.clone();
            }
            l
        })
        .collect();
    // Written straight: the new half exists, so the links resolve.
    let pid = store.iter().find(|e| e.class == "IfcProject").map(|e| e.id()).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, mep::CONNECTIONS_KEY, serde_json::to_value(links).expect("serialises"))
}

/// Puts the run in the system of what it is connected to, when one carries its classification
/// (or the run has none).
fn join_system(txn: &mut Transaction<'_>, made: &[ElementId], c: Option<Classification>) -> Result<Option<ElementId>> {
    let store = txn.store().clone();
    let Some(first) = made.first() else { return Ok(None) };
    let net: Vec<ElementId> = mep::network(&store, *first);
    let system = crate::mep_systems::systems(&store).into_iter().find(|s| {
        c.is_none_or(|c| c == s.classification) && s.elements.iter().any(|e| net.contains(e) && !made.contains(e))
    });
    let Some(s) = system else { return Ok(None) };
    let e = store.get(s.id).expect("listed");
    let mut r = crate::mep_systems::recipe(e).expect("a system");
    for id in made {
        let guid = store.get(*id).expect("made").guid().to_string();
        for p in mep::ports(&store, store.get(*id).expect("made")) {
            let sp = mep::StoredPort { element: guid.clone(), port: p.index };
            if !r.members.contains(&sp) {
                r.members.push(sp);
            }
        }
    }
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(s.id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    Ok(Some(s.id))
}

/// Draws a duct run (`duct.create`).
pub(crate) fn create_duct(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::Duct, p)
}

/// Draws a pipe run (`pipe.create`).
pub(crate) fn create_pipe(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::Pipe, p)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSizeParams {
    pub element_ids: Vec<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
}

/// Resizes segments and the legs of fittings joined to them (`duct.set_size`, `pipe.set_size`).
pub(crate) fn set_size(txn: &mut Transaction<'_>, p: SetSizeParams) -> Result<()> {
    let store = txn.store().clone();
    let partners = mep::partners(&store);
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such segment"))?;
        let mut s = store
            .get(id)
            .and_then(segment_recipe)
            .ok_or_else(|| bad("mep_run.not_a_segment", format!("element {id} is not a duct or pipe segment")))?;
        if let Some(d) = p.diameter {
            s.size.diameter = d;
        }
        if let Some(w) = p.width {
            s.size.width = w;
        }
        if let Some(h) = p.height {
            s.size.height = h;
        }
        s.size.check(s.domain)?;
        let v = serde_json::to_value(&s).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        for end in [0usize, 1] {
            for (fid, leg) in partners.get(&(id, end)).into_iter().flatten() {
                let Some(mut f) = txn.store().get(*fid).and_then(fitting_recipe) else { continue };
                if let Some(l) = f.legs.get_mut(*leg) {
                    l.size = s.size;
                }
                let v = serde_json::to_value(&f).expect("serialises");
                txn.modify(*fid, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                })?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapParams {
    /// Segments and fittings; every one when empty.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// Caps the open ends of segments and fittings (`duct.cap_open_ends`, `pipe.cap_open_ends`);
/// returns the caps.
pub(crate) fn cap_open_ends(txn: &mut Transaction<'_>, p: CapParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = if p.element_ids.is_empty() {
        store.iter().filter(|e| is_run_element(e)).map(|e| e.id()).collect()
    } else {
        p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?
    };
    let open = mep::open_ports(&store, &[]);
    let mut caps = Vec::new();
    for port in open.iter().filter(|o| ids.contains(&o.element)) {
        let e = store.get(port.element).expect("listed");
        if !is_run_element(e) {
            continue;
        }
        let (domain, level, class_, type_guid) = match (segment_recipe(e), fitting_recipe(e)) {
            (Some(s), _) => (s.domain, s.level, s.classification, Some(s.segment_type)),
            (_, Some(f)) => (f.domain, f.level, f.classification, None),
            _ => continue,
        };
        let routing = type_guid
            .as_deref()
            .and_then(GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|t| store.get(t))
            .and_then(segment_type)
            .map(|t| t.routing)
            .unwrap_or_default();
        let size = Size { shape: port.connector.shape, diameter: port.connector.diameter, width: port.connector.width, height: port.connector.height };
        let Some(part) = routing.choose(Role::Cap, size.nominal()) else {
            warn(txn, "mep.routing.no_part", format!("no cap in the routing preferences fits {}", size.text()), vec![port.element]);
            continue;
        };
        let cx = Ctx { domain, type_guid: String::new(), routing, level, classification: class_ };
        // The cap's one leg points back into the end it closes, so the two ports face.
        let centre = add(port.position, mul(port.direction, 0.03));
        let id = make_fitting(txn, &cx, part, centre, vec![Leg { direction: mul(port.direction, -1.0), length: 0.03, size, across: None }])?;
        mep::connect_coincident(txn, &[id])?;
        caps.push(id);
    }
    Ok(caps)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReapplyParams {
    /// Fittings (or segments, for the fittings joined to them); every fitting when empty.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// What a reapply did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ReapplyReport {
    pub replaced: Vec<ElementId>,
    /// Fittings whose role has no row that fits: removed, their connections left open.
    pub removed: Vec<ElementId>,
}

/// Re-chooses fittings by their segments' routing preferences (`mep_run.reapply_type`): a
/// fitting whose part changes is remade; one whose role has no fitting row is removed with a
/// warning, its ends left open.
pub(crate) fn reapply_type(txn: &mut Transaction<'_>, p: ReapplyParams) -> Result<ReapplyReport> {
    let store = txn.store().clone();
    let partners = mep::partners(&store);
    let mut fittings: Vec<ElementId> = Vec::new();
    if p.element_ids.is_empty() {
        fittings = store.iter().filter(|e| fitting_recipe(e).is_some()).map(|e| e.id()).collect();
    } else {
        for r in &p.element_ids {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
            let e = store.get(id).expect("resolved");
            if fitting_recipe(e).is_some() {
                fittings.push(id);
            } else if segment_recipe(e).is_some() {
                for end in [0, 1] {
                    for (f, _) in partners.get(&(id, end)).into_iter().flatten() {
                        if store.get(*f).and_then(fitting_recipe).is_some() && !fittings.contains(f) {
                            fittings.push(*f);
                        }
                    }
                }
            }
        }
    }
    let mut report = ReapplyReport::default();
    for fid in fittings {
        let f = store.get(fid).and_then(fitting_recipe).expect("a fitting");
        let role = match f.part {
            Part::Elbow => Role::Elbow,
            Part::Tee | Part::Tap => Role::Junction,
            Part::Cross => Role::Cross,
            Part::Transition => Role::Transition,
            Part::Cap => Role::Cap,
        };
        // The routing of a segment it joins.
        let routing = (0..f.legs.len())
            .flat_map(|k| partners.get(&(fid, k)).into_iter().flatten())
            .find_map(|(s, _)| {
                let sr = store.get(*s).and_then(segment_recipe)?;
                let t = store.find_by_guid(&GlobalId::parse(&sr.segment_type)?)?;
                store.get(t).and_then(segment_type).map(|t| t.routing)
            })
            .unwrap_or_default();
        let size = match role {
            Role::Junction => f.legs.last().map_or(0.0, |l| l.size.nominal()),
            _ => f.legs.first().map_or(0.0, |l| l.size.nominal()),
        };
        match routing.choose(role, size) {
            Some(part) if part == f.part => {}
            Some(part) if role == Role::Junction && (part == Part::Tap) != (f.part == Part::Tap) => {
                // A tee becoming a tap (or back) changes the host segments; left as it is.
                warn(txn, "mep.routing.junction_kind", format!("the junction would now be a {}; redraw the branch to change it", part.label().to_lowercase()), vec![fid]);
            }
            Some(part) => {
                let mut nf = f.clone();
                nf.part = part;
                let v = serde_json::to_value(&nf).expect("serialises");
                txn.modify(fid, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                    e.name = Some(part.label().into());
                    e.params.insert("@PredefinedType".into(), Value::from(part.ifc()));
                })?;
                report.replaced.push(fid);
            }
            None => {
                warn(
                    txn,
                    "mep.routing.no_part",
                    format!("no {} in the routing preferences fits {:.0} mm; the fitting is removed and its ends left open", role.label(), size * 1000.0),
                    vec![fid],
                );
                txn.delete(fid)?;
                report.removed.push(fid);
            }
        }
    }
    // Links to removed fittings drop out.
    let store = txn.store().clone();
    let links = mep::stored_links(&store);
    mep::save_links(txn, links)?;
    Ok(report)
}

/// Moves a segment or fitting: its points through `point` in plan and `dz` up; its directions
/// turned (and mirrored) with them.
pub(crate) fn transform(txn: &mut Transaction<'_>, id: ElementId, point: &dyn Fn([f64; 2]) -> [f64; 2], dz: f64) -> Result<()> {
    let e = txn.store().get(id).cloned().ok_or_else(|| bad("element.not_found", "no such element"))?;
    let p3 = |p: V3| {
        let q = point([p[0], p[1]]);
        [q[0], q[1], p[2] + dz]
    };
    // A direction through the plan map: the image of its tip less the image of its tail.
    let d3 = |d: V3| {
        let (o, t) = (point([0.0, 0.0]), point([d[0], d[1]]));
        let v = [t[0] - o[0], t[1] - o[1], d[2]];
        unit(v).unwrap_or(d)
    };
    let value = if let Some(mut s) = segment_recipe(&e) {
        s.start = p3(s.start);
        s.end = p3(s.end);
        s.across = s.across.map(d3);
        for t in &mut s.taps {
            t.at = p3(t.at);
            t.direction = d3(t.direction);
        }
        serde_json::to_value(&s).expect("serialises")
    } else if let Some(mut f) = fitting_recipe(&e) {
        f.centre = p3(f.centre);
        for l in &mut f.legs {
            l.direction = d3(l.direction);
            l.across = l.across.map(d3);
        }
        serde_json::to_value(&f).expect("serialises")
    } else {
        return Err(bad("mep_run.not_a_run", format!("element {id} is not a duct, pipe or fitting")));
    };
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// The colour each segment and fitting shows: its system type's, else a domain grey.
pub fn colors(store: &Store) -> BTreeMap<ElementId, [f32; 4]> {
    let by_system = crate::mep_systems::element_colors(store);
    store
        .iter()
        .filter(|e| is_run_element(e))
        .map(|e| {
            let duct = segment_recipe(e).map(|s| s.domain).or(fitting_recipe(e).map(|f| f.domain)) == Some(Domain::Duct);
            let base = if duct { [0.74, 0.76, 0.80, 1.0] } else { [0.62, 0.66, 0.60, 1.0] };
            (e.id(), by_system.get(&e.id()).copied().unwrap_or(base))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_rows_choose_by_size_and_preference() {
        let r = Routing {
            junction: vec![Row { part: Part::Tap, min: 0.0, max: 0.3 }, Row { part: Part::Tee, min: 0.3, max: 0.0 }],
            preferred_junction: Junction::Tee,
            ..Routing::default()
        };
        assert_eq!(r.choose(Role::Junction, 0.25), Some(Part::Tap));
        assert_eq!(r.choose(Role::Junction, 0.4), Some(Part::Tee));
        let none = Routing { elbow: vec![Row { part: Part::Elbow, min: 0.0, max: 0.2 }], ..Routing::default() };
        assert_eq!(none.choose(Role::Elbow, 0.3), None);
        assert!(Routing { elbow: vec![Row { part: Part::Tee, min: 0.0, max: 0.0 }], ..Routing::default() }.check().is_err());
    }

    #[test]
    fn a_rectangular_section_is_level_across() {
        let s = Size { shape: Shape::Rectangular, diameter: 0.0, width: 0.6, height: 0.3 };
        let pts = section([0.0; 3], [0.0, 1.0, 0.0], &s, None);
        // Across is x for a run along y; up is z.
        assert!((pts[1][0] - pts[0][0]).abs() - 0.6 < 1e-9 && (pts[2][2] - pts[1][2] - 0.3).abs() < 1e-9, "{pts:?}");
    }
}
