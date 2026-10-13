// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Editing sketch curves in place (ARC-SKCH-001): moving one curve parallel to itself with its
//! neighbours stretched or shortened to meet it, moving a corner shared by curves, and changing
//! a curve's length. Shared by the sketch session and the footprint grips of roofs, floors and
//! ceilings.

use reviera_core::walls::Line;

use crate::sketch::Curve;

/// Points closer than this (metres) are the same corner.
const SAME: f64 = 1e-6;

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < SAME
}

fn unit(l: Line) -> [f64; 2] {
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let len = d[0].hypot(d[1]).max(1e-12);
    [d[0] / len, d[1] / len]
}

/// Where the infinite lines through `a` and `b` cross; `None` when they are parallel.
fn crossing(a: Line, b: Line) -> Option<[f64; 2]> {
    let (u, v) = (unit(a), unit(b));
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-9 {
        return None;
    }
    let w = [b.start[0] - a.start[0], b.start[1] - a.start[1]];
    let t = (w[0] * v[1] - w[1] * v[0]) / den;
    Some([a.start[0] + u[0] * t, a.start[1] + u[1] * t])
}

/// Moves curve `i` by the part of `v` across it (it stays on its own direction). Each curve
/// meeting it at a corner keeps its direction and is stretched or shortened to meet it again.
/// The moved curve leaves the wall it was locked to; the neighbours keep theirs, since they
/// stay on their lines.
pub fn move_curve(curves: &mut [Curve], i: usize, v: [f64; 2]) {
    let old = curves[i].line;
    let u = unit(old);
    let n = [-u[1], u[0]];
    let d = v[0] * n[0] + v[1] * n[1];
    if d.abs() < 1e-12 {
        return;
    }
    let shift = |p: [f64; 2]| [p[0] + n[0] * d, p[1] + n[1] * d];
    let line = Line {
        start: shift(old.start),
        end: shift(old.end),
    };
    let mut new = line;
    for (j, c) in curves.iter_mut().enumerate() {
        if j == i {
            continue;
        }
        for (k, corner) in [old.start, old.end].into_iter().enumerate() {
            let moved = if k == 0 { line.start } else { line.end };
            let at = crossing(c.line, line).unwrap_or(moved);
            let mut met = false;
            if close(c.line.start, corner) {
                c.line.start = at;
                met = true;
            }
            if close(c.line.end, corner) {
                c.line.end = at;
                met = true;
            }
            // The moved curve ends where its neighbour now meets it.
            if met {
                if k == 0 {
                    new.start = at;
                } else {
                    new.end = at;
                }
            }
        }
    }
    curves[i].line = new;
    curves[i].lock = None;
}

/// Moves the corner at `from` to `to`: every curve ending there follows, and those leave the
/// walls they were locked to.
pub fn move_corner(curves: &mut [Curve], from: [f64; 2], to: [f64; 2]) {
    for c in curves.iter_mut() {
        let mut moved = false;
        if close(c.line.start, from) {
            c.line.start = to;
            moved = true;
        }
        if close(c.line.end, from) {
            c.line.end = to;
            moved = true;
        }
        if moved {
            c.lock = None;
        }
    }
}

/// Changes curve `i`'s length, keeping its start; the corner at its end moves along it.
pub fn set_length(curves: &mut [Curve], i: usize, length: f64) {
    let l = curves[i].line;
    let u = unit(l);
    let to = [l.start[0] + u[0] * length, l.start[1] + u[1] * length];
    move_corner(curves, l.end, to);
}

/// The curve nearest to `p` within `reach`, and the corner of it within `reach` if any.
pub fn nearest(curves: &[Curve], p: [f64; 2], reach: f64) -> Option<(usize, Option<[f64; 2]>)> {
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    // Corners first: they are small targets.
    let corner = curves
        .iter()
        .enumerate()
        .flat_map(|(i, c)| [(i, c.line.start), (i, c.line.end)])
        .filter(|(_, q)| d(p, *q) <= reach)
        .min_by(|a, b| d(p, a.1).total_cmp(&d(p, b.1)));
    if let Some((i, q)) = corner {
        return Some((i, Some(q)));
    }
    curves
        .iter()
        .enumerate()
        .map(|(i, c)| (i, segment_distance(p, c.line)))
        .filter(|(_, dist)| *dist <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| (i, None))
}

/// The distance from `p` to the segment `l`.
pub fn segment_distance(p: [f64; 2], l: Line) -> f64 {
    let (a, b) = (l.start, l.end);
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-18);
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Vec<Curve> {
        let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
        (0..4)
            .map(|k| {
                Curve::new(
                    Line {
                        start: c[k],
                        end: c[(k + 1) % 4],
                    },
                    None,
                )
            })
            .collect()
    }

    #[test]
    fn moving_a_side_stretches_its_neighbours() {
        let mut r = rect();
        // The right side out by 2 m; the along-side part of the drag is ignored.
        move_curve(&mut r, 1, [2.0, 5.0]);
        assert_eq!(r[1].line.start, [12.0, 0.0]);
        assert_eq!(r[1].line.end, [12.0, 6.0]);
        assert_eq!(r[0].line.end, [12.0, 0.0]);
        assert_eq!(r[2].line.start, [12.0, 6.0]);
        // A slanted neighbour keeps its direction.
        let mut r = rect();
        r[0].line.end = [10.0, 0.0];
        r[1].line = Line {
            start: [10.0, 0.0],
            end: [8.0, 6.0],
        };
        r[2].line.start = [8.0, 6.0];
        move_curve(&mut r, 2, [0.0, 3.0]);
        let e = r[1].line.end;
        assert!(
            (e[1] - 9.0).abs() < 1e-9 && (e[0] - 7.0).abs() < 1e-9,
            "{e:?}"
        );
        assert_eq!(r[2].line.start, e);
    }

    #[test]
    fn corners_and_lengths_move_shared_ends() {
        let mut r = rect();
        move_corner(&mut r, [10.0, 6.0], [11.0, 7.0]);
        assert_eq!(r[1].line.end, [11.0, 7.0]);
        assert_eq!(r[2].line.start, [11.0, 7.0]);
        let mut r = rect();
        set_length(&mut r, 0, 12.0);
        assert_eq!(r[0].line.end, [12.0, 0.0]);
        assert_eq!(r[1].line.start, [12.0, 0.0]);
        assert_eq!(nearest(&r, [6.0, 0.1], 0.3), Some((0, None)));
        assert_eq!(
            nearest(&r, [0.1, 0.1], 0.3).map(|n| n.1),
            Some(Some([0.0, 0.0]))
        );
    }
}
