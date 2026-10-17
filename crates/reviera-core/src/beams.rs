// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Beams (STR-BEAM-001) and their justification, extensions, cutback and joins (STR-BEAM-002).
//!
//! A beam is an `IfcBeam` (or an `IfcMember`, for purlins and horizontal bracing) with a
//! `recipe.beam`: its location line in plan, from `start` to `end`, at its reference level plus
//! an offset at each end (so it can slope). Its type (`IfcBeamType` with a `recipe.beam_type`)
//! gives its section, as a column type does ([`ColumnType`]): rolled, hollow or cast, with its
//! material and designation.
//!
//! The section is swept along the location line. *z justification* puts the line at the
//! section's top (the default: beams hang from their level), centre, bottom or origin, and
//! *y justification* at its left, centre, right or origin, each with an offset; the section can be
//! turned about the axis (*cross-section rotation*). Its orientation is *normal* (square to the
//! axis) or *horizontal* (upright, cut plumb, for sloped members).
//!
//! Each end is joined to what supports it, found from the model: a column or wall the end lies
//! in, or a beam the end frames into from the side. A joined end stops at the support's face,
//! less a cutback (12.5 mm by default for steel members; none for others); *disjoin* releases an
//! end, and extensions lengthen either end beyond that. The supports also give a beam whose usage
//! is automatic its usage: on columns or walls a girder; on girders a joist; on joists a purlin.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::columns::{ColumnType, MaterialKind};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::{Line, RECIPE_KEY};

pub const BEAM_RECIPE: &str = "recipe.beam";
pub const BEAM_TYPE_RECIPE: &str = "recipe.beam_type";
/// The default steel cutback at a joined end (metres).
pub const STEEL_CUTBACK: f64 = 0.0125;
/// How far an end may lie outside a support and still be on it (metres).
const TOLERANCE: f64 = 0.01;

/// A beam's structural usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Usage {
    /// From its supports (only in a recipe; a beam's resolved usage is never automatic).
    #[default]
    Automatic,
    Girder,
    Joist,
    Purlin,
    HorizontalBracing,
    Other,
}

impl Usage {
    pub fn name(self) -> &'static str {
        match self {
            Usage::Automatic => "Automatic",
            Usage::Girder => "Girder",
            Usage::Joist => "Joist",
            Usage::Purlin => "Purlin",
            Usage::HorizontalBracing => "Horizontal Bracing",
            Usage::Other => "Other",
        }
    }

    /// The IFC class a new member of this usage is: purlins and bracing are members.
    pub fn class(self) -> &'static str {
        match self {
            Usage::Purlin | Usage::HorizontalBracing => "IfcMember",
            _ => "IfcBeam",
        }
    }

    /// Its PredefinedType and ObjectType as `class`.
    pub fn ifc(self, class: &str) -> (&'static str, Option<&'static str>) {
        let member = class == "IfcMember";
        match (self, member) {
            (Usage::Purlin, true) => ("PURLIN", None),
            (Usage::HorizontalBracing, true) => ("BRACE", None),
            (Usage::Girder, false) => ("BEAM", Some("GIRDER")),
            (Usage::Joist, false) => ("JOIST", None),
            (Usage::Other | Usage::Automatic, false) => ("BEAM", None),
            (Usage::Other | Usage::Automatic, true) => ("MEMBER", None),
            (u, _) => (
                "USERDEFINED",
                Some(match u {
                    Usage::Girder => "GIRDER",
                    Usage::Joist => "JOIST",
                    Usage::Purlin => "PURLIN",
                    _ => "HORIZONTAL BRACING",
                }),
            ),
        }
    }
}

/// Where the location line runs across the section (its local x, positive to the left).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum YJustification {
    Origin,
    /// Along the section's left face: the beam lies to the right of the line.
    Left,
    #[default]
    Centre,
    Right,
}

/// Where the location line runs up the section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZJustification {
    Origin,
    /// Along its top: the beam hangs below the line.
    #[default]
    Top,
    Centre,
    Bottom,
}

/// How the section sits on a sloped beam.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Square to the beam's axis.
    #[default]
    Normal,
    /// Upright, its ends cut plumb.
    Horizontal,
}

fn yes() -> [bool; 2] {
    [true, true]
}

/// The stored recipe of a beam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BeamRecipe {
    pub kind: String,
    /// The location line in plan (metres).
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// The reference level (GlobalId).
    pub level: String,
    /// Each end's height above the level (metres).
    #[serde(default)]
    pub start_offset: f64,
    #[serde(default)]
    pub end_offset: f64,
    /// The section turned about the axis (radians).
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub y_justification: YJustification,
    /// Positive to the left (metres).
    #[serde(default)]
    pub y_offset: f64,
    #[serde(default)]
    pub z_justification: ZJustification,
    /// Positive up (metres).
    #[serde(default)]
    pub z_offset: f64,
    #[serde(default)]
    pub orientation: Orientation,
    /// Beyond each end (after any join; metres).
    #[serde(default)]
    pub start_extension: f64,
    #[serde(default)]
    pub end_extension: f64,
    /// The gap left at each joined end (metres); default [`STEEL_CUTBACK`] for steel, else 0.
    #[serde(default, skip_serializing_if = "is_unset")]
    pub cutback: [Option<f64>; 2],
    /// Each end joins its support (stops at its face); `false` once disjoined.
    #[serde(default = "yes")]
    pub joined: [bool; 2],
    /// Set by the user (locked), or automatic.
    #[serde(default)]
    pub usage: Usage,
}

fn is_unset(c: &[Option<f64>; 2]) -> bool {
    c.iter().all(Option::is_none)
}

pub fn beam_recipe(e: &Element) -> Option<BeamRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == BEAM_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A beam type's section (a column type's recipe under its own kind).
pub fn beam_type(e: &Element) -> Option<ColumnType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == BEAM_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn type_of(store: &Store, e: &Element) -> ColumnType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(beam_type)
        .unwrap_or_else(|| ColumnType {
            kind: BEAM_TYPE_RECIPE.into(),
            ..ColumnType::default()
        })
}

// ---- Geometry ----

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// A beam's frame and section, before its ends are joined.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    id: ElementId,
    ty: ColumnType,
    recipe: BeamRecipe,
    /// The location line's ends.
    a: V3,
    b: V3,
    /// Along the axis, to the left (horizontal), and up the section.
    u: V3,
    left: V3,
    up: V3,
    /// The section, turned and justified: its outline and holes in (left, up) coordinates.
    outline: Vec<[f64; 2]>,
    holes: Vec<Vec<[f64; 2]>>,
    /// Its extent across (left) and up.
    x: [f64; 2],
    y: [f64; 2],
}

impl Frame {
    fn new(store: &Store, e: &Element) -> Result<Frame> {
        let r =
            beam_recipe(e).ok_or_else(|| Error::new("beam.not_reviera", "not a Reviera beam"))?;
        let ty = type_of(store, e);
        let z = crate::levels::elevation_of(store, &r.level)?;
        let a = [r.start[0], r.start[1], z + r.start_offset];
        let b = [r.end[0], r.end[1], z + r.end_offset];
        let d = sub(b, a);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let plan = d[0].hypot(d[1]);
        if plan < 1e-3 || len < 1e-3 {
            return Err(Error::new(
                "beam.too_short",
                "a beam needs two ends apart in plan (use a column for vertical members)",
            ));
        }
        let u = scale(d, 1.0 / len);
        let left = [-d[1] / plan, d[0] / plan, 0.0];
        let up = match r.orientation {
            Orientation::Normal => cross(u, left),
            Orientation::Horizontal => [0.0, 0.0, 1.0],
        };
        let (s, c) = r.rotation.sin_cos();
        let (outline, holes) = ty.profile();
        let turn = |p: &[f64; 2]| [p[0] * c - p[1] * s, p[0] * s + p[1] * c];
        let outline: Vec<[f64; 2]> = outline.iter().map(turn).collect();
        let holes: Vec<Vec<[f64; 2]>> =
            holes.iter().map(|h| h.iter().map(turn).collect()).collect();
        let lo = |k: usize| outline.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
        let hi = |k: usize| {
            outline
                .iter()
                .map(|p| p[k])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let dx = r.y_offset
            - match r.y_justification {
                YJustification::Origin => 0.0,
                YJustification::Left => hi(0),
                YJustification::Centre => (lo(0) + hi(0)) / 2.0,
                YJustification::Right => lo(0),
            };
        let dy = r.z_offset
            - match r.z_justification {
                ZJustification::Origin => 0.0,
                ZJustification::Top => hi(1),
                ZJustification::Centre => (lo(1) + hi(1)) / 2.0,
                ZJustification::Bottom => lo(1),
            };
        let shift = |p: &[f64; 2]| [p[0] + dx, p[1] + dy];
        let x = [lo(0) + dx, hi(0) + dx];
        let y = [lo(1) + dy, hi(1) + dy];
        Ok(Frame {
            id: e.id(),
            outline: outline.iter().map(shift).collect(),
            holes: holes
                .iter()
                .map(|h| h.iter().map(shift).collect())
                .collect(),
            ty,
            recipe: r,
            a,
            b,
            u,
            left,
            up,
            x,
            y,
        })
    }

    fn plan_dir(&self) -> [f64; 2] {
        [self.left[1], -self.left[0]]
    }

    /// The location line's height over a plan point on it (by its plan parameter).
    fn z_at(&self, t: f64) -> f64 {
        self.a[2] + (self.b[2] - self.a[2]) * t
    }

    fn plan_length(&self) -> f64 {
        (self.b[0] - self.a[0]).hypot(self.b[1] - self.a[1])
    }

    fn depth(&self) -> f64 {
        self.y[1] - self.y[0]
    }
}

/// What an end of a beam rests on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Support {
    Column(ElementId),
    Wall(ElementId),
    Beam(ElementId),
}

/// The model's supports, gathered once.
struct Context {
    /// Columns: id, plan ring, base and top.
    columns: Vec<(ElementId, Vec<[f64; 2]>, f64, f64)>,
    /// Walls: id, footprint, base and top.
    walls: Vec<(ElementId, Vec<[f64; 2]>, f64, f64)>,
    beams: Vec<Frame>,
}

impl Context {
    fn new(store: &Store) -> Context {
        let columns = crate::columns::all(store)
            .into_iter()
            .map(|(id, g)| {
                let top = g.top();
                (id, g.ring, g.z0, top)
            })
            .collect();
        let walls = store
            .iter()
            .filter(|e| crate::walls::wall_recipe(e).is_some())
            .filter_map(|e| {
                let g = crate::walls::regenerate(store, e).ok()?;
                Some((
                    e.id(),
                    g.footprint().to_vec(),
                    g.base_z,
                    g.base_z + g.height,
                ))
            })
            .collect();
        let beams = store
            .iter()
            .filter(|e| beam_recipe(e).is_some())
            .filter_map(|e| Frame::new(store, e).ok())
            .collect();
        Context {
            columns,
            walls,
            beams,
        }
    }

    /// What end `k` (0 start, 1 end) of a beam rests on, and how far (in plan, along the beam
    /// from that end inwards) its face is.
    fn support(&self, f: &Frame, k: usize) -> Option<(Support, f64)> {
        let p = if k == 0 { f.a } else { f.b };
        let p2 = [p[0], p[1]];
        let dir = f.plan_dir();
        let inward = if k == 0 { dir } else { [-dir[0], -dir[1]] };
        let tol = TOLERANCE.max(f.depth());
        for (id, ring, z0, z1) in &self.columns {
            if p[2] >= z0 - tol && p[2] <= z1 + tol && inside(p2, ring) {
                return Some((Support::Column(*id), exit(p2, inward, ring)));
            }
        }
        for (id, ring, z0, z1) in &self.walls {
            if p[2] >= z0 - tol && p[2] <= z1 + tol && inside(p2, ring) {
                return Some((Support::Wall(*id), exit(p2, inward, ring)));
            }
        }
        // Framing into another beam's side.
        let mut best: Option<(Support, f64, f64)> = None;
        for g in &self.beams {
            if g.id == f.id {
                continue;
            }
            let (ga, gd, gl) = ([g.a[0], g.a[1]], g.plan_dir(), g.plan_length());
            let rel = [p2[0] - ga[0], p2[1] - ga[1]];
            let t = dot2(rel, gd);
            if t <= TOLERANCE || t >= gl - TOLERANCE {
                continue;
            }
            // Across g (positive to its left).
            let gl2 = [g.left[0], g.left[1]];
            let h = dot2(rel, gl2);
            if h < g.x[0] - TOLERANCE || h > g.x[1] + TOLERANCE {
                continue;
            }
            let dz = (g.z_at(t / gl) - p[2]).abs();
            if dz > g.depth().max(f.depth()) {
                continue;
            }
            let rate = dot2(inward, gl2);
            if rate.abs() < 1e-6 {
                continue;
            }
            let face = if rate > 0.0 { g.x[1] } else { g.x[0] };
            let s = ((face - h) / rate).max(0.0);
            if best.is_none_or(|b| dz < b.2) {
                best = Some((Support::Beam(g.id), s, dz));
            }
        }
        best.map(|(s, d, _)| (s, d))
    }

    fn frame(&self, id: ElementId) -> Option<&Frame> {
        self.beams.iter().find(|f| f.id == id)
    }

    /// A beam's usage: its own when set, else from its supports.
    fn usage(&self, id: ElementId, seen: &mut HashSet<ElementId>) -> Usage {
        let Some(f) = self.frame(id) else {
            return Usage::Other;
        };
        if f.recipe.usage != Usage::Automatic {
            return f.recipe.usage;
        }
        if !seen.insert(id) {
            return Usage::Other;
        }
        let supports: Vec<Support> = (0..2)
            .filter_map(|k| self.support(f, k).map(|(s, _)| s))
            .collect();
        if supports
            .iter()
            .any(|s| matches!(s, Support::Column(_) | Support::Wall(_)))
        {
            return Usage::Girder;
        }
        let on: Vec<Usage> = supports
            .iter()
            .filter_map(|s| match s {
                Support::Beam(b) => Some(self.usage(*b, seen)),
                _ => None,
            })
            .collect();
        if on.contains(&Usage::Girder) {
            Usage::Joist
        } else if on.contains(&Usage::Joist) {
            Usage::Purlin
        } else {
            Usage::Other
        }
    }

    fn geometry(&self, f: &Frame) -> BeamGeometry {
        let steel = f.ty.material_kind == Some(MaterialKind::Steel);
        let along = f.plan_length() / dist(f.a, f.b);
        let mut supports = [None, None];
        let mut trims = [0.0; 2];
        for k in 0..2 {
            if let Some((s, face)) = self.support(f, k) {
                supports[k] = Some(s);
                if f.recipe.joined[k] {
                    let cut =
                        f.recipe.cutback[k].unwrap_or(if steel { STEEL_CUTBACK } else { 0.0 });
                    trims[k] = (face + cut) / along.max(1e-9);
                }
            }
        }
        let len = dist(f.a, f.b);
        let t0 = trims[0] - f.recipe.start_extension;
        let t1 = len - trims[1] + f.recipe.end_extension;
        let usage = self.usage(f.id, &mut HashSet::new());
        BeamGeometry {
            ty: f.ty.clone(),
            usage,
            a: f.a,
            b: f.b,
            start: add(f.a, scale(f.u, t0)),
            end: add(f.a, scale(f.u, t1)),
            u: f.u,
            left: f.left,
            up: f.up,
            outline: f.outline.clone(),
            holes: f.holes.clone(),
            x: f.x,
            y: f.y,
            supports,
        }
    }
}

fn dist(a: V3, b: V3) -> f64 {
    let d = sub(b, a);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c || ring_distance(p, ring) < TOLERANCE
}

fn ring_distance(p: [f64; 2], ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let l2 = dot2(ab, ab).max(1e-18);
            let t = (dot2([p[0] - a[0], p[1] - a[1]], ab) / l2).clamp(0.0, 1.0);
            (p[0] - a[0] - ab[0] * t).hypot(p[1] - a[1] - ab[1] * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// How far from `p` along `d` the ray leaves the ring (0 when it does not cross it).
fn exit(p: [f64; 2], d: [f64; 2], ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    let mut best: f64 = 0.0;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let den = d[0] * e[1] - d[1] * e[0];
        if den.abs() < 1e-12 {
            continue;
        }
        let ap = [a[0] - p[0], a[1] - p[1]];
        let s = (ap[0] * e[1] - ap[1] * e[0]) / den;
        let t = (ap[0] * d[1] - ap[1] * d[0]) / den;
        if (-1e-9..=1.0 + 1e-9).contains(&t) && s > best {
            best = s;
        }
    }
    best
}

/// A beam as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct BeamGeometry {
    pub ty: ColumnType,
    /// Its usage, resolved from its supports when automatic.
    pub usage: Usage,
    /// The location line's ends.
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// The body's ends on the location line, after joins and extensions.
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// Along the axis, to the left and up the section.
    pub u: [f64; 3],
    pub left: [f64; 3],
    pub up: [f64; 3],
    /// The justified section in (left, up) coordinates, and its extents.
    pub outline: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
    pub x: [f64; 2],
    pub y: [f64; 2],
    /// What each end rests on.
    pub supports: [Option<Support>; 2],
}

impl BeamGeometry {
    /// The body's length along its axis (the cut length).
    pub fn length(&self) -> f64 {
        dist(self.start, self.end)
    }

    /// The closed body: the section swept from `start` to `end`.
    pub fn mesh(&self) -> crate::bodies::Mesh {
        let len = self.length();
        let (points, indices) = crate::bodies::extrude(&self.outline, &self.holes, 0.0, len);
        let points = points
            .into_iter()
            .map(|[x, y, t]| {
                add(
                    add(self.start, scale(self.u, t)),
                    add(scale(self.left, x), scale(self.up, y)),
                )
            })
            .collect();
        (points, indices)
    }

    /// The body's outline in plan (its four extreme corners).
    pub fn plan_outline(&self) -> Vec<[f64; 2]> {
        let at = |p: V3, x: f64| [p[0] + self.left[0] * x, p[1] + self.left[1] * x];
        vec![
            at(self.start, self.x[0]),
            at(self.end, self.x[0]),
            at(self.end, self.x[1]),
            at(self.start, self.x[1]),
        ]
    }
}

/// Regenerates a beam from its recipe, type, level and supports.
pub fn regenerate(store: &Store, e: &Element) -> Result<BeamGeometry> {
    let f = Frame::new(store, e)?;
    let cx = Context::new(store);
    Ok(cx.geometry(&f))
}

/// Every Reviera beam that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, BeamGeometry)> {
    if !store.iter().any(|e| beam_recipe(e).is_some()) {
        return Vec::new();
    }
    let cx = Context::new(store);
    cx.beams.iter().map(|f| (f.id, cx.geometry(f))).collect()
}

/// Brings each beam's IFC attributes up to date with its usage and length: PredefinedType,
/// ObjectType and `Pset_BeamCommon.Span` (or `Pset_MemberCommon.Span`).
fn refresh(txn: &mut crate::txn::Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    for (id, g) in all(&store) {
        let e = store.get(id).expect("listed");
        let (predefined, object) = g.usage.ifc(&e.class);
        let pset = if e.class == "IfcMember" {
            "Pset_MemberCommon"
        } else {
            "Pset_BeamCommon"
        };
        let span = json!({ "type": "IfcPositiveLengthMeasure", "value": (g.length() * 1e6).round() / 1e6 });
        let span_key = format!("{pset}.Span");
        let same = e.param("@PredefinedType").and_then(Value::as_str) == Some(predefined)
            && e.param("@ObjectType").and_then(Value::as_str) == object
            && e.param(&span_key) == Some(&span);
        if same {
            continue;
        }
        txn.modify(id, |e| {
            e.params.insert("@PredefinedType".into(), predefined.into());
            match object {
                Some(o) => {
                    e.params.insert("@ObjectType".into(), o.into());
                }
                None => {
                    e.params.remove("@ObjectType");
                }
            }
            e.params.insert(span_key, span);
        })?;
    }
    Ok(())
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &BeamRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    Frame::new(&store, store.get(id).expect("exists")).map(|_| ())
}

/// The project's default beam type: the region's usual rolled beam (W12X26, IPE 300 or
/// UB 305x165x40), made on first use.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let (catalog, designation) = crate::codes::settings(txn.store()).code.structural().beam;
    load_section(
        txn,
        crate::steel_sections::LoadParams {
            catalog: catalog.into(),
            designation: designation.into(),
            material: None,
        },
    )
}

/// Makes (or finds) a beam type of a catalogue section (`beam.load_section`).
pub(crate) fn load_section(
    txn: &mut crate::txn::Transaction<'_>,
    p: crate::steel_sections::LoadParams,
) -> Result<ElementId> {
    let c = crate::steel_sections::catalog(&p.catalog)
        .ok_or_else(|| Error::new("section.no_catalog", format!("no catalogue {}", p.catalog)))?;
    let mut t = crate::steel_sections::column_type(&c, &p.designation)?;
    t.kind = BEAM_TYPE_RECIPE.into();
    if let Some(m) = p.material {
        t.material = Some(m);
    }
    if let Some(e) = txn.store().iter().find(|e| {
        beam_type(e).is_some_and(|x| x.designation == t.designation && x.material == t.material)
    }) {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcBeamType")
            .name(&p.designation)
            .param("@PredefinedType", "BEAM")
            .param(RECIPE_KEY, serde_json::to_value(t).expect("serialises")),
    )
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such beam type"))?;
    if beam_type(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "beam.not_a_type",
            format!("element {id} is not a beam type"),
        ));
    }
    Ok(id)
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<String> {
    store
        .resolve(r)
        .and_then(|id| store.get(id))
        .filter(|e| e.class == crate::levels::CLASS)
        .map(|e| e.guid().to_string())
        .ok_or_else(|| Error::new("level.not_found", format!("no level {r:?}")))
}

fn resolve_beams(store: &Store, refs: &[ElementRef]) -> Result<Vec<(ElementId, BeamRecipe)>> {
    refs.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let recipe = store.get(id).and_then(beam_recipe).ok_or_else(|| {
                Error::new(
                    "beam.not_reviera",
                    format!("element {id} is not a Reviera beam"),
                )
            })?;
            Ok((id, recipe))
        })
        .collect()
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A beam type; omitted, the region's usual rolled beam, made on first use.
    pub type_id: Option<ElementRef>,
    /// The location line in plan (metres).
    pub curve: Line,
    pub reference_level: ElementRef,
    /// Each end's height above the level.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start_offset: f64,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_offset: f64,
    /// Default automatic (from its supports).
    #[serde(default)]
    pub usage: Usage,
    #[serde(default)]
    pub orientation: Orientation,
    pub y_justification: Option<YJustification>,
    pub z_justification: Option<ZJustification>,
    /// Cross-section rotation; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// Places a beam (`beam.create`, STR-BEAM-001).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let id = create_one(txn, p)?;
    refresh(txn)?;
    Ok(id)
}

fn create_one(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let recipe = BeamRecipe {
        kind: BEAM_RECIPE.into(),
        start: p.curve.start,
        end: p.curve.end,
        level: resolve_level(&store, &p.reference_level)?,
        start_offset: p.start_offset,
        end_offset: p.end_offset,
        rotation: p.rotation,
        y_justification: p.y_justification.unwrap_or_default(),
        y_offset: 0.0,
        z_justification: p.z_justification.unwrap_or_default(),
        z_offset: 0.0,
        orientation: p.orientation,
        start_extension: 0.0,
        end_extension: 0.0,
        cutback: [None, None],
        joined: [true, true],
        usage: p.usage,
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let value = serde_json::to_value(&recipe).expect("serialises");
    // Its class follows its usage when it is made: purlins and bracing are members.
    let class = if p.usage == Usage::Automatic {
        let probe = NewElement::new("IfcBeam")
            .param("@type", guid.clone())
            .param(RECIPE_KEY, value.clone());
        let id = txn.create(probe)?;
        let store = txn.store().clone();
        let f = Frame::new(&store, store.get(id).expect("created"))?;
        let usage = Context::new(&store).usage(f.id, &mut HashSet::new());
        if usage.class() == "IfcBeam" {
            return finish(txn, id, "IfcBeam");
        }
        txn.delete(id)?;
        usage.class()
    } else {
        p.usage.class()
    };
    let id = txn.create(
        NewElement::new(class)
            .param("@type", guid)
            .param(RECIPE_KEY, value),
    )?;
    finish(txn, id, class)
}

fn finish(txn: &mut crate::txn::Transaction<'_>, id: ElementId, class: &str) -> Result<ElementId> {
    let pset = if class == "IfcMember" {
        "Pset_MemberCommon"
    } else {
        "Pset_BeamCommon"
    };
    txn.modify(id, |e| {
        e.params.insert(
            format!("{pset}.LoadBearing"),
            json!({ "type": "IfcBoolean", "value": true }),
        );
    })?;
    let store = txn.store().clone();
    Frame::new(&store, store.get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OnGridsParams {
    pub type_id: Option<ElementRef>,
    /// Grid lines; along each, a beam spans each bay between consecutive columns.
    pub grid_ids: Vec<ElementRef>,
    /// The level the beams' tops are at (the columns must reach it).
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

/// Places a beam in each bay along the given grid lines (`beam.create_on_grids`): between
/// consecutive columns on the line that reach the level; one undo step.
pub(crate) fn create_on_grids(
    txn: &mut crate::txn::Transaction<'_>,
    p: OnGridsParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let level = resolve_level(&store, &p.level)?;
    let z = crate::levels::elevation_of(&store, &level)? + p.offset;
    let lines: Vec<_> = p
        .grid_ids
        .iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .filter(|id| store.get(*id).is_some_and(crate::grids::is_grid))
                .ok_or_else(|| Error::new("grid.not_found", format!("no grid line {r:?}")))?;
            Ok(crate::grids::grid_data(store.get(id).expect("resolved"))
                .expect("a grid")
                .curve)
        })
        .collect::<Result<_>>()?;
    let columns = crate::columns::all(&store);
    let mut out = Vec::new();
    for line in lines {
        let d = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
        let l = d[0].hypot(d[1]);
        if l < 1e-9 {
            continue;
        }
        let d = [d[0] / l, d[1] / l];
        let mut stops: Vec<(f64, [f64; 2])> = Vec::new();
        for (cid, g) in &columns {
            let r =
                crate::columns::column_recipe(store.get(*cid).expect("listed")).expect("a column");
            let rel = [r.point[0] - line.start[0], r.point[1] - line.start[1]];
            let t = dot2(rel, d);
            let off = (rel[0] * d[1] - rel[1] * d[0]).abs();
            if off < 0.05
                && (-0.05..=l + 0.05).contains(&t)
                && g.z0 < z - 0.1
                && g.top() >= z - 0.05
            {
                stops.push((t, [line.start[0] + d[0] * t, line.start[1] + d[1] * t]));
            }
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        stops.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-3);
        for w in stops.windows(2) {
            out.push(create_one(
                txn,
                CreateParams {
                    type_id: p.type_id.clone(),
                    curve: Line {
                        start: w[0].1,
                        end: w[1].1,
                    },
                    reference_level: ElementRef::Guid(
                        GlobalId::parse(&level).expect("a level's GlobalId"),
                    ),
                    start_offset: p.offset,
                    end_offset: p.offset,
                    usage: Usage::Automatic,
                    orientation: Orientation::Normal,
                    y_justification: None,
                    z_justification: None,
                    rotation: 0.0,
                },
            )?);
        }
    }
    if out.is_empty() {
        return Err(Error::new(
            "beam.no_bays",
            "no grid line has two columns on it reaching the level",
        ));
    }
    refresh(txn)?;
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetUsageParams {
    pub beam_ids: Vec<ElementRef>,
    /// `automatic` unlocks it, so it follows its supports again.
    pub usage: Usage,
}

/// Sets (and locks) beams' structural usage, or returns them to automatic (`beam.set_usage`).
pub(crate) fn set_usage(txn: &mut crate::txn::Transaction<'_>, p: SetUsageParams) -> Result<()> {
    let store = txn.store().clone();
    for (id, mut r) in resolve_beams(&store, &p.beam_ids)? {
        r.usage = p.usage;
        write(txn, id, &r)?;
    }
    refresh(txn)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub beam_ids: Vec<ElementRef>,
    pub reference_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start_offset: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_offset: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    pub orientation: Option<Orientation>,
    pub y_justification: Option<YJustification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub y_offset: Option<f64>,
    pub z_justification: Option<ZJustification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub z_offset: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start_extension: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_extension: Option<f64>,
    pub usage: Option<Usage>,
}

/// Edits beams' instance properties (`beam.set`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let level = p
        .reference_level
        .as_ref()
        .map(|r| resolve_level(&store, r))
        .transpose()?;
    for (id, mut r) in resolve_beams(&store, &p.beam_ids)? {
        if let Some(l) = &level {
            // Keep the ends where they are.
            let dz = crate::levels::elevation_of(&store, &r.level)?
                - crate::levels::elevation_of(&store, l)?;
            r.start_offset += dz;
            r.end_offset += dz;
            r.level = l.clone();
        }
        macro_rules! take {
            ($($f:ident),*) => { $(if let Some(v) = p.$f { r.$f = v; })* };
        }
        take!(
            start_offset,
            end_offset,
            rotation,
            orientation,
            y_justification,
            y_offset,
            z_justification,
            z_offset,
            start_extension,
            end_extension,
            usage
        );
        write(txn, id, &r)?;
    }
    refresh(txn)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JustificationParams {
    pub ids: Vec<ElementRef>,
    pub y_just: Option<YJustification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub y_offset: Option<f64>,
    pub z_just: Option<ZJustification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub z_offset: Option<f64>,
}

/// Sets framing justification (`framing.set_justification`, STR-BEAM-002).
pub(crate) fn set_justification(
    txn: &mut crate::txn::Transaction<'_>,
    p: JustificationParams,
) -> Result<()> {
    set(
        txn,
        SetParams {
            beam_ids: p.ids,
            y_justification: p.y_just,
            y_offset: p.y_offset,
            z_justification: p.z_just,
            z_offset: p.z_offset,
            ..SetParams::default()
        },
    )
}

/// A member's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

impl End {
    fn index(self) -> usize {
        match self {
            End::Start => 0,
            End::End => 1,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndLengthParams {
    pub id: ElementRef,
    pub end: End,
    /// Metres (or with units); negative shortens.
    #[serde(alias = "distance")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: f64,
}

fn one(store: &Store, r: &ElementRef) -> Result<(ElementId, BeamRecipe)> {
    Ok(resolve_beams(store, std::slice::from_ref(r))?.remove(0))
}

/// Sets an end's extension beyond its join (`framing.set_extension`).
pub(crate) fn set_extension(
    txn: &mut crate::txn::Transaction<'_>,
    p: EndLengthParams,
) -> Result<()> {
    let (id, mut r) = one(txn.store(), &p.id)?;
    match p.end {
        End::Start => r.start_extension = p.length,
        End::End => r.end_extension = p.length,
    }
    write(txn, id, &r)?;
    refresh(txn)
}

/// Sets the gap left at a joined end (`framing.set_cutback`).
pub(crate) fn set_cutback(txn: &mut crate::txn::Transaction<'_>, p: EndLengthParams) -> Result<()> {
    if p.length < 0.0 {
        return Err(Error::new(
            "framing.invalid_cutback",
            "a cutback cannot be negative",
        ));
    }
    let (id, mut r) = one(txn.store(), &p.id)?;
    r.cutback[p.end.index()] = Some(p.length);
    write(txn, id, &r)?;
    refresh(txn)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndParams {
    pub id: ElementRef,
    pub end: End,
}

/// Releases an end from its join (`framing.disjoin`): it runs to its location line's end, with
/// no cutback.
pub(crate) fn disjoin(txn: &mut crate::txn::Transaction<'_>, p: EndParams) -> Result<()> {
    let (id, mut r) = one(txn.store(), &p.id)?;
    r.joined[p.end.index()] = false;
    write(txn, id, &r)?;
    refresh(txn)
}

/// Joins a disjoined end again (`framing.join`).
pub(crate) fn join(txn: &mut crate::txn::Transaction<'_>, p: EndParams) -> Result<()> {
    let (id, mut r) = one(txn.store(), &p.id)?;
    r.joined[p.end.index()] = true;
    write(txn, id, &r)?;
    refresh(txn)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub beam_ids: Vec<ElementRef>,
}

/// A beam as reported (`beam.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub beam_id: ElementId,
    pub usage: Usage,
    /// Set by the user (not automatic).
    pub usage_locked: bool,
    /// Its cut length (metres).
    pub length: f64,
    /// What each end rests on: `column`, `wall`, `beam` or none, with the element.
    pub supports: [Option<(String, ElementId)>; 2],
    pub designation: Option<String>,
    pub material: Option<String>,
}

/// Reports beams' usage, length and supports (`beam.info`).
pub fn info(store: &Store, p: &InfoParams) -> Result<Vec<Info>> {
    let all = all(store);
    resolve_beams(store, &p.beam_ids)?
        .into_iter()
        .map(|(id, r)| {
            let g = &all
                .iter()
                .find(|(b, _)| *b == id)
                .ok_or_else(|| {
                    Error::new("beam.invalid", format!("beam {id} does not regenerate"))
                })?
                .1;
            let s = |k: usize| {
                g.supports[k].map(|s| match s {
                    Support::Column(c) => ("column".to_owned(), c),
                    Support::Wall(w) => ("wall".to_owned(), w),
                    Support::Beam(b) => ("beam".to_owned(), b),
                })
            };
            Ok(Info {
                beam_id: id,
                usage: g.usage,
                usage_locked: r.usage != Usage::Automatic,
                length: g.length(),
                supports: [s(0), s(1)],
                designation: g.ty.designation.clone(),
                material: g.ty.material.clone(),
            })
        })
        .collect()
}

/// Moves, turns or mirrors a beam (`element.move`, `.rotate`, `.mirror`, `.copy`).
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    mirror: bool,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = beam_recipe(store.get(id).expect("exists")).expect("a beam");
    r.start = x(r.start);
    r.end = x(r.end);
    if mirror {
        r.rotation = -r.rotation;
        r.y_offset = -r.y_offset;
        r.y_justification = match r.y_justification {
            YJustification::Left => YJustification::Right,
            YJustification::Right => YJustification::Left,
            j => j,
        };
    }
    r.start_offset += dz;
    r.end_offset += dz;
    write(txn, id, &r)?;
    refresh(txn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exits_and_insides() {
        let sq = vec![[-0.2, -0.2], [0.2, -0.2], [0.2, 0.2], [-0.2, 0.2]];
        assert!(inside([0.0, 0.0], &sq));
        assert!(!inside([0.5, 0.0], &sq));
        assert!((exit([0.0, 0.0], [1.0, 0.0], &sq) - 0.2).abs() < 1e-12);
        assert!((exit([0.1, 0.0], [-1.0, 0.0], &sq) - 0.3).abs() < 1e-12);
    }

    #[test]
    fn usages_map_to_ifc() {
        assert_eq!(Usage::Purlin.class(), "IfcMember");
        assert_eq!(Usage::Purlin.ifc("IfcMember"), ("PURLIN", None));
        assert_eq!(Usage::Girder.ifc("IfcBeam"), ("BEAM", Some("GIRDER")));
        assert_eq!(
            Usage::Purlin.ifc("IfcBeam"),
            ("USERDEFINED", Some("PURLIN"))
        );
    }
}
