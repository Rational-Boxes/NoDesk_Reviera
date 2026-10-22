// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Flexible duct (MEP-DUCT-005).
//!
//! - **Types**: duct segment types marked `flexible` (built-in *Round Flex Duct* and
//!   *Rectangular Flex Duct*, made on first use): a roughness, a minimum bend radius (the bend
//!   radius multiplier times the diameter or width, 1 by default) and a loss multiplier for
//!   compressed flex (1 by default). A flexible type cannot draw rigid runs, nor a rigid one flex.
//! - **Flex duct** is an `IfcDuctSegment` FLEXIBLESEGMENT whose recipe holds a spline through
//!   its points ([`FlexPath`]): a cubic Hermite curve, each point's tangent along the bisector of
//!   its two chords and each span's tangents as long as its chord, so it passes through every
//!   point; the end tangents follow the connectors it starts and ends on (else the first and
//!   last chords). Its body is its section swept along the curve (the across direction carried
//!   along without twist), its length the curve's.
//! - `flex_duct.create` starts on an open duct connector, or on a rigid duct's centreline (a
//!   tap, chosen from the rigid duct's routing preferences, is put there; none fitting refuses
//!   the start naming the role and size), or at a free point; it ends on a connector (joined) or
//!   at a free point. A bend tighter than the type's minimum is drawn but warned about.
//! - `duct.convert_to_flex` replaces, per terminal, the final rigid segment (and the elbow at
//!   the terminal, if any) with flex of its size and system, when no longer than the limit;
//!   other terminals are listed with the reason.
//! - The pressure-drop calculation (MEP-CALC-002) uses the type's roughness, the spline length
//!   and the loss multiplier.
//!
//! Not yet: vertex, tangent and end grips and Flex Duct Controls, the flex pattern in plans,
//! flex pipe, a swept-disk body along an IFC B-spline (written as a faceted body).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{self, Classification, Domain, Port, PortRef, Shape};
use crate::mep_runs::{self, RunReport, SegmentRecipe, SegmentType, Size, Solid};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

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
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}
fn plan_dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Built-in flex duct roughness (metres): 3 mm, fully extended fabric-and-wire flex (E-30).
pub const ROUGHNESS: f64 = 0.003;
/// How far in plan a snapped click finds a connector (metres).
const SNAP: f64 = 0.3;

// ---- The spline ----

/// A flex duct's spline: through `points` (the first and last are its ends), leaving the first
/// along `start_tangent` and arriving at the last along `end_tangent` (both unit, in the
/// direction of travel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlexPath {
    pub points: Vec<V3>,
    pub start_tangent: V3,
    pub end_tangent: V3,
}

impl FlexPath {
    /// Each point's unit tangent: the ends' given, the others along the bisector of their
    /// chords.
    fn tangents(&self) -> Vec<V3> {
        let n = self.points.len();
        (0..n)
            .map(|k| {
                if k == 0 {
                    return self.start_tangent;
                }
                if k == n - 1 {
                    return self.end_tangent;
                }
                let (a, b) = (unit(sub(self.points[k], self.points[k - 1])), unit(sub(self.points[k + 1], self.points[k])));
                match (a, b) {
                    (Some(a), Some(b)) => unit(add(a, b)).unwrap_or(b),
                    (a, b) => a.or(b).unwrap_or(self.start_tangent),
                }
            })
            .collect()
    }

    /// Points along the curve with their unit tangents, `per_span` steps a span.
    pub fn samples(&self, per_span: usize) -> Vec<(V3, V3)> {
        let t = self.tangents();
        let mut out = Vec::new();
        for k in 0..self.points.len().saturating_sub(1) {
            let (p0, p1) = (self.points[k], self.points[k + 1]);
            let c = len(sub(p1, p0));
            let (m0, m1) = (mul(t[k], c), mul(t[k + 1], c));
            for j in 0..=per_span {
                if k > 0 && j == 0 {
                    continue;
                }
                let s = j as f64 / per_span as f64;
                let (s2, s3) = (s * s, s * s * s);
                let p = add(add(mul(p0, 2.0 * s3 - 3.0 * s2 + 1.0), mul(m0, s3 - 2.0 * s2 + s)), add(mul(p1, -2.0 * s3 + 3.0 * s2), mul(m1, s3 - s2)));
                let d = add(add(mul(p0, 6.0 * s2 - 6.0 * s), mul(m0, 3.0 * s2 - 4.0 * s + 1.0)), add(mul(p1, -6.0 * s2 + 6.0 * s), mul(m1, 3.0 * s2 - 2.0 * s)));
                out.push((p, unit(d).unwrap_or(t[k])));
            }
        }
        out
    }

    /// The curve's length (metres).
    pub fn length(&self) -> f64 {
        self.samples(64).windows(2).map(|w| len(sub(w[1].0, w[0].0))).sum()
    }

    /// The tightest bend's radius (metres); none for a straight run.
    pub fn min_radius(&self) -> Option<f64> {
        self.samples(32)
            .windows(2)
            .filter_map(|w| {
                let angle = dot(w[0].1, w[1].1).clamp(-1.0, 1.0).acos();
                (angle > 1e-4).then(|| len(sub(w[1].0, w[0].0)) / angle)
            })
            .min_by(f64::total_cmp)
    }
}

/// The section's rings along the curve, the across direction carried along without twist.
fn rings(f: &FlexPath, size: &Size) -> Vec<Vec<V3>> {
    let path = f.samples(12);
    let Some(first) = path.first() else { return Vec::new() };
    let mut w = mep_runs::frame(first.1).0;
    path.iter()
        .map(|(p, t)| {
            w = unit(sub(w, mul(*t, dot(w, *t)))).unwrap_or_else(|| mep_runs::frame(*t).0);
            mep_runs::section(*p, *t, size, Some(w))
        })
        .collect()
}

/// A flex duct's body: its section swept along the spline.
pub(crate) fn solid(f: &FlexPath, size: &Size) -> Option<Solid> {
    let rings = rings(f, size);
    let m = rings.first()?.len();
    if rings.len() < 2 {
        return None;
    }
    let points: Vec<V3> = rings.iter().flatten().copied().collect();
    let last = (rings.len() - 1) * m;
    let mut faces = vec![(0..m).rev().collect::<Vec<_>>(), (last..last + m).collect()];
    for k in 0..rings.len() - 1 {
        for j in 0..m {
            let (a, b) = (k * m + j, k * m + (j + 1) % m);
            faces.push(vec![a, b, b + m, a + m]);
        }
    }
    Some(Solid { points, faces })
}

/// Insulation or lining round a flex duct: a hollow sweep from `inner` to `outer`.
pub(crate) fn tube(f: &FlexPath, outer: &Size, inner: &Size) -> Option<Solid> {
    let (o, i) = (rings(f, outer), rings(f, inner));
    let m = o.first()?.len();
    let n = o.len();
    if n < 2 || i.first().map(Vec::len) != Some(m) {
        return None;
    }
    let base = n * m;
    let mut points: Vec<V3> = o.iter().flatten().copied().collect();
    points.extend(i.iter().flatten().copied());
    let last = (n - 1) * m;
    let mut faces = Vec::new();
    for j in 0..m {
        let jj = (j + 1) % m;
        for k in 0..n - 1 {
            let (a, b) = (k * m + j, k * m + jj);
            faces.push(vec![a, b, b + m, a + m]);
            faces.push(vec![base + a, base + a + m, base + b + m, base + b]);
        }
        faces.push(vec![jj, j, base + j, base + jj]);
        faces.push(vec![last + j, last + jj, base + last + jj, base + last + j]);
    }
    Some(Solid { points, faces })
}

// ---- Types ----

fn default_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Round => "Round Flex Duct",
        _ => "Rectangular Flex Duct",
    }
}

/// The built-in flex duct type of a shape: an existing one, or a new one.
fn default_type(txn: &mut Transaction<'_>, shape: Shape) -> Result<ElementId> {
    let shape = if shape == Shape::Round { Shape::Round } else { Shape::Rectangular };
    let name = default_name(shape);
    if let Some(e) = txn
        .store()
        .iter()
        .find(|e| e.name.as_deref() == Some(name) && mep_runs::segment_type(e).is_some_and(|t| t.flexible && t.shape == shape))
    {
        return Ok(e.id());
    }
    mep_runs::create_type(
        txn,
        mep_runs::CreateTypeParams {
            domain: Domain::Duct,
            name: name.into(),
            shape,
            routing: None,
            material: Some("Aluminium Foil and Wire Helix".into()),
            roughness: Some(ROUGHNESS),
            with_fittings: None,
            tray_kind: None,
            bend_radius_multiplier: Some(1.0),
            standard: None,
            sizes: None,
            flexible: Some(true),
            loss_multiplier: None,
        },
    )
}

/// The flex type to use: the one given (which must be flexible), else the built-in of the shape.
fn flex_type(txn: &mut Transaction<'_>, given: Option<&ElementRef>, shape: Shape) -> Result<(ElementId, SegmentType)> {
    let id = match given {
        Some(r) => {
            let (id, t) = mep_runs::resolve_type(txn.store(), r)?;
            if !t.flexible {
                return Err(bad("mep_segment_type.flexible", "that is a rigid duct type; flex duct needs a flexible one"));
            }
            id
        }
        None => default_type(txn, shape)?,
    };
    let t = mep_runs::segment_type(txn.store().get(id).expect("made")).expect("a type");
    Ok((id, t))
}

/// The minimum bend radius of a flex type at a size.
fn min_bend(t: &SegmentType, size: &Size) -> f64 {
    let base = if size.shape == Shape::Round { size.diameter } else { size.width.min(size.height) };
    base * t.bend_radius_multiplier
}

// ---- Making flex duct ----

#[allow(clippy::too_many_arguments)]
fn make(txn: &mut Transaction<'_>, type_id: ElementId, t: &SegmentType, path: FlexPath, size: Size, level: String, classification: Option<Classification>) -> Result<ElementId> {
    let r = SegmentRecipe {
        kind: mep_runs::SEGMENT.into(),
        domain: Domain::Duct,
        segment_type: txn.store().get(type_id).expect("a type").guid().to_string(),
        start: path.points[0],
        end: *path.points.last().expect("two"),
        size,
        level,
        classification,
        taps: Vec::new(),
        slope: 0.0,
        across: None,
        corner: None,
        size_locked: false,
        riser: None,
        flex: Some(path.clone()),
        justification: None,
    };
    let type_guid = r.segment_type.clone();
    let id = txn.create(
        NewElement::new(mep_runs::segment_class(Domain::Duct))
            .param("@PredefinedType", "FLEXIBLESEGMENT")
            .param("@type", type_guid)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    if let Some(rad) = path.min_radius()
        && rad < min_bend(t, &size) - 1e-6
    {
        txn.warn(Warning {
            id: "flex_duct.bend_radius".into(),
            message: format!(
                "the flex duct bends at a {:.0} mm radius, tighter than its type's minimum of {:.0} mm",
                rad * 1000.0,
                min_bend(t, &size) * 1000.0
            ),
            element_ids: vec![id],
        });
    }
    Ok(id)
}

/// A run element's classification.
fn classification_of(store: &Store, id: ElementId) -> Option<Classification> {
    let e = store.get(id)?;
    mep_runs::segment_recipe(e).and_then(|s| s.classification).or_else(|| mep_runs::fitting_recipe(e).and_then(|f| f.classification))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A flexible duct type; the built-in flex type of the shape when omitted.
    pub type_id: Option<ElementRef>,
    /// The points the spline passes through: `[x, y, z]` (height above the level) or `[x, y]`
    /// at `middle_elevation`.
    pub points: Vec<Vec<f64>>,
    pub level: ElementRef,
    /// Height above the level of points given in plan (metres; default 2.7).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub middle_elevation: Option<f64>,
    pub shape: Option<Shape>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// The system type it carries; else that of what it starts or ends on.
    pub system_type_id: Option<String>,
    /// The connector it starts on (the first point moves to it).
    pub start_port: Option<PortRef>,
    /// The connector it ends on (the last point moves to it).
    pub end_port: Option<PortRef>,
    /// The first and last points find, in plan within 300 mm, an open duct connector, and the
    /// first a rigid duct's centreline (taking their height); otherwise they must meet them.
    #[serde(default)]
    pub snap: bool,
}

/// Where a flex end lies.
enum EndOn {
    Port(Port),
    /// A rigid duct's side: the duct and the point on its axis.
    Side(ElementId, SegmentRecipe, V3),
    Free,
}

fn port_of(store: &Store, r: &PortRef) -> Result<Port> {
    let id = store.resolve(&r.element).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let p = mep::port(store, id, r.port).ok_or_else(|| bad("connector.not_found", format!("element {id} has no connector {}", r.port)))?;
    if p.connector.domain != Domain::Duct {
        return Err(bad("flex_duct.port", "flex duct connects to duct connectors"));
    }
    if mep::partners(store).contains_key(&(id, r.port)) {
        return Err(bad("flex_duct.port", format!("connector {} of element {id} is already connected", r.port)));
    }
    Ok(p)
}

/// What a point lies on: an open duct connector, or (`side`) a rigid straight duct's
/// centreline away from its ends; in plan when `plan`.
fn end_on(store: &Store, p: V3, plan: bool, side: bool) -> EndOn {
    let d = |a: V3, b: V3| if plan { plan_dist(a, b) } else { len(sub(a, b)) };
    let reach = if plan { SNAP } else { mep::POSITION_TOLERANCE * 2.0 };
    if let Some(port) = mep::open_ports(store, &[Domain::Duct])
        .into_iter()
        .map(|o| (d(o.position, p), o))
        .filter(|(x, _)| *x <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
    {
        return EndOn::Port(port);
    }
    if !side {
        return EndOn::Free;
    }
    store
        .iter()
        .filter_map(|e| {
            let s = mep_runs::segment_recipe(e)?;
            if s.domain != Domain::Duct || s.corner.is_some() || s.flex.is_some() {
                return None;
            }
            let u = unit(sub(s.end, s.start))?;
            let l = len(sub(s.end, s.start));
            let t = if plan {
                let lh = u[0] * u[0] + u[1] * u[1];
                if lh < 1e-6 {
                    return None;
                }
                ((p[0] - s.start[0]) * u[0] + (p[1] - s.start[1]) * u[1]) / lh
            } else {
                dot(sub(p, s.start), u)
            };
            let margin = mep_runs::leg_length(&s.size);
            if t <= margin || t >= l - margin {
                return None;
            }
            let q = add(s.start, mul(u, t));
            let off = d(q, p);
            (off <= s.size.nominal() / 2.0 + 1e-6).then_some((off, e.id(), s, q))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(EndOn::Free, |(_, id, s, q)| EndOn::Side(id, s, q))
}

/// `flex_duct.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    let store = txn.store().clone();
    let (level, z0) = mep_runs::level_of(&store, &p.level)?;
    let mid = p.middle_elevation.unwrap_or(2.7);
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
        return Err(bad("mep_run.points", "a flex duct needs two different points"));
    }
    let n = pts.len();
    let start = match &p.start_port {
        Some(r) => EndOn::Port(port_of(&store, r)?),
        None => end_on(&store, pts[0], p.snap, true),
    };
    let end = match &p.end_port {
        Some(r) => EndOn::Port(port_of(&store, r)?),
        None => end_on(&store, pts[n - 1], p.snap, false),
    };
    if let (EndOn::Port(a), EndOn::Port(b)) = (&start, &end)
        && (a.element, a.index) == (b.element, b.index)
    {
        return Err(bad("flex_duct.port", "the flex duct starts and ends on the same connector"));
    }
    // The size: given, else the connector's it starts or ends on.
    let from_port = [&start, &end].into_iter().find_map(|e| match e {
        EndOn::Port(port) => Some(port.connector.clone()),
        _ => None,
    });
    let shape = p.shape.or(from_port.as_ref().map(|c| c.shape)).unwrap_or(Shape::Round);
    let given = p.diameter.is_some() || p.width.is_some() || p.height.is_some();
    let size = match (&from_port, given) {
        (Some(c), false) if c.shape == shape => Size { shape, diameter: c.diameter, width: c.width, height: c.height },
        _ => Size { shape, diameter: p.diameter.unwrap_or(0.0), width: p.width.unwrap_or(0.0), height: p.height.unwrap_or(0.0) },
    };
    if shape == Shape::Oval {
        return Err(bad("mep_run.shape", "flex duct is round or rectangular"));
    }
    let (type_id, t) = flex_type(txn, p.type_id.as_ref(), shape)?;
    if t.shape != shape {
        return Err(bad("mep_run.shape", format!("the type is {}", if t.shape == Shape::Round { "round" } else { "rectangular" })));
    }
    t.check_size(&store, &size)?;
    let mut classification = match p.system_type_id.as_deref() {
        Some(id) => {
            let st = crate::mep_systems::system_type(&store, id).ok_or_else(|| bad("system_type.not_found", format!("no system type {id:?}")))?;
            if !st.classification.fits(Domain::Duct) {
                return Err(bad("mep_run.classification", format!("a duct cannot carry {}", st.classification.label())));
            }
            Some(st.classification)
        }
        None => None,
    };
    if classification.is_none() {
        classification = [&start, &end].into_iter().find_map(|e| match e {
            EndOn::Port(port) => classification_of(&store, port.element),
            EndOn::Side(id, ..) => classification_of(&store, *id),
            EndOn::Free => None,
        });
    }
    let mut report = RunReport::default();
    // The start: a connector, a tap on a duct's side, or the first point.
    let after_start = |k: usize, pts: &[V3]| pts.get(k).copied();
    let start_tangent = match &start {
        EndOn::Port(port) => {
            pts[0] = port.position;
            port.direction
        }
        EndOn::Side(host, hs, q) => {
            let host_type = mep_runs::type_of(&store, hs);
            let routing = host_type.as_ref().map(|t| t.routing.clone()).unwrap_or_default();
            let tap_fits = routing.junction.iter().any(|r| r.part == mep_runs::Part::Tap && (r.min <= size.nominal() + 1e-9) && (r.max <= 0.0 || size.nominal() <= r.max + 1e-9));
            if !tap_fits {
                return Err(bad(
                    "flex_duct.no_tap",
                    format!("no tap (junction) in the rigid duct's routing preferences fits {}; add one or start on a connector", size.text()),
                ));
            }
            let axis = unit(sub(hs.end, hs.start)).expect("a segment");
            let next = after_start(1, &pts).expect("two");
            let d = sub(next, *q);
            let branch = unit(sub(d, mul(axis, dot(d, axis)))).ok_or_else(|| bad("flex_duct.branch", "the flex duct must leave the duct to one side"))?;
            if size.nominal() > hs.size.nominal() + 1e-9 {
                return Err(bad("flex_duct.branch", format!("a {} tap is larger than the {} duct", size.text(), hs.size.text())));
            }
            let (tap, tip) = mep_runs::make_tap(txn, *host, *q, branch, size, classification)?;
            report.fittings.push(tap);
            pts[0] = tip;
            if n > 2 && len(sub(pts[1], tip)) < 1e-3 {
                pts.remove(1);
            }
            branch
        }
        EndOn::Free => unit(sub(pts[1], pts[0])).expect("distinct"),
    };
    let last = pts.len() - 1;
    let end_tangent = match &end {
        EndOn::Port(port) => {
            pts[last] = port.position;
            mul(port.direction, -1.0)
        }
        _ => unit(sub(pts[last], pts[last - 1])).ok_or_else(|| bad("mep_run.points", "the flex duct's last two points meet"))?,
    };
    if pts.windows(2).any(|w| len(sub(w[1], w[0])) < 1e-3) {
        return Err(bad("mep_run.points", "two of the flex duct's points meet"));
    }
    let path = FlexPath { points: pts, start_tangent, end_tangent };
    let id = make(txn, type_id, &t, path, size, level, classification)?;
    report.segments.push(id);
    let made: Vec<ElementId> = report.segments.iter().chain(&report.fittings).copied().collect();
    let before: Vec<ElementId> = store.iter().map(|e| e.id()).collect();
    for c in mep::connect_coincident(txn, &made)? {
        for e in [c.a.0, c.b.0] {
            if before.contains(&e) && !report.connected_to.contains(&e) {
                report.connected_to.push(e);
            }
        }
    }
    if let EndOn::Port(port) = &end
        && !mep::partners(txn.store()).contains_key(&(port.element, port.index))
    {
        txn.warn(Warning {
            id: "flex_duct.not_connected".into(),
            message: format!("the flex duct's end could not join the connector (it is {}, the flex {})", connector_text(&port.connector), size.text()),
            element_ids: vec![id, port.element],
        });
    }
    report.system = mep_runs::join_system(txn, &made, classification)?;
    Ok(report)
}

fn connector_text(c: &mep::Connector) -> String {
    Size { shape: c.shape, diameter: c.diameter, width: c.width, height: c.height }.text()
}

// ---- Convert to flex ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertParams {
    /// The terminals (air terminals or any element with a duct connector).
    pub terminal_ids: Vec<ElementRef>,
    /// The longest final segment (with the elbow at the terminal) converted (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub max_length: f64,
    /// A flexible duct type; the built-in flex type of the duct's shape when omitted.
    pub type_id: Option<ElementRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Skipped {
    pub terminal: ElementId,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ConvertReport {
    /// The flex ducts made.
    pub ids: Vec<ElementId>,
    /// Terminals left as they are, with the reason.
    pub skipped: Vec<Skipped>,
}

/// The final rigid run at a terminal: its duct port, the segment, the segment's far port and
/// the elbow between them; or why there is none.
fn final_run(store: &Store, terminal: ElementId, max_length: f64) -> std::result::Result<(Port, ElementId, SegmentRecipe, usize, Option<ElementId>), String> {
    let e = store.get(terminal).ok_or("no such element")?;
    let partners = mep::partners(store);
    let duct_ports: Vec<Port> = mep::ports(store, e).into_iter().filter(|p| p.connector.domain == Domain::Duct).collect();
    let (tport, (mut el, mut k)) = duct_ports
        .iter()
        .find_map(|p| partners.get(&(terminal, p.index)).and_then(|v| v.first()).map(|x| (p.clone(), *x)))
        .ok_or("not connected to a duct")?;
    let mut elbow = None;
    let mut extra = 0.0;
    if let Some(f) = store.get(el).and_then(mep_runs::fitting_recipe) {
        if f.part != mep_runs::Part::Elbow || f.legs.len() != 2 {
            return Err(format!("ends in a {}, not a straight duct", f.part.label().to_lowercase()));
        }
        extra = f.legs.iter().map(|l| l.length).sum();
        elbow = Some(el);
        let next = partners.get(&(el, 1 - k)).and_then(|v| v.first()).copied().ok_or("the elbow leads nowhere")?;
        (el, k) = next;
    }
    let s = store.get(el).and_then(mep_runs::segment_recipe).ok_or("not ended by a straight duct segment")?;
    if s.flex.is_some() {
        return Err("already flex".into());
    }
    if s.domain != Domain::Duct || s.corner.is_some() || !s.taps.is_empty() || k > 1 {
        return Err("not ended by a plain straight duct segment".into());
    }
    let l = mep_runs::segment_length(&s) + extra;
    if l > max_length + 1e-9 {
        return Err(format!("its final segment is {:.2} m, over the {:.2} m limit", l, max_length));
    }
    Ok((tport, el, s, 1 - k, elbow))
}

/// `duct.convert_to_flex`.
pub(crate) fn convert(txn: &mut Transaction<'_>, p: ConvertParams) -> Result<ConvertReport> {
    if !(p.max_length > 0.0 && p.max_length.is_finite()) {
        return Err(bad("flex_duct.max_length", "the maximum length is above zero"));
    }
    if p.terminal_ids.is_empty() {
        return Err(bad("flex_duct.terminals", "select terminals to convert"));
    }
    let mut report = ConvertReport::default();
    for r in &p.terminal_ids {
        let store = txn.store().clone();
        let terminal = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such terminal"))?;
        let (tport, seg, s, far, elbow) = match final_run(&store, terminal, p.max_length) {
            Ok(x) => x,
            Err(reason) => {
                report.skipped.push(Skipped { terminal, reason });
                continue;
            }
        };
        let (type_id, t) = match flex_type(txn, p.type_id.as_ref(), s.size.shape) {
            Ok(x) if x.1.shape == s.size.shape => x,
            Ok(_) => {
                report.skipped.push(Skipped { terminal, reason: format!("the flex type is not {}", if s.size.shape == Shape::Round { "round" } else { "rectangular" }) });
                continue;
            }
            Err(e) => return Err(e),
        };
        let fp = mep::port(&store, seg, far).expect("a segment end");
        let path = FlexPath { points: vec![fp.position, tport.position], start_tangent: mul(fp.direction, -1.0), end_tangent: mul(tport.direction, -1.0) };
        for old in std::iter::once(seg).chain(elbow) {
            for c in crate::mep_coverings::of_host(&store, old) {
                txn.delete(c)?;
            }
            txn.delete(old)?;
        }
        let id = make(txn, type_id, &t, path, s.size, s.level.clone(), s.classification)?;
        mep::connect_coincident(txn, &[id])?;
        mep_runs::join_system(txn, &[id], s.classification)?;
        report.ids.push(id);
    }
    // Links to the replaced elements drop out.
    let links = mep::stored_links(txn.store());
    mep::save_links(txn, links)?;
    Ok(report)
}
