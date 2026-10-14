// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketches (spec Ch 3 §3.1.3–3.1.4, ARC-SKCH-001/002): the boundary curves of sketch-based
//! elements (floors now; ceilings, roofs and openings later), their validation, and the profile
//! they give.
//!
//! A sketch is stored with its host (`rxd:sketch`, RXD element data `sketch`): curves with
//! stable keys, and the loops as ordered keys, outer loops counter-clockwise and holes
//! clockwise. A curve picked from a wall with *Lock* keeps the wall face it came from; each
//! regeneration takes the face's current line and finds the corners again where neighbouring
//! curves meet, so the host follows the wall.
//!
//! A curve may pass through points between its ends, as a curved wall does
//! ([`crate::walls::curved`]): one point makes a circular arc, more a smooth spline. It is
//! validated and gives profiles as its sampled polyline; locked to a curved wall's face, it
//! follows that face's curve. Slope arrows and span directions come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::annotations::{self, DimRef};
use crate::error::{Error, Result};
use crate::id::{ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::{Line, TOL_SHORT};

/// Element parameter holding a sketch.
pub const SKETCH_KEY: &str = "rxd:sketch";
/// Endpoints closer than this are the same point (`TOL_POINT`); a gap of 0.01 mm is open.
pub const TOL_POINT: f64 = 1e-6;
/// Picked curves whose ends miss each other by up to this much are trimmed or extended to meet.
pub const PICK_GAP: f64 = 0.6;

/// What a picked curve is locked to: a wall face or location line (`face:ext`, `face:int`,
/// `axis`), as dimension references name them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Lock {
    /// The element's GlobalId.
    pub element: String,
    pub sub: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchCurve {
    pub key: u32,
    pub line: Line,
    /// Points the curve passes through between its ends (an arc or a spline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<Lock>,
}

/// A stored sketch: curves, and loops of curve keys in order (each curve runs along its loop).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sketch {
    pub curves: Vec<SketchCurve>,
    pub loops: Vec<Vec<u32>>,
}

/// An outer boundary (counter-clockwise) with its holes (clockwise), in plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub outer: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
}

impl Profile {
    /// Area of the outer boundary minus the holes.
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs()
            - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }

    /// Length of every edge, outer and holes.
    pub fn perimeter(&self) -> f64 {
        std::iter::once(&self.outer)
            .chain(&self.holes)
            .map(|l| ring_length(l))
            .sum()
    }
}

fn ring_length(l: &[[f64; 2]]) -> f64 {
    (0..l.len())
        .map(|i| {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            (b[0] - a[0]).hypot(b[1] - a[1])
        })
        .sum()
}

pub fn signed_area(l: &[[f64; 2]]) -> f64 {
    (0..l.len())
        .map(|i| {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

fn close(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= tol
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dir(l: Line) -> [f64; 2] {
    [l.end[0] - l.start[0], l.end[1] - l.start[1]]
}

/// The points a curve passes through between its ends, sampled (empty for a straight one).
pub fn interior(line: Line, through: &[[f64; 2]]) -> Vec<[f64; 2]> {
    match crate::walls::curved::path(line, through) {
        Some(p) if p.points.len() > 2 => p.points[1..p.points.len() - 1].to_vec(),
        _ => Vec::new(),
    }
}

/// A curve's sampled polyline from its start to its end.
fn polyline(line: Line, through: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut pts = vec![line.start];
    pts.extend(interior(line, through));
    pts.push(line.end);
    pts
}

/// Whether segments `a` and `b` cross or touch, and where.
fn segments_meet(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Option<[f64; 2]> {
    let (d, e) = (
        [a[1][0] - a[0][0], a[1][1] - a[0][1]],
        [b[1][0] - b[0][0], b[1][1] - b[0][1]],
    );
    let x = meet(a[0], d, b[0], e)?;
    let on = |s: [[f64; 2]; 2], d: [f64; 2]| {
        let t = ((x[0] - s[0][0]) * d[0] + (x[1] - s[0][1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]);
        (-1e-9..=1.0 + 1e-9).contains(&t)
    };
    (on(a, d) && on(b, e)).then_some(x)
}

/// The intersection of two infinite lines given by a point and a direction.
fn meet(p: [f64; 2], d: [f64; 2], q: [f64; 2], e: [f64; 2]) -> Option<[f64; 2]> {
    let den = cross(d, e);
    let scale = (d[0].hypot(d[1]) * e[0].hypot(e[1])).max(f64::EPSILON);
    if den.abs() / scale < 1e-6 {
        return None;
    }
    let w = [q[0] - p[0], q[1] - p[1]];
    let t = cross(w, e) / den;
    Some([p[0] + d[0] * t, p[1] + d[1] * t])
}

fn sketch_error(code: &'static str, message: &str, points: &[[f64; 2]]) -> Error {
    Error::new(code, message).with_details(json!({ "points": points }))
}

/// Trims or extends curves whose ends miss a neighbour by up to [`PICK_GAP`] (picked wall
/// faces stop short of, or run past, the corners) so that they meet.
pub fn close_corners(lines: &mut [Line]) {
    close_corners_except(lines, &vec![false; lines.len()]);
}

/// Like [`close_corners`], leaving the `fixed` curves (curved ones) as they are and not
/// extending others to them.
fn close_corners_except(lines: &mut [Line], fixed: &[bool]) {
    let n = lines.len();
    for i in 0..n {
        if fixed[i] {
            continue;
        }
        for end in 0..2 {
            let e = if end == 0 {
                lines[i].start
            } else {
                lines[i].end
            };
            let shared = (0..n).any(|j| {
                j != i && (close(lines[j].start, e, TOL_POINT) || close(lines[j].end, e, TOL_POINT))
            });
            if shared {
                continue;
            }
            let mut best: Option<(f64, [f64; 2])> = None;
            for j in 0..n {
                if j == i || fixed[j] {
                    continue;
                }
                let Some(x) = meet(lines[i].start, dir(lines[i]), lines[j].start, dir(lines[j]))
                else {
                    continue;
                };
                let d = (x[0] - e[0]).hypot(x[1] - e[1]);
                let near_j = close(x, lines[j].start, PICK_GAP) || close(x, lines[j].end, PICK_GAP);
                if d <= PICK_GAP && near_j && best.is_none_or(|b| d < b.0) {
                    best = Some((d, x));
                }
            }
            if let Some((_, x)) = best {
                if end == 0 {
                    lines[i].start = x;
                } else {
                    lines[i].end = x;
                }
            }
        }
    }
    // Ends moved to the same corner from both sides are now equal within rounding; snap them.
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let pa = if a == 0 { lines[i].start } else { lines[i].end };
                let pb = if b == 0 { lines[j].start } else { lines[j].end };
                if close(pa, pb, 1e-9) && pa != pb && !fixed[j] {
                    if b == 0 {
                        lines[j].start = pa;
                    } else {
                        lines[j].end = pa;
                    }
                }
            }
        }
    }
}

/// Validates boundary curves (ARC-SKCH-002) and orders them into loops: each loop as curve
/// indices with a flag for curves that run backwards, outer loops counter-clockwise and holes
/// clockwise.
pub fn build_loops(lines: &[Line]) -> Result<Vec<Vec<(usize, bool)>>> {
    build_curved_loops(lines, &vec![Vec::new(); lines.len()])
}

/// Like [`build_loops`], for curves that may pass through points between their ends
/// (`through[i]` for curve `i`).
pub fn build_curved_loops(
    lines: &[Line],
    through: &[Vec<[f64; 2]>],
) -> Result<Vec<Vec<(usize, bool)>>> {
    let inner: Vec<Vec<[f64; 2]>> = lines
        .iter()
        .zip(through)
        .map(|(l, t)| interior(*l, t))
        .collect();
    let curved = |i: usize| !inner[i].is_empty();
    if lines.is_empty() {
        return Err(Error::new("sketch.empty", "The sketch has no boundary"));
    }
    // 1. Short curves.
    for l in lines {
        if l.length() < TOL_SHORT {
            // A curve closing on itself (a full circle) is drawn as two arcs.
            return Err(sketch_error(
                "sketch.short",
                "A curve is too short",
                &[l.start, l.end],
            ));
        }
    }
    // 3. Overlaps: collinear curves sharing a positive length.
    for (i, a) in lines.iter().enumerate() {
        for (j, b) in lines.iter().enumerate().skip(i + 1) {
            if curved(i) || curved(j) {
                continue;
            }
            let (da, db) = (dir(*a), dir(*b));
            let la = a.length();
            let off = cross(da, [b.start[0] - a.start[0], b.start[1] - a.start[1]]) / la;
            if cross(da, db).abs() / (la * b.length()) > 1e-9 || off.abs() > TOL_POINT {
                continue;
            }
            let t = |p: [f64; 2]| ((p[0] - a.start[0]) * da[0] + (p[1] - a.start[1]) * da[1]) / la;
            let (t0, t1) = (t(b.start).min(t(b.end)), t(b.start).max(t(b.end)));
            if t1.min(la) - t0.max(0.0) > TOL_POINT {
                return Err(sketch_error(
                    "sketch.overlap",
                    "Curves overlap",
                    &[b.start, b.end],
                ));
            }
        }
    }
    // 4. Loop building: merged endpoints, each of degree 2.
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut vertex = |p: [f64; 2]| -> usize {
        match points.iter().position(|q| close(*q, p, TOL_POINT)) {
            Some(i) => i,
            None => {
                points.push(p);
                points.len() - 1
            }
        }
    };
    let ends: Vec<[usize; 2]> = lines
        .iter()
        .map(|l| [vertex(l.start), vertex(l.end)])
        .collect();
    let mut degree = vec![0usize; points.len()];
    for [a, b] in &ends {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    let open: Vec<[f64; 2]> = (0..points.len())
        .filter(|v| degree[*v] == 1)
        .map(|v| points[v])
        .collect();
    if !open.is_empty() {
        return Err(sketch_error(
            "sketch.open",
            "Curves must form closed loops (open ends)",
            &open,
        ));
    }
    let branches: Vec<[f64; 2]> = (0..points.len())
        .filter(|v| degree[*v] > 2)
        .map(|v| points[v])
        .collect();
    if !branches.is_empty() {
        return Err(sketch_error(
            "sketch.branch",
            "Curves must form closed loops (branches)",
            &branches,
        ));
    }
    // 5. Self-intersection: no two curves may cross except at a shared endpoint.
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            if curved(i) || curved(j) {
                // Curved: no segment of one may meet a segment of the other, except at an end
                // they share.
                let (pa, pb) = (
                    polyline(lines[i], &through[i]),
                    polyline(lines[j], &through[j]),
                );
                let shared: Vec<[f64; 2]> = ends[i]
                    .iter()
                    .filter(|v| ends[j].contains(v))
                    .map(|v| points[*v])
                    .collect();
                for sa in pa.windows(2) {
                    for sb in pb.windows(2) {
                        if let Some(x) = segments_meet([sa[0], sa[1]], [sb[0], sb[1]])
                            && !shared.iter().any(|p| close(*p, x, 1e-6))
                        {
                            return Err(sketch_error("sketch.intersect", "Curves intersect", &[x]));
                        }
                    }
                }
                continue;
            }
            let (a, b) = (lines[i], lines[j]);
            let Some(x) = meet(a.start, dir(a), b.start, dir(b)) else {
                continue;
            };
            let on = |l: Line, p: [f64; 2]| {
                let d = dir(l);
                let t = ((p[0] - l.start[0]) * d[0] + (p[1] - l.start[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            };
            let shared = ends[i].iter().any(|v| ends[j].contains(v))
                && ends[i].iter().any(|v| close(points[*v], x, TOL_POINT));
            if on(a, x) && on(b, x) && !shared {
                return Err(sketch_error("sketch.intersect", "Curves intersect", &[x]));
            }
        }
    }
    // Walk the loops.
    let mut used = vec![false; lines.len()];
    let mut loops: Vec<Vec<(usize, bool)>> = Vec::new();
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        let mut lp = vec![(first, false)];
        used[first] = true;
        let start = ends[first][0];
        let mut at = ends[first][1];
        while at != start {
            let next = (0..lines.len()).find(|k| !used[*k] && ends[*k].contains(&at));
            let Some(k) = next else {
                return Err(sketch_error(
                    "sketch.open",
                    "Curves must form closed loops",
                    &[points[at]],
                ));
            };
            used[k] = true;
            let reversed = ends[k][1] == at;
            lp.push((k, reversed));
            at = if reversed { ends[k][0] } else { ends[k][1] };
        }
        loops.push(lp);
    }
    // 6. Nesting and orientation: even depth outer (counter-clockwise), odd depth holes.
    let rings: Vec<Vec<[f64; 2]>> = loops
        .iter()
        .map(|lp| curved_ring_of(lines, &inner, lp))
        .collect();
    for (i, lp) in loops.iter_mut().enumerate() {
        let depth = rings
            .iter()
            .enumerate()
            .filter(|(j, r)| *j != i && inside(rings[i][0], r))
            .count();
        let ccw = signed_area(&rings[i]) > 0.0;
        if ccw != (depth % 2 == 0) {
            lp.reverse();
            for c in lp.iter_mut() {
                c.1 = !c.1;
            }
        }
    }
    Ok(loops)
}

/// The corner points of a loop, in order.
#[cfg(test)]
fn ring_of(lines: &[Line], lp: &[(usize, bool)]) -> Vec<[f64; 2]> {
    curved_ring_of(lines, &vec![Vec::new(); lines.len()], lp)
}

/// The points of a loop in order: each curve's start, then the points between its ends.
fn curved_ring_of(lines: &[Line], inner: &[Vec<[f64; 2]>], lp: &[(usize, bool)]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for (i, rev) in lp {
        if *rev {
            out.push(lines[*i].end);
            out.extend(inner[*i].iter().rev());
        } else {
            out.push(lines[*i].start);
            out.extend(inner[*i].iter());
        }
    }
    out
}

/// A curve argument of a create or edit command: a line, and optionally the wall face it was
/// picked from (to follow it).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveArg {
    pub line: Line,
    /// Points the curve passes through between its ends: one makes an arc, more a spline.
    #[serde(default)]
    pub through: Vec<[f64; 2]>,
    /// Lock to a wall face or location line: `{element, sub}` (`face:ext`, `face:int`, `axis`).
    pub lock: Option<LockArg>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockArg {
    pub element: ElementRef,
    pub sub: String,
}

/// A validated sketch from command arguments: picked curves closed at their corners, loops
/// ordered and oriented, locks resolved to GlobalIds.
pub fn from_args(store: &Store, curves: &[CurveArg]) -> Result<Sketch> {
    let mut lines: Vec<Line> = curves.iter().map(|c| c.line).collect();
    // Through points in line with the ends make a straight curve.
    let through: Vec<Vec<[f64; 2]>> = curves
        .iter()
        .map(|c| {
            if interior(c.line, &c.through).is_empty() {
                Vec::new()
            } else {
                c.through.clone()
            }
        })
        .collect();
    let fixed: Vec<bool> = through.iter().map(|t| !t.is_empty()).collect();
    close_corners_except(&mut lines, &fixed);
    let loops = build_curved_loops(&lines, &through)?;
    let mut sketch = Sketch::default();
    for (i, c) in curves.iter().enumerate() {
        let lock = match &c.lock {
            Some(l) => {
                let id = store.resolve(&l.element).ok_or_else(|| {
                    Error::new("element.not_found", "the locked element does not exist")
                })?;
                Some(Lock {
                    element: store.get(id).expect("resolved").guid().to_string(),
                    sub: l.sub.clone(),
                })
            }
            None => None,
        };
        sketch.curves.push(SketchCurve {
            key: i as u32 + 1,
            line: lines[i],
            through: through[i].clone(),
            lock,
        });
    }
    // Curves stored running along their loops.
    for lp in &loops {
        for (i, rev) in lp {
            if *rev {
                let c = &mut sketch.curves[*i];
                c.line = Line {
                    start: c.line.end,
                    end: c.line.start,
                };
                c.through.reverse();
            }
        }
        sketch
            .loops
            .push(lp.iter().map(|(i, _)| *i as u32 + 1).collect());
    }
    Ok(sketch)
}

/// The current line of a curve: a locked curve takes its wall face's current line (keeping its
/// direction); a curve whose lock target is gone keeps its stored line.
fn current(store: &Store, c: &SketchCurve) -> ([f64; 2], [f64; 2]) {
    let stored = (c.line.start, dir(c.line));
    let Some(lock) = &c.lock else { return stored };
    let Some(locked) = GlobalId::parse(&lock.element)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
    else {
        return stored;
    };
    // A straight piece of a curved wall's face (a ceiling's facet) keeps its line: the wall's
    // own line is only its chord.
    if crate::walls::wall_recipe(locked).is_some_and(|r| !r.through.is_empty()) {
        return stored;
    }
    let r = DimRef {
        element: Some(lock.element.clone()),
        sub: lock.sub.clone(),
        point: c.line.start,
        direction: [1.0, 0.0],
    };
    match annotations::resolve_ref(store, &r) {
        Some((p, d)) => {
            let s = if d[0] * stored.1[0] + d[1] * stored.1[1] < 0.0 {
                -1.0
            } else {
                1.0
            };
            (p, [d[0] * s, d[1] * s])
        }
        None => stored,
    }
}

/// The profiles of a sketch now: each loop's corners where neighbouring curves' current lines
/// meet, grouped into outer boundaries and their holes.
pub fn profiles(store: &Store, sketch: &Sketch) -> Vec<Profile> {
    let rings: Vec<Vec<[f64; 2]>> = rings(store, sketch).into_iter().map(|(r, _)| r).collect();
    let mut out: Vec<Profile> = Vec::new();
    let depth = |i: usize| {
        rings
            .iter()
            .enumerate()
            .filter(|(j, r)| *j != i && !rings[i].is_empty() && inside(rings[i][0], r))
            .count()
    };
    for (i, r) in rings.iter().enumerate() {
        if r.len() < 3 || depth(i) % 2 == 1 {
            continue;
        }
        let holes = rings
            .iter()
            .enumerate()
            .filter(|(j, h)| h.len() >= 3 && depth(*j) == depth(i) + 1 && inside(h[0], r))
            .map(|(_, h)| h.clone())
            .collect();
        out.push(Profile {
            outer: r.clone(),
            holes,
        });
    }
    out
}

/// A curve of a loop now: its key, its ends (where it meets its neighbours) and, for a curved
/// one, the sampled points between them.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopCurve {
    pub key: u32,
    pub line: Line,
    pub interior: Vec<[f64; 2]>,
}

/// What a curve is now: a straight line (a point and direction), or a polyline.
enum Shape {
    Straight([f64; 2], [f64; 2]),
    Curved(Vec<[f64; 2]>),
}

/// The face (or location line) of a curved wall a curve is locked to, now, as a polyline
/// running the curve's stored way; `None` if the lock is not on a curved wall.
fn curved_face(
    store: &Store,
    solved: &std::cell::OnceCell<crate::walls::Solved>,
    c: &SketchCurve,
) -> Option<Vec<[f64; 2]>> {
    use crate::walls;
    let lock = c.lock.as_ref()?;
    let id = GlobalId::parse(&lock.element).and_then(|g| store.find_by_guid(&g))?;
    let e = store.get(id)?;
    let r = walls::wall_recipe(e)?;
    let path = walls::curved::path_of(&r)?;
    let g = walls::regenerate(store, e).ok()?;
    let solved = solved.get_or_init(|| walls::solve(store));
    let ([minus, plus], _) =
        walls::curved::outline(&path, g.width, walls::curved::ends_of(solved, id));
    // The exterior face is on the +normal (left) side unless the wall is flipped.
    let mut pts = match (lock.sub.as_str(), r.flip) {
        ("face:ext", false) | ("face:int", true) => plus,
        ("face:int", false) | ("face:ext", true) => minus,
        ("axis", _) => path.points.clone(),
        _ => return None,
    };
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let (first, last) = (pts[0], *pts.last()?);
    if d(first, c.line.end) + d(last, c.line.start) < d(first, c.line.start) + d(last, c.line.end) {
        pts.reverse();
    }
    Some(pts)
}

/// Each loop of a sketch now, curve by curve: straight curves meet their straight neighbours
/// where their current lines cross; a curved curve keeps its own ends (a curve locked to a
/// curved wall's face takes the face's current curve).
pub fn loop_curves(store: &Store, sketch: &Sketch) -> Vec<Vec<LoopCurve>> {
    let solved = std::cell::OnceCell::new();
    let by_key = |k: u32| sketch.curves.iter().find(|c| c.key == k);
    sketch
        .loops
        .iter()
        .map(|lp| {
            let curves: Vec<&SketchCurve> = lp.iter().filter_map(|k| by_key(*k)).collect();
            let shapes: Vec<Shape> = curves
                .iter()
                .map(|c| {
                    if !c.through.is_empty()
                        && let Some(pts) = curved_face(store, &solved, c)
                    {
                        return Shape::Curved(pts);
                    }
                    if !interior(c.line, &c.through).is_empty() {
                        return Shape::Curved(polyline(c.line, &c.through));
                    }
                    let (p, d) = current(store, c);
                    Shape::Straight(p, d)
                })
                .collect();
            let n = curves.len();
            let corners: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let prev = (i + n - 1) % n;
                    match (&shapes[prev], &shapes[i]) {
                        (_, Shape::Curved(pts)) => pts[0],
                        (Shape::Curved(pts), _) => *pts.last().expect("points"),
                        (Shape::Straight(p, d), Shape::Straight(q, e)) => {
                            meet(*p, *d, *q, *e).unwrap_or(curves[i].line.start)
                        }
                    }
                })
                .collect();
            (0..n)
                .map(|i| LoopCurve {
                    key: curves[i].key,
                    line: Line {
                        start: corners[i],
                        end: corners[(i + 1) % n],
                    },
                    interior: match &shapes[i] {
                        Shape::Curved(pts) => pts[1..pts.len() - 1].to_vec(),
                        Shape::Straight(..) => Vec::new(),
                    },
                })
                .collect()
        })
        .collect()
}

/// Each loop of a sketch now: its points in order (corners, and the points along curved
/// curves), and the key of the curve from each point to the next.
pub fn rings(store: &Store, sketch: &Sketch) -> Vec<(Vec<[f64; 2]>, Vec<u32>)> {
    loop_curves(store, sketch)
        .into_iter()
        .map(|lp| {
            let (mut pts, mut keys) = (Vec::new(), Vec::new());
            for c in lp {
                pts.push(c.line.start);
                keys.push(c.key);
                for p in c.interior {
                    pts.push(p);
                    keys.push(c.key);
                }
            }
            (pts, keys)
        })
        .collect()
}

/// The sketch of an element, if it has one.
pub fn sketch_of(e: &crate::Element) -> Option<Sketch> {
    serde_json::from_value(e.param(SKETCH_KEY)?.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(a: [f64; 2], b: [f64; 2]) -> Line {
        Line { start: a, end: b }
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<Line> {
        vec![
            l([x, y], [x + s, y]),
            l([x + s, y], [x + s, y + s]),
            l([x + s, y + s], [x, y + s]),
            l([x, y + s], [x, y]),
        ]
    }

    #[test]
    fn a_square_with_a_hole_gives_ccw_outer_and_cw_hole() {
        let mut lines = square(0.0, 0.0, 10.0);
        // The hole drawn counter-clockwise, curves in any direction.
        let mut hole = square(4.0, 4.0, 2.0);
        hole[1] = l(hole[1].end, hole[1].start);
        lines.extend(hole);
        let loops = build_loops(&lines).unwrap();
        assert_eq!(loops.len(), 2);
        let rings: Vec<f64> = loops
            .iter()
            .map(|lp| signed_area(&ring_of(&lines, lp)))
            .collect();
        assert!((rings[0] - 100.0).abs() < 1e-9);
        assert!((rings[1] + 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_square_drawn_short_reports_its_open_vertex() {
        let mut lines = square(0.0, 0.0, 10.0);
        lines[1].end = [10.0, 9.99999];
        let e = build_loops(&lines).unwrap_err();
        assert_eq!(e.code, "sketch.open");
        let pts: Vec<[f64; 2]> = serde_json::from_value(e.details["points"].clone()).unwrap();
        assert!(pts.iter().any(|p| close(*p, [10.0, 9.99999], 1e-9)));
        assert!(pts.iter().any(|p| close(*p, [10.0, 10.0], 1e-9)));
    }

    #[test]
    fn crossings_branches_and_overlaps_are_refused() {
        let bow = vec![
            l([0.0, 0.0], [4.0, 4.0]),
            l([4.0, 4.0], [4.0, 0.0]),
            l([4.0, 0.0], [0.0, 4.0]),
            l([0.0, 4.0], [0.0, 0.0]),
        ];
        assert_eq!(build_loops(&bow).unwrap_err().code, "sketch.intersect");
        let mut branch = square(0.0, 0.0, 4.0);
        branch.push(l([0.0, 0.0], [2.0, 2.0]));
        branch.push(l([2.0, 2.0], [4.0, 4.0]));
        assert_eq!(build_loops(&branch).unwrap_err().code, "sketch.branch");
        let mut overlap = square(0.0, 0.0, 4.0);
        overlap.push(l([1.0, 0.0], [3.0, 0.0]));
        assert_eq!(build_loops(&overlap).unwrap_err().code, "sketch.overlap");
    }

    #[test]
    fn picked_faces_are_closed_at_their_corners() {
        // Interior faces of four 200 mm walls around a 4 × 3 room: each stops 100 mm short.
        let mut lines = vec![
            l([0.0, 0.1], [4.0, 0.1]),
            l([3.9, 0.0], [3.9, 3.0]),
            l([4.0, 2.9], [0.0, 2.9]),
            l([0.1, 3.0], [0.1, 0.0]),
        ];
        close_corners(&mut lines);
        let loops = build_loops(&lines).unwrap();
        let ring = ring_of(&lines, &loops[0]);
        assert!((signed_area(&ring) - 3.8 * 2.8).abs() < 1e-9);
    }

    #[test]
    fn two_arcs_make_a_circle_and_an_arc_may_not_cross_a_line() {
        // A circle of radius 2 as two half arcs, one drawn backwards.
        let lines = vec![l([2.0, 0.0], [-2.0, 0.0]), l([2.0, 0.0], [-2.0, 0.0])];
        let through = vec![vec![[0.0, 2.0]], vec![[0.0, -2.0]]];
        let loops = build_curved_loops(&lines, &through).unwrap();
        assert_eq!(loops.len(), 1);
        let inner: Vec<_> = lines
            .iter()
            .zip(&through)
            .map(|(l, t)| interior(*l, t))
            .collect();
        let area = signed_area(&curved_ring_of(&lines, &inner, &loops[0]));
        let exact = std::f64::consts::PI * 4.0;
        assert!((area - exact).abs() / exact < 1e-3, "{area}");
        // A D shape whose arc bulges across a line through it.
        let lines = vec![
            l([0.0, 0.0], [0.0, 4.0]),
            l([0.0, 4.0], [0.0, 0.0]),
            l([1.0, -1.0], [1.0, 5.0]),
            l([1.0, 5.0], [3.0, 5.0]),
            l([3.0, 5.0], [3.0, -1.0]),
            l([3.0, -1.0], [1.0, -1.0]),
        ];
        let mut through = vec![Vec::new(); 6];
        through[1] = vec![[2.0, 2.0]];
        assert_eq!(
            build_curved_loops(&lines, &through).unwrap_err().code,
            "sketch.intersect"
        );
    }

    #[test]
    fn a_half_round_floor_from_arguments() {
        let store = Store::default();
        let args: Vec<CurveArg> = serde_json::from_value(json!([
            { "line": { "start": [-3, 0], "end": [3, 0] } },
            { "line": { "start": [3, 0], "end": [-3, 0] }, "through": [[0, 3]] },
        ]))
        .unwrap();
        let sketch = from_args(&store, &args).unwrap();
        let p = profiles(&store, &sketch);
        assert_eq!(p.len(), 1);
        let exact = std::f64::consts::PI * 9.0 / 2.0;
        assert!(
            (p[0].area() - exact).abs() / exact < 1e-3,
            "{}",
            p[0].area()
        );
        // Through points in line with the ends make a straight curve.
        let args: Vec<CurveArg> = serde_json::from_value(json!([
            { "line": { "start": [0, 0], "end": [4, 0] }, "through": [[2, 0]] },
            { "line": { "start": [4, 0], "end": [4, 3] } },
            { "line": { "start": [4, 3], "end": [0, 0] } },
        ]))
        .unwrap();
        let sketch = from_args(&store, &args).unwrap();
        assert!(sketch.curves.iter().all(|c| c.through.is_empty()));
    }
}
