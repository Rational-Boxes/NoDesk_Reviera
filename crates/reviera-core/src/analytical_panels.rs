// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical panels and openings (STR-AM-002): surface elements that idealise walls, floors and
//! roofs. A **panel** (`IfcStructuralSurfaceMember`, `recipe.analytical_panel`) is bounded by its
//! corner nodes, shared with members and other panels like members' end nodes are (STR-AM-003),
//! so the model stays connected. It is planar (by boundary) or a vertical extrusion of an arc
//! (a curved panel: its bottom and top edges are arcs through a plan point). It has a thickness,
//! a material, a role (floor, wall or roof), *Analyze As* (shell, membrane or bending plate),
//! openings (inner boundaries, kept in its recipe) and the physical elements it idealises.
//! **Cladding** panels carry loads to their supports but add no stiffness; providers are told to
//! leave them out of the stiffness matrix.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::analytical::{self, find};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const PANEL_RECIPE: &str = "recipe.analytical_panel";

type V3 = [f64; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelRole {
    #[default]
    Floor,
    Wall,
    Roof,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelAnalyzeAs {
    /// Membrane and bending together.
    #[default]
    Shell,
    /// In-plane forces only.
    Membrane,
    /// Out-of-plane bending only.
    BendingPlate,
}

/// A curved panel: its bottom edge runs from the first corner through `through` (plan) to the
/// second, its top edge likewise from the fourth to the third.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelArc {
    pub through: [f64; 2],
}

/// The stored recipe of a panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelRecipe {
    pub kind: String,
    /// Its corner nodes (GlobalIds), in order around the outer boundary.
    pub corners: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<PanelArc>,
    /// Metres.
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default)]
    pub role: PanelRole,
    #[serde(default)]
    pub analyze_as: PanelAnalyzeAs,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cladding: bool,
    /// Inner boundaries (metres, on the panel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<V3>>,
    /// The physical elements it idealises (GlobalIds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub physical: Vec<String>,
}

pub fn panel_recipe(e: &Element) -> Option<PanelRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == PANEL_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A panel's PredefinedType.
pub fn predefined(r: &PanelRecipe) -> &'static str {
    if r.cladding {
        return "USERDEFINED";
    }
    match r.analyze_as {
        PanelAnalyzeAs::Shell => "SHELL",
        PanelAnalyzeAs::Membrane => "MEMBRANE_ELEMENT",
        PanelAnalyzeAs::BendingPlate => "BENDING_ELEMENT",
    }
}

// ---- Geometry ----

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = norm(a).max(1e-300);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// The unit normal of a polygon (Newell's method), or `None` when it is degenerate.
pub fn normal(points: &[V3]) -> Option<V3> {
    let mut n = [0.0; 3];
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    (norm(n) > 1e-9).then(|| unit(n))
}

/// A frame on a plane: origin, in-plane axes u and v.
struct Frame {
    o: V3,
    u: V3,
    v: V3,
    n: V3,
}

impl Frame {
    fn of(points: &[V3]) -> Option<Self> {
        let n = normal(points)?;
        let o = points[0];
        let u = unit(
            points
                .iter()
                .map(|p| sub(*p, o))
                .find(|d| norm(*d) > 1e-6)?,
        );
        let v = cross(n, u);
        Some(Self { o, u, v, n })
    }
    fn to2(&self, p: V3) -> [f64; 2] {
        let d = sub(p, self.o);
        [dot(d, self.u), dot(d, self.v)]
    }
    fn to3(&self, q: [f64; 2]) -> V3 {
        [0, 1, 2].map(|k| self.o[k] + self.u[k] * q[0] + self.v[k] * q[1])
    }
    fn off(&self, p: V3) -> f64 {
        dot(sub(p, self.o), self.n)
    }
}

fn signed_area(p: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x, y) = (p[i], p[(i + 1) % p.len()]);
        a += x[0] * y[1] - y[0] * x[1];
    }
    a / 2.0
}

fn inside(q: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The plan arc from `a` through `m` to `b`, sampled every 7.5° at most (ends included).
pub fn arc_points(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let (c, r) = crate::walls::curved::circle(a, m, b)?;
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let tau = std::f64::consts::TAU;
    let (a0, am, ab) = (ang(a), ang(m), ang(b));
    let rel = |x: f64| (x - a0).rem_euclid(tau);
    let sweep = if rel(am) < rel(ab) {
        rel(ab)
    } else {
        rel(ab) - tau
    };
    let n = ((sweep.abs() / 7.5f64.to_radians()).ceil() as usize).max(2);
    Some(
        (0..=n)
            .map(|k| {
                let t = a0 + sweep * k as f64 / n as f64;
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect(),
    )
}

/// A panel as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelGeometry {
    /// The corner nodes and their points.
    pub nodes: Vec<ElementId>,
    pub corners: Vec<V3>,
    /// The outer boundary (arcs sampled).
    pub outline: Vec<V3>,
    /// The plane's unit normal (planar panels).
    pub normal: Option<V3>,
    /// A curved panel's circle in plan: centre and radius.
    pub circle: Option<([f64; 2], f64)>,
    pub recipe: PanelRecipe,
}

impl PanelGeometry {
    /// The distance from `p` to the panel's surface (its plane or cylinder), ignoring its edges.
    pub fn surface_distance(&self, p: V3) -> f64 {
        if let Some((c, r)) = self.circle {
            return ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs();
        }
        match self.normal {
            Some(n) => dot(sub(p, self.corners[0]), n).abs(),
            None => f64::INFINITY,
        }
    }

    /// Whether `p` lies on the panel within `tol`: on its surface and inside its boundary.
    pub fn contains(&self, p: V3, tol: f64) -> bool {
        if self.surface_distance(p) > tol {
            return false;
        }
        if let Some((c, _)) = self.circle {
            // Between the bottom and top, and within the arc's sweep (by the outline in plan).
            let (lo, hi) = (
                self.corners.iter().map(|q| q[2]).fold(f64::INFINITY, f64::min),
                self.corners.iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max),
            );
            if p[2] < lo - tol || p[2] > hi + tol {
                return false;
            }
            let ang = |q: V3| (q[1] - c[1]).atan2(q[0] - c[0]);
            let half = self.outline.len() / 2;
            let a: Vec<f64> = self.outline[..half].iter().map(|q| ang(*q)).collect();
            let x = ang(p);
            return a.windows(2).any(|w| {
                let (s, e) = (w[0], w[1]);
                let d = (e - s + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                let t = (x - s + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                (d >= 0.0 && t >= -1e-9 && t <= d + 1e-9) || (d < 0.0 && t <= 1e-9 && t >= d - 1e-9)
            });
        }
        let Some(f) = Frame::of(&self.corners) else {
            return false;
        };
        let ring: Vec<[f64; 2]> = self.corners.iter().map(|q| f.to2(*q)).collect();
        let q = f.to2(p);
        inside(q, &ring) || edges(&self.corners).any(|(a, b)| seg_dist(p, a, b) <= tol)
    }

    /// The area of the panel (openings removed).
    pub fn area(&self) -> f64 {
        let Some(f) = Frame::of(&self.outline) else {
            return 0.0;
        };
        if let Some((_, r)) = self.circle {
            let half = self.outline.len() / 2;
            let len: f64 = self.outline[..half]
                .windows(2)
                .map(|w| norm(sub(w[1], w[0])))
                .sum();
            let h = (self.corners[3][2] - self.corners[0][2]).abs();
            let _ = r;
            return len * h;
        }
        let ring: Vec<[f64; 2]> = self.outline.iter().map(|q| f.to2(*q)).collect();
        let holes: f64 = self
            .recipe
            .openings
            .iter()
            .map(|o| signed_area(&o.iter().map(|q| f.to2(*q)).collect::<Vec<_>>()).abs())
            .sum();
        signed_area(&ring).abs() - holes
    }
}

/// The edges of a closed polygon.
pub fn edges(points: &[V3]) -> impl Iterator<Item = (V3, V3)> + '_ {
    (0..points.len()).map(move |i| (points[i], points[(i + 1) % points.len()]))
}

fn seg_dist(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    };
    norm(sub(p, [0, 1, 2].map(|k| a[k] + ab[k] * t)))
}

fn outline_of(corners: &[V3], arc: &Option<PanelArc>) -> (Vec<V3>, Option<([f64; 2], f64)>) {
    let Some(arc) = arc.as_ref().filter(|_| corners.len() == 4) else {
        return (corners.to_vec(), None);
    };
    let plan = |p: V3| [p[0], p[1]];
    let (Some(bottom), Some(circle)) = (
        arc_points(plan(corners[0]), arc.through, plan(corners[1])),
        crate::walls::curved::circle(plan(corners[0]), arc.through, plan(corners[1])),
    ) else {
        return (corners.to_vec(), None);
    };
    let (z0, z1) = (corners[0][2], corners[3][2]);
    let mut out: Vec<V3> = bottom.iter().map(|q| [q[0], q[1], z0]).collect();
    out.extend(bottom.iter().rev().map(|q| [q[0], q[1], z1]));
    (out, Some(circle))
}

/// A panel's geometry from its corner nodes.
pub fn geometry(store: &Store, e: &Element) -> Option<PanelGeometry> {
    let r = panel_recipe(e)?;
    let mut nodes = Vec::new();
    let mut corners = Vec::new();
    for g in &r.corners {
        let id = find(store, g)?;
        corners.push(analytical::node_point(store, store.get(id)?)?);
        nodes.push(id);
    }
    let (outline, circle) = outline_of(&corners, &r.arc);
    Some(PanelGeometry {
        normal: if circle.is_none() { normal(&corners) } else { None },
        nodes,
        corners,
        outline,
        circle,
        recipe: r,
    })
}

/// Every panel whose nodes exist.
pub fn panels(store: &Store) -> Vec<(ElementId, PanelGeometry)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), geometry(store, e)?)))
        .collect()
}

/// Whether an element depends on `guid`: a panel on its corner nodes.
pub fn uses(e: &Element, guid: &str) -> bool {
    panel_recipe(e).is_some_and(|r| r.corners.iter().any(|c| c == guid))
}

// ---- Creating ----

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &PanelRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    let pre = predefined(r);
    let cladding = r.cladding;
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@PredefinedType".into(), pre.into());
        if cladding {
            e.params.insert("@ObjectType".into(), "CLADDING".into());
        } else {
            e.params.remove("@ObjectType");
        }
    })
}

fn check_thickness(t: f64) -> Result<()> {
    if !(t > 0.0 && t <= 5.0) {
        return Err(Error::new(
            "analytical.bad_thickness",
            "a panel's thickness is above 0 and at most 5 m",
        ));
    }
    Ok(())
}

/// Makes a panel on the given corner points: nodes there (or existing ones within the merge
/// distance).
fn new_panel(
    txn: &mut crate::txn::Transaction<'_>,
    points: &[V3],
    mut r: PanelRecipe,
) -> Result<ElementId> {
    check_thickness(r.thickness)?;
    let tol = analytical::tolerance(txn.store());
    let mut corners: Vec<String> = Vec::new();
    for p in points {
        let g = analytical::node_at(txn, *p, tol)?;
        if corners.last() == Some(&g) || corners.first() == Some(&g) && corners.len() > 1 {
            continue;
        }
        corners.push(g);
    }
    if corners.len() < 3 {
        return Err(Error::new(
            "analytical.degenerate_panel",
            "a panel needs at least three distinct corners",
        ));
    }
    r.corners = corners;
    analytical::ensure_model(txn)?;
    let id = txn.create(
        NewElement::new("IfcStructuralSurfaceMember")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    write(txn, id, &r)?;
    Ok(id)
}

/// Checks a boundary: at least three points, planar within a millimetre, not self-intersecting.
fn check_boundary(points: &[V3]) -> Result<Frame> {
    if points.len() < 3 {
        return Err(Error::new(
            "analytical.degenerate_panel",
            "a boundary needs at least three points",
        ));
    }
    let f = Frame::of(points).ok_or_else(|| {
        Error::new(
            "analytical.degenerate_panel",
            "the boundary's points are in line",
        )
    })?;
    if let Some((k, p)) = points.iter().enumerate().find(|(_, p)| f.off(**p).abs() > 1e-3) {
        return Err(Error::new(
            "analytical.not_planar",
            format!(
                "the boundary is not planar: point {k} is {:.0} mm off its plane",
                f.off(*p).abs() * 1000.0
            ),
        ));
    }
    let ring: Vec<[f64; 2]> = points.iter().map(|p| f.to2(*p)).collect();
    let n = ring.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crosses(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]) {
                return Err(Error::new(
                    "analytical.self_intersecting",
                    format!("the boundary crosses itself: segments {i} and {j}"),
                ));
            }
        }
    }
    Ok(f)
}

fn crosses(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    d1 * d2 < -1e-12 && d3 * d4 < -1e-12
}

fn default_thickness() -> f64 {
    0.2
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryParams {
    /// The outer boundary's corners (metres), planar.
    pub boundary: Vec<V3>,
    /// Metres (default 200 mm).
    #[serde(default = "default_thickness")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: f64,
    pub material: Option<String>,
    #[serde(default)]
    pub role: PanelRole,
    #[serde(default)]
    pub analyze_as: PanelAnalyzeAs,
}

fn recipe(
    thickness: f64,
    material: Option<String>,
    role: PanelRole,
    analyze_as: PanelAnalyzeAs,
) -> PanelRecipe {
    PanelRecipe {
        kind: PANEL_RECIPE.into(),
        corners: Vec::new(),
        arc: None,
        thickness,
        material,
        role,
        analyze_as,
        cladding: false,
        openings: Vec::new(),
        physical: Vec::new(),
    }
}

/// Panel by Boundary (`analytical_panel.create_by_boundary`).
pub(crate) fn create_by_boundary(
    txn: &mut crate::txn::Transaction<'_>,
    p: BoundaryParams,
) -> Result<ElementId> {
    check_boundary(&p.boundary)?;
    new_panel(
        txn,
        &p.boundary,
        recipe(p.thickness, p.material, p.role, p.analyze_as),
    )
}

/// A profile in plan for Panel by Extrusion.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    /// An arc from `start` through `through` to `end`.
    Arc {
        start: [f64; 2],
        through: [f64; 2],
        end: [f64; 2],
    },
}

fn default_depth() -> f64 {
    3.0
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrusionParams {
    /// The profile, drawn on a horizontal work plane at `elevation`.
    pub profile: Profile,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub elevation: f64,
    /// Extruded upwards (metres, default 3 m).
    #[serde(default = "default_depth")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: f64,
    #[serde(default = "default_thickness")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: f64,
    pub material: Option<String>,
    /// Default wall.
    pub role: Option<PanelRole>,
    #[serde(default)]
    pub analyze_as: PanelAnalyzeAs,
}

/// Panel by Extrusion (`analytical_panel.create_by_extrusion`): a straight or curved profile on a
/// horizontal work plane, extruded upwards; a curved one makes a panel on a cylinder.
pub(crate) fn create_by_extrusion(
    txn: &mut crate::txn::Transaction<'_>,
    p: ExtrusionParams,
) -> Result<ElementId> {
    if p.depth.abs() < 1e-3 {
        return Err(Error::new(
            "analytical.degenerate_panel",
            "the extrusion depth must not be zero",
        ));
    }
    let (z0, z1) = (p.elevation, p.elevation + p.depth);
    let mut r = recipe(
        p.thickness,
        p.material,
        p.role.unwrap_or(PanelRole::Wall),
        p.analyze_as,
    );
    let (a, b) = match p.profile {
        Profile::Line { start, end } => (start, end),
        Profile::Arc {
            start,
            through,
            end,
        } => {
            if crate::walls::curved::circle(start, through, end).is_none() {
                return Err(Error::new(
                    "analytical.degenerate_panel",
                    "the arc's three points are in line: draw a line instead",
                ));
            }
            r.arc = Some(PanelArc { through });
            (start, end)
        }
    };
    if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3 {
        return Err(Error::new(
            "analytical.degenerate_panel",
            "the profile's ends are in the same place",
        ));
    }
    new_panel(
        txn,
        &[[a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], z1], [a[0], a[1], z1]],
        r,
    )
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, PanelGeometry)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let g = store.get(id).and_then(|e| geometry(store, e)).ok_or_else(|| {
        Error::new(
            "analytical.not_a_panel",
            format!("element {id} is not an analytical panel"),
        )
    })?;
    Ok((id, g))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpeningParams {
    pub panel_id: ElementRef,
    /// The opening's boundary (metres), on the panel.
    pub boundary: Vec<V3>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OpeningOut {
    pub panel_id: ElementId,
    /// Its index among the panel's openings.
    pub opening: usize,
}

/// An opening in a planar panel (`analytical_opening.create`): its points are put on the
/// panel's plane, and must lie inside the panel and outside its other openings.
pub(crate) fn create_opening(
    txn: &mut crate::txn::Transaction<'_>,
    p: OpeningParams,
) -> Result<OpeningOut> {
    let store = txn.store().clone();
    let (id, g) = resolve(&store, &p.panel_id)?;
    if g.circle.is_some() {
        return Err(Error::new(
            "analytical.curved_opening",
            "openings in curved panels are not supported yet",
        ));
    }
    let f = Frame::of(&g.corners).ok_or_else(|| {
        Error::new("analytical.degenerate_panel", "the panel is degenerate")
    })?;
    if p.boundary.len() < 3 {
        return Err(Error::new(
            "analytical.degenerate_opening",
            "an opening needs at least three points",
        ));
    }
    let tol = crate::structural::settings(&store).support_distance;
    if let Some(q) = p.boundary.iter().find(|q| f.off(**q).abs() > tol) {
        return Err(Error::new(
            "analytical.opening_off_panel",
            format!(
                "an opening point is {:.0} mm off the panel",
                f.off(*q).abs() * 1000.0
            ),
        ));
    }
    let ring: Vec<[f64; 2]> = g.corners.iter().map(|q| f.to2(*q)).collect();
    let pts: Vec<[f64; 2]> = p.boundary.iter().map(|q| f.to2(*q)).collect();
    if let Some(k) = pts.iter().position(|q| !inside(*q, &ring)) {
        return Err(Error::new(
            "analytical.opening_outside",
            format!("opening point {k} lies outside the panel"),
        ));
    }
    if signed_area(&pts).abs() < 1e-6 {
        return Err(Error::new(
            "analytical.degenerate_opening",
            "the opening has no area",
        ));
    }
    for (k, o) in g.recipe.openings.iter().enumerate() {
        let other: Vec<[f64; 2]> = o.iter().map(|q| f.to2(*q)).collect();
        if pts.iter().any(|q| inside(*q, &other)) || other.iter().any(|q| inside(*q, &pts)) {
            return Err(Error::new(
                "analytical.openings_overlap",
                format!("the opening overlaps opening {k}"),
            ));
        }
    }
    let mut r = g.recipe;
    r.openings.push(pts.iter().map(|q| f.to3(*q)).collect());
    write(txn, id, &r)?;
    Ok(OpeningOut {
        panel_id: id,
        opening: r.openings.len() - 1,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveOpeningParams {
    pub panel_id: ElementRef,
    pub opening: usize,
}

/// Removes an opening (`analytical_opening.remove`).
pub(crate) fn remove_opening(
    txn: &mut crate::txn::Transaction<'_>,
    p: RemoveOpeningParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, g) = resolve(&store, &p.panel_id)?;
    let mut r = g.recipe;
    if p.opening >= r.openings.len() {
        return Err(Error::new(
            "analytical.no_opening",
            format!("the panel has {} opening(s)", r.openings.len()),
        ));
    }
    r.openings.remove(p.opening);
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CladdingParams {
    pub panel_id: ElementRef,
    pub cladding: bool,
}

/// Marks a panel as cladding, or not (`analytical_panel.set_cladding`).
pub(crate) fn set_cladding(
    txn: &mut crate::txn::Transaction<'_>,
    p: CladdingParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, g) = resolve(&store, &p.panel_id)?;
    let mut r = g.recipe;
    r.cladding = p.cladding;
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub panel_id: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
    pub material: Option<String>,
    pub role: Option<PanelRole>,
    pub analyze_as: Option<PanelAnalyzeAs>,
}

/// Changes a panel's properties (`analytical_panel.set`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, g) = resolve(&store, &p.panel_id)?;
    let mut r = g.recipe;
    if let Some(t) = p.thickness {
        check_thickness(t)?;
        r.thickness = t;
    }
    if let Some(m) = p.material {
        r.material = (!m.trim().is_empty()).then_some(m);
    }
    if let Some(x) = p.role {
        r.role = x;
    }
    if let Some(x) = p.analyze_as {
        r.analyze_as = x;
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitParams {
    pub panel_id: ElementRef,
    /// Two points (metres) of the split line, on or near the panel's plane.
    pub line: [V3; 2],
}

/// Splits a planar panel along a line (`analytical_panel.split`): two panels, sharing new nodes
/// where the line crosses the boundary; each keeps the properties, association and the openings
/// on its side. Returns both (the first is the original panel).
pub(crate) fn split(
    txn: &mut crate::txn::Transaction<'_>,
    p: SplitParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, g) = resolve(&store, &p.panel_id)?;
    if g.circle.is_some() {
        return Err(Error::new(
            "analytical.curved_split",
            "curved panels cannot be split yet",
        ));
    }
    let f = Frame::of(&g.corners)
        .ok_or_else(|| Error::new("analytical.degenerate_panel", "the panel is degenerate"))?;
    let (a, b) = (f.to2(p.line[0]), f.to2(p.line[1]));
    let d = [b[0] - a[0], b[1] - a[1]];
    if d[0].hypot(d[1]) < 1e-6 {
        return Err(Error::new(
            "analytical.bad_split",
            "the split line's points are in the same place (seen on the panel)",
        ));
    }
    let side = |q: [f64; 2]| d[0] * (q[1] - a[1]) - d[1] * (q[0] - a[0]);
    let ring: Vec<[f64; 2]> = g.corners.iter().map(|q| f.to2(*q)).collect();
    let n = ring.len();
    // Walk the boundary, putting each corner on its side and the crossing points on both.
    let (mut left, mut right): (Vec<[f64; 2]>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
    let mut crossings = 0;
    for i in 0..n {
        let (q, r) = (ring[i], ring[(i + 1) % n]);
        let (sq, sr) = (side(q), side(r));
        let eps = 1e-9;
        if sq >= -eps {
            left.push(q);
        }
        if sq <= eps {
            right.push(q);
        }
        if (sq > eps && sr < -eps) || (sq < -eps && sr > eps) {
            let t = sq / (sq - sr);
            let x = [q[0] + (r[0] - q[0]) * t, q[1] + (r[1] - q[1]) * t];
            left.push(x);
            right.push(x);
            crossings += 1;
        } else if sq.abs() <= eps {
            crossings += 1;
        }
    }
    let area = signed_area(&ring).abs();
    let (al, ar) = (signed_area(&left).abs(), signed_area(&right).abs());
    if left.len() < 3 || right.len() < 3 || al < 1e-6 || ar < 1e-6 {
        return Err(Error::new(
            "analytical.bad_split",
            "the line does not cross the panel",
        ));
    }
    if crossings != 2 || (al + ar - area).abs() > 1e-6 * area.max(1.0) {
        return Err(Error::new(
            "analytical.bad_split",
            "the line crosses the panel more than once (a non-convex panel): split it in parts",
        ));
    }
    // Openings go to the side of their centre; one the line crosses is refused.
    let mut keep_l = Vec::new();
    let mut keep_r = Vec::new();
    for (k, o) in g.recipe.openings.iter().enumerate() {
        let pts: Vec<[f64; 2]> = o.iter().map(|q| f.to2(*q)).collect();
        let sides: Vec<f64> = pts.iter().map(|q| side(*q)).collect();
        if sides.iter().any(|s| *s > 1e-9) && sides.iter().any(|s| *s < -1e-9) {
            return Err(Error::new(
                "analytical.bad_split",
                format!("the line crosses opening {k}: remove it or split elsewhere"),
            ));
        }
        if sides.iter().all(|s| *s >= -1e-9) {
            keep_l.push(o.clone());
        } else {
            keep_r.push(o.clone());
        }
    }
    let tol = analytical::tolerance(&store);
    let corners = |txn: &mut crate::txn::Transaction<'_>,
                       ring: &[[f64; 2]]|
     -> Result<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for q in ring {
            let g = analytical::node_at(txn, f.to3(*q), tol)?;
            if out.last() != Some(&g) {
                out.push(g);
            }
        }
        Ok(out)
    };
    let mut r = g.recipe.clone();
    r.corners = corners(txn, &left)?;
    r.openings = keep_l;
    write(txn, id, &r)?;
    let mut r2 = g.recipe;
    r2.corners = corners(txn, &right)?;
    r2.openings = keep_r;
    let other = txn.create(
        NewElement::new("IfcStructuralSurfaceMember")
            .param(RECIPE_KEY, serde_json::to_value(&r2).expect("serialises")),
    )?;
    write(txn, other, &r2)?;
    Ok(vec![id, other])
}

// ---- From physical walls and floors (STR-AM-006) ----

/// The panel for a structural wall or floor: its corner points and recipe, or why there is none.
pub(crate) fn physical_panel(
    store: &Store,
    e: &Element,
) -> std::result::Result<(Vec<V3>, PanelRecipe), String> {
    if crate::walls::wall_recipe(e).is_some() {
        if crate::structural::wall_usage(e) == crate::structural::WallUsage::NonBearing {
            return Err("a non-bearing wall carries no load".into());
        }
        let w = crate::walls::regenerate(store, e).map_err(|x| x.message)?;
        let r = crate::walls::wall_recipe(e).expect("checked");
        let (z0, z1) = (w.base_z, w.base_z + w.height);
        let (a, b) = (w.start, w.end);
        let mut rec = recipe(w.width, wall_material(store, e), PanelRole::Wall, PanelAnalyzeAs::Shell);
        rec.physical = vec![e.guid().to_string()];
        if let [m] = r.through.as_slice()
            && crate::walls::curved::circle(a, *m, b).is_some()
        {
            rec.arc = Some(PanelArc { through: *m });
        } else if !r.through.is_empty() {
            return Err("spline walls have no analytical panel yet".into());
        }
        return Ok((
            vec![[a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], z1], [a[0], a[1], z1]],
            rec,
        ));
    }
    if crate::floors::slab_recipe(e).is_some() {
        if !crate::structural::is_structural_floor(e) {
            return Err("a non-structural floor carries no load".into());
        }
        let g = crate::floors::regenerate(store, e).map_err(|x| x.message)?;
        let [profile] = g.profiles.as_slice() else {
            return Err("a floor of several separate parts: split it first".into());
        };
        let pts: Vec<V3> = profile
            .outer
            .iter()
            .map(|q| [q[0], q[1], g.top])
            .collect();
        let mut rec = recipe(g.thickness, floor_material(store, e), PanelRole::Floor, PanelAnalyzeAs::Shell);
        rec.physical = vec![e.guid().to_string()];
        rec.openings = profile
            .holes
            .iter()
            .map(|h| h.iter().map(|q| [q[0], q[1], g.top]).collect())
            .collect();
        return Ok((pts, rec));
    }
    Err("not a wall or floor".into())
}

/// The structural layer's material of a wall's or floor's type (the layer marked structural, else
/// the first Structure layer); the region's concrete when the type has none.
fn type_material(store: &Store, e: &Element) -> Option<String> {
    let t = e
        .param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t));
    let from_layers = t.and_then(crate::compound::of_type).and_then(|c| {
        c.layers
            .iter()
            .find(|l| l.structural_material)
            .or_else(|| {
                c.layers
                    .iter()
                    .find(|l| l.function == crate::compound::Function::Structure)
            })
            .map(crate::compound::layer_material)
            .filter(|m| !m.is_empty())
    });
    from_layers.or_else(|| {
        Some(
            crate::codes::settings(store)
                .code
                .structural()
                .concrete
                .into(),
        )
    })
}

fn wall_material(store: &Store, e: &Element) -> Option<String> {
    type_material(store, e)
}

fn floor_material(store: &Store, e: &Element) -> Option<String> {
    type_material(store, e)
}

/// The panel a physical element is already associated with, if any.
pub fn associated_with(store: &Store, guid: &str) -> Option<ElementId> {
    store
        .iter()
        .find(|e| panel_recipe(e).is_some_and(|r| r.physical.iter().any(|p| p == guid)))
        .map(Element::id)
}

/// Makes the panel for a physical wall or floor (automation).
pub(crate) fn from_physical(
    txn: &mut crate::txn::Transaction<'_>,
    e: &Element,
) -> std::result::Result<ElementId, String> {
    let store = txn.store().clone();
    let (pts, r) = physical_panel(&store, e)?;
    new_panel(txn, &pts, r).map_err(|x| x.message)
}
