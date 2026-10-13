// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plan arrangements of wall faces (the boundary arrangement of ARC-ROOM-002, used by automatic
//! ceilings now and by rooms later): the outlines of the walls cut at a height are split where
//! they meet, and the faces of the resulting planar graph are traced. The loop around a point is
//! the smallest face that contains it; each of its edges knows the wall face it lies on.
//!
//! Built so far: walls only, at their finish faces; islands inside a loop are not holes yet.

use std::collections::HashMap;

use crate::id::ElementId;
use crate::store::Store;
use crate::walls::{self, Line};

/// Points closer than this are one vertex.
const TOL: f64 = 1e-6;

/// An edge of the arrangement: a line, and the wall face it lies on (`face:ext` / `face:int`),
/// if any (wall ends have none).
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub line: Line,
    pub lock: Option<(ElementId, String)>,
}

/// The plan outlines of the Reviera walls whose height spans `z`: each wall's two faces (with
/// the face they are) and its two ends, as joined.
pub fn wall_edges(store: &Store, z: f64) -> Vec<Edge> {
    let solved = walls::solve(store);
    let mut out = Vec::new();
    for (id, body) in &solved.bodies {
        let g = &body.geometry;
        if z < g.base_z - 1e-9 || z > g.base_z + g.height + 1e-9 {
            continue;
        }
        let flip = store
            .get(*id)
            .and_then(walls::wall_recipe)
            .is_some_and(|r| r.flip);
        // Footprint: start-interior, end-interior, end-exterior, start-exterior. The "interior"
        // pair is on the −normal side, which is the exterior face when the wall is flipped.
        let f = body.footprint;
        let minus = if flip { "face:ext" } else { "face:int" };
        let plus = if flip { "face:int" } else { "face:ext" };
        let edge = |a: [f64; 2], b: [f64; 2], sub: Option<&str>| Edge {
            line: Line { start: a, end: b },
            lock: sub.map(|s| (*id, s.to_owned())),
        };
        out.push(edge(f[0], f[1], Some(minus)));
        out.push(edge(f[1], f[2], None));
        out.push(edge(f[2], f[3], Some(plus)));
        out.push(edge(f[3], f[0], None));
    }
    out
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Parameters along `a` (0..1) where it meets or touches `b`.
fn cuts(a: Line, b: Line) -> Vec<f64> {
    let (d, e) = (sub(a.end, a.start), sub(b.end, b.start));
    let den = cross(d, e);
    let len2 = d[0] * d[0] + d[1] * d[1];
    let mut out = Vec::new();
    if den.abs() > 1e-12 * len2.sqrt() * (e[0] * e[0] + e[1] * e[1]).sqrt() {
        let w = sub(b.start, a.start);
        let t = cross(w, e) / den;
        let u = cross(w, d) / den;
        let lb = (e[0] * e[0] + e[1] * e[1]).sqrt();
        if (-TOL / len2.sqrt()..=1.0 + TOL / len2.sqrt()).contains(&t)
            && (-TOL / lb..=1.0 + TOL / lb).contains(&u)
        {
            out.push(t);
        }
    } else {
        // Collinear overlap: b's ends on a.
        for p in [b.start, b.end] {
            let w = sub(p, a.start);
            if cross(d, w).abs() / len2.sqrt() < TOL {
                out.push((w[0] * d[0] + w[1] * d[1]) / len2);
            }
        }
    }
    out
}

/// The faces of the arrangement of `edges` as loops of edges (counter-clockwise, interior on
/// the left), with their signed areas.
fn faces(edges: &[Edge]) -> Vec<(f64, Vec<Edge>)> {
    // Split every edge where others meet or touch it.
    let mut pieces: Vec<Edge> = Vec::new();
    for (i, a) in edges.iter().enumerate() {
        let mut ts = vec![0.0, 1.0];
        for (j, b) in edges.iter().enumerate() {
            if i != j {
                ts.extend(
                    cuts(a.line, b.line)
                        .into_iter()
                        .filter(|t| (0.0..=1.0).contains(t)),
                );
            }
        }
        ts.sort_by(f64::total_cmp);
        let at = |t: f64| {
            [
                a.line.start[0] + (a.line.end[0] - a.line.start[0]) * t,
                a.line.start[1] + (a.line.end[1] - a.line.start[1]) * t,
            ]
        };
        for w in ts.windows(2) {
            let (p, q) = (at(w[0]), at(w[1]));
            if (q[0] - p[0]).hypot(q[1] - p[1]) > TOL {
                pieces.push(Edge {
                    line: Line { start: p, end: q },
                    lock: a.lock.clone(),
                });
            }
        }
    }
    // Vertices.
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut vertex = |p: [f64; 2]| -> usize {
        match points
            .iter()
            .position(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < TOL * 10.0)
        {
            Some(i) => i,
            None => {
                points.push(p);
                points.len() - 1
            }
        }
    };
    let mut half: Vec<(usize, usize, usize)> = Vec::new(); // (from, to, piece)
    let mut seen: HashMap<(usize, usize), ()> = HashMap::new();
    for (k, p) in pieces.iter().enumerate() {
        let (a, b) = (vertex(p.line.start), vertex(p.line.end));
        if a == b || seen.contains_key(&(a.min(b), a.max(b))) {
            continue;
        }
        seen.insert((a.min(b), a.max(b)), ());
        half.push((a, b, k));
        half.push((b, a, k));
    }
    // Outgoing half-edges of each vertex, by angle.
    let angle = |h: &(usize, usize, usize)| {
        let d = sub(points[h.1], points[h.0]);
        d[1].atan2(d[0])
    };
    let mut out_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, h) in half.iter().enumerate() {
        out_of.entry(h.0).or_default().push(i);
    }
    for list in out_of.values_mut() {
        list.sort_by(|a, b| angle(&half[*a]).total_cmp(&angle(&half[*b])));
    }
    let twin = |i: usize| if i.is_multiple_of(2) { i + 1 } else { i - 1 };
    // Next half-edge: at the end vertex, the one just clockwise of the way back, so faces run
    // counter-clockwise with their interior on the left.
    let next = |i: usize| {
        let back = twin(i);
        let list = &out_of[&half[i].1];
        let k = list.iter().position(|h| *h == back).expect("listed");
        list[(k + list.len() - 1) % list.len()]
    };
    let mut used = vec![false; half.len()];
    let mut out = Vec::new();
    for start in 0..half.len() {
        if used[start] {
            continue;
        }
        let mut lp = Vec::new();
        let mut h = start;
        while !used[h] {
            used[h] = true;
            lp.push(h);
            h = next(h);
        }
        let ring: Vec<[f64; 2]> = lp.iter().map(|h| points[half[*h].0]).collect();
        let area = crate::sketch::signed_area(&ring);
        let edges = lp
            .iter()
            .map(|h| Edge {
                line: Line {
                    start: points[half[*h].0],
                    end: points[half[*h].1],
                },
                lock: pieces[half[*h].2].lock.clone(),
            })
            .collect();
        out.push((area, edges));
    }
    out
}

fn inside(p: [f64; 2], ring: &[Edge]) -> bool {
    let mut c = false;
    for e in ring {
        let (a, b) = (e.line.start, e.line.end);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The loop of the smallest face of the arrangement around `p` (counter-clockwise), with
/// consecutive collinear pieces of one wall face merged.
pub fn loop_around(edges: &[Edge], p: [f64; 2]) -> Option<Vec<Edge>> {
    let (_, lp) = faces(edges)
        .into_iter()
        .filter(|(a, lp)| *a > 0.0 && inside(p, lp))
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let mut merged: Vec<Edge> = Vec::new();
    for e in lp {
        if let Some(last) = merged.last_mut() {
            let (d0, d1) = (
                sub(last.line.end, last.line.start),
                sub(e.line.end, e.line.start),
            );
            let collinear =
                cross(d0, d1).abs() < 1e-9 * (d0[0].hypot(d0[1]) * d1[0].hypot(d1[1])).max(1e-12);
            if collinear && last.lock == e.lock {
                last.line.end = e.line.end;
                continue;
            }
        }
        merged.push(e);
    }
    // The first and last pieces may be one.
    if merged.len() > 2 {
        let (first, last) = (merged[0].clone(), merged[merged.len() - 1].clone());
        let (d0, d1) = (
            sub(last.line.end, last.line.start),
            sub(first.line.end, first.line.start),
        );
        if cross(d0, d1).abs() < 1e-9 * (d0[0].hypot(d0[1]) * d1[0].hypot(d1[1])).max(1e-12)
            && first.lock == last.lock
        {
            merged[0].line.start = last.line.start;
            merged.pop();
        }
    }
    Some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(a: [f64; 2], b: [f64; 2]) -> Edge {
        Edge {
            line: Line { start: a, end: b },
            lock: None,
        }
    }

    #[test]
    fn the_loop_around_a_point_is_the_smallest_face() {
        // Two rooms side by side: a 10 × 7 rectangle split at x = 5.
        let edges = vec![
            e([0.0, 0.0], [10.0, 0.0]),
            e([10.0, 0.0], [10.0, 7.0]),
            e([10.0, 7.0], [0.0, 7.0]),
            e([0.0, 7.0], [0.0, 0.0]),
            e([5.0, 0.0], [5.0, 7.0]),
        ];
        let lp = loop_around(&edges, [2.0, 3.0]).unwrap();
        let ring: Vec<[f64; 2]> = lp.iter().map(|e| e.line.start).collect();
        assert!((crate::sketch::signed_area(&ring) - 35.0).abs() < 1e-9);
        assert_eq!(lp.len(), 4);
        assert!(loop_around(&edges, [20.0, 3.0]).is_none());
    }
}
