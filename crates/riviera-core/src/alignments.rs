// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Alignments (ARC-ALN-001): a route set out by chainage, the shared foundation for roads and
//! drives and for guideway devices. The horizontal layout is tangent lines and circular curves
//! fitted at points of intersection (PIs); the vertical profile is grades between vertical PIs
//! with symmetric parabolic curves. Stored as an `IfcAnnotation` ("Alignment") with its recipe;
//! IFC 4.3's `IfcAlignment` comes later (ARC-ALN-002).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];

pub const RECIPE_KEY: &str = "rxd:recipe";
pub const RECIPE: &str = "recipe.alignment";

fn bad(msg: impl Into<String>) -> Error {
    Error::new("alignment.invalid", msg)
}

/// A vertical PI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pvi {
    pub station: f64,
    pub elevation: f64,
    /// The length of the symmetric parabolic curve centred on it (m; 0 or omitted: none).
    #[serde(default)]
    pub curve: Option<f64>,
}

/// An alignment's definition, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AlignmentRecipe {
    pub kind: String,
    /// Its level (GlobalId).
    pub level: String,
    pub points: Vec<V2>,
    /// One per interior PI (0: no curve).
    pub radii: Vec<f64>,
    pub start_station: f64,
    #[serde(default)]
    pub profile: Vec<Pvi>,
}

pub fn recipe(e: &crate::element::Element) -> Option<AlignmentRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok()).flatten()
}

/// A piece of the horizontal layout.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Piece {
    Line { start: V2, end: V2, length: f64 },
    /// From `start` round `centre`, turning left when `left`, through `angle` radians.
    Arc { start: V2, end: V2, centre: V2, radius: f64, left: bool, angle: f64, length: f64 },
}

impl Piece {
    fn length(&self) -> f64 {
        match self {
            Piece::Line { length, .. } | Piece::Arc { length, .. } => *length,
        }
    }

    /// The point and unit direction `s` along it.
    fn at(&self, s: f64) -> (V2, V2) {
        match self {
            Piece::Line { start, end, length } => {
                let d = [(end[0] - start[0]) / length, (end[1] - start[1]) / length];
                ([start[0] + d[0] * s, start[1] + d[1] * s], d)
            }
            Piece::Arc { start, centre, radius, left, .. } => {
                let a0 = (start[1] - centre[1]).atan2(start[0] - centre[0]);
                let turn = if *left { 1.0 } else { -1.0 };
                let a = a0 + turn * s / radius;
                let p = [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()];
                let d = if *left { [-a.sin(), a.cos()] } else { [a.sin(), -a.cos()] };
                (p, d)
            }
        }
    }
}

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn norm(a: V2) -> f64 {
    a[0].hypot(a[1])
}

/// A curve fitted at a PI: where it leaves the back leg and joins the forward one, its centre,
/// radius, hand and angle.
#[derive(Clone, Copy)]
struct Cut {
    pc: V2,
    pt: V2,
    centre: V2,
    radius: f64,
    left: bool,
    angle: f64,
}

/// The tangent lines and arcs of a horizontal layout.
pub fn pieces(points: &[V2], radii: &[f64]) -> Result<Vec<Piece>> {
    if points.len() < 2 {
        return Err(bad("an alignment needs two or more points"));
    }
    if radii.len() > points.len().saturating_sub(2) {
        return Err(bad("one radius per interior point at most"));
    }
    for w in points.windows(2) {
        if norm(sub(w[1], w[0])) < 1e-6 {
            return Err(bad("two points of the alignment coincide"));
        }
    }
    // Each interior PI's curve: where it leaves the back leg and joins the forward one.
    let mut cuts: Vec<Option<Cut>> = vec![None; points.len()];
    let mut used = vec![0.0f64; points.len()];
    for i in 1..points.len() - 1 {
        let r = radii.get(i - 1).copied().unwrap_or(0.0);
        if r <= 0.0 {
            continue;
        }
        if !r.is_finite() {
            return Err(bad("a radius must be finite"));
        }
        let (a, p, b) = (points[i - 1], points[i], points[i + 1]);
        let (u, v) = (sub(p, a), sub(b, p));
        let (u, v) = ([u[0] / norm(u), u[1] / norm(u)], [v[0] / norm(v), v[1] / norm(v)]);
        let cross = u[0] * v[1] - u[1] * v[0];
        let delta = cross.atan2(u[0] * v[0] + u[1] * v[1]);
        if delta.abs() < 1e-9 {
            continue;
        }
        let t = r * (delta.abs() / 2.0).tan();
        let left = delta > 0.0;
        let pc = [p[0] - u[0] * t, p[1] - u[1] * t];
        let pt = [p[0] + v[0] * t, p[1] + v[1] * t];
        let n = if left { [-u[1], u[0]] } else { [u[1], -u[0]] };
        let centre = [pc[0] + n[0] * r, pc[1] + n[1] * r];
        cuts[i] = Some(Cut { pc, pt, centre, radius: r, left, angle: delta.abs() });
        used[i] = t;
    }
    for i in 0..points.len() - 1 {
        let room = norm(sub(points[i + 1], points[i]));
        if used[i] + used[i + 1] > room + 1e-9 {
            return Err(bad(format!("the curves at points {} and {} need {:.2} m of a {room:.2} m leg: smaller radii", i + 1, i + 2, used[i] + used[i + 1])));
        }
    }
    let mut out = Vec::new();
    let mut from = points[0];
    for i in 1..points.len() {
        let to = cuts[i].map_or(points[i], |c| c.pc);
        let l = norm(sub(to, from));
        if l > 1e-9 {
            out.push(Piece::Line { start: from, end: to, length: l });
        }
        if let Some(Cut { pc, pt, centre, radius, left, angle }) = cuts[i] {
            out.push(Piece::Arc { start: pc, end: pt, centre, radius, left, angle, length: radius * angle });
            from = pt;
        } else {
            from = to;
        }
    }
    Ok(out)
}

/// The elevation and grade at `station` on a profile (0 and 0 without one).
pub fn elevation(profile: &[Pvi], station: f64) -> (f64, f64) {
    if profile.is_empty() {
        return (0.0, 0.0);
    }
    if profile.len() == 1 {
        return (profile[0].elevation, 0.0);
    }
    let g = |i: usize| (profile[i + 1].elevation - profile[i].elevation) / (profile[i + 1].station - profile[i].station);
    // Within a vertical curve?
    for (i, pvi) in profile.iter().enumerate().take(profile.len() - 1).skip(1) {
        let l = pvi.curve.unwrap_or(0.0);
        if l <= 0.0 {
            continue;
        }
        let (g1, g2) = (g(i - 1), g(i));
        let bvc = pvi.station - l / 2.0;
        if station >= bvc && station <= bvc + l {
            let x = station - bvc;
            let z0 = pvi.elevation - g1 * l / 2.0;
            return (z0 + g1 * x + (g2 - g1) * x * x / (2.0 * l), g1 + (g2 - g1) * x / l);
        }
    }
    let i = (0..profile.len() - 1).find(|&i| station <= profile[i + 1].station).unwrap_or(profile.len() - 2);
    let gi = g(i);
    (profile[i].elevation + gi * (station - profile[i].station), gi)
}

fn check_profile(profile: &[Pvi]) -> Result<()> {
    for w in profile.windows(2) {
        if w[1].station <= w[0].station {
            return Err(bad("the profile's stations must increase"));
        }
    }
    for i in 1..profile.len().saturating_sub(1) {
        let l = profile[i].curve.unwrap_or(0.0);
        if l < 0.0 || !l.is_finite() {
            return Err(bad("a vertical curve's length must be 0 or more"));
        }
        let room = (profile[i].station - profile[i - 1].station).min(profile[i + 1].station - profile[i].station);
        if l / 2.0 > room + 1e-9 {
            return Err(bad(format!("the vertical curve at station {:.2} is longer than its grades allow", profile[i].station)));
        }
    }
    if profile.first().is_some_and(|p| p.curve.unwrap_or(0.0) > 0.0) || (profile.len() > 1 && profile.last().is_some_and(|p| p.curve.unwrap_or(0.0) > 0.0)) {
        return Err(bad("the first and last profile points cannot have a vertical curve"));
    }
    Ok(())
}

/// An alignment worked out.
#[derive(Debug, Clone)]
pub struct Alignment {
    pub recipe: AlignmentRecipe,
    pub pieces: Vec<Piece>,
    pub length: f64,
}

impl Alignment {
    pub fn of(r: AlignmentRecipe) -> Result<Self> {
        let pieces = pieces(&r.points, &r.radii)?;
        let length = pieces.iter().map(Piece::length).sum();
        Ok(Alignment { recipe: r, pieces, length })
    }

    pub fn end_station(&self) -> f64 {
        self.recipe.start_station + self.length
    }

    /// The point, direction, elevation and grade at `station` (clamped to the alignment).
    pub fn at(&self, station: f64) -> (V2, V2, f64, f64) {
        let mut s = (station - self.recipe.start_station).clamp(0.0, self.length);
        let (z, g) = elevation(&self.recipe.profile, station);
        for p in &self.pieces {
            if s <= p.length() + 1e-12 {
                let (q, d) = p.at(s);
                return (q, d, z, g);
            }
            s -= p.length();
        }
        let last = self.pieces.last().expect("an alignment has pieces");
        let (q, d) = last.at(last.length());
        (q, d, z, g)
    }

    /// The chainage and left offset of the nearest point to `q`.
    pub fn station_of(&self, q: V2) -> (f64, f64) {
        let mut best = (f64::INFINITY, 0.0, 0.0);
        let mut s0 = self.recipe.start_station;
        for p in &self.pieces {
            let (s, off) = match p {
                Piece::Line { start, end, length } => {
                    let d = [(end[0] - start[0]) / length, (end[1] - start[1]) / length];
                    let w = sub(q, *start);
                    let t = (w[0] * d[0] + w[1] * d[1]).clamp(0.0, *length);
                    (t, d[0] * w[1] - d[1] * w[0])
                }
                Piece::Arc { start, centre, radius, left, angle, .. } => {
                    let a0 = (start[1] - centre[1]).atan2(start[0] - centre[0]);
                    let aq = (q[1] - centre[1]).atan2(q[0] - centre[0]);
                    let turn = if *left { 1.0 } else { -1.0 };
                    let mut da = turn * (aq - a0);
                    da = da.rem_euclid(std::f64::consts::TAU);
                    if da > *angle {
                        da = if da - angle < std::f64::consts::TAU - da { *angle } else { 0.0 };
                    }
                    let rq = norm(sub(q, *centre));
                    // Left of the direction of travel is toward the centre on a left-hand curve.
                    (radius * da, if *left { radius - rq } else { rq - radius })
                }
            };
            let (pt, _) = p.at(s);
            let dist = norm(sub(q, pt));
            if dist < best.0 - 1e-12 {
                best = (dist, s0 + s, off);
            }
            s0 += p.length();
        }
        (best.1, best.2)
    }

    /// Points along it every `step` metres or less (arcs in short chords), for drawing.
    pub fn polyline(&self, step: f64) -> Vec<V2> {
        let mut out = vec![self.pieces[0].at(0.0).0];
        for p in &self.pieces {
            let n = match p {
                Piece::Line { .. } => 1,
                Piece::Arc { length, .. } => ((length / step).ceil() as usize).max(2),
            };
            for k in 1..=n {
                out.push(p.at(p.length() * k as f64 / n as f64).0);
            }
        }
        out
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level: ElementRef,
    /// The points of intersection in plan, in order.
    pub points: Vec<V2>,
    /// The curve radius at each interior point (0: none).
    #[serde(default)]
    pub radii: Vec<f64>,
    /// The chainage at the first point (default 0).
    #[serde(default)]
    pub start_station: f64,
    /// Vertical PIs in increasing chainage.
    #[serde(default)]
    pub profile: Vec<Pvi>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    pub alignment: ElementId,
    pub length: f64,
    pub start_station: f64,
    pub end_station: f64,
}

/// `alignment.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store();
    let level = store.resolve(&p.level).ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let guid = crate::levels::list(store).into_iter().find(|l| l.id == level).map(|l| l.guid).ok_or_else(|| bad(format!("element {level} is not a level")))?;
    if !p.start_station.is_finite() || p.points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad("points and stations must be finite"));
    }
    check_profile(&p.profile)?;
    let r = AlignmentRecipe { kind: RECIPE.into(), level: guid, points: p.points, radii: p.radii, start_station: p.start_station, profile: p.profile };
    let a = Alignment::of(r.clone())?;
    let n = store.iter().filter(|e| recipe(e).is_some()).count() + 1;
    let id = txn.create(
        NewElement::new("IfcAnnotation")
            .name(p.name.unwrap_or_else(|| format!("Alignment {n}")))
            .param("@ObjectType", "Alignment")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    Ok(Created { alignment: id, length: a.length, start_station: r.start_station, end_station: a.end_station() })
}

fn get(store: &Store, r: &ElementRef) -> Result<(ElementId, Alignment)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", "no such alignment"))?;
    let rec = store.get(id).and_then(recipe).ok_or_else(|| bad(format!("element {id} is not an alignment")))?;
    Ok((id, Alignment::of(rec)?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PointAtParams {
    pub alignment_id: ElementRef,
    pub station: f64,
    /// To the left of the direction of travel (m; negative: right).
    #[serde(default)]
    pub offset: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PointAt {
    pub point: V2,
    pub elevation: f64,
    /// The unit direction of travel, and the grade (rise over run).
    pub direction: V2,
    pub grade: f64,
}

/// `alignment.point_at`.
pub(crate) fn point_at(store: &Store, p: PointAtParams) -> Result<PointAt> {
    let (_, a) = get(store, &p.alignment_id)?;
    if p.station < a.recipe.start_station - 1e-9 || p.station > a.end_station() + 1e-9 {
        return Err(bad(format!("station {:.2} is off the alignment ({:.2} to {:.2})", p.station, a.recipe.start_station, a.end_station())));
    }
    let (q, d, z, g) = a.at(p.station);
    Ok(PointAt { point: [q[0] - d[1] * p.offset, q[1] + d[0] * p.offset], elevation: z, direction: d, grade: g })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationOfParams {
    pub alignment_id: ElementRef,
    pub point: V2,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StationOf {
    pub station: f64,
    /// Left of the direction of travel (m; negative: right).
    pub offset: f64,
}

/// `alignment.station_of`.
pub(crate) fn station_of(store: &Store, p: StationOfParams) -> Result<StationOf> {
    let (_, a) = get(store, &p.alignment_id)?;
    let (station, offset) = a.station_of(p.point);
    Ok(StationOf { station, offset })
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// A curve of the horizontal layout.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CurveInfo {
    pub radius: f64,
    pub length: f64,
    /// Its chainage where it starts and ends, and whether it turns left.
    pub start_station: f64,
    pub end_station: f64,
    pub left: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub alignment: ElementId,
    pub name: String,
    pub level: String,
    pub length: f64,
    pub start_station: f64,
    pub end_station: f64,
    pub curves: Vec<CurveInfo>,
    /// The lowest and highest elevation along it (none without a profile).
    pub elevations: Option<[f64; 2]>,
}

/// Every alignment.
pub fn alignments(store: &Store) -> Vec<(ElementId, Alignment)> {
    store.iter().filter_map(|e| Some((e.id(), Alignment::of(recipe(e)?).ok()?))).collect()
}

/// `alignment.list`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<Info>> {
    let levels = crate::levels::list(store);
    Ok(alignments(store)
        .into_iter()
        .map(|(id, a)| {
            let mut s0 = a.recipe.start_station;
            let mut curves = Vec::new();
            for p in &a.pieces {
                if let Piece::Arc { radius, left, length, .. } = p {
                    curves.push(CurveInfo { radius: *radius, length: *length, start_station: s0, end_station: s0 + length, left: *left });
                }
                s0 += p.length();
            }
            let elevations = (!a.recipe.profile.is_empty()).then(|| {
                let n = (a.length / 1.0).ceil().max(1.0) as usize;
                let zs: Vec<f64> = (0..=n).map(|k| elevation(&a.recipe.profile, a.recipe.start_station + a.length * k as f64 / n as f64).0).collect();
                [zs.iter().copied().fold(f64::INFINITY, f64::min), zs.iter().copied().fold(f64::NEG_INFINITY, f64::max)]
            });
            Info {
                alignment: id,
                name: store.get(id).and_then(|e| e.name.clone()).unwrap_or_default(),
                level: levels.iter().find(|l| l.guid == a.recipe.level).map_or_else(String::new, |l| l.name.clone()),
                length: a.length,
                start_station: a.recipe.start_station,
                end_station: a.end_station(),
                curves,
                elevations,
            }
        })
        .collect())
}

/// Moves, turns or mirrors an alignment's points (the Modify tools); its curves follow.
pub(crate) fn transform(txn: &mut Transaction<'_>, id: ElementId, point: &dyn Fn(V2) -> V2) -> Result<()> {
    let mut r = txn.store().get(id).and_then(recipe).ok_or_else(|| bad(format!("element {id} is not an alignment")))?;
    r.points = r.points.iter().map(|p| point(*p)).collect();
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}
