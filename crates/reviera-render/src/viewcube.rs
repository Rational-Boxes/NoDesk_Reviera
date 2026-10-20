// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Orientation Cube (spec Ch 1 §1.19.2, UI-NAV-002), drawn by the renderer in a corner of a 3D
//! view. Reviera's own design: a cube with 26 clickable regions (6 faces, 12 edges, 8 corners), a
//! compass ring with N/E/S/W below it, and a Home button. Labels are drawn with a small built-in
//! stroke font, so no font files or artwork are needed.
//!
//! Cube space: the cube spans −1..1 on each axis, +X east, +Y north, +Z up. The Front face looks
//! south (−Y), as in plan conventions.

use glam::{IVec3, Mat4, Vec2, Vec3};

use crate::{Camera, Gizmo, SceneMesh};

/// What a point of the gizmo is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CubeHit {
    /// A face, edge or corner: the direction toward the eye, each component −1, 0 or 1.
    Region(IVec3),
    /// A compass letter: the horizontal direction it names.
    Compass(IVec3),
    /// The compass ring away from the letters (drag to turn about the vertical).
    Ring,
    Home,
    /// The projection button: switches between perspective and isometric (parallel).
    Projection,
}

/// Where the cube is drawn in a view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewCube {
    /// Width and height of the gizmo, in pixels.
    pub size: f32,
    /// Distance from the view's top and right edges, in pixels.
    pub margin: f32,
    /// Opacity when the cursor is elsewhere (0..1).
    pub inactive_opacity: f32,
}

impl Default for ViewCube {
    fn default() -> Self {
        Self {
            size: 150.0,
            margin: 8.0,
            inactive_opacity: 0.8,
        }
    }
}

/// Half the width of the edge and corner bands on each face (of 2).
const BAND: f32 = 0.3;
const RING: (f32, f32) = (1.3, 1.65);
const RING_Z: f32 = -1.0;
/// Half the extent of the gizmo's view, in cube units.
const EXTENT: f32 = 2.05;
const HOME: (Vec2, Vec2) = (Vec2::new(-0.95, 0.62), Vec2::new(-0.65, 0.92));
const PROJECTION: (Vec2, Vec2) = (Vec2::new(-0.98, -0.98), Vec2::new(-0.42, -0.78));
const BUTTON: [f32; 4] = [0.93, 0.95, 0.98, 1.0];

const FACE: [f32; 4] = [0.93, 0.95, 0.98, 1.0];
const BAND_COLOR: [f32; 4] = [0.76, 0.80, 0.87, 1.0];
const HOVER: [f32; 4] = [0.33, 0.60, 0.95, 1.0];
const INK: [f32; 4] = [0.10, 0.12, 0.16, 1.0];
const RING_COLOR: [f32; 4] = [0.62, 0.67, 0.75, 1.0];

impl ViewCube {
    /// The gizmo's pixel rectangle in a `width` × `height` view.
    pub fn rect(&self, width: f32, _height: f32) -> [f32; 4] {
        [
            width - self.size - self.margin,
            self.margin,
            self.size,
            self.size,
        ]
    }

    /// Cube space to the gizmo's clip space: the main camera's rotation, parallel projection.
    pub fn view_proj(&self, camera: &Camera) -> Mat4 {
        let eye = camera.direction() * 5.0;
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, camera.up());
        let proj = glam::camera::rh::proj::directx::orthographic(
            -EXTENT, EXTENT, -EXTENT, EXTENT, 0.1, 10.0,
        );
        proj * view
    }

    /// The pixel position of (`x`, `y`) in the gizmo's clip space, if inside it.
    fn ndc(&self, x: f32, y: f32, width: f32, height: f32) -> Option<Vec2> {
        let [rx, ry, rw, rh] = self.rect(width, height);
        let n = Vec2::new((x - rx) / rw * 2.0 - 1.0, 1.0 - (y - ry) / rh * 2.0);
        (n.x.abs() <= 1.0 && n.y.abs() <= 1.0).then_some(n)
    }

    /// What is under pixel (`x`, `y`) of a `width` × `height` view.
    pub fn pick(
        &self,
        camera: &Camera,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<CubeHit> {
        let n = self.ndc(x, y, width, height)?;
        if n.cmpge(HOME.0).all() && n.cmple(HOME.1).all() {
            return Some(CubeHit::Home);
        }
        if n.cmpge(PROJECTION.0).all() && n.cmple(PROJECTION.1).all() {
            return Some(CubeHit::Projection);
        }
        let inv = self.view_proj(camera).inverse();
        let o = inv.project_point3(n.extend(0.0));
        let d = (inv.project_point3(n.extend(1.0)) - o).normalize();
        let cube = slab(o, d).map(|(t, p)| (t, classify(p)));
        let ring = (d.z.abs() > 1e-6)
            .then(|| (RING_Z - o.z) / d.z)
            .filter(|t| *t > 0.0)
            .and_then(|t| {
                let p = o + d * t;
                let r = p.truncate().length();
                (RING.0..=RING.1)
                    .contains(&r)
                    .then(|| (t, ring_hit(p.truncate())))
            });
        match (cube, ring) {
            (Some(c), Some(r)) => Some(if c.0 <= r.0 { c.1 } else { r.1 }),
            (Some(c), None) => Some(c.1),
            (None, Some(r)) => Some(r.1),
            (None, None) => None,
        }
    }

    /// The gizmo to draw for `camera`, highlighting `hover`; opaque while the cursor is over it.
    pub fn gizmo(&self, camera: &Camera, width: f32, height: f32, hover: Option<CubeHit>) -> Gizmo {
        let alpha = if hover.is_some() {
            1.0
        } else {
            self.inactive_opacity
        };
        let tint = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * alpha];
        let toward = camera.direction();
        let mut meshes = Vec::new();
        let mut quads = Mesh::default();
        let mut ink = Mesh::default();
        for (axis, sign) in faces() {
            let normal = axis_vec(axis) * sign;
            if normal.dot(toward) <= 1e-4 {
                // Back faces are not drawn, so the cube reads as a solid.
                continue;
            }
            let (u, v) = face_axes(axis, sign);
            let cuts = [-1.0, -1.0 + 2.0 * BAND, 1.0 - 2.0 * BAND, 1.0];
            for i in 0..3 {
                for j in 0..3 {
                    let region = normal.round().as_ivec3()
                        + u.as_ivec3() * (i as i32 - 1)
                        + v.as_ivec3() * (j as i32 - 1);
                    let base = if i == 1 && j == 1 { FACE } else { BAND_COLOR };
                    let color = if hover == Some(CubeHit::Region(region)) {
                        HOVER
                    } else {
                        base
                    };
                    let (u0, u1, v0, v1) = (cuts[i], cuts[i + 1], cuts[j], cuts[j + 1]);
                    let p = |a: f32, b: f32| normal + u * a + v * b;
                    quads.quad([p(u0, v0), p(u1, v0), p(u1, v1), p(u0, v1)], tint(color));
                }
            }
            let label = face_label(axis, sign);
            let (right, up) = label_axes(axis, sign);
            text(&mut ink, label, normal * 1.004, right, up, 0.27, tint(INK));
        }
        quads.into_scene(&mut meshes);
        ink.into_scene(&mut meshes);

        // Compass ring and letters, flat below the cube.
        let mut ring = Mesh::default();
        let segments = 64;
        for k in 0..segments {
            let a0 = k as f32 / segments as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let at = |a: f32, r: f32| Vec3::new(a.cos() * r, a.sin() * r, RING_Z);
            let color = if hover == Some(CubeHit::Ring) {
                HOVER
            } else {
                RING_COLOR
            };
            ring.quad(
                [
                    at(a0, RING.0),
                    at(a1, RING.0),
                    at(a1, RING.1),
                    at(a0, RING.1),
                ],
                tint(color),
            );
        }
        let mut letters = Mesh::default();
        for (name, dir) in COMPASS {
            let out = dir.as_vec3();
            let centre = out * ((RING.0 + RING.1) / 2.0) + Vec3::Z * (RING_Z + 0.004);
            // Letters read with north up, like a plan.
            let color = if hover == Some(CubeHit::Compass(dir)) {
                HOVER
            } else {
                INK
            };
            text(
                &mut letters,
                name,
                centre,
                Vec3::X,
                Vec3::Y,
                0.26,
                tint(color),
            );
        }
        ring.into_scene(&mut meshes);
        letters.into_scene(&mut meshes);

        // The Home button: a house, in the gizmo's top-left corner.
        let mut home = Mesh::default();
        let color = tint(if hover == Some(CubeHit::Home) {
            HOVER
        } else {
            INK
        });
        let (lo, hi) = HOME;
        let w = hi.x - lo.x;
        let at = |x: f32, y: f32| Vec3::new(lo.x + x * w, lo.y + y * w, 0.0);
        home.tri([at(0.05, 0.55), at(0.95, 0.55), at(0.5, 0.95)], color);
        home.quad(
            [at(0.2, 0.05), at(0.8, 0.05), at(0.8, 0.55), at(0.2, 0.55)],
            color,
        );

        // The projection button: shows the current projection; a click switches it.
        let (lo, hi) = PROJECTION;
        let at = |x: f32, y: f32| Vec3::new(x, y, 0.0);
        let hovered = hover == Some(CubeHit::Projection);
        home.quad(
            [
                at(lo.x, lo.y),
                at(hi.x, lo.y),
                at(hi.x, hi.y),
                at(lo.x, hi.y),
            ],
            tint(if hovered { HOVER } else { BUTTON }),
        );
        let label = if camera.perspective { "PERSP" } else { "ISO" };
        let centre = Vec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, 0.0);
        text(&mut home, label, centre, Vec3::X, Vec3::Y, 0.11, tint(INK));

        Gizmo {
            rect: self.rect(width, height),
            view_proj: self.view_proj(camera),
            eye: toward * 5.0,
            meshes,
            flat: {
                let mut flat = Vec::new();
                home.into_scene(&mut flat);
                flat
            },
        }
    }
}

/// The camera direction (toward the eye) for a region or compass letter.
pub fn hit_direction(hit: CubeHit) -> Option<Vec3> {
    match hit {
        CubeHit::Region(d) | CubeHit::Compass(d) => Some(d.as_vec3().normalize()),
        _ => None,
    }
}

const COMPASS: [(&str, IVec3); 4] = [
    ("N", IVec3::new(0, 1, 0)),
    ("E", IVec3::new(1, 0, 0)),
    ("S", IVec3::new(0, -1, 0)),
    ("W", IVec3::new(-1, 0, 0)),
];

fn ring_hit(p: Vec2) -> CubeHit {
    let angle = p.y.atan2(p.x);
    COMPASS
        .iter()
        .find(|(_, d)| {
            let a = (d.y as f32).atan2(d.x as f32);
            let diff = (angle - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            diff.abs() < 0.3
        })
        .map_or(CubeHit::Ring, |(_, d)| CubeHit::Compass(*d))
}

/// The first hit of a ray with the cube: distance and point.
fn slab(o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    let (mut t0, mut t1) = (f32::MIN, f32::MAX);
    for k in 0..3 {
        if d[k].abs() < 1e-9 {
            if o[k].abs() > 1.0 {
                return None;
            }
            continue;
        }
        let (a, b) = ((-1.0 - o[k]) / d[k], (1.0 - o[k]) / d[k]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    (t0 <= t1 && t1 > 0.0).then(|| (t0, o + d * t0))
}

/// The region of a point on the cube's surface.
fn classify(p: Vec3) -> CubeHit {
    let edge = 1.0 - 2.0 * BAND;
    let c = |v: f32| {
        if v > edge - 1e-4 {
            1
        } else if v < -edge + 1e-4 {
            -1
        } else {
            0
        }
    };
    CubeHit::Region(IVec3::new(c(p.x), c(p.y), c(p.z)))
}

fn faces() -> [(usize, f32); 6] {
    [
        (0, 1.0),
        (0, -1.0),
        (1, 1.0),
        (1, -1.0),
        (2, 1.0),
        (2, -1.0),
    ]
}

fn axis_vec(axis: usize) -> Vec3 {
    [Vec3::X, Vec3::Y, Vec3::Z][axis]
}

/// Two axes spanning a face.
fn face_axes(axis: usize, _sign: f32) -> (Vec3, Vec3) {
    match axis {
        0 => (Vec3::Y, Vec3::Z),
        1 => (Vec3::X, Vec3::Z),
        _ => (Vec3::X, Vec3::Y),
    }
}

fn face_label(axis: usize, sign: f32) -> &'static str {
    match (axis, sign > 0.0) {
        (0, true) => "RIGHT",
        (0, false) => "LEFT",
        (1, true) => "BACK",
        (1, false) => "FRONT",
        (2, true) => "TOP",
        _ => "BOTTOM",
    }
}

/// Reading directions of a face label, seen from outside the cube.
fn label_axes(axis: usize, sign: f32) -> (Vec3, Vec3) {
    match (axis, sign > 0.0) {
        (0, true) => (Vec3::Y, Vec3::Z),
        (0, false) => (-Vec3::Y, Vec3::Z),
        (1, true) => (-Vec3::X, Vec3::Z),
        (1, false) => (Vec3::X, Vec3::Z),
        (2, true) => (Vec3::X, Vec3::Y),
        _ => (Vec3::X, -Vec3::Y),
    }
}

/// Triangles of one colour: colour, positions, indices.
type Group = ([f32; 4], Vec<[f32; 3]>, Vec<u32>);

/// Triangles grouped by colour (a scene mesh has one colour).
#[derive(Default)]
pub(crate) struct Mesh {
    groups: Vec<Group>,
}

impl Mesh {
    pub(crate) fn tri(&mut self, p: [Vec3; 3], color: [f32; 4]) {
        let k = match self.groups.iter().position(|g| g.0 == color) {
            Some(k) => k,
            None => {
                self.groups.push((color, Vec::new(), Vec::new()));
                self.groups.len() - 1
            }
        };
        let (_, positions, indices) = &mut self.groups[k];
        let base = positions.len() as u32;
        positions.extend(p.map(|v| v.to_array()));
        indices.extend([base, base + 1, base + 2]);
    }

    pub(crate) fn quad(&mut self, p: [Vec3; 4], color: [f32; 4]) {
        self.tri([p[0], p[1], p[2]], color);
        self.tri([p[0], p[2], p[3]], color);
    }

    pub(crate) fn into_scene(self, out: &mut Vec<SceneMesh>) {
        out.extend(
            self.groups
                .into_iter()
                .map(|(color, positions, indices)| SceneMesh {
                    element: None,
                    class: "gizmo".into(),
                    positions,
                    indices,
                    color,
                }),
        );
    }
}

/// Lays out `s` centred at `centre`, reading along `right` with `up` upward, letters `height`
/// tall, as thin quads (the strokes of [`crate::font`]).
pub(crate) fn text(
    mesh: &mut Mesh,
    s: &str,
    centre: Vec3,
    right: Vec3,
    up: Vec3,
    height: f32,
    color: [f32; 4],
) {
    let width = crate::font::width(s, height);
    let origin = centre - right * (width / 2.0) - up * (height / 2.0);
    let stroke = height / 6.0 * 0.9;
    let normal = right.cross(up);
    for [a, b] in crate::font::strokes(s, origin, right, up, height) {
        let along = (b - a).normalize_or_zero();
        let side = normal.cross(along) * (stroke / 2.0);
        let ext = along * (stroke / 2.0);
        mesh.quad(
            [
                a - side - ext,
                b - side + ext,
                b + side + ext,
                a + side - ext,
            ],
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera_from(dir: Vec3) -> Camera {
        let mut c = Camera::default();
        c.orient(dir);
        c
    }

    #[test]
    fn the_centre_of_the_cube_is_the_face_toward_the_camera() {
        let cube = ViewCube::default();
        let [rx, ry, rw, rh] = cube.rect(800.0, 600.0);
        let centre = (rx + rw / 2.0, ry + rh / 2.0);
        for dir in [Vec3::Z, -Vec3::Y, Vec3::X, Vec3::new(1.0, -1.0, 1.0)] {
            let cam = camera_from(dir);
            let hit = cube.pick(&cam, centre.0, centre.1, 800.0, 600.0);
            let expected = IVec3::new(
                dir.x.signum() as i32 * (dir.x != 0.0) as i32,
                dir.y.signum() as i32 * (dir.y != 0.0) as i32,
                dir.z.signum() as i32 * (dir.z != 0.0) as i32,
            );
            assert_eq!(hit, Some(CubeHit::Region(expected)), "{dir}");
        }
    }

    #[test]
    fn all_26_regions_give_distinct_directions() {
        let mut seen = std::collections::HashSet::new();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if (x, y, z) == (0, 0, 0) {
                        continue;
                    }
                    let d = hit_direction(CubeHit::Region(IVec3::new(x, y, z))).unwrap();
                    let mut cam = Camera::default();
                    cam.orient(d);
                    assert!((cam.direction() - d).length() < 1e-5, "{d}");
                    seen.insert((x, y, z));
                }
            }
        }
        assert_eq!(seen.len(), 26);
    }

    #[test]
    fn home_and_compass_are_hit() {
        let cube = ViewCube::default();
        let [rx, ry, rw, rh] = cube.rect(800.0, 600.0);
        let px = |n: Vec2| (rx + (n.x + 1.0) / 2.0 * rw, ry + (1.0 - n.y) / 2.0 * rh);
        let (x, y) = px((HOME.0 + HOME.1) / 2.0);
        let cam = camera_from(Vec3::Z);
        assert_eq!(cube.pick(&cam, x, y, 800.0, 600.0), Some(CubeHit::Home));
        let (x, y) = px((PROJECTION.0 + PROJECTION.1) / 2.0);
        assert_eq!(
            cube.pick(&cam, x, y, 800.0, 600.0),
            Some(CubeHit::Projection)
        );
        // From the top with north up, the N letter is above the cube.
        let r = (RING.0 + RING.1) / 2.0 / EXTENT;
        let (x, y) = px(Vec2::new(0.0, r));
        assert_eq!(
            cube.pick(&cam, x, y, 800.0, 600.0),
            Some(CubeHit::Compass(IVec3::new(0, 1, 0)))
        );
    }
}
