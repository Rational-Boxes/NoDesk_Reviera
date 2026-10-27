// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! A bounding volume hierarchy over a scene's triangles, for ray casts many times a frame: whether
//! a floating label's point is hidden behind the model (DOC-ANN-025). Built once per model
//! change by splitting the triangles at the median of their centres along the box's longest axis.

use glam::Vec3;

use crate::{RenderScene, is_virtual};

/// Triangles per leaf.
const LEAF: usize = 8;

#[derive(Debug, Clone)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    /// A leaf holds triangles `start..start + count`; an inner node's children are the nodes
    /// `start` (left) and `count` (right).
    start: u32,
    count: u32,
    leaf: bool,
}

/// The hierarchy, and each triangle with the mesh it came from.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<([Vec3; 3], u32)>,
}

impl Bvh {
    /// Every triangle of the scene's meshes but virtual volumes.
    pub fn build(scene: &RenderScene) -> Bvh {
        Self::build_of(scene, &|m| !is_virtual(&m.class))
    }

    /// The triangles of what one cannot see through: opaque meshes (no glass, no translucent
    /// room or space volumes, no openings or virtual volumes).
    pub fn build_opaque(scene: &RenderScene) -> Bvh {
        Self::build_of(scene, &|m| {
            let c = m.class.as_str();
            !is_virtual(c) && m.color[3] >= 0.9 && !c.starts_with("IfcSpace") && !c.starts_with("IfcOpeningElement") && !c.starts_with("IfcVirtualElement")
        })
    }

    fn build_of(scene: &RenderScene, take: &dyn Fn(&crate::SceneMesh) -> bool) -> Bvh {
        let mut tris = Vec::new();
        for (i, m) in scene.meshes.iter().enumerate() {
            if !take(m) {
                continue;
            }
            for t in m.indices.as_chunks::<3>().0 {
                tris.push((t.map(|k| Vec3::from(m.positions[k as usize])), i as u32));
            }
        }
        let mut b = Bvh { nodes: Vec::new(), tris };
        if !b.tris.is_empty() {
            let n = b.tris.len();
            b.split(0, n);
        }
        b
    }

    fn bounds(&self, start: usize, end: usize) -> (Vec3, Vec3) {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (t, _) in &self.tris[start..end] {
            for p in t {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        (lo, hi)
    }

    /// Makes the node for `start..end` and its children; returns its index.
    fn split(&mut self, start: usize, end: usize) -> u32 {
        let (lo, hi) = self.bounds(start, end);
        let me = self.nodes.len() as u32;
        self.nodes.push(Node { lo, hi, start: start as u32, count: (end - start) as u32, leaf: true });
        if end - start <= LEAF {
            return me;
        }
        let ext = hi - lo;
        let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
        let mid = (start + end) / 2;
        let centre = |t: &([Vec3; 3], u32)| (t.0[0][axis] + t.0[1][axis] + t.0[2][axis]) / 3.0;
        self.tris[start..end].select_nth_unstable_by(mid - start, |a, b| centre(a).total_cmp(&centre(b)));
        let left = self.split(start, mid);
        let right = self.split(mid, end);
        let n = &mut self.nodes[me as usize];
        n.leaf = false;
        n.start = left;
        n.count = right;
        me
    }

    /// The nearest hit along the ray `origin + dir t` for `t` below `max_t` whose point `accept`
    /// takes (`dir` need not be unit length; `t` is in its units), with the mesh hit.
    pub fn nearest(&self, origin: Vec3, dir: Vec3, max_t: f32, accept: &dyn Fn(usize, Vec3) -> bool) -> Option<(usize, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = Vec3::ONE / dir;
        let mut best: Option<(usize, f32)> = None;
        let mut stack = vec![0u32];
        while let Some(k) = stack.pop() {
            let n = &self.nodes[k as usize];
            let limit = best.map_or(max_t, |b| b.1);
            if !slab(origin, inv, n.lo, n.hi, limit) {
                continue;
            }
            if n.leaf {
                for (t, mesh) in &self.tris[n.start as usize..(n.start + n.count) as usize] {
                    if let Some(d) = ray_triangle(origin, dir, t[0], t[1], t[2])
                        && d < best.map_or(max_t, |b| b.1)
                        && accept(*mesh as usize, origin + dir * d)
                    {
                        best = Some((*mesh as usize, d));
                    }
                }
            } else {
                stack.push(n.start);
                stack.push(n.count);
            }
        }
        best
    }
}

/// Whether the ray meets the box before `limit`.
fn slab(origin: Vec3, inv: Vec3, lo: Vec3, hi: Vec3, limit: f32) -> bool {
    let (t0, t1) = ((lo - origin) * inv, (hi - origin) * inv);
    let (near, far) = (t0.min(t1), t0.max(t1));
    let enter = near.max_element().max(0.0);
    let exit = far.min_element().min(limit);
    enter <= exit
}

fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-6).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneMesh;

    fn quad(z: f32, x0: f32, element: u64) -> SceneMesh {
        SceneMesh {
            element: Some(reviera_core::ElementId(element)),
            class: "IfcSlab".into(),
            positions: vec![[x0, 0.0, z], [x0 + 1.0, 0.0, z], [x0 + 1.0, 1.0, z], [x0, 1.0, z]],
            indices: vec![0, 1, 2, 0, 2, 3],
            color: [1.0; 4],
        }
    }

    #[test]
    fn rays_find_the_nearest_triangle_as_a_scan_does() {
        // A hundred squares in two layers.
        let mut meshes = Vec::new();
        for k in 0..50 {
            meshes.push(quad(0.0, k as f32 * 2.0, k));
            meshes.push(quad(3.0, k as f32 * 2.0, 100 + k));
        }
        let scene = RenderScene { meshes };
        let bvh = Bvh::build(&scene);
        for k in 0..50 {
            let origin = Vec3::new(k as f32 * 2.0 + 0.5, 0.5, 10.0);
            let dir = Vec3::new(0.0, 0.0, -1.0);
            let hit = bvh.nearest(origin, dir, f32::MAX, &|_, _| true);
            assert_eq!(hit.map(|h| h.0), scene.pick(origin, dir).map(|h| h.0));
            assert!((hit.unwrap().1 - 7.0).abs() < 1e-5, "the upper layer");
            // Passing through the upper layer reaches the lower one; nothing within 5.
            let through = bvh.nearest(origin, dir, f32::MAX, &|m, _| scene.meshes[m].element.unwrap().0 < 100);
            assert!((through.unwrap().1 - 10.0).abs() < 1e-5);
            assert!(bvh.nearest(origin, dir, 5.0, &|_, _| true).is_none());
        }
        // Between the squares, nothing.
        assert!(bvh.nearest(Vec3::new(1.5, 0.5, 10.0), Vec3::new(0.0, 0.0, -1.0), f32::MAX, &|_, _| true).is_none());
    }
}
