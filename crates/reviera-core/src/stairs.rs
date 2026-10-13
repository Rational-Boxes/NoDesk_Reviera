// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stairs by component (spec Ch 3 §3.7, ARC-STAIR-001…005): straight runs joined by automatic
//! landings, between a base level and a top level (or an unconnected height).
//!
//! - **Calculation (ARC-STAIR-003).** The height `H` and the type's maximum riser height give
//!   the minimum riser count `N = ceil(H / R_max)`, or the desired count when set; the actual
//!   riser height is `H / N`. A run of `k` risers that begins and ends with a riser has `k − 1`
//!   treads of the actual tread depth.
//! - **Runs** are stored by their start, direction, width and riser count, so a run keeps its
//!   steps when the levels move. Runs whose risers do not add up to `N` are reported.
//! - **Geometry (ARC-STAIR-004).** A run is cast in place (monolithic): its side profile of
//!   risers and treads, with a smooth underside parallel to the pitch line at the type's
//!   structural depth, extruded across its width. A landing between two runs is the convex hull
//!   of the first run's end edge and the second's start edge (extended by the run width for
//!   turns), at the shared height, of the type's landing thickness.
//!
//! The stair is an `IfcStair` with an `IfcStairType`; its runs and landings are the stair's own
//! body for now (aggregated `IfcStairFlight` and `IfcSlab` parts come later).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const CLASS: &str = "IfcStair";
pub const TYPE_CLASS: &str = "IfcStairType";
pub const STAIR_RECIPE: &str = "recipe.stair";
pub const STAIR_TYPE_RECIPE: &str = "recipe.stair_type";
pub const DEFAULT_TYPE_NAME: &str = "Cast-In-Place Stair";

/// A closed mesh: positions and triangles (outward).
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// The calculation rules and construction of a stair type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StairType {
    pub kind: String,
    pub max_riser: f64,
    pub min_tread: f64,
    pub min_width: f64,
    /// The depth of a monolithic run below its pitch line, measured square to it.
    pub structural_depth: f64,
    pub landing_thickness: f64,
}

impl Default for StairType {
    fn default() -> Self {
        Self {
            kind: STAIR_TYPE_RECIPE.into(),
            max_riser: 0.18,
            min_tread: 0.28,
            min_width: 1.0,
            structural_depth: 0.15,
            landing_thickness: 0.2,
        }
    }
}

/// One run: straight, or a spiral about a centre.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Run {
    /// The start of the run's path (plan, metres): the middle of its first riser.
    pub start: [f64; 2],
    /// The up direction at the start (unit): a spiral's tangent there.
    pub direction: [f64; 2],
    pub width: f64,
    pub risers: u32,
    /// A spiral run turns about `center`; its path (the walking line) is the circle through
    /// `start`, and each tread is the tread depth along it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spiral: Option<Spiral>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Spiral {
    pub center: [f64; 2],
    /// Turning counter-clockwise going up.
    pub ccw: bool,
}

/// What a stair reaches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StairTop {
    /// A level (GlobalId).
    Level(String),
    /// A height above the base, metres.
    Unconnected(f64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StairRecipe {
    pub kind: String,
    pub base_level: String,
    #[serde(default)]
    pub base_offset: f64,
    pub top: StairTop,
    #[serde(default)]
    pub top_offset: f64,
    /// The desired number of risers; `None` takes the fewest the type allows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desired_risers: Option<u32>,
    pub tread_depth: f64,
    pub runs: Vec<Run>,
}

pub fn stair_recipe(e: &Element) -> Option<StairRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == STAIR_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn stair_type(e: &Element) -> Option<StairType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == STAIR_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A stair as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct StairGeometry {
    pub z_base: f64,
    pub height: f64,
    pub risers: u32,
    pub riser_height: f64,
    /// The riser count the runs add up to.
    pub placed: u32,
    pub tread_depth: f64,
    /// Each run's and landing's solid.
    pub runs: Vec<Mesh>,
    pub landings: Vec<Mesh>,
    /// The tread lines of each run in plan (nosings), for drawing.
    pub treads: Vec<[[f64; 2]; 2]>,
    /// Problems to report (riser height above the maximum, risers not all placed, …).
    pub warnings: Vec<String>,
    /// `IfcStair.PredefinedType`.
    pub predefined: &'static str,
}

impl StairGeometry {
    pub fn treads(&self) -> u32 {
        self.placed.saturating_sub(self.runs.len() as u32)
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn elevation(store: &Store, guid: &str) -> Option<f64> {
    store.get(find(store, guid)?)?.param("@Elevation")?.as_f64()
}

/// The fewest risers that keep each at most `max_riser` over `height`.
pub fn min_risers(height: f64, max_riser: f64) -> u32 {
    ((height / max_riser) - 1e-9).ceil().max(1.0) as u32
}

/// The risers a straight run of plan length `length` holds at tread depth `tread`: one more
/// than its treads.
pub fn risers_for_length(length: f64, tread: f64) -> u32 {
    (length / tread).round().max(0.0) as u32 + 1
}

/// A closed prism: the polygon `poly` (in the plane of `u` and `v` through `origin`) swept by
/// `d`. The polygon may run either way round.
fn prism(poly: &[[f64; 2]], origin: [f64; 3], u: [f64; 3], v: [f64; 3], d: [f64; 3]) -> Mesh {
    let area: f64 = (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    // Counter-clockwise in (u, v) with u × v along d.
    let uv = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let along = uv[0] * d[0] + uv[1] * d[1] + uv[2] * d[2] > 0.0;
    let mut poly = poly.to_vec();
    if (area > 0.0) != along {
        poly.reverse();
    }
    let at = |p: [f64; 2]| {
        [
            origin[0] + u[0] * p[0] + v[0] * p[1],
            origin[1] + u[1] * p[0] + v[1] * p[1],
            origin[2] + u[2] * p[0] + v[2] * p[1],
        ]
    };
    let n = poly.len();
    let mut positions: Vec<[f64; 3]> = poly.iter().map(|p| at(*p)).collect();
    let top: Vec<[f64; 3]> = positions
        .iter()
        .map(|p| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
        .collect();
    positions.extend(top);
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(poly.iter().copied(), &[] as &[u32], &mut tris);
    let nn = n as u32;
    // Triangles counter-clockwise in the polygon's own orientation.
    let ccw_in = |t: &[u32]| {
        let (a, b, c) = (
            poly[t[0] as usize],
            poly[t[1] as usize],
            poly[t[2] as usize],
        );
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0
    };
    let sign = if along { 1.0 } else { -1.0 };
    let mut indices = Vec::new();
    for t in tris.chunks(3) {
        let (a, b, c) = if ccw_in(t) == (sign > 0.0) {
            (t[0], t[1], t[2])
        } else {
            (t[0], t[2], t[1])
        };
        // The base faces away from d, the far cap toward it.
        indices.extend([a, c, b]);
        indices.extend([nn + a, nn + b, nn + c]);
    }
    for i in 0..nn {
        let j = (i + 1) % nn;
        indices.extend([i, j, nn + j, i, nn + j, nn + i]);
    }
    (positions, indices)
}

/// The side profile of a monolithic run of `k` risers in its (s, z) plane: steps up from
/// (0, 0), and the underside parallel to the pitch line at `depth`; cut at the floor
/// (`on_floor`) or else hanging below the run's start.
fn run_profile(k: u32, riser: f64, tread: f64, depth: f64, on_floor: bool) -> Vec<[f64; 2]> {
    let k = k.max(1);
    if k == 1 {
        return vec![[0.0, 0.0], [tread, 0.0], [tread, riser], [0.0, riser]];
    }
    let mut steps = vec![[0.0, 0.0]];
    for i in 0..k {
        let s = f64::from(i) * tread;
        steps.push([s, f64::from(i + 1) * riser]);
        if i + 1 < k {
            steps.push([s + tread, f64::from(i + 1) * riser]);
        }
    }
    // The pitch line through the nosings, z = riser + s · slope; the underside below it.
    let slope = riser / tread;
    let drop = depth * (1.0 + slope * slope).sqrt();
    let under = |s: f64| riser + s * slope - drop;
    let end = f64::from(k - 1) * tread;
    let mut out = steps;
    out.push([end, under(end)]);
    if on_floor && under(0.0) < 0.0 {
        let s0 = (drop - riser) / slope;
        if s0 < end {
            out.push([s0, 0.0]);
        } else {
            out.push([end, 0.0]);
        }
    } else {
        out.push([0.0, under(0.0)]);
    }
    // Drop repeated points.
    out.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
    out
}

/// The convex hull of points (counter-clockwise).
fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 1e-12
        {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 1e-12
        {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn left(d: [f64; 2]) -> [f64; 2] {
    [-d[1], d[0]]
}

impl Run {
    /// The run's plan length along its path at tread depth `tread`.
    pub fn length(&self, tread: f64) -> f64 {
        f64::from(self.risers.saturating_sub(1)) * tread
    }

    /// A spiral's walking-line radius, start angle and turn per tread (signed).
    fn turn(&self, s: &Spiral, tread: f64) -> (f64, f64, f64) {
        let d = [self.start[0] - s.center[0], self.start[1] - s.center[1]];
        let r = d[0].hypot(d[1]).max(1e-6);
        let sign = if s.ccw { 1.0 } else { -1.0 };
        (r, d[1].atan2(d[0]), sign * tread / r)
    }

    /// The point of a spiral's walking line at angle `a`.
    fn on_circle(s: &Spiral, r: f64, a: f64) -> [f64; 2] {
        [s.center[0] + r * a.cos(), s.center[1] + r * a.sin()]
    }

    /// The end of the run's path.
    pub fn end(&self, tread: f64) -> [f64; 2] {
        if let Some(s) = &self.spiral {
            let (r, a0, step) = self.turn(s, tread);
            return Self::on_circle(s, r, a0 + step * f64::from(self.risers.saturating_sub(1)));
        }
        let l = self.length(tread);
        [
            self.start[0] + self.direction[0] * l,
            self.start[1] + self.direction[1] * l,
        ]
    }

    /// The up direction at the end of the path.
    pub fn end_direction(&self, tread: f64) -> [f64; 2] {
        match &self.spiral {
            Some(s) => {
                let (_, a0, step) = self.turn(s, tread);
                let a = a0 + step * f64::from(self.risers.saturating_sub(1));
                let k = if s.ccw { 1.0 } else { -1.0 };
                [-a.sin() * k, a.cos() * k]
            }
            None => self.direction,
        }
    }

    /// The edge across the run at its start and at its end.
    fn start_edge(&self) -> [[f64; 2]; 2] {
        self.edge_at(self.start, self.direction)
    }

    fn end_edge(&self, tread: f64) -> [[f64; 2]; 2] {
        self.edge_at(self.end(tread), self.end_direction(tread))
    }

    fn edge_at(&self, p: [f64; 2], d: [f64; 2]) -> [[f64; 2]; 2] {
        let n = left(d);
        let h = self.width / 2.0;
        [
            [p[0] + n[0] * h, p[1] + n[1] * h],
            [p[0] - n[0] * h, p[1] - n[1] * h],
        ]
    }
}

/// The stair from its recipe, levels and type.
pub fn generate(r: &StairRecipe, z_base: f64, z_top: f64, t: &StairType) -> Result<StairGeometry> {
    let height = z_top - z_base;
    if height <= 0.01 {
        return Err(Error::new(
            "stair.no_height",
            "the stair's top must be above its base",
        ));
    }
    if r.runs.is_empty() {
        return Err(Error::new(
            "stair.no_runs",
            "a stair needs at least one run",
        ));
    }
    if r.tread_depth.is_nan() || r.tread_depth <= 0.0 {
        return Err(Error::new(
            "stair.bad_tread",
            "the tread depth must be positive",
        ));
    }
    let n = r
        .desired_risers
        .unwrap_or_else(|| min_risers(height, t.max_riser))
        .max(1);
    let riser = height / f64::from(n);
    let mut warnings = Vec::new();
    if riser > t.max_riser + 1e-9 {
        warnings.push(format!(
            "Riser height {:.0} mm exceeds the type's maximum of {:.0} mm",
            riser * 1000.0,
            t.max_riser * 1000.0
        ));
    }
    if r.tread_depth < t.min_tread - 1e-9 {
        warnings.push(format!(
            "Tread depth {:.0} mm is below the type's minimum of {:.0} mm",
            r.tread_depth * 1000.0,
            t.min_tread * 1000.0
        ));
    }
    let placed: u32 = r.runs.iter().map(|run| run.risers).sum();
    if placed < n {
        warnings.push(format!(
            "Not enough risers placed: {placed} of {n} (the top is not reached)"
        ));
    } else if placed > n {
        warnings.push(format!("{placed} risers placed where {n} are needed"));
    }
    let mut runs = Vec::new();
    let mut landings = Vec::new();
    let mut treads = Vec::new();
    let mut done = 0u32;
    for (i, run) in r.runs.iter().enumerate() {
        let len = run.direction[0].hypot(run.direction[1]);
        if len < 1e-9 || run.risers == 0 || run.width <= 0.0 {
            return Err(Error::new(
                "stair.bad_run",
                format!("run {} needs a direction, risers and a width", i + 1),
            ));
        }
        if run.width < t.min_width - 1e-9 {
            warnings.push(format!(
                "Run {} is {:.0} mm wide, below the minimum of {:.0} mm",
                i + 1,
                run.width * 1000.0,
                t.min_width * 1000.0
            ));
        }
        let z0 = z_base + f64::from(done) * riser;
        // A landing from the previous run to this one, at this run's base.
        if i > 0 {
            let prev = r.runs[i - 1];
            let a = prev.end_edge(r.tread_depth);
            let b = run.start_edge();
            let pd = prev.end_direction(r.tread_depth);
            let turn = (pd[0] * run.direction[1] - pd[1] * run.direction[0]).abs() > 1e-6
                || pd[0] * run.direction[0] + pd[1] * run.direction[1] < 0.0;
            let mut pts = vec![a[0], a[1], b[0], b[1]];
            if turn {
                let w = prev.width.max(run.width);
                let (u, v) = (pd, run.direction);
                pts.extend(a.iter().map(|p| [p[0] + u[0] * w, p[1] + u[1] * w]));
                pts.extend(b.iter().map(|p| [p[0] - v[0] * w, p[1] - v[1] * w]));
            }
            let outline = hull(pts);
            if outline.len() >= 3 {
                landings.push(prism(
                    &outline,
                    [0.0, 0.0, z0 - t.landing_thickness],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, t.landing_thickness],
                ));
            }
        }
        if let Some(sp) = &run.spiral {
            let (steps, lines) =
                spiral_steps(run, sp, z0, riser, r.tread_depth, t.structural_depth);
            runs.extend(steps);
            treads.extend(lines);
            done += run.risers;
            continue;
        }
        let profile = run_profile(run.risers, riser, r.tread_depth, t.structural_depth, i == 0);
        let u = [run.direction[0], run.direction[1], 0.0];
        let n_left = left(run.direction);
        let half = run.width / 2.0;
        let origin = [
            run.start[0] + n_left[0] * half,
            run.start[1] + n_left[1] * half,
            z0,
        ];
        runs.push(prism(
            &profile,
            origin,
            u,
            [0.0, 0.0, 1.0],
            [-n_left[0] * run.width, -n_left[1] * run.width, 0.0],
        ));
        for k in 0..run.risers.saturating_sub(1) {
            let s = f64::from(k + 1) * r.tread_depth;
            let p = [
                run.start[0] + run.direction[0] * s,
                run.start[1] + run.direction[1] * s,
            ];
            treads.push(run.edge_at(p, run.direction));
        }
        treads.push(run.start_edge());
        done += run.risers;
    }
    let predefined = classify(&r.runs);
    Ok(StairGeometry {
        z_base,
        height,
        risers: n,
        riser_height: riser,
        placed,
        tread_depth: r.tread_depth,
        runs,
        landings,
        treads,
        warnings,
        predefined,
    })
}

/// A spiral run's steps: each tread an annular sector (a wedge where the inner radius reaches
/// the centre), as a solid from its top down by the riser height and the structural depth (the
/// first stands on the floor); and the radial tread lines.
fn spiral_steps(
    run: &Run,
    s: &Spiral,
    z0: f64,
    riser: f64,
    tread: f64,
    depth: f64,
) -> (Vec<Mesh>, Vec<[[f64; 2]; 2]>) {
    let (r, a0, step) = run.turn(s, tread);
    let (r_in, r_out) = ((r - run.width / 2.0).max(0.0), r + run.width / 2.0);
    let k = run.risers.max(1);
    let at = |rad: f64, a: f64| [s.center[0] + rad * a.cos(), s.center[1] + rad * a.sin()];
    let mut solids = Vec::new();
    let mut lines = Vec::new();
    // k risers, k − 1 treads: tread i spans the angle from i · step to (i + 1) · step and stands
    // at the top of riser i + 1. A single riser gets one tread.
    let treads = k.saturating_sub(1).max(1);
    for i in 0..treads {
        let (a, b) = (a0 + step * f64::from(i), a0 + step * f64::from(i + 1));
        let segs = ((b - a).abs().to_degrees() / 5.0).ceil().max(2.0) as usize;
        let mut poly: Vec<[f64; 2]> = (0..=segs)
            .map(|j| at(r_out, a + (b - a) * j as f64 / segs as f64))
            .collect();
        if r_in < 1e-3 {
            poly.push(s.center);
        } else {
            poly.extend(
                (0..=segs)
                    .rev()
                    .map(|j| at(r_in, a + (b - a) * j as f64 / segs as f64)),
            );
        }
        let top = z0 + riser * f64::from(i + 1);
        let bottom = (top - riser - depth).max(z0);
        solids.push(prism(
            &poly,
            [0.0, 0.0, bottom],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, top - bottom],
        ));
        lines.push([at(r_in, a), at(r_out, a)]);
    }
    let end = a0 + step * f64::from(treads);
    lines.push([at(r_in, end), at(r_out, end)]);
    (solids, lines)
}

/// `IfcStair.PredefinedType` from the runs' arrangement.
fn classify(runs: &[Run]) -> &'static str {
    if runs.iter().any(|r| r.spiral.is_some()) {
        return if runs.len() == 1 {
            "SPIRAL_STAIR"
        } else {
            "USERDEFINED"
        };
    }
    match runs {
        [_] => "STRAIGHT_RUN_STAIR",
        [a, b] => {
            let dot = a.direction[0] * b.direction[0] + a.direction[1] * b.direction[1];
            if dot > 0.999 {
                "TWO_STRAIGHT_RUN_STAIR"
            } else if dot < -0.999 {
                "HALF_TURN_STAIR"
            } else if dot.abs() < 0.001 {
                "QUARTER_TURN_STAIR"
            } else {
                "USERDEFINED"
            }
        }
        _ => "USERDEFINED",
    }
}

/// The base and top elevations of a stair.
fn heights(store: &Store, r: &StairRecipe) -> Result<(f64, f64)> {
    let base = elevation(store, &r.base_level)
        .ok_or_else(|| Error::new("stair.no_level", "the stair's base level does not exist"))?
        + r.base_offset;
    let top = match &r.top {
        StairTop::Level(g) => {
            elevation(store, g).ok_or_else(|| {
                Error::new("stair.no_level", "the stair's top level does not exist")
            })? + r.top_offset
        }
        StairTop::Unconnected(h) => base + h,
    };
    Ok((base, top))
}

/// The type of a stair element.
fn type_of(store: &Store, e: &Element) -> StairType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| store.get(find(store, g)?))
        .and_then(stair_type)
        .unwrap_or_default()
}

/// Regenerates a stair from its recipe, levels and type.
pub fn regenerate(store: &Store, e: &Element) -> Result<StairGeometry> {
    let r =
        stair_recipe(e).ok_or_else(|| Error::new("stair.not_reviera", "not a Reviera stair"))?;
    let (base, top) = heights(store, &r)?;
    generate(&r, base, top, &type_of(store, e))
}

/// Every Reviera stair that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, StairGeometry)> {
    store
        .iter()
        .filter(|e| stair_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The stair height and type rules for a new stair: its riser count and riser height.
pub fn plan_risers(
    store: &Store,
    base_level: ElementId,
    top: Option<ElementId>,
    height: Option<f64>,
    type_id: Option<ElementId>,
) -> Option<(u32, f64)> {
    let z0 = store.get(base_level)?.param("@Elevation")?.as_f64()?;
    let h = match (top, height) {
        (Some(t), _) => store.get(t)?.param("@Elevation")?.as_f64()? - z0,
        (None, Some(h)) => h,
        (None, None) => return None,
    };
    let t = type_id
        .and_then(|t| store.get(t))
        .and_then(stair_type)
        .or_else(|| {
            store
                .iter()
                .find(|e| e.name.as_deref() == Some(DEFAULT_TYPE_NAME))
                .and_then(stair_type)
        })
        .unwrap_or_default();
    let n = min_risers(h, t.max_riser);
    Some((n, h / f64::from(n)))
}

// ---- Commands ----

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "stair.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<String> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != crate::levels::CLASS {
        return Err(Error::new(
            "stair.not_a_level",
            format!("element {id} is not a level"),
        ));
    }
    Ok(e.guid().to_string())
}

/// The project's default stair type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| stair_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new(TYPE_CLASS)
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "NOTDEFINED")
            .param(
                RECIPE_KEY,
                serde_json::to_value(StairType::default()).expect("serialises"),
            ),
    )
}

/// The top of a stair: a level, or `{"unconnected": height}`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopArg {
    Level(ElementRef),
    /// Metres, or text with units.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    Unconnected(Value),
}

/// A run: straight from the middle of its first riser toward `end`; or, with `center`, a spiral
/// about it whose walking line passes through `start`, turning toward `end` (or placing every
/// remaining riser when `end` is omitted: a full-step spiral).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunArg {
    pub start: [f64; 2],
    /// Required for a straight run.
    pub end: Option<[f64; 2]>,
    /// A spiral's centre.
    pub center: Option<[f64; 2]>,
    /// A spiral turns counter-clockwise going up (default true).
    pub ccw: Option<bool>,
    /// Default the stair's width.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    /// Default: as many as the run's length holds at the tread depth, at most those left.
    pub risers: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub type_id: Option<ElementRef>,
    pub base_level: ElementRef,
    /// Default the level above the base level.
    pub top: Option<TopArg>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<Value>,
    pub desired_risers: Option<u32>,
    /// Default the type's minimum tread depth.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tread_depth: Option<Value>,
    /// The width of runs that give none (default 1.2 m).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    /// Straight runs in order up the stair; an automatic landing joins each to the next.
    pub runs: Vec<RunArg>,
}

/// The level above `level` (by elevation).
fn level_above(store: &Store, level: &str) -> Option<String> {
    let z = elevation(store, level)?;
    store
        .iter()
        .filter(|e| e.class == crate::levels::CLASS)
        .filter_map(|e| Some((e.guid().to_string(), e.param("@Elevation")?.as_f64()?)))
        .filter(|(_, e)| *e > z + 1e-6)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(g, _)| g)
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let base_level = resolve_level(&store, &p.base_level)?;
    let num = |v: &Option<Value>, t: &str, what: &str| {
        v.as_ref().map(|v| number(&store, v, t, what)).transpose()
    };
    let top = match &p.top {
        Some(TopArg::Level(l)) => StairTop::Level(resolve_level(&store, l)?),
        Some(TopArg::Unconnected(h)) => {
            StairTop::Unconnected(number(&store, h, "length", "the stair height")?)
        }
        None => StairTop::Level(level_above(&store, &base_level).ok_or_else(|| {
            Error::new(
                "stair.no_top",
                "there is no level above; give `top` as a level or an unconnected height",
            )
        })?),
    };
    let type_id = match &p.type_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such stair type"))?;
            if !store.get(id).is_some_and(|e| stair_type(e).is_some()) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} is not a stair type"),
                ));
            }
            id
        }
        None => default_type(txn)?,
    };
    let t = txn
        .store()
        .get(type_id)
        .and_then(stair_type)
        .unwrap_or_default();
    let tread = num(&p.tread_depth, "length", "the tread depth")?.unwrap_or(t.min_tread);
    let width = num(&p.width, "length", "the width")?.unwrap_or(1.2);
    let mut recipe = StairRecipe {
        kind: STAIR_RECIPE.into(),
        base_level,
        base_offset: num(&p.base_offset, "length", "the base offset")?.unwrap_or(0.0),
        top,
        top_offset: num(&p.top_offset, "length", "the top offset")?.unwrap_or(0.0),
        desired_risers: p.desired_risers,
        tread_depth: tread,
        runs: Vec::new(),
    };
    let (z0, z1) = heights(&store, &recipe)?;
    let n = p
        .desired_risers
        .unwrap_or_else(|| min_risers(z1 - z0, t.max_riser));
    let mut left_over = n;
    for (i, a) in p.runs.iter().enumerate() {
        let width = num(&a.width, "length", "the run width")?.unwrap_or(width);
        if let Some(c) = a.center {
            let d = [a.start[0] - c[0], a.start[1] - c[1]];
            let rad = d[0].hypot(d[1]);
            if rad < width / 2.0 + 0.05 {
                return Err(Error::new(
                    "stair.spiral_too_tight",
                    format!(
                        "run {}: the walking line's radius must be over half the width plus 50 mm",
                        i + 1
                    ),
                ));
            }
            let ccw = a.ccw.unwrap_or(true);
            let sign = if ccw { 1.0 } else { -1.0 };
            // The turn toward `end` (in the run's direction), else all that is left.
            let risers = match (a.risers, a.end) {
                (Some(k), _) => k,
                (None, Some(e)) => {
                    let a0 = d[1].atan2(d[0]);
                    let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
                    let mut turn = (a1 - a0) * sign;
                    while turn <= 1e-9 {
                        turn += std::f64::consts::TAU;
                    }
                    risers_for_length(turn * rad, tread).min(left_over.max(1))
                }
                (None, None) => left_over.max(1),
            };
            left_over = left_over.saturating_sub(risers);
            let u = [d[0] / rad, d[1] / rad];
            recipe.runs.push(Run {
                start: a.start,
                direction: [-u[1] * sign, u[0] * sign],
                width,
                risers,
                spiral: Some(Spiral { center: c, ccw }),
            });
            continue;
        }
        let end = a.end.ok_or_else(|| {
            Error::new(
                "stair.bad_run",
                format!("run {} needs an `end` (or a `center` for a spiral)", i + 1),
            )
        })?;
        let d = [end[0] - a.start[0], end[1] - a.start[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-6 {
            return Err(Error::new(
                "stair.bad_run",
                format!("run {} has no length", i + 1),
            ));
        }
        let risers = a
            .risers
            .unwrap_or_else(|| risers_for_length(len, tread).min(left_over.max(1)));
        left_over = left_over.saturating_sub(risers);
        recipe.runs.push(Run {
            start: a.start,
            direction: [d[0] / len, d[1] / len],
            width,
            risers,
            spiral: None,
        });
    }
    generate(&recipe, z0, z1, &t)?;
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let predefined = classify(&recipe.runs);
    txn.create(
        NewElement::new(CLASS)
            .param("@PredefinedType", predefined)
            .param("@type", type_guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )
}

fn resolve_stair(store: &Store, r: &ElementRef) -> Result<(ElementId, StairRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such stair"))?;
    let recipe = stair_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "stair.not_reviera",
            format!("element {id} is not a Reviera stair"),
        )
    })?;
    Ok((id, recipe))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub stair_ids: Vec<ElementRef>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    pub top: Option<TopArg>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<Value>,
    /// `0` returns to the fewest risers the type allows.
    pub desired_risers: Option<u32>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tread_depth: Option<Value>,
    /// The width of every run.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    /// With a new riser count or height: share the risers out among the runs again, the last
    /// run taking the difference (default on).
    pub rebalance: Option<bool>,
}

/// Sets stairs' levels, offsets, riser count, tread depth and run width (`stair.set_params`).
pub(crate) fn set_params(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let num = |v: &Option<Value>, t: &str, what: &str| {
        v.as_ref().map(|v| number(&store, v, t, what)).transpose()
    };
    let base_level = p
        .base_level
        .as_ref()
        .map(|l| resolve_level(&store, l))
        .transpose()?;
    let top = match &p.top {
        Some(TopArg::Level(l)) => Some(StairTop::Level(resolve_level(&store, l)?)),
        Some(TopArg::Unconnected(h)) => Some(StairTop::Unconnected(number(
            &store,
            h,
            "length",
            "the stair height",
        )?)),
        None => None,
    };
    let base_offset = num(&p.base_offset, "length", "the base offset")?;
    let top_offset = num(&p.top_offset, "length", "the top offset")?;
    let tread = num(&p.tread_depth, "length", "the tread depth")?;
    let width = num(&p.width, "length", "the width")?;
    for r in &p.stair_ids {
        let (id, mut recipe) = resolve_stair(&store, r)?;
        crate::modify::check_unpinned(&store, id)?;
        let e = store.get(id).expect("resolved");
        let t = type_of(&store, e);
        if let Some(l) = &base_level {
            recipe.base_level.clone_from(l);
        }
        if let Some(o) = base_offset {
            recipe.base_offset = o;
        }
        if let Some(top) = &top {
            recipe.top = top.clone();
        }
        if let Some(o) = top_offset {
            recipe.top_offset = o;
        }
        if let Some(n) = p.desired_risers {
            recipe.desired_risers = (n > 0).then_some(n);
        }
        if let Some(d) = tread {
            recipe.tread_depth = d;
        }
        if let Some(w) = width {
            for run in &mut recipe.runs {
                run.width = w;
            }
        }
        // The runs keep their share; the last one takes the difference.
        let (z0, z1) = heights(&store, &recipe)?;
        let n = recipe
            .desired_risers
            .unwrap_or_else(|| min_risers(z1 - z0, t.max_riser));
        let placed: u32 = recipe.runs.iter().map(|r| r.risers).sum();
        if p.rebalance.unwrap_or(true) && placed != n {
            let others: u32 = recipe.runs.iter().rev().skip(1).map(|r| r.risers).sum();
            if let Some(last) = recipe.runs.last_mut() {
                last.risers = n.saturating_sub(others).max(1);
            }
        }
        generate(&recipe, z0, z1, &t)?;
        let predefined = classify(&recipe.runs);
        let value = serde_json::to_value(&recipe).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
            e.params.insert("@PredefinedType".into(), predefined.into());
        })?;
    }
    Ok(())
}

/// Moves, turns or mirrors a stair's runs (`element.move` and friends).
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let store = txn.store().clone();
    let mut recipe = stair_recipe(store.get(id).expect("exists")).expect("a stair");
    for run in &mut recipe.runs {
        let a = point(run.start);
        let b = point([
            run.start[0] + run.direction[0],
            run.start[1] + run.direction[1],
        ]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]).max(1e-12);
        run.start = a;
        run.direction = [d[0] / len, d[1] / len];
        // A spiral keeps turning the way its tangent goes about its (moved) centre; a mirror
        // reverses it.
        if let Some(s) = &mut run.spiral {
            s.center = point(s.center);
            let r = [a[0] - s.center[0], a[1] - s.center[1]];
            s.ccw = r[0] * run.direction[1] - r[1] * run.direction[0] > 0.0;
        }
    }
    let value = serde_json::to_value(&recipe).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_volume(m: &Mesh) -> f64 {
        m.1.chunks(3)
            .map(|t| {
                let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    fn recipe(runs: Vec<Run>) -> StairRecipe {
        StairRecipe {
            kind: STAIR_RECIPE.into(),
            base_level: String::new(),
            base_offset: 0.0,
            top: StairTop::Unconnected(3.0),
            top_offset: 0.0,
            desired_risers: None,
            tread_depth: 0.28,
            runs,
        }
    }

    #[test]
    fn the_specified_riser_counts() {
        // ARC-STAIR-002: 3.0 m at a 0.18 m maximum is 17 risers of 0.17647 m.
        assert_eq!(min_risers(3.0, 0.18), 17);
        assert!((3.0_f64 / 17.0 - 0.17647).abs() < 1e-5);
        // ARC-STAIR-003: 2.8 m at 0.19 m is 15 risers and 14 treads.
        assert_eq!(min_risers(2.8, 0.19), 15);
        assert_eq!(risers_for_length(14.0 * 0.28, 0.28), 15);
    }

    #[test]
    fn a_straight_run_is_a_closed_solid_with_its_steps() {
        let r = recipe(vec![Run {
            start: [0.0, 0.0],
            direction: [1.0, 0.0],
            width: 1.2,
            risers: 17,
            spiral: None,
        }]);
        let g = generate(&r, 0.0, 3.0, &StairType::default()).unwrap();
        assert_eq!(g.risers, 17);
        assert_eq!(g.treads(), 16);
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
        assert_eq!(g.predefined, "STRAIGHT_RUN_STAIR");
        let v = closed_volume(&g.runs[0]);
        assert!(v > 0.0, "outward faces give a positive volume, got {v}");
        // The band below the pitch line, structural depth 150 mm square to it, less the step
        // notches: (16 · T · drop − 16 · T · R / 2) · width.
        let (t, r) = (0.28_f64, 3.0 / 17.0);
        let drop = 0.15 * (1.0 + (r / t) * (r / t)).sqrt();
        let expected = (16.0 * t * drop - 16.0 * t * r / 2.0) * 1.2;
        assert!((v - expected).abs() / expected < 0.02, "{v} vs {expected}");
        let top = g.runs[0].0.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
        assert!((top - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_full_step_spiral() {
        // 3 m at 180 mm: 17 risers and 16 treads of 280 mm along a 1 m walking line, 1 m wide.
        let run = Run {
            start: [1.0, 0.0],
            direction: [0.0, 1.0],
            width: 1.0,
            risers: 17,
            spiral: Some(Spiral {
                center: [0.0, 0.0],
                ccw: true,
            }),
        };
        let g = generate(&recipe(vec![run]), 0.0, 3.0, &StairType::default()).unwrap();
        assert_eq!(g.predefined, "SPIRAL_STAIR");
        assert_eq!(g.runs.len(), 16);
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
        // Each tread is a sector of 0.28 rad between radii 0.5 and 1.5 (0.28 m²); the first stands
        // on the floor, the rest are a riser and the structural depth deep.
        let r = 3.0 / 17.0;
        let expected = 0.28 * (r + 15.0 * (r + 0.15));
        let v: f64 = g.runs.iter().map(closed_volume).sum();
        assert!((v - expected).abs() / expected < 0.01, "{v} vs {expected}");
        // The run ends 16 treads round, and the top tread is 16 risers up.
        let end = run.end(0.28);
        let a = 16.0_f64 * 0.28;
        assert!((end[0] - a.cos()).abs() < 1e-9 && (end[1] - a.sin()).abs() < 1e-9);
        let top = g.runs[15].0.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
        assert!((top - 16.0 * r).abs() < 1e-9);
        // Clockwise turns the other way.
        let cw = Run {
            spiral: Some(Spiral {
                center: [0.0, 0.0],
                ccw: false,
            }),
            direction: [0.0, -1.0],
            ..run
        };
        let e = cw.end(0.28);
        assert!((e[0] - (-a).cos()).abs() < 1e-9 && (e[1] - (-a).sin()).abs() < 1e-9);
    }

    #[test]
    fn two_runs_with_a_landing_and_a_turn() {
        // 9 risers east, then 8 north from a quarter-turn landing.
        let a = Run {
            start: [0.0, 0.0],
            direction: [1.0, 0.0],
            width: 1.2,
            risers: 9,
            spiral: None,
        };
        let b = Run {
            start: [8.0 * 0.28 + 0.6, 0.6],
            direction: [0.0, 1.0],
            width: 1.2,
            risers: 8,
            spiral: None,
        };
        let g = generate(&recipe(vec![a, b]), 0.0, 3.0, &StairType::default()).unwrap();
        assert_eq!(g.predefined, "QUARTER_TURN_STAIR");
        assert_eq!(g.landings.len(), 1);
        let landing = &g.landings[0];
        assert!(closed_volume(landing) > 0.0);
        let top = landing.0.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
        assert!((top - 9.0 * 3.0 / 17.0).abs() < 1e-9);
        // Too few risers are reported.
        let mut short = recipe(vec![a]);
        short.runs[0].risers = 9;
        let g = generate(&short, 0.0, 3.0, &StairType::default()).unwrap();
        assert!(g.warnings[0].starts_with("Not enough risers"));
    }
}
