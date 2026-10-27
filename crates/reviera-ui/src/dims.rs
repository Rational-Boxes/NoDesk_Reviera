// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Aligned dimensions in plan views (DOC-ANN-001): drawing a view's dimensions and the
//! Dimension tool. The tool offers the reference under the cursor (a Reviera wall's faces and
//! location line first, then cut edges of any element); after the first reference only parallel
//! ones; a click in empty space places the dimension line through the cursor.

use glam::Vec2;
use reviera_core::ElementId;
use reviera_core::annotations;
use reviera_render::RenderBackend;
use reviera_render::annotation::{Dimension, GridMark, dimension_lines, grid_lines};
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

/// Candidates closer than this many pixels to the cursor are offered.
const PICK_PX: f32 = 8.0;
const DIM_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// Orphaned dimensions (a reference's element is gone, DOC-ANN-000).
const ORPHAN_COLOR: [f32; 4] = [0.95, 0.35, 0.0, 1.0];
const PREVIEW_COLOR: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

/// A reference the tool can pick: the `dim.create_aligned` argument and its line in plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    /// The reference argument (`dim.create_aligned` refs, `element.align`).
    pub arg: Value,
    pub point: [f64; 2],
    pub direction: [f64; 2],
    /// The segment to highlight.
    pub segment: [[f64; 2]; 2],
}

/// The Dimension tool.
#[derive(Debug, Default)]
pub struct DimTool {
    picked: Vec<Candidate>,
    hover: Option<Candidate>,
    cursor: Option<[f64; 2]>,
}

/// The grids of a model, ready to draw in their grid types' styles.
/// The grids a plan shows (view GlobalId, cut height), with its 2D extents, bubbles and elbows.
pub(crate) fn grid_marks(
    store: &reviera_core::Store,
    view: Option<&str>,
    cut: f64,
) -> Vec<(ElementId, GridMark)> {
    reviera_render::datums::plan_grids(store, view, cut)
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

impl Gui {
    /// The scale and drawing depth of view `index`'s annotation: a plan's at its cut, a section's
    /// or elevation's just in front of its view plane (its frame's z = 0). `None` for other
    /// views.
    pub(crate) fn annotation_space(&self, index: usize) -> Option<(f32, f32)> {
        let v = self.views.get(index)?;
        if let Some(p) = &v.plan {
            return Some((p.scale, p.range.cut as f32));
        }
        v.section.as_ref().map(|s| (s.scale, 0.0))
    }

    /// [`Gui::annotation_space`] of the active view.
    pub(crate) fn active_annotation_space(&self) -> Option<(f32, f32)> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        self.annotation_space(index)
    }

    /// Formats a length in project units.
    pub(crate) fn format_length(&mut self, v: f64) -> String {
        self.run("units.format", json!({ "value": v, "data_type": "length" }))
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{v:.3}"))
    }

    /// The lines of the dimensions of view `index` (drawn with the plan).
    pub(crate) fn annotation_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let (Some(element), Some((scale, z))) = (v.element, self.annotation_space(index)) else {
            return Vec::new();
        };
        let plan = v.plan.clone();
        let doc = v.doc;
        let Some(store) = self.app.document(doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let Some(view) = store.get(element) else {
            return Vec::new();
        };
        let z = z + 0.002;
        // Dimensions use annotation pen 1 (DOC-GFX-009).
        let width = reviera_core::standards::pen_set(&store).annotation_width(1) as f32;
        let mut normal = Lines {
            color: DIM_COLOR,
            width,
            segments: Vec::new(),
        };
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let mut highlighted = Lines {
            color: reviera_render::HIGHLIGHT,
            width,
            segments: Vec::new(),
        };
        let mut orphaned = Lines {
            color: ORPHAN_COLOR,
            width,
            segments: Vec::new(),
        };
        for (id, data) in annotations::of_view(&store, view.guid().as_str()) {
            let Some(m) = annotations::measure(&store, &data) else {
                continue;
            };
            let mut texts: Vec<String> = m.values.iter().map(|v| self.format_length(*v)).collect();
            // EQ segments read "EQ"; locked ones carry a padlock (ARC-CNST-001).
            if let Some(dim) = store.get(id) {
                let cons = reviera_core::constraints::of_dimension(&store, dim.guid().as_str());
                if cons
                    .iter()
                    .any(|(_, c)| c.kind == reviera_core::constraints::Kind::Equality)
                {
                    texts.iter_mut().for_each(|t| *t = "EQ".to_owned());
                }
                for (_, c) in &cons {
                    if let Some(i) = c.segment
                        && let Some(t) = texts.get_mut(i)
                    {
                        t.push_str(" \u{1F512}");
                    }
                }
            }
            let d = Dimension {
                points: m.points.clone(),
                along: m.along,
                line: data.line,
                texts,
            };
            let target = if selected.contains(&id) {
                &mut highlighted
            } else if m.orphaned.iter().any(|o| *o) {
                &mut orphaned
            } else {
                &mut normal
            };
            target.segments.extend(dimension_lines(&d, scale, z));
        }
        // Sections and elevations draw their grids with the model.
        let Some(plan) = plan else {
            return vec![normal, orphaned, highlighted];
        };
        // Grids show in every plan, in their types' styles; selected ones highlighted.
        let mut grids = Vec::new();
        for (id, mark) in grid_marks(&store, Some(view.guid().as_str()), plan.range.cut) {
            for mut l in grid_lines(&mark, plan.scale, z) {
                if selected.contains(&id) {
                    l.color = reviera_render::HIGHLIGHT;
                }
                grids.push(l);
            }
        }
        let mut out = vec![normal, orphaned];
        out.extend(grids);
        out.push(highlighted);
        // Multi-rebar annotations and bending details (STR-RBR-018).
        out.extend(self.rebar_annotation_lines(index, &selected).0);
        out
    }

    /// The grid under pixel (`x`, `y`) of the active plan, if any (its line or a bubble).
    pub(crate) fn grid_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let plan = v.plan.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let z = plan.range.cut as f32;
        let view = v
            .element
            .and_then(|e| store.get(e))
            .map(|e| e.guid().to_string());
        let mut best: Option<(f32, ElementId)> = None;
        for (id, mut mark) in grid_marks(store, view.as_deref(), plan.range.cut) {
            mark.name.clear();
            for s in grid_lines(&mark, plan.scale, z)
                .into_iter()
                .flat_map(|l| l.segments)
            {
                let seg = [s[0], s[1]].map(|p| [f64::from(p[0]), f64::from(p[1])]);
                let dist = self.pixel_distance(x, y, seg, z);
                if dist <= PICK_PX && best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, id));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The extent of the selected annotation in the active view: in a plan, section or elevation,
    /// its notes', tags' and dimensions' outlines as drawn; in a 3D view, the points of their floating labels. `None`
    /// when no annotation is selected there.
    pub(crate) fn annotation_bounds(&self, selected: &std::collections::HashSet<ElementId>) -> Option<(glam::Vec3, glam::Vec3)> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let v = &self.views[index];
        let mut points: Vec<glam::Vec3> = Vec::new();
        if let Some((scale, z)) = self.annotation_space(index) {
            for n in self.view_notes(index).into_iter().filter(|n| selected.contains(&n.id)) {
                points.extend(n.parts.outline.iter().map(|p| glam::Vec3::new(p[0], p[1], z)));
            }
            let store = self.app.document(v.doc)?.doc.store();
            let view = store.get(v.element?)?;
            for (id, data) in annotations::of_view(store, view.guid().as_str()) {
                if !selected.contains(&id) {
                    continue;
                }
                let Some(m) = annotations::measure(store, &data) else { continue };
                let d = Dimension { points: m.points, along: m.along, line: data.line, texts: Vec::new() };
                for s in dimension_lines(&d, scale, z) {
                    points.extend(s.map(glam::Vec3::from));
                }
            }
        } else if let Some(labels) = self.tip_boxes.get(&v.id) {
            points.extend(labels.iter().filter(|(l, _)| selected.contains(&l.id)).map(|(l, _)| l.tip.anchor));
        }
        points.iter().fold(None, |acc, p| match acc {
            None => Some((*p, *p)),
            Some((lo, hi)) => Some((lo.min(*p), hi.max(*p))),
        })
    }

    /// The dimension under pixel (`x`, `y`) of the active plan, section or elevation, if any.
    pub(crate) fn dim_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let (scale, z) = self.active_annotation_space()?;
        let v = self.active_view()?;
        let store = self.app.document(v.doc)?.doc.store();
        let view = store.get(v.element?)?;
        let mut best: Option<(f32, ElementId)> = None;
        for (id, data) in annotations::of_view(store, view.guid().as_str()) {
            let Some(m) = annotations::measure(store, &data) else {
                continue;
            };
            // The text is not needed to hit the lines.
            let d = Dimension {
                points: m.points,
                along: m.along,
                line: data.line,
                texts: Vec::new(),
            };
            for s in dimension_lines(&d, scale, z) {
                let seg = [s[0], s[1]].map(|p| [f64::from(p[0]), f64::from(p[1])]);
                let dist = self.pixel_distance(x, y, seg, z);
                if dist <= PICK_PX && best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, id));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Deletes the selection (Delete key), as one undoable step.
    pub(crate) fn delete_selection(&mut self) {
        let ids = self.selection();
        if ids.is_empty() {
            return;
        }
        let frame = self.frame;
        if !self.confirm_delete(&ids) {
            return;
        }
        match self.run("element.delete", json!({ "element_ids": ids })) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("Deleted {n} element(s)"), 0);
                let _ = self.run("selection.clear", json!({}));
                self.refresh_scene(&frame);
                self.selection_changed();
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    // ---- The tool ----

    /// Starts the Dimension tool (plans only).
    pub(crate) fn start_dim_tool(&mut self, frame: &Frame) {
        if self.active_annotation_space().is_none() {
            return frame.set_status_text(
                "Dimensions are placed in plans, sections and elevations (View › New Floor Plan)",
                0,
            );
        }
        let f = self.frame;
        self.end_tool(&f);
        self.end_grid_tool();
        self.end_line_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        self.dim_tool = Some(DimTool::default());
        self.clear_selection();
        self.dim_prompt();
    }

    pub(crate) fn end_dim_tool(&mut self) -> bool {
        if self.dim_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Ready", 0);
        true
    }

    /// Esc in the tool: drops the picked references, then ends the tool.
    pub(crate) fn dim_escape(&mut self) {
        match &mut self.dim_tool {
            Some(t) if !t.picked.is_empty() => {
                t.picked.clear();
                self.dim_preview();
                self.dim_prompt();
            }
            Some(_) => {
                self.end_dim_tool();
            }
            None => {}
        }
    }

    fn dim_prompt(&mut self) {
        let Some(t) = &self.dim_tool else { return };
        let text = match t.picked.len() {
            0 => "Aligned Dimension: click a wall face, wall centreline or edge".to_owned(),
            1 => "Click the next parallel reference".to_owned(),
            n => format!(
                "{n} references: click another, or click in empty space to place the dimension line (Esc to restart)"
            ),
        };
        self.frame.set_status_text(&text, 0);
    }

    /// The plan point under pixel (`x`, `y`) of the active view.
    pub(crate) fn plan_xy(&self, x: i32, y: i32) -> Option<[f64; 2]> {
        if let Some(p) = crate::coords::typed() {
            return Some(p);
        }
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        let (o, _) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        Some([f64::from(o.x), f64::from(o.y)])
    }

    /// Pixel distance from (`x`, `y`) to a plan segment.
    pub(crate) fn pixel_distance(&self, x: i32, y: i32, seg: [[f64; 2]; 2], z: f32) -> f32 {
        let Some(v) = self.active_view() else {
            return f32::MAX;
        };
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let p = |q: [f64; 2]| {
            v.camera
                .project(glam::Vec3::new(q[0] as f32, q[1] as f32, z), w, h)
                .map(|(a, b)| Vec2::new(a, b))
        };
        let (Some(a), Some(b)) = (p(seg[0]), p(seg[1])) else {
            return f32::MAX;
        };
        let c = Vec2::new(x as f32, y as f32);
        let ab = b - a;
        let t = if ab.length_squared() < 1e-6 {
            0.0
        } else {
            ((c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        };
        (a + ab * t - c).length()
    }

    /// The reference under the cursor, parallel to the first picked one if there is one.
    fn dim_candidate(&self, x: i32, y: i32) -> Option<Candidate> {
        let first = self.dim_tool.as_ref()?.picked.first().map(|c| c.direction);
        self.reference_at(x, y, first)
    }

    /// The reference line under pixel (`x`, `y`) of the active plan, parallel to `first` if
    /// given: Reviera wall faces and location lines on the plan's level and grids, else cut
    /// edges.
    pub(crate) fn reference_at(
        &self,
        x: i32,
        y: i32,
        first: Option<[f64; 2]>,
    ) -> Option<Candidate> {
        let v = self.active_view()?;
        if v.section.is_some() {
            return self.section_reference_at(x, y, first);
        }
        let plan = v.plan.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let level_guid = store.get(plan.level)?.guid().to_string();
        let z = plan.range.cut as f32;
        let parallel = |d: [f64; 2]| first.is_none_or(|f| cross(f, d).abs() < 1e-3);
        let near = |seg: [[f64; 2]; 2]| self.pixel_distance(x, y, seg, z);
        let cursor = self.plan_xy(x, y)?;
        // Reviera walls: exterior face, location line, interior face.
        let mut best: Option<(f32, Candidate)> = None;
        for e in store.iter() {
            let Some(recipe) = reviera_core::walls::wall_recipe(e) else {
                continue;
            };
            if recipe.base_level != level_guid {
                continue;
            }
            let Ok(g) = reviera_core::walls::regenerate(store, e) else {
                continue;
            };
            let n = g.normal();
            let ext = if recipe.flip { -1.0 } else { 1.0 };
            for (sub, offset) in [
                ("face:ext", ext * g.width / 2.0),
                ("axis", 0.0),
                ("face:int", -ext * g.width / 2.0),
            ] {
                let shift = |p: [f64; 2]| [p[0] + n[0] * offset, p[1] + n[1] * offset];
                let seg = [shift(g.start), shift(g.end)];
                let d = near(seg);
                if d > PICK_PX
                    || !parallel(g.direction())
                    || best.as_ref().is_some_and(|b| b.0 <= d)
                {
                    continue;
                }
                // The picked point: the cursor projected onto the line.
                let dir = g.direction();
                let t = (cursor[0] - seg[0][0]) * dir[0] + (cursor[1] - seg[0][1]) * dir[1];
                let point = [seg[0][0] + dir[0] * t, seg[0][1] + dir[1] * t];
                best = Some((
                    d,
                    Candidate {
                        arg: json!({ "element": e.id(), "sub": sub, "point": point }),
                        point,
                        direction: dir,
                        segment: seg,
                    },
                ));
            }
        }
        // Grids, by their line.
        let view = v
            .element
            .and_then(|e| store.get(e))
            .map(|e| e.guid().to_string());
        for (id, g) in grid_marks(store, view.as_deref(), plan.range.cut) {
            let seg = [g.start, g.end];
            let (dx, dy) = (seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            let dir = [dx / len, dy / len];
            let d = near(seg);
            if d > PICK_PX || !parallel(dir) || best.as_ref().is_some_and(|b| b.0 <= d) {
                continue;
            }
            let t = (cursor[0] - seg[0][0]) * dir[0] + (cursor[1] - seg[0][1]) * dir[1];
            let point = [seg[0][0] + dir[0] * t, seg[0][1] + dir[1] * t];
            best = Some((
                d,
                Candidate {
                    arg: json!({ "element": id, "sub": "axis", "point": point }),
                    point,
                    direction: dir,
                    segment: seg,
                },
            ));
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        // Cut edges of other elements.
        for (element, seg) in plan.cut_edges.iter() {
            let seg = seg.map(|p| [f64::from(p[0]), f64::from(p[1])]);
            let (dx, dy) = (seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]);
            let len = dx.hypot(dy);
            if len < 1e-4 {
                continue;
            }
            let dir = [dx / len, dy / len];
            let d = near(seg);
            if d > PICK_PX || !parallel(dir) || best.as_ref().is_some_and(|b| b.0 <= d) {
                continue;
            }
            let t = (cursor[0] - seg[0][0]) * dir[0] + (cursor[1] - seg[0][1]) * dir[1];
            let point = [seg[0][0] + dir[0] * t, seg[0][1] + dir[1] * t];
            let element: Option<ElementId> = *element;
            best = Some((
                d,
                Candidate {
                    arg: json!({ "element": element, "sub": "line", "point": point, "direction": dir }),
                    point,
                    direction: dir,
                    segment: seg,
                },
            ));
        }
        best.map(|b| b.1)
    }

    /// The reference line under pixel (`x`, `y`) of the active section or elevation, parallel to
    /// `first` if given, in the view's frame: levels across the view; Reviera walls' bases and
    /// tops, and their faces, axes and ends where the view shows them as lines; grids crossing
    /// the view; else cut edges.
    fn section_reference_at(&self, x: i32, y: i32, first: Option<[f64; 2]>) -> Option<Candidate> {
        let v = self.active_view()?;
        let s = v.section.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let frame = s.def.frame();
        let cursor = self.plan_xy(x, y)?;
        let parallel = |d: [f64; 2]| first.is_none_or(|f| cross(f, d).abs() < 1e-3);
        let mut best: Option<(f32, Candidate)> = None;
        let offer = |best: &mut Option<(f32, Candidate)>, arg: Value, seg: [[f64; 2]; 2], dir: [f64; 2]| {
            let d = self.pixel_distance(x, y, seg, 0.0);
            if d > PICK_PX || !parallel(dir) || best.as_ref().is_some_and(|b| b.0 <= d) {
                return;
            }
            let t = (cursor[0] - seg[0][0]) * dir[0] + (cursor[1] - seg[0][1]) * dir[1];
            let point = [seg[0][0] + dir[0] * t, seg[0][1] + dir[1] * t];
            let mut arg = arg;
            arg["point"] = json!(point);
            *best = Some((d, Candidate { arg, point, direction: dir, segment: seg }));
        };
        let levels = reviera_core::levels::list(store);
        let (lo, hi) = levels.iter().fold((0.0f64, 3.0f64), |(a, b), l| (a.min(l.elevation), b.max(l.elevation + 3.0)));
        for l in &levels {
            offer(&mut best, json!({ "element": l.id, "sub": "level" }), [[0.0, l.elevation], [frame.width, l.elevation]], [1.0, 0.0]);
        }
        let x_of = |p: [f64; 2]| frame.to_local([p[0], p[1], 0.0]);
        for e in store.iter() {
            if reviera_core::walls::wall_recipe(e).is_none() {
                continue;
            }
            let Ok(g) = reviera_core::walls::regenerate(store, e) else { continue };
            // In the view's depth and width.
            let (a, b) = (x_of(g.start), x_of(g.end));
            // A wall the view crosses square on still shows its thickness.
            let (x0, x1) = ((a[0].min(b[0]) - g.width / 2.0).max(0.0), (a[0].max(b[0]) + g.width / 2.0).min(frame.width));
            if x1 < x0 || a[2].min(b[2]) > 1e-6 || a[2].max(b[2]) < -frame.far {
                continue;
            }
            let (z0, z1) = (g.base_z, g.base_z + g.height);
            for sub in ["base", "top", "face:ext", "axis", "face:int", "end:start", "end:end"] {
                let r = reviera_core::annotations::DimRef { element: Some(e.guid().to_string()), sub: sub.into(), point: [0.0, 0.0], direction: [1.0, 0.0] };
                let Some((p, dir)) = reviera_core::annotations::resolve_ref_in_section(store, &r, &frame) else { continue };
                let seg = if dir[1].abs() < 0.5 { [[x0, p[1]], [x1, p[1]]] } else { [[p[0], z0], [p[0], z1]] };
                offer(&mut best, json!({ "element": e.id(), "sub": sub }), seg, dir);
            }
        }
        for g in reviera_core::grids::list(store) {
            if let Some(gx) = frame.grid_crossing(g.curve.start, g.curve.end) {
                offer(&mut best, json!({ "element": g.id, "sub": "axis" }), [[gx, lo - 1.0], [gx, hi]], [0.0, 1.0]);
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        // Cut and projected edges of other elements, as free lines.
        for (element, seg) in s.cut_edges.iter() {
            let seg = seg.map(|p| [f64::from(p[0]), f64::from(p[1])]);
            let (dx, dy) = (seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]);
            let len = dx.hypot(dy);
            if len < 1e-4 {
                continue;
            }
            let dir = [dx / len, dy / len];
            let element: Option<ElementId> = *element;
            offer(&mut best, json!({ "element": element, "sub": "line", "direction": dir }), seg, dir);
        }
        best.map(|b| b.1)
    }

    /// Mouse movement in the tool: the hovered reference and the dimension preview.
    pub(crate) fn dim_motion(&mut self, x: i32, y: i32) {
        let hover = self.dim_candidate(x, y);
        let cursor = self.plan_xy(x, y);
        if let Some(t) = &mut self.dim_tool {
            t.hover = hover;
            t.cursor = cursor;
        }
        self.dim_preview();
    }

    /// A click in the tool: picks the reference under the cursor, or places the dimension.
    pub(crate) fn dim_click(&mut self, x: i32, y: i32) {
        let candidate = self.dim_candidate(x, y);
        let point = self.plan_xy(x, y);
        let Some(t) = &mut self.dim_tool else { return };
        match candidate {
            Some(c) if !t.picked.iter().any(|p| p.arg == c.arg) => {
                t.picked.push(c);
            }
            Some(_) => {}
            None if t.picked.len() >= 2 => {
                // In order across, so the segments follow one another.
                let along = t.picked[0].direction;
                let across = [-along[1], along[0]];
                let mut picked = t.picked.clone();
                picked.sort_by(|a, b| {
                    let pos = |c: &Candidate| c.point[0] * across[0] + c.point[1] * across[1];
                    pos(a).total_cmp(&pos(b))
                });
                let refs: Vec<Value> = picked.iter().map(|c| c.arg.clone()).collect();
                let view = self.active_view().and_then(|v| v.element);
                let args = json!({ "view_id": view, "refs": refs, "line_point": point });
                match self.run("dim.create_aligned", args) {
                    Ok(_) => {
                        if let Some(t) = &mut self.dim_tool {
                            t.picked.clear();
                        }
                        let frame = self.frame;
                        self.refresh_scene(&frame);
                    }
                    Err(e) => self.frame.set_status_text(&e, 0),
                }
            }
            None => {}
        }
        self.dim_preview();
        self.dim_prompt();
    }

    /// Draws the tool's state: picked references, the hovered one, and the dimension so far.
    fn dim_preview(&mut self) {
        let Some(t) = &self.dim_tool else { return };
        let Some((scale, z)) = self.active_annotation_space() else {
            return;
        };
        let z = z + 0.003;
        let seg3 = |s: [[f64; 2]; 2]| s.map(|p| [p[0] as f32, p[1] as f32, z]);
        let mut lines = Lines {
            color: PREVIEW_COLOR,
            width: 0.25,
            segments: Vec::new(),
        };
        for c in t.picked.iter().chain(t.hover.iter()) {
            lines.segments.push(seg3(c.segment));
        }
        // The dimension through the cursor, once two references are picked (or one and a
        // hovered parallel one).
        let mut refs: Vec<&Candidate> = t.picked.iter().collect();
        if let Some(h) = &t.hover
            && !refs.iter().any(|c| c.arg == h.arg)
        {
            refs.push(h);
        }
        let cursor = t.cursor;
        let dimension = (refs.len() >= 2).then(|| {
            let along = refs[0].direction;
            let across = [-along[1], along[0]];
            let mut points: Vec<[f64; 2]> = refs.iter().map(|c| c.point).collect();
            points.sort_by(|a, b| {
                (a[0] * across[0] + a[1] * across[1])
                    .total_cmp(&(b[0] * across[0] + b[1] * across[1]))
            });
            (points, along, across)
        });
        if let (Some((points, along, across)), Some(line)) = (dimension, cursor) {
            let pos = |p: [f64; 2]| p[0] * across[0] + p[1] * across[1];
            let values: Vec<f64> = points
                .windows(2)
                .map(|w| (pos(w[1]) - pos(w[0])).abs())
                .collect();
            let texts = values.iter().map(|v| self.format_length(*v)).collect();
            let d = Dimension {
                points,
                along,
                line,
                texts,
            };
            lines.segments.extend(dimension_lines(&d, scale, z));
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }
}
