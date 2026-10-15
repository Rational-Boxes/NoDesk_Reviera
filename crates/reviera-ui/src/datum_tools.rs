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
    /// Two opposite corners of a scope box (ARC-SCOPE-001).
    ScopeBox,
    /// A roof by extrusion's open profile, clicked in a section or elevation (ARC-ROOF-002);
    /// Enter finishes it.
    RoofExtrusion,
    /// A slope arrow on the selected roof: its tail on a gable edge, its head inside
    /// (ARC-ROOF-005).
    SlopeArrow,
    /// Join/Unjoin Roof (ARC-ROOF-006): a roof's edge, then the wall or roof to extend it to.
    JoinRoof,
    /// Floor: Slab Edge (ARC-SLED-001): click floor edges, one run per floor.
    SlabEdge,
    /// Shape Editing › Add Point on the selected floor (ARC-SHAPE-001).
    ShapePoint,
    /// Split with Gap (ARC-WALL-011): click a wall where the gap goes.
    SplitGap,
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
    /// The roof a slope arrow goes on.
    target: Option<ElementId>,
    /// The roof edge picked for Join Roof: the roof and its footprint curve.
    join_edge: Option<(ElementId, u32)>,
    /// The slab edge being built and its floor.
    sweep: Option<(ElementId, ElementId)>,
    points: Vec<[f64; 2]>,
    cursor: Option<snap::Snap>,
    /// A typed length (ARC-DIM-002) and its direction, frozen when typing began.
    pub(crate) typed: String,
    pub(crate) typed_dir: Option<[f64; 2]>,
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
        if mode == LineMode::RoofExtrusion {
            if self.active_view().is_none_or(|v| v.section.is_none()) {
                return frame.set_status_text(
                    "Roof by Extrusion: open a section or elevation looking at the roof's profile",
                    0,
                );
            }
        } else if !self.ensure_plan(false) {
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
        let target = (mode == LineMode::SlopeArrow)
            .then(|| self.selection())
            .and_then(|ids| {
                let store = self.store_now()?;
                ids.into_iter().find(|id| {
                    store
                        .get(*id)
                        .and_then(reviera_core::roofs::roof_recipe)
                        .is_some_and(|r| {
                            r.arch.is_none() && r.dome.is_none() && r.extrusion.is_none()
                        })
                })
            });
        if mode == LineMode::SlopeArrow && target.is_none() {
            return frame.set_status_text("Slope Arrow: select a footprint roof first", 0);
        }
        let target = if mode == LineMode::ShapePoint {
            let floor = self.selection().into_iter().find(|id| {
                self.store_now().is_some_and(|s| {
                    s.get(*id)
                        .and_then(reviera_core::floors::floor_recipe)
                        .is_some()
                })
            });
            if floor.is_none() {
                return frame.set_status_text("Add Point: select a floor first", 0);
            }
            floor
        } else {
            target
        };
        self.line_tool = Some(LineTool {
            target,
            join_edge: None,
            sweep: None,
            mode,
            shape: Shape::Line,
            chain: mode == LineMode::ModelLine,
            style,
            name: String::new(),
            points: Vec::new(),
            cursor: None,
            typed: String::new(),
            typed_dir: None,
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
            LineMode::ScopeBox => "Scope Box",
            LineMode::RoofExtrusion => "Roof by Extrusion",
            LineMode::SlopeArrow => "Slope Arrow",
            LineMode::JoinRoof => "Join/Unjoin Roof",
            LineMode::SlabEdge => "Slab Edge",
            LineMode::ShapePoint => "Add Point",
            LineMode::SplitGap => "Split with Gap",
        };
        let mut text = match (t.points.len(), t.shape) {
            _ if t.mode == LineMode::SplitGap => {
                format!("{what}: click a wall where the gap goes")
            }
            _ if t.mode == LineMode::ShapePoint => {
                format!("{what}: click a point on the floor, then type its height from the surface")
            }
            _ if t.mode == LineMode::SlabEdge => {
                format!("{what}: click a floor's edges; each one joins the run (Esc ends)")
            }
            _ if t.mode == LineMode::JoinRoof && t.join_edge.is_none() => {
                format!("{what}: click the roof edge to extend")
            }
            _ if t.mode == LineMode::JoinRoof => {
                "Click the wall or roof to join it to (the same roof unjoins it)".to_owned()
            }
            (0, _) if t.mode == LineMode::SlopeArrow => {
                format!("{what}: click its tail on a gable edge of the roof")
            }
            (1, _) if t.mode == LineMode::SlopeArrow => {
                "Click its head, inside the roof (toward the ridge)".to_owned()
            }
            (0, _) if t.mode == LineMode::RoofExtrusion => {
                format!("{what}: click the profile's first point (the roof's top)")
            }
            (_, _) if t.mode == LineMode::RoofExtrusion => {
                "Click the next point; Enter finishes the profile, Esc restarts it".to_owned()
            }
            (0, _) if t.mode == LineMode::ScopeBox => format!("{what}: click a corner"),
            (1, _) if t.mode == LineMode::ScopeBox => {
                "Click the opposite corner. Esc restarts.".to_owned()
            }
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
        // In a section or elevation: the point on the view's plane (across, up), to the mm.
        if v.section.is_some() {
            let size = v.panel.get_client_size();
            let (o, d) = v
                .camera
                .ray(x as f32, y as f32, size.width as f32, size.height as f32);
            if d.z.abs() < 1e-9 {
                return None;
            }
            let k = -o.z / d.z;
            let p = [f64::from(o.x + d.x * k), f64::from(o.y + d.y * k)];
            return Some(snap::Snap {
                point: p.map(|v| (v * 1000.0).round() / 1000.0),
                kind: snap::SnapKind::None,
            });
        }
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
        if self
            .line_tool
            .as_ref()
            .is_some_and(|t| t.mode == LineMode::JoinRoof)
        {
            return self.join_click(x, y);
        }
        if self
            .line_tool
            .as_ref()
            .is_some_and(|t| t.mode == LineMode::SlabEdge)
        {
            return self.slab_edge_click(x, y);
        }
        if self
            .line_tool
            .as_ref()
            .is_some_and(|t| t.mode == LineMode::ShapePoint)
        {
            return self.shape_point_click(x, y);
        }
        if self
            .line_tool
            .as_ref()
            .is_some_and(|t| t.mode == LineMode::SplitGap)
        {
            return self.split_gap_click(x, y);
        }
        let Some(p) = self.line_snapped(x, y) else {
            return;
        };
        self.line_click_at(p.point);
    }

    /// Join/Unjoin Roof: first the roof edge nearest the click (within 0.5 m of a footprint
    /// roof's curve), then the target under the cursor.
    fn join_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(store) = self.store_now() else {
            return;
        };
        let edge = self.line_tool.as_ref().and_then(|t| t.join_edge);
        match edge {
            None => {
                let mut best: Option<(f64, ElementId, u32)> = None;
                for e in store.iter() {
                    let Some(r) = reviera_core::roofs::roof_recipe(e) else {
                        continue;
                    };
                    if r.arch.is_some() || r.dome.is_some() || r.extrusion.is_some() {
                        continue;
                    }
                    let Some(sk) = reviera_core::sketch::sketch_of(e) else {
                        continue;
                    };
                    for c in &sk.curves {
                        let (a, b) = (c.line.start, c.line.end);
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                        let s =
                            (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                        let dist = (a[0] + d[0] * s - p[0]).hypot(a[1] + d[1] * s - p[1]);
                        if dist < 0.5 && best.is_none_or(|b| dist < b.0) {
                            best = Some((dist, e.id(), c.key));
                        }
                    }
                }
                match best {
                    Some((_, roof, key)) => {
                        if let Some(t) = &mut self.line_tool {
                            t.join_edge = Some((roof, key));
                        }
                        self.line_prompt();
                    }
                    None => frame.set_status_text("Click on a footprint roof's edge", 0),
                }
            }
            Some((roof, key)) => {
                let panel = match self.active_view() {
                    Some(v) => v.panel,
                    None => return,
                };
                let Some(target) = self.element_at(x, y, &panel) else {
                    return frame.set_status_text("Click a wall or a roof", 0);
                };
                let result = if target == roof {
                    self.run("roof.unjoin", json!({ "roof_id": roof, "key": key }))
                } else {
                    self.run(
                        "roof.join",
                        json!({ "roof_id": roof, "key": key, "target_id": target }),
                    )
                };
                match result {
                    Ok(_) => {
                        self.refresh_scene(&frame);
                        if let Some(t) = &mut self.line_tool {
                            t.join_edge = None;
                        }
                        self.line_prompt();
                    }
                    Err(e) => frame.set_status_text(&e, 0),
                }
            }
        }
    }

    /// The last point and the cursor of a straight line being drawn (typed lengths apply).
    pub(crate) fn line_ends(&self) -> Option<([f64; 2], [f64; 2])> {
        let t = self.line_tool.as_ref()?;
        (t.shape == Shape::Line || t.mode == LineMode::ScopeBox)
            .then_some(())
            .and_then(|_| Some((*t.points.last()?, t.cursor?.point)))
    }

    pub(crate) fn line_click_at(&mut self, point: [f64; 2]) {
        let Some(t) = &mut self.line_tool else { return };
        t.typed.clear();
        t.typed_dir = None;
        t.points.push(point);
        if t.mode == LineMode::RoofExtrusion {
            self.properties_stale = true;
            self.line_preview();
            return self.line_prompt();
        }
        let needed = match t.shape {
            Shape::Arc if t.mode != LineMode::ScopeBox => 3,
            _ => 2,
        };
        if t.points.len() < needed {
            self.line_preview();
            return self.line_prompt();
        }
        let t = t.clone();
        let (start, end) = (t.points[0], t.points[1]);
        let through: Vec<[f64; 2]> = t.points.get(2).copied().into_iter().collect();
        let result = match t.mode {
            // Finished with Enter (`line_enter`), or by its own clicks.
            LineMode::RoofExtrusion
            | LineMode::JoinRoof
            | LineMode::SlabEdge
            | LineMode::ShapePoint
            | LineMode::SplitGap => return,
            LineMode::SlopeArrow => {
                let roof = t.target;
                // The footprint curve nearest the tail.
                let key = self.store_now().and_then(|st| {
                    let e = st.get(roof?)?;
                    let sk = reviera_core::sketch::sketch_of(e)?;
                    sk.curves
                        .iter()
                        .map(|c| {
                            let (a, b) = (c.line.start, c.line.end);
                            let d = [b[0] - a[0], b[1] - a[1]];
                            let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                            let s = (((start[0] - a[0]) * d[0] + (start[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                            let f = [a[0] + d[0] * s, a[1] + d[1] * s];
                            ((f[0] - start[0]).hypot(f[1] - start[1]), c.key, f)
                        })
                        .min_by(|x, y| x.0.total_cmp(&y.0))
                });
                match key {
                    Some((_, key, foot)) => self.run(
                        "roof.set_slope_arrow",
                        json!({ "roof_id": roof, "key": key, "tail": foot, "head": end, "specify": "slope" }),
                    ),
                    None => Err("The selected roof has no footprint".to_owned()),
                }
            }
            LineMode::ScopeBox => {
                let mut args = json!({ "rectangle": { "corner_a": start, "corner_b": end } });
                if !t.name.trim().is_empty() {
                    args["name"] = json!(t.name.trim());
                }
                self.run("scopebox.create", args)
            }
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
        // A plan draws over its cut; a section in its own frame, over the view plane.
        let (z, scale) = match self.active_view() {
            Some(v) if v.plan.is_some() => {
                let p = v.plan.as_ref().expect("checked");
                (p.range.cut as f32 + 0.003, p.scale)
            }
            Some(v) if v.section.is_some() => (0.003, v.section.as_ref().expect("checked").scale),
            _ => return,
        };
        struct Scale {
            scale: f32,
        }
        let plan = Scale { scale };
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
                _ if t.mode == LineMode::RoofExtrusion => {
                    let mut v = t.points.clone();
                    v.push(c.point);
                    v
                }
                _ if t.mode == LineMode::ScopeBox => vec![
                    start,
                    [c.point[0], start[1]],
                    c.point,
                    [start[0], c.point[1]],
                    start,
                ],
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
        let mut all = vec![lines];
        all.extend(self.typed_overlay(z, plan.scale));
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&all);
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
            LineMode::ScopeBox => {
                grid.append(Property::category("Scope Box", "c_lt"));
                grid.append(Property::string("Name", "lt_name", t.name.clone()));
                return true;
            }
            LineMode::SlopeArrow => {
                grid.append(Property::category("Slope Arrow", "c_lt"));
                return true;
            }
            LineMode::JoinRoof => {
                grid.append(Property::category("Join/Unjoin Roof", "c_lt"));
                return true;
            }
            LineMode::SlabEdge => {
                grid.append(Property::category("Slab Edge", "c_lt"));
                return true;
            }
            LineMode::ShapePoint => {
                grid.append(Property::category("Add Point", "c_lt"));
                return true;
            }
            LineMode::SplitGap => {
                grid.append(Property::category("Split with Gap", "c_lt"));
                return true;
            }
            LineMode::RoofExtrusion => {
                grid.append(Property::category("Roof by Extrusion", "c_lt"));
                grid.append(Property::string(
                    "Profile Points",
                    "lt_count",
                    t.points.len().to_string(),
                ));
                grid.set_property_read_only("lt_count", true);
                return true;
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
            self.datum_view_rows(grid, id);
            return true;
        }
        if let Some(b) = reviera_core::scope_boxes::recipe(e) {
            grid.append(Property::category("Scope Box", "c_dt"));
            grid.append(Property::string(
                "Name",
                "dt_sb_name",
                e.name.clone().unwrap_or_default(),
            ));
            grid.append(Property::string(
                "Height",
                "dt_sb_height",
                self.format_length(b.height),
            ));
            grid.append(Property::string(
                "Rotation",
                "dt_sb_rotation",
                format!("{:.1}°", b.rotation.to_degrees()),
            ));
            let n = reviera_core::datum_extents::assigned_to(&store, e.guid().as_str()).len();
            grid.append(Property::string("Datums", "dt_sb_datums", n.to_string()));
            grid.set_property_read_only("dt_sb_datums", true);
            return true;
        }
        if let Some(r) = reviera_core::slab_edges::recipe(e) {
            grid.append(Property::category("Slab Edge", "c_dt"));
            for (label, name, v) in [
                ("Width", "dt_se_width", r.width),
                ("Depth", "dt_se_depth", r.depth),
                ("Horizontal Profile Offset", "dt_se_h", r.horizontal_offset),
                ("Vertical Profile Offset", "dt_se_v", r.vertical_offset),
            ] {
                grid.append(Property::string(label, name, self.format_length(v)));
            }
            grid.append(Property::boolean(
                "Flip Horizontal",
                "dt_se_fh",
                r.flip_horizontal,
            ));
            grid.append(Property::boolean(
                "Flip Vertical",
                "dt_se_fv",
                r.flip_vertical,
            ));
            if let Ok(g) = reviera_core::slab_edges::regenerate(&store, e) {
                grid.append(Property::string(
                    "Length",
                    "dt_se_len",
                    self.format_length(g.length),
                ));
                grid.set_property_read_only("dt_se_len", true);
                grid.append(Property::string(
                    "Volume",
                    "dt_se_vol",
                    format!("{:.3} m³", g.volume),
                ));
                grid.set_property_read_only("dt_se_vol", true);
            }
            return true;
        }
        if let Some(r) = reviera_core::wall_sweeps::recipe(e) {
            use reviera_core::wall_sweeps::{Orientation, Side};
            grid.append(Property::category(
                if r.reveal {
                    "Wall Reveal"
                } else {
                    "Wall Sweep"
                },
                "c_dt",
            ));
            grid.append(Property::enumeration(
                "Side",
                "dt_ws_side",
                vec![("Exterior".to_owned(), 0), ("Interior".to_owned(), 1)],
                i32::from(r.side == Side::Interior),
            ));
            let (label, v) = match r.orientation {
                Orientation::Horizontal => ("Offset From Level", r.offset),
                Orientation::Vertical => ("Distance Along Wall", r.distance),
            };
            grid.append(Property::string(label, "dt_ws_at", self.format_length(v)));
            grid.append(Property::string(
                "Wall Offset",
                "dt_ws_woff",
                self.format_length(r.wall_offset),
            ));
            let (lo, hi) = r
                .profile
                .iter()
                .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                });
            // A reveal reaches a millimetre past the face.
            let depth = if r.reveal { -lo[0] } else { hi[0] - lo[0] };
            grid.append(Property::string(
                if r.reveal { "Depth" } else { "Projection" },
                "dt_ws_w",
                self.format_length(depth),
            ));
            grid.append(Property::string(
                "Profile Height",
                "dt_ws_h",
                self.format_length(hi[1] - lo[1]),
            ));
            if !r.reveal {
                grid.append(Property::boolean("Cuts Wall", "dt_ws_cuts", r.cuts_wall));
            }
            grid.append(Property::string(
                "Walls",
                "dt_ws_walls",
                r.walls.len().to_string(),
            ));
            grid.set_property_read_only("dt_ws_walls", true);
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
            "se_width" | "se_depth" | "se_h" | "se_v" => {
                let v = self
                    .run(
                        "units.parse",
                        json!({ "text": text, "data_type": "length" }),
                    )
                    .ok()
                    .and_then(|v| v.as_f64());
                let field = match row {
                    "se_width" => "width",
                    "se_depth" => "depth",
                    "se_h" => "horizontal_offset",
                    _ => "vertical_offset",
                };
                match v {
                    Some(v) => self.run("slabedge.set", json!({ "slab_edge_id": id, field: v })),
                    None => Err("Type a length".to_owned()),
                }
            }
            "se_fh" | "se_fv" => self.run(
                "hostsweep.flip",
                json!({ "id": id, "axis": if row == "se_fh" { "Horizontal" } else { "Vertical" } }),
            ),
            "sb_name" => self.run(
                "scopebox.set_extents",
                json!({ "scope_box_id": id, "name": text }),
            ),
            "sb_rotation" => {
                let deg: Option<f64> = text.trim().trim_end_matches('°').trim().parse().ok();
                match deg {
                    Some(d) => self.run(
                        "scopebox.set_extents",
                        json!({ "scope_box_id": id, "rotation": d.to_radians() }),
                    ),
                    None => Err("Type an angle in degrees".to_owned()),
                }
            }
            "sb_height" => {
                let height = self
                    .run(
                        "units.parse",
                        json!({ "text": text, "data_type": "length" }),
                    )
                    .ok()
                    .and_then(|v| v.as_f64());
                let b = self
                    .store_now()
                    .and_then(|st| st.get(id).and_then(reviera_core::scope_boxes::recipe));
                match (height, b) {
                    (Some(h), Some(b)) => {
                        let (u, v) = b.axes();
                        let c = b.corners();
                        let _ = (u, v);
                        self.run(
                            "scopebox.set_extents",
                            json!({
                                "scope_box_id": id,
                                "min": [c[0][0], c[0][1], b.bottom],
                                "max": [c[2][0], c[2][1], b.bottom + h],
                            }),
                        )
                    }
                    _ => Err("Type a length".to_owned()),
                }
            }
            "ws_side" => self.run(
                "wallsweep.set",
                json!({ "sweep_id": id, "side": if text == "Interior" { "interior" } else { "exterior" } }),
            ),
            "ws_cuts" => self.run(
                "wallsweep.set",
                json!({ "sweep_id": id, "cuts_wall": text == "true" || text == "1" }),
            ),
            "ws_at" | "ws_woff" | "ws_w" | "ws_h" => {
                let v = self
                    .run("units.parse", json!({ "text": text, "data_type": "length" }))
                    .ok()
                    .and_then(|v| v.as_f64());
                let r = self.store_now().and_then(|s| {
                    s.get(id).and_then(reviera_core::wall_sweeps::recipe)
                });
                match (v, r) {
                    (Some(v), Some(r)) => {
                        let vertical =
                            r.orientation == reviera_core::wall_sweeps::Orientation::Vertical;
                        let (lo, hi) = r.profile.iter().fold(
                            ([f64::MAX; 2], [f64::MIN; 2]),
                            |(lo, hi), p| {
                                ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
                            },
                        );
                        let (mut w, mut h) = (
                            if r.reveal { -lo[0] } else { hi[0] - lo[0] },
                            hi[1] - lo[1],
                        );
                        let args = match row {
                            "ws_at" if vertical => json!({ "sweep_id": id, "distance_along": v }),
                            "ws_at" => json!({ "sweep_id": id, "offset_from_level": v }),
                            "ws_woff" => json!({ "sweep_id": id, "wall_offset": v }),
                            _ => {
                                if row == "ws_w" {
                                    w = v;
                                } else {
                                    h = v;
                                }
                                let profile = if r.reveal {
                                    json!([[-w, 0.0], [0.001, 0.0], [0.001, h], [-w, h]])
                                } else {
                                    json!([[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]])
                                };
                                json!({ "sweep_id": id, "profile": profile })
                            }
                        };
                        self.run("wallsweep.set", args)
                    }
                    _ => Err("Type a length".to_owned()),
                }
            }
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
        let view = v
            .element
            .and_then(|e| store.get(e))
            .map(|e| e.guid().to_string());
        let mut out = reviera_render::datums::scope_box_lines(
            &store,
            plan.range.cut,
            plan.range.cut + 0.002,
            f64::from(plan.scale),
            &selected,
        );
        out.extend(reviera_render::datums::plan_lines(
            &store,
            view.as_deref(),
            plan.range.cut,
            f64::from(lo),
            f64::from(hi),
            plan.range.cut + 0.002,
            f64::from(plan.scale),
            &selected,
        ));
        out
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
        let view = v
            .element
            .and_then(|e| store.get(e))
            .map(|e| e.guid().to_string());
        for r in reviera_core::ref_planes::list(store) {
            let Some(e) = store.get(r.id) else { continue };
            if r.normal[2].abs() >= 1e-9
                || !reviera_core::datum_extents::shows_in_plan(store, e, plan.range.cut)
            {
                continue;
            }
            if let Some(l) = reviera_core::datum_extents::plan_line(e, view.as_deref()) {
                consider(r.id, l.start, l.end);
            }
        }
        // Scope boxes, by their outlines.
        for (id, _, b) in reviera_core::scope_boxes::in_plan(store, plan.range.cut) {
            let c = b.corners();
            for k in 0..4 {
                consider(id, c[k], c[(k + 1) % 4]);
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

impl Gui {
    /// The active view's element and GlobalId, if it is a plan, section or elevation.
    fn extent_view(&self) -> Option<(ElementId, String, bool)> {
        let v = self.active_view()?;
        let plan = v.plan.is_some();
        if !plan && v.section.is_none() {
            return None;
        }
        let id = v.element?;
        let guid = self
            .app
            .document(v.doc)?
            .doc
            .store()
            .get(id)?
            .guid()
            .to_string();
        Some((id, guid, plan))
    }

    /// Rows of a selected level, grid or reference plane: its extents in the active view (3D or
    /// 2D, bubbles, a grid's elbows) and its scope box (ARC-DATUM-001, ARC-SCOPE-001).
    pub(crate) fn datum_view_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        use reviera_core::datum_extents::{self, Kind};
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let Some(kind) = datum_extents::kind_of(e) else {
            return;
        };
        grid.append(Property::category("Extents", "c_dx"));
        if let Some((_, guid, plan)) = self.extent_view() {
            let v = datum_extents::in_view(e, &guid);
            // Levels show in sections and elevations only.
            if !(kind == Kind::Level && plan) {
                grid.append(Property::enumeration(
                    "Extent in View",
                    "dx_mode",
                    vec![("3D".to_owned(), 0), ("2D".to_owned(), 1)],
                    i32::from(v.is_2d()),
                ));
                let default = match kind {
                    Kind::Grid if plan => reviera_core::grids::list(&store)
                        .into_iter()
                        .find(|g| g.id == id)
                        .map_or([false, true], |g| g.bubbles),
                    _ => [false, true],
                };
                let b = v.bubbles.unwrap_or(default);
                if kind == Kind::Grid {
                    grid.append(Property::boolean("Bubble at Start", "dx_b0", b[0]));
                    grid.append(Property::boolean("Bubble at End", "dx_b1", b[1]));
                }
                if kind == Kind::Grid && plan {
                    grid.append(Property::boolean(
                        "Elbow at Start",
                        "dx_e0",
                        v.elbows[0].is_some(),
                    ));
                    grid.append(Property::boolean(
                        "Elbow at End",
                        "dx_e1",
                        v.elbows[1].is_some(),
                    ));
                }
            }
        }
        let boxes = reviera_core::scope_boxes::list(&store);
        let current = datum_extents::extent(e).scope_box;
        let mut choices = vec![("None".to_owned(), 0)];
        choices.extend(
            boxes
                .iter()
                .enumerate()
                .map(|(i, b)| (b.name.clone(), i as i32 + 1)),
        );
        let index = current
            .and_then(|g| boxes.iter().position(|b| b.guid == g))
            .map_or(0, |i| i as i32 + 1);
        grid.append(Property::enumeration(
            "Scope Box",
            "dx_scope",
            choices,
            index,
        ));
    }

    /// Edits of the extent rows; `false` if `property` is not one.
    pub(crate) fn datum_view_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(row) = property.strip_prefix("dx_") else {
            return false;
        };
        let text = text.unwrap_or_default();
        let on = text == "true" || text == "1";
        let Some(&id) = self.selection().first() else {
            return true;
        };
        let view = self.extent_view();
        let end = |k: &str| if k.ends_with('0') { "start" } else { "end" };
        let result = match (row, &view) {
            ("mode", Some((view, guid, plan))) => {
                if text == "3D" {
                    self.run(
                        "datum.reset_to_3d",
                        json!({ "datum_ids": [id], "view_id": view }),
                    )
                } else {
                    // The 2D extent starts as the 3D one.
                    let store = self.store_now();
                    let point = store.as_ref().and_then(|st| {
                        let e = st.get(id)?;
                        if *plan {
                            reviera_core::datum_extents::plan_line(e, Some(guid)).map(|l| l.start)
                        } else {
                            let h = reviera_core::datum_extents::heights(st, e, Some(guid))
                                .unwrap_or_else(|| reviera_core::datum_extents::default_heights(st));
                            Some([0.0, h[0]])
                        }
                    });
                    match point {
                        Some(p) => self.run(
                            "datum.set_extent",
                            json!({ "datum_id": id, "view_id": view, "end": "start", "point": p }),
                        ),
                        None => Ok(Value::Null),
                    }
                }
            }
            ("b0" | "b1", Some((view, _, _))) => self.run(
                "datum.set_bubble",
                json!({ "datum_id": id, "view_id": view, "end": end(row), "visible": on }),
            ),
            ("e0" | "e1", Some((view, _, _))) => self.run(
                "datum.add_elbow",
                json!({
                    "datum_id": id, "view_id": view, "end": end(row),
                    "offset": if on { json!(reviera_core::datum_extents::ELBOW_MM) } else { json!(0.0) },
                }),
            ),
            ("scope", _) => {
                let guid = self.store_now().and_then(|st| {
                    reviera_core::scope_boxes::list(&st)
                        .into_iter()
                        .find(|b| b.name == text)
                        .map(|b| b.guid)
                });
                self.run(
                    "datum.assign_scope_box",
                    json!({ "datum_ids": [id], "scope_box_id": guid }),
                )
            }
            _ => return true,
        };
        let frame = self.frame;
        match result {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.update_grips();
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
        true
    }

    /// Modify | Grids/Levels/Reference Planes › Reset to 3D Extents (the active view).
    pub(crate) fn datum_reset_to_3d(&mut self) {
        let frame = self.frame;
        let Some((view, _, _)) = self.extent_view() else {
            return frame.set_status_text("Open the plan, section or elevation to reset", 0);
        };
        let ids = self.selection();
        match self.run(
            "datum.reset_to_3d",
            json!({ "datum_ids": ids, "view_id": view }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.update_grips();
        self.properties_stale = true;
    }

    /// Maximize 3D Extents.
    pub(crate) fn datum_maximize_3d(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        match self.run("datum.maximize_3d", json!({ "datum_ids": ids })) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.update_grips();
        self.properties_stale = true;
    }

    /// Propagate Extents…: the selected datums' extents in this view to chosen views of the same
    /// kind.
    pub(crate) fn datum_propagate(&mut self) {
        let frame = self.frame;
        let Some((view, _, plan)) = self.extent_view() else {
            return frame
                .set_status_text("Open the plan, section or elevation to propagate from", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let candidates: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| e.id() != view)
            .filter_map(|e| {
                let d = reviera_core::views::view_data(e)?;
                let is_plan = reviera_core::plans::is_plan(&d.kind);
                let is_section = reviera_core::sections::section_of(e).is_some();
                ((plan && is_plan) || (!plan && is_section))
                    .then(|| (e.id(), e.name.clone().unwrap_or_default()))
            })
            .collect();
        if candidates.is_empty() {
            return frame.set_status_text("There is no other view of this kind", 0);
        }
        let labels: Vec<&str> = candidates.iter().map(|c| c.1.as_str()).collect();
        let d = MultiChoiceDialog::builder(
            &frame,
            "Propagate the extents to:",
            "Propagate Extents",
            &labels,
        )
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let targets: Vec<ElementId> = d
            .get_selections()
            .into_iter()
            .filter_map(|i| candidates.get(i as usize).map(|c| c.0))
            .collect();
        let ids = self.selection();
        match self.run(
            "datum.propagate_extents",
            json!({ "datum_ids": ids, "source_view_id": view, "target_view_ids": targets }),
        ) {
            Ok(_) => {
                frame.set_status_text(&format!("Extents propagated to {} views", targets.len()), 0);
                self.refresh_scene(&frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}

impl Gui {
    /// Enter in the Roof by Extrusion tool: the roof on the section's plane, from the profile
    /// clicked; `true` when it was the tool's.
    pub(crate) fn line_enter(&mut self, key: i32) -> bool {
        if !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            return false;
        }
        let Some(t) = &self.line_tool else {
            return false;
        };
        if t.mode != LineMode::RoofExtrusion {
            return false;
        }
        let frame = self.frame;
        if t.points.len() < 2 {
            frame.set_status_text("Click at least two points of the profile", 0);
            return true;
        }
        let points = t.points.clone();
        let Some(def) = self
            .active_view()
            .and_then(|v| v.section.as_ref())
            .map(|s| s.def)
        else {
            return true;
        };
        let f = def.frame();
        // The view's plane: along its right, up; its normal right × up.
        let origin = f.to_world([0.0, 0.0, 0.0]);
        let x_dir = [f.right[0], f.right[1], 0.0];
        let normal = [f.right[1], -f.right[0], 0.0];
        // The Reference Level: the highest at or below the profile's lowest point.
        let low = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let level = self.store_now().and_then(|st| {
            let levels = reviera_core::levels::list(&st);
            levels
                .iter()
                .rev()
                .find(|l| l.elevation <= low + 1e-6)
                .or(levels.first())
                .map(|l| (l.id, l.elevation))
        });
        let Some((level, elevation)) = level else {
            frame.set_status_text("The model has no level", 0);
            return true;
        };
        let profile: Vec<Value> = points
            .windows(2)
            .map(|w| json!({ "line": { "start": [w[0][0], w[0][1] - elevation], "end": [w[1][0], w[1][1] - elevation] } }))
            .collect();
        match self.run(
            "roof.create_extrusion",
            json!({
                "plane": { "origin": origin, "x_dir": x_dir, "normal": normal },
                "profile": profile,
                "ref_level": level,
            }),
        ) {
            Ok(_) => {
                self.end_line_tool();
                self.refresh_scene(&frame);
                frame.set_status_text("Roof by extrusion created", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
        true
    }
}

impl Gui {
    /// Slab Edge: the floor edge nearest the click (within 0.5 m) starts a run, or joins the run on
    /// the same floor.
    fn slab_edge_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(store) = self.store_now() else {
            return;
        };
        let mut best: Option<(f64, ElementId, u32)> = None;
        for e in store.iter() {
            if reviera_core::floors::floor_recipe(e).is_none() {
                continue;
            }
            let Some(sk) = reviera_core::sketch::sketch_of(e) else {
                continue;
            };
            for c in &sk.curves {
                let (a, b) = (c.line.start, c.line.end);
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                let s = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                let dist = (a[0] + d[0] * s - p[0]).hypot(a[1] + d[1] * s - p[1]);
                if dist < 0.5 && best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, e.id(), c.key));
                }
            }
        }
        let Some((_, floor, key)) = best else {
            return frame.set_status_text("Click a floor's edge", 0);
        };
        let current = self.line_tool.as_ref().and_then(|t| t.sweep);
        let result = match current {
            Some((se, f)) if f == floor => self
                .run(
                    "hostsweep.add_segments",
                    json!({ "id": se, "edge_refs": [key] }),
                )
                .map(|_| se),
            _ => self
                .run(
                    "slabedge.create",
                    json!({ "host_id": floor, "keys": [key] }),
                )
                .and_then(|v| {
                    v.as_u64()
                        .map(ElementId)
                        .ok_or_else(|| "no slab edge".to_owned())
                }),
        };
        match result {
            Ok(se) => {
                if let Some(t) = &mut self.line_tool {
                    t.sweep = Some((se, floor));
                }
                self.refresh_scene(&frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}

impl Gui {
    /// Add Point: a point on the selected floor, its height typed relative to the surface there.
    fn shape_point_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(floor) = self.line_tool.as_ref().and_then(|t| t.target) else {
            return;
        };
        let dialog = TextEntryDialog::builder(
            &frame,
            "Height from the floor's surface here (negative lowers it, e.g. -50 for a drain):",
            "Add Point",
        )
        .with_default_value("0")
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let typed = dialog.get_value().unwrap_or_default();
        let offset = match self.run(
            "units.parse",
            json!({ "text": typed, "data_type": "length" }),
        ) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(_) => return frame.set_status_text(&format!("Invalid value: {typed}"), 0),
        };
        match self.run(
            "shape.add_point",
            json!({ "element_id": floor, "xy": p, "mode": { "along_surface": { "offset": offset } } }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Shape Editing › Reset Shape on the selected floors.
    pub(crate) fn reset_shape(&mut self) {
        let frame = self.frame;
        for id in self.selection() {
            if let Err(e) = self.run("shape.reset", json!({ "element_id": id })) {
                frame.set_status_text(&e, 0);
            }
        }
        self.refresh_scene(&frame);
    }
}

impl Gui {
    /// Split with Gap: the wall under the click, cut there with a typed gap.
    fn split_gap_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(panel) = self.active_view().map(|v| v.panel) else {
            return;
        };
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(wall) = self.element_at(x, y, &panel).filter(|w| {
            self.store_now().is_some_and(|s| {
                s.get(*w)
                    .and_then(reviera_core::walls::wall_recipe)
                    .is_some()
            })
        }) else {
            return frame.set_status_text("Click a wall", 0);
        };
        let dialog =
            TextEntryDialog::builder(&frame, "Joint Gap (1.6 to 305 mm):", "Split with Gap")
                .with_default_value("20")
                .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let typed = dialog.get_value().unwrap_or_default();
        let gap = match self.run(
            "units.parse",
            json!({ "text": typed, "data_type": "length" }),
        ) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(_) => return frame.set_status_text(&format!("Invalid value: {typed}"), 0),
        };
        match self.run(
            "element.split_with_gap",
            json!({ "element_id": wall, "point": p, "gap": gap }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Modify | Walls › Split Horizontally: the selected wall at a level it passes, or a height.
    pub(crate) fn split_horizontal(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(wall) = self.selection().into_iter().find(|w| {
            store
                .get(*w)
                .and_then(reviera_core::walls::wall_recipe)
                .is_some()
        }) else {
            return frame.set_status_text("Select a wall first", 0);
        };
        let Ok(g) = reviera_core::walls::regenerate(&store, store.get(wall).expect("selected"))
        else {
            return;
        };
        let (lo, hi) = (g.base_z, g.base_z + g.height);
        let levels: Vec<(ElementId, String)> = reviera_core::levels::list(&store)
            .into_iter()
            .filter(|l| l.elevation > lo + 0.01 && l.elevation < hi - 0.01)
            .map(|l| (l.id, l.name))
            .collect();
        let mut labels: Vec<String> = levels.iter().map(|l| format!("At {}", l.1)).collect();
        labels.push("At a height…".to_owned());
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Split the wall:", "Split Horizontally", &refs)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let k = usize::try_from(d.get_selection()).unwrap_or(usize::MAX);
        let args = if let Some((level, _)) = levels.get(k) {
            json!({ "wall_id": wall, "level_id": level })
        } else {
            let t =
                TextEntryDialog::builder(&frame, "Split at the elevation:", "Split Horizontally")
                    .with_default_value(&self.format_length((lo + hi) / 2.0))
                    .build();
            if t.show_modal() != ID_OK {
                return;
            }
            let typed = t.get_value().unwrap_or_default();
            match self.run(
                "units.parse",
                json!({ "text": typed, "data_type": "length" }),
            ) {
                Ok(v) => json!({ "wall_id": wall, "z": v.as_f64().unwrap_or_default() }),
                Err(_) => return frame.set_status_text(&format!("Invalid value: {typed}"), 0),
            }
        };
        match self.run("wall.split_horizontal", args) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}

impl Gui {
    /// A straight wall's Cross-Section rows (ARC-WALL-005).
    pub(crate) fn wall_cross_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        use reviera_core::walls::cross_section::CrossSection;
        let Some(r) = self
            .store_now()
            .and_then(|s| s.get(id).and_then(reviera_core::walls::wall_recipe))
        else {
            return;
        };
        let stacked = self.store_now().is_some_and(|s| {
            s.get(id)
                .is_some_and(|e| reviera_core::walls::stacked::of_wall(&s, e).is_some())
        });
        if !r.through.is_empty() || stacked {
            return;
        }
        grid.append(Property::category("Cross-Section", "c_wc"));
        let (mode, a, e, i) = match r.cross_section {
            None => (0, 0.0, 0.0, 0.0),
            Some(CrossSection::Slanted { angle }) => (1, angle, 0.0, 0.0),
            Some(CrossSection::Tapered { exterior, interior }) => (2, 0.0, exterior, interior),
        };
        grid.append(Property::enumeration(
            "Cross-Section",
            "wc_mode",
            vec![
                ("Vertical".to_owned(), 0),
                ("Slanted".to_owned(), 1),
                ("Tapered".to_owned(), 2),
            ],
            mode,
        ));
        let deg = |v: f64| format!("{:.2}°", v.to_degrees());
        if mode == 1 {
            grid.append(Property::string("Angle From Vertical", "wc_angle", deg(a)));
        }
        if mode == 2 {
            grid.append(Property::string("Exterior Angle", "wc_ext", deg(e)));
            grid.append(Property::string("Interior Angle", "wc_int", deg(i)));
        }
    }

    pub(crate) fn wall_cross_edit(&mut self, property: &str, text: Option<String>) -> bool {
        use reviera_core::walls::cross_section::CrossSection;
        let Some(row) = property.strip_prefix("wc_") else {
            return false;
        };
        let text = text.unwrap_or_default();
        let ids = self.selection();
        let Some(&id) = ids.first() else { return true };
        let current = self
            .store_now()
            .and_then(|s| s.get(id).and_then(reviera_core::walls::wall_recipe))
            .and_then(|r| r.cross_section);
        let angle = |t: &str| {
            t.trim()
                .trim_end_matches('°')
                .trim()
                .parse::<f64>()
                .ok()
                .map(f64::to_radians)
        };
        let (mut a, mut e, mut i) = match current {
            Some(CrossSection::Slanted { angle }) => (angle, 0.0, 0.0),
            Some(CrossSection::Tapered { exterior, interior }) => (0.0, exterior, interior),
            None => (0.0, 0.0, 0.0),
        };
        let mut mode = match current {
            None => "vertical",
            Some(CrossSection::Slanted { .. }) => "slanted",
            Some(CrossSection::Tapered { .. }) => "tapered",
        };
        match row {
            "mode" => {
                mode = match text.as_str() {
                    "Slanted" => "slanted",
                    "Tapered" => "tapered",
                    _ => "vertical",
                }
            }
            "angle" => a = angle(&text).unwrap_or(a),
            "ext" => e = angle(&text).unwrap_or(e),
            "int" => i = angle(&text).unwrap_or(i),
            _ => return true,
        }
        let frame = self.frame;
        match self.run(
            "wall.set_cross_section",
            json!({ "wall_ids": ids, "mode": mode, "angle": a, "exterior_angle": e, "interior_angle": i }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(err) => frame.set_status_text(&err, 0),
        }
        self.properties_stale = true;
        true
    }
}

impl Gui {
    /// A layered wall's extension and end-wrapping rows (ARC-CMPD-004, ARC-WRAP-001).
    pub(crate) fn wall_layer_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        use reviera_core::walls::layered::EndWrap;
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let Some(r) = reviera_core::walls::wall_recipe(e) else {
            return;
        };
        let Some(t) = e
            .param("@type")
            .and_then(|v| v.as_str())
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|t| store.get(t))
        else {
            return;
        };
        if !reviera_core::compound::is_layered(t) || !r.through.is_empty() {
            return;
        }
        let Some(c) = reviera_core::compound::of_wall_type(t) else {
            return;
        };
        grid.append(Property::category("Layers", "c_wl"));
        let top = c.layers.iter().any(|l| l.extend_top);
        let base = c.layers.iter().any(|l| l.extend_base);
        grid.append(Property::string(
            "Top Extension Distance",
            "wl_top",
            self.format_length(r.extension[1]),
        ));
        if !top || r.attach_top.is_some() {
            grid.set_property_read_only("wl_top", true);
        }
        grid.append(Property::string(
            "Base Extension Distance",
            "wl_base",
            self.format_length(r.extension[0]),
        ));
        if !base {
            grid.set_property_read_only("wl_base", true);
        }
        let modes = || {
            vec![
                ("By Type".to_owned(), 0),
                ("Exterior".to_owned(), 1),
                ("Interior".to_owned(), 2),
                ("None".to_owned(), 3),
            ]
        };
        let index = |w: EndWrap| match w {
            EndWrap::ByType => 0,
            EndWrap::Exterior => 1,
            EndWrap::Interior => 2,
            EndWrap::None => 3,
        };
        grid.append(Property::enumeration(
            "Wrapping at Start",
            "wl_wrap0",
            modes(),
            index(r.end_wrap[0]),
        ));
        grid.append(Property::enumeration(
            "Wrapping at End",
            "wl_wrap1",
            modes(),
            index(r.end_wrap[1]),
        ));
    }

    pub(crate) fn wall_layer_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(row) = property.strip_prefix("wl_") else {
            return false;
        };
        let text = text.unwrap_or_default();
        let ids = self.selection();
        let Some(&id) = ids.first() else { return true };
        let result = match row {
            "top" | "base" => match self
                .run(
                    "units.parse",
                    json!({ "text": text, "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
            {
                Some(v) => self.run("wall.set_extension", json!({ "wall_ids": ids, row: v })),
                None => Err("Type a length".to_owned()),
            },
            "wrap0" | "wrap1" => {
                let mode = match text.as_str() {
                    "Exterior" => "exterior",
                    "Interior" => "interior",
                    "None" => "none",
                    _ => "by_type",
                };
                self.run(
                    "wall.set_end_wrap",
                    json!({ "wall_id": id, "end": if row == "wrap0" { "start" } else { "end" }, "mode": mode }),
                )
            }
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
}
