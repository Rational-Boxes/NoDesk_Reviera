// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Render-agnostic Render Scene and the render backend interface (spec Ch 5 DOC-RBE-001…002).
//!
//! The scene is what any backend draws: triangle meshes per element with a display colour. The
//! built-in viewport (`reviera-render-wgpu`) is one backend; external engines will receive the
//! same scene through the Reviera Scene Protocol.

use glam::{Mat4, Vec3};
use reviera_core::ElementId;

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

/// An orbit camera around a target point, Z up.
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
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 20.0,
            yaw: -0.8,
            pitch: 0.5,
            fov: 45f32.to_radians(),
        }
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let dir = Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        );
        self.target + dir * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.01);
        let far = self.distance * 100.0;
        // wgpu clip space: depth 0..1, Y up (glam's DirectX convention).
        let proj =
            glam::camera::rh::proj::directx::perspective(self.fov, aspect.max(1e-3), near, far);
        proj * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    /// Frames the scene bounds in view.
    pub fn fit(&mut self, bounds: (Vec3, Vec3)) {
        let (lo, hi) = bounds;
        self.target = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(0.5);
        self.distance = radius / (self.fov * 0.5).sin() * 1.1;
    }

    /// Orbits by a mouse drag in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.55, 1.55);
    }

    /// Pans by a mouse drag in pixels, for a viewport `height` pixels tall.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let forward = (self.target - self.eye()).normalize();
        let right = forward.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(forward);
        let scale = 2.0 * self.distance * (self.fov * 0.5).tan() / height.max(1.0);
        self.target += (-right * dx + up * dy) * scale;
    }

    /// Zooms by wheel steps (positive is closer).
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.85f32.powf(steps)).max(0.1);
    }
}

/// A render backend (DOC-RBE-001): draws a Render Scene with a camera.
pub trait RenderBackend {
    fn name(&self) -> &str;
    fn set_scene(&mut self, scene: &RenderScene);
    fn resize(&mut self, width: u32, height: u32);
    fn render(&mut self, camera: &Camera) -> Result<(), String>;
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
        let before = cam.target;
        cam.pan(100.0, 0.0, 600.0);
        assert!(cam.target.distance(before) > 0.0);
    }
}
