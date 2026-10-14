// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Railings in the window (ARC-RAIL-001):
//! - **Railing (Sketch Path)**: in a plan, click the path's points; clicking the first point
//!   closes it, and Esc finishes an open path (`railing.create`).
//! - **Guard Edges** (Modify | Floors and Modify | Roofs): railings along every open edge of the
//!   selected floors, decks and flat roofs (`railing.create_on_edges`).
//! - **Railings** (Modify | Stairs): railings on both sides of the selected stairs, following
//!   their nosing lines and landings (`railing.create_on_stair`; new stairs get them anyway).
//! - **Opening**, **Gate** and **Double Gate** (Modify | Railings, ARC-RAIL-003): with a railing
//!   selected, each click on it (in a plan, or on the railing in 3D) adds an opening or a gate
//!   there (`railing.add_opening`, `railing.add_gate`). Esc ends the tool. Deleting a gate leaves
//!   its opening.

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

/// What the railing feature tool adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
    Opening,
    Gate { leaves: u8 },
}

/// The railing Opening / Gate tool: the railing, and what each click adds to it.
#[derive(Debug, Clone, Copy)]
pub struct FeatureTool {
    railing: ElementId,
    kind: FeatureKind,
}

impl Gui {
    /// Modify | Railings › Opening, Gate or Double Gate.
    pub(crate) fn start_feature_tool(&mut self, kind: FeatureKind) {
        let frame = self.frame;
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some(railing) = self.selection().into_iter().find(|id| {
            store
                .get(*id)
                .and_then(reviera_core::railings::railing_recipe)
                .is_some()
        }) else {
            return frame.set_status_text("Select the railing to add openings or gates to", 0);
        };
        self.end_tool(&frame);
        self.feature_tool = Some(FeatureTool { railing, kind });
        let what = match kind {
            FeatureKind::Opening => "an opening",
            FeatureKind::Gate { leaves: 1 } => "a gate",
            FeatureKind::Gate { .. } => "a double gate",
        };
        frame.set_status_text(
            &format!(
                "Click the railing where {what} goes (in a plan, or on the railing in 3D); Esc ends"
            ),
            0,
        );
    }

    pub(crate) fn end_feature_tool(&mut self) -> bool {
        self.feature_tool.take().is_some()
    }

    pub(crate) fn feature_click(&mut self, x: i32, y: i32, panel: &wxdragon::prelude::Panel) {
        let frame = self.frame;
        let Some(t) = self.feature_tool else { return };
        let Some(p) = self.model_point(x, y, panel) else {
            return frame.set_status_text("Click on the railing", 0);
        };
        let result = match t.kind {
            FeatureKind::Opening => self.run(
                "railing.add_opening",
                json!({ "railing_id": t.railing, "point": p }),
            ),
            FeatureKind::Gate { leaves } => self.run(
                "railing.add_gate",
                json!({ "railing_id": t.railing, "point": p, "leaves": leaves }),
            ),
        };
        match result {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Added; click for another, or Esc", 0);
            }
            Err(e) => error(&frame, &format!("Nothing was added:\n{e}")),
        }
    }

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

    /// Modify | Stairs › Railings: railings on both sides of the selected stairs.
    pub(crate) fn rail_stairs(&mut self) {
        let frame = self.frame;
        let selected = self.selection();
        let stairs: Vec<ElementId> = {
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
                    store
                        .get(*id)
                        .is_some_and(|e| reviera_core::stairs::stair_recipe(e).is_some())
                })
                .collect()
        };
        if stairs.is_empty() {
            return frame.set_status_text("Select the stairs to put railings on", 0);
        }
        let mut n = 0;
        for s in stairs {
            match self.run("railing.create_on_stair", json!({ "stair_id": s })) {
                Ok(v) => n += v.as_array().map_or(0, Vec::len),
                Err(e) => return error(&frame, &format!("No railings were placed:\n{e}")),
            }
        }
        self.refresh_scene(&frame);
        frame.set_status_text(&format!("Placed {n} railing(s) on the stairs"), 0);
    }
}
