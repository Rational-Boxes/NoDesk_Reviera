// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketched rebar and rebar constraints (STR-RBR-007).
//!
//! A sketched set's bar is drawn as lines and arcs in a plane inside its host; the corners are
//! kept in world coordinates and the set spreads along the plane's normal. Its shape is matched
//! to a loaded shape code (00, 11, 21, 51) or becomes 99 (a special shape).
//!
//! Each handle — a segment (`segment:N`, from 0), or an end (`start`, `end`) — can be constrained
//! to a planar reference: a host face's **cover**, a host **surface**, or **another bar**, with an
//! offset further in. The planes are found again every time the bars are worked out, so
//! constrained handles follow the host while unconstrained ones keep their absolute positions. A
//! segment moves square to its plane, an end along its bar. A reference must be within 6° of
//! parallel to its handle (for an end, of square to the bar).

use std::cell::Cell;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::rebar::{self, Bar, BarType, HostKind, RebarRecipe, SetGeometry, V3, add, dot, len, mul, sub, unit};
use crate::NewElement;
use crate::store::Store;

/// The largest angle between a reference and its handle.
const TOLERANCE_DEG: f64 = 6.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchPlacement {
    /// The bar's centreline corners (world, metres).
    pub points: Vec<V3>,
    /// The sketch plane's normal: the direction the set spreads in.
    pub normal: V3,
    /// `standard` or `stirrup_tie`.
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_end: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,
}

fn standard() -> String {
    "standard".into()
}

/// A handle kept at an offset from a planar reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Constraint {
    /// `segment:N` (from 0), `start` or `end`.
    pub handle: String,
    pub target: Target,
    /// Further in (metres): from the cover plane, from the surface to the bar's centreline, or
    /// between the two bars' centrelines.
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A host face's cover: the plane where a bar's centre sits at the cover.
    Cover { face: String },
    /// A host face itself.
    Surface { face: String },
    /// Another set's bar (GlobalId; the bar by its place in the set, from 0).
    Bar {
        rebar: String,
        #[serde(default)]
        bar: usize,
    },
}

/// A planar host face: a point on it and its outward normal.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Face {
    pub name: String,
    pub point: V3,
    pub outward: V3,
    /// The cover face it takes (`top`, `bottom`, `exterior`, `interior` or `other`).
    #[serde(skip)]
    cover: &'static str,
}

/// A host's planar faces.
pub fn host_faces(store: &Store, host: &Element, kind: HostKind) -> Result<Vec<Face>> {
    let face = |name: &str, point: V3, outward: V3, cover: &'static str| Face { name: name.to_owned(), point, outward, cover };
    match kind {
        HostKind::Column | HostKind::Beam => {
            let f = crate::steel_detail::member_frame(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host does not regenerate"))?;
            let (hw, hd) = (f.ty.width / 2.0, f.ty.depth / 2.0);
            let mid = mul(add(f.start, f.end), 0.5);
            let beam = kind == HostKind::Beam;
            Ok(vec![
                face("top", add(mid, mul(f.deep, hd)), f.deep, if beam { "top" } else { "other" }),
                face("bottom", add(mid, mul(f.deep, -hd)), mul(f.deep, -1.0), if beam { "bottom" } else { "other" }),
                face("left", add(mid, mul(f.across, hw)), f.across, "other"),
                face("right", add(mid, mul(f.across, -hw)), mul(f.across, -1.0), "other"),
                face("start", f.start, mul(f.axis, -1.0), if beam { "other" } else { "bottom" }),
                face("end", f.end, f.axis, if beam { "other" } else { "top" }),
            ])
        }
        HostKind::Floor => {
            let g = crate::floors::regenerate(store, host)?;
            let mut out = vec![
                face("top", [0.0, 0.0, g.top], [0.0, 0.0, 1.0], "top"),
                face("bottom", [0.0, 0.0, g.top - g.thickness], [0.0, 0.0, -1.0], "bottom"),
            ];
            if let Some(p) = g.profiles.first() {
                let o = &p.outer;
                let area: f64 = (0..o.len()).map(|i| o[i][0] * o[(i + 1) % o.len()][1] - o[(i + 1) % o.len()][0] * o[i][1]).sum();
                let s = if area >= 0.0 { 1.0 } else { -1.0 };
                for i in 0..o.len() {
                    let (a, b) = (o[i], o[(i + 1) % o.len()]);
                    let n = unit([(b[1] - a[1]) * s, -(b[0] - a[0]) * s, 0.0]);
                    out.push(face(&format!("edge:{i}"), [a[0], a[1], g.top], n, "other"));
                }
            }
            Ok(out)
        }
        HostKind::Roof => Err(Error::new("rebar.curved", "a roof's faces are not planes here: use a free-form distribution")),
        HostKind::Wall => {
            let w = crate::walls::wall_recipe(host).ok_or_else(|| Error::new("rebar.no_host", "the host is not a Reviera wall"))?;
            if !w.through.is_empty() {
                return Err(Error::new("rebar.curved", "the wall is curved: sketched bars need its faces to be planes"));
            }
            let g = crate::walls::regenerate(store, host)?;
            let (d, n) = (g.direction(), g.normal());
            let (d, n) = ([d[0], d[1], 0.0], [n[0], n[1], 0.0]);
            let s = [g.start[0], g.start[1], g.base_z];
            let e = [g.end[0], g.end[1], g.base_z];
            Ok(vec![
                face("exterior", add(s, mul(n, g.width / 2.0)), n, "exterior"),
                face("interior", add(s, mul(n, -g.width / 2.0)), mul(n, -1.0), "interior"),
                face("bottom", s, [0.0, 0.0, -1.0], "bottom"),
                face("top", add(s, [0.0, 0.0, g.height]), [0.0, 0.0, 1.0], "top"),
                face("start", s, mul(d, -1.0), "other"),
                face("end", e, d, "other"),
            ])
        }
    }
}

/// The cover face a host face takes.
pub fn face_cover(f: &Face) -> &'static str {
    f.cover
}

fn find_face<'a>(faces: &'a [Face], name: &str) -> Result<&'a Face> {
    faces.iter().find(|f| f.name == name).ok_or_else(|| {
        Error::new("rebar.face", format!("the host has no face {name} (it has: {})", faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")))
    })
}

/// The plane a host is sketched in by default: a floor's and a wall's middle, and a beam's or
/// column's long section through its axis.
pub fn host_plane(store: &Store, host: &Element) -> Option<crate::work_planes::Plane> {
    use crate::work_planes::{Plane, cross};
    match rebar::host_kind(store, host)? {
        HostKind::Floor => {
            let g = crate::floors::regenerate(store, host).ok()?;
            Plane::new([0.0, 0.0, g.top - g.thickness / 2.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).ok()
        }
        HostKind::Wall => {
            let g = crate::walls::regenerate(store, host).ok()?;
            let (d, n) = (g.direction(), g.normal());
            // y up: the normal is x × up.
            let _ = n;
            let x = [d[0], d[1], 0.0];
            Plane::new([g.start[0], g.start[1], g.base_z], x, cross(x, [0.0, 0.0, 1.0])).ok()
        }
        HostKind::Beam => {
            let f = crate::steel_detail::member_frame(store, host)?;
            Plane::new(f.start, f.axis, cross(f.axis, f.deep)).ok()
        }
        HostKind::Column => {
            let f = crate::steel_detail::member_frame(store, host)?;
            Plane::new(f.start, f.across, cross(f.across, f.axis)).ok()
        }
        HostKind::Roof => None,
    }
}

thread_local! {
    /// How deep bar-to-bar references are being followed (a bar's own references to other bars
    /// are not followed again, so two bars referencing each other settle).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// A handle's direction (along the bar) and whether it is an end, by its id.
fn handle(points: &[V3], id: &str) -> Result<(Vec<usize>, V3, bool)> {
    let n = points.len();
    let seg = |i: usize| unit(sub(points[i + 1], points[i]));
    match id {
        "start" => Ok((vec![0], unit(sub(points[0], points[1])), true)),
        "end" => Ok((vec![n - 1], seg(n - 2), true)),
        s => {
            let i = s
                .strip_prefix("segment:")
                .and_then(|k| k.parse::<usize>().ok())
                .filter(|i| i + 1 < n)
                .ok_or_else(|| Error::new("rebar.handle", format!("no handle {s}: the bar has start, end and segment:0 … segment:{}", n - 2)))?;
            Ok((vec![i, i + 1], seg(i), false))
        }
    }
}

/// A reference's plane: a point, and the direction the offset grows in (into the host or away
/// from the other bar).
fn target_plane(store: &Store, host: &Element, kind: HostKind, d: f64, t: &Target, at: V3) -> Result<(V3, V3, Option<V3>)> {
    match t {
        Target::Cover { face } | Target::Surface { face } => {
            let faces = host_faces(store, host, kind)?;
            let f = find_face(&faces, face)?;
            let inward = mul(f.outward, -1.0);
            let shift = if matches!(t, Target::Cover { .. }) { rebar::cover(store, host, kind, f.cover) + d / 2.0 } else { 0.0 };
            Ok((add(f.point, mul(inward, shift)), inward, None))
        }
        Target::Bar { rebar: guid, bar } => {
            let other = GlobalId::parse(guid)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|i| store.get(i))
                .and_then(rebar::rebar_recipe)
                .ok_or_else(|| Error::new("rebar.target", "the bar it references is gone"))?;
            DEPTH.with(|c| c.set(c.get() + 1));
            let g = rebar::geometry(store, &other);
            DEPTH.with(|c| c.set(c.get() - 1));
            let g = g?;
            let b = g.bars.get(*bar).ok_or_else(|| Error::new("rebar.target", format!("the referenced set has no bar {bar}")))?;
            // Its segment nearest the handle, and the way from it to the handle.
            let (a, dir) = b
                .points
                .windows(2)
                .filter(|w| len(sub(w[1], w[0])) > 1e-9)
                .map(|w| {
                    let dir = unit(sub(w[1], w[0]));
                    let s = dot(sub(at, w[0]), dir).clamp(0.0, len(sub(w[1], w[0])));
                    (len(sub(at, add(w[0], mul(dir, s)))), w[0], dir)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, a, dir)| (a, dir))
                .ok_or_else(|| Error::new("rebar.target", "the referenced bar has no length"))?;
            let w = sub(at, a);
            let away = sub(w, mul(dir, dot(w, dir)));
            if len(away) < 1e-9 {
                return Err(Error::new("rebar.target", "the handle lies on the referenced bar"));
            }
            Ok((a, unit(away), Some(dir)))
        }
    }
}

/// How far a reference is from parallel to a handle (degrees).
fn skew(dir: V3, end: bool, normal: V3, along: Option<V3>) -> f64 {
    let deg = |c: f64| c.clamp(-1.0, 1.0).acos().to_degrees();
    match along {
        // Another bar: the two directions.
        Some(t) => deg(dot(dir, t).abs()),
        // A plane: square to an end, parallel to a segment.
        None if end => deg(dot(dir, normal).abs()),
        None => 90.0 - deg(dot(dir, normal).abs()),
    }
}

/// A sketch's corners with its constraints met.
fn solved(store: &Store, host: &Element, kind: HostKind, d: f64, s: &SketchPlacement) -> Result<Vec<V3>> {
    let mut pts = s.points.clone();
    let follow_bars = DEPTH.with(Cell::get) == 0;
    let mut planes = Vec::new();
    for c in &s.constraints {
        if matches!(c.target, Target::Bar { .. }) && !follow_bars {
            continue;
        }
        let (idx, _, end) = handle(&pts, &c.handle)?;
        let mid = mul(idx.iter().fold([0.0; 3], |a, i| add(a, pts[*i])), 1.0 / idx.len() as f64);
        let (p, n, _) = target_plane(store, host, kind, d, &c.target, mid)?;
        planes.push((c.handle.clone(), idx, end, p, n, c.offset));
    }
    // Alternating projections: each handle onto its plane in turn, until they agree.
    for _ in 0..32 {
        let mut moved = 0.0f64;
        for (id, idx, end, p, n, off) in &planes {
            if *end {
                let (_, dir, _) = handle(&pts, id)?;
                let c = dot(dir, *n);
                if c.abs() < 1e-6 {
                    continue;
                }
                let k = idx[0];
                let t = (off - dot(sub(pts[k], *p), *n)) / c;
                pts[k] = add(pts[k], mul(dir, t));
                moved = moved.max(t.abs());
            } else {
                for &k in idx {
                    let t = off - dot(sub(pts[k], *p), *n);
                    pts[k] = add(pts[k], mul(*n, t));
                    moved = moved.max(t.abs());
                }
            }
        }
        if moved < 1e-10 {
            break;
        }
    }
    Ok(pts)
}

/// The shape code a sketch matches: 00 straight, 11 one 90° bend, 21 two 90° bends to one side,
/// 51 a closed rectangle; anything else is 99, a special shape.
pub fn match_shape(points: &[V3]) -> &'static str {
    let n = points.len();
    let dirs: Vec<V3> = points.windows(2).map(|w| unit(sub(w[1], w[0]))).collect();
    let square = |a: V3, b: V3| dot(a, b).abs() < 1e-3;
    let closed = n >= 2 && len(sub(points[0], points[n - 1])) < 1e-3;
    match n {
        2 => "00",
        3 if square(dirs[0], dirs[1]) => "11",
        4 if square(dirs[0], dirs[1]) && square(dirs[1], dirs[2]) && dot(dirs[0], dirs[2]) < -0.999 => "21",
        5 if closed && dirs.windows(2).all(|w| square(w[0], w[1])) => "51",
        _ => "99",
    }
}

/// Where a hook at a bar's end turns: square to the bar, toward the host's middle.
fn hook_inward(host_mid: V3, end: V3, out: V3) -> V3 {
    let w = sub(host_mid, end);
    let v = sub(w, mul(out, dot(w, out)));
    if len(v) < 1e-9 { [0.0, 0.0, -1.0] } else { unit(v) }
}

/// A sketched set's bars.
pub(crate) fn geometry(store: &Store, r: &RebarRecipe, host: &Element, kind: HostKind, bt: &BarType, s: &SketchPlacement) -> Result<SetGeometry> {
    if s.points.len() < 2 {
        return Err(Error::new("rebar.sketch", "a sketched bar needs at least two points"));
    }
    let d = bt.diameter;
    let pts = solved(store, host, kind, d, s)?;
    let normal = unit(s.normal);
    let faces = host_faces(store, host, kind)?;
    let host_mid = mul(faces.iter().fold([0.0; 3], |a, f| add(a, f.point)), 1.0 / faces.len().max(1) as f64);
    // The set runs from the sketch to the far face's cover along the normal.
    let spread = faces
        .iter()
        .filter(|f| dot(f.outward, normal) > 0.999)
        .map(|f| dot(sub(f.point, pts[0]), normal) - rebar::cover(store, host, kind, f.cover) - d / 2.0)
        .fold(f64::NAN, f64::max);
    let spread = if spread.is_nan() { 0.0 } else { spread.max(0.0) };
    let pos = rebar::positions(&r.layout, spread, d)?;
    let stirrup = s.style == "stirrup_tie";
    let bend = if stirrup { bt.stirrup_bend } else { bt.standard_bend };
    let mut line = pts.clone();
    let n = line.len();
    if let Some(h) = &s.hook_start {
        let h = rebar::hook_type(store, h)?;
        let out = unit(sub(pts[0], pts[1]));
        let mut v = rebar::hook_points(pts[0], out, hook_inward(host_mid, pts[0], out), &h, bt, 10.0);
        v.reverse();
        line.splice(0..1, v);
    }
    if let Some(h) = &s.hook_end {
        let h = rebar::hook_type(store, h)?;
        let out = unit(sub(pts[n - 1], pts[n - 2]));
        let v = rebar::hook_points(pts[n - 1], out, hook_inward(host_mid, pts[n - 1], out), &h, bt, 10.0);
        let k = line.len();
        line.splice(k - 1..k, v);
    }
    let centre = rebar::rounded(&line, bend / 2.0 + d / 2.0);
    let bars = pos.iter().map(|t| Bar { points: centre.iter().map(|p| add(*p, mul(normal, *t))).collect() }).collect();
    // Each segment's outside length: its corners' run plus half a bar at a bend.
    let segs = pts.len() - 1;
    let dims = (0..segs)
        .map(|i| {
            let l = len(sub(pts[i + 1], pts[i]));
            let bends = usize::from(i > 0) + usize::from(i + 1 < segs);
            (crate::rebar_docs::SEGMENTS.chars().nth(i).map_or(format!("S{i}"), |c| c.to_string()), l + d / 2.0 * bends as f64)
        })
        .collect();
    Ok(SetGeometry { bars, dimensions: dims, role: if stirrup { "ligature".into() } else { "main".into() }, array_length: spread, indices: Vec::new() })
}

// ---- Commands ----

/// A sketch plane: origin, x direction and normal.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlaneArg {
    pub origin: V3,
    pub x: V3,
    pub normal: V3,
}

/// A line or an arc through a point, in the plane's coordinates.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    Line { start: [f64; 2], end: [f64; 2] },
    Arc { start: [f64; 2], end: [f64; 2], through: [f64; 2] },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SketchParams {
    pub host_id: ElementRef,
    /// The sketch plane; the host's own when omitted (a floor's or wall's middle, a beam's or
    /// column's long section through its axis).
    #[serde(default)]
    pub plane: Option<PlaneArg>,
    /// Lines and arcs end to end, in the plane's coordinates (metres).
    pub curves: Vec<Curve>,
    pub bar_type: String,
    /// `standard` or `stirrup_tie`.
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub hook_start: Option<String>,
    #[serde(default)]
    pub hook_end: Option<String>,
    /// The set's layout along the plane's normal; a single bar when omitted.
    #[serde(default = "single")]
    pub layout: rebar::Layout,
}

fn single() -> rebar::Layout {
    rebar::Layout { rule: rebar::Rule::Single, ..Default::default() }
}

/// An arc's points from `a` to `b` through `m` (in the plane), 8 segments.
fn arc_points(a: [f64; 2], b: [f64; 2], m: [f64; 2]) -> Result<Vec<[f64; 2]>> {
    let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
    if d.abs() < 1e-12 {
        return Err(Error::new("rebar.sketch", "the arc's three points are in a line"));
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let c = [
        (sq(a) * (m[1] - b[1]) + sq(m) * (b[1] - a[1]) + sq(b) * (a[1] - m[1])) / d,
        (sq(a) * (b[0] - m[0]) + sq(m) * (a[0] - b[0]) + sq(b) * (m[0] - a[0])) / d,
    ];
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let r = (sq([a[0] - c[0], a[1] - c[1]])).sqrt();
    let tau = std::f64::consts::TAU;
    let (t0, tm, t1) = (ang(a), ang(m), ang(b));
    // Counter-clockwise from a to b, unless m is not on that way.
    let ccw = |x: f64| (x - t0).rem_euclid(tau);
    let sweep = if ccw(tm) <= ccw(t1) { ccw(t1) } else { ccw(t1) - tau };
    Ok((1..=8).map(|k| {
        let t = t0 + sweep * k as f64 / 8.0;
        [c[0] + r * t.cos(), c[1] + r * t.sin()]
    }).collect())
}

/// Sketches a set (`rebar.sketch`).
pub(crate) fn sketch(txn: &mut crate::txn::Transaction<'_>, p: SketchParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = store.resolve(&p.host_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.host_id)))?;
    let host = store.get(id).expect("resolved");
    rebar::host_check(&store, host).map_err(|why| Error::new("rebar.not_a_host", format!("element {id} cannot host rebar: {why}")))?;
    let plane = match &p.plane {
        Some(a) => crate::work_planes::Plane::new(a.origin, a.x, a.normal)?,
        None => host_plane(&store, host).ok_or_else(|| Error::new("rebar.sketch", "the host's plane cannot be worked out; give one"))?,
    };
    if !["standard", "stirrup_tie"].contains(&p.style.as_str()) {
        return Err(Error::new("rebar.style", format!("the style is standard or stirrup_tie, not {}", p.style)));
    }
    let mut flat: Vec<[f64; 2]> = Vec::new();
    let mut arcs = false;
    for c in &p.curves {
        let (a, more) = match c {
            Curve::Line { start, end } => (*start, vec![*end]),
            Curve::Arc { start, end, through } => {
                arcs = true;
                (*start, arc_points(*start, *end, *through)?)
            }
        };
        match flat.last() {
            None => flat.push(a),
            Some(l) if (l[0] - a[0]).hypot(l[1] - a[1]) < 1e-3 => {}
            Some(_) => return Err(Error::new("rebar.sketch", "the curves must join end to start")),
        }
        flat.extend(more);
    }
    flat.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6);
    if flat.len() < 2 {
        return Err(Error::new("rebar.sketch", "sketch at least one line"));
    }
    let points: Vec<V3> = flat.iter().map(|q| plane.to_world(*q)).collect();
    let code = if arcs { "99" } else { match_shape(&points) };
    let bt = rebar::bar_type(&store, &p.bar_type)?;
    for h in [&p.hook_start, &p.hook_end].into_iter().flatten() {
        rebar::hook_type(&store, h)?;
    }
    let r = RebarRecipe {
        kind: rebar::REBAR_RECIPE.into(),
        host: host.guid().to_string(),
        shape: code.into(),
        bar_type: bt.id.clone(),
        placement: rebar::Placement::Sketch(SketchPlacement {
            points,
            normal: plane.normal,
            style: p.style.clone(),
            hook_start: p.hook_start.clone(),
            hook_end: p.hook_end.clone(),
            constraints: Vec::new(),
        }),
        layout: p.layout.clone(),
        edits: rebar::BarEdits::default(),
        splice: None,
        varying: false,
    };
    let g = rebar::geometry(&store, &r)?;
    let predefined = if g.role == "ligature" { "LIGATURE" } else { "MAIN" };
    let id = txn.create(
        NewElement::new("IfcReinforcingBar")
            .param("@PredefinedType", predefined)
            .param(crate::walls::RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}

fn sketched(store: &Store, r: &ElementRef) -> Result<(ElementId, RebarRecipe, SketchPlacement)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let rec = store.get(id).and_then(rebar::rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
    match &rec.placement {
        rebar::Placement::Sketch(s) => {
            let s = s.clone();
            Ok((id, rec, s))
        }
        _ => Err(Error::new("rebar.not_sketched", format!("set {id} is not sketched; only sketched sets take constraints yet"))),
    }
}

/// Keeps a set's handles where its constraints put them, so a change of constraints does not
/// move what it leaves.
fn bake(store: &Store, rec: &RebarRecipe, s: &mut SketchPlacement) -> Result<()> {
    let host = GlobalId::parse(&rec.host)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|i| store.get(i))
        .ok_or_else(|| Error::new("rebar.no_host", "the bar set's host is gone"))?;
    let kind = rebar::host_kind(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    let d = rebar::bar_type(store, &rec.bar_type)?.diameter;
    s.points = solved(store, host, kind, d, s)?;
    Ok(())
}

fn save(txn: &mut crate::txn::Transaction<'_>, id: ElementId, mut rec: RebarRecipe, s: SketchPlacement) -> Result<()> {
    rec.placement = rebar::Placement::Sketch(s);
    rebar::geometry(txn.store(), &rec)?;
    txn.set_param(id, crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
    crate::rebar_numbers::persist(txn)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConstrainParams {
    pub rebar_id: ElementRef,
    /// `segment:N` (from 0), `start` or `end`.
    pub handle_id: String,
    /// `cover`, `surface` or `bar`; `none` removes the handle's constraint (it stays where it is).
    pub target_kind: String,
    /// A host face (`top`, `bottom`, `left`, `right`, `start`, `end`; a wall's `exterior`,
    /// `interior`; a floor's `edge:N`), or for `bar` the other set (and `bar_index`).
    #[serde(default)]
    pub target_ref: Option<Value>,
    #[serde(default)]
    pub bar_index: usize,
    /// Further in (metres). Defaults: 0 from a cover; the handle's present distance from a
    /// surface or bar, so it does not move.
    #[serde(default)]
    pub offset: Option<f64>,
}

/// Constrains a handle (`rebar.constrain`).
pub(crate) fn constrain(txn: &mut crate::txn::Transaction<'_>, p: ConstrainParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, rec, mut s) = sketched(&store, &p.rebar_id)?;
    bake(&store, &rec, &mut s)?;
    let (idx, dir, end) = handle(&s.points, &p.handle_id)?;
    s.constraints.retain(|c| c.handle != p.handle_id);
    if p.target_kind == "none" {
        return save(txn, id, rec, s);
    }
    let face = || {
        p.target_ref
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::new("rebar.target", "give the host face as target_ref"))
    };
    let target = match p.target_kind.as_str() {
        "cover" => Target::Cover { face: face()? },
        "surface" => Target::Surface { face: face()? },
        "bar" => {
            let r: ElementRef = p
                .target_ref
                .clone()
                .and_then(|v| serde_json::from_value(v).ok())
                .ok_or_else(|| Error::new("rebar.target", "give the other set as target_ref"))?;
            let oid = store.resolve(&r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            if oid == id {
                return Err(Error::new("rebar.target", "a bar cannot reference its own set"));
            }
            let o = store.get(oid).filter(|e| rebar::rebar_recipe(e).is_some()).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {oid} is not a rebar set")))?;
            Target::Bar { rebar: o.guid().to_string(), bar: p.bar_index }
        }
        k => return Err(Error::new("rebar.target", format!("the target kind is cover, surface, bar or none, not {k}"))),
    };
    let host = GlobalId::parse(&rec.host).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i)).expect("baked");
    let kind = rebar::host_kind(&store, host).expect("baked");
    let d = rebar::bar_type(&store, &rec.bar_type)?.diameter;
    let mid = mul(idx.iter().fold([0.0; 3], |a, i| add(a, s.points[*i])), 1.0 / idx.len() as f64);
    let (pt, n, along) = target_plane(&store, host, kind, d, &target, mid)?;
    let off = skew(dir, end, n, along);
    if off > TOLERANCE_DEG + 1e-9 {
        let what = match &target {
            Target::Cover { face } => format!("the {face} cover"),
            Target::Surface { face } => format!("the {face} face"),
            Target::Bar { .. } => "the other bar".to_owned(),
        };
        return Err(Error::new(
            "rebar.not_parallel",
            format!(
                "{what} is {off:.1}° from {} the {}; a constraint needs it within {TOLERANCE_DEG:.0}°",
                if end && along.is_none() { "square to" } else { "parallel to" },
                if end { "bar's end" } else { "segment" }
            ),
        ));
    }
    let offset = match (p.offset, &target) {
        (Some(o), _) => o,
        (None, Target::Cover { .. }) => 0.0,
        (None, _) => dot(sub(mid, pt), n),
    };
    s.constraints.push(Constraint { handle: p.handle_id.clone(), target, offset });
    save(txn, id, rec, s)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ResetParams {
    pub rebar_id: ElementRef,
}

/// Removes a set's constraints, its handles staying where they are (`rebar.reset_constraints`).
pub(crate) fn reset_constraints(txn: &mut crate::txn::Transaction<'_>, p: ResetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, rec, mut s) = sketched(&store, &p.rebar_id)?;
    bake(&store, &rec, &mut s)?;
    s.constraints.clear();
    save(txn, id, rec, s)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HandleInfo {
    pub id: String,
    /// The handle's middle and direction now.
    pub at: V3,
    pub direction: V3,
    pub constraint: Option<Constraint>,
    /// Where the reference is: a point on its plane and the way the offset grows.
    pub target_point: Option<V3>,
    pub target_normal: Option<V3>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConstraintsInfo {
    pub handles: Vec<HandleInfo>,
    /// The host's faces a handle can reference.
    pub faces: Vec<Face>,
}

/// A sketched set's handles, their constraints and the host's faces (`rebar.constraints`).
pub fn constraints(store: &Store, p: ResetParams) -> Result<ConstraintsInfo> {
    let (_, rec, s) = sketched(store, &p.rebar_id)?;
    let host = GlobalId::parse(&rec.host)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|i| store.get(i))
        .ok_or_else(|| Error::new("rebar.no_host", "the bar set's host is gone"))?;
    let kind = rebar::host_kind(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    let d = rebar::bar_type(store, &rec.bar_type)?.diameter;
    let pts = solved(store, host, kind, d, &s)?;
    let n = pts.len();
    let ids = std::iter::once("start".to_owned()).chain((0..n - 1).map(|i| format!("segment:{i}"))).chain(std::iter::once("end".to_owned()));
    let handles = ids
        .map(|h| {
            let (idx, dir, _) = handle(&pts, &h)?;
            let at = mul(idx.iter().fold([0.0; 3], |a, i| add(a, pts[*i])), 1.0 / idx.len() as f64);
            let c = s.constraints.iter().find(|c| c.handle == h).cloned();
            let plane = c.as_ref().and_then(|c| target_plane(store, host, kind, d, &c.target, at).ok());
            Ok(HandleInfo { id: h, at, direction: dir, constraint: c, target_point: plane.map(|p| p.0), target_normal: plane.map(|p| p.1) })
        })
        .collect::<Result<_>>()?;
    Ok(ConstraintsInfo { handles, faces: host_faces(store, host, kind)? })
}
