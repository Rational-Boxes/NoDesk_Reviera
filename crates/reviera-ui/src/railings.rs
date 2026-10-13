// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Railings in the window (ARC-RAIL-001):
//! - **Railing (Sketch Path)**: in a plan, click the path's points; clicking the first point
//!   closes it, and Esc finishes an open path (`railing.create`).
//! - **Guard Edges** (Modify | Floors and Modify | Roofs): railings along every open edge of the
//!   selected floors, decks and flat roofs (`railing.create_on_edges`).

use reviera_core::ElementId;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.85, 0.0, 0.75, 1.0];

/// The Railing tool: the level and the path so far.
#[derive(Debug, Clone)]
pub struct RailingTool {
    level: ElementId,
    points: Vec<[f64; 2]>,
    cursor: Option<[f64; 2]>,
}

impl Gui {
    /// Architecture › Railing.
    pub(crate) fn start_railing_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return;
        };
        self.end_tool(&frame);
        self.railing_tool = Some(RailingTool {
            level,
            points: Vec::new(),
            cursor: None,
        });
        frame.set_status_text(
            "Railing: click the path's points; click the first point to close it, Esc to finish",
            0,
        );
    }

    pub(crate) fn end_railing_tool(&mut self) -> bool {
        if self.railing_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    pub(crate) fn railing_motion(&mut self, x: i32, y: i32) {
        let from = self
            .railing_tool
            .as_ref()
            .and_then(|t| t.points.last().copied());
        let lines = self.snap_lines();
        let p = self.snapped_at(x, y, from, &lines).map(|s| s.point);
        if let Some(t) = &mut self.railing_tool {
            t.cursor = p;
        }
        self.railing_preview();
    }

    pub(crate) fn railing_click(&mut self, x: i32, y: i32) {
        let from = self
            .railing_tool
            .as_ref()
            .and_then(|t| t.points.last().copied());
        let lines = self.snap_lines();
        let Some(p) = self.snapped_at(x, y, from, &lines).map(|s| s.point) else {
            return;
        };
        let Some(t) = &mut self.railing_tool else {
            return;
        };
        let closes =
            t.points.len() >= 3 && (p[0] - t.points[0][0]).hypot(p[1] - t.points[0][1]) < 1e-6;
        if closes {
            return self.finish_railing(true);
        }
        if t.points.last() != Some(&p) {
            t.points.push(p);
        }
        self.railing_preview();
    }

    /// Esc: an open path of two or more points becomes a railing; otherwise the tool ends.
    pub(crate) fn railing_escape(&mut self) {
        if self
            .railing_tool
            .as_ref()
            .is_some_and(|t| t.points.len() >= 2)
        {
            self.finish_railing(false);
        } else {
            self.end_railing_tool();
            self.frame.set_status_text("Railing cancelled", 0);
        }
    }

    fn finish_railing(&mut self, closed: bool) {
        let frame = self.frame;
        let Some(t) = self.railing_tool.take() else {
            return;
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        match self.run(
            "railing.create",
            json!({ "path": t.points, "closed": closed, "base_level": t.level }),
        ) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Railing created", 0);
            }
            Err(e) => error(&frame, &format!("The railing was not created:\n{e}")),
        }
    }

    fn railing_preview(&mut self) {
        let Some(t) = self.railing_tool.clone() else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let mut pts = t.points.clone();
        if let Some(c) = t.cursor {
            pts.push(c);
        }
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: pts
                .windows(2)
                .map(|w| {
                    [
                        [w[0][0] as f32, w[0][1] as f32, z],
                        [w[1][0] as f32, w[1][1] as f32, z],
                    ]
                })
                .collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Modify | Floors / Roofs › Guard Edges: railings along the selection's open edges.
    pub(crate) fn guard_edges(&mut self) {
        let frame = self.frame;
        let selected = self.selection();
        let hosts: Vec<ElementId> = {
            let Some(store) = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store())
            else {
                return;
            };
            selected
                .into_iter()
                .filter(|id| {
                    store.get(*id).is_some_and(|e| {
                        reviera_core::floors::slab_recipe(e).is_some()
                            || reviera_core::roofs::roof_recipe(e).is_some()
                    })
                })
                .collect()
        };
        if hosts.is_empty() {
            return frame.set_status_text("Select floors, decks or flat roofs to guard", 0);
        }
        let mut n = 0;
        for h in hosts {
            match self.run("railing.create_on_edges", json!({ "host_id": h })) {
                Ok(v) => n += v.as_array().map_or(0, Vec::len),
                Err(e) => return error(&frame, &format!("No railings were placed:\n{e}")),
            }
        }
        self.refresh_scene(&frame);
        frame.set_status_text(&format!("Placed {n} railing(s) along the open edges"), 0);
    }
}
