// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The volume overlap engine (ARC-MOD-030): where the closed bodies of elements intersect, and
//! by how much. It is the one low-level operation behind:
//! - flagging overlaps for boolean resolution (ARC-MOD-028);
//! - clash detection (COORD);
//! - virtual-volume checks such as door swings and accessibility clearances;
//! - automatic openings for pipes, ducts and conduits.
//!
//! **Broad phase.** Each body's axis-aligned box. Boxes are sorted along x and swept, so only
//! pairs whose boxes overlap by more than the tolerance in every axis go on.
//!
//! **Narrow phase.** Within the two boxes' common box, a grid of vertical lines (up to
//! [`Options::resolution`] each way, at cell centres slightly offset to miss mesh edges). On
//! each line, every closed mesh of a body is crossed an even number of times. Paired, the
//! crossings give the intervals inside it, and the body's intervals are their union. The
//! overlap's length on the line is the length of the intersection of the two bodies' intervals,
//! less anything shorter than the tolerance.
//!
//! Summed over the cells this gives:
//! - the overlap's volume (the cells' area times the lengths);
//! - its depth (the longest vertical run);
//! - its box and its centroid.
//!
//! Faces that only touch give lengths of zero, so contact is not an overlap. The volume
//! converges with the resolution; slivers thinner than a cell across both plan axes may be
//! missed, so callers wanting certainty raise the resolution.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::{Body, Mesh};
use crate::id::ElementId;

/// How finely to measure, and what to ignore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Overlaps thinner than this (metres, along each vertical line) are contact, not overlap.
    pub tolerance: f64,
    /// Overlaps smaller than this (cubic metres) are not reported.
    pub min_volume: f64,
    /// Lines across the common box, each way (at most).
    pub resolution: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tolerance: 0.001,
            min_volume: 1e-6,
            resolution: 48,
        }
    }
}

/// An overlap between two bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Overlap {
    pub a: ElementId,
    pub b: ElementId,
    /// Cubic metres.
    pub volume: f64,
    /// The longest vertical run through both (metres).
    pub depth: f64,
    /// The overlap's box.
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
}

/// A body prepared for measuring: its box and its triangles.
struct Prepared<'a> {
    body: &'a Body,
    lo: [f64; 3],
    hi: [f64; 3],
}

fn prepare(b: &Body) -> Option<Prepared<'_>> {
    let (lo, hi) = b.bounds()?;
    Some(Prepared { body: b, lo, hi })
}

/// The heights where the vertical line at `(x, y)` crosses a mesh, sorted.
fn crossings(mesh: &Mesh, tris: &[usize], x: f64, y: f64, out: &mut Vec<f64>) {
    out.clear();
    let (p, idx) = mesh;
    for &t in tris {
        let (a, b, c) = (
            p[idx[3 * t] as usize],
            p[idx[3 * t + 1] as usize],
            p[idx[3 * t + 2] as usize],
        );
        let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if d.abs() < 1e-18 {
            continue; // vertical: parallel to the line
        }
        // Barycentric coordinates of (x, y) in the triangle's plan.
        let u = ((x - a[0]) * (c[1] - a[1]) - (y - a[1]) * (c[0] - a[0])) / d;
        let v = ((b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])) / d;
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            continue;
        }
        out.push(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]));
    }
    out.sort_by(f64::total_cmp);
}

/// The union of sorted-by-start intervals.
fn union(mut v: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    v.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out: Vec<[f64; 2]> = Vec::new();
    for iv in v {
        match out.last_mut() {
            Some(last) if iv[0] <= last[1] => last[1] = last[1].max(iv[1]),
            _ => out.push(iv),
        }
    }
    out
}

/// The intervals of the line at `(x, y)` inside a body (the union over its closed parts).
fn inside(
    body: &Body,
    tris: &[Vec<usize>],
    x: f64,
    y: f64,
    scratch: &mut Vec<f64>,
) -> Vec<[f64; 2]> {
    let mut ivs = Vec::new();
    for (part, t) in body.parts.iter().zip(tris) {
        crossings(&part.mesh, t, x, y, scratch);
        // A closed mesh crosses evenly; an odd count (a grazed edge) drops the last.
        for pair in scratch.as_chunks::<2>().0 {
            if pair[1] > pair[0] {
                ivs.push([pair[0], pair[1]]);
            }
        }
    }
    union(ivs)
}

/// The triangles of each part whose plan box meets the box `lo`–`hi` (plan only).
fn candidates(body: &Body, lo: [f64; 2], hi: [f64; 2]) -> Vec<Vec<usize>> {
    body.parts
        .iter()
        .map(|part| {
            let (p, idx) = &part.mesh;
            (0..idx.len() / 3)
                .filter(|t| {
                    let v = [idx[3 * t], idx[3 * t + 1], idx[3 * t + 2]].map(|i| p[i as usize]);
                    let (x0, x1) = (
                        v.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min),
                        v.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max),
                    );
                    let (y0, y1) = (
                        v.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min),
                        v.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max),
                    );
                    x1 >= lo[0] && x0 <= hi[0] && y1 >= lo[1] && y0 <= hi[1]
                })
                .collect()
        })
        .collect()
}

/// How two prepared bodies overlap, if they do.
fn measure(a: &Prepared<'_>, b: &Prepared<'_>, o: &Options) -> Option<Overlap> {
    let lo: [f64; 3] = std::array::from_fn(|k| a.lo[k].max(b.lo[k]));
    let hi: [f64; 3] = std::array::from_fn(|k| a.hi[k].min(b.hi[k]));
    if (0..3).any(|k| hi[k] - lo[k] <= o.tolerance) {
        return None;
    }
    // Cells: square-ish, at most `resolution` each way.
    let (wx, wy) = (hi[0] - lo[0], hi[1] - lo[1]);
    let cell = (wx.max(wy) / o.resolution as f64).max(1e-6);
    let nx = ((wx / cell).ceil() as usize).clamp(1, o.resolution);
    let ny = ((wy / cell).ceil() as usize).clamp(1, o.resolution);
    let (dx, dy) = (wx / nx as f64, wy / ny as f64);
    let ta = candidates(a.body, [lo[0], lo[1]], [hi[0], hi[1]]);
    let tb = candidates(b.body, [lo[0], lo[1]], [hi[0], hi[1]]);
    let mut scratch = Vec::new();
    let (mut volume, mut depth) = (0.0f64, 0.0f64);
    let mut moment = [0.0f64; 3];
    let mut olo = [f64::INFINITY; 3];
    let mut ohi = [f64::NEG_INFINITY; 3];
    for i in 0..nx {
        for j in 0..ny {
            // Cell centres, nudged off the grid lines meshes are often built on.
            let (cx, cy) = (lo[0] + (i as f64 + 0.5) * dx, lo[1] + (j as f64 + 0.5) * dy);
            let x = cx + 1.234_567e-4 * dx;
            let y = cy + 2.345_678e-4 * dy;
            let ia = inside(a.body, &ta, x, y, &mut scratch);
            if ia.is_empty() {
                continue;
            }
            let ib = inside(b.body, &tb, x, y, &mut scratch);
            let (mut p, mut q) = (0, 0);
            while p < ia.len() && q < ib.len() {
                let z0 = ia[p][0].max(ib[q][0]);
                let z1 = ia[p][1].min(ib[q][1]);
                let len = z1 - z0;
                if len > o.tolerance {
                    let w = len * dx * dy;
                    volume += w;
                    depth = depth.max(len);
                    moment[0] += cx * w;
                    moment[1] += cy * w;
                    moment[2] += (z0 + z1) / 2.0 * w;
                    olo = [
                        olo[0].min(cx - dx / 2.0),
                        olo[1].min(cy - dy / 2.0),
                        olo[2].min(z0),
                    ];
                    ohi = [
                        ohi[0].max(cx + dx / 2.0),
                        ohi[1].max(cy + dy / 2.0),
                        ohi[2].max(z1),
                    ];
                }
                if ia[p][1] < ib[q][1] {
                    p += 1;
                } else {
                    q += 1;
                }
            }
        }
    }
    if volume <= o.min_volume {
        return None;
    }
    Some(Overlap {
        a: a.body.element,
        b: b.body.element,
        volume,
        depth,
        // The cells' box, kept within the common box.
        min: std::array::from_fn(|k| olo[k].max(lo[k])),
        max: std::array::from_fn(|k| ohi[k].min(hi[k])),
        centroid: moment.map(|m| m / volume),
    })
}

/// How two bodies overlap, if they do.
pub fn between(a: &Body, b: &Body, o: &Options) -> Option<Overlap> {
    measure(&prepare(a)?, &prepare(b)?, o)
}

/// Every overlap among `bodies`, or between `bodies` and `against` when it is given; pairs that
/// `skip` names are left out (for example elements that belong together). Each pair is reported
/// once, the lower element id first, sorted by volume, largest first.
pub fn find(
    bodies: &[Body],
    against: Option<&[Body]>,
    skip: &dyn Fn(ElementId, ElementId) -> bool,
    o: &Options,
) -> Vec<Overlap> {
    // One list: the first set, then (when given) the second, marked by side.
    let mut all: Vec<(Prepared<'_>, bool)> = bodies
        .iter()
        .filter_map(prepare)
        .map(|p| (p, false))
        .collect();
    if let Some(other) = against {
        all.extend(other.iter().filter_map(prepare).map(|p| (p, true)));
    }
    all.sort_by(|x, y| x.0.lo[0].total_cmp(&y.0.lo[0]));
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for i in 0..all.len() {
        let (a, side_a) = &all[i];
        for (b, side_b) in &all[i + 1..] {
            if b.lo[0] >= a.hi[0] - o.tolerance {
                break; // sorted: no later box reaches back
            }
            if against.is_some() && side_a == side_b {
                continue;
            }
            let (ea, eb) = (a.body.element, b.body.element);
            if ea == eb {
                continue;
            }
            let key = (ea.min(eb), ea.max(eb));
            if !seen.insert(key) || skip(key.0, key.1) {
                continue;
            }
            if (1..3).any(|k| a.hi[k].min(b.hi[k]) - a.lo[k].max(b.lo[k]) <= o.tolerance) {
                continue;
            }
            if let Some(mut ov) = measure(a, b, o) {
                if ov.a > ov.b {
                    std::mem::swap(&mut ov.a, &mut ov.b);
                }
                out.push(ov);
            }
        }
    }
    out.sort_by(|x, y| y.volume.total_cmp(&x.volume));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bodies::{Part, extrude};

    fn cube(id: u64, lo: [f64; 3], hi: [f64; 3]) -> Body {
        let ring = [
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ];
        Body {
            element: ElementId(id),
            parts: vec![Part {
                mesh: extrude(&ring, &[], lo[2], hi[2]),
                class: "IfcWall".into(),
                plan_hidden: false,
            }],
        }
    }

    #[test]
    fn boxes_overlap_touch_and_miss() {
        let o = Options::default();
        let a = cube(1, [0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let b = cube(2, [1.0, 1.0, 1.0], [3.0, 3.0, 3.0]);
        let ov = between(&a, &b, &o).unwrap();
        assert!((ov.volume - 1.0).abs() < 1e-9, "{}", ov.volume);
        assert!((ov.depth - 1.0).abs() < 1e-9);
        for k in 0..3 {
            assert!((ov.centroid[k] - 1.5).abs() < 1e-9);
            assert!((ov.min[k] - 1.0).abs() < 1e-9 && (ov.max[k] - 2.0).abs() < 1e-9);
        }
        // Touching faces: no overlap.
        let c = cube(3, [2.0, 0.0, 0.0], [4.0, 2.0, 2.0]);
        assert!(between(&a, &c, &o).is_none());
        // Sitting on top: no overlap.
        let d = cube(4, [0.0, 0.0, 2.0], [2.0, 2.0, 3.0]);
        assert!(between(&a, &d, &o).is_none());
        // Inside: the whole of the smaller one.
        let e = cube(5, [0.5, 0.5, 0.5], [1.0, 1.5, 1.0]);
        assert!((between(&a, &e, &o).unwrap().volume - 0.25).abs() < 1e-9);
        // Through a hole: none.
        let ring = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let slab = Body {
            element: ElementId(6),
            parts: vec![Part {
                mesh: extrude(&ring, &[hole], 0.0, 0.3),
                class: "IfcSlab".into(),
                plan_hidden: false,
            }],
        };
        let pipe = cube(7, [1.5, 1.5, -1.0], [2.5, 2.5, 1.0]);
        assert!(between(&slab, &pipe, &o).is_none());
        let pipe = cube(8, [0.5, 1.5, -1.0], [1.5, 2.5, 1.0]);
        let ov = between(&slab, &pipe, &o).unwrap();
        assert!((ov.volume - 0.5 * 1.0 * 0.3).abs() < 1e-9, "{}", ov.volume);
    }

    #[test]
    fn a_rotated_box_converges() {
        // A 45°-turned 2 × 2 square prism through a box: the exact overlap is a known area.
        let r = std::f64::consts::FRAC_1_SQRT_2 * 2.0;
        let diamond = [[0.0, -r], [r, 0.0], [0.0, r], [-r, 0.0]];
        let a = Body {
            element: ElementId(1),
            parts: vec![Part {
                mesh: extrude(&diamond, &[], 0.0, 1.0),
                class: "IfcColumn".into(),
                plan_hidden: false,
            }],
        };
        let b = cube(2, [0.0, -5.0, 0.0], [5.0, 5.0, 1.0]);
        // Half the diamond (area 4): 2 m³.
        let ov = between(&a, &b, &Options::default()).unwrap();
        assert!((ov.volume - 2.0).abs() < 0.05, "{}", ov.volume);
        let fine = Options {
            resolution: 256,
            ..Options::default()
        };
        assert!((between(&a, &b, &fine).unwrap().volume - 2.0).abs() < 0.01);
    }

    #[test]
    fn find_sweeps_pairs_once() {
        let bodies = vec![
            cube(1, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            cube(2, [0.5, 0.0, 0.0], [1.5, 1.0, 1.0]),
            cube(3, [1.0, 0.0, 0.0], [2.0, 1.0, 1.0]),
            cube(4, [10.0, 0.0, 0.0], [11.0, 1.0, 1.0]),
        ];
        let o = Options::default();
        let all = find(&bodies, None, &|_, _| false, &o);
        let pairs: Vec<(u64, u64)> = all.iter().map(|v| (v.a.0, v.b.0)).collect();
        assert_eq!(pairs.len(), 2, "{pairs:?}");
        assert!(pairs.contains(&(1, 2)) && pairs.contains(&(2, 3)));
        // Skipped pairs are left out; `against` limits to cross pairs.
        let skipped = find(&bodies, None, &|a, b| (a.0, b.0) == (1, 2), &o);
        assert_eq!(skipped.len(), 1);
        let cross = find(&bodies[..1], Some(&bodies[1..]), &|_, _| false, &o);
        assert_eq!(cross.len(), 1);
        assert_eq!((cross[0].a.0, cross[0].b.0), (1, 2));
    }
}
