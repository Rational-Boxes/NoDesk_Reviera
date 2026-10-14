// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Footprint grips (ARC-SKCH-001): a selected roof, floor or ceiling in a plan shows a grip at
//! each corner and at the middle of each edge. Dragging an edge moves it across (its neighbours
//! stretch to meet it); dragging a corner moves it. Releasing changes the footprint in one
//! undoable step, without opening the sketch.

use reviera_core::ElementId;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;

use super::Gui;
use crate::sketch::{Curve, SketchDrag, SketchKind, curves_json, edit_command};
use crate::sketch_edit::{move_corner, move_curve};

const GRIP_PX: f32 = 8.0;
const GHOST: [f32; 4] = [0.95, 0.45, 0.9, 1.0];

/// A footprint drag in progress: the element, what moves, and where the drag started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootprintDrag {
    pub element: ElementId,
    pub what: SketchDrag,
    pub from: [f64; 2],
}

impl Gui {
    /// The selected roof, floor or ceiling whose footprint shows grips, with its curves.
    pub(crate) fn footprint(&mut self) -> Option<(ElementId, SketchKind, Vec<Curve>)> {
        if !self.in_plan() || self.any_tool() {
            return None;
        }
        let [id] = self.selection()[..] else {
            return None;
        };
        let (kind, _, curves) = self.element_curves(id)?;
        Some((id, kind, curves))
    }

    /// The grips of a footprint: its corners, and the middle of each edge with the edge's index.
    pub(crate) fn footprint_grips(curves: &[Curve]) -> Vec<(SketchDrag, [f64; 2])> {
        let mut out: Vec<(SketchDrag, [f64; 2])> = Vec::new();
        for c in curves {
            if !out.iter().any(|(_, p)| *p == c.line.start) {
                out.push((SketchDrag::Corner(c.line.start), c.line.start));
            }
        }
        for (i, c) in curves.iter().enumerate() {
            // On a curved edge, the grip sits on the curve.
            let pts = c.points();
            let m = if c.is_curved() {
                pts[pts.len() / 2]
            } else {
                [
                    (c.line.start[0] + c.line.end[0]) / 2.0,
                    (c.line.start[1] + c.line.end[1]) / 2.0,
                ]
            };
            out.push((SketchDrag::Curve(i), m));
        }
        out
    }

    /// The footprint grip under pixel (`x`, `y`).
    pub(crate) fn footprint_grip_at(&mut self, x: i32, y: i32) -> Option<FootprintDrag> {
        let (element, _, curves) = self.footprint()?;
        let z = self.active_view()?.plan.as_ref()?.range.cut as f32;
        let (what, at) = Self::footprint_grips(&curves)
            .into_iter()
            .map(|(w, p)| (w, p, self.pixel_distance(x, y, [p, p], z)))
            .filter(|g| g.2 <= GRIP_PX)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(w, p, _)| (w, p))?;
        Some(FootprintDrag {
            element,
            what,
            from: at,
        })
    }

    /// The footprint's curves with the drag applied, to `to`.
    fn footprint_dragged(
        &self,
        d: FootprintDrag,
        to: [f64; 2],
    ) -> Option<(SketchKind, Vec<Curve>)> {
        let (kind, _, mut curves) = self.element_curves(d.element)?;
        match d.what {
            SketchDrag::Curve(i) => {
                move_curve(&mut curves, i, [to[0] - d.from[0], to[1] - d.from[1]]);
            }
            SketchDrag::Corner(c) => move_corner(&mut curves, c, to),
        }
        Some((kind, curves))
    }

    fn footprint_point(&self, d: FootprintDrag, x: i32, y: i32) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        self.snapped_at(x, y, Some(d.from), &lines).map(|s| s.point)
    }

    /// Dragging: the footprint as it would be, drawn over the plan.
    pub(crate) fn footprint_drag_motion(&mut self, d: FootprintDrag, x: i32, y: i32) {
        let Some(to) = self.footprint_point(d, x, y) else {
            return;
        };
        let Some((_, curves)) = self.footprint_dragged(d, to) else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let lines = Lines {
            color: GHOST,
            width: 0.5,
            segments: curves
                .iter()
                .flat_map(|c| {
                    c.points()
                        .windows(2)
                        .map(|w| {
                            [
                                [w[0][0] as f32, w[0][1] as f32, z],
                                [w[1][0] as f32, w[1][1] as f32, z],
                            ]
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Releasing: the element takes the new footprint (one undoable step).
    pub(crate) fn footprint_drag_end(&mut self, d: FootprintDrag, x: i32, y: i32, click: bool) {
        let frame = self.frame;
        if !click
            && let Some(to) = self.footprint_point(d, x, y)
            && let Some((kind, curves)) = self.footprint_dragged(d, to)
        {
            let (command, args) = edit_command(kind, d.element, curves_json(kind, &curves));
            if let Err(e) = self.run(command, args) {
                super::error(&frame, &format!("The footprint was not changed:\n{e}"));
            }
            self.refresh_scene(&frame);
            self.properties_stale = true;
        }
        self.update_grips();
    }
}
