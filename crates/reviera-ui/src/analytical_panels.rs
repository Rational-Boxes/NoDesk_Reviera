// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical panels in the window (STR-AM-002): Analyze › Analytical › Panel by Boundary (a plan
//! sketch), Panel by Extrusion and Curved Panel (clicked in plan, from the plan's level up to the
//! next), Opening (a sketch inside a panel), Split Panel (two clicks across a panel) and Cladding
//! (toggles the selected panels). Show Analytical draws panels' outlines and openings.

use reviera_core::ElementId;
use serde_json::json;

use super::Gui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    /// Two clicks: a straight profile.
    Extrude,
    /// Three clicks: start, a point on the arc, end.
    Curved,
    /// Two clicks: the split line.
    Split,
}

/// The panel click tool: its level, mode and the points clicked so far.
#[derive(Debug, Clone)]
pub struct PanelTool {
    level: ElementId,
    mode: PanelMode,
    points: Vec<[f64; 2]>,
}

impl Gui {
    /// Starts a panel click tool in the active plan.
    pub(crate) fn start_panel_tool(&mut self, mode: PanelMode) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Analytical panels are drawn in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.panel_tool = Some(PanelTool {
            level,
            mode,
            points: Vec::new(),
        });
        frame.set_status_text(
            match mode {
                PanelMode::Extrude => {
                    "Panel by Extrusion: click the profile's start, then its end; the panel rises from this level to the next. Esc ends the tool."
                }
                PanelMode::Curved => {
                    "Curved Panel: click the arc's start, a point on it, then its end; the panel rises from this level to the next. Esc ends the tool."
                }
                PanelMode::Split => {
                    "Split Panel: click two points of the line across a panel at this level. Esc ends the tool."
                }
            },
            0,
        );
    }

    pub(crate) fn end_panel_tool(&mut self) -> bool {
        self.panel_tool.take().is_some()
    }

    /// This level's elevation and the next level's (3 m above when it is the top one).
    fn level_span(&self, level: ElementId) -> (f64, f64) {
        let Some(store) = self.store_now() else {
            return (0.0, 3.0);
        };
        let z = store
            .get(level)
            .map(reviera_core::levels::elevation)
            .unwrap_or(0.0);
        let next = reviera_core::levels::list(&store)
            .into_iter()
            .map(|l| l.elevation)
            .filter(|e| *e > z + 1e-6)
            .fold(f64::INFINITY, f64::min);
        (z, if next.is_finite() { next } else { z + 3.0 })
    }

    pub(crate) fn panel_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.panel_tool.clone() else {
            return;
        };
        let Some(p) = self.beam_point(x, y, t.points.last().copied()) else {
            return;
        };
        let mut points = t.points.clone();
        points.push(p);
        let needed = if t.mode == PanelMode::Curved { 3 } else { 2 };
        if points.len() < needed {
            if let Some(t) = &mut self.panel_tool {
                t.points = points.clone();
            }
            let next = match (t.mode, points.len()) {
                (PanelMode::Curved, 1) => "click a point on the arc",
                (PanelMode::Curved, _) => "click the arc's end",
                (PanelMode::Split, _) => "click the line's second point",
                _ => "click the profile's end",
            };
            return frame.set_status_text(next, 0);
        }
        let (z0, z1) = self.level_span(t.level);
        let result = match t.mode {
            PanelMode::Extrude => self.run(
                "analytical_panel.create_by_extrusion",
                json!({ "profile": { "line": { "start": points[0], "end": points[1] } }, "elevation": z0, "depth": z1 - z0 }),
            ),
            PanelMode::Curved => self.run(
                "analytical_panel.create_by_extrusion",
                json!({ "profile": { "arc": { "start": points[0], "through": points[1], "end": points[2] } }, "elevation": z0, "depth": z1 - z0 }),
            ),
            PanelMode::Split => match self.panel_at(points[0], points[1], z0) {
                Some(panel) => self.run(
                    "analytical_panel.split",
                    json!({ "panel_id": panel, "line": [[points[0][0], points[0][1], z0], [points[1][0], points[1][1], z0]] }),
                ),
                None => Err("no horizontal analytical panel at this level under that line".into()),
            },
        };
        if let Some(t) = &mut self.panel_tool {
            t.points.clear();
        }
        match result {
            Ok(_) => {
                frame.set_status_text(
                    match t.mode {
                        PanelMode::Split => "Panel split; click the next line, or Esc",
                        _ => "Panel made; click the next one's start, or Esc",
                    },
                    0,
                );
                if !self.show_analytical {
                    self.toggle_analytical();
                }
                self.refresh_scene(&frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The horizontal panel at elevation `z` that the line's middle lies on.
    fn panel_at(&self, a: [f64; 2], b: [f64; 2], z: f64) -> Option<ElementId> {
        let store = self.store_now()?;
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, z];
        reviera_core::analytical_panels::panels(&store)
            .into_iter()
            .find(|(_, g)| g.contains(mid, 0.05))
            .map(|(id, _)| id)
    }

    /// Analyze › Analytical › Opening: sketch the opening in a plan; Finish puts it in the panel
    /// at this level it lies on.
    pub(crate) fn start_analytical_opening_sketch(&mut self) {
        self.start_floor_sketch();
        if let Some(s) = &mut self.sketch {
            s.analytical_opening = true;
            self.frame.set_status_text(
                "Analytical Opening: sketch the opening inside a panel at this level. Finish to make it.",
                0,
            );
        }
    }

    /// The opening sketch's command: the panel at the level that holds the loop.
    pub(crate) fn analytical_opening_args(
        &self,
        level: ElementId,
        ring: Vec<[f64; 2]>,
    ) -> Result<serde_json::Value, String> {
        let (z, _) = self.level_span(level);
        let mut ring: Vec<[f64; 3]> = ring.iter().map(|q| [q[0], q[1], z]).collect();
        if ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        let store = self.store_now().ok_or("open a model first")?;
        let n = ring.len().max(1) as f64;
        let c = [0, 1, 2].map(|k| ring.iter().map(|q| q[k]).sum::<f64>() / n);
        let panel = reviera_core::analytical_panels::panels(&store)
            .into_iter()
            .find(|(_, g)| g.contains(c, 0.05))
            .map(|(id, _)| id)
            .ok_or("the opening lies on no analytical panel at this level")?;
        Ok(json!({ "panel_id": panel, "boundary": ring }))
    }

    /// Analyze › Analytical › Cladding: toggles the selected panels' cladding flag.
    pub(crate) fn toggle_panel_cladding(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return;
        };
        let panels: Vec<(ElementId, bool)> = ids
            .iter()
            .filter_map(|id| {
                let r = reviera_core::analytical_panels::panel_recipe(store.get(*id)?)?;
                Some((*id, r.cladding))
            })
            .collect();
        if panels.is_empty() {
            return frame.set_status_text("Select analytical panels first", 0);
        }
        let mut done = 0;
        for (id, cladding) in &panels {
            match self.run(
                "analytical_panel.set_cladding",
                json!({ "panel_id": id, "cladding": !cladding }),
            ) {
                Ok(_) => done += 1,
                Err(e) => return frame.set_status_text(&e, 0),
            }
        }
        frame.set_status_text(
            &format!("{done} panel(s) toggled: cladding carries loads but adds no stiffness"),
            0,
        );
        self.refresh_scene(&frame);
    }
}

type Segments = Vec<[[f32; 3]; 2]>;

/// Panels' outlines and openings for Show Analytical, and the cladding panels' apart.
pub(crate) fn panel_segments(store: &reviera_core::Store) -> (Segments, Segments) {
    let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
    let ring = |pts: &[[f64; 3]], out: &mut Vec<[[f32; 3]; 2]>| {
        for i in 0..pts.len() {
            out.push([f(pts[i]), f(pts[(i + 1) % pts.len()])]);
        }
    };
    let (mut solid, mut cladding) = (Vec::new(), Vec::new());
    for (_, g) in reviera_core::analytical_panels::panels(store) {
        let out = if g.recipe.cladding {
            &mut cladding
        } else {
            &mut solid
        };
        ring(&g.outline, out);
        for o in &g.recipe.openings {
            ring(o, out);
        }
        // A diagonal marks the surface.
        if g.corners.len() >= 3 && g.circle.is_none() {
            out.push([f(g.corners[0]), f(g.corners[2])]);
        }
    }
    (solid, cladding)
}
