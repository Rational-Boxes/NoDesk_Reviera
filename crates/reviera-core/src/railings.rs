// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Railings (spec Ch 3 §3.7.7–3.7.8, ARC-RAIL-001/002): guard rails along a sketched path, or
//! along the open edges of a floor, deck or flat roof (Pick Edges, *Guard all open edges*).
//!
//! - **Path.** A free railing keeps its plan path and its base (a level and an offset). A
//!   hosted railing keeps its host and the host's footprint curves it runs along, and its path
//!   is those curves set in by the setback, at the host's top: it follows the host's outline.
//! - **Parts** (from the railing type): a top rail and a bottom rail swept along the path, posts
//!   at the start, the end and every corner, and balusters by the pattern algorithm of
//!   ARC-RAIL-002 (one row; each segment is a pattern run, justified at its beginning, with no
//!   excess fill; balusters inside a post's reserved space are removed).
//!
//! A railing is an `IfcRailing` (GUARDRAIL) with an `IfcRailingType`; its parts are the body for
//! now (aggregated members come later). Built so far: flat paths. Railings on stairs (By Host
//! heights), continuous handrails and baluster families come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::{Line, RECIPE_KEY};

pub const CLASS: &str = "IfcRailing";
pub const TYPE_CLASS: &str = "IfcRailingType";
pub const RAILING_RECIPE: &str = "recipe.railing";
pub const RAILING_TYPE_RECIPE: &str = "recipe.railing_type";
pub const DEFAULT_TYPE_NAME: &str = "Guard Rail - 1000mm";

/// A closed mesh: positions and triangles (outward).
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// A railing type: its height and its parts' sizes (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RailingType {
    pub kind: String,
    /// The top of the top rail above the path.
    pub height: f64,
    /// Top rail: width and depth of its section.
    pub top_rail: [f64; 2],
    /// Bottom rail: its section and the height of its underside.
    pub bottom_rail: [f64; 2],
    pub bottom_rail_height: f64,
    /// Balusters: square section and spacing (the pattern length).
    pub baluster: f64,
    pub baluster_spacing: f64,
    /// Posts: square section, and the space each reserves along the path on either side.
    pub post: f64,
    pub post_space: f64,
}

impl Default for RailingType {
    fn default() -> Self {
        Self {
            kind: RAILING_TYPE_RECIPE.into(),
            height: 1.0,
            top_rail: [0.05, 0.05],
            bottom_rail: [0.04, 0.04],
            bottom_rail_height: 0.08,
            baluster: 0.02,
            baluster_spacing: 0.12,
            post: 0.08,
            post_space: 0.05,
        }
    }
}

/// Where a railing runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RailPath {
    /// A sketched path at a level plus an offset.
    Free {
        points: Vec<[f64; 2]>,
        closed: bool,
        level: String,
        base_offset: f64,
    },
    /// Along a host's footprint curves (in order), set in by `setback`, at the host's top.
    Host {
        host: String,
        keys: Vec<u32>,
        closed: bool,
        setback: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RailingRecipe {
    pub kind: String,
    pub path: RailPath,
    /// Openings and gates along the path (ARC-RAIL-003).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<RailFeature>,
}

/// An opening or a gate in a railing, by its centre's distance along the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RailFeature {
    pub key: u32,
    /// The centre's distance from the path's start (metres).
    pub at: f64,
    /// The clear width between the posts either side.
    pub width: f64,
    pub kind: FeatureKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    Opening,
    Gate {
        /// One leaf or two.
        leaves: u8,
        /// Swings out (to the right of the path) rather than in.
        swing_out: bool,
        /// Hinged at the start side (single leaf).
        hinge_start: bool,
        /// The gate element (GlobalId of its `IfcDoor`).
        gate: String,
    },
}

pub fn railing_recipe(e: &Element) -> Option<RailingRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RAILING_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn railing_type(e: &Element) -> Option<RailingType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RAILING_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A railing as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct RailingGeometry {
    /// The path in plan and its height.
    pub path: Vec<[f64; 2]>,
    pub closed: bool,
    pub z: f64,
    /// Rails, posts and balusters, each a closed box.
    pub parts: Vec<Mesh>,
    pub posts: usize,
    pub balusters: usize,
    pub length: f64,
}

/// A box of half-width `hw` across the segment `a`–`b` (plan), from `z0` to `z1`.
fn bar(a: [f64; 2], b: [f64; 2], hw: f64, z0: f64, z1: f64) -> Mesh {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]).max(1e-12);
    let n = [-d[1] / l * hw, d[0] / l * hw];
    // Counter-clockwise from above.
    let ring = [
        [a[0] - n[0], a[1] - n[1]],
        [b[0] - n[0], b[1] - n[1]],
        [b[0] + n[0], b[1] + n[1]],
        [a[0] + n[0], a[1] + n[1]],
    ];
    let mut positions: Vec<[f64; 3]> = ring.iter().map(|p| [p[0], p[1], z0]).collect();
    positions.extend(ring.iter().map(|p| [p[0], p[1], z1]));
    let mut indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7];
    for i in 0..4u32 {
        let j = (i + 1) % 4;
        indices.extend([i, j, 4 + j, i, 4 + j, 4 + i]);
    }
    (positions, indices)
}

/// A square column of half-size `h` at `p`, turned along `dir`, from `z0` to `z1`.
fn column(p: [f64; 2], dir: [f64; 2], h: f64, z0: f64, z1: f64) -> Mesh {
    bar(
        [p[0] - dir[0] * h, p[1] - dir[1] * h],
        [p[0] + dir[0] * h, p[1] + dir[1] * h],
        h,
        z0,
        z1,
    )
}

/// The positions (distance along each segment) of the posts and balusters of a path, by the
/// ARC-RAIL-002 algorithm: posts at the start (open paths), every corner and the end; each
/// segment a pattern run with balusters at every multiple of the spacing short of its end;
/// balusters within a post's reserved space removed. Returns, per segment, its post positions
/// and baluster positions.
pub fn layout(lengths: &[f64], closed: bool, t: &RailingType) -> Vec<(Vec<f64>, Vec<f64>)> {
    let n = lengths.len();
    let mut out = Vec::with_capacity(n);
    for (i, &len) in lengths.iter().enumerate() {
        // Posts at this segment's start (a corner or the path's start) and, for the last
        // segment of an open path, its end. A closed path's corners all have posts.
        let mut posts = vec![0.0];
        if !closed && i + 1 == n {
            posts.push(len);
        }
        // Posts reserve space on both sides; the next segment's start post reserves this
        // segment's end too.
        let reserved: Vec<f64> = if i + 1 < n || closed {
            let mut r = posts.clone();
            r.push(len);
            r
        } else {
            posts.clone()
        };
        let mut balusters = Vec::new();
        let mut k = 1;
        loop {
            let s = t.baluster_spacing * f64::from(k);
            if s >= len - 1e-9 {
                break;
            }
            if !reserved
                .iter()
                .any(|p| (s - p).abs() <= t.post_space + 1e-9)
            {
                balusters.push(s);
            }
            k += 1;
        }
        out.push((posts, balusters));
    }
    out
}

/// Builds a railing along `path` at height `z`.
pub fn generate(
    path: &[[f64; 2]],
    closed: bool,
    z: f64,
    t: &RailingType,
    features: &[RailFeature],
) -> Result<RailingGeometry> {
    if path.len() < 2 {
        return Err(Error::new(
            "railing.short_path",
            "a railing needs a path of at least two points",
        ));
    }
    let mut pts = path.to_vec();
    if closed {
        pts.push(path[0]);
    }
    let segs: Vec<([f64; 2], [f64; 2])> = pts.windows(2).map(|w| (w[0], w[1])).collect();
    let lengths: Vec<f64> = segs
        .iter()
        .map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
        .collect();
    if lengths.iter().any(|l| *l < 1e-6) {
        return Err(Error::new(
            "railing.short_path",
            "the railing's path has a segment with no length",
        ));
    }
    let plan = layout(&lengths, closed, t);
    // Each feature's gap on its segment: between the centres of the posts either side.
    let gaps = feature_gaps(&lengths, t, features)?;
    let mut parts = Vec::new();
    let (mut posts, mut balusters) = (0, 0);
    let top = z + t.height;
    let rail_under = top - t.top_rail[1];
    let bottom_top = z + t.bottom_rail_height + t.bottom_rail[1];
    for (i, ((a, b), (post_at, baluster_at))) in segs.iter().zip(&plan).enumerate() {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let at = |s: f64| [a[0] + u[0] * s, a[1] + u[1] * s];
        let here: Vec<(f64, f64)> = gaps
            .iter()
            .filter(|g| g.0 == i)
            .map(|g| (g.1, g.2))
            .collect();
        // Rails run between post centres, overlapping the posts, and stop at the gaps.
        let mut from = 0.0;
        let mut pieces = Vec::new();
        for (g0, g1) in &here {
            pieces.push((from, *g0));
            from = *g1;
        }
        pieces.push((from, len));
        for (p, q) in pieces.into_iter().filter(|(p, q)| q - p > 1e-6) {
            parts.push(bar(at(p), at(q), t.top_rail[0] / 2.0, rail_under, top));
            parts.push(bar(
                at(p),
                at(q),
                t.bottom_rail[0] / 2.0,
                z + t.bottom_rail_height,
                bottom_top,
            ));
        }
        let mut post_at = post_at.clone();
        for (g0, g1) in &here {
            post_at.extend([*g0, *g1]);
        }
        for s in &post_at {
            parts.push(column(at(*s), u, t.post / 2.0, z, top));
            posts += 1;
        }
        // Balusters keep out of the gaps and the gap posts' reserved space.
        for s in baluster_at.iter().filter(|s| {
            !here
                .iter()
                .any(|(g0, g1)| **s > g0 - t.post_space - 1e-9 && **s < g1 + t.post_space + 1e-9)
        }) {
            parts.push(column(at(*s), u, t.baluster / 2.0, bottom_top, rail_under));
            balusters += 1;
        }
    }
    Ok(RailingGeometry {
        path: path.to_vec(),
        closed,
        z,
        parts,
        posts,
        balusters,
        length: lengths.iter().sum(),
    })
}

/// Each feature's segment and gap (between its posts' centres) along that segment. A feature
/// must lie within one segment, clear of the corner posts, and not overlap another.
fn feature_gaps(
    lengths: &[f64],
    t: &RailingType,
    features: &[RailFeature],
) -> Result<Vec<(usize, f64, f64)>> {
    let mut gaps: Vec<(usize, f64, f64)> = Vec::new();
    for f in features {
        if f.width.is_nan() || f.width <= 0.2 {
            return Err(Error::new(
                "railing.feature_too_narrow",
                format!("opening {} must be over 200 mm wide", f.key),
            ));
        }
        let half = f.width / 2.0 + t.post / 2.0;
        let mut start = 0.0;
        let mut placed = None;
        for (i, len) in lengths.iter().enumerate() {
            let local = f.at - start;
            if local - half >= t.post + t.post_space - 1e-9
                && local + half <= len - t.post - t.post_space + 1e-9
            {
                placed = Some((i, local - half, local + half));
                break;
            }
            start += len;
        }
        let Some(g) = placed else {
            return Err(Error::new(
                "railing.feature_off_segment",
                format!(
                    "opening {} must lie within one straight run of the railing, clear of its corners",
                    f.key
                ),
            ));
        };
        if gaps
            .iter()
            .any(|o| o.0 == g.0 && o.1 < g.2 + t.post && g.1 < o.2 + t.post)
        {
            return Err(Error::new(
                "railing.feature_overlap",
                format!("opening {} overlaps another opening or gate", f.key),
            ));
        }
        gaps.push(g);
    }
    gaps.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).expect("finite"));
    Ok(gaps)
}

/// The distance along a path (closed: back to its start) of the point nearest to `p`.
pub fn distance_along(path: &[[f64; 2]], closed: bool, p: [f64; 2]) -> f64 {
    let mut pts = path.to_vec();
    if closed && !path.is_empty() {
        pts.push(path[0]);
    }
    let mut start = 0.0;
    let mut best = (f64::MAX, 0.0);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (len * len).max(1e-18))
            .clamp(0.0, 1.0);
        let q = [a[0] + d[0] * t, a[1] + d[1] * t];
        let dist = (p[0] - q[0]).hypot(p[1] - q[1]);
        if dist < best.0 {
            best = (dist, start + t * len);
        }
        start += len;
    }
    best.1
}

/// The leaves of a gate filling the gap between two post faces `a` and `b` (plan), at `z`: a
/// frame of two stiles, a top and a bottom rail, and balusters, per leaf (10 mm clear at the
/// posts and between leaves).
pub fn gate_parts(a: [f64; 2], b: [f64; 2], leaves: u8, z: f64, t: &RailingType) -> Vec<Mesh> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]).max(1e-9);
    let u = [d[0] / len, d[1] / len];
    let at = |s: f64| [a[0] + u[0] * s, a[1] + u[1] * s];
    let clear = 0.01;
    let n = f64::from(leaves.clamp(1, 2));
    let leaf = (len - clear * (n + 1.0)) / n;
    let stile = t.post * 0.6;
    let (bottom, top) = (z + 0.05, z + t.height - 0.05);
    let mut out = Vec::new();
    for k in 0..leaves.clamp(1, 2) {
        let s0 = clear + f64::from(k) * (leaf + clear);
        let s1 = s0 + leaf;
        out.push(column(at(s0 + stile / 2.0), u, stile / 2.0, bottom, top));
        out.push(column(at(s1 - stile / 2.0), u, stile / 2.0, bottom, top));
        out.push(bar(
            at(s0),
            at(s1),
            t.top_rail[0] / 2.0,
            top - t.top_rail[1],
            top,
        ));
        out.push(bar(
            at(s0),
            at(s1),
            t.bottom_rail[0] / 2.0,
            bottom,
            bottom + t.bottom_rail[1],
        ));
        let mut s = s0 + stile + t.baluster_spacing;
        while s < s1 - stile - t.baluster / 2.0 {
            out.push(column(
                at(s),
                u,
                t.baluster / 2.0,
                bottom + t.bottom_rail[1],
                top - t.top_rail[1],
            ));
            s += t.baluster_spacing;
        }
    }
    out
}

/// The parts of a gate element: its railing's feature, laid out now.
pub fn gate_geometry(store: &Store, gate: &Element) -> Result<Vec<Mesh>> {
    let (railing, key) =
        gate_of(gate).ok_or_else(|| Error::new("railing.not_a_gate", "not a railing gate"))?;
    let r = find(store, &railing)
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("railing.no_railing", "the gate's railing is gone"))?;
    let recipe = railing_recipe(r).expect("a railing");
    let f = recipe
        .features
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| Error::new("railing.no_feature", "the gate's opening is gone"))?;
    let FeatureKind::Gate { leaves, .. } = f.kind else {
        return Err(Error::new("railing.no_feature", "the opening has no gate"));
    };
    let (path, closed, z) = resolve_path(store, &recipe)?;
    let t = type_of(store, r);
    if !fitting(&path, closed, &t, &recipe.features).contains(f) {
        return Err(Error::new(
            "railing.feature_off_segment",
            "the gate no longer fits its railing",
        ));
    }
    let at = |s: f64| point_at(&path, closed, s);
    let a = at(f.at - f.width / 2.0);
    let b = at(f.at + f.width / 2.0);
    Ok(gate_parts(a, b, leaves, z, &t))
}

/// The point at distance `s` along a path.
fn point_at(path: &[[f64; 2]], closed: bool, s: f64) -> [f64; 2] {
    let mut pts = path.to_vec();
    if closed {
        pts.push(path[0]);
    }
    let mut left = s;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if left <= len {
            let t = left / len.max(1e-12);
            return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        }
        left -= len;
    }
    *pts.last().expect("a path")
}

pub const GATE_RECIPE: &str = "recipe.gate";

/// A gate element's railing (GlobalId) and feature key.
pub fn gate_of(e: &Element) -> Option<(String, u32)> {
    let v = e.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != GATE_RECIPE {
        return None;
    }
    Some((
        v.get("railing")?.as_str()?.to_owned(),
        u32::try_from(v.get("key")?.as_u64()?).ok()?,
    ))
}

/// A gate's IFC `OperationType`, overall width and height.
pub fn gate_operation(store: &Store, gate: &Element) -> Option<(&'static str, f64, f64)> {
    let (railing, key) = gate_of(gate)?;
    let r = store.get(find(store, &railing)?)?;
    let f = railing_recipe(r)?
        .features
        .into_iter()
        .find(|f| f.key == key)?;
    let FeatureKind::Gate {
        leaves,
        hinge_start,
        ..
    } = f.kind
    else {
        return None;
    };
    let op = match (leaves, hinge_start) {
        (2, _) => "DOUBLE_DOOR_SINGLE_SWING",
        (_, true) => "SINGLE_SWING_LEFT",
        _ => "SINGLE_SWING_RIGHT",
    };
    Some((op, f.width, type_of(store, r).height))
}

/// Every gate that regenerates.
pub fn all_gates(store: &Store) -> Vec<(ElementId, Vec<Mesh>)> {
    store
        .iter()
        .filter(|e| gate_of(e).is_some())
        .filter_map(|e| Some((e.id(), gate_geometry(store, e).ok()?)))
        .collect()
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A floor's, deck's or flat roof's top elevation.
fn host_top(store: &Store, host: &Element) -> Result<f64> {
    if crate::floors::slab_recipe(host).is_some() {
        return Ok(crate::floors::regenerate(store, host)?.top);
    }
    if crate::roofs::roof_recipe(host).is_some() {
        let g = crate::roofs::regenerate(store, host)?;
        if g.predefined != "FLAT_ROOF" {
            return Err(Error::new(
                "railing.sloped_host",
                "railings go on flat roofs (not sloped ones)",
            ));
        }
        return Ok(g.plate + g.thickness);
    }
    Err(Error::new(
        "railing.bad_host",
        "railings follow the edges of floors, decks and flat roofs",
    ))
}

/// Lines `lines` (a chain, in order) each moved `d` to their left, met at the corners.
fn inset(lines: &[Line], closed: bool, d: f64) -> Vec<[f64; 2]> {
    let moved: Vec<Line> = lines
        .iter()
        .map(|l| {
            let v = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
            let len = v[0].hypot(v[1]).max(1e-12);
            let n = [-v[1] / len * d, v[0] / len * d];
            Line {
                start: [l.start[0] + n[0], l.start[1] + n[1]],
                end: [l.end[0] + n[0], l.end[1] + n[1]],
            }
        })
        .collect();
    let meet = |a: &Line, b: &Line| -> [f64; 2] {
        let (u, v) = (
            [a.end[0] - a.start[0], a.end[1] - a.start[1]],
            [b.end[0] - b.start[0], b.end[1] - b.start[1]],
        );
        let den = u[0] * v[1] - u[1] * v[0];
        if den.abs() < 1e-12 {
            return a.end;
        }
        let w = [b.start[0] - a.start[0], b.start[1] - a.start[1]];
        let t = (w[0] * v[1] - w[1] * v[0]) / den;
        [a.start[0] + u[0] * t, a.start[1] + u[1] * t]
    };
    let n = moved.len();
    let mut pts = Vec::new();
    for i in 0..n {
        if i == 0 {
            pts.push(if closed {
                meet(&moved[n - 1], &moved[0])
            } else {
                moved[0].start
            });
        } else {
            pts.push(meet(&moved[i - 1], &moved[i]));
        }
    }
    if !closed {
        pts.push(moved[n - 1].end);
    }
    pts
}

/// A railing's path and height now.
pub fn resolve_path(store: &Store, r: &RailingRecipe) -> Result<(Vec<[f64; 2]>, bool, f64)> {
    match &r.path {
        RailPath::Free {
            points,
            closed,
            level,
            base_offset,
        } => {
            let z = find(store, level)
                .and_then(|l| store.get(l))
                .map(crate::levels::elevation)
                .ok_or_else(|| Error::new("railing.no_level", "the railing's level is gone"))?;
            Ok((points.clone(), *closed, z + base_offset))
        }
        RailPath::Host {
            host,
            keys,
            closed,
            setback,
        } => {
            let h = find(store, host)
                .and_then(|h| store.get(h))
                .ok_or_else(|| Error::new("railing.no_host", "the railing's host is gone"))?;
            let curves = crate::sketch_edit::current_curves(store, h)
                .ok_or_else(|| Error::new("railing.bad_host", "the host has no outline"))?;
            let lines: Vec<Line> = keys
                .iter()
                .map(|k| {
                    curves
                        .iter()
                        .find(|c| c.0 == *k)
                        .map(|c| c.1)
                        .ok_or_else(|| {
                            Error::new(
                                "railing.edge_gone",
                                format!("the host's edge {k} no longer exists"),
                            )
                        })
                })
                .collect::<Result<_>>()?;
            Ok((
                inset(&lines, *closed, *setback),
                *closed,
                host_top(store, h)?,
            ))
        }
    }
}

fn type_of(store: &Store, e: &Element) -> RailingType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| store.get(find(store, g)?))
        .and_then(railing_type)
        .unwrap_or_default()
}

/// Regenerates a railing.
pub fn regenerate(store: &Store, e: &Element) -> Result<RailingGeometry> {
    let r = railing_recipe(e)
        .ok_or_else(|| Error::new("railing.not_reviera", "not a Reviera railing"))?;
    let (path, closed, z) = resolve_path(store, &r)?;
    let t = type_of(store, e);
    generate(
        &path,
        closed,
        z,
        &t,
        &fitting(&path, closed, &t, &r.features),
    )
}

/// The features that still fit the path, in order: after the path changes, one that no
/// longer lies within a straight run, or overlaps one before it, is left out.
fn fitting(
    path: &[[f64; 2]],
    closed: bool,
    t: &RailingType,
    features: &[RailFeature],
) -> Vec<RailFeature> {
    let mut pts = path.to_vec();
    if closed && !path.is_empty() {
        pts.push(path[0]);
    }
    let lengths: Vec<f64> = pts
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .collect();
    let mut kept: Vec<RailFeature> = Vec::new();
    for f in features {
        kept.push(f.clone());
        if feature_gaps(&lengths, t, &kept).is_err() {
            kept.pop();
        }
    }
    kept
}

/// Every Reviera railing that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, RailingGeometry)> {
    store
        .iter()
        .filter(|e| railing_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// Whether a wall runs along `l` (parallel, its centreline within half its width and 150 mm).
fn walled(store: &Store, l: &Line) -> bool {
    let m = [(l.start[0] + l.end[0]) / 2.0, (l.start[1] + l.end[1]) / 2.0];
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let dl = d[0].hypot(d[1]).max(1e-12);
    crate::walls::solve(store).bodies.values().any(|b| {
        let g = &b.geometry;
        let u = g.direction();
        let parallel = ((u[0] * d[1] - u[1] * d[0]) / dl).abs() < 0.1;
        let w = [g.end[0] - g.start[0], g.end[1] - g.start[1]];
        let len2 = (w[0] * w[0] + w[1] * w[1]).max(1e-18);
        let t = ((m[0] - g.start[0]) * w[0] + (m[1] - g.start[1]) * w[1]) / len2;
        let near = if (0.0..=1.0).contains(&t) {
            let q = [g.start[0] + w[0] * t, g.start[1] + w[1] * t];
            (m[0] - q[0]).hypot(m[1] - q[1]) <= g.width / 2.0 + 0.15
        } else {
            false
        };
        parallel && near
    })
}

/// The open edges of a host's outline in chains: each loop's runs of edges with no wall along
/// them (a whole loop is a closed chain).
pub fn open_chains(store: &Store, host: &Element) -> Vec<(Vec<u32>, bool)> {
    let Some(sketch) = crate::sketch::sketch_of(host) else {
        return Vec::new();
    };
    let Some(curves) = crate::sketch_edit::current_curves(store, host) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for lp in &sketch.loops {
        let open: Vec<bool> = lp
            .iter()
            .map(|k| {
                curves
                    .iter()
                    .find(|c| c.0 == *k)
                    .is_some_and(|c| !walled(store, &c.1))
            })
            .collect();
        if open.iter().all(|o| *o) {
            out.push((lp.clone(), true));
            continue;
        }
        // Start after a walled edge, so chains are not split at the loop's seam.
        let n = lp.len();
        let first = (0..n).find(|i| !open[*i]).map_or(0, |i| (i + 1) % n);
        let mut chain = Vec::new();
        for k in 0..n {
            let i = (first + k) % n;
            if open[i] {
                chain.push(lp[i]);
            } else if !chain.is_empty() {
                out.push((std::mem::take(&mut chain), false));
            }
        }
        if !chain.is_empty() {
            out.push((chain, false));
        }
    }
    out
}

// ---- Commands ----

fn number(store: &Store, v: &Value, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, "length", &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "railing.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

/// The project's default railing type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| railing_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new(TYPE_CLASS)
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "GUARDRAIL")
            .param(
                RECIPE_KEY,
                serde_json::to_value(RailingType::default()).expect("serialises"),
            ),
    )
}

fn make(
    txn: &mut crate::txn::Transaction<'_>,
    type_id: Option<&ElementRef>,
    path: RailPath,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match type_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such railing type"))?;
            if !store.get(id).is_some_and(|e| railing_type(e).is_some()) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} is not a railing type"),
                ));
            }
            id
        }
        None => default_type(txn)?,
    };
    let recipe = RailingRecipe {
        kind: RAILING_RECIPE.into(),
        path,
        features: Vec::new(),
    };
    let (pts, closed, z) = resolve_path(txn.store(), &recipe)?;
    let t = txn
        .store()
        .get(type_id)
        .and_then(railing_type)
        .unwrap_or_default();
    generate(&pts, closed, z, &t, &[])?;
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    txn.create(
        NewElement::new(CLASS)
            .param("@PredefinedType", "GUARDRAIL")
            .param("@type", type_guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub type_id: Option<ElementRef>,
    /// The path's points in plan (metres).
    pub path: Vec<[f64; 2]>,
    #[serde(default)]
    pub closed: bool,
    pub base_level: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
}

/// A railing along a sketched path (Sketch Path).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = store
        .resolve(&p.base_level)
        .and_then(|l| store.get(l))
        .filter(|l| l.class == crate::levels::CLASS)
        .ok_or_else(|| Error::new("railing.no_level", "no such level"))?;
    let base_offset = p
        .base_offset
        .as_ref()
        .map(|v| number(&store, v, "the base offset"))
        .transpose()?
        .unwrap_or(0.0);
    make(
        txn,
        p.type_id.as_ref(),
        RailPath::Free {
            points: p.path,
            closed: p.closed,
            level: level.guid().to_string(),
            base_offset,
        },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateOnEdgesParams {
    pub type_id: Option<ElementRef>,
    /// A floor, deck or flat roof.
    pub host_id: ElementRef,
    /// Footprint curve keys, in order along one chain; omitted, every open edge (one railing per
    /// chain of edges with no wall along them).
    pub edges: Option<Vec<u32>>,
    /// How far in from the edge (default 50 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub setback: Option<Value>,
}

/// Railings along a host's edges (Pick Edges, Guard all open edges).
pub(crate) fn create_on_edges(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateOnEdgesParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let host = store
        .resolve(&p.host_id)
        .and_then(|h| store.get(h))
        .ok_or_else(|| Error::new("element.not_found", "no such host"))?;
    host_top(&store, host)?;
    let setback = p
        .setback
        .as_ref()
        .map(|v| number(&store, v, "the setback"))
        .transpose()?
        .unwrap_or(0.05);
    let chains = match p.edges {
        Some(keys) => {
            // A chain closes when it is a whole loop.
            let closed = crate::sketch::sketch_of(host).is_some_and(|s| {
                s.loops
                    .iter()
                    .any(|lp| lp.len() == keys.len() && lp.iter().all(|k| keys.contains(k)))
            });
            vec![(keys, closed)]
        }
        None => open_chains(&store, host),
    };
    if chains.is_empty() {
        return Err(Error::new(
            "railing.no_open_edges",
            "every edge of the host has a wall along it",
        ));
    }
    let guid = host.guid().to_string();
    chains
        .into_iter()
        .map(|(keys, closed)| {
            make(
                txn,
                p.type_id.as_ref(),
                RailPath::Host {
                    host: guid.clone(),
                    keys,
                    closed,
                    setback,
                },
            )
        })
        .collect()
}

// ---- Openings and gates (ARC-RAIL-003) ----

/// The gate element's class.
pub const GATE_CLASS: &str = "IfcDoor";

/// Where a feature goes: a distance along the path, or a plan point projected onto it.
fn feature_at(
    store: &Store,
    r: &RailingRecipe,
    at: Option<&Value>,
    point: Option<[f64; 2]>,
) -> Result<f64> {
    match (at, point) {
        (Some(v), _) => number(store, v, "the distance"),
        (None, Some(p)) => {
            let (path, closed, _) = resolve_path(store, r)?;
            Ok(distance_along(&path, closed, p))
        }
        (None, None) => Err(Error::new(
            "railing.no_position",
            "give `at` (a distance along the railing) or `point` (a plan point on it)",
        )),
    }
}

fn railing_of(store: &Store, r: &ElementRef) -> Result<(ElementId, RailingRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such railing"))?;
    let recipe = store.get(id).and_then(railing_recipe).ok_or_else(|| {
        Error::new(
            "railing.not_reviera",
            format!("element {id} is not a Reviera railing"),
        )
    })?;
    Ok((id, recipe))
}

/// Checks the features against the railing's path and type, and saves them.
fn save_features(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    mut r: RailingRecipe,
    features: Vec<RailFeature>,
) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    let (path, closed, z) = resolve_path(&store, &r)?;
    generate(&path, closed, z, &type_of(&store, e), &features)?;
    r.features = features;
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddOpeningParams {
    pub railing_id: ElementRef,
    /// The opening's centre: its distance from the path's start.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub at: Option<Value>,
    /// Or a plan point, projected onto the path.
    pub point: Option<[f64; 2]>,
    /// The clear width between the posts (default 900 mm, or 1 m for a gate).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
}

fn add_feature(
    txn: &mut crate::txn::Transaction<'_>,
    p: &AddOpeningParams,
    gate: Option<(u8, bool, bool)>,
) -> Result<(ElementId, u32)> {
    let store = txn.store().clone();
    let (id, r) = railing_of(&store, &p.railing_id)?;
    let at = feature_at(&store, &r, p.at.as_ref(), p.point)?;
    let width = p
        .width
        .as_ref()
        .map(|v| number(&store, v, "the width"))
        .transpose()?
        .unwrap_or(if gate.is_some_and(|g| g.0 == 2) {
            1.2
        } else if gate.is_some() {
            1.0
        } else {
            0.9
        });
    let key = r.features.iter().map(|f| f.key).max().unwrap_or(0) + 1;
    let kind = match gate {
        None => FeatureKind::Opening,
        Some((leaves, swing_out, hinge_start)) => FeatureKind::Gate {
            leaves,
            swing_out,
            hinge_start,
            gate: String::new(),
        },
    };
    let mut features = r.features.clone();
    features.push(RailFeature {
        key,
        at,
        width,
        kind,
    });
    save_features(txn, id, r, features)?;
    Ok((id, key))
}

/// Adds an opening to a railing: a post each side, the rails and balusters stopping there.
pub(crate) fn add_opening(
    txn: &mut crate::txn::Transaction<'_>,
    p: AddOpeningParams,
) -> Result<u32> {
    add_feature(txn, &p, None).map(|(_, key)| key)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddGateParams {
    pub railing_id: ElementRef,
    /// The gate's centre: its distance from the path's start.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub at: Option<Value>,
    /// Or a plan point, projected onto the path.
    pub point: Option<[f64; 2]>,
    /// The clear width between the posts (default 1 m single, 1.2 m double).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    /// 1 (single, the default) or 2 (double).
    pub leaves: Option<u8>,
    /// Swings out, to the right of the path's direction (default: in).
    #[serde(default)]
    pub swing_out: bool,
    /// A single leaf's hinge at the end side (default: the start side).
    #[serde(default)]
    pub hinge_end: bool,
}

/// Adds a gate: an opening filled with one or two leaves, its own `IfcDoor` (GATE) element.
pub(crate) fn add_gate(
    txn: &mut crate::txn::Transaction<'_>,
    p: AddGateParams,
) -> Result<ElementId> {
    let leaves = p.leaves.unwrap_or(1);
    if !(1..=2).contains(&leaves) {
        return Err(Error::new(
            "railing.bad_leaves",
            "a gate has one leaf or two",
        ));
    }
    let opening = AddOpeningParams {
        railing_id: p.railing_id,
        at: p.at,
        point: p.point,
        width: p.width,
    };
    let (id, key) = add_feature(txn, &opening, Some((leaves, p.swing_out, !p.hinge_end)))?;
    let railing = txn.store().get(id).expect("exists").guid().to_string();
    let gate = txn.create(
        NewElement::new(GATE_CLASS)
            .name(if leaves == 2 { "Double Gate" } else { "Gate" })
            .param("@PredefinedType", "GATE")
            .param(
                RECIPE_KEY,
                serde_json::json!({ "kind": GATE_RECIPE, "railing": railing, "key": key }),
            ),
    )?;
    let guid = txn.store().get(gate).expect("created").guid().to_string();
    let mut r = railing_recipe(txn.store().get(id).expect("exists")).expect("a railing");
    for f in &mut r.features {
        if f.key == key
            && let FeatureKind::Gate { gate, .. } = &mut f.kind
        {
            *gate = guid.clone();
        }
    }
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    Ok(gate)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureParams {
    pub railing_id: ElementRef,
    /// The opening's or gate's key (from `railing.features`).
    pub key: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveFeatureParams {
    pub railing_id: ElementRef,
    pub key: u32,
    /// The new centre: a distance along the path, or a plan point projected onto it.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub at: Option<Value>,
    pub point: Option<[f64; 2]>,
    /// A new clear width.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
}

/// Moves or resizes an opening or gate along its railing.
pub(crate) fn move_feature(
    txn: &mut crate::txn::Transaction<'_>,
    p: MoveFeatureParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, r) = railing_of(&store, &p.railing_id)?;
    let mut features = r.features.clone();
    let i = features
        .iter()
        .position(|f| f.key == p.key)
        .ok_or_else(|| no_feature(p.key))?;
    if p.at.is_some() || p.point.is_some() {
        features[i].at = feature_at(&store, &r, p.at.as_ref(), p.point)?;
    }
    if let Some(w) = &p.width {
        features[i].width = number(&store, w, "the width")?;
    }
    save_features(txn, id, r, features)
}

fn no_feature(key: u32) -> Error {
    Error::new(
        "railing.no_feature",
        format!("the railing has no opening or gate {key}"),
    )
}

/// Removes an opening or gate (and the gate's element); the railing closes over it.
pub(crate) fn remove_feature(
    txn: &mut crate::txn::Transaction<'_>,
    p: FeatureParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, r) = railing_of(&store, &p.railing_id)?;
    let mut features = r.features.clone();
    let i = features
        .iter()
        .position(|f| f.key == p.key)
        .ok_or_else(|| no_feature(p.key))?;
    let f = features.remove(i);
    if let FeatureKind::Gate { gate, .. } = &f.kind
        && let Some(g) = find(&store, gate)
    {
        txn.delete(g)?;
    }
    save_features(txn, id, r, features)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeaturesParams {
    pub railing_id: ElementRef,
}

/// A railing's openings and gates.
pub fn features(store: &Store, p: FeaturesParams) -> Result<Vec<RailFeature>> {
    railing_of(store, &p.railing_id).map(|(_, r)| r.features)
}

/// Moves, turns or mirrors a free railing's path (hosted railings follow their host).
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = railing_recipe(store.get(id).expect("exists")).expect("a railing");
    match &mut r.path {
        RailPath::Free { points, .. } => {
            for p in points.iter_mut() {
                *p = point(*p);
            }
        }
        RailPath::Host { .. } => {
            return Err(Error::new(
                "railing.hosted",
                "a railing on a host's edges follows the host; move the host",
            ));
        }
    }
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_specified_baluster_count() {
        // ARC-RAIL-002 acceptance: a 3.05 m path, 0.12 m pattern, posts reserving 0.05 m: the
        // baluster at 3.00 falls in the end post's space, leaving 24.
        let t = RailingType::default();
        let plan = layout(&[3.05], false, &t);
        assert_eq!(plan[0].0, vec![0.0, 3.05]);
        assert_eq!(plan[0].1.len(), 24);
        assert!((plan[0].1[0] - 0.12).abs() < 1e-12);
        assert!((plan[0].1[23] - 2.88).abs() < 1e-9);
        let g = generate(&[[0.0, 0.0], [3.05, 0.0]], false, 0.0, &t, &[]).unwrap();
        assert_eq!((g.posts, g.balusters), (2, 24));
        // Two rails, two posts, 24 balusters.
        assert_eq!(g.parts.len(), 28);
    }

    #[test]
    fn a_closed_path_has_a_post_at_every_corner() {
        let t = RailingType::default();
        let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let g = generate(&sq, true, 1.0, &t, &[]).unwrap();
        assert_eq!(g.posts, 4);
        assert!((g.length - 8.0).abs() < 1e-12);
        let top = g
            .parts
            .iter()
            .flat_map(|m| m.0.iter())
            .map(|p| p[2])
            .fold(f64::MIN, f64::max);
        assert!((top - 2.0).abs() < 1e-12);
    }
}
