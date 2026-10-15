// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reference planes (ARC-REFP-001), model lines (ARC-MLIN-001) and work planes (ARC-WP-001) in
//! the window.
//!
//! - **Ref Plane** (`RP`) in a plan: click the two ends of the line; the plane is vertical
//!   through it. Its Name is set in Properties while the tool runs (or afterwards).
//! - **Model Line** (`LI`) in a plan: on the view's work plane (its level unless one is set).
//!   Click points; with *Chain* each line starts where the last ended; *Arc* takes a third click
//!   on the arc. Line Style is chosen in Properties. Esc ends the chain, then the tool.
//! - **Work Plane › Set** chooses a view's work plane by name (levels, grids, named reference
//!   planes); **Show** toggles its grid.
//! - Plans draw both (reference planes dashed green), sections and elevations draw what they
//!   look along, 3D views draw model lines; they are picked in plans by their lines.

use std::collections::HashSet;

use reviera_core::ElementId;
use reviera_core::model_lines::{self, STYLES};
use reviera_core::work_planes::{Plane, WorkPlaneRef};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, snap};

const PREVIEW_COLOR: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
/// Pick distance in pixels.
const PICK_PX: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMode {
    RefPlane,
    ModelLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Line,
    /// Start, end, then a point on the arc.
    Arc,
}

const SHAPES: [(Shape, &str); 2] = [(Shape::Line, "Line"), (Shape::Arc, "Arc")];

/// The Ref Plane and Model Line tools.
#[derive(Debug, Clone)]
pub struct LineTool {
    mode: LineMode,
    shape: Shape,
    chain: bool,
    style: String,
    /// The name the next reference plane gets.
    name: String,
    points: Vec<[f64; 2]>,
    cursor: Option<snap::Snap>,
    /// Lines to snap to: grids, walls, reference planes, model lines and cut edges.
    lines: Vec<([f64; 2], [f64; 2])>,
}

fn choices<T>(table: &[(T, &'static str)]) -> Vec<(String, i32)> {
    table
        .iter()
        .enumerate()
        .map(|(i, x)| (x.1.to_owned(), i as i32))
        .collect()
}

fn style_choices() -> Vec<(String, i32)> {
    STYLES
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.to_owned(), i as i32))
        .collect()
}

fn style_index(name: &str) -> i32 {
    STYLES.iter().position(|s| s.name == name).unwrap_or(0) as i32
}

/// The command arguments naming a work plane.
pub(crate) fn plane_args(r: &WorkPlaneRef) -> Value {
    match r {
        WorkPlaneRef::Level { level, offset } => json!({ "level": level, "offset": offset }),
        WorkPlaneRef::Grid { grid } => json!({ "grid": grid }),
        WorkPlaneRef::RefPlane { ref_plane } => json!({ "ref_plane": ref_plane }),
        WorkPlaneRef::Explicit {
            origin,
            x_dir,
            normal,
        } => json!({ "plane": { "origin": origin, "x_dir": x_dir, "normal": normal } }),
    }
}

impl Gui {
    /// Starts the Ref Plane or Model Line tool (plans).
    pub(crate) fn start_line_tool(&mut self, mode: LineMode) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        let style = self.line_tool.as_ref().map_or_else(
            || model_lines::DEFAULT_STYLE.to_owned(),
            |t| t.style.clone(),
        );
        self.line_tool = Some(LineTool {
            mode,
            shape: Shape::Line,
            chain: mode == LineMode::ModelLine,
            style,
            name: String::new(),
            points: Vec::new(),
            cursor: None,
            lines: Vec::new(),
        });
        self.update_line_snaps();
        self.clear_selection();
        self.properties_stale = true;
        self.line_prompt();
    }

    pub(crate) fn end_line_tool(&mut self) -> bool {
        if self.line_tool.take().is_none() {
            return false;
        }
        self.properties_stale = true;
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Ready", 0);
        true
    }

    /// Esc: drops the points so far, then ends the tool.
    pub(crate) fn line_escape(&mut self) {
        match &mut self.line_tool {
            Some(t) if !t.points.is_empty() => {
                t.points.clear();
                self.line_preview();
                self.line_prompt();
            }
            Some(_) => {
                self.end_line_tool();
            }
            None => {}
        }
    }

    fn update_line_snaps(&mut self) {
        let Some(store) = self.store_now() else {
            return;
        };
        let mut lines: Vec<([f64; 2], [f64; 2])> = reviera_core::grids::list(&store)
            .into_iter()
            .map(|g| (g.curve.start, g.curve.end))
            .collect();
        lines.extend(
            store
                .iter()
                .filter_map(reviera_core::walls::wall_recipe)
                .map(|r| (r.curve.start, r.curve.end)),
        );
        for r in reviera_core::ref_planes::list(&store) {
            lines.push(([r.start[0], r.start[1]], [r.end[0], r.end[1]]));
        }
        for l in model_lines::all(&store) {
            for w in l.points.windows(2) {
                lines.push(([w[0][0], w[0][1]], [w[1][0], w[1][1]]));
            }
        }
        if let Some(plan) = self.active_view().and_then(|v| v.plan.as_ref()) {
            let p = |q: [f32; 2]| [f64::from(q[0]), f64::from(q[1])];
            lines.extend(plan.cut_edges.iter().map(|(_, s)| (p(s[0]), p(s[1]))));
        }
        if let Some(t) = &mut self.line_tool {
            t.lines = lines;
        }
    }

    fn line_prompt(&mut self) {
        let plane = self.view_work_plane().map(|w| w.0);
        let Some(t) = &self.line_tool else { return };
        let what = match t.mode {
            LineMode::RefPlane => "Reference Plane",
            LineMode::ModelLine => "Model Line",
        };
        let mut text = match (t.points.len(), t.shape) {
            (0, _) => format!("{what}: click the start point"),
            (1, Shape::Line) => "Click the end point. Esc restarts.".to_owned(),
            (1, Shape::Arc) => "Click the arc's end point. Esc restarts.".to_owned(),
            _ => "Click a point on the arc. Esc restarts.".to_owned(),
        };
        if t.mode == LineMode::ModelLine
            && let Some(wp) = plane
        {
            text += &format!(" · on {wp}");
        }
        if let Some(c) = t.cursor
            && c.kind != snap::SnapKind::None
        {
            text += &format!(" · {}", c.kind.label());
        }
        self.frame.set_status_text(&text, 0);
    }

    /// The active view's work plane: its label, reference and resolved plane.
    pub(crate) fn view_work_plane(&mut self) -> Option<(String, WorkPlaneRef, Plane)> {
        let view = self.active_view()?.element?;
        let wp = self.run("workplane.get", json!({ "view_id": view })).ok()?;
        let plane: WorkPlaneRef = serde_json::from_value(wp["plane"].clone()).ok()?;
        let resolved: Plane = serde_json::from_value(wp["resolved"].clone()).ok()?;
        Some((wp["label"].as_str()?.to_owned(), plane, resolved))
    }

    fn line_snapped(&self, x: i32, y: i32) -> Option<snap::Snap> {
        let t = self.line_tool.as_ref()?;
        let v = self.active_view()?;
        let z = v.plan.as_ref()?.range.cut as f32;
        let raw = self.plan_xy(x, y)?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let to_screen = |p: [f64; 2]| {
            v.camera
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let last = t.points.last().copied();
        let mut s = snap::snap(raw, (x as f32, y as f32), last, &t.lines, to_screen);
        if s.kind == snap::SnapKind::None {
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    pub(crate) fn line_motion(&mut self, x: i32, y: i32) {
        let cursor = self.line_snapped(x, y);
        if let Some(t) = &mut self.line_tool {
            t.cursor = cursor;
        }
        self.line_preview();
        self.line_prompt();
    }

    pub(crate) fn line_click(&mut self, x: i32, y: i32) {
        let Some(p) = self.line_snapped(x, y) else {
            return;
        };
        let Some(t) = &mut self.line_tool else { return };
        t.points.push(p.point);
        let needed = match t.shape {
            Shape::Line => 2,
            Shape::Arc => 3,
        };
        if t.points.len() < needed {
            self.line_preview();
            return self.line_prompt();
        }
        let t = t.clone();
        let (start, end) = (t.points[0], t.points[1]);
        let through: Vec<[f64; 2]> = t.points.get(2).copied().into_iter().collect();
        let result = match t.mode {
            LineMode::RefPlane => {
                let level = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level);
                let mut args = json!({ "start": start, "end": end, "level": level });
                if !t.name.trim().is_empty() {
                    args["name"] = json!(t.name.trim());
                }
                self.run("refplane.create", args)
            }
            LineMode::ModelLine => match self.view_work_plane() {
                Some((_, r, plane)) if plane.is_horizontal() => {
                    let local = |q: [f64; 2]| plane.to_local([q[0], q[1], plane.origin[2]]);
                    let mut args = plane_args(&r);
                    args["curves"] = json!([{
                        "line": { "start": local(start), "end": local(end) },
                        "through": through.iter().map(|q| local(*q)).collect::<Vec<_>>(),
                    }]);
                    args["line_style"] = json!(t.style);
                    self.run("modelline.create", args)
                }
                Some(_) => Err("The view's work plane is vertical: model lines on it are drawn \
                                with `modelline.create` for now (Work Plane › Set a level to draw here)"
                    .to_owned()),
                None => Err("The view has no work plane (Work Plane › Set)".to_owned()),
            },
        };
        match result {
            Ok(_) => {
                if let Some(tool) = &mut self.line_tool {
                    let last = tool.points[1];
                    tool.points.clear();
                    if tool.mode == LineMode::ModelLine && tool.chain {
                        tool.points.push(last);
                    }
                    tool.name.clear();
                }
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.update_line_snaps();
                self.properties_stale = true;
                self.line_preview();
                self.line_prompt();
            }
            Err(e) => {
                if let Some(tool) = &mut self.line_tool {
                    tool.points.pop();
                }
                self.frame.set_status_text(&e, 0);
            }
        }
    }

    /// The snap marker and the line or arc being drawn.
    pub(crate) fn line_preview(&mut self) {
        let Some(t) = &self.line_tool else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.003;
        let mut lines = Lines {
            color: PREVIEW_COLOR,
            width: 0.25,
            segments: Vec::new(),
        };
        if let Some(c) = t.cursor {
            let r = 0.75 * plan.scale / 1000.0;
            let [x, y] = c.point.map(|v| v as f32);
            let corners = [
                [x - r, y - r],
                [x + r, y - r],
                [x + r, y + r],
                [x - r, y + r],
            ];
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                lines.segments.push([[a[0], a[1], z], [b[0], b[1], z]]);
            }
        }
        if let (Some(&start), Some(c)) = (t.points.first(), t.cursor) {
            let pts: Vec<[f64; 2]> = match (t.shape, t.points.len()) {
                (Shape::Arc, 2) => {
                    let line = reviera_core::walls::Line {
                        start,
                        end: t.points[1],
                    };
                    let mut v = vec![start];
                    v.extend(reviera_core::sketch::interior(line, &[c.point]));
                    v.push(t.points[1]);
                    v
                }
                _ => vec![start, c.point],
            };
            for w in pts.windows(2) {
                lines.segments.push([
                    [w[0][0] as f32, w[0][1] as f32, z],
                    [w[1][0] as f32, w[1][1] as f32, z],
                ]);
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Properties while the tool runs; `false` when it does not.
    pub(crate) fn line_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(t) = self.line_tool.clone() else {
            return false;
        };
        match t.mode {
            LineMode::RefPlane => {
                grid.append(Property::category("Reference Plane", "c_lt"));
                grid.append(Property::string("Name", "lt_name", t.name.clone()));
            }
            LineMode::ModelLine => {
                grid.append(Property::category("Model Line", "c_lt"));
                grid.append(Property::enumeration(
                    "Line Style",
                    "lt_style",
                    style_choices(),
                    style_index(&t.style),
                ));
                grid.append(Property::boolean("Chain", "lt_chain", t.chain));
                let label = self
                    .view_work_plane()
                    .map_or_else(|| "<none>".to_owned(), |w| w.0);
                grid.append(Property::string("Work Plane", "lt_plane", label));
                grid.set_property_read_only("lt_plane", true);
            }
        }
        grid.append(Property::enumeration(
            "Shape",
            "lt_shape",
            choices(&SHAPES),
            SHAPES.iter().position(|s| s.0 == t.shape).unwrap_or(0) as i32,
        ));
        true
    }

    /// Rows of a selected reference plane or model line; `false` if `id` is neither.
    pub(crate) fn datum_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else {
            return false;
        };
        if let Some(r) = reviera_core::ref_planes::recipe(e) {
            grid.append(Property::category("Reference Plane", "c_dt"));
            grid.append(Property::string(
                "Subcategory",
                "dt_rp_sub",
                r.subcategory.unwrap_or_default(),
            ));
            let len = reviera_core::work_planes::dot(
                reviera_core::work_planes::sub(r.end, r.start),
                reviera_core::work_planes::sub(r.end, r.start),
            )
            .sqrt();
            grid.append(Property::string(
                "Length",
                "dt_len",
                self.format_length(len),
            ));
            grid.set_property_read_only("dt_len", true);
            return true;
        }
        if let Some(r) = model_lines::recipe(e) {
            grid.append(Property::category("Model Line", "c_dt"));
            grid.append(Property::enumeration(
                "Line Style",
                "dt_ml_style",
                style_choices(),
                style_index(&r.line_style),
            ));
            let label = reviera_core::work_planes::label(&store, &r.plane);
            grid.append(Property::string("Work Plane", "dt_ml_plane", label));
            grid.set_property_read_only("dt_ml_plane", true);
            let len: f64 = r
                .curve
                .points()
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum();
            grid.append(Property::string(
                "Length",
                "dt_len",
                self.format_length(len),
            ));
            grid.set_property_read_only("dt_len", true);
            return true;
        }
        false
    }

    /// Edits of the tool's and selected elements' rows; `false` if `property` is not one.
    pub(crate) fn datum_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let text = text.unwrap_or_default();
        if let Some(row) = property.strip_prefix("lt_") {
            if let Some(t) = &mut self.line_tool {
                match row {
                    "name" => t.name = text,
                    "style" => {
                        if let Some(s) = model_lines::style(&text) {
                            t.style = s.name.to_owned();
                        }
                    }
                    "chain" => t.chain = text == "true" || text == "1",
                    "shape" => {
                        t.shape = SHAPES.iter().find(|s| s.1 == text).map_or(t.shape, |s| s.0);
                        t.points.truncate(1);
                    }
                    _ => {}
                }
            }
            self.properties_stale = true;
            return true;
        }
        let Some(row) = property.strip_prefix("dt_") else {
            return false;
        };
        let ids = self.selection();
        let Some(&id) = ids.first() else {
            return true;
        };
        let result = match row {
            "rp_name" => self.run(
                "refplane.rename",
                json!({ "ref_plane_id": id, "name": text }),
            ),
            "rp_sub" => self.run(
                "refplane.set",
                json!({ "ref_plane_id": id, "subcategory": text }),
            ),
            "ml_style" => self.run(
                "modelline.set",
                json!({ "model_line_ids": ids, "line_style": text }),
            ),
            _ => return true,
        };
        let frame = self.frame;
        match result {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
        true
    }

    fn selected_set(&mut self) -> HashSet<ElementId> {
        self.selection().into_iter().collect()
    }

    /// Reference planes and model lines of plan `index`.
    pub(crate) fn datum_plan_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected = self.selected_set();
        let v = &self.views[index];
        let Some(plan) = &v.plan else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let (lo, hi) = plan.drawn();
        reviera_render::datums::plan_lines(
            &store,
            f64::from(lo),
            f64::from(hi),
            plan.range.cut + 0.002,
            f64::from(plan.scale),
            &selected,
        )
    }

    /// Model lines of 3D view `index` (none in 2D views).
    pub(crate) fn datum_3d_lines(&mut self, index: usize) -> Vec<Lines> {
        if self.views[index].is_2d() {
            return Vec::new();
        }
        let selected = self.selected_set();
        let doc = self.views[index].doc;
        self.app
            .document(doc)
            .map(|d| reviera_render::datums::model_lines_3d(d.doc.store(), &selected))
            .unwrap_or_default()
    }

    /// The reference plane or model line under pixel (`x`, `y`) of the active plan.
    pub(crate) fn datum_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let plan = v.plan.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let z = plan.range.cut as f32;
        let (lo, hi) = plan.drawn();
        let mut best: Option<(f32, ElementId)> = None;
        let mut consider = |id: ElementId, a: [f64; 2], b: [f64; 2]| {
            let d = self.pixel_distance(x, y, [a, b], z);
            if d <= PICK_PX && best.is_none_or(|b| d < b.0) {
                best = Some((d, id));
            }
        };
        for r in reviera_core::ref_planes::list(store) {
            if r.normal[2].abs() < 1e-9 {
                consider(r.id, [r.start[0], r.start[1]], [r.end[0], r.end[1]]);
            }
        }
        for l in model_lines::all(store) {
            let inside = l
                .points
                .iter()
                .all(|p| p[2] >= f64::from(lo) - 1e-6 && p[2] <= f64::from(hi) + 1e-6);
            if !inside {
                continue;
            }
            for w in l.points.windows(2) {
                consider(l.id, [w[0][0], w[0][1]], [w[1][0], w[1][1]]);
            }
        }
        best.map(|b| b.1)
    }

    /// Work Plane › Set: chooses the active view's work plane by name.
    pub(crate) fn set_work_plane(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return frame.set_status_text("Open a view first", 0);
        };
        let Ok(named) = self.run("workplane.list", json!({})) else {
            return;
        };
        let named: Vec<reviera_core::work_planes::NamedPlane> =
            serde_json::from_value(named).unwrap_or_default();
        if named.is_empty() {
            return frame
                .set_status_text("The model has no level, grid or named reference plane", 0);
        }
        let current = self.view_work_plane().map(|w| w.0).unwrap_or_default();
        let labels: Vec<&str> = named.iter().map(|n| n.label.as_str()).collect();
        let d = SingleChoiceDialog::builder(
            &frame,
            &format!(
                "Current: {}\n\nSpecify a new work plane by name:",
                if current.is_empty() {
                    "<none>"
                } else {
                    &current
                }
            ),
            "Work Plane",
            &labels,
        )
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(choice) = usize::try_from(d.get_selection())
            .ok()
            .and_then(|i| named.get(i))
        else {
            return;
        };
        let mut args = plane_args(&choice.plane);
        args["view_id"] = json!(view);
        match self.run("workplane.set", args) {
            Ok(wp) => {
                let label = wp["label"].as_str().unwrap_or_default().to_owned();
                if wp["resolved"]["normal"][2]
                    .as_f64()
                    .is_some_and(|n| n.abs() < 1e-9)
                    && self.active_view().is_some_and(|v| v.plan.is_some())
                {
                    frame.set_status_text(
                        &format!("Work plane: {label}. It is vertical: open a section or elevation along it to draw on it."),
                        0,
                    );
                } else {
                    frame.set_status_text(&format!("Work plane: {label}"), 0);
                }
                self.properties_stale = true;
                let doc = self.active_view().map(|v| v.doc);
                if let Some(doc) = doc {
                    self.plans_redraw(doc);
                }
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Work Plane › Show: toggles the active view's work plane grid.
    pub(crate) fn toggle_work_plane(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return;
        };
        let shown = self
            .run("workplane.get", json!({ "view_id": view }))
            .ok()
            .and_then(|w| w["show"].as_bool())
            .unwrap_or(false);
        match self.run("workplane.show", json!({ "view_id": view, "on": !shown })) {
            Ok(_) => {
                if let Some(doc) = self.active_view().map(|v| v.doc) {
                    self.plans_redraw(doc);
                }
                if let Some(v) = self.active_view() {
                    v.panel.refresh(false, None);
                }
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The shown work plane of view `index` as a grid of lines (none when not shown).
    pub(crate) fn work_plane_grid(&mut self, index: usize) -> Vec<Lines> {
        let Some(view) = self.views[index].element else {
            return Vec::new();
        };
        let Ok(wp) = self.run("workplane.get", json!({ "view_id": view })) else {
            return Vec::new();
        };
        if wp["show"].as_bool() != Some(true) {
            return Vec::new();
        }
        let Ok(plane) = serde_json::from_value::<Plane>(wp["resolved"].clone()) else {
            return Vec::new();
        };
        let step = wp["grid_spacing"].as_f64().unwrap_or(1.0).max(0.01);
        // Around the model's extent in the plane, at most 200 lines a direction.
        let doc = self.views[index].doc;
        let bounds = self.scenes.get(&doc).and_then(|s| s.bounds());
        let (mut lo, mut hi) = ([-10.0f64, -10.0], [10.0f64, 10.0]);
        if let Some((a, b)) = bounds {
            let corners = [
                [a.x, a.y, a.z],
                [b.x, a.y, a.z],
                [a.x, b.y, a.z],
                [b.x, b.y, a.z],
                [a.x, a.y, b.z],
                [b.x, a.y, b.z],
                [a.x, b.y, b.z],
                [b.x, b.y, b.z],
            ];
            for c in corners {
                let l = plane.to_local(c.map(f64::from));
                for k in 0..2 {
                    lo[k] = lo[k].min(l[k] - 2.0 * step);
                    hi[k] = hi[k].max(l[k] + 2.0 * step);
                }
            }
        }
        // On grid lines, so the lines of each direction end on the other's last ones.
        for k in 0..2 {
            lo[k] = (lo[k] / step).floor() * step;
            hi[k] = ((hi[k] / step).ceil() * step).min(lo[k] + 200.0 * step);
        }
        let mut segments = Vec::new();
        for k in 0..2 {
            let first = (lo[k] / step).round() as i64;
            let last = (hi[k] / step).round() as i64;
            for i in first..=last {
                let t = i as f64 * step;
                let (a, b) = if k == 0 {
                    ([t, lo[1]], [t, hi[1]])
                } else {
                    ([lo[0], t], [hi[0], t])
                };
                let (a, b) = (plane.to_world(a), plane.to_world(b));
                segments.push([a.map(|v| v as f32), b.map(|v| v as f32)]);
            }
        }
        vec![Lines {
            color: [0.35, 0.55, 0.85, 0.5],
            width: 0.13,
            segments,
        }]
    }
}
