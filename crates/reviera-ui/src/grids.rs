// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Grid tool (ARC-GRID-001) in plan views: click the start and the end of a grid line. Points
//! snap like the Wall tool's (ends and midpoints of grids and walls, cut edges, 15° directions
//! from the start); the preview shows the line with its bubble and the name it will get.

use reviera_render::RenderBackend;
use reviera_render::annotation::{GridMark, grid_lines};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, snap};

const PREVIEW_COLOR: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

/// The Grid tool.
#[derive(Debug, Default)]
pub struct GridTool {
    start: Option<[f64; 2]>,
    cursor: Option<snap::Snap>,
    /// Lines to snap to: grids, Reviera walls and the plan's cut edges.
    lines: Vec<([f64; 2], [f64; 2])>,
}

impl Gui {
    /// Starts the Grid tool (plans only).
    pub(crate) fn start_grid_tool(&mut self) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame
                .set_status_text("Grids are drawn in plan views (View › New Floor Plan)", 0);
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.leave_walk();
        self.grid_tool = Some(GridTool::default());
        self.update_grid_snaps();
        self.clear_selection();
        self.grid_prompt();
    }

    pub(crate) fn end_grid_tool(&mut self) -> bool {
        if self.grid_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Ready", 0);
        true
    }

    /// Esc: drops the start point, then ends the tool.
    pub(crate) fn grid_escape(&mut self) {
        match &mut self.grid_tool {
            Some(t) if t.start.is_some() => {
                t.start = None;
                self.grid_preview();
                self.grid_prompt();
            }
            Some(_) => {
                self.end_grid_tool();
            }
            None => {}
        }
    }

    fn update_grid_snaps(&mut self) {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
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
        if let Some(plan) = self.active_view().and_then(|v| v.plan.as_ref()) {
            let p = |q: [f32; 2]| [f64::from(q[0]), f64::from(q[1])];
            lines.extend(plan.cut_edges.iter().map(|(_, s)| (p(s[0]), p(s[1]))));
        }
        if let Some(t) = &mut self.grid_tool {
            t.lines = lines;
        }
    }

    fn grid_prompt(&mut self) {
        let Some(t) = &self.grid_tool else { return };
        let mut text = if t.start.is_none() {
            "Grid: click the start point".to_owned()
        } else {
            "Click the end point (the bubble goes at the end). Esc restarts.".to_owned()
        };
        if let Some(c) = t.cursor
            && c.kind != snap::SnapKind::None
        {
            text += &format!(" · {}", c.kind.label());
        }
        self.frame.set_status_text(&text, 0);
    }

    /// The snapped plan point under pixel (`x`, `y`).
    fn grid_snapped(&self, x: i32, y: i32) -> Option<snap::Snap> {
        let t = self.grid_tool.as_ref()?;
        let v = self.active_view()?;
        let z = v.plan.as_ref()?.range.cut as f32;
        let raw = self.plan_xy(x, y)?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let to_screen = |p: [f64; 2]| {
            v.camera
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let mut s = snap::snap(raw, (x as f32, y as f32), t.start, &t.lines, to_screen);
        if s.kind == snap::SnapKind::None {
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    pub(crate) fn grid_motion(&mut self, x: i32, y: i32) {
        let cursor = self.grid_snapped(x, y);
        if let Some(t) = &mut self.grid_tool {
            t.cursor = cursor;
        }
        self.grid_preview();
        self.grid_prompt();
    }

    pub(crate) fn grid_click(&mut self, x: i32, y: i32) {
        let Some(p) = self.grid_snapped(x, y) else {
            return;
        };
        let Some(t) = &mut self.grid_tool else { return };
        let Some(start) = t.start else {
            t.start = Some(p.point);
            self.grid_preview();
            return self.grid_prompt();
        };
        let args = json!({ "curve": { "line": { "start": start, "end": p.point } } });
        match self.run("grid.create", args) {
            Ok(_) => {
                if let Some(t) = &mut self.grid_tool {
                    t.start = None;
                }
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.update_grid_snaps();
                self.grid_preview();
                self.grid_prompt();
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
    }

    /// The snap marker and the grid being drawn, with the name it will get.
    fn grid_preview(&mut self) {
        let Some(t) = &self.grid_tool else { return };
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
            // A small square, 1.5 mm on paper.
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
        if let (Some(start), Some(c)) = (t.start, t.cursor) {
            let name = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| reviera_core::grids::auto_name(d.doc.store()))
                .unwrap_or_default();
            let mark = GridMark {
                start,
                end: c.point,
                name,
                bubbles: [false, true],
            };
            lines.segments.extend(grid_lines(&mark, plan.scale, z));
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }
}
