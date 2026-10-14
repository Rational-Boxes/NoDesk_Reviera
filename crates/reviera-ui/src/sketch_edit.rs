// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Editing sketch curves in place (ARC-SKCH-001) for the sketch session and the footprint
//! grips: the core's edits (`reviera_core::sketch_edit`, also the `sketch.move_*` commands) on
//! the session's curves, which drop their locks when they leave their line; and picking.

use reviera_core::walls::Line;

use crate::sketch::Curve;

/// Runs a core edit on the curves' lines; the curves that left their line drop their locks.
fn edit(curves: &mut [Curve], f: impl FnOnce(&mut [Line]) -> Vec<usize>) {
    let mut lines: Vec<Line> = curves.iter().map(|c| c.line).collect();
    let moved = f(&mut lines);
    for (c, l) in curves.iter_mut().zip(lines) {
        c.line = l;
    }
    for i in moved {
        curves[i].lock = None;
    }
}

/// Moves curve `i` across itself by `v`; its neighbours stretch or shorten to meet it.
pub fn move_curve(curves: &mut [Curve], i: usize, v: [f64; 2]) {
    // A curved curve's points move across its chord with it.
    let l = curves[i].line;
    let len = (l.end[0] - l.start[0]).hypot(l.end[1] - l.start[1]).max(1e-12);
    let n = [-(l.end[1] - l.start[1]) / len, (l.end[0] - l.start[0]) / len];
    let d = v[0] * n[0] + v[1] * n[1];
    for p in &mut curves[i].through {
        *p = [p[0] + n[0] * d, p[1] + n[1] * d];
    }
    edit(curves, |l| reviera_core::sketch_edit::move_curve(l, i, v));
}

/// Moves the corner at `from` to `to`, with every curve ending there.
pub fn move_corner(curves: &mut [Curve], from: [f64; 2], to: [f64; 2]) {
    edit(curves, |l| {
        reviera_core::sketch_edit::move_corner(l, from, to)
    });
}

/// Changes curve `i`'s length, keeping its start.
pub fn set_length(curves: &mut [Curve], i: usize, length: f64) {
    edit(curves, |l| {
        reviera_core::sketch_edit::set_length(l, i, length)
    });
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
        .map(|(i, c)| {
            let d = c
                .points()
                .windows(2)
                .map(|w| segment_distance(p, Line { start: w[0], end: w[1] }))
                .fold(f64::INFINITY, f64::min);
            (i, d)
        })
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
