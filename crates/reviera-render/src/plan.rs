// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plan graphics for the GPU viewport (spec Ch 5 §5.3 and §5.12, interim): what a plan shows at
//! its cut plane and below it.
//!
//! - **Cut poché**: every mesh is sliced at the cut plane; the section loops are filled (holes by
//!   nesting) and outlined with cut lines.
//! - **Projection**: model faces between the cut plane and the bottom of the range, drawn light,
//!   with their feature edges (outlines and creases, not the diagonals of flat faces) as lines.
//!
//! The exact hidden-line drawing of §5.12 (OCCT sections and HLR) replaces this for printing and
//! for the full plan conventions; this is what the interactive viewport draws today.

use std::collections::HashMap;

use crate::{RenderScene, SceneMesh};

/// A set of line segments of one colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    pub color: [f32; 4],
    /// Width in paper millimetres (drawn at least one pixel wide).
    pub width: f32,
    pub segments: Vec<[[f32; 3]; 2]>,
}

/// An edge of the model and the element it belongs to.
pub type Edge = (Option<reviera_core::ElementId>, [[f32; 3]; 2]);

/// Line widths of an element in paper millimetres: cut lines (`None` if its category is not cut)
/// and projection lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub cut: Option<f32>,
    pub projection: f32,
}

/// A cut edge in plan and the element it belongs to.
pub type CutEdge = (Option<reviera_core::ElementId>, [[f32; 2]; 2]);

/// What a plan view draws over and instead of the shaded model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanGraphics {
    /// Geometry above this elevation is not drawn.
    pub top: f32,
    /// Geometry below this elevation is not drawn.
    pub bottom: f32,
    /// Filled section regions at the cut plane.
    pub poche: Vec<SceneMesh>,
    pub lines: Vec<Lines>,
    /// The drawing scale denominator (paper widths become model widths with it).
    pub scale: f32,
    /// The cut edges with their element, for snapping and annotation references.
    pub cut_edges: Vec<CutEdge>,
}

pub const POCHE: [f32; 4] = [0.035, 0.038, 0.045, 1.0];
pub const CUT_LINE: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
pub const PROJECTION_LINE: [f32; 4] = [0.025, 0.027, 0.032, 1.0];
/// Edges sharper than this are drawn.
const CREASE_DEGREES: f32 = 20.0;
/// Points closer than this (metres) are the same point when joining sections.
const WELD: f32 = 1e-4;

fn key(p: [f32; 3]) -> (i64, i64, i64) {
    let q = |v: f32| (v / WELD).round() as i64;
    (q(p[0]), q(p[1]), q(p[2]))
}

/// The feature edges of a scene: boundary edges and creases between faces.
pub fn feature_edges(scene: &RenderScene) -> Vec<Edge> {
    let mut out = Vec::new();
    let cos = CREASE_DEGREES.to_radians().cos();
    for m in scene
        .meshes
        .iter()
        .filter(|m| !m.class.ends_with(crate::PLAN_HIDDEN))
    {
        // Welded vertex ids, so edges of separately listed triangles meet.
        let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut points: Vec<[f32; 3]> = Vec::new();
        let welded: Vec<u32> = m
            .positions
            .iter()
            .map(|p| {
                *ids.entry(key(*p)).or_insert_with(|| {
                    points.push(*p);
                    points.len() as u32 - 1
                })
            })
            .collect();
        let mut edges: HashMap<(u32, u32), Vec<glam::Vec3>> = HashMap::new();
        for tri in m.indices.as_chunks::<3>().0 {
            let v = tri.map(|i| welded[i as usize]);
            if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
                continue;
            }
            let p = v.map(|i| glam::Vec3::from(points[i as usize]));
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                edges.entry((a.min(b), a.max(b))).or_default().push(n);
            }
        }
        for ((a, b), normals) in edges {
            let feature = match normals.as_slice() {
                [_] => true,
                [n1, n2] => n1.dot(*n2).abs() < cos,
                _ => true,
            };
            if feature {
                out.push((m.element, [points[a as usize], points[b as usize]]));
            }
        }
    }
    out
}

/// The parts of segments between `bottom` and `top` (elevations).
pub fn clip_segments(segments: &[[[f32; 3]; 2]], bottom: f32, top: f32) -> Vec<[[f32; 3]; 2]> {
    let mut out = Vec::new();
    for &[a, b] in segments {
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let dz = b[2] - a[2];
        for (limit, below) in [(bottom, true), (top, false)] {
            if dz.abs() < 1e-9 {
                let outside = if below { a[2] < limit } else { a[2] > limit };
                if outside {
                    t0 = 1.0;
                    t1 = 0.0;
                }
                continue;
            }
            let t = (limit - a[2]) / dz;
            // Entering or leaving the slab between the planes.
            let entering = if below { dz > 0.0 } else { dz < 0.0 };
            if entering {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        if t0 < t1 {
            let at = |t: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
            out.push([at(t0), at(t1)]);
        }
    }
    out
}

/// The section of one mesh at elevation `z`: segments in plan.
fn slice(m: &SceneMesh, z: f32) -> Vec<[[f32; 2]; 2]> {
    let mut out = Vec::new();
    for tri in m.indices.as_chunks::<3>().0 {
        let p = tri.map(|i| m.positions[i as usize]);
        let d = p.map(|v| v[2] - z);
        let mut points = Vec::with_capacity(2);
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            if (d[i] < 0.0) != (d[j] < 0.0) {
                let t = d[i] / (d[i] - d[j]);
                points.push([
                    p[i][0] + (p[j][0] - p[i][0]) * t,
                    p[i][1] + (p[j][1] - p[i][1]) * t,
                ]);
            }
        }
        if let [a, b] = points[..]
            && (a[0] - b[0]).hypot(a[1] - b[1]) > WELD
        {
            out.push([a, b]);
        }
    }
    out
}

/// Joins segments into chains; closed chains are loops (the last point is not repeated).
/// A chain of points in plan.
type Chain = Vec<[f32; 2]>;

fn chains(segments: &[[[f32; 2]; 2]]) -> (Vec<Chain>, Vec<Chain>) {
    let k = |p: [f32; 2]| ((p[0] / WELD).round() as i64, (p[1] / WELD).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(k(s[0])).or_default().push(i);
        at.entry(k(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let (mut loops, mut open) = (Vec::new(), Vec::new());
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain = vec![segments[start][0], segments[start][1]];
        let first = k(chain[0]);
        loop {
            let end = *chain.last().expect("non-empty");
            if k(end) == first && chain.len() > 3 {
                chain.pop();
                loops.push(chain);
                break;
            }
            let next = at
                .get(&k(end))
                .and_then(|c| c.iter().copied().find(|&i| !used[i]));
            let Some(i) = next else {
                open.push(chain);
                break;
            };
            used[i] = true;
            let s = segments[i];
            chain.push(if k(s[0]) == k(end) { s[1] } else { s[0] });
        }
    }
    (loops, open)
}

fn area(poly: &[[f32; 2]]) -> f32 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        / 2.0
}

fn inside(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// Filled regions of closed loops at elevation `z` (holes found by nesting).
fn fill(loops: &[Vec<[f32; 2]>], z: f32, element: Option<reviera_core::ElementId>) -> SceneMesh {
    let loops: Vec<&Vec<[f32; 2]>> = loops.iter().filter(|l| area(l).abs() > 1e-8).collect();
    let depth: Vec<usize> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|&j| j != i && inside(loops[i][0], loops[j]))
                .count()
        })
        .collect();
    let mut mesh = SceneMesh {
        element,
        class: "poche".into(),
        positions: Vec::new(),
        indices: Vec::new(),
        color: POCHE,
    };
    let mut earcut = earcut::Earcut::new();
    for (i, outer) in loops.iter().enumerate() {
        if depth[i] % 2 == 1 {
            continue;
        }
        // Holes: loops one level deeper whose innermost container is this one.
        let holes: Vec<&Vec<[f32; 2]>> = loops
            .iter()
            .enumerate()
            .filter(|(j, h)| depth[*j] == depth[i] + 1 && inside(h[0], outer))
            .map(|(_, h)| *h)
            .collect();
        let mut points: Vec<[f32; 2]> = outer.to_vec();
        let mut starts = Vec::new();
        for h in &holes {
            starts.push(points.len() as u32);
            points.extend(h.iter().copied());
        }
        let mut tris: Vec<u32> = Vec::new();
        earcut.earcut(
            points.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]),
            &starts,
            &mut tris,
        );
        let base = mesh.positions.len() as u32;
        mesh.positions
            .extend(points.iter().map(|p| [p[0], p[1], z]));
        mesh.indices.extend(tris.into_iter().map(|t| t + base));
    }
    mesh
}

/// The plan graphics of `scene` cut at `cut`, showing geometry down to `bottom` (or up to it,
/// when it is above the cut: a reflected ceiling plan). `edges` are the scene's
/// [`feature_edges`] (computed once per scene).
pub fn plan_graphics(
    scene: &RenderScene,
    edges: &[Edge],
    cut: f32,
    bottom: f32,
    scale: f32,
    weights: &dyn Fn(Option<reviera_core::ElementId>) -> Weights,
) -> PlanGraphics {
    let mut poche = Vec::new();
    let mut cut_edges = Vec::new();
    // Lines grouped by colour and width.
    let mut groups: Vec<Lines> = Vec::new();
    let mut add = |color: [f32; 4], width: f32, segment: [[f32; 3]; 2]| match groups
        .iter_mut()
        .find(|l| l.color == color && l.width == width)
    {
        Some(l) => l.segments.push(segment),
        None => groups.push(Lines {
            color,
            width,
            segments: vec![segment],
        }),
    };
    for m in scene
        .meshes
        .iter()
        .filter(|m| !m.class.ends_with(crate::PLAN_HIDDEN))
    {
        let segments = slice(m, cut);
        if segments.is_empty() {
            continue;
        }
        // Elements whose category is not cut are drawn in projection only.
        let Some(cut_width) = weights(m.element).cut else {
            continue;
        };
        for [a, b] in &segments {
            add(CUT_LINE, cut_width, [[a[0], a[1], cut], [b[0], b[1], cut]]);
        }
        cut_edges.extend(segments.iter().map(|s| (m.element, *s)));
        let (loops, _) = chains(&segments);
        let filled = fill(&loops, cut, m.element);
        if !filled.indices.is_empty() {
            poche.push(filled);
        }
    }
    // Projection between `bottom` and the cut, or, when `bottom` is above the cut (a ceiling
    // plan, looking up), between the cut and it.
    let (lo, hi) = if bottom <= cut {
        (bottom, cut)
    } else {
        (cut, bottom)
    };
    for (element, segment) in edges {
        let width = weights(*element).projection;
        for s in clip_segments(&[*segment], lo, hi) {
            add(PROJECTION_LINE, width, s);
        }
    }
    PlanGraphics {
        top: hi,
        bottom: lo,
        poche,
        lines: groups,
        scale,
        cut_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEIGHTS: fn(Option<reviera_core::ElementId>) -> Weights = |_| Weights {
        cut: Some(0.5),
        projection: 0.18,
    };

    /// A closed box as a mesh (8 corners, 12 triangles).
    fn cube(lo: [f32; 3], hi: [f32; 3]) -> SceneMesh {
        let c = |x: usize, y: usize, z: usize| {
            [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]]
        };
        let positions = vec![
            c(0, 0, 0),
            c(1, 0, 0),
            c(1, 1, 0),
            c(0, 1, 0),
            c(0, 0, 1),
            c(1, 0, 1),
            c(1, 1, 1),
            c(0, 1, 1),
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7,
            6, 3, 0, 4, 3, 4, 7,
        ];
        SceneMesh {
            element: None,
            class: "IfcWall".into(),
            positions,
            indices,
            color: [1.0; 4],
        }
    }

    fn filled_area(m: &SceneMesh) -> f32 {
        m.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let p = t.map(|i| m.positions[i as usize]);
                ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                    - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]))
                    .abs()
                    / 2.0
            })
            .sum()
    }

    #[test]
    fn a_wall_cut_at_the_cut_plane_is_filled() {
        let scene = RenderScene {
            meshes: vec![cube([0.0, 0.0, 0.0], [5.0, 0.2, 3.0])],
        };
        let edges = feature_edges(&scene);
        assert_eq!(
            edges.len(),
            12,
            "a box has 12 edges; face diagonals are not drawn"
        );
        let g = plan_graphics(&scene, &edges, 1.2, 0.0, 100.0, &WEIGHTS);
        assert_eq!(g.poche.len(), 1);
        assert!((filled_area(&g.poche[0]) - 1.0).abs() < 1e-4);
        assert!(g.poche[0].positions.iter().all(|p| p[2] == 1.2));
        // Projection edges stop at the cut: the four verticals are clipped, the top is gone.
        let projection = &g
            .lines
            .iter()
            .find(|l| l.color == PROJECTION_LINE)
            .unwrap()
            .segments;
        let cut_width = g.lines.iter().find(|l| l.color == CUT_LINE).unwrap().width;
        assert_eq!(cut_width, 0.5);
        assert!(
            projection
                .iter()
                .all(|s| s[0][2] <= 1.2 + 1e-6 && s[1][2] <= 1.2 + 1e-6)
        );
        assert_eq!(projection.len(), 8);
        // A low wall is not cut.
        let low = RenderScene {
            meshes: vec![cube([0.0, 0.0, 0.0], [5.0, 0.2, 0.9])],
        };
        assert!(
            plan_graphics(&low, &feature_edges(&low), 1.2, 0.0, 100.0, &WEIGHTS)
                .poche
                .is_empty()
        );
    }

    #[test]
    fn touching_boxes_in_one_mesh_fill_their_own_areas() {
        // Four walls around a 2 × 2 opening, as one mesh.
        let mut ring = SceneMesh {
            element: None,
            class: "IfcWall".into(),
            positions: vec![],
            indices: vec![],
            color: [1.0; 4],
        };
        for (lo, hi) in [
            ([0.0, 0.0, 0.0], [4.0, 1.0, 3.0]),
            ([0.0, 3.0, 0.0], [4.0, 4.0, 3.0]),
            ([0.0, 1.0, 0.0], [1.0, 3.0, 3.0]),
            ([3.0, 1.0, 0.0], [4.0, 3.0, 3.0]),
        ] {
            let c = cube(lo, hi);
            let base = ring.positions.len() as u32;
            ring.positions.extend(c.positions);
            ring.indices.extend(c.indices.into_iter().map(|i| i + base));
        }
        let scene = RenderScene { meshes: vec![ring] };
        let g = plan_graphics(&scene, &[], 1.0, 0.0, 100.0, &WEIGHTS);
        // Four separate boxes give four filled rectangles: 4 + 4 + 2 + 2 = 12 m².
        let total: f32 = g.poche.iter().map(filled_area).sum();
        assert!((total - 12.0).abs() < 1e-3, "{total}");
    }

    #[test]
    fn a_square_with_a_hole_is_filled_around_it() {
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let inner = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let m = fill(&[outer, inner], 0.0, None);
        assert!((filled_area(&m) - 12.0).abs() < 1e-4);
    }
}
