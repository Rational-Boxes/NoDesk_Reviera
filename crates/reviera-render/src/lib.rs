// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Render-agnostic Render Scene and the render backend interface (spec Ch 5 DOC-RBE-001…002).
//!
//! The scene is what any backend draws: triangle meshes per element with a display colour. The
//! built-in viewport (`reviera-render-wgpu`) is one backend; external engines will receive the
//! same scene through the Reviera Scene Protocol.

use glam::{Mat4, Vec3};
use reviera_core::ElementId;

pub mod viewcube;

/// One element's triangulated geometry, in world coordinates (metres, Z up).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneMesh {
    pub element: Option<ElementId>,
    pub class: String,
    pub positions: Vec<[f32; 3]>,
    /// Triangles as index triples into `positions`.
    pub indices: Vec<u32>,
    /// Linear RGBA.
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderScene {
    pub meshes: Vec<SceneMesh>,
}

impl RenderScene {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.indices.len() / 3).sum()
    }

    /// Axis-aligned bounds of everything in the scene.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self
            .meshes
            .iter()
            .flat_map(|m| m.positions.iter())
            .map(|p| Vec3::from(*p));
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}

impl RenderScene {
    /// The nearest mesh hit by a ray, with the distance along it (Möller–Trumbore per triangle).
    pub fn pick(&self, origin: Vec3, dir: Vec3) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in self.meshes.iter().enumerate() {
            for tri in m.indices.as_chunks::<3>().0 {
                let [a, b, c] = tri.map(|k| Vec3::from(m.positions[k as usize]));
                if let Some(t) = ray_triangle(origin, dir, a, b, c)
                    && best.is_none_or(|(_, d)| t < d)
                {
                    best = Some((i, t));
                }
            }
        }
        best
    }
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

/// The selection highlight colour.
pub const HIGHLIGHT: [f32; 4] = [0.15, 0.45, 0.95, 1.0];

/// Default display colours by IFC class, until object styles and materials drive them.
pub fn class_color(class: &str) -> [f32; 4] {
    let c = class.strip_suffix("StandardCase").unwrap_or(class);
    match c {
        "IfcWall" => [0.85, 0.83, 0.78, 1.0],
        "IfcSlab" => [0.70, 0.70, 0.72, 1.0],
        "IfcRoof" => [0.62, 0.30, 0.22, 1.0],
        "IfcWindow" => [0.55, 0.75, 0.90, 0.55],
        "IfcDoor" => [0.60, 0.42, 0.25, 1.0],
        "IfcColumn" | "IfcBeam" | "IfcMember" => [0.55, 0.57, 0.62, 1.0],
        "IfcStair" | "IfcStairFlight" | "IfcRailing" => [0.65, 0.60, 0.52, 1.0],
        "IfcSpace" => [0.40, 0.80, 0.50, 0.15],
        "IfcFurniture" | "IfcFurnishingElement" => [0.75, 0.55, 0.35, 1.0],
        "IfcPlate" | "IfcCurtainWall" => [0.50, 0.70, 0.85, 0.5],
        _ => [0.78, 0.78, 0.78, 1.0],
    }
}

/// An orbit camera around a target point, Z up (UI-NAV-001).
///
/// The eye is at `distance` from `target` in the direction given by `yaw` (about Z, from +X) and
/// `pitch` (above the XY plane). Top and bottom views (pitch ±90°) are exact: the up vector
/// follows the yaw, so a top view with the eye on the south side has north up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    /// Rotation about Z, radians.
    pub yaw: f32,
    /// Elevation above the XY plane, radians.
    pub pitch: f32,
    /// Vertical field of view, radians.
    pub fov: f32,
    /// Perspective or parallel (orthographic) projection.
    pub perspective: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 20.0,
            yaw: -0.8,
            pitch: 0.5,
            fov: 45f32.to_radians(),
            perspective: true,
        }
    }
}

/// The largest pitch: straight down onto the model (or straight up).
pub const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2;

impl Camera {
    /// Unit vector from the target to the eye.
    pub fn direction(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.direction() * self.distance
    }

    /// The screen's up direction in the world.
    pub fn up(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.cos() * self.pitch.sin(),
            -self.yaw.sin() * self.pitch.sin(),
            self.pitch.cos(),
        )
    }

    /// The screen's right direction in the world.
    pub fn right(&self) -> Vec3 {
        (-self.direction()).cross(self.up()).normalize_or_zero()
    }

    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye(), self.target, self.up())
    }

    /// Half the height of the view at the target, in metres (the parallel projection's scale).
    pub fn half_height(&self) -> f32 {
        self.distance * (self.fov * 0.5).tan()
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-3);
        // wgpu clip space: depth 0..1, Y up (glam's DirectX convention).
        let proj = if self.perspective {
            let near = (self.distance * 0.01).max(0.01);
            let far = self.distance * 100.0;
            glam::camera::rh::proj::directx::perspective(self.fov, aspect, near, far)
        } else {
            // Parallel: the eye moves back so nothing between it and the target is clipped.
            let h = self.half_height();
            let depth = self.distance * 100.0;
            glam::camera::rh::proj::directx::orthographic(
                -h * aspect,
                h * aspect,
                -h,
                h,
                -depth,
                depth,
            )
        };
        proj * self.view()
    }

    /// Frames the scene bounds in view.
    pub fn fit(&mut self, bounds: (Vec3, Vec3)) {
        let (lo, hi) = bounds;
        self.target = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(0.5);
        self.distance = radius / (self.fov * 0.5).sin() * 1.1;
    }

    /// The ray through pixel (`x`, `y`) of a `width` × `height` viewport: origin and direction.
    pub fn ray(&self, x: f32, y: f32, width: f32, height: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(width / height.max(1.0)).inverse();
        let ndc_x = 2.0 * x / width.max(1.0) - 1.0;
        let ndc_y = 1.0 - 2.0 * y / height.max(1.0);
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        (near, (far - near).normalize())
    }

    /// The pixel position of a world point in a `width` × `height` viewport, or `None` when it is
    /// behind the camera. The inverse of [`Camera::ray`].
    pub fn project(&self, p: Vec3, width: f32, height: f32) -> Option<(f32, f32)> {
        let clip = self.view_proj(width / height.max(1.0)) * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(((ndc.x + 1.0) * 0.5 * width, (1.0 - ndc.y) * 0.5 * height))
    }

    /// Orbits about the target by a mouse drag in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        let pivot = self.target;
        self.orbit_about(dx, dy, pivot);
    }

    /// Orbits about `pivot` by a mouse drag in pixels (UI-NAV-001: the selection's centre, or the
    /// model's centre).
    pub fn orbit_about(&mut self, dx: f32, dy: f32, pivot: Vec3) {
        let (yaw, pitch) = (
            self.yaw - dx * 0.008,
            (self.pitch + dy * 0.008).clamp(-MAX_PITCH, MAX_PITCH),
        );
        let before = self.right();
        let r_pitch = glam::Quat::from_axis_angle(before, -(pitch - self.pitch));
        let r_yaw = glam::Quat::from_rotation_z(yaw - self.yaw);
        let rotation = r_yaw * r_pitch;
        self.target = pivot + rotation * (self.target - pivot);
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Turns the view about the eye (look around), by a mouse drag in pixels.
    pub fn look(&mut self, dx: f32, dy: f32) {
        let eye = self.eye();
        self.yaw -= dx * 0.004;
        self.pitch = (self.pitch + dy * 0.004).clamp(-1.5, 1.5);
        self.target = eye - self.direction() * self.distance;
    }

    /// Moves the eye and target together: `forward` along the level ground, `right` sideways,
    /// `up` vertically, in metres.
    pub fn walk(&mut self, forward: f32, right: f32, up: f32) {
        let ahead = Vec3::new(-self.yaw.cos(), -self.yaw.sin(), 0.0);
        let side = Vec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0);
        self.target += ahead * forward + side * right + Vec3::Z * up;
    }

    /// Pans by a mouse drag in pixels, for a viewport `height` pixels tall.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let scale = 2.0 * self.half_height() / height.max(1.0);
        self.target += (-self.right() * dx + self.up() * dy) * scale;
    }

    /// Zooms by wheel steps (positive is closer).
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.85f32.powf(steps)).max(0.1);
    }

    /// Zooms by wheel steps keeping the point under pixel (`x`, `y`) fixed on screen.
    pub fn zoom_at(&mut self, steps: f32, x: f32, y: f32, width: f32, height: f32) {
        let k = 0.85f32.powf(steps);
        if self.distance * k < 0.1 {
            return;
        }
        // The point under the cursor on the plane through the target, facing the camera.
        let (o, d) = self.ray(x, y, width, height);
        let n = self.direction();
        let denom = d.dot(n);
        if denom.abs() < 1e-6 {
            return self.zoom(steps);
        }
        let p = o + d * ((self.target - o).dot(n) / denom);
        // Scaling the eye and target about `p` keeps `p` on the same pixel.
        self.target = p + (self.target - p) * k;
        self.distance *= k;
    }

    /// Looks at the target from direction `dir` (from the target toward the eye). Straight up or
    /// down keeps north up on screen.
    pub fn orient(&mut self, dir: Vec3) {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        let horizontal = dir.x.hypot(dir.y);
        self.pitch = dir.z.atan2(horizontal);
        self.yaw = if horizontal < 1e-6 {
            -std::f32::consts::FRAC_PI_2
        } else {
            dir.y.atan2(dir.x)
        };
    }

    /// The stored form of this camera (DOC-VIEW-006) for a view `aspect` (width / height) wide:
    /// eye, target, up, projection, horizontal field of view and parallel scale.
    pub fn to_view(&self, aspect: f32) -> reviera_core::views::ViewCamera {
        let v = |p: Vec3| p.to_array().map(f64::from);
        let horizontal = 2.0 * ((self.fov * 0.5).tan() * aspect.max(1e-3)).atan();
        reviera_core::views::ViewCamera {
            eye: v(self.eye()),
            target: v(self.target),
            up: v(self.up()),
            perspective: self.perspective,
            field_of_view: f64::from(horizontal),
            parallel_scale: f64::from(self.half_height()),
        }
    }

    /// A camera from its stored form, for a view `aspect` wide. The horizontal field of view is
    /// kept, so a view shows the same width in any window; parallel views keep their scale.
    pub fn from_view(c: &reviera_core::views::ViewCamera, aspect: f32) -> Camera {
        let p = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        let (eye, target) = (p(c.eye), p(c.target));
        let fov = 2.0 * (((c.field_of_view as f32) * 0.5).tan() / aspect.max(1e-3)).atan();
        let mut cam = Camera {
            target,
            distance: (eye - target).length().max(0.01),
            fov: fov.clamp(0.05, 3.0),
            perspective: c.perspective,
            ..Camera::default()
        };
        cam.orient(eye - target);
        if !c.perspective {
            // Same visible height: the half-height comes from the distance.
            cam.distance = (c.parallel_scale as f32 / (cam.fov * 0.5).tan()).max(0.01);
            cam.target = target;
        }
        cam
    }

    /// The camera a fraction `t` (0..1) of the way from `self` to `to`, for animated
    /// transitions: the yaw turns the short way and the distance changes geometrically.
    pub fn lerp(&self, to: &Camera, t: f32) -> Camera {
        let t = t.clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        let tau = std::f32::consts::TAU;
        let dyaw =
            ((to.yaw - self.yaw + std::f32::consts::PI).rem_euclid(tau)) - std::f32::consts::PI;
        Camera {
            target: self.target.lerp(to.target, s),
            distance: self.distance * (to.distance / self.distance).powf(s),
            yaw: self.yaw + dyaw * s,
            pitch: self.pitch + (to.pitch - self.pitch) * s,
            fov: self.fov + (to.fov - self.fov) * s,
            perspective: if s < 0.5 {
                self.perspective
            } else {
                to.perspective
            },
        }
    }
}

/// Geometry drawn in a screen rectangle over the scene (the Orientation Cube).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gizmo {
    /// Pixel rectangle: x, y (top-left), width, height.
    pub rect: [f32; 4],
    /// Transform of `meshes` into the rectangle's clip space (lit).
    pub view_proj: Mat4,
    /// The direction the light comes from, for shading `meshes`.
    pub eye: Vec3,
    pub meshes: Vec<SceneMesh>,
    /// Unlit geometry already in the rectangle's clip space (-1..1), drawn on top: buttons,
    /// labels.
    pub flat: Vec<SceneMesh>,
}

/// A render backend (DOC-RBE-001): draws a Render Scene with a camera.
pub trait RenderBackend {
    fn name(&self) -> &str;
    fn set_scene(&mut self, scene: &RenderScene);
    fn resize(&mut self, width: u32, height: u32);
    fn render(&mut self, camera: &Camera) -> Result<(), String>;
    /// A gizmo drawn in a corner over the scene with its own camera, such as the Orientation
    /// Cube; `None` hides it.
    fn set_gizmo(&mut self, gizmo: Option<Gizmo>);
    /// Draws these elements in the highlight colour.
    fn set_highlight(&mut self, elements: &[ElementId]);
    /// Transient geometry drawn over the scene, such as the preview of a wall being drawn.
    /// Cheap to replace on every mouse move; the scene is not uploaded again.
    fn set_overlay(&mut self, meshes: &[SceneMesh]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_the_scene_in_front_of_the_camera() {
        let mut cam = Camera::default();
        cam.fit((Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 8.0, 6.0)));
        let clip = cam.view_proj(1.5) * glam::Vec4::new(5.0, 4.0, 3.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4 && (0.0..1.0).contains(&ndc.z),
            "{ndc}"
        );
        let (px, py) = cam.project(Vec3::new(5.0, 4.0, 3.0), 600.0, 400.0).unwrap();
        assert!(
            (px - 300.0).abs() < 1e-2 && (py - 200.0).abs() < 1e-2,
            "{px} {py}"
        );
        let (o, d) = cam.ray(150.0, 80.0, 600.0, 400.0);
        let (px, py) = cam.project(o + d * 7.0, 600.0, 400.0).unwrap();
        assert!(
            (px - 150.0).abs() < 1e-2 && (py - 80.0).abs() < 1e-2,
            "{px} {py}"
        );
        // The ray through the centre pixel hits a box centred in view.
        let (o, d) = cam.ray(300.0, 200.0, 600.0, 400.0);
        let tri = |p: [[f32; 3]; 3]| SceneMesh {
            element: None,
            class: "IfcWall".into(),
            positions: p.to_vec(),
            indices: vec![0, 1, 2],
            color: [1.0; 4],
        };
        let scene = RenderScene {
            meshes: vec![
                tri([[0.0, 0.0, 3.0], [10.0, 0.0, 3.0], [0.0, 8.0, 3.0]]),
                tri([[10.0, 0.0, 3.0], [10.0, 8.0, 3.0], [0.0, 8.0, 3.0]]),
            ],
        };
        let (hit, dist) = scene
            .pick(o, d)
            .expect("the centre ray hits the plane at z = 3");
        assert!(hit <= 1 && dist > 0.0);
        let (o, d) = cam.ray(0.0, 0.0, 600.0, 400.0);
        assert!(scene.pick(o, d).is_none() || scene.pick(o, d).unwrap().1 > 0.0);
        let before = cam.target;
        cam.pan(100.0, 0.0, 600.0);
        assert!(cam.target.distance(before) > 0.0);
    }

    #[test]
    fn wheel_zoom_keeps_the_point_under_the_cursor() {
        for perspective in [true, false] {
            let mut cam = Camera {
                perspective,
                ..Camera::default()
            };
            let (x, y) = (520.0, 130.0);
            let (o, d) = cam.ray(x, y, 800.0, 600.0);
            let n = cam.direction();
            let p = o + d * ((cam.target - o).dot(n) / d.dot(n));
            cam.zoom_at(3.0, x, y, 800.0, 600.0);
            let (px, py) = cam.project(p, 800.0, 600.0).unwrap();
            assert!(
                (px - x).abs() < 0.5 && (py - y).abs() < 0.5,
                "{perspective}: {px} {py}"
            );
        }
    }

    #[test]
    fn orbiting_about_a_pivot_keeps_it_in_place() {
        let mut cam = Camera::default();
        let pivot = Vec3::new(3.0, -2.0, 1.0);
        let before = cam.project(pivot, 800.0, 600.0).unwrap();
        let eye_to_pivot = (pivot - cam.eye()).length();
        cam.orbit_about(40.0, -25.0, pivot);
        assert!(((pivot - cam.eye()).length() - eye_to_pivot).abs() < 1e-3);
        let after = cam.project(pivot, 800.0, 600.0).unwrap();
        assert!(
            (before.0 - after.0).abs() < 0.5 && (before.1 - after.1).abs() < 0.5,
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn a_top_view_has_north_up() {
        let mut cam = Camera::default();
        cam.orient(Vec3::Z);
        assert!((cam.pitch - MAX_PITCH).abs() < 1e-6);
        let centre = cam.project(cam.target, 800.0, 600.0).unwrap();
        let north = cam.project(cam.target + Vec3::Y, 800.0, 600.0).unwrap();
        let east = cam.project(cam.target + Vec3::X, 800.0, 600.0).unwrap();
        assert!(
            north.1 < centre.1 - 1.0 && (north.0 - centre.0).abs() < 1e-3,
            "{north:?}"
        );
        assert!(east.0 > centre.0 + 1.0, "{east:?}");
        // Halfway between two views is between them; the ends are exact.
        let mut side = cam;
        side.orient(-Vec3::Y);
        assert_eq!(cam.lerp(&side, 1.0).direction(), side.direction());
        let mid = cam.lerp(&side, 0.5).direction();
        assert!(mid.z > 0.1 && mid.y < -0.1, "{mid}");
    }

    #[test]
    fn stored_cameras_round_trip() {
        for perspective in [true, false] {
            let mut cam = Camera {
                perspective,
                ..Camera::default()
            };
            cam.fit((Vec3::ZERO, Vec3::new(12.0, 9.0, 6.0)));
            cam.orbit(30.0, -12.0);
            let back = Camera::from_view(&cam.to_view(1.5), 1.5);
            assert!((back.eye() - cam.eye()).length() < 1e-3, "{perspective}");
            assert!((back.target - cam.target).length() < 1e-3);
            assert!((back.fov - cam.fov).abs() < 1e-5 && back.perspective == perspective);
            assert!((back.half_height() - cam.half_height()).abs() < 1e-3);
        }
    }
}
