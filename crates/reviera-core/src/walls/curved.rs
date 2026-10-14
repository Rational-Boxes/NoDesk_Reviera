// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curved walls (ARC-WALL-002): a wall whose location curve passes through `through` points
//! between its start and end:
//! - one point: a **circular arc** through the start, that point and the end;
//! - two or more: a smooth **spline** through them all (centripetal Catmull–Rom).
//!
//! The curve is sampled into a fine polyline (arcs at most 3° per segment; splines 16
//! segments per span). That gives:
//! - the curve's length;
//! - the point and direction at any distance along it (doors and windows are placed by it);
//! - the wall's body: short prisms between the curve offset to each side, with mitred shared
//!   edges, so the body is closed and watertight. Doors and windows cut it along the curve.
//!
//! Curved walls do not join yet (their ends butt), and their tops do not attach to roofs.

use super::{Line, WallRecipe};
use crate::bodies::Mesh;

/// A curve sampled as a polyline, with the distance along it at each point.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub points: Vec<[f64; 2]>,
    pub along: Vec<f64>,
}

impl Path {
    fn new(points: Vec<[f64; 2]>) -> Self {
        let mut along = vec![0.0];
        for w in points.windows(2) {
            let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            along.push(along.last().expect("one") + d);
        }
        Self { points, along }
    }

    pub fn length(&self) -> f64 {
        *self.along.last().expect("points")
    }

    /// The segment holding distance `s` (clamped to the curve).
    fn segment(&self, s: f64) -> usize {
        let n = self.points.len() - 1;
        self.along[1..]
            .iter()
            .position(|a| *a >= s)
            .unwrap_or(n - 1)
            .min(n - 1)
    }

    /// A segment's unit direction.
    fn direction(&self, i: usize) -> [f64; 2] {
        let (a, b) = (self.points[i], self.points[i + 1]);
        let len = (self.along[i + 1] - self.along[i]).max(1e-12);
        [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
    }

    /// The tangent at sample point `k`: its two segments' directions averaged (exact on an arc).
    fn tangent(&self, k: usize) -> [f64; 2] {
        let n = self.points.len() - 1;
        let inner = |k: usize| {
            let (u, v) = (self.direction(k - 1), self.direction(k));
            let d = [u[0] + v[0], u[1] + v[1]];
            let l = d[0].hypot(d[1]).max(1e-12);
            [d[0] / l, d[1] / l]
        };
        let d = match k {
            // At the ends, the first (last) segment's direction turned as far again as the next
            // point's tangent turns from it: exact on an arc.
            0 if n >= 2 => {
                let (d0, t1) = (self.direction(0), inner(1));
                [2.0 * d0[0] - t1[0], 2.0 * d0[1] - t1[1]]
            }
            k if k >= n && n >= 2 => {
                let (d1, t1) = (self.direction(n - 1), inner(n - 1));
                [2.0 * d1[0] - t1[0], 2.0 * d1[1] - t1[1]]
            }
            0 => self.direction(0),
            k if k >= n => self.direction(n - 1),
            k => inner(k),
        };
        let l = d[0].hypot(d[1]).max(1e-12);
        [d[0] / l, d[1] / l]
    }

    /// The point and unit tangent at distance `s` along the curve (the tangent blended between
    /// the sample points').
    pub fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let i = self.segment(s);
        let (a, b) = (self.points[i], self.points[i + 1]);
        let len = (self.along[i + 1] - self.along[i]).max(1e-12);
        let t = ((s - self.along[i]) / len).clamp(0.0, 1.0);
        let (u, v) = (self.tangent(i), self.tangent(i + 1));
        let d = [u[0] + (v[0] - u[0]) * t, u[1] + (v[1] - u[1]) * t];
        let l = d[0].hypot(d[1]).max(1e-12);
        (
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            [d[0] / l, d[1] / l],
        )
    }

    /// The distance along the curve of the point nearest to `p`.
    pub fn project(&self, p: [f64; 2]) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for i in 0..self.points.len() - 1 {
            let (a, b) = (self.points[i], self.points[i + 1]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-24);
            let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
            let q = [a[0] + d[0] * t, a[1] + d[1] * t];
            let dist = (p[0] - q[0]).hypot(p[1] - q[1]);
            if dist < best.0 {
                best = (dist, self.along[i] + t * l2.sqrt());
            }
        }
        best.1
    }

    /// The curve offset by `d` to its left: along each point's tangent normal (square to the
    /// curve at the ends), mitred at the points between.
    pub fn offset(&self, d: f64) -> Vec<[f64; 2]> {
        let n = self.points.len();
        (0..n)
            .map(|k| {
                let t = self.tangent(k);
                let m = [-t[1], t[0]];
                // Between segments, longer by the half-turn so both sides stay parallel.
                let scale = if k == 0 || k == n - 1 {
                    1.0
                } else {
                    let u = self.direction(k);
                    (m[0] * -u[1] + m[1] * u[0]).max(0.2).recip()
                };
                let p = self.points[k];
                [p[0] + m[0] * d * scale, p[1] + m[1] * d * scale]
            })
            .collect()
    }
}

/// The circle through three points: centre and radius; `None` when they are in line.
pub fn circle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let x = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let y = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    Some(([x, y], (a[0] - x).hypot(a[1] - y)))
}

/// The arc from `a` through `m` to `b`, sampled at most 3° per segment.
fn arc(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let (c, r) = circle(a, m, b)?;
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let tau = std::f64::consts::TAU;
    let (a0, am, a1) = (ang(a), ang(m), ang(b));
    // Counter-clockwise sweeps from a to b and to m; the arc goes the way that passes m.
    let ccw = |x: f64| (x - a0).rem_euclid(tau);
    let sweep = if ccw(am) < ccw(a1) {
        ccw(a1)
    } else {
        ccw(a1) - tau
    };
    let n = ((sweep.abs().to_degrees() / 3.0).ceil() as usize).max(2);
    Some(
        (0..=n)
            .map(|k| {
                let t = a0 + sweep * k as f64 / n as f64;
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect(),
    )
}

/// A centripetal Catmull–Rom spline through `pts`, 16 segments per span.
fn spline(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let n = pts.len();
    // End tangents by reflecting the second and second-to-last points.
    let get = |i: isize| -> [f64; 2] {
        if i < 0 {
            let (p0, p1) = (pts[0], pts[1]);
            [2.0 * p0[0] - p1[0], 2.0 * p0[1] - p1[1]]
        } else if i as usize >= n {
            let (p0, p1) = (pts[n - 1], pts[n - 2]);
            [2.0 * p0[0] - p1[0], 2.0 * p0[1] - p1[1]]
        } else {
            pts[i as usize]
        }
    };
    let mut out = vec![pts[0]];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            get(i as isize - 1),
            get(i as isize),
            get(i as isize + 1),
            get(i as isize + 2),
        );
        let knot = |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]).hypot(b[1] - a[1])).sqrt().max(1e-9);
        let (t0, t1) = (0.0, knot(p0, p1));
        let (t2, t3) = (t1 + knot(p1, p2), t1 + knot(p1, p2) + knot(p2, p3));
        let lerp = |a: [f64; 2], b: [f64; 2], ta: f64, tb: f64, t: f64| {
            let w = (t - ta) / (tb - ta);
            [a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w]
        };
        for k in 1..=16 {
            let t = t1 + (t2 - t1) * k as f64 / 16.0;
            let a1 = lerp(p0, p1, t0, t1, t);
            let a2 = lerp(p1, p2, t1, t2, t);
            let a3 = lerp(p2, p3, t2, t3, t);
            let b1 = lerp(a1, a2, t0, t2, t);
            let b2 = lerp(a2, a3, t1, t3, t);
            out.push(lerp(b1, b2, t1, t2, t));
        }
    }
    out
}

/// The sampled curve of a wall from its start, `through` points and end; `None` for a
/// straight wall (or through points in line with it).
pub fn path(curve: Line, through: &[[f64; 2]]) -> Option<Path> {
    match through {
        [] => None,
        [m] => arc(curve.start, *m, curve.end).map(Path::new),
        many => {
            let mut pts = vec![curve.start];
            pts.extend_from_slice(many);
            pts.push(curve.end);
            Some(Path::new(spline(&pts)))
        }
    }
}

/// A wall recipe's sampled curve, if it is curved.
pub fn path_of(r: &WallRecipe) -> Option<Path> {
    path(r.curve, &r.through)
}

/// A closed prism over the plan quad `q` (any orientation) from `z0` to `z1`.
fn prism(q: [[f64; 2]; 4], z0: f64, z1: f64) -> Mesh {
    let area: f64 = (0..4)
        .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1])
        .sum();
    let q = if area < 0.0 {
        [q[0], q[3], q[2], q[1]]
    } else {
        q
    };
    crate::bodies::extrude(&q, &[], z0, z1)
}

/// The wall body of one plan quad (`[start-left, end-left, end-right, start-right]`) from `z0`:
/// as a short straight wall, for attaching its top.
fn piece_body(q: [[f64; 2]; 4], width: f64, z0: f64, height: f64) -> super::WallBody {
    let start = [(q[0][0] + q[3][0]) / 2.0, (q[0][1] + q[3][1]) / 2.0];
    let end = [(q[1][0] + q[2][0]) / 2.0, (q[1][1] + q[2][1]) / 2.0];
    super::WallBody {
        geometry: super::WallGeometry {
            start,
            end,
            base_z: z0,
            height,
            width,
        },
        footprint: [q[3], q[2], q[1], q[0]],
    }
}

/// Whether a curved wall's top can attach to `roof` all along; the lowest point of the top.
pub fn attach_check(
    p: &Path,
    width: f64,
    base_z: f64,
    roof: &crate::roofs::RoofGeometry,
) -> crate::error::Result<f64> {
    let (left, right) = (p.offset(width / 2.0), p.offset(-width / 2.0));
    let mut z_min = f64::INFINITY;
    for i in 0..p.points.len() - 1 {
        let q = [left[i], left[i + 1], right[i + 1], right[i]];
        let a = super::attach::attach_to(roof, &piece_body(q, width, base_z, 1.0))?;
        z_min = z_min.min(a.z_min);
    }
    Ok(z_min)
}

/// A curved wall's body: per segment of its curve (split further at opening edges), the prism
/// between the curve offset half the width to each side, from the base to the top, less each
/// opening's `along` and height range (`cuts`, as for straight walls). With `roof`, the top
/// follows the roof's underside (ARC-WALL-008), piece by piece.
pub fn body(
    p: &Path,
    width: f64,
    base_z: f64,
    top_z: f64,
    cuts: &[super::openings::Cut],
    roof: Option<&crate::roofs::RoofGeometry>,
) -> Vec<Mesh> {
    let (left, right) = (p.offset(width / 2.0), p.offset(-width / 2.0));
    let mut out = Vec::new();
    for i in 0..p.points.len() - 1 {
        let (s0, s1) = (p.along[i], p.along[i + 1]);
        // Split at the openings' edges within the segment.
        let mut stops = vec![s0, s1];
        for c in cuts {
            for x in c.x {
                if x > s0 + 1e-9 && x < s1 - 1e-9 {
                    stops.push(x);
                }
            }
        }
        stops.sort_by(f64::total_cmp);
        let at = |s: f64, side: &[[f64; 2]]| {
            let t = ((s - s0) / (s1 - s0).max(1e-12)).clamp(0.0, 1.0);
            let (a, b) = (side[i], side[i + 1]);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        };
        for w in stops.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a < 1e-9 {
                continue;
            }
            let mid = (a + b) / 2.0;
            let q = [at(a, &left), at(b, &left), at(b, &right), at(a, &right)];
            // The heights left solid: the full height less the openings over this piece.
            let mut solid = vec![(base_z, top_z)];
            for c in cuts.iter().filter(|c| c.x[0] <= mid && mid <= c.x[1]) {
                solid = solid
                    .into_iter()
                    .flat_map(|(z0, z1)| {
                        let mut keep = Vec::new();
                        if c.z[0] > z0 {
                            keep.push((z0, c.z[0].min(z1)));
                        }
                        if c.z[1] < z1 {
                            keep.push((c.z[1].max(z0), z1));
                        }
                        keep
                    })
                    .filter(|(z0, z1)| z1 - z0 > 1e-9)
                    .collect();
            }
            let last = solid.len();
            for (k, (z0, z1)) in solid.into_iter().enumerate() {
                // The run reaching the top follows the roof, when the wall is attached.
                let attached = (k + 1 == last && (z1 - top_z).abs() < 1e-9)
                    .then_some(roof)
                    .flatten()
                    .and_then(|r| {
                        super::attach::attach_to(r, &piece_body(q, width, z0, z1 - z0)).ok()
                    });
                match attached {
                    Some(a) => out.push(a.solid),
                    None => out.push(prism(q, z0, z1)),
                }
            }
        }
    }
    out
}

/// A curved wall's plan outline: its two sides (the `-` side, then the `+` side, each from
/// start to end) and its two ends (each `[- side, + side]`).
pub type Outline = ([Vec<[f64; 2]>; 2], [[[f64; 2]; 2]; 2]);

/// A curved wall's faces in plan, for room boundaries.
pub fn outline(p: &Path, width: f64) -> Outline {
    let (plus, minus) = (p.offset(width / 2.0), p.offset(-width / 2.0));
    let n = plus.len();
    (
        [minus.clone(), plus.clone()],
        [[minus[0], plus[0]], [minus[n - 1], plus[n - 1]]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcs_through_three_points() {
        // A half circle of radius 5 from (−5, 0) through (0, 5) to (5, 0).
        let p = path(
            Line {
                start: [-5.0, 0.0],
                end: [5.0, 0.0],
            },
            &[[0.0, 5.0]],
        )
        .unwrap();
        let exact = std::f64::consts::PI * 5.0;
        assert!((p.length() - exact).abs() / exact < 2e-4, "{}", p.length());
        // Halfway along: the top, heading −x... no: heading +x (clockwise from the left).
        let (mid, d) = p.at(p.length() / 2.0);
        assert!(
            (mid[1] - 5.0).abs() < 0.01 && mid[0].abs() < 0.01,
            "{mid:?}"
        );
        assert!(d[0] > 0.99, "{d:?}");
        // The other way round passes below.
        let q = path(
            Line {
                start: [-5.0, 0.0],
                end: [5.0, 0.0],
            },
            &[[0.0, -5.0]],
        )
        .unwrap();
        assert!(q.at(q.length() / 2.0).0[1] < -4.99);
        // In line: straight.
        assert!(
            path(
                Line {
                    start: [0.0, 0.0],
                    end: [4.0, 0.0],
                },
                &[[2.0, 0.0]],
            )
            .is_none()
        );
    }

    #[test]
    fn a_curved_body_is_the_annulus_less_its_openings() {
        let p = path(
            Line {
                start: [5.0, 0.0],
                end: [-5.0, 0.0],
            },
            &[[0.0, 5.0]],
        )
        .unwrap();
        let volume = |parts: &[Mesh]| -> f64 {
            parts
                .iter()
                .map(|(pts, idx)| {
                    idx.chunks(3)
                        .map(|t| {
                            let (a, b, c) =
                                (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
                            (a[0] * (b[1] * c[2] - b[2] * c[1])
                                - a[1] * (b[0] * c[2] - b[2] * c[0])
                                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                                / 6.0
                        })
                        .sum::<f64>()
                })
                .sum()
        };
        // A half annulus 0.2 m thick, 3 m high: π (5.1² − 4.9²) / 2 × 3.
        let exact = std::f64::consts::PI * (5.1f64.powi(2) - 4.9f64.powi(2)) / 2.0 * 3.0;
        let v = volume(&body(&p, 0.2, 0.0, 3.0, &[], None));
        assert!((v - exact).abs() / exact < 1e-3, "{v} vs {exact}");
        // A 1 × 2 m opening along the curve takes about its area times the thickness.
        let cut = super::super::openings::Cut {
            x: [7.0, 8.0],
            z: [0.0, 2.0],
        };
        let w = volume(&body(&p, 0.2, 0.0, 3.0, &[cut], None));
        assert!(((v - w) - 1.0 * 2.0 * 0.2).abs() < 0.01, "{}", v - w);
    }

    #[test]
    fn splines_pass_through_their_points() {
        let pts = [[0.0, 0.0], [3.0, 2.0], [6.0, -1.0], [9.0, 0.0]];
        let p = path(
            Line {
                start: pts[0],
                end: pts[3],
            },
            &pts[1..3],
        )
        .unwrap();
        for q in pts {
            assert!(
                p.points
                    .iter()
                    .any(|x| (x[0] - q[0]).hypot(x[1] - q[1]) < 1e-9),
                "{q:?}"
            );
        }
    }
}
