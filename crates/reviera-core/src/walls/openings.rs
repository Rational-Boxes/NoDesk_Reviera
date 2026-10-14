// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall bodies cut by openings (ARC-HOST-001), rectangular or shaped (ARC-WIN-002), for
//! Reviera's own display. IFC keeps
//! the uncut body and an `IfcOpeningElement` per opening, so other tools cut the wall themselves.
//!
//! An opening passes through the whole wall: an interval along the wall and an interval of
//! height. The cut body is built as one closed mesh in the wall's local frame (X along the wall
//! from its start, Y toward the exterior, Z up): the two long faces with the openings left out
//! (split on a grid of the openings' edges, so neighbouring pieces share their vertices), the
//! end faces, the top and bottom (split where an opening reaches them) and the reveals.
//!
//! A shaped opening (a round or arched window) has a convex outline. Its bounding rectangle is
//! left out of the grid like any opening, then the corners between the rectangle and the
//! outline are filled back strip by strip (between the outline's points), and the reveals follow
//! the outline.

use super::WallBody;

/// An opening through a wall, in its local frame: `x` along the wall (metres from its start),
/// `z` absolute elevations; for a shaped opening, its convex `outline` (counter-clockwise, in
/// the same x and z) within that box. Empty for a rectangle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cut {
    pub x: [f64; 2],
    pub z: [f64; 2],
    pub outline: Vec<[f64; 2]>,
}

impl Cut {
    pub fn rect(x: [f64; 2], z: [f64; 2]) -> Self {
        Self {
            x,
            z,
            outline: Vec::new(),
        }
    }

    /// The heights open at `x` along the wall; `None` outside the opening.
    pub fn z_at(&self, x: f64) -> Option<[f64; 2]> {
        if x < self.x[0] - 1e-9 || x > self.x[1] + 1e-9 {
            return None;
        }
        if self.outline.is_empty() {
            return Some(self.z);
        }
        crate::inserts::shape::span_at(&self.outline, x)
    }

    /// Where the open heights change slope along the wall: the outline's points (none for a
    /// rectangle).
    pub fn stations(&self) -> Vec<f64> {
        crate::inserts::shape::stations(&self.outline)
    }
}

/// The interval along the wall where its section is whole (between the joined ends), in which
/// openings may lie.
pub fn clear_span(body: &WallBody) -> [f64; 2] {
    let f = body.local_footprint();
    [f[0][0].max(f[3][0]), f[1][0].min(f[2][0])]
}

struct Mesh {
    positions: Vec<[f64; 3]>,
    indices: Vec<u32>,
    origin: [f64; 2],
    u: [f64; 2],
    n: [f64; 2],
}

impl Mesh {
    fn world(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [
            self.origin[0] + self.u[0] * x + self.n[0] * y,
            self.origin[1] + self.u[1] * x + self.n[1] * y,
            z,
        ]
    }

    /// A planar polygon (local points, convex, in order) facing `outward` (local direction).
    fn polygon(&mut self, points: &[[f64; 3]], outward: [f64; 3]) {
        if points.len() < 3 {
            return;
        }
        let (a, b, c) = (points[0], points[1], points[2]);
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let flip = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2] < 0.0;
        let base = self.positions.len() as u32;
        for p in points {
            let w = self.world(*p);
            self.positions.push(w);
        }
        for k in 1..points.len() as u32 - 1 {
            if flip {
                self.indices.extend([base, base + k + 1, base + k]);
            } else {
                self.indices.extend([base, base + k, base + k + 1]);
            }
        }
    }
}

/// Clips a convex polygon to `lo <= x <= hi`.
fn clip_x(poly: &[[f64; 2]], lo: f64, hi: f64) -> Vec<[f64; 2]> {
    let clip = |poly: Vec<[f64; 2]>, keep: &dyn Fn(f64) -> bool, at: f64| {
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (pin, qin) = (keep(p[0]), keep(q[0]));
            if pin {
                out.push(p);
            }
            if pin != qin {
                let t = (at - p[0]) / (q[0] - p[0]);
                out.push([at, p[1] + (q[1] - p[1]) * t]);
            }
        }
        out
    };
    let poly = clip(poly.to_vec(), &|x| x >= lo, lo);
    clip(poly, &|x| x <= hi, hi)
}

/// The intervals of `[lo, hi]` not covered by `holes`.
fn remaining(lo: f64, hi: f64, holes: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut holes = holes.to_vec();
    holes.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out = Vec::new();
    let mut at = lo;
    for [a, b] in holes {
        if a > at {
            out.push([at, a.min(hi)]);
        }
        at = at.max(b);
    }
    if at < hi {
        out.push([at, hi]);
    }
    out.retain(|[a, b]| b - a > 1e-9);
    out
}

/// The body of a wall with openings, in project coordinates. Openings outside the clear span or
/// the wall's height are skipped (the caller reports them).
pub fn cut_mesh(body: &WallBody, cuts: &[Cut]) -> (Vec<[f64; 3]>, Vec<u32>) {
    let g = &body.geometry;
    let (z0, z1) = (g.base_z, g.base_z + g.height);
    let span = clear_span(body);
    let cuts: Vec<Cut> = cuts
        .iter()
        .filter(|c| {
            c.x[0] >= span[0] - 1e-9
                && c.x[1] <= span[1] + 1e-9
                && c.x[1] - c.x[0] > 1e-6
                && c.z[0] >= z0 - 1e-9
                && c.z[1] <= z1 + 1e-9
                && c.z[1] - c.z[0] > 1e-6
        })
        .map(|c| Cut {
            x: c.x,
            z: [c.z[0].max(z0), c.z[1].min(z1)],
            outline: c.outline.clone(),
        })
        .collect();
    if cuts.is_empty() {
        return body.mesh();
    }
    let f = body.local_footprint();
    let mut m = Mesh {
        positions: Vec::new(),
        indices: Vec::new(),
        origin: g.start,
        u: g.direction(),
        n: g.normal(),
    };
    let (yi, ye) = (f[0][1], f[2][1]);

    // Long faces: a grid of the openings' edges, leaving out the cells inside an opening.
    for (y, x0, x1, outward) in [
        (yi, f[0][0], f[1][0], [0.0, -1.0, 0.0]),
        (ye, f[3][0], f[2][0], [0.0, 1.0, 0.0]),
    ] {
        let mut xs = vec![x0, x1];
        let mut zs = vec![z0, z1];
        for c in &cuts {
            xs.extend(c.x);
            zs.extend(c.z);
        }
        xs.sort_by(f64::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        zs.sort_by(f64::total_cmp);
        zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for xw in xs.windows(2) {
            for zw in zs.windows(2) {
                let (cx, cz) = ((xw[0] + xw[1]) / 2.0, (zw[0] + zw[1]) / 2.0);
                let open = cuts
                    .iter()
                    .any(|c| c.x[0] < cx && cx < c.x[1] && c.z[0] < cz && cz < c.z[1]);
                if !open {
                    m.polygon(
                        &[
                            [xw[0], y, zw[0]],
                            [xw[1], y, zw[0]],
                            [xw[1], y, zw[1]],
                            [xw[0], y, zw[1]],
                        ],
                        outward,
                    );
                }
            }
        }
    }
    // Shaped openings: the corners of their boxes outside the outline, on both long faces.
    for c in cuts.iter().filter(|c| !c.outline.is_empty()) {
        for w in c.stations().windows(2) {
            let (Some(a), Some(b)) = (c.z_at(w[0]), c.z_at(w[1])) else {
                continue;
            };
            for (y, outward) in [(yi, [0.0, -1.0, 0.0]), (ye, [0.0, 1.0, 0.0])] {
                let below = [
                    [w[0], y, c.z[0]],
                    [w[1], y, c.z[0]],
                    [w[1], y, b[0]],
                    [w[0], y, a[0]],
                ];
                let above = [
                    [w[0], y, a[1]],
                    [w[1], y, b[1]],
                    [w[1], y, c.z[1]],
                    [w[0], y, c.z[1]],
                ];
                for quad in [below, above] {
                    m.polygon(&distinct(&quad), outward);
                }
            }
        }
    }
    // End faces.
    for (a, b, outward) in [
        (f[3], f[0], [-1.0, 0.0, 0.0]),
        (f[1], f[2], [1.0, 0.0, 0.0]),
    ] {
        m.polygon(
            &[
                [a[0], a[1], z0],
                [b[0], b[1], z0],
                [b[0], b[1], z1],
                [a[0], a[1], z1],
            ],
            outward,
        );
    }
    // Bottom and top, without the openings that reach them.
    let outline: Vec<[f64; 2]> = f.to_vec();
    let (lo, hi) = (
        f.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
        f.iter().map(|p| p[0]).fold(f64::MIN, f64::max),
    );
    for (z, outward) in [(z0, [0.0, 0.0, -1.0]), (z1, [0.0, 0.0, 1.0])] {
        let holes: Vec<[f64; 2]> = cuts
            .iter()
            .filter(|c| (c.z[0] - z).abs() < 1e-9 || (c.z[1] - z).abs() < 1e-9)
            .flat_map(|c| {
                if c.outline.is_empty() {
                    return vec![c.x];
                }
                // Where the outline runs along the bottom or top.
                c.stations()
                    .windows(2)
                    .filter(|w| {
                        [w[0], w[1]].iter().all(|x| {
                            c.z_at(*x)
                                .is_some_and(|s| (s[0] - z).abs() < 1e-9 || (s[1] - z).abs() < 1e-9)
                        })
                    })
                    .map(|w| [w[0], w[1]])
                    .collect()
            })
            .collect();
        for [a, b] in remaining(lo, hi, &holes) {
            let piece = clip_x(&outline, a, b);
            let points: Vec<[f64; 3]> = piece.iter().map(|p| [p[0], p[1], z]).collect();
            m.polygon(&points, outward);
        }
    }
    // Reveals: jambs, sill and head; a shaped opening's follow its outline.
    for c in &cuts {
        if !c.outline.is_empty() {
            shaped_reveals(&mut m, c, yi, ye, z0, z1);
            continue;
        }
        let [a, b] = c.x;
        let [c0, c1] = c.z;
        for (x, dir) in [(a, 1.0), (b, -1.0)] {
            m.polygon(
                &[[x, yi, c0], [x, ye, c0], [x, ye, c1], [x, yi, c1]],
                [dir, 0.0, 0.0],
            );
        }
        for (z, dir) in [(c0, 1.0), (c1, -1.0)] {
            if (z - z0).abs() < 1e-9 || (z - z1).abs() < 1e-9 {
                continue;
            }
            m.polygon(
                &[[a, yi, z], [b, yi, z], [b, ye, z], [a, ye, z]],
                [0.0, 0.0, dir],
            );
        }
    }
    (m.positions, m.indices)
}

/// A polygon without repeated points (a quad with a corner on the box edge is a triangle).
fn distinct(points: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let same = |a: &[f64; 3], b: &[f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
    let mut out: Vec<[f64; 3]> = Vec::new();
    for p in points {
        if !out.last().is_some_and(|q| same(q, p)) {
            out.push(*p);
        }
    }
    while out.len() > 1 && same(&out[0], &out[out.len() - 1]) {
        out.pop();
    }
    out
}

/// The reveals of a shaped opening: per strip between the outline's points, its lower and upper
/// edge across the wall (facing into the opening), and the jambs where its ends are upright.
/// Edges on the wall's bottom or top are left open.
fn shaped_reveals(m: &mut Mesh, c: &Cut, yi: f64, ye: f64, z0: f64, z1: f64) {
    let stations = c.stations();
    for w in stations.windows(2) {
        let (Some(a), Some(b)) = (c.z_at(w[0]), c.z_at(w[1])) else {
            continue;
        };
        let (dx, (lo_dz, hi_dz)) = (w[1] - w[0], (b[0] - a[0], b[1] - a[1]));
        let on = |za: f64, zb: f64, z: f64| (za - z).abs() < 1e-9 && (zb - z).abs() < 1e-9;
        if !on(a[0], b[0], z0) {
            m.polygon(
                &[
                    [w[0], yi, a[0]],
                    [w[1], yi, b[0]],
                    [w[1], ye, b[0]],
                    [w[0], ye, a[0]],
                ],
                [-lo_dz, 0.0, dx],
            );
        }
        if !on(a[1], b[1], z1) {
            m.polygon(
                &[
                    [w[0], yi, a[1]],
                    [w[1], yi, b[1]],
                    [w[1], ye, b[1]],
                    [w[0], ye, a[1]],
                ],
                [hi_dz, 0.0, -dx],
            );
        }
    }
    for (x, dir) in [
        (stations.first().copied(), 1.0),
        (stations.last().copied(), -1.0),
    ] {
        let Some(x) = x else { continue };
        let Some([lo, hi]) = c.z_at(x) else { continue };
        if hi - lo > 1e-9 {
            m.polygon(
                &[[x, yi, lo], [x, ye, lo], [x, ye, hi], [x, yi, hi]],
                [dir, 0.0, 0.0],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walls::WallGeometry;

    fn body() -> WallBody {
        let geometry = WallGeometry {
            start: [0.0, 0.0],
            end: [5.0, 0.0],
            base_z: 0.0,
            height: 3.0,
            width: 0.2,
        };
        WallBody {
            geometry,
            footprint: geometry.footprint(),
        }
    }

    /// Signed volume of a closed mesh (divergence theorem).
    fn volume((p, i): &(Vec<[f64; 3]>, Vec<u32>)) -> f64 {
        i.chunks(3)
            .map(|t| {
                let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    #[test]
    fn openings_remove_their_volume() {
        let b = body();
        assert!((volume(&b.mesh()) - 3.0).abs() < 1e-9);
        // A door from the floor and a window: 0.9 × 2.1 and 1.2 × 1.2 through 0.2 m.
        let cut = cut_mesh(
            &b,
            &[
                Cut::rect([0.5, 1.4], [0.0, 2.1]),
                Cut::rect([2.5, 3.7], [0.9, 2.1]),
            ],
        );
        let expected = 3.0 - 0.2 * (0.9 * 2.1 + 1.2 * 1.2);
        assert!((volume(&cut) - expected).abs() < 1e-9, "{}", volume(&cut));
    }

    #[test]
    fn openings_outside_the_clear_span_are_skipped() {
        let b = body();
        let cut = cut_mesh(&b, &[Cut::rect([4.5, 5.5], [0.0, 2.0])]);
        assert_eq!(cut, b.mesh());
        assert_eq!(clear_span(&b), [0.0, 5.0]);
    }
}
