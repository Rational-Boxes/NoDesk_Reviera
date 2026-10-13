// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Closed meshes of simple solids for the viewport: a plan profile with holes extruded between
//! two elevations (floors now; ceilings and roofs later).

/// A closed mesh of the profile `outer` (with `holes`) between `z0` and `z1`: top and bottom
/// faces triangulated with their holes, and a side face per edge, all facing out.
pub fn extrude(
    outer: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    z0: f64,
    z1: f64,
) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut points: Vec<[f64; 2]> = outer.to_vec();
    let mut starts = Vec::new();
    for h in holes {
        starts.push(points.len() as u32);
        points.extend(h.iter().copied());
    }
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(points.iter().copied(), &starts, &mut tris);
    let area = |t: &[u32]| {
        let [a, b, c] = [
            points[t[0] as usize],
            points[t[1] as usize],
            points[t[2] as usize],
        ];
        (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
    };
    for (z, up) in [(z1, true), (z0, false)] {
        let base = positions.len() as u32;
        positions.extend(points.iter().map(|p| [p[0] as f32, p[1] as f32, z as f32]));
        for t in tris.chunks(3) {
            // Counter-clockwise from above faces up.
            let ccw = area(t) > 0.0;
            if ccw == up {
                indices.extend([base + t[0], base + t[1], base + t[2]]);
            } else {
                indices.extend([base + t[0], base + t[2], base + t[1]]);
            }
        }
    }
    // Sides: outer rings counter-clockwise and holes clockwise face out on their right.
    let ccw = |r: &[[f64; 2]]| {
        (0..r.len())
            .map(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            > 0.0
    };
    let rings = std::iter::once((outer, true)).chain(holes.iter().map(|h| (h.as_slice(), false)));
    for (ring, is_outer) in rings {
        let forward = ccw(ring) == is_outer;
        for i in 0..ring.len() {
            let (mut a, mut b) = (ring[i], ring[(i + 1) % ring.len()]);
            if !forward {
                std::mem::swap(&mut a, &mut b);
            }
            let base = positions.len() as u32;
            for (p, z) in [(a, z0), (b, z0), (b, z1), (a, z1)] {
                positions.push([p[0] as f32, p[1] as f32, z as f32]);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (positions, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slab_with_a_hole_is_closed_with_the_right_volume() {
        let outer = [[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]];
        let hole = vec![[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]];
        let (p, i) = extrude(&outer, &[hole], -0.3, 0.0);
        let v: f64 = i
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|k| p[k as usize].map(f64::from));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum();
        assert!((v - 0.3 * (80.0 - 4.0)).abs() < 1e-4, "{v}");
    }
}
