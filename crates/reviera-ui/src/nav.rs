// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View navigation (spec Ch 1 §1.19): the Orientation Cube, standard and isometric views with
//! animated transitions, Home, perspective and parallel projection, Previous/Next pan/zoom, orbit
//! about the selection, and walk mode.
//!
//! Navigation changes only the view's camera: it adds no undo entries and does not mark the model
//! modified (UI-NAV-001).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use glam::Vec3;
use reviera_render::viewcube::{CubeHit, ViewCube, hit_direction};
use reviera_render::{Camera, RenderBackend};
use wxdragon::prelude::*;

use super::Gui;

/// Length of animated view changes (Orientation Cube option, §1.19.2).
const ANIMATION: Duration = Duration::from_millis(450);
/// Navigation history per view (Previous/Next pan/zoom).
const HISTORY: usize = 50;
/// Eye height above the level in walk mode (DOC-VIEW-007 default eye elevation).
pub const EYE_HEIGHT: f32 = 1.75;
/// Walking speed in metres per second; Shift runs at three times this.
const WALK_SPEED: f32 = 1.4;

/// Navigation state of one view.
#[derive(Default)]
pub struct Nav {
    pub cube: ViewCube,
    /// What of the cube is under the cursor.
    pub hover: Option<CubeHit>,
    /// The cube part a left press started on.
    pub cube_press: Option<CubeHit>,
    /// The view's home orientation; the default is a south-east isometric of the whole model.
    pub home: Option<Camera>,
    back: Vec<Camera>,
    forward: Vec<Camera>,
    /// An animated change: from, to, start.
    anim: Option<(Camera, Camera, Instant)>,
    pub walk: Option<Walk>,
    /// When the last wheel zoom was remembered (a burst of wheel steps is one history entry).
    last_wheel: Option<Instant>,
}

/// Walk mode: the keys held down, the speed factor, and the camera to return to.
pub struct Walk {
    keys: HashSet<i32>,
    shift: bool,
    pub speed: f32,
    last: Instant,
}

/// The standard orientations of the View menu and the cube's menu: (label, direction toward the
/// eye).
pub const PRESETS: [(&str, [f32; 3]); 14] = [
    ("Top", [0.0, 0.0, 1.0]),
    ("Bottom", [0.0, 0.0, -1.0]),
    ("Front (south)", [0.0, -1.0, 0.0]),
    ("Back (north)", [0.0, 1.0, 0.0]),
    ("Left (west)", [-1.0, 0.0, 0.0]),
    ("Right (east)", [1.0, 0.0, 0.0]),
    ("South-East Isometric", [1.0, -1.0, 1.0]),
    ("South-West Isometric", [-1.0, -1.0, 1.0]),
    ("North-East Isometric", [1.0, 1.0, 1.0]),
    ("North-West Isometric", [-1.0, 1.0, 1.0]),
    ("North-East", [1.0, 1.0, 0.0]),
    ("North-West", [-1.0, 1.0, 0.0]),
    ("South-East", [1.0, -1.0, 0.0]),
    ("South-West", [-1.0, -1.0, 0.0]),
];

impl Gui {
    /// Records the active view's camera for Previous Pan/Zoom.
    pub(crate) fn remember(&mut self) {
        let Some(v) = self.active_view_mut() else {
            return;
        };
        let camera = v.camera;
        if v.nav.back.last() != Some(&camera) {
            v.nav.back.push(camera);
            if v.nav.back.len() > HISTORY {
                v.nav.back.remove(0);
            }
        }
        v.nav.forward.clear();
    }

    /// Remembers the camera once per burst of wheel steps.
    pub(crate) fn remember_wheel(&mut self) {
        let fresh = self
            .active_view()
            .and_then(|v| v.nav.last_wheel)
            .is_none_or(|t| t.elapsed() > Duration::from_millis(600));
        if fresh {
            self.remember();
        }
        if let Some(v) = self.active_view_mut() {
            v.nav.last_wheel = Some(Instant::now());
        }
    }

    /// Animates the active view to `to` (the change is remembered for Previous).
    pub(crate) fn animate_to(&mut self, to: Camera) {
        self.leave_walk();
        self.remember();
        let Some(v) = self.active_view_mut() else {
            return;
        };
        v.nav.anim = Some((v.camera, to, Instant::now()));
        self.start_timer();
    }

    /// Looks at the active view's target from `dir`, keeping the projection and distance.
    pub(crate) fn orient(&mut self, dir: Vec3) {
        if self.in_2d() {
            return self
                .frame
                .set_status_text("Plans always look straight down", 0);
        }
        let Some(v) = self.active_view() else { return };
        let mut to = v.camera;
        to.orient(dir);
        self.animate_to(to);
    }

    /// The home orientation of the active view: its saved home, or a south-east isometric view of
    /// the whole model.
    fn home_camera(&self) -> Option<Camera> {
        let v = self.active_view()?;
        if let Some(h) = v.nav.home {
            return Some(h);
        }
        let mut c = Camera {
            perspective: v.camera.perspective,
            ..Camera::default()
        };
        c.orient(Vec3::new(1.0, -1.0, 1.0));
        match self.scene().bounds() {
            Some(b) => c.fit(b),
            None => c.fit((Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 3.0))),
        }
        Some(c)
    }

    pub(crate) fn go_home(&mut self) {
        if self.in_2d() {
            return self.zoom_to_fit();
        }
        if let Some(c) = self.home_camera() {
            self.animate_to(c);
        }
    }

    pub(crate) fn set_home(&mut self) {
        let mut saved = None;
        if let Some(v) = self.active_view_mut() {
            v.nav.home = Some(v.camera);
            let size = v.panel.get_client_size();
            let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
            saved = v.element.map(|e| (v.doc, e, v.camera.to_view(aspect)));
        }
        // Home is view state of the session, saved with the next save.
        if let Some((doc, element, home)) = saved {
            self.app.set_view_home(doc, element, home);
        }
        self.frame
            .set_status_text("The current view is now Home", 0);
    }

    pub(crate) fn toggle_perspective(&mut self) {
        if self.in_2d() {
            return self.frame.set_status_text(
                "Perspective and isometric apply to 3D views; this is a plan",
                0,
            );
        }
        self.leave_walk();
        let Some(v) = self.active_view_mut() else {
            return;
        };
        v.camera.perspective = !v.camera.perspective;
        let text = if v.camera.perspective {
            "Perspective"
        } else {
            "Isometric (parallel projection)"
        };
        self.frame.set_status_text(text, 0);
        self.properties_stale = true;
        self.refresh_active();
    }

    /// Previous (`back`) or Next pan/zoom.
    pub(crate) fn history(&mut self, back: bool) {
        self.leave_walk();
        let Some(v) = self.active_view_mut() else {
            return;
        };
        let (from, to) = if back {
            (&mut v.nav.back, &mut v.nav.forward)
        } else {
            (&mut v.nav.forward, &mut v.nav.back)
        };
        let Some(camera) = from.pop() else { return };
        to.push(v.camera);
        v.nav.anim = Some((v.camera, camera, Instant::now()));
        self.start_timer();
    }

    /// Frames the model, or the selection when there is one (Zoom to Fit).
    pub(crate) fn zoom_to_fit(&mut self) {
        let bounds = self.selection_bounds().or_else(|| self.scene().bounds());
        let (Some(b), Some(v)) = (bounds, self.active_view()) else {
            return;
        };
        let mut to = v.camera;
        to.fit(b);
        self.animate_to(to);
    }

    /// The bounding box of the selected elements in the active view's scene.
    fn selection_bounds(&mut self) -> Option<(Vec3, Vec3)> {
        let selected: HashSet<_> = self.selection().into_iter().collect();
        if selected.is_empty() {
            return None;
        }
        let points = self
            .scene()
            .meshes
            .iter()
            .filter(|m| m.element.is_some_and(|e| selected.contains(&e)))
            .flat_map(|m| m.positions.iter().map(|p| Vec3::from(*p)));
        points.fold(None, |acc, p| match acc {
            None => Some((p, p)),
            Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
        })
    }

    /// The orbit centre: the selection's centre, else the view's target (UI-NAV-001).
    pub(crate) fn pivot(&mut self) -> Vec3 {
        match self.selection_bounds() {
            Some((lo, hi)) => (lo + hi) * 0.5,
            None => self.camera().target,
        }
    }

    // ---- Orientation Cube ----

    /// The cube part under pixel (`x`, `y`) of the active view.
    pub(crate) fn cube_pick(&self, x: i32, y: i32) -> Option<CubeHit> {
        let v = self.active_view()?;
        if v.nav.walk.is_some() || v.is_2d() {
            return None;
        }
        let size = v.panel.get_client_size();
        v.nav.cube.pick(
            &v.camera,
            x as f32,
            y as f32,
            size.width as f32,
            size.height as f32,
        )
    }

    /// Updates the hovered cube part; returns whether the view needs redrawing.
    pub(crate) fn cube_hover(&mut self, x: Option<(i32, i32)>) -> bool {
        let hit = x.and_then(|(x, y)| self.cube_pick(x, y));
        let Some(v) = self.active_view_mut() else {
            return false;
        };
        let changed = v.nav.hover != hit;
        v.nav.hover = hit;
        changed
    }

    /// A click on the cube: a face, edge, corner or compass letter turns the view to look from
    /// there; Home goes home.
    pub(crate) fn cube_click(&mut self, hit: CubeHit) {
        match hit {
            CubeHit::Home => return self.go_home(),
            CubeHit::Projection => return self.toggle_perspective(),
            _ => {}
        }
        if let Some(dir) = hit_direction(hit) {
            // A compass letter keeps the current elevation of the view.
            let dir = match hit {
                CubeHit::Compass(_) => {
                    let pitch = self.camera().pitch;
                    let h = dir.normalize() * pitch.cos();
                    Vec3::new(h.x, h.y, pitch.sin())
                }
                _ => dir,
            };
            self.orient(dir);
        }
    }

    /// The cube's right-click menu (§1.19.2).
    pub(crate) fn cube_menu(&self) -> Menu {
        let perspective = self.camera().perspective;
        let mut orient = Menu::builder();
        for (k, (label, _)) in PRESETS.iter().enumerate() {
            orient = orient.append_item(super::ID_ORIENT_FIRST + k as i32, label, "");
        }
        let menu = Menu::builder()
            .append_item(super::ID_HOME, "Go &Home", "")
            .append_item(super::ID_SET_HOME, "Set Current View as Home", "")
            .append_separator()
            .append_item(
                super::ID_PERSPECTIVE,
                if perspective {
                    "Switch to &Isometric (parallel)"
                } else {
                    "Switch to &Perspective"
                },
                "",
            )
            .build();
        menu.append_submenu(orient.build(), "Orient to &Direction", "");
        menu
    }

    // ---- Walk mode ----

    /// Enters or leaves walk mode in the active view.
    pub(crate) fn toggle_walk(&mut self) {
        if self.in_2d() {
            return self.frame.set_status_text("Walk mode is for 3D views", 0);
        }
        if self.leave_walk() {
            return;
        }
        let floor = self.walk_floor();
        let frame = self.frame;
        self.end_tool(&frame);
        let Some(v) = self.active_view_mut() else {
            return;
        };
        v.nav.back.push(v.camera);
        // Stand at the view's target, at eye height above the level below it, looking level.
        let mut c = v.camera;
        let mut ground = c.target;
        ground.z = floor + EYE_HEIGHT;
        let look = Vec3::new(-c.yaw.cos(), -c.yaw.sin(), 0.0);
        c.perspective = true;
        c.pitch = 0.0;
        c.distance = 1.0;
        c.target = ground + look;
        v.camera = c;
        v.nav.walk = Some(Walk {
            keys: HashSet::new(),
            shift: false,
            speed: 1.0,
            last: Instant::now(),
        });
        v.panel.set_focus();
        v.panel.refresh(false, None);
        frame.set_status_text(
            "Walk: W/A/S/D or arrows to move, drag to look, Q/E down/up, Shift to run, \
             wheel for speed, Esc to stop",
            0,
        );
    }

    /// Leaves walk mode in the active view, back to orbiting about a point ahead of the eye, with
    /// the Orientation Cube shown again. Returns whether the view was in walk mode.
    pub(crate) fn leave_walk(&mut self) -> bool {
        let frame = self.frame;
        let Some(v) = self.active_view_mut() else {
            return false;
        };
        if v.nav.walk.take().is_none() {
            return false;
        }
        let eye = v.camera.eye();
        let ahead = -v.camera.direction();
        v.camera.distance = 10.0;
        v.camera.target = eye + ahead * 10.0;
        v.nav.hover = None;
        v.panel.refresh(false, None);
        frame.set_status_text("Ready", 0);
        true
    }

    /// The elevation of the level at or below the active view's target (0 without levels).
    fn walk_floor(&self) -> f32 {
        let Some(doc) = self.doc.and_then(|d| self.app.document(d)) else {
            return 0.0;
        };
        let z = self.camera().target.z;
        let levels: Vec<f32> = doc
            .doc
            .store()
            .iter()
            .filter(|e| e.class == "IfcBuildingStorey")
            .filter_map(|e| e.param("@Elevation").and_then(serde_json::Value::as_f64))
            .map(|v| v as f32)
            .collect();
        levels
            .iter()
            .copied()
            .filter(|l| *l <= z + 0.01)
            .fold(None, |a: Option<f32>, l| Some(a.map_or(l, |a| a.max(l))))
            .or_else(|| levels.iter().copied().reduce(f32::min))
            .unwrap_or(0.0)
    }

    /// A key going down (`down`) or up in walk mode; returns whether it was used.
    pub(crate) fn walk_key(&mut self, key: i32, down: bool, shift: bool) -> bool {
        let Some(v) = self.active_view_mut() else {
            return false;
        };
        let Some(w) = &mut v.nav.walk else {
            return false;
        };
        let key = match key {
            k if (97..=122).contains(&k) => k - 32,
            k => k,
        };
        let moves = [
            'W' as i32,
            'A' as i32,
            'S' as i32,
            'D' as i32,
            'Q' as i32,
            'E' as i32,
            WXK_UP,
            WXK_DOWN,
            WXK_LEFT,
            WXK_RIGHT,
            WXK_PAGEUP,
            WXK_PAGEDOWN,
        ];
        w.shift = shift;
        if !moves.contains(&key) {
            return false;
        }
        if down {
            if w.keys.is_empty() {
                w.last = Instant::now();
            }
            w.keys.insert(key);
            self.start_timer();
        } else {
            w.keys.remove(&key);
        }
        true
    }

    /// Changes the walking speed by wheel steps.
    pub(crate) fn walk_speed(&mut self, steps: f32) -> bool {
        let Some(v) = self.active_view_mut() else {
            return false;
        };
        let Some(w) = &mut v.nav.walk else {
            return false;
        };
        w.speed = (w.speed * 1.25f32.powf(steps)).clamp(0.1, 50.0);
        let text = format!("Walk speed {:.1} m/s", WALK_SPEED * w.speed);
        self.frame.set_status_text(&text, 0);
        true
    }

    // ---- Animation ----

    pub(crate) fn start_timer(&self) {
        if let Some(t) = &self.timer
            && !t.is_running()
        {
            t.start(16, false);
        }
    }

    /// Advances animations and walking; returns whether anything is still moving.
    pub(crate) fn tick(&mut self) -> bool {
        let mut busy = false;
        for v in &mut self.views {
            let mut moved = false;
            if let Some((from, to, start)) = v.nav.anim {
                let t = start.elapsed().as_secs_f32() / ANIMATION.as_secs_f32();
                v.camera = from.lerp(&to, t);
                if t >= 1.0 {
                    v.camera = to;
                    v.nav.anim = None;
                } else {
                    busy = true;
                }
                moved = true;
            }
            if let Some(w) = &mut v.nav.walk
                && !w.keys.is_empty()
            {
                let now = Instant::now();
                let dt = (now - w.last).as_secs_f32().min(0.1);
                w.last = now;
                let step = WALK_SPEED * w.speed * if w.shift { 3.0 } else { 1.0 } * dt;
                let held = |keys: &[i32]| keys.iter().any(|k| w.keys.contains(k));
                let axis =
                    |pos: &[i32], neg: &[i32]| (held(pos) as i32 - held(neg) as i32) as f32 * step;
                let forward = axis(&['W' as i32, WXK_UP], &['S' as i32, WXK_DOWN]);
                let right = axis(&['D' as i32, WXK_RIGHT], &['A' as i32, WXK_LEFT]);
                let up = axis(&['E' as i32, WXK_PAGEUP], &['Q' as i32, WXK_PAGEDOWN]);
                v.camera.walk(forward, right, up);
                busy = true;
                moved = true;
            }
            if moved {
                v.panel.refresh(false, None);
            }
        }
        busy
    }

    /// The gizmo of view `index` for its current camera (none in walk mode).
    pub(crate) fn update_gizmo(&mut self, index: usize, scale: f64) {
        let v = &mut self.views[index];
        let Some(viewport) = &mut v.viewport else {
            return;
        };
        if v.nav.walk.is_some() || v.plan.is_some() || v.section.is_some() {
            viewport.set_gizmo(None);
            return;
        }
        let size = v.panel.get_client_size();
        let mut g = v.nav.cube.gizmo(
            &v.camera,
            size.width as f32,
            size.height as f32,
            v.nav.hover,
        );
        g.rect = g.rect.map(|c| c * scale as f32);
        viewport.set_gizmo(Some(g));
    }
}
