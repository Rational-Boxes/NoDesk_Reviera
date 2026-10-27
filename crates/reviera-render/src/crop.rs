// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Cropping a drawing to a rectangle in its plane (DOC-GFX-015): line segments clipped
//! (Liang–Barsky), filled regions clipped triangle by triangle (Sutherland–Hodgman), labels kept
//! only when they lie wholly inside. The window and output crop alike.

use crate::annotation::Label;
use crate::font;
use crate::plan::{Lines, PlanGraphics};

/// A rectangle in the drawing's plane (x, y), metres.
pub type Rect = ([f64; 2], [f64; 2]);

/// The part of segment `a`–`b` inside `r` (z interpolated), if any.
pub fn clip_segment(s: [[f32; 3]; 2], r: Rect) -> Option<[[f32; 3]; 2]> {
    let (a, b) = (s[0], s[1]);
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let (lo, hi) = ([r.0[0] as f32, r.0[1] as f32], [r.1[0] as f32, r.1[1] as f32]);
    for (p, q) in [(-d[0], a[0] - lo[0]), (d[0], hi[0] - a[0]), (-d[1], a[1] - lo[1]), (d[1], hi[1] - a[1])] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f32| [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
    Some([at(t0), at(t1)])
}

pub fn crop_lines(lines: &mut [Lines], r: Rect) {
    for l in lines {
        l.segments = l.segments.iter().filter_map(|s| clip_segment(*s, r)).collect();
    }
}

/// A convex polygon clipped to `r` (Sutherland–Hodgman), z interpolated.
fn clip_polygon(poly: Vec<[f32; 3]>, r: Rect) -> Vec<[f32; 3]> {
    let (lo, hi) = ([r.0[0] as f32, r.0[1] as f32], [r.1[0] as f32, r.1[1] as f32]);
    // Each edge of the rectangle: inside test and the crossing parameter.
    let edges: [(usize, f32, bool); 4] = [(0, lo[0], true), (0, hi[0], false), (1, lo[1], true), (1, hi[1], false)];
    let mut out = poly;
    for (axis, v, keep_above) in edges {
        if out.is_empty() {
            break;
        }
        let inside = |p: &[f32; 3]| if keep_above { p[axis] >= v } else { p[axis] <= v };
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let (cur, prev) = (input[i], input[(i + input.len() - 1) % input.len()]);
            let cross = |a: [f32; 3], b: [f32; 3]| {
                let t = (v - a[axis]) / (b[axis] - a[axis]);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
            };
            match (inside(&prev), inside(&cur)) {
                (true, true) => out.push(cur),
                (true, false) => out.push(cross(prev, cur)),
                (false, true) => {
                    out.push(cross(prev, cur));
                    out.push(cur);
                }
                (false, false) => {}
            }
        }
    }
    out
}

/// Filled regions (poche) clipped to `r`.
pub fn crop_meshes(meshes: &mut Vec<crate::SceneMesh>, r: Rect) {
    for m in meshes.iter_mut() {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for t in m.indices.as_chunks::<3>().0 {
            let poly = clip_polygon(t.iter().map(|k| m.positions[*k as usize]).collect(), r);
            if poly.len() < 3 {
                continue;
            }
            let base = positions.len() as u32;
            positions.extend(poly.iter().copied());
            for k in 1..poly.len() as u32 - 1 {
                indices.extend([base, base + k, base + k + 1]);
            }
        }
        m.positions = positions;
        m.indices = indices;
    }
    meshes.retain(|m| !m.indices.is_empty());
}

/// A plan or section drawing's model graphics cropped to `r`.
pub fn crop_graphics(g: &mut PlanGraphics, r: Rect) {
    crop_lines(&mut g.lines, r);
    crop_meshes(&mut g.poche, r);
    g.cut_edges.retain_mut(|(_, s)| {
        let seg = [[s[0][0], s[0][1], 0.0], [s[1][0], s[1][1], 0.0]];
        match clip_segment(seg, r) {
            Some(c) => {
                *s = [[c[0][0], c[0][1]], [c[1][0], c[1][1]]];
                true
            }
            None => false,
        }
    });
}

/// Whether a label lies wholly inside `r`.
pub fn label_inside(l: &Label, r: Rect) -> bool {
    let w = font::width(&l.text, l.height);
    let corners = [
        [l.origin[0], l.origin[1]],
        [l.origin[0] + l.right[0] * w, l.origin[1] + l.right[1] * w],
        [l.origin[0] + l.up[0] * l.height, l.origin[1] + l.up[1] * l.height],
        [l.origin[0] + l.right[0] * w + l.up[0] * l.height, l.origin[1] + l.right[1] * w + l.up[1] * l.height],
    ];
    corners.iter().all(|c| {
        f64::from(c[0]) >= r.0[0] - 1e-6 && f64::from(c[0]) <= r.1[0] + 1e-6 && f64::from(c[1]) >= r.0[1] - 1e-6 && f64::from(c[1]) <= r.1[1] + 1e-6
    })
}

/// The four sides of `r` at height `z`, for drawing the crop region.
pub fn outline(r: Rect, z: f32) -> Vec<[[f32; 3]; 2]> {
    let (x0, y0, x1, y1) = (r.0[0] as f32, r.0[1] as f32, r.1[0] as f32, r.1[1] as f32);
    vec![[[x0, y0, z], [x1, y0, z]], [[x1, y0, z], [x1, y1, z]], [[x1, y1, z], [x0, y1, z]], [[x0, y1, z], [x0, y0, z]]]
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = ([0.0, 0.0], [10.0, 5.0]);

    #[test]
    fn segments_are_cut_at_the_rectangle() {
        assert_eq!(clip_segment([[-5.0, 2.0, 1.0], [15.0, 2.0, 1.0]], R), Some([[0.0, 2.0, 1.0], [10.0, 2.0, 1.0]]));
        assert_eq!(clip_segment([[1.0, 1.0, 0.0], [2.0, 2.0, 0.0]], R), Some([[1.0, 1.0, 0.0], [2.0, 2.0, 0.0]]));
        assert_eq!(clip_segment([[-5.0, 7.0, 0.0], [15.0, 7.0, 0.0]], R), None);
        let c = clip_segment([[5.0, 2.5, 0.0], [15.0, 7.5, 0.0]], R).unwrap();
        assert!((c[1][0] - 10.0).abs() < 1e-5 && (c[1][1] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn filled_regions_keep_their_part_inside() {
        // A 20 × 20 square over the rectangle keeps the rectangle's area.
        let mut meshes = vec![crate::SceneMesh {
            element: None,
            class: "poche".into(),
            positions: vec![[-5.0, -5.0, 0.0], [15.0, -5.0, 0.0], [15.0, 15.0, 0.0], [-5.0, 15.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            color: [0.0; 4],
        }];
        crop_meshes(&mut meshes, R);
        let area: f32 = meshes[0]
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|k| meshes[0].positions[k as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum();
        assert!((area - 50.0).abs() < 1e-3, "{area}");
        // One wholly outside goes.
        let mut outside = vec![crate::SceneMesh { positions: vec![[20.0, 0.0, 0.0], [21.0, 0.0, 0.0], [21.0, 1.0, 0.0]], indices: vec![0, 1, 2], ..meshes[0].clone() }];
        crop_meshes(&mut outside, R);
        assert!(outside.is_empty());
    }
}
