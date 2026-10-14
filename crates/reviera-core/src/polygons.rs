// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Boolean operations on plan polygons (union, difference, intersection), through the iOverlay
//! engine. A shape is an outer ring and its holes; rings need not be closed or oriented.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;

pub type Ring = Vec<[f64; 2]>;

/// An outer ring (counter-clockwise) and its holes (clockwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub outer: Ring,
    pub holes: Vec<Ring>,
}

impl Shape {
    pub fn area(&self) -> f64 {
        area(&self.outer).abs() - self.holes.iter().map(|h| area(h).abs()).sum::<f64>()
    }

    fn rings(&self) -> Vec<Ring> {
        std::iter::once(self.outer.clone())
            .chain(self.holes.iter().cloned())
            .collect()
    }
}

/// The signed area of a ring (positive counter-clockwise).
pub fn area(r: &[[f64; 2]]) -> f64 {
    (0..r.len())
        .map(|i| {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn shapes(result: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Shape> {
    result
        .into_iter()
        .filter_map(|mut s| {
            if s.is_empty() {
                return None;
            }
            let outer = s.remove(0);
            Some(Shape { outer, holes: s })
        })
        .filter(|s| s.outer.len() >= 3 && s.area() > 1e-9)
        .collect()
}

fn all_rings(s: &[Shape]) -> Vec<Ring> {
    s.iter().flat_map(Shape::rings).collect()
}

/// The union of rings (each a filled region).
pub fn union(rings: &[Ring]) -> Vec<Shape> {
    let rings: Vec<Ring> = rings.iter().filter(|r| r.len() >= 3).cloned().collect();
    if rings.is_empty() {
        return Vec::new();
    }
    // Each ring counter-clockwise, so overlapping ones add up under non-zero.
    let rings: Vec<Ring> = rings
        .into_iter()
        .map(|mut r| {
            if area(&r) < 0.0 {
                r.reverse();
            }
            r
        })
        .collect();
    shapes(rings.simplify_shape(FillRule::NonZero))
}

/// `subject` less the regions of `clip` rings.
pub fn difference(subject: &[Shape], clip: &[Ring]) -> Vec<Shape> {
    let clip: Vec<Ring> = clip.iter().filter(|r| r.len() >= 3).cloned().collect();
    if clip.is_empty() {
        return subject.to_vec();
    }
    shapes(all_rings(subject).overlay(&clip, OverlayRule::Difference, FillRule::EvenOdd))
}

/// Where `a` and `b` overlap.
pub fn intersection(a: &[Shape], b: &[Shape]) -> Vec<Shape> {
    shapes(all_rings(a).overlay(&all_rings(b), OverlayRule::Intersect, FillRule::EvenOdd))
}

/// A ring without points on a straight run between their neighbours, or repeated.
pub fn simplify(r: &[[f64; 2]]) -> Ring {
    let mut out: Ring = Vec::new();
    for p in r {
        if !out
            .last()
            .is_some_and(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-9)
        {
            out.push(*p);
        }
    }
    loop {
        let n = out.len();
        if n < 4 {
            return out;
        }
        let k = (0..n).find(|&i| {
            let (a, b, c) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let len = (c[0] - a[0]).hypot(c[1] - a[1]).max(1e-12);
            (cross / len).abs() < 1e-7
        });
        match k {
            Some(i) => {
                out.remove(i);
            }
            None => return out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Ring {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn unions_differences_and_intersections() {
        let u = union(&[rect(0.0, 0.0, 2.0, 1.0), rect(1.0, 0.0, 3.0, 1.0)]);
        assert_eq!(u.len(), 1);
        assert!((u[0].area() - 3.0).abs() < 1e-6);
        let floor = vec![Shape {
            outer: rect(0.0, 0.0, 10.0, 10.0),
            holes: Vec::new(),
        }];
        // A hole inside, and a notch at the edge.
        let d = difference(
            &floor,
            &[rect(4.0, 4.0, 5.0, 5.0), rect(-1.0, 2.0, 1.0, 3.0)],
        );
        let total: f64 = d.iter().map(Shape::area).sum();
        assert!((total - (100.0 - 1.0 - 1.0)).abs() < 1e-6, "{total}");
        let i = intersection(
            &floor,
            &[Shape {
                outer: rect(9.0, 9.0, 11.0, 11.0),
                holes: Vec::new(),
            }],
        );
        assert!((i.iter().map(Shape::area).sum::<f64>() - 1.0).abs() < 1e-6);
        assert_eq!(
            simplify(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]).len(),
            4
        );
    }
}
