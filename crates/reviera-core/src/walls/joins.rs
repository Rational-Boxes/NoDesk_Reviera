// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall joins (spec Ch 3 §3.3.12, ARC-JOIN-001) for single-layer walls.
//!
//! Joins are derived, not stored: they are detected from the walls' location lines every time the
//! walls are solved, so moving, deleting or undoing a wall updates its neighbours too. Each wall
//! end may disallow joining (`join_allowed` in its recipe).
//!
//! - **L node:** two ends meet (within `TOL_JOIN`, or their extended location lines cross within a
//!   wall width of both ends). With one layer on each side, both bands reach the corner cell and
//!   the cell is split by the miter diagonal, from the outer-face intersection to the inner-face
//!   intersection (rule D.4).
//! - **T node:** an end, extended by at most half the other wall's width plus 1 mm, lands inside
//!   another wall away from its ends. The continuous wall owns the overlap, so the arriving wall
//!   stops at its near face.
//!
//! Not yet: layered priority painting, height intervals, nodes of three or more walls (ends pair
//! off two at a time), join types other than the default, and join-order overrides.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{WallGeometry, regenerate, wall_recipe};
use crate::id::ElementId;
use crate::store::Store;

/// The fixed part of `TOL_JOIN` (Ch 3 §3.0): 1 mm, plus half the other wall's width.
pub const TOL_JOIN: f64 = 0.001;
/// Location lines closer to parallel than this do not join (rule A.5).
const MIN_ANGLE: f64 = 1.0 * std::f64::consts::PI / 180.0;
/// A miter corner further than this many widths from the end is not used (very sharp angles).
const MAX_MITER: f64 = 10.0;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

impl End {
    pub fn index(self) -> usize {
        match self {
            End::Start => 0,
            End::End => 1,
        }
    }
}

/// One pairwise connection. `other_end` is `None` for a T node (the end meets the other wall's
/// path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
pub struct Join {
    pub wall: ElementId,
    pub end: End,
    pub other: ElementId,
    pub other_end: Option<End>,
}

/// A wall's solved body: its geometry and plan outline after joins, ordered start-interior,
/// end-interior, end-exterior, start-exterior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallBody {
    pub geometry: WallGeometry,
    pub footprint: [[f64; 2]; 4],
}

impl WallBody {
    pub fn mesh(&self) -> (Vec<[f64; 3]>, Vec<u32>) {
        let g = &self.geometry;
        super::prism(&self.footprint, g.base_z, g.base_z + g.height)
    }

    /// The outline in the wall's local coordinates (X along the wall from its start, Y toward the
    /// exterior).
    pub fn local_footprint(&self) -> [[f64; 2]; 4] {
        let (u, n, s) = (
            self.geometry.direction(),
            self.geometry.normal(),
            self.geometry.start,
        );
        self.footprint.map(|c| {
            let d = sub(c, s);
            [dot(d, u), dot(d, n)]
        })
    }

    /// Whether the outline is the plain rectangle of an unjoined wall.
    pub fn is_rectangular(&self) -> bool {
        let plain = self.geometry.footprint();
        self.footprint
            .iter()
            .zip(&plain)
            .all(|(a, b)| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9)
    }
}

/// Every Reviera wall of a model, regenerated and joined.
#[derive(Debug, Clone, Default)]
pub struct Solved {
    pub bodies: BTreeMap<ElementId, WallBody>,
    pub joins: Vec<Join>,
}

impl Solved {
    /// The joins that involve `wall`.
    pub fn joins_of(&self, wall: ElementId) -> Vec<Join> {
        self.joins
            .iter()
            .filter(|j| j.wall == wall || j.other == wall)
            .copied()
            .collect()
    }
}

type P = [f64; 2];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: P, k: f64) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn left(a: P) -> P {
    [-a[1], a[0]]
}

/// Intersection of the lines `p + t·d` and `q + s·e`, as `(point, t, s)`.
fn intersect(p: P, d: P, q: P, e: P) -> Option<(P, f64, f64)> {
    let c = cross(d, e);
    if c.abs() < 1e-12 {
        return None;
    }
    let w = sub(q, p);
    let (t, s) = (cross(w, e) / c, cross(w, d) / c);
    Some((add(p, mul(d, t)), t, s))
}

/// One wall end: its point, the direction pointing into the wall, and the wall's data.
#[derive(Debug, Clone, Copy)]
struct WallEnd {
    wall: usize,
    end: End,
    point: P,
    /// Unit direction from the end into the wall.
    inward: P,
}

struct Wall {
    id: ElementId,
    g: WallGeometry,
    allowed: [bool; 2],
    footprint: [[f64; 2]; 4],
}

impl Wall {
    fn end(&self, wall: usize, end: End) -> WallEnd {
        let u = self.g.direction();
        match end {
            End::Start => WallEnd {
                wall,
                end,
                point: self.g.start,
                inward: u,
            },
            End::End => WallEnd {
                wall,
                end,
                point: self.g.end,
                inward: mul(u, -1.0),
            },
        }
    }

    fn half(&self) -> f64 {
        self.g.width / 2.0
    }

    fn overlaps_vertically(&self, o: &Wall) -> bool {
        let (a0, a1) = (self.g.base_z, self.g.base_z + self.g.height);
        let (b0, b1) = (o.g.base_z, o.g.base_z + o.g.height);
        a0 < b1 - 1e-9 && b0 < a1 - 1e-9
    }

    /// Footprint indices of the corners at `end`: (on the left of `inward`, on the right).
    fn corners(end: End) -> (usize, usize) {
        // At the start, `inward` = u and its left is the exterior (+n): corners 3 and 0.
        // At the end, `inward` = −u and its left is the interior (−n): corners 1 and 2.
        match end {
            End::Start => (3, 0),
            End::End => (1, 2),
        }
    }
}

fn angle_ok(a: P, b: P) -> bool {
    cross(a, b).abs() > MIN_ANGLE.sin()
}

/// Regenerates every Reviera wall in `store` and resolves the joins between them. Walls that
/// cannot regenerate are left out.
pub fn solve(store: &Store) -> Solved {
    let mut walls: Vec<Wall> = store
        .iter()
        .filter_map(|e| {
            let recipe = wall_recipe(e)?;
            // Curved walls do not join yet (ARC-WALL-002); their ends butt.
            if !recipe.through.is_empty() {
                return None;
            }
            let g = regenerate(store, e).ok()?;
            Some(Wall {
                id: e.id(),
                g,
                allowed: recipe.join_allowed,
                footprint: g.footprint(),
            })
        })
        .collect();

    // L candidates: pairs of allowed ends of different walls.
    let ends: Vec<WallEnd> = walls
        .iter()
        .enumerate()
        .flat_map(|(i, w)| {
            [End::Start, End::End]
                .into_iter()
                .filter(|e| w.allowed[e.index()])
                .map(move |e| w.end(i, e))
        })
        .collect();
    let mut candidates = Vec::new();
    for (i, a) in ends.iter().enumerate() {
        for b in &ends[i + 1..] {
            let (wa, wb) = (&walls[a.wall], &walls[b.wall]);
            if a.wall == b.wall || !wa.overlaps_vertically(wb) || !angle_ok(a.inward, b.inward) {
                continue;
            }
            let gap = dist(a.point, b.point);
            let reach = wa.g.width.max(wb.g.width);
            let near = gap <= TOL_JOIN + reach / 2.0;
            let crossing = intersect(a.point, a.inward, b.point, b.inward)
                .is_some_and(|(x, _, _)| dist(x, a.point) <= reach && dist(x, b.point) <= reach);
            if near || crossing {
                candidates.push((gap, *a, *b));
            }
        }
    }
    candidates.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut used: BTreeSet<(usize, End)> = BTreeSet::new();
    let mut joins = Vec::new();
    for (_, a, b) in candidates {
        if used.contains(&(a.wall, a.end)) || used.contains(&(b.wall, b.end)) {
            continue;
        }
        if miter(&mut walls, a, b) {
            used.insert((a.wall, a.end));
            used.insert((b.wall, b.end));
            joins.push(ordered(&walls, a, b));
        }
    }

    // T candidates: the remaining allowed ends against the span of another wall.
    for a in ends {
        if used.contains(&(a.wall, a.end)) {
            continue;
        }
        let best = (0..walls.len())
            .filter(|&j| j != a.wall)
            .filter_map(|j| tee_cut(&walls, a, j).map(|cut| (cut.0, j, cut.1)))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((_, j, corners)) = best {
            let (l, r) = Wall::corners(a.end);
            let w = &mut walls[a.wall];
            (w.footprint[l], w.footprint[r]) = corners;
            joins.push(Join {
                wall: walls[a.wall].id,
                end: a.end,
                other: walls[j].id,
                other_end: None,
            });
        }
    }

    joins.sort();
    Solved {
        bodies: walls
            .into_iter()
            .map(|w| {
                (
                    w.id,
                    WallBody {
                        geometry: w.g,
                        footprint: w.footprint,
                    },
                )
            })
            .collect(),
        joins,
    }
}

/// An L join with the first-drawn wall (lower ID) as the relating one.
fn ordered(walls: &[Wall], a: WallEnd, b: WallEnd) -> Join {
    let (x, y) = if walls[a.wall].id <= walls[b.wall].id {
        (a, b)
    } else {
        (b, a)
    };
    Join {
        wall: walls[x.wall].id,
        end: x.end,
        other: walls[y.wall].id,
        other_end: Some(y.end),
    }
}

/// Cuts both ends of an L node along the miter diagonal. Returns `false` when the corner would be
/// unreasonably far from the ends (very sharp angles), leaving both ends square.
fn miter(walls: &mut [Wall], a: WallEnd, b: WallEnd) -> bool {
    let (ha, hb) = (walls[a.wall].half(), walls[b.wall].half());
    // A face on the left of A's inward direction meets B's face on the right of B's, and the other
    // way round: these are the outer and inner corners.
    let face = |e: WallEnd, h: f64, side: f64| add(e.point, mul(left(e.inward), h * side));
    let q1 = intersect(face(a, ha, 1.0), a.inward, face(b, hb, -1.0), b.inward);
    let q2 = intersect(face(a, ha, -1.0), a.inward, face(b, hb, 1.0), b.inward);
    let (Some((q1, ..)), Some((q2, ..))) = (q1, q2) else {
        return false;
    };
    let limit = MAX_MITER * (ha + hb) * 2.0;
    if [q1, q2]
        .iter()
        .any(|q| dist(*q, a.point) > limit || dist(*q, b.point) > limit)
    {
        return false;
    }
    let (al, ar) = Wall::corners(a.end);
    let (bl, br) = Wall::corners(b.end);
    let wa = &mut walls[a.wall];
    (wa.footprint[al], wa.footprint[ar]) = (q1, q2);
    let wb = &mut walls[b.wall];
    (wb.footprint[bl], wb.footprint[br]) = (q2, q1);
    true
}

/// A T join of end `a` into the span of wall `j`: the distance the end moves and its new corners
/// (left and right of the inward direction), on `j`'s near face.
fn tee_cut(walls: &[Wall], a: WallEnd, j: usize) -> Option<(f64, (P, P))> {
    let (wa, wb) = (&walls[a.wall], &walls[j]);
    if !wa.overlaps_vertically(wb) || !angle_ok(a.inward, wb.g.direction()) {
        return None;
    }
    let (u, hb) = (wb.g.direction(), wb.half());
    let outward = mul(a.inward, -1.0);
    // Where A's location line crosses B's: `s` along A's outward direction, `t` along B.
    let (_, s, t) = intersect(a.point, outward, wb.g.start, u)?;
    let reach = hb + TOL_JOIN;
    // The end may stop short of B by up to half its width, or overshoot to its far face.
    if s < -hb - TOL_JOIN || s > reach {
        return None;
    }
    let margin = wa.half() + TOL_JOIN;
    if t < margin || t > wb.g.length() - margin {
        return None;
    }
    // B's near face is on the side of B where A's body is.
    let body = add(a.point, mul(a.inward, wa.g.length() / 2.0));
    let side = dot(sub(body, wb.g.start), wb.g.normal()).signum();
    let near = add(wb.g.start, mul(wb.g.normal(), hb * side));
    let ha = wa.half();
    let left_face = add(a.point, mul(left(a.inward), ha));
    let right_face = add(a.point, mul(left(a.inward), -ha));
    let (l, ..) = intersect(left_face, a.inward, near, u)?;
    let (r, ..) = intersect(right_face, a.inward, near, u)?;
    Some((s.abs(), (l, r)))
}
