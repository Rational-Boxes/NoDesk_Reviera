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
use reviera_render::annotation::{Dimension, dimension_lines};
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
struct Candidate {
    arg: Value,
    point: [f64; 2],
    direction: [f64; 2],
    /// The segment to highlight.
    segment: [[f64; 2]; 2],
}

/// The Dimension tool.
#[derive(Debug, Default)]
pub struct DimTool {
    picked: Vec<Candidate>,
    hover: Option<Candidate>,
    cursor: Option<[f64; 2]>,
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

impl Gui {
    /// Formats a length in project units.
    fn format_length(&mut self, v: f64) -> String {
        self.run("units.format", json!({ "value": v, "data_type": "length" }))
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{v:.3}"))
    }

    /// The lines of the dimensions of view `index` (drawn with the plan).
    pub(crate) fn annotation_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let (Some(element), Some(plan)) = (v.element, v.plan.clone()) else {
            return Vec::new();
        };
        let doc = v.doc;
        let Some(store) = self.app.document(doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let Some(view) = store.get(element) else {
            return Vec::new();
        };
        let z = plan.range.cut as f32 + 0.002;
        let mut normal = Lines {
            color: DIM_COLOR,
            segments: Vec::new(),
        };
        let mut orphaned = Lines {
            color: ORPHAN_COLOR,
            segments: Vec::new(),
        };
        for (_, data) in annotations::of_view(&store, view.guid().as_str()) {
            let Some(m) = annotations::measure(&store, &data) else {
                continue;
            };
            let texts = m.values.iter().map(|v| self.format_length(*v)).collect();
            let d = Dimension {
                points: m.points.clone(),
                along: m.along,
                line: data.line,
                texts,
            };
            let target = if m.orphaned.iter().any(|o| *o) {
                &mut orphaned
            } else {
                &mut normal
            };
            target.segments.extend(dimension_lines(&d, plan.scale, z));
        }
        vec![normal, orphaned]
    }

    // ---- The tool ----

    /// Starts the Dimension tool (plans only).
    pub(crate) fn start_dim_tool(&mut self, frame: &Frame) {
        if !self.in_plan() {
            return frame.set_status_text(
                "Dimensions are placed in plan views (View › New Floor Plan)",
                0,
            );
        }
        let f = self.frame;
        self.end_tool(&f);
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
    fn plan_xy(&self, x: i32, y: i32) -> Option<[f64; 2]> {
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        let (o, _) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        Some([f64::from(o.x), f64::from(o.y)])
    }

    /// Pixel distance from (`x`, `y`) to a plan segment.
    fn pixel_distance(&self, x: i32, y: i32, seg: [[f64; 2]; 2], z: f32) -> f32 {
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

    /// The reference under the cursor: Reviera wall faces and location lines on the plan's
    /// level, else cut edges; parallel to the first picked reference if there is one.
    fn dim_candidate(&self, x: i32, y: i32) -> Option<Candidate> {
        let v = self.active_view()?;
        let plan = v.plan.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let level_guid = store.get(plan.level)?.guid().to_string();
        let z = plan.range.cut as f32;
        let first = self.dim_tool.as_ref()?.picked.first().map(|c| c.direction);
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
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.003;
        let seg3 = |s: [[f64; 2]; 2]| s.map(|p| [p[0] as f32, p[1] as f32, z]);
        let mut lines = Lines {
            color: PREVIEW_COLOR,
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
            lines.segments.extend(dimension_lines(&d, plan.scale, z));
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }
}
