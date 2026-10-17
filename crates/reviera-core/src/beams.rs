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
    /// Set by the user (locked), or automatic. Braces' usage is always from their ends.
    #[serde(default)]
    pub usage: Usage,
    /// A brace (STR-BRACE-001): an `IfcMember` (`BRACE`) whose ends may be at different levels
    /// and attached to columns and beams.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub brace: bool,
    /// The end's level (GlobalId), when it is not `level`; `end_offset` is above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_level: Option<String>,
    /// Each end attached to a column or beam: it follows the member; `start`/`end` and the
    /// offsets are where it stays if the member goes.
    #[serde(default, skip_serializing_if = "is_free")]
    pub attach: [Option<Attachment>; 2],
    /// A beam of a beam system (STR-BSYS-001) or a member of a truss (STR-TRUSS-001): laid out
    /// by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemSlot>,
    /// A vertical member's facing: the plan direction its section's width runs across (a truss
    /// web kept when its truss is removed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<[f64; 2]>,
}

/// A beam's place in its beam system: the system (GlobalId), the layout line and the piece of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemSlot {
    pub system: String,
    pub slot: usize,
    #[serde(default)]
    pub piece: usize,
}

/// A brace end attached to a column's or beam's axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Attachment {
    /// The column or beam (GlobalId).
    pub target: String,
    /// Along its axis from its start (base), 0 to 1; or a distance in metres when `by_distance`.
    pub at: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_distance: bool,
}

fn is_free(a: &[Option<Attachment>; 2]) -> bool {
    a.iter().all(Option::is_none)
}

/// A column's or (non-brace) beam's axis, for attaching to.
fn target_axis(store: &Store, guid: &str) -> Option<(V3, V3)> {
    let e = store.get(find(store, guid)?)?;
    if crate::columns::column_recipe(e).is_some() {
        return crate::columns::regenerate(store, e).ok().map(|g| g.axis());
    }
    let r = beam_recipe(e).filter(|r| !r.brace)?;
    line_of(store, &r).ok()
}

/// A recipe's location line, ignoring attachments.
fn line_of(store: &Store, r: &BeamRecipe) -> Result<(V3, V3)> {
    let z = crate::levels::elevation_of(store, &r.level)?;
    let z_end = match &r.end_level {
        Some(l) => crate::levels::elevation_of(store, l)?,
        None => z,
    };
    Ok((
        [r.start[0], r.start[1], z + r.start_offset],
        [r.end[0], r.end[1], z_end + r.end_offset],
    ))
}

fn at_attachment(store: &Store, a: &Attachment) -> Option<V3> {
    let (p, q) = target_axis(store, &a.target)?;
    let len = dist(p, q);
    let t = if a.by_distance {
        if len < 1e-9 { 0.0 } else { a.at / len }
    } else {
        a.at
    };
    Some(add(p, scale(sub(q, p), t)))
}

/// The column or beam whose axis passes through `p` (within 50 mm), and how far along it.
fn attachment_at(store: &Store, p: V3, not: Option<ElementId>) -> Option<Attachment> {
    let near = |a: V3, b: V3| -> Option<f64> {
        let ab = sub(b, a);
        let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        if l2 < 1e-12 {
            return None;
        }
        let ap = sub(p, a);
        let t = ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / l2).clamp(0.0, 1.0);
        (dist(add(a, scale(ab, t)), p) < 0.05).then_some(t)
    };
    let columns = crate::columns::all(store)
        .into_iter()
        .filter_map(|(id, g)| {
            let (a, b) = g.axis();
            Some((id, near(a, b)?))
        });
    let beams = store
        .iter()
        .filter(|e| Some(e.id()) != not)
        .filter_map(|e| Some((e.id(), beam_recipe(e).filter(|r| !r.brace)?)))
        .filter_map(|(id, r)| {
            let (a, b) = line_of(store, &r).ok()?;
            Some((id, near(a, b)?))
        });
    let (id, t) = columns.chain(beams).next()?;
    Some(Attachment {
        target: store.get(id)?.guid().to_string(),
        at: (t * 1e9).round() / 1e9,
        by_distance: false,
    })
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
        let (mut a, mut b) = line_of(store, &r)?;
        if let Some((p, q)) = r.system.as_ref().and_then(|s| system_piece(store, s)) {
            (a, b) = (p, q);
        }
        // A truss's vertical web faces across the truss.
        let side = r
            .system
            .as_ref()
            .and_then(|s| truss_direction(store, s))
            .or(r.side);
        if let Some(p) = r.attach[0].as_ref().and_then(|t| at_attachment(store, t)) {
            a = p;
        }
        if let Some(p) = r.attach[1].as_ref().and_then(|t| at_attachment(store, t)) {
            b = p;
        }
        let d = sub(b, a);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let plan = d[0].hypot(d[1]);
        if (plan < 1e-3 && side.is_none()) || len < 1e-3 {
            return Err(Error::new(
                "beam.too_short",
                "a beam needs two ends apart in plan (use a column for vertical members)",
            ));
        }
        let u = scale(d, 1.0 / len);
        let left = match side {
            Some(t) if plan < 1e-3 => [-t[1], t[0], 0.0],
            _ => [-d[1] / plan, d[0] / plan, 0.0],
        };
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

/// A column as a support: id, plan ring at the base, base, top, and the top's shift.
type ColumnSupport = (ElementId, Vec<[f64; 2]>, f64, f64, [f64; 2]);

/// The model's supports, gathered once.
struct Context {
    /// Columns: id, plan ring at the base, base and top, and the top's shift (slanted).
    columns: Vec<ColumnSupport>,
    /// Walls: id, footprint, base and top.
    walls: Vec<(ElementId, Vec<[f64; 2]>, f64, f64)>,
    beams: Vec<Frame>,
    /// Point-to-point steel (Structural Settings): steel members are not shortened at joins.
    point_to_point: bool,
}

impl Context {
    fn new(store: &Store) -> Context {
        let columns = crate::columns::all(store)
            .into_iter()
            .map(|(id, g)| {
                let top = g.top();
                (id, g.ring, g.z0, top, g.shift)
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
            point_to_point: crate::structural::settings(store).point_to_point_steel,
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
        for (id, ring, z0, z1, shift) in &self.columns {
            // A slanted column's section where the end meets it.
            let t = ((p[2] - z0) / (z1 - z0).max(1e-9)).clamp(0.0, 1.0);
            let ring: Vec<[f64; 2]> = ring
                .iter()
                .map(|q| [q[0] + shift[0] * t, q[1] + shift[1] * t])
                .collect();
            if p[2] >= z0 - tol && p[2] <= z1 + tol && inside(p2, &ring) {
                return Some((Support::Column(*id), exit(p2, inward, &ring)));
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
        if f.recipe.brace {
            return if (f.a[2] - f.b[2]).abs() < 1e-3 {
                Usage::HorizontalBracing
            } else {
                Usage::Other
            };
        }
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
                if f.recipe.joined[k] && !(steel && self.point_to_point) {
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
        let r = beam_recipe(e);
        let brace = r.as_ref().is_some_and(|r| r.brace);
        let chord = r
            .as_ref()
            .and_then(|r| r.system.as_ref())
            .and_then(|s| truss_direction(&store, s).map(|_| s.slot < 2));
        let (predefined, object) = match (brace, chord) {
            (_, Some(true)) => ("CHORD", None),
            (_, Some(false)) => ("STRUT", None),
            (true, None) => ("BRACE", None),
            (false, None) => g.usage.ifc(&e.class),
        };
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
    load_section_as(txn, p, "IfcBeamType", "BEAM")
}

/// Makes (or finds) a brace type (an `IfcMemberType`, `BRACE`) of a catalogue section
/// (`brace.load_section`).
pub(crate) fn load_brace_section(
    txn: &mut crate::txn::Transaction<'_>,
    p: crate::steel_sections::LoadParams,
) -> Result<ElementId> {
    load_section_as(txn, p, "IfcMemberType", "BRACE")
}

fn load_section_as(
    txn: &mut crate::txn::Transaction<'_>,
    p: crate::steel_sections::LoadParams,
    class: &str,
    predefined: &str,
) -> Result<ElementId> {
    let c = crate::steel_sections::catalog(&p.catalog)
        .ok_or_else(|| Error::new("section.no_catalog", format!("no catalogue {}", p.catalog)))?;
    let mut t = crate::steel_sections::section_type(txn.store(), &c, &p.designation, p.material)?;
    t.kind = BEAM_TYPE_RECIPE.into();
    if let Some(e) = txn.store().iter().find(|e| {
        e.class == class
            && beam_type(e)
                .is_some_and(|x| x.designation == t.designation && x.material == t.material)
    }) {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new(class)
            .name(&p.designation)
            .param("@PredefinedType", predefined)
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
        brace: false,
        end_level: None,
        attach: [None, None],
        system: None,
        side: None,
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
    /// A new location line in plan (an attached brace end follows).
    pub curve: Option<Line>,
    pub reference_level: Option<ElementRef>,
    /// The end's own level (its offset is kept, now above it).
    pub end_level: Option<ElementRef>,
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
    let end_level = p
        .end_level
        .as_ref()
        .map(|r| resolve_level(&store, r))
        .transpose()?;
    for (id, mut r) in resolve_beams(&store, &p.beam_ids)? {
        if let Some(l) = &level {
            // Keep the ends where they are.
            let dz = crate::levels::elevation_of(&store, &r.level)?
                - crate::levels::elevation_of(&store, l)?;
            r.start_offset += dz;
            if r.end_level.is_none() {
                r.end_offset += dz;
            }
            r.level = l.clone();
        }
        if let Some(c) = &p.curve {
            r.start = c.start;
            r.end = c.end;
        }
        if let Some(l) = &end_level {
            // A new end level frees an attached end.
            r.end_level = (*l != r.level).then(|| l.clone());
            r.attach[1] = None;
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
    // Where it is now, then moved; a moved member leaves what it was attached to.
    if let Ok(f) = Frame::new(&store, store.get(id).expect("exists")) {
        let z = crate::levels::elevation_of(&store, &r.level)?;
        let z_end = match &r.end_level {
            Some(l) => crate::levels::elevation_of(&store, l)?,
            None => z,
        };
        r.start = [f.a[0], f.a[1]];
        r.end = [f.b[0], f.b[1]];
        r.start_offset = f.a[2] - z;
        r.end_offset = f.b[2] - z_end;
    }
    r.attach = [None, None];
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BraceParams {
    /// A brace type (a beam or brace type); omitted, the region's usual brace section (HSS6X6X3/8,
    /// HEA 160 or UC 152x152x23), made on first use.
    pub type_id: Option<ElementRef>,
    /// Each end's plan point, level and offset above it.
    pub start_point: [f64; 2],
    pub start_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start_offset: f64,
    pub end_point: [f64; 2],
    pub end_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_offset: f64,
    /// Attach the ends to the columns or beams whose axes they lie on (default true).
    #[serde(default = "yes1")]
    pub attach: bool,
}

fn yes1() -> bool {
    true
}

/// Places a brace (`brace.create`, STR-BRACE-001): an `IfcMember` (`BRACE`) between two ends,
/// each at its level and offset. An end on a column's or beam's axis is attached to it (by its
/// ratio along it) and follows it. Its usage is horizontal bracing when both ends are level,
/// otherwise other.
pub(crate) fn create_brace(
    txn: &mut crate::txn::Transaction<'_>,
    p: BraceParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = resolve_level(&store, &p.start_level)?;
    let end_level = resolve_level(&store, &p.end_level)?;
    let z0 = crate::levels::elevation_of(&store, &level)? + p.start_offset;
    let z1 = crate::levels::elevation_of(&store, &end_level)? + p.end_offset;
    if (p.end_point[0] - p.start_point[0])
        .hypot(p.end_point[1] - p.start_point[1])
        .hypot(z1 - z0)
        < 1e-3
    {
        return Err(Error::new(
            "brace.zero_length",
            "the brace's ends are in the same place",
        ));
    }
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => {
            let (catalog, designation) = crate::codes::settings(&store).code.structural().brace;
            load_brace_section(
                txn,
                crate::steel_sections::LoadParams {
                    catalog: catalog.into(),
                    designation: designation.into(),
                    material: None,
                },
            )?
        }
    };
    let store = txn.store().clone();
    let ends = [
        [p.start_point[0], p.start_point[1], z0],
        [p.end_point[0], p.end_point[1], z1],
    ];
    let end_level = (end_level != level).then_some(end_level);
    let attach = if p.attach {
        ends.map(|e| attachment_at(&store, e, None))
    } else {
        [None, None]
    };
    let recipe = BeamRecipe {
        kind: BEAM_RECIPE.into(),
        start: p.start_point,
        end: p.end_point,
        level,
        start_offset: p.start_offset,
        end_offset: p.end_offset,
        rotation: 0.0,
        y_justification: YJustification::Centre,
        y_offset: 0.0,
        z_justification: ZJustification::Centre,
        z_offset: 0.0,
        orientation: Orientation::Normal,
        start_extension: 0.0,
        end_extension: 0.0,
        cutback: [None, None],
        joined: [true, true],
        usage: Usage::Automatic,
        brace: true,
        end_level,
        attach,
        system: None,
        side: None,
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(NewElement::new("IfcMember").param("@type", guid).param(
        RECIPE_KEY,
        serde_json::to_value(&recipe).expect("serialises"),
    ))?;
    finish(txn, id, "IfcMember")?;
    refresh(txn)?;
    Ok(id)
}

/// Frees brace ends attached to elements about to be deleted: each end stays where it is now,
/// and a warning names the brace.
pub(crate) fn release_attachments(
    txn: &mut crate::txn::Transaction<'_>,
    going: &std::collections::BTreeSet<ElementId>,
) -> Result<()> {
    let store = txn.store().clone();
    let guids: HashSet<String> = going
        .iter()
        .filter_map(|id| store.get(*id).map(|e| e.guid().to_string()))
        .collect();
    for e in store.iter() {
        let Some(mut r) = beam_recipe(e) else {
            continue;
        };
        if going.contains(&e.id()) || !r.attach.iter().flatten().any(|a| guids.contains(&a.target))
        {
            continue;
        }
        let f = Frame::new(&store, e)?;
        let z = crate::levels::elevation_of(&store, &r.level)?;
        let z_end = match &r.end_level {
            Some(l) => crate::levels::elevation_of(&store, l)?,
            None => z,
        };
        for k in 0..2 {
            if r.attach[k]
                .as_ref()
                .is_some_and(|a| guids.contains(&a.target))
            {
                r.attach[k] = None;
                let (p, base) = if k == 0 { (f.a, z) } else { (f.b, z_end) };
                if k == 0 {
                    r.start = [p[0], p[1]];
                    r.start_offset = p[2] - base;
                } else {
                    r.end = [p[0], p[1]];
                    r.end_offset = p[2] - base;
                }
            }
        }
        let id = e.id();
        write(txn, id, &r)?;
        txn.warn(crate::txn::Warning {
            id: "str.brace.end_released".into(),
            message: format!(
                "Brace {id} lost what an end was attached to; the end stays where it was"
            ),
            element_ids: vec![id],
        });
    }
    Ok(())
}

// ---- Beam systems (STR-BSYS-001) ----

pub const BEAM_SYSTEM_RECIPE: &str = "recipe.beam_system";

/// How a beam system spaces its beams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayoutRule {
    /// `value` is the centre spacing.
    FixedDistance,
    /// `value` is the number of beams.
    FixedNumber,
    /// The fewest beams that keep the spacing at or below `value`.
    MaximumSpacing,
    /// `value` is the clear spacing between beam faces.
    ClearSpacing,
}

/// Where a spaced layout starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayoutJustification {
    #[default]
    Beginning,
    End,
    Centre,
}

/// The stored recipe of a beam system: its boundary (a plan polygon whose edges may be locked to
/// their supports), its level and offset, the beams' direction, type and layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemRecipe {
    pub kind: String,
    pub boundary: Vec<[f64; 2]>,
    /// Edge `i` (from vertex `i` to `i + 1`) locked to this beam or wall (GlobalId).
    #[serde(default)]
    pub supports: Vec<Option<String>>,
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    /// The beams' direction in plan (a unit vector).
    pub direction: [f64; 2],
    pub beam_type: String,
    pub rule: LayoutRule,
    pub value: f64,
    #[serde(default)]
    pub justification: LayoutJustification,
}

pub fn system_recipe(e: &Element) -> Option<SystemRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == BEAM_SYSTEM_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A support's line in plan: a (non-system) beam's location line or a wall's.
fn support_line(store: &Store, guid: &str) -> Option<([f64; 2], [f64; 2])> {
    let e = store.get(find(store, guid)?)?;
    if let Some(r) = beam_recipe(e).filter(|r| r.system.is_none()) {
        return Some((r.start, r.end));
    }
    let w = crate::walls::wall_recipe(e)?;
    Some((w.curve.start, w.curve.end))
}

fn meet(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])) -> Option<[f64; 2]> {
    let r = [a.1[0] - a.0[0], a.1[1] - a.0[1]];
    let t = [b.1[0] - b.0[0], b.1[1] - b.0[1]];
    let den = r[0] * t[1] - r[1] * t[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let qp = [b.0[0] - a.0[0], b.0[1] - a.0[1]];
    let u = (qp[0] * t[1] - qp[1] * t[0]) / den;
    Some([a.0[0] + u * r[0], a.0[1] + u * r[1]])
}

/// A system's boundary as it is now: each locked edge on its support's line, the corners where
/// consecutive edges meet.
pub fn system_boundary(store: &Store, r: &SystemRecipe) -> Vec<[f64; 2]> {
    let n = r.boundary.len();
    let line = |i: usize| {
        r.supports
            .get(i)
            .and_then(Option::as_deref)
            .and_then(|g| support_line(store, g))
            .unwrap_or((r.boundary[i], r.boundary[(i + 1) % n]))
    };
    (0..n)
        .map(|i| meet(line((i + n - 1) % n), line(i)).unwrap_or(r.boundary[i]))
        .collect()
}

/// A system's layout: the beams' widths, the offsets across the bay of its layout lines, and the
/// pieces of each line inside the boundary.
pub fn system_layout(store: &Store, r: &SystemRecipe) -> Vec<Vec<([f64; 2], [f64; 2])>> {
    let ring = system_boundary(store, r);
    let d = r.direction;
    let n = [-d[1], d[0]];
    let across: Vec<f64> = ring.iter().map(|p| dot2(*p, n)).collect();
    let lo = across.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = across.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let w = hi - lo;
    let width = find(store, &r.beam_type)
        .and_then(|t| store.get(t))
        .and_then(beam_type)
        .map_or(0.15, |t| t.width);
    let spaced = |s: f64| -> Vec<f64> {
        if s <= 1e-6 {
            return Vec::new();
        }
        let k = ((w - 1e-6) / s).floor() as i64;
        match r.justification {
            LayoutJustification::Beginning => (1..=k).map(|i| lo + i as f64 * s).collect(),
            LayoutJustification::End => (1..=k).map(|i| hi - i as f64 * s).collect(),
            LayoutJustification::Centre => {
                let m = (lo + hi) / 2.0;
                let count = ((w / s) - 1e-9).floor().max(0.0) as i64;
                (0..count)
                    .map(|i| m + (i as f64 - (count - 1) as f64 / 2.0) * s)
                    .collect()
            }
        }
    };
    let even = |count: usize| -> Vec<f64> {
        (1..=count)
            .map(|i| lo + w * i as f64 / (count + 1) as f64)
            .collect()
    };
    let positions: Vec<f64> = match r.rule {
        LayoutRule::FixedDistance => spaced(r.value),
        LayoutRule::ClearSpacing => spaced(r.value + width),
        LayoutRule::FixedNumber => even(r.value.max(0.0).round() as usize),
        LayoutRule::MaximumSpacing if r.value > 1e-6 => {
            even(((w / r.value) - 1e-9).ceil().max(1.0) as usize - 1)
        }
        LayoutRule::MaximumSpacing => Vec::new(),
    };
    positions
        .into_iter()
        .filter(|t| *t > lo + 1e-6 && *t < hi - 1e-6)
        .map(|t| pieces(&ring, d, n, t))
        .collect()
}

/// The pieces of the line `n·p = t` (running along `d`) inside the polygon.
fn pieces(ring: &[[f64; 2]], d: [f64; 2], n: [f64; 2], t: f64) -> Vec<([f64; 2], [f64; 2])> {
    let mut hits: Vec<f64> = Vec::new();
    let m = ring.len();
    for i in 0..m {
        let (a, b) = (ring[i], ring[(i + 1) % m]);
        let (ta, tb) = (dot2(a, n), dot2(b, n));
        if (ta - t) * (tb - t) < 0.0 || ((ta - t).abs() < 1e-12) != ((tb - t).abs() < 1e-12) {
            if (ta - tb).abs() < 1e-12 {
                continue;
            }
            let u = (t - ta) / (tb - ta);
            if (0.0..1.0).contains(&u) || ((tb - t).abs() < 1e-12) {
                let p = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
                hits.push(dot2(p, d));
            }
        }
    }
    hits.sort_by(f64::total_cmp);
    hits.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let at = |s: f64| [n[0] * t + d[0] * s, n[1] * t + d[1] * s];
    hits.chunks(2)
        .filter(|c| c.len() == 2 && c[1] - c[0] > 1e-6)
        .map(|c| (at(c[0]), at(c[1])))
        .collect()
}

/// A system beam's ends as its system lays it out now.
fn system_piece(store: &Store, s: &SystemSlot) -> Option<(V3, V3)> {
    let e = store.get(find(store, &s.system)?)?;
    if let Some(t) = truss_recipe(e) {
        let m = truss_members(store, &t).ok()?;
        return m.get(s.slot).map(|m| (m.0, m.1));
    }
    let r = system_recipe(e)?;
    let z = crate::levels::elevation_of(store, &r.level).ok()? + r.offset;
    let (a, b) = *system_layout(store, &r).get(s.slot)?.get(s.piece)?;
    Some(([a[0], a[1], z], [b[0], b[1], z]))
}

/// Makes a system's beams afresh: deletes those it has and lays out new ones.
fn relayout(txn: &mut crate::txn::Transaction<'_>, id: ElementId) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let e = store.get(id).expect("a system");
    let guid = e.guid().to_string();
    let r = system_recipe(e).expect("a system");
    for b in store.iter() {
        if beam_recipe(b)
            .and_then(|r| r.system)
            .is_some_and(|s| s.system == guid)
        {
            txn.delete(b.id())?;
        }
    }
    let type_id = find(&store, &r.beam_type)
        .ok_or_else(|| Error::new("beam_system.no_type", "the system's beam type is gone"))?;
    let level_ref = ElementRef::Guid(GlobalId::parse(&r.level).expect("a level's GlobalId"));
    let mut out = Vec::new();
    for (slot, line) in system_layout(&store, &r).into_iter().enumerate() {
        for (piece, (a, b)) in line.into_iter().enumerate() {
            let beam = create_one(
                txn,
                CreateParams {
                    type_id: Some(ElementRef::Id(type_id)),
                    curve: Line { start: a, end: b },
                    reference_level: level_ref.clone(),
                    start_offset: r.offset,
                    end_offset: r.offset,
                    usage: Usage::Automatic,
                    orientation: Orientation::Normal,
                    y_justification: None,
                    z_justification: None,
                    rotation: 0.0,
                },
            )?;
            let s = txn.store().clone();
            let mut br = beam_recipe(s.get(beam).expect("created")).expect("a beam");
            br.system = Some(SystemSlot {
                system: guid.clone(),
                slot,
                piece,
            });
            write(txn, beam, &br)?;
            out.push(beam);
        }
    }
    refresh(txn)?;
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemSketchParams {
    /// The boundary's corners in plan (a closed polygon).
    pub boundary: Vec<[f64; 2]>,
    /// The beams run parallel to this edge (from corner `i` to `i + 1`); default 0.
    #[serde(default)]
    pub direction_edge: usize,
    pub type_id: Option<ElementRef>,
    pub layout_rule: LayoutRule,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub value: f64,
    #[serde(default)]
    pub justification: LayoutJustification,
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Lock edges lying on beams' or walls' location lines to them (default true).
    #[serde(default = "yes1")]
    pub lock_to_supports: bool,
}

/// The beam or wall (not of a system) whose location line an edge lies on.
fn support_under(store: &Store, a: [f64; 2], b: [f64; 2]) -> Option<String> {
    let on = |p: [f64; 2], (s, e): ([f64; 2], [f64; 2])| {
        let d = [e[0] - s[0], e[1] - s[1]];
        let l = d[0].hypot(d[1]);
        l > 1e-9 && ((p[0] - s[0]) * d[1] - (p[1] - s[1]) * d[0]).abs() / l < 0.01
    };
    store
        .iter()
        .filter(|e| {
            beam_recipe(e).is_some_and(|r| r.system.is_none())
                || crate::walls::wall_recipe(e).is_some()
        })
        .find(|e| support_line(store, &e.guid().to_string()).is_some_and(|l| on(a, l) && on(b, l)))
        .map(|e| e.guid().to_string())
}

fn new_system(
    txn: &mut crate::txn::Transaction<'_>,
    mut ring: Vec<[f64; 2]>,
    mut supports: Vec<Option<String>>,
    direction: [f64; 2],
    p: (
        Option<ElementRef>,
        LayoutRule,
        f64,
        LayoutJustification,
        ElementRef,
        f64,
    ),
) -> Result<ElementId> {
    let (type_id, rule, value, justification, level, offset) = p;
    if ring.len() < 3 {
        return Err(Error::new(
            "beam_system.boundary",
            "a boundary needs at least three corners",
        ));
    }
    // Counter-clockwise.
    if crate::sketch::signed_area(&ring) < 0.0 {
        // Edge i of the reversed ring is edge n - 2 - i of the original.
        ring.reverse();
        supports.reverse();
        supports.rotate_left(1);
    }
    let store = txn.store().clone();
    let type_id = match &type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let l = direction[0].hypot(direction[1]);
    if l < 1e-9 {
        return Err(Error::new(
            "beam_system.direction",
            "the beam direction has no length",
        ));
    }
    let recipe = SystemRecipe {
        kind: BEAM_SYSTEM_RECIPE.into(),
        boundary: ring,
        supports,
        level: resolve_level(&store, &level)?,
        offset,
        direction: [direction[0] / l, direction[1] / l],
        beam_type: store.get(type_id).expect("resolved").guid().to_string(),
        rule,
        value,
        justification,
    };
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .param("@PredefinedType", "BEAM_GRID")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    relayout(txn, id)?;
    Ok(id)
}

/// Creates a beam system from a sketched boundary (`beam_system.create_sketch`).
pub(crate) fn create_system_sketch(
    txn: &mut crate::txn::Transaction<'_>,
    p: SystemSketchParams,
) -> Result<ElementId> {
    let n = p.boundary.len();
    if n < 3 || p.direction_edge >= n {
        return Err(Error::new(
            "beam_system.boundary",
            "a boundary needs at least three corners, and the direction edge one of its edges",
        ));
    }
    let (a, b) = (
        p.boundary[p.direction_edge],
        p.boundary[(p.direction_edge + 1) % n],
    );
    let store = txn.store().clone();
    let supports = (0..n)
        .map(|i| {
            p.lock_to_supports
                .then(|| support_under(&store, p.boundary[i], p.boundary[(i + 1) % n]))
                .flatten()
        })
        .collect();
    new_system(
        txn,
        p.boundary,
        supports,
        [b[0] - a[0], b[1] - a[1]],
        (
            p.type_id,
            p.layout_rule,
            p.value,
            p.justification,
            p.level,
            p.offset,
        ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemAutoParams {
    /// A point in the bay.
    pub point: [f64; 2],
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    pub type_id: Option<ElementRef>,
    pub layout_rule: LayoutRule,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub value: f64,
    #[serde(default)]
    pub justification: LayoutJustification,
}

/// Creates a beam system in the bay around a point (`beam_system.create_auto`): the smallest loop
/// of beams (at the level) and walls (through it) around the point, its edges locked to them, the
/// beams parallel to the support nearest the point.
pub(crate) fn create_system_auto(
    txn: &mut crate::txn::Transaction<'_>,
    p: SystemAutoParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = resolve_level(&store, &p.level)?;
    let z = crate::levels::elevation_of(&store, &level)? + p.offset;
    let mut edges = Vec::new();
    for e in store.iter() {
        let line = if let Some(r) = beam_recipe(e).filter(|r| r.system.is_none()) {
            let Ok((a, b)) = line_of(&store, &r) else {
                continue;
            };
            if (a[2] - z).abs() > 0.05 || (b[2] - z).abs() > 0.05 {
                continue;
            }
            Line {
                start: r.start,
                end: r.end,
            }
        } else if crate::walls::wall_recipe(e).is_some() {
            let Ok(g) = crate::walls::regenerate(&store, e) else {
                continue;
            };
            if z < g.base_z - 1e-6 || z > g.base_z + g.height + 1e-6 {
                continue;
            }
            Line {
                start: g.start,
                end: g.end,
            }
        } else {
            continue;
        };
        edges.push(crate::arrangement::Edge {
            line,
            lock: Some((e.id(), "support".into())),
        });
    }
    let lp = crate::arrangement::loop_around(&edges, p.point).ok_or_else(|| {
        Error::new(
            "beam_system.no_bay",
            "no closed bay of beams or walls around the point at that level",
        )
    })?;
    let ring: Vec<[f64; 2]> = lp.iter().map(|e| e.line.start).collect();
    let supports: Vec<Option<String>> = lp
        .iter()
        .map(|e| {
            e.lock
                .as_ref()
                .and_then(|(id, _)| store.get(*id))
                .map(|s| s.guid().to_string())
        })
        .collect();
    // Parallel to the edge nearest the point.
    let nearest = lp
        .iter()
        .min_by(|a, b| {
            ring_distance(p.point, &[a.line.start, a.line.end])
                .total_cmp(&ring_distance(p.point, &[b.line.start, b.line.end]))
        })
        .expect("a loop has edges");
    let d = [
        nearest.line.end[0] - nearest.line.start[0],
        nearest.line.end[1] - nearest.line.start[1],
    ];
    new_system(
        txn,
        ring,
        supports,
        d,
        (
            p.type_id,
            p.layout_rule,
            p.value,
            p.justification,
            p.level,
            p.offset,
        ),
    )
}

fn resolve_system(store: &Store, r: &ElementRef) -> Result<(ElementId, SystemRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let recipe = store.get(id).and_then(system_recipe).ok_or_else(|| {
        Error::new(
            "beam_system.not_a_system",
            format!("element {id} is not a beam system"),
        )
    })?;
    Ok((id, recipe))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLayoutParams {
    pub system_id: ElementRef,
    pub rule: Option<LayoutRule>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub value: Option<f64>,
    pub justification: Option<LayoutJustification>,
    pub type_id: Option<ElementRef>,
}

/// Changes a system's layout (or just lays it out again for its bay as it is now) and makes its
/// beams afresh (`beam_system.set_layout`).
pub(crate) fn set_layout(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetLayoutParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_system(&store, &p.system_id)?;
    if let Some(v) = p.rule {
        r.rule = v;
    }
    if let Some(v) = p.value {
        r.value = v;
    }
    if let Some(v) = p.justification {
        r.justification = v;
    }
    if let Some(t) = &p.type_id {
        let t = resolve_type(&store, t)?;
        r.beam_type = store.get(t).expect("resolved").guid().to_string();
    }
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    relayout(txn, id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemParams {
    pub system_id: ElementRef,
}

/// Removes a beam system, keeping its beams as independent framing where they are
/// (`beam_system.remove`).
pub(crate) fn remove_system(
    txn: &mut crate::txn::Transaction<'_>,
    p: SystemParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, _) = resolve_system(&store, &p.system_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    let kept = release_members(txn, &guid)?;
    txn.delete(id)?;
    Ok(kept)
}

/// A beam system as reported (`beam_system.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SystemInfo {
    pub system_id: ElementId,
    pub boundary: Vec<[f64; 2]>,
    pub direction: [f64; 2],
    pub rule: LayoutRule,
    pub value: f64,
    /// Its beams, and the centre spacing between layout lines (none for one line).
    pub beams: Vec<ElementId>,
    pub spacing: Option<f64>,
}

/// Reports a beam system's boundary, layout and beams (`beam_system.info`).
pub fn system_info(store: &Store, p: &SystemParams) -> Result<SystemInfo> {
    let (id, r) = resolve_system(store, &p.system_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    let layout = system_layout(store, &r);
    let n = [-r.direction[1], r.direction[0]];
    let offs: Vec<f64> = layout
        .iter()
        .filter_map(|l| l.first().map(|(a, _)| dot2(*a, n)))
        .collect();
    let spacing = (offs.len() >= 2).then(|| (offs[1] - offs[0]).abs());
    Ok(SystemInfo {
        system_id: id,
        boundary: system_boundary(store, &r),
        direction: r.direction,
        rule: r.rule,
        value: r.value,
        beams: store
            .iter()
            .filter(|b| {
                beam_recipe(b)
                    .and_then(|r| r.system)
                    .is_some_and(|s| s.system == guid)
            })
            .map(Element::id)
            .collect(),
        spacing,
    })
}

// ---- Trusses (STR-TRUSS-001) ----

pub const TRUSS_RECIPE: &str = "recipe.truss";

/// A truss's web pattern.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrussLayout {
    /// Verticals, diagonals sloping down toward mid-span.
    #[default]
    Pratt,
    /// Verticals, diagonals sloping up toward mid-span.
    Howe,
    /// Alternating diagonals, no interior verticals.
    Warren,
}

/// Which chord the truss's line runs along.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BearingChord {
    /// The bottom chord on the line, the top chord `height` above.
    #[default]
    Bottom,
    /// The top chord on the line, the bottom chord `height` below.
    Top,
}

/// The stored recipe of a truss: its line in plan at a level, its layout and member types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrussRecipe {
    pub kind: String,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub layout: TrussLayout,
    /// Panels along the span (kept when the truss is stretched).
    pub panels: usize,
    pub height: f64,
    #[serde(default)]
    pub bearing: BearingChord,
    /// The chords' and webs' types (GlobalIds).
    pub chord_type: String,
    pub web_type: String,
}

pub fn truss_recipe(e: &Element) -> Option<TrussRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == TRUSS_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The plan direction of the truss a member belongs to.
fn truss_direction(store: &Store, s: &SystemSlot) -> Option<[f64; 2]> {
    let t = truss_recipe(store.get(find(store, &s.system)?)?)?;
    let d = [t.end[0] - t.start[0], t.end[1] - t.start[1]];
    let l = d[0].hypot(d[1]);
    (l > 1e-9).then(|| [d[0] / l, d[1] / l])
}

/// A truss's members as they are now: each one's ends and whether it is a chord. The top chord
/// comes first, then the bottom chord, the verticals and the diagonals.
pub fn truss_members(store: &Store, t: &TrussRecipe) -> Result<Vec<(V3, V3, bool)>> {
    let z = crate::levels::elevation_of(store, &t.level)? + t.offset;
    let (zb, zt) = match t.bearing {
        BearingChord::Bottom => (z, z + t.height),
        BearingChord::Top => (z - t.height, z),
    };
    let n = t.panels.max(1);
    let at = |i: usize, top: bool| {
        let k = i as f64 / n as f64;
        [
            t.start[0] + (t.end[0] - t.start[0]) * k,
            t.start[1] + (t.end[1] - t.start[1]) * k,
            if top { zt } else { zb },
        ]
    };
    let mut out = vec![
        (at(0, true), at(n, true), true),
        (at(0, false), at(n, false), true),
    ];
    let verticals: Vec<usize> = match t.layout {
        TrussLayout::Warren => vec![0, n],
        _ => (0..=n).collect(),
    };
    for i in verticals {
        out.push((at(i, false), at(i, true), false));
    }
    for k in 0..n {
        let left_half = 2 * k < n;
        // Which end of panel k is up.
        let up_at_start = match t.layout {
            TrussLayout::Pratt => left_half,
            TrussLayout::Howe => !left_half,
            TrussLayout::Warren => k % 2 == 1,
        };
        out.push(if up_at_start {
            (at(k, true), at(k + 1, false), false)
        } else {
            (at(k, false), at(k + 1, true), false)
        });
    }
    Ok(out)
}

/// Makes a truss's members afresh.
fn relayout_truss(txn: &mut crate::txn::Transaction<'_>, id: ElementId) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let e = store.get(id).expect("a truss");
    let guid = e.guid().to_string();
    let t = truss_recipe(e).expect("a truss");
    for b in store.iter() {
        if beam_recipe(b)
            .and_then(|r| r.system)
            .is_some_and(|s| s.system == guid)
        {
            txn.delete(b.id())?;
        }
    }
    let z = crate::levels::elevation_of(&store, &t.level)?;
    let mut out = Vec::new();
    for (slot, (a, b, chord)) in truss_members(&store, &t)?.into_iter().enumerate() {
        let ty = if chord { &t.chord_type } else { &t.web_type };
        let recipe = BeamRecipe {
            kind: BEAM_RECIPE.into(),
            start: [a[0], a[1]],
            end: [b[0], b[1]],
            level: t.level.clone(),
            start_offset: a[2] - z,
            end_offset: b[2] - z,
            rotation: 0.0,
            y_justification: YJustification::Centre,
            y_offset: 0.0,
            z_justification: ZJustification::Centre,
            z_offset: 0.0,
            orientation: Orientation::Normal,
            start_extension: 0.0,
            end_extension: 0.0,
            cutback: [None, None],
            joined: [false, false],
            usage: Usage::Other,
            brace: false,
            end_level: None,
            attach: [None, None],
            system: Some(SystemSlot {
                system: guid.clone(),
                slot,
                piece: 0,
            }),
            side: None,
        };
        let m = txn.create(
            NewElement::new("IfcMember")
                .param("@type", ty.clone())
                .param(
                    RECIPE_KEY,
                    serde_json::to_value(&recipe).expect("serialises"),
                ),
        )?;
        finish(txn, m, "IfcMember")?;
        out.push(m);
    }
    refresh(txn)?;
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrussParams {
    /// The truss's line in plan, at `level` + `offset`.
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    #[serde(default)]
    pub layout: TrussLayout,
    /// Default: about one panel per 1.5 m, even, at least 4.
    pub panels: Option<usize>,
    /// Default: a tenth of the span, at least 600 mm.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    #[serde(default)]
    pub bearing: BearingChord,
    /// The chords' and webs' types (beam or brace types); default the region's usual brace
    /// section for both.
    pub chord_type_id: Option<ElementRef>,
    pub web_type_id: Option<ElementRef>,
}

/// Creates a truss (`truss.create`, STR-TRUSS-001): an `IfcElementAssembly` (`TRUSS`) whose chords
/// (`IfcMember` `CHORD`) and webs (`STRUT`) are members laid out from it.
pub(crate) fn create_truss(
    txn: &mut crate::txn::Transaction<'_>,
    p: TrussParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let span = (p.end[0] - p.start[0]).hypot(p.end[1] - p.start[1]);
    if span < 0.5 {
        return Err(Error::new(
            "truss.too_short",
            "a truss needs a span of at least 500 mm",
        ));
    }
    let default_type = |txn: &mut crate::txn::Transaction<'_>| -> Result<ElementId> {
        let (catalog, designation) = crate::codes::settings(txn.store()).code.structural().brace;
        load_brace_section(
            txn,
            crate::steel_sections::LoadParams {
                catalog: catalog.into(),
                designation: designation.into(),
                material: None,
            },
        )
    };
    let chord = match &p.chord_type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let web = match &p.web_type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let guid = |id: ElementId| store.get(id).expect("resolved").guid().to_string();
    let panels = p
        .panels
        .unwrap_or_else(|| ((span / 1.5).round() as usize).max(4).div_ceil(2) * 2);
    if panels == 0 {
        return Err(Error::new(
            "truss.panels",
            "a truss needs at least one panel",
        ));
    }
    let height = p.height.unwrap_or((span / 10.0).max(0.6));
    if height <= 0.0 {
        return Err(Error::new(
            "truss.height",
            "a truss's height must be above zero",
        ));
    }
    let recipe = TrussRecipe {
        kind: TRUSS_RECIPE.into(),
        start: p.start,
        end: p.end,
        level: resolve_level(&store, &p.level)?,
        offset: p.offset,
        layout: p.layout,
        panels,
        height,
        bearing: p.bearing,
        chord_type: guid(chord),
        web_type: guid(web),
    };
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .param("@PredefinedType", "TRUSS")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    relayout_truss(txn, id)?;
    Ok(id)
}

fn resolve_truss(store: &Store, r: &ElementRef) -> Result<(ElementId, TrussRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let t = store
        .get(id)
        .and_then(truss_recipe)
        .ok_or_else(|| Error::new("truss.not_a_truss", format!("element {id} is not a truss")))?;
    Ok((id, t))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrussSetParams {
    pub truss_id: ElementRef,
    /// A new line (the truss stretches; its panel count stays).
    pub start: Option<[f64; 2]>,
    pub end: Option<[f64; 2]>,
    pub layout: Option<TrussLayout>,
    pub panels: Option<usize>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    pub bearing: Option<BearingChord>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
}

/// Changes a truss (`truss.set`): its line (the members follow), or its layout, panels, height or
/// bearing chord (the members are made afresh when the layout or panels change).
pub(crate) fn set_truss(txn: &mut crate::txn::Transaction<'_>, p: TrussSetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_truss(&store, &p.truss_id)?;
    let remake = p.layout.is_some() || p.panels.is_some();
    if let Some(v) = p.start {
        t.start = v;
    }
    if let Some(v) = p.end {
        t.end = v;
    }
    if let Some(v) = p.layout {
        t.layout = v;
    }
    if let Some(v) = p.panels {
        if v == 0 {
            return Err(Error::new(
                "truss.panels",
                "a truss needs at least one panel",
            ));
        }
        t.panels = v;
    }
    if let Some(v) = p.height {
        if v <= 0.0 {
            return Err(Error::new(
                "truss.height",
                "a truss's height must be above zero",
            ));
        }
        t.height = v;
    }
    if let Some(v) = p.bearing {
        t.bearing = v;
    }
    if let Some(v) = p.offset {
        t.offset = v;
    }
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    if remake {
        relayout_truss(txn, id)?;
    } else {
        refresh(txn)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrussIdParams {
    pub truss_id: ElementRef,
}

/// Frees a host's (beam system's or truss's) members where they are now.
fn release_members(txn: &mut crate::txn::Transaction<'_>, guid: &str) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut kept = Vec::new();
    for b in store.iter() {
        let Some(mut r) = beam_recipe(b) else {
            continue;
        };
        if !r.system.as_ref().is_some_and(|s| s.system == guid) {
            continue;
        }
        if let Ok(f) = Frame::new(&store, b) {
            let z = crate::levels::elevation_of(&store, &r.level)?;
            r.start = [f.a[0], f.a[1]];
            r.end = [f.b[0], f.b[1]];
            r.start_offset = f.a[2] - z;
            r.end_offset = f.b[2] - z;
        }
        // A vertical web keeps facing across its truss.
        if (r.start[0] - r.end[0]).hypot(r.start[1] - r.end[1]) < 1e-3 {
            r.side = r.system.as_ref().and_then(|s| truss_direction(&store, s));
        }
        r.system = None;
        write(txn, b.id(), &r)?;
        kept.push(b.id());
    }
    Ok(kept)
}

/// Removes a truss, keeping its members as plain framing where they are (`truss.remove`).
pub(crate) fn remove_truss(
    txn: &mut crate::txn::Transaction<'_>,
    p: TrussIdParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, _) = resolve_truss(&store, &p.truss_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    let kept = release_members(txn, &guid)?;
    txn.delete(id)?;
    Ok(kept)
}

/// A truss as reported (`truss.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrussInfo {
    pub truss_id: ElementId,
    pub layout: TrussLayout,
    pub panels: usize,
    pub height: f64,
    pub span: f64,
    pub chords: Vec<ElementId>,
    pub webs: Vec<ElementId>,
}

/// Reports a truss's layout and members (`truss.info`).
pub fn truss_info(store: &Store, p: &TrussIdParams) -> Result<TrussInfo> {
    let (id, t) = resolve_truss(store, &p.truss_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    let mut members: Vec<(usize, ElementId)> = store
        .iter()
        .filter_map(|b| {
            let s = beam_recipe(b)?.system?;
            (s.system == guid).then_some((s.slot, b.id()))
        })
        .collect();
    members.sort();
    Ok(TrussInfo {
        truss_id: id,
        layout: t.layout,
        panels: t.panels,
        height: t.height,
        span: (t.end[0] - t.start[0]).hypot(t.end[1] - t.start[1]),
        chords: members.iter().filter(|m| m.0 < 2).map(|m| m.1).collect(),
        webs: members.iter().filter(|m| m.0 >= 2).map(|m| m.1).collect(),
    })
}

/// A Coarse plan's single line for a member (STR-SET-001).
#[derive(Debug, Clone, PartialEq)]
pub struct CoarseLine {
    pub element: ElementId,
    pub segment: [[f64; 3]; 2],
}

/// The single lines of the beams, braces and truss members whose location lines pass between
/// `z_lo` and `z_hi`, for a Coarse plan drawn at `scale` (100 for 1:100): each along its
/// location line between its body's ends, less the coarse cutback at joined ends; braces drawn
/// with their symbol (a parallel line offset to one side, or none).
pub fn coarse_lines(store: &Store, z_lo: f64, z_hi: f64, drawing_scale: f64) -> Vec<CoarseLine> {
    let s = crate::structural::settings(store);
    let paper = |mm: f64| mm * drawing_scale / 1000.0;
    let mut out = Vec::new();
    for (id, g) in all(store) {
        let (lo, hi) = (g.start[2].min(g.end[2]), g.start[2].max(g.end[2]));
        if hi < z_lo - 1e-6 || lo > z_hi + 1e-6 {
            continue;
        }
        let Some(r) = store.get(id).and_then(beam_recipe) else {
            continue;
        };
        let len = g.length();
        let cut = paper(s.coarse_cutback).min(len / 4.0);
        let t0 = if g.supports[0].is_some() && r.joined[0] {
            cut
        } else {
            0.0
        };
        let t1 = if g.supports[1].is_some() && r.joined[1] {
            cut
        } else {
            0.0
        };
        let mut a = add(g.start, scale(g.u, t0));
        let mut b = add(g.start, scale(g.u, len - t1));
        if r.brace {
            // A kicker: one end on a beam's span.
            let kicker = r.attach.iter().flatten().any(|t| {
                find(store, &t.target)
                    .and_then(|e| store.get(e))
                    .is_some_and(|e| beam_recipe(e).is_some())
                    && t.at > 1e-6
                    && t.at < 1.0 - 1e-6
            });
            let symbol = if kicker {
                s.kicker_symbol
            } else {
                s.brace_symbol
            };
            if symbol == crate::structural::BraceSymbol::None {
                continue;
            }
            let off = scale(g.left, paper(s.brace_offset));
            a = add(a, off);
            b = add(b, off);
        }
        out.push(CoarseLine {
            element: id,
            segment: [a, b],
        });
    }
    out
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
