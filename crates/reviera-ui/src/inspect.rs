// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Inspection tools of the window: the section box and slice planes of 3D views with their grips
//! (DOC-VIEW-008, DOC-VIEW-014), temporary hide, isolate and translucency (DOC-GFX-008), and the
//! Model Tree pane (UI-TREE-001).

use std::collections::HashSet;

use glam::Vec3;
use reviera_core::ElementId;
use reviera_core::inspect::{NodeKind, TreeNode};
use reviera_core::views::{self, MIN_BOX, SectionBox, SlicePlane};
use reviera_render::plan::Lines;
use reviera_render::{Camera, Clipping, RenderBackend, Visibility};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, View};

/// Section box outline and grips.
const BOX_COLOR: [f32; 4] = [0.95, 0.55, 0.10, 1.0];
/// Slice plane outlines and grips.
const SLICE_COLOR: [f32; 4] = [0.10, 0.65, 0.70, 1.0];
/// How near (pixels) the pointer must be to a grip to take it.
const GRIP_REACH: f32 = 9.0;

/// A grip of the section box (one per face: 2k is the min face of axis k, 2k + 1 the max face)
/// or of a slice plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipGrip {
    Face(usize),
    Slice(usize),
}

/// A section box or slice plane being dragged: what the view had, and what it shows now.
#[derive(Debug, Clone)]
pub(crate) struct ClipEdit {
    pub grip: ClipGrip,
    pub from: (i32, i32),
    pub start: (Option<SectionBox>, Vec<SlicePlane>),
    pub now: (Option<SectionBox>, Vec<SlicePlane>),
}

/// The kind of slice plane the next click places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SliceKind {
    Horizontal,
    Vertical,
}

/// The Model Tree pane's state: the elements of each item (item data indexes `nodes`).
#[derive(Debug, Default)]
pub(crate) struct ModelTree {
    pub nodes: Vec<Vec<ElementId>>,
    /// Item keys that were expanded, kept across rebuilds.
    pub expanded: HashSet<String>,
    pub keys: Vec<String>,
    /// The node a context menu acts on.
    pub target: Option<usize>,
    /// Selection changes made by the window itself, not the user.
    pub quiet: bool,
    pub stale: bool,
}

/// A model tree item's context-menu commands.
pub(crate) const ID_TREE_FIRST: i32 = wxdragon::id::ID_HIGHEST + 120;
pub(crate) const TREE_MENU: &[(&str, &str)] = &[
    ("&Select", "select"),
    ("&Hide", "hide"),
    ("&Isolate", "isolate"),
    ("Isolate, &Ghost Others", "ghost"),
    ("&Translucent", "translucent"),
    ("Sho&w", "show"),
    ("-", ""),
    ("&Zoom To", "zoom"),
    ("Section &Box", "box"),
];

/// Metres per pixel at a point in a view `height` pixels tall.
fn metres_per_pixel(camera: &Camera, p: Vec3, height: f32) -> f32 {
    let half = if camera.perspective {
        let depth = (p - camera.eye()).dot(-camera.direction()).max(0.01);
        depth * (camera.fov * 0.5).tan()
    } else {
        camera.half_height()
    };
    2.0 * half / height.max(1.0)
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32)
}

/// The world position of a point given in a box's coordinates.
fn box_point(b: &SectionBox, c: [f64; 3]) -> Vec3 {
    let [x, y, z, o] = b.frame();
    v3(o) + v3(x) * c[0] as f32 + v3(y) * c[1] as f32 + v3(z) * c[2] as f32
}

/// The centre of face `k` of a box and its outward axis.
fn face(b: &SectionBox, k: usize) -> (Vec3, Vec3) {
    let axis = k / 2;
    let mut c = [0, 1, 2].map(|i| (b.min[i] + b.max[i]) / 2.0);
    c[axis] = if k.is_multiple_of(2) {
        b.min[axis]
    } else {
        b.max[axis]
    };
    let dir = v3(b.frame()[axis]).normalize_or_zero();
    (
        box_point(b, c),
        if k.is_multiple_of(2) { -dir } else { dir },
    )
}

/// Two unit vectors spanning the plane with normal `n`.
fn plane_basis(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = n.cross(helper).normalize_or_zero();
    (u, n.cross(u).normalize_or_zero())
}

impl Gui {
    /// The section box and slice planes a view shows: those being dragged, else its own (3D
    /// views only).
    pub(crate) fn view_clip(&self, v: &View) -> (Option<SectionBox>, Vec<SlicePlane>) {
        if v.is_2d() {
            return (None, Vec::new());
        }
        if let Some(e) = &v.clip_edit {
            return e.now.clone();
        }
        v.element
            .and_then(|e| self.app.document(v.doc)?.doc.store().get(e))
            .and_then(views::view_data)
            .map(|d| (d.section_box, d.slice_planes))
            .unwrap_or_default()
    }

    /// The temporary visibility of a view.
    pub(crate) fn view_visibility(&self, v: &View) -> Visibility {
        let mut vis = self.app.temp_state(v.doc, v.element).visibility();
        vis.virtual_volumes = self.virtual_mode(v);
        // Parts or their originals, by the view's Parts Visibility (ARC-PART-004).
        if let Some(d) = self.app.document(v.doc) {
            let store = d.doc.store();
            vis.hidden
                .extend(reviera_core::parts::hidden_in(store, v.element));
            // Hidden in the view for good (DOC-GFX-007).
            vis.hidden
                .extend(reviera_core::view_hiding::hidden_in(store, v.element));
            // A Coarse plan draws structural framing as single lines, not bodies (STR-SET-001).
            let coarse = v.plan.is_some()
                && v.element
                    .and_then(|e| store.get(e))
                    .map(reviera_core::views::detail_level)
                    == Some(reviera_core::views::DetailLevel::Coarse);
            if coarse {
                vis.hidden.extend(
                    reviera_core::beams::all(store)
                        .into_iter()
                        .map(|(id, _)| id),
                );
            }
            // Steel connections' plates, bolts and welds are drawn at Fine detail only
            // (STR-STL-001).
            let fine = v.element
                .and_then(|e| store.get(e))
                .map(reviera_core::views::detail_level)
                == Some(reviera_core::views::DetailLevel::Fine);
            if !fine {
                vis.hidden.extend(
                    store
                        .iter()
                        .filter(|e| reviera_core::steel_fab::fab_recipe(e).is_some() || reviera_core::steel_detail::is_detail(e))
                        .map(|e| e.id()),
                );
            }
        }
        vis
    }

    /// How a view shows virtual volumes (ARC-MOD-031), from the view's setting.
    pub(crate) fn virtual_mode(&self, v: &View) -> reviera_render::VirtualMode {
        let mode = v.element.and_then(|e| {
            let store = self.app.document(v.doc)?.doc.store();
            reviera_core::clearances::view_mode(store, e)
        });
        match mode.as_deref() {
            Some("translucent") => reviera_render::VirtualMode::Translucent,
            Some("solid") => reviera_render::VirtualMode::Solid,
            _ => reviera_render::VirtualMode::Hidden,
        }
    }

    /// Hands a view's clipping, visibility and box and slice decorations to its viewport.
    pub(crate) fn update_inspection(&mut self, index: usize) {
        let v = &self.views[index];
        let (section_box, slices) = self.view_clip(v);
        let mut clipping = Clipping::of(section_box.as_ref(), &slices);
        // A cropped plan or section clips its shaded model to the crop region (DOC-GFX-015), in
        // the view's own coordinates (a section's scene is in its frame).
        if (v.plan.is_some() || v.section.is_some())
            && let Some(c) = v.element.and_then(|e| {
                let st = self.app.document(v.doc)?.doc.store();
                reviera_core::crop::rects(st, st.get(e)?)
            })
        {
            let (lo, hi) = (c.model.0.map(|x| x as f32), c.model.1.map(|x| x as f32));
            clipping.planes.extend([[1.0, 0.0, 0.0, lo[0]], [-1.0, 0.0, 0.0, -hi[0]], [0.0, 1.0, 0.0, lo[1]], [0.0, -1.0, 0.0, -hi[1]]]);
            clipping.caps = false;
        }
        let mut visibility = self.view_visibility(v);
        let mut lines = self.clip_lines(v, section_box.as_ref(), &slices);
        // Rebar as this view presents it (STR-RBR-017).
        let (rebar_lines, mut rebar_top, rebar_picks) = self.rebar_view_drawing(index, &mut visibility);
        rebar_top.extend(self.constraint_lines.iter().cloned());
        // MEP connector glyphs and check markers (Ch 4 §4.6), drawn through the model.
        rebar_top.extend(self.mep_lines(index));
        lines.extend(rebar_lines);
        lines.extend(self.datum_3d_lines(index));
        lines.extend(self.analytical_lines(index));
        if !self.views[index].is_2d() {
            lines.extend(self.check_marks());
        }
        lines.extend(self.results_lines(index));
        lines.extend(self.adjust_lines(index));
        lines.extend(self.automation_lines(index));
        lines.extend(self.work_plane_grid(index));
        let v = &mut self.views[index];
        v.rebar_picks = rebar_picks;
        if let Some(vp) = &mut v.viewport {
            vp.set_clipping(&clipping);
            vp.set_visibility(&visibility);
            vp.set_view_lines(&lines);
            vp.set_view_lines_on_top(&rebar_top);
        }
    }

    /// Outline and grips of the section box, and outline, arrow and grip of each slice plane.
    fn clip_lines(&self, v: &View, b: Option<&SectionBox>, slices: &[SlicePlane]) -> Vec<Lines> {
        let height = v.panel.get_client_size().height as f32;
        let cam = &v.camera;
        let (right, up) = (cam.right(), cam.up());
        // A small square facing the camera around `p`, and an arrow along `dir`.
        let grip = |p: Vec3, dir: Vec3, out: &mut Vec<[[f32; 3]; 2]>| {
            let m = metres_per_pixel(cam, p, height);
            let (r, u) = (right * 5.0 * m, up * 5.0 * m);
            let corners = [p - r - u, p + r - u, p + r + u, p - r + u];
            for k in 0..4 {
                out.push([corners[k].to_array(), corners[(k + 1) % 4].to_array()]);
            }
            let tip = p + dir * 24.0 * m;
            out.push([p.to_array(), tip.to_array()]);
        };
        let mut out = Vec::new();
        if let Some(b) = b.filter(|b| b.enabled) {
            let mut segments = Vec::new();
            let corner = |i: usize| {
                box_point(
                    b,
                    [0, 1, 2].map(|k| if i >> k & 1 == 1 { b.max[k] } else { b.min[k] }),
                )
            };
            for i in 0..8usize {
                for k in 0..3 {
                    if i >> k & 1 == 0 {
                        segments.push([corner(i).to_array(), corner(i | 1 << k).to_array()]);
                    }
                }
            }
            for k in 0..6 {
                let (p, dir) = face(b, k);
                grip(p, dir, &mut segments);
            }
            out.push(Lines {
                color: BOX_COLOR,
                width: 0.35,
                segments,
            });
        }
        let model = self.scenes.get(&v.doc).and_then(|s| s.bounds());
        for s in slices.iter().filter(|s| s.enabled) {
            let (o, n) = (v3(s.origin), v3(s.normal));
            let (u, w) = plane_basis(n);
            let (centre, half) = match model {
                Some((lo, hi)) => {
                    let c = (lo + hi) * 0.5;
                    (c - n * (c - o).dot(n), (hi - lo).length() * 0.55)
                }
                None => (o, 10.0),
            };
            let mut segments = Vec::new();
            let corners = [
                centre - u * half - w * half,
                centre + u * half - w * half,
                centre + u * half + w * half,
                centre - u * half + w * half,
            ];
            for k in 0..4 {
                segments.push([corners[k].to_array(), corners[(k + 1) % 4].to_array()]);
            }
            // The arrow points to the side that is removed.
            grip(o, -n, &mut segments);
            out.push(Lines {
                color: SLICE_COLOR,
                width: 0.35,
                segments,
            });
        }
        out
    }

    /// The section box or slice plane grip under pixel (`x`, `y`) of the active 3D view.
    pub(crate) fn clip_grip_at(&self, x: i32, y: i32) -> Option<ClipGrip> {
        let v = self.active_view()?;
        if v.is_2d() || v.element.is_none() {
            return None;
        }
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let (b, slices) = self.view_clip(v);
        let mut grips: Vec<(ClipGrip, Vec3)> = Vec::new();
        if let Some(b) = b.filter(|b| b.enabled) {
            grips.extend((0..6).map(|k| (ClipGrip::Face(k), face(&b, k).0)));
        }
        grips.extend(
            slices
                .iter()
                .enumerate()
                .filter(|(_, s)| s.enabled)
                .map(|(i, s)| (ClipGrip::Slice(i), v3(s.origin))),
        );
        grips
            .into_iter()
            .filter_map(|(g, p)| {
                let (px, py) = v.camera.project(p, w, h)?;
                let d = (px - x as f32).hypot(py - y as f32);
                (d <= GRIP_REACH).then_some((g, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// Starts dragging a grip.
    pub(crate) fn clip_drag_start(&mut self, grip: ClipGrip, x: i32, y: i32) {
        let Some(v) = self.active_view() else { return };
        let start = self.view_clip(v);
        if let Some(v) = self.active_view_mut() {
            v.clip_edit = Some(ClipEdit {
                grip,
                from: (x, y),
                start: start.clone(),
                now: start,
            });
        }
    }

    /// Moves the dragged face or plane with the pointer, along its axis.
    pub(crate) fn clip_drag_motion(&mut self, x: i32, y: i32) {
        let Some(v) = self.active_view_mut() else {
            return;
        };
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let cam = v.camera;
        let Some(edit) = &mut v.clip_edit else { return };
        let (anchor, axis) = match edit.grip {
            ClipGrip::Face(k) => match &edit.start.0 {
                Some(b) => {
                    let (p, out) = face(b, k);
                    // Movement is measured along the axis the coordinate grows on.
                    (p, if k.is_multiple_of(2) { -out } else { out })
                }
                None => return,
            },
            ClipGrip::Slice(i) => match edit.start.1.get(i) {
                Some(s) => (v3(s.origin), v3(s.normal)),
                None => return,
            },
        };
        let m = metres_per_pixel(&cam, anchor, h) * 100.0;
        let (Some(a), Some(b)) = (
            cam.project(anchor, w, h),
            cam.project(anchor + axis * m, w, h),
        ) else {
            return;
        };
        let per_metre = glam::Vec2::new(b.0 - a.0, b.1 - a.1) / m;
        if per_metre.length_squared() < 1e-6 {
            return;
        }
        let moved = glam::Vec2::new((x - edit.from.0) as f32, (y - edit.from.1) as f32);
        let delta = f64::from(moved.dot(per_metre) / per_metre.length_squared());
        let mut now = edit.start.clone();
        match edit.grip {
            ClipGrip::Face(k) => {
                if let Some(b) = &mut now.0 {
                    let i = k / 2;
                    if k.is_multiple_of(2) {
                        b.min[i] = (b.min[i] + delta).min(b.max[i] - MIN_BOX);
                    } else {
                        b.max[i] = (b.max[i] + delta).max(b.min[i] + MIN_BOX);
                    }
                }
            }
            ClipGrip::Slice(i) => {
                if let Some(s) = now.1.get_mut(i) {
                    s.origin = [0, 1, 2].map(|k| s.origin[k] + s.normal[k] * delta);
                }
            }
        }
        edit.now = now;
    }

    /// Ends a grip drag: the new box or planes become one undoable edit.
    pub(crate) fn clip_drag_end(&mut self, click: bool) {
        let Some(v) = self.active_view_mut() else {
            return;
        };
        let Some(edit) = v.clip_edit.take() else {
            return;
        };
        let Some(view) = v.element else { return };
        if click {
            return self.refresh_active();
        }
        let result = match (edit.grip, &edit.now.0) {
            (ClipGrip::Face(_), Some(b)) => self.run(
                "view.set_section_box",
                json!({
                    "view_id": view, "enabled": b.enabled, "transform": b.frame(),
                    "min": b.min, "max": b.max,
                }),
            ),
            _ => self.run(
                "view.set_slice_planes",
                json!({ "view_id": view, "planes": edit.now.1 }),
            ),
        };
        if let Err(e) = result {
            self.frame.set_status_text(&e, 0);
        }
        self.after_view_edit();
    }

    /// The typed extents under a 3D view's Section Box and Slice Planes rows: the box's six faces
    /// (in its own axes) and each slice's position (along the axis it faces, or its normal).
    pub(crate) fn clip_rows(&mut self, grid: &PropertyGrid, section_box: Option<SectionBox>, slices: &[SlicePlane]) {
        if let Some(b) = section_box.filter(|b| b.enabled) {
            for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
                for (end, side, v) in [("min", "From", b.min[k]), ("max", "To", b.max[k])] {
                    let key = format!("clip_box_{end}_{k}");
                    let text = self.format_length(v);
                    grid.append(Property::string(format!("Box {axis} {side}"), &key, text));
                    grid.set_help_string(&key, "A face of the section box, in the box's own axes");
                }
            }
        }
        for (i, s) in slices.iter().enumerate() {
            let (label, v) = slice_position(s);
            let key = format!("clip_slice_{i}");
            let text = self.format_length(v);
            grid.append(Property::string(format!("Slice {} {label}", i + 1), &key, text));
            grid.set_help_string(&key, "Where the slice plane is: type a position to move it");
            let key = format!("clip_on_{i}");
            grid.append(Property::boolean(format!("Slice {} Enabled", i + 1), &key, s.enabled));
            grid.set_help_string(&key, "Untick to keep the slice plane but stop it cutting the view");
        }
    }

    /// An edit of a typed box face or slice position; `false` if `property` is not one.
    pub(crate) fn clip_row_edit(&mut self, property: &str, text: Option<&str>) -> bool {
        let Some(rest) = property.strip_prefix("clip_") else { return false };
        if let Some(i) = rest.strip_prefix("on_").and_then(|i| i.parse::<usize>().ok()) {
            let Some(view) = self.active_view().filter(|v| !v.is_2d()).and_then(|v| v.element) else { return true };
            let (_, mut slices) = self.view_clip(self.active_view().expect("checked"));
            let Some(s) = slices.get_mut(i) else { return true };
            s.enabled = text.is_some_and(|t| matches!(t, "True" | "true" | "1"));
            if let Err(e) = self.run("view.set_slice_planes", json!({ "view_id": view, "planes": slices })) {
                self.frame.set_status_text(&e, 0);
            }
            self.after_view_edit();
            return true;
        }
        let (Some(view), Some(text)) = (self.active_view().filter(|v| !v.is_2d()).and_then(|v| v.element), text) else { return true };
        let at = match self.run("units.parse", json!({ "text": text, "data_type": "length" })) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(e) => {
                super::error(&self.frame, &format!("Invalid length:\n{e}"));
                self.properties_stale = true;
                return true;
            }
        };
        let (b, mut slices) = self.view_clip(self.active_view().expect("checked"));
        let result = if let Some(face) = rest.strip_prefix("box_") {
            let Some(mut b) = b else { return true };
            let (end, k) = face.split_once('_').unwrap_or_default();
            let k: usize = k.parse().unwrap_or(0).min(2);
            if end == "min" {
                b.min[k] = at.min(b.max[k] - MIN_BOX);
            } else {
                b.max[k] = at.max(b.min[k] + MIN_BOX);
            }
            self.run("view.set_section_box", json!({ "view_id": view, "enabled": b.enabled, "transform": b.frame(), "min": b.min, "max": b.max }))
        } else if let Some(i) = rest.strip_prefix("slice_").and_then(|i| i.parse::<usize>().ok()).filter(|i| *i < slices.len()) {
            move_slice(&mut slices[i], at);
            self.run("view.set_slice_planes", json!({ "view_id": view, "planes": slices }))
        } else {
            return true;
        };
        if let Err(e) = result {
            self.frame.set_status_text(&e, 0);
        }
        self.after_view_edit();
        true
    }

    /// After a change to a view's definition: captions (modified mark), palette and redraw.
    fn after_view_edit(&mut self) {
        self.update_captions();
        self.properties_stale = true;
        self.refresh_active();
    }

    /// The active 3D view's model view, saving the unsaved `{3D}` view first. Plans give `None`
    /// with a message.
    fn active_3d(&mut self) -> Option<ElementId> {
        let v = self.active_view()?;
        if v.is_2d() {
            self.frame.set_status_text(
                "Section boxes and slice planes belong to 3D views: open a 3D view first",
                0,
            );
            return None;
        }
        if v.element.is_none() {
            self.default_3d_view();
        }
        self.active_view()?.element
    }

    /// View › Section Box: turns the active 3D view's box on (around the model the first time)
    /// or off.
    pub(crate) fn toggle_section_box(&mut self) {
        let Some(view) = self.active_3d() else { return };
        let Some(v) = self.active_view() else { return };
        let (current, _) = self.view_clip(v);
        let args = match current {
            Some(b) => json!({ "view_id": view, "enabled": !b.enabled }),
            None => {
                let Some((lo, hi)) = self.scene().bounds() else {
                    return self
                        .frame
                        .set_status_text("The model has no geometry to box", 0);
                };
                let pad = 0.3;
                json!({
                    "view_id": view, "enabled": true,
                    "min": (lo - Vec3::splat(pad)).to_array(),
                    "max": (hi + Vec3::splat(pad)).to_array(),
                })
            }
        };
        match self.run("view.set_section_box", args) {
            Ok(_) => {
                let on = !current.is_some_and(|b| b.enabled);
                self.frame.set_status_text(
                    if on {
                        "Section box on: drag its orange grips to resize it"
                    } else {
                        "Section box off"
                    },
                    0,
                );
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.after_view_edit();
    }

    /// View › Selection Box: the section box around the selection (BX).
    pub(crate) fn selection_box(&mut self) {
        let ids = self.selection();
        if ids.is_empty() {
            return self
                .frame
                .set_status_text("Select the elements to box first", 0);
        }
        if self.in_plan() || self.active_view().is_some_and(|v| v.element.is_none()) {
            // The command picks the default 3D view; save an unsaved one first.
            if !self.in_plan() {
                self.default_3d_view();
            }
        }
        match self.run("view.fit_section_box", json!({ "element_ids": ids })) {
            Ok(v) => {
                let view = ElementId(v.as_u64().unwrap_or_default());
                if self.active_view().and_then(|v| v.element) != Some(view)
                    && let Some(doc) = self.doc
                {
                    self.open_view(doc, Some(view));
                }
                self.frame
                    .set_status_text("Section box fitted to the selection", 0);
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.browser_stale = true;
        self.after_view_edit();
    }

    /// View › Horizontal / Vertical Slice: the next click in the 3D view places the plane.
    pub(crate) fn start_slice(&mut self, kind: SliceKind) {
        if self.active_3d().is_none() {
            return;
        }
        self.slice_pick = Some(kind);
        self.frame.set_status_text(
            match kind {
                SliceKind::Horizontal => {
                    "Click a point: the model above its height is cut away (Esc cancels)"
                }
                SliceKind::Vertical => {
                    "Click a point: the model between you and it is cut away (Esc cancels)"
                }
            },
            0,
        );
    }

    /// Places the slice plane of a slice tool through the point under (`x`, `y`).
    pub(crate) fn slice_click(&mut self, x: i32, y: i32) {
        let Some(kind) = self.slice_pick.take() else {
            return;
        };
        let Some(v) = self.active_view() else { return };
        let Some(view) = v.element else { return };
        let size = v.panel.get_client_size();
        let cam = v.camera;
        let (origin, dir) = cam.ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (b, mut slices) = self.view_clip(v);
        let clipping = Clipping::of(b.as_ref(), &slices);
        let visibility = self.view_visibility(v);
        let hit = self
            .scene()
            .pick_visible(origin, dir, &|p| clipping.keeps(p), &visibility)
            .map(|(_, t)| origin + dir * t);
        let point = hit.unwrap_or_else(|| {
            // The plane through the target, facing the camera.
            let n = cam.direction();
            let t = (cam.target - origin).dot(n) / dir.dot(n).min(-1e-6);
            origin + dir * t
        });
        let normal = match kind {
            SliceKind::Horizontal => Vec3::NEG_Z,
            SliceKind::Vertical => {
                // Away from the camera, level, and square to the model axes within 10°.
                let d = -cam.direction();
                let d = Vec3::new(d.x, d.y, 0.0).normalize_or(Vec3::X);
                let axes = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y];
                axes.into_iter()
                    .find(|a| a.dot(d) > 10f32.to_radians().cos())
                    .unwrap_or(d)
            }
        };
        if slices.len() >= views::MAX_SLICES {
            return self.frame.set_status_text(
                "This view already has eight slice planes: clear some first",
                0,
            );
        }
        slices.push(SlicePlane {
            origin: point.to_array().map(f64::from),
            normal: normal.to_array().map(f64::from),
            enabled: true,
        });
        match self.run(
            "view.set_slice_planes",
            json!({ "view_id": view, "planes": slices }),
        ) {
            Ok(_) => self.frame.set_status_text(
                "Slice placed: drag its grip to move it; Clear Slices removes it",
                0,
            ),
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.after_view_edit();
    }

    pub(crate) fn clear_slices(&mut self) {
        let Some(view) = self.active_3d() else { return };
        if let Err(e) = self.run(
            "view.set_slice_planes",
            json!({ "view_id": view, "planes": [] }),
        ) {
            self.frame.set_status_text(&e, 0);
        }
        self.after_view_edit();
    }

    /// Runs a temporary visibility command (`view.temp_*`) on `ids` in the active view.
    pub(crate) fn temp(&mut self, command: &str, ids: &[ElementId], extra: Value) {
        let Some(v) = self.active_view() else { return };
        if ids.is_empty() {
            return self
                .frame
                .set_status_text("Select the elements to hide, isolate or show first", 0);
        }
        let mut args = json!({ "element_ids": ids });
        if let Some(e) = v.element {
            args["view_id"] = json!(e);
        }
        if let (Value::Object(a), Value::Object(x)) = (&mut args, extra) {
            a.extend(x);
        }
        match self.run(command, args) {
            Ok(r) => {
                let n = r["count"].as_u64().unwrap_or(0);
                self.frame.set_status_text(
                    &format!(
                        "Temporary Hide/Isolate: {n} element{} (Reset Hide/Isolate, HR, shows all)",
                        if n == 1 { "" } else { "s" }
                    ),
                    0,
                );
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.after_temp_change();
    }

    /// Runs a ribbon or menu temporary visibility action on the selection.
    pub(crate) fn temp_action(&mut self, action: &str) {
        let ids = self.selection();
        let (command, by, extra) = match action {
            "inspect.hide" => ("view.temp_hide", "element", json!({})),
            "inspect.hide_subtree" => ("view.temp_hide", "subtree", json!({})),
            "inspect.hide_category" => ("view.temp_hide", "category", json!({})),
            "inspect.hide_type" => ("view.temp_hide", "type", json!({})),
            "inspect.isolate" => ("view.temp_isolate", "element", json!({})),
            "inspect.isolate_subtree" => ("view.temp_isolate", "subtree", json!({})),
            "inspect.isolate_category" => ("view.temp_isolate", "category", json!({})),
            "inspect.isolate_type" => ("view.temp_isolate", "type", json!({})),
            "inspect.ghost_others" => (
                "view.temp_isolate",
                "element",
                json!({ "ghost_others": true }),
            ),
            "inspect.translucent" => ("view.temp_translucent", "element", json!({})),
            "inspect.translucent_subtree" => ("view.temp_translucent", "subtree", json!({})),
            _ => return,
        };
        let mut extra = extra;
        extra["by"] = json!(by);
        self.temp(command, &ids, extra);
        // Hidden elements cannot stay selected.
        if command == "view.temp_hide" {
            self.clear_selection();
        }
    }

    /// Hide in View (EH), or Hide Category in View (VH): the selection hidden in the active view
    /// for good.
    pub(crate) fn hide_in_view(&mut self, by_category: bool) {
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return self.frame.set_status_text("Hide in View: open a saved view (the default 3D view keeps no hiding)", 0);
        };
        let ids = self.selection();
        if ids.is_empty() {
            return self.frame.set_status_text("Hide in View: select the elements to hide first", 0);
        }
        match self.run("view.hide", json!({ "view_id": view, "element_ids": ids, "by_category": by_category })) {
            Ok(_) => self.frame.set_status_text("Hidden in this view (Unhide All in View shows them again)", 0),
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.clear_selection();
        self.after_temp_change();
        self.after_view_edit();
    }

    /// Unhide All in View: everything the active view hides shown again.
    pub(crate) fn unhide_all_in_view(&mut self) {
        let Some(view) = self.active_view().and_then(|v| v.element) else { return };
        if let Err(e) = self.run("view.unhide", json!({ "view_id": view, "all": true })) {
            self.frame.set_status_text(&e, 0);
        }
        self.after_temp_change();
        self.after_view_edit();
    }

    /// Apply Hide/Isolate to View: the temporary hide and isolate made permanent.
    pub(crate) fn apply_temp_to_view(&mut self) {
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return self.frame.set_status_text("Apply Hide/Isolate to View: open a saved view first", 0);
        };
        match self.run("view.temp_apply", json!({ "view_id": view })) {
            Ok(r) => {
                let n = r["hidden"].as_u64().unwrap_or(0);
                let kept = if r["kept_translucency"] == true { "; translucency stays temporary" } else { "" };
                self.frame.set_status_text(&format!("Hidden in this view for good: {n} element{}{kept}", if n == 1 { "" } else { "s" }), 0);
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
        self.after_temp_change();
        self.after_view_edit();
    }

    pub(crate) fn temp_reset(&mut self) {
        let Some(v) = self.active_view() else { return };
        let args = match v.element {
            Some(e) => json!({ "view_id": e }),
            None => json!({}),
        };
        if let Err(e) = self.run("view.temp_reset", args) {
            self.frame.set_status_text(&e, 0);
        } else {
            self.frame
                .set_status_text("Temporary Hide/Isolate reset", 0);
        }
        self.after_temp_change();
    }

    /// After temporary visibility changed: plans redraw, captions mark the view, the tree
    /// restyles.
    fn after_temp_change(&mut self) {
        if let Some(id) = self.active
            && let Some(v) = self.views.iter_mut().find(|v| v.id == id)
        {
            if let Some(p) = &mut v.plan {
                p.stale = true;
            }
            if let Some(s) = &mut v.section {
                s.stale = true;
            }
        }
        self.tree.stale = true;
        self.update_captions();
        self.refresh_active();
    }

    /// Whether the active view has temporary state (its tab is marked).
    pub(crate) fn temp_mark(&self, v: &View) -> &'static str {
        if self.app.temp_active(v.doc, v.element) {
            " [Hide/Isolate]"
        } else {
            ""
        }
    }

    // ---- Model Tree ----

    /// Rebuilds the Model Tree for the active model and view, keeping expanded items.
    pub(crate) fn rebuild_model_tree(&mut self) {
        self.tree.stale = false;
        let t = self.model_tree;
        // Remember what was expanded.
        let mut expanded = std::mem::take(&mut self.tree.expanded);
        if let Some(root) = t.get_root_item() {
            let mut stack = vec![root];
            while let Some(item) = stack.pop() {
                if let Some(i) = t
                    .get_custom_data(&item)
                    .and_then(|d| d.downcast_ref::<u64>().copied())
                    && let Some(key) = self.tree.keys.get(i as usize)
                {
                    if t.is_expanded(&item) {
                        expanded.insert(key.clone());
                    } else {
                        expanded.remove(key);
                    }
                }
                if let Some((child, mut cookie)) = t.get_first_child(&item) {
                    stack.push(child);
                    while let Some(next) = t.get_next_child(&item, &mut cookie) {
                        stack.push(next);
                    }
                }
            }
        }
        self.tree.quiet = true;
        t.cleanup_custom_data();
        t.delete_all_items();
        self.tree.nodes.clear();
        self.tree.keys.clear();
        self.tree.expanded = expanded;
        let Some(doc) = self.doc else {
            self.tree.quiet = false;
            return;
        };
        let Ok(tree) = self.app.model_tree(doc) else {
            self.tree.quiet = false;
            return;
        };
        let state = self
            .active_view()
            .map(|v| self.app.temp_state(v.doc, v.element))
            .unwrap_or_default();
        let Some(root) = t.add_root(&tree.label, None, None) else {
            self.tree.quiet = false;
            return;
        };
        let index = self.tree_node_data(&tree, "root");
        t.set_custom_data_direct(&root, index);
        self.add_tree_children(&root, &tree, &state, "root");
        t.expand(&root);
        self.tree.quiet = false;
    }

    fn tree_node_data(&mut self, node: &TreeNode, key: &str) -> u64 {
        self.tree.nodes.push(node.elements());
        self.tree.keys.push(key.to_owned());
        (self.tree.nodes.len() - 1) as u64
    }

    fn add_tree_children(
        &mut self,
        parent: &TreeItemId,
        node: &TreeNode,
        state: &reviera_app::inspect::TempState,
        parent_key: &str,
    ) {
        let t = self.model_tree;
        for child in &node.children {
            let key = match child.id {
                Some(id) => format!("e{}", id.0),
                None => format!(
                    "{parent_key}/{}",
                    child.label.split(" (").next().unwrap_or(&child.label)
                ),
            };
            let mut label = child.label.clone();
            if let Some(id) = child.id
                && state.translucent.contains_key(&id)
            {
                label.push_str(" (translucent)");
            }
            let index = self.tree_node_data(child, &key);
            let Some(item) = t.append_item_with_data(parent, &label, index, None, None) else {
                continue;
            };
            if let Some(id) = child.id {
                let isolated = state
                    .isolated
                    .as_ref()
                    .is_some_and(|i| i.elements.contains(&id));
                if isolated {
                    t.set_item_bold(&item, true);
                }
                let hidden = state.hidden.contains(&id)
                    || state.isolated.as_ref().is_some_and(|i| {
                        !i.elements.contains(&id) && i.ghost_others.is_none() && id < i.after
                    });
                if hidden {
                    t.set_item_text_colour(&item, Colour::new(140, 140, 140, 255));
                }
            }
            self.add_tree_children(&item, child, state, &key);
            let spatial = matches!(
                child.kind,
                NodeKind::Project | NodeKind::Site | NodeKind::Building
            );
            if spatial || self.tree.expanded.contains(&key) {
                t.expand(&item);
            }
        }
    }

    /// The elements of a Model Tree item.
    pub(crate) fn tree_elements(&self, item: &TreeItemId) -> Option<usize> {
        self.model_tree
            .get_custom_data(item)
            .and_then(|d| d.downcast_ref::<u64>().copied())
            .map(|i| i as usize)
            .filter(|i| *i < self.tree.nodes.len())
    }

    /// Selecting an item selects its elements in the canvas.
    pub(crate) fn tree_selected(&mut self, node: usize) {
        if self.tree.quiet {
            return;
        }
        let ids = self.tree.nodes[node].clone();
        if self.run("selection.set", json!({ "ids": ids })).is_ok() {
            self.selection_changed();
            self.refresh_active();
        }
    }

    /// A Model Tree context-menu command on the target item.
    pub(crate) fn tree_command(&mut self, what: &str) {
        let Some(node) = self.tree.target.take() else {
            return;
        };
        let ids = self.tree.nodes.get(node).cloned().unwrap_or_default();
        match what {
            "hide" => return self.temp("view.temp_hide", &ids, json!({})),
            "isolate" => return self.temp("view.temp_isolate", &ids, json!({})),
            "ghost" => {
                return self.temp("view.temp_isolate", &ids, json!({ "ghost_others": true }));
            }
            "translucent" => return self.temp("view.temp_translucent", &ids, json!({})),
            "show" => return self.temp("view.temp_show", &ids, json!({})),
            _ => {}
        }
        // Select, then zoom to or box the selection.
        if self.run("selection.set", json!({ "ids": ids })).is_err() {
            return;
        }
        self.selection_changed();
        match what {
            "zoom" => self.zoom_to_fit(),
            "box" => self.selection_box(),
            _ => self.refresh_active(),
        }
    }
}

/// The axis a slice plane faces (X, Y or Z) and its origin's coordinate on it; or, for a plane
/// turned off the axes, its distance from the origin along its normal.
fn slice_position(s: &SlicePlane) -> (&'static str, f64) {
    let len = s.normal.iter().map(|n| n * n).sum::<f64>().sqrt().max(1e-12);
    match s.normal.iter().position(|n| (n / len).abs() > 0.9999) {
        Some(k) => (["(X)", "(Y)", "(Z)"][k], s.origin[k]),
        None => ("(along normal)", s.origin.iter().zip(s.normal).map(|(o, n)| o * n).sum::<f64>() / len),
    }
}

/// Moves a slice plane, along its normal, to position `at` (as `slice_position` gives it).
fn move_slice(s: &mut SlicePlane, at: f64) {
    let (_, now) = slice_position(s);
    let len = s.normal.iter().map(|n| n * n).sum::<f64>().sqrt().max(1e-12);
    // Along an axis the coordinate is the origin's (the normal may point either way).
    let step = match s.normal.iter().position(|n| (n / len).abs() > 0.9999) {
        Some(k) => (at - now) * s.normal[k].signum(),
        None => at - now,
    };
    for k in 0..3 {
        s.origin[k] += s.normal[k] / len * step;
    }
}

#[cfg(test)]
mod typed_clip_tests {
    use super::*;

    #[test]
    fn typing_a_slice_position_moves_it_along_its_normal() {
        let mut down = SlicePlane { origin: [1.0, 2.0, 3.0], normal: [0.0, 0.0, -1.0], enabled: true };
        assert_eq!(slice_position(&down), ("(Z)", 3.0));
        move_slice(&mut down, 1.5);
        assert_eq!(down.origin, [1.0, 2.0, 1.5]);
        let mut turned = SlicePlane { origin: [0.0, 0.0, 0.0], normal: [0.6, 0.8, 0.0], enabled: true };
        move_slice(&mut turned, 2.0);
        assert!((slice_position(&turned).1 - 2.0).abs() < 1e-12, "{turned:?}");
    }
}
