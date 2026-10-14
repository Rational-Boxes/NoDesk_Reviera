// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! B-rep geometry, booleans, sweeps, sections, tessellation and hidden-line output through OCCT
//! (TECH_STACK §3.2).
//!
//! Built so far: **solid booleans on closed meshes** ([`boolean`]). Each closed part becomes an
//! OCCT solid (its triangles as planar faces sewn into a shell, turned to face out). The
//! subtraction, intersection or union runs on the two compounds, coplanar faces are merged
//! (`ShapeUpgrade_UnifySameDomain`), and the result is triangulated back into closed parts.

pub mod csg;
mod ffi;

pub use reviera_core::bodies::Mesh;

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A − B.
    Subtract,
    /// A ∩ B.
    Intersect,
    /// A ∪ B.
    Union,
}

fn pack(parts: &[Mesh]) -> ffi::bridge::Meshes {
    let mut out = ffi::bridge::Meshes {
        verts: Vec::new(),
        faces: Vec::new(),
        parts: Vec::new(),
    };
    for (points, indices) in parts {
        let base = (out.verts.len() / 3) as u32;
        out.verts.extend(points.iter().flatten());
        out.faces.extend(indices.iter().map(|i| i + base));
        out.parts.push((indices.len() / 3) as u32);
    }
    out
}

/// A boolean of two sets of closed parts: the result's closed parts. An empty result (nothing in
/// common, or everything subtracted) is an empty list.
pub fn boolean(op: Op, a: &[Mesh], b: &[Mesh], tolerance: f64) -> Result<Vec<Mesh>, String> {
    let code = match op {
        Op::Subtract => 0,
        Op::Intersect => 1,
        Op::Union => 2,
    };
    let out = ffi::bridge::mesh_boolean(code, &pack(a), &pack(b), tolerance)
        .map_err(|e| e.what().to_owned())?;
    // Split back into parts, each with its own vertices.
    let mut parts = Vec::new();
    let mut first = 0usize;
    for n in out.parts {
        let tris = &out.faces[3 * first..3 * (first + n as usize)];
        let mut map = std::collections::HashMap::new();
        let mut points: Vec<[f64; 3]> = Vec::new();
        let indices = tris
            .iter()
            .map(|&i| {
                *map.entry(i).or_insert_with(|| {
                    let k = 3 * i as usize;
                    points.push([out.verts[k], out.verts[k + 1], out.verts[k + 2]]);
                    (points.len() - 1) as u32
                })
            })
            .collect();
        parts.push((points, indices));
        first += n as usize;
    }
    Ok(parts)
}

/// The linked OpenCASCADE version.
pub fn occt_version() -> String {
    ffi::bridge::occt_version()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviera_core::bodies::extrude;

    fn cube(lo: [f64; 3], hi: [f64; 3]) -> Mesh {
        let ring = [
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ];
        extrude(&ring, &[], lo[2], hi[2])
    }

    fn volume(parts: &[Mesh]) -> f64 {
        parts
            .iter()
            .map(|(p, idx)| {
                idx.chunks(3)
                    .map(|t| {
                        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                            + a[2] * (b[0] * c[1] - b[1] * c[0]))
                            / 6.0
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    #[test]
    fn subtract_intersect_union() {
        let a = [cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0])];
        let b = [cube([1.0, 1.0, 1.0], [3.0, 3.0, 3.0])];
        let cut = boolean(Op::Subtract, &a, &b, 1e-6).unwrap();
        assert!((volume(&cut) - 7.0).abs() < 1e-6, "{}", volume(&cut));
        let common = boolean(Op::Intersect, &a, &b, 1e-6).unwrap();
        assert!((volume(&common) - 1.0).abs() < 1e-6);
        let union = boolean(Op::Union, &a, &b, 1e-6).unwrap();
        assert!((volume(&union) - 15.0).abs() < 1e-6);
        // A hole right through: the slab keeps its volume less the hole's.
        let slab = [cube([0.0, 0.0, 0.0], [4.0, 4.0, 0.3])];
        let pipe = [cube([1.0, 1.0, -1.0], [2.0, 2.0, 1.0])];
        let holed = boolean(Op::Subtract, &slab, &pipe, 1e-6).unwrap();
        assert!((volume(&holed) - (16.0 - 1.0) * 0.3).abs() < 1e-6);
        // Nothing in common.
        let far = [cube([10.0, 10.0, 10.0], [11.0, 11.0, 11.0])];
        assert!(boolean(Op::Intersect, &a, &far, 1e-6).unwrap().is_empty());
        assert!(!occt_version().is_empty());
    }
}
