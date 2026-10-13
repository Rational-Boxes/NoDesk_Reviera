// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Stair tool (ARC-STAIR-002) and stair Properties (ARC-STAIR-005).
//!
//! In a plan, each run takes two clicks: its start and toward its end. A run holds as many
//! risers as its length allows at the tread depth, at most those left; the status bar counts
//! them down. The next click starts the next run, joined by an automatic landing. When every
//! riser is placed the stair is made; Esc makes it from the runs so far (or cancels when there
//! are none).

use reviera_core::ElementId;
use reviera_core::stairs::{self, risers_for_length};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.85, 0.0, 0.75, 1.0];
/// Width of new stair runs, metres.
const WIDTH: f64 = 1.2;
/// Height of a stair on the highest level (no level above to reach), metres.
const UNCONNECTED: f64 = 3.0;

/// The Stair tool's state.
#[derive(Debug, Clone)]
pub struct StairTool {
    level: ElementId,
    /// The level above, or `None` for an unconnected height.
    top: Option<ElementId>,
    risers: u32,
    tread: f64,
    /// Runs placed: start and end.
    runs: Vec<([f64; 2], [f64; 2], u32)>,
    start: Option<[f64; 2]>,
    cursor: Option<[f64; 2]>,
}

impl StairTool {
    fn left(&self) -> u32 {
        self.risers
            .saturating_sub(self.runs.iter().map(|r| r.2).sum::<u32>())
    }

    /// The run from `a` toward `b`: its end (on a whole tread) and risers.
    fn run(&self, a: [f64; 2], b: [f64; 2]) -> Option<([f64; 2], u32)> {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-6 {
            return None;
        }
        let k = risers_for_length(len, self.tread).min(self.left()).max(1);
        let l = f64::from(k - 1) * self.tread;
        Some(([a[0] + d[0] / len * l, a[1] + d[1] / len * l], k))
    }
}

impl Gui {
    /// Architecture › Stair.
    pub(crate) fn start_stair_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.cancel_sketch();
        self.end_ceiling_tool();
        self.end_room_tool();
        self.end_note_tool();
        let levels = self.levels();
        let top = levels
            .iter()
            .position(|l| l.id == plan.level)
            .and_then(|i| levels.get(i + 1))
            .map(|l| l.id);
        let Some(store) = self.doc.and_then(|d| self.app.document(d)) else {
            return;
        };
        let height = top.is_none().then_some(UNCONNECTED);
        let Some((risers, _)) =
            stairs::plan_risers(store.doc.store(), plan.level, top, height, None)
        else {
            return;
        };
        self.stair_tool = Some(StairTool {
            level: plan.level,
            top,
            risers,
            tread: stairs::StairType::default().min_tread,
            runs: Vec::new(),
            start: None,
            cursor: None,
        });
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        self.stair_prompt();
    }

    pub(crate) fn end_stair_tool(&mut self) -> bool {
        if self.stair_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    fn stair_prompt(&mut self) {
        let Some(t) = &self.stair_tool else { return };
        let left = t.left();
        let now = match (t.start, t.cursor) {
            (Some(a), Some(c)) => t.run(a, c).map_or(0, |r| r.1),
            _ => 0,
        };
        let text = match t.start {
            None if t.runs.is_empty() => format!(
                "Stair: click the start of the first run · {} risers to place (Esc cancels)",
                t.risers
            ),
            None => format!(
                "Stair: click the start of the next run (a landing joins them) · {left} risers left · Esc makes the stair now"
            ),
            Some(_) => format!(
                "Stair: click the end of the run · {now} risers in this run, {} left after it",
                left.saturating_sub(now)
            ),
        };
        self.frame.set_status_text(&text, 0);
    }

    pub(crate) fn stair_motion(&mut self, x: i32, y: i32) {
        let from = self.stair_tool.as_ref().and_then(|t| t.start);
        let lines = self.snap_lines();
        let p = self.snapped_at(x, y, from, &lines).map(|s| s.point);
        if let Some(t) = &mut self.stair_tool {
            t.cursor = p;
        }
        self.stair_preview();
        self.stair_prompt();
    }

    pub(crate) fn stair_click(&mut self, x: i32, y: i32) {
        let from = self.stair_tool.as_ref().and_then(|t| t.start);
        let lines = self.snap_lines();
        let Some(p) = self.snapped_at(x, y, from, &lines).map(|s| s.point) else {
            return;
        };
        let Some(t) = &mut self.stair_tool else {
            return;
        };
        match t.start {
            None => t.start = Some(p),
            Some(a) => {
                let Some((end, k)) = t.run(a, p) else { return };
                t.runs.push((a, end, k));
                t.start = None;
                if t.left() == 0 {
                    return self.finish_stair();
                }
            }
        }
        self.stair_preview();
        self.stair_prompt();
    }

    /// Esc: drops a run being drawn, else makes the stair from the runs so far.
    pub(crate) fn stair_escape(&mut self) {
        let Some(t) = &mut self.stair_tool else {
            return;
        };
        if t.start.take().is_some() {
            self.stair_preview();
            return self.stair_prompt();
        }
        if t.runs.is_empty() {
            self.end_stair_tool();
            return self.frame.set_status_text("Stair cancelled", 0);
        }
        self.finish_stair();
    }

    fn finish_stair(&mut self) {
        let frame = self.frame;
        let Some(t) = self.stair_tool.take() else {
            return;
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        let mut args = json!({
            "base_level": t.level,
            "width": WIDTH,
            "tread_depth": t.tread,
            "runs": t.runs.iter().map(|(a, b, k)| json!({ "start": a, "end": b, "risers": k })).collect::<Vec<_>>(),
        });
        match t.top {
            Some(l) => args["top"] = json!({ "level": l }),
            None => args["top"] = json!({ "unconnected": UNCONNECTED }),
        }
        match self.run("stair.create", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                if let Some(id) = v.as_u64() {
                    let _ = self.run("selection.set", json!({ "ids": [id] }));
                    self.selection_changed();
                }
                let note = if t.left() > 0 {
                    format!(
                        " — {} risers were not placed, so the top is not reached",
                        t.left()
                    )
                } else {
                    String::new()
                };
                frame.set_status_text(&format!("Stair created{note}"), 0);
            }
            Err(e) => error(&frame, &format!("The stair was not created:\n{e}")),
        }
    }

    /// The runs placed and the one being drawn, with their treads.
    fn stair_preview(&mut self) {
        let Some(t) = self.stair_tool.clone() else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let seg = |a: [f64; 2], b: [f64; 2]| {
            [[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]
        };
        let mut lines = Lines {
            color: PREVIEW,
            width: 0.35,
            segments: Vec::new(),
        };
        let mut runs = t.runs.clone();
        if let (Some(a), Some(c)) = (t.start, t.cursor)
            && let Some((end, k)) = t.run(a, c)
        {
            runs.push((a, end, k));
        }
        for (a, b, k) in runs {
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = d[0].hypot(d[1]);
            let u = if len > 1e-9 {
                [d[0] / len, d[1] / len]
            } else {
                [1.0, 0.0]
            };
            let n = [-u[1] * WIDTH / 2.0, u[0] * WIDTH / 2.0];
            let edge = |p: [f64; 2]| ([p[0] + n[0], p[1] + n[1]], [p[0] - n[0], p[1] - n[1]]);
            for i in 0..k {
                let s = f64::from(i) * t.tread;
                let (l, r) = edge([a[0] + u[0] * s, a[1] + u[1] * s]);
                lines.segments.push(seg(l, r));
            }
            let (l0, r0) = edge(a);
            let (l1, r1) = edge(b);
            lines.segments.push(seg(l0, l1));
            lines.segments.push(seg(r0, r1));
            // The path, with an arrow pointing up the stair.
            lines.segments.push(seg(a, b));
            let h = 0.25;
            let back = [b[0] - u[0] * h, b[1] - u[1] * h];
            lines
                .segments
                .push(seg(b, [back[0] - u[1] * h / 2.0, back[1] + u[0] * h / 2.0]));
            lines
                .segments
                .push(seg(b, [back[0] + u[1] * h / 2.0, back[1] - u[0] * h / 2.0]));
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Properties of a selected stair.
    pub(crate) fn stair_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(recipe) = stairs::stair_recipe(e) else {
            return false;
        };
        let level_name = |guid: &str| {
            reviera_core::GlobalId::parse(guid)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| store.get(l)?.name.clone())
                .unwrap_or_default()
        };
        grid.append(Property::category("Constraints", "c_stair"));
        grid.append(Property::string(
            "Base Level",
            "st_base",
            level_name(&recipe.base_level),
        ));
        grid.set_property_read_only("st_base", true);
        let top = match &recipe.top {
            stairs::StairTop::Level(g) => level_name(g),
            stairs::StairTop::Unconnected(h) => format!("Unconnected ({})", self.format_length(*h)),
        };
        grid.append(Property::string("Top", "st_top", top));
        grid.set_property_read_only("st_top", true);
        grid.append(Property::category("Dimensions", "c_stair_dims"));
        let desired = recipe
            .desired_risers
            .map_or_else(String::new, |n| n.to_string());
        grid.append(Property::string(
            "Desired Number of Risers",
            "st_risers",
            desired,
        ));
        grid.set_help_string("st_risers", "Empty takes the fewest the type allows");
        let tread = self.format_length(recipe.tread_depth);
        grid.append(Property::string("Actual Tread Depth", "st_tread", tread));
        let width = recipe
            .runs
            .first()
            .map(|r| self.format_length(r.width))
            .unwrap_or_default();
        grid.append(Property::string("Run Width", "st_width", width));
        match stairs::regenerate(&store, e) {
            Ok(g) => {
                let rows = [
                    ("Actual Number of Risers", g.placed.to_string()),
                    ("Actual Number of Treads", g.treads().to_string()),
                    ("Actual Riser Height", self.format_length(g.riser_height)),
                ];
                for (i, (label, value)) in rows.into_iter().enumerate() {
                    let key = format!("st_ro{i}");
                    grid.append(Property::string(label, &key, value));
                    grid.set_property_read_only(&key, true);
                }
                for (i, w) in g.warnings.iter().enumerate() {
                    let key = format!("st_warn{i}");
                    grid.append(Property::string("Warning", &key, w.clone()));
                    grid.set_property_read_only(&key, true);
                }
            }
            Err(err) => {
                grid.append(Property::string("Error", "st_err", err.message));
                grid.set_property_read_only("st_err", true);
            }
        }
        true
    }

    /// An edit of a stair row; `false` if `property` is not one.
    pub(crate) fn stair_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let ids: Vec<ElementId> = {
            let store = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store().clone());
            self.selection()
                .into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(stairs::stair_recipe)
                        .is_some()
                })
                .collect()
        };
        let args = match property {
            "st_risers" => {
                let n = if text.trim().is_empty() {
                    Ok(0)
                } else {
                    text.trim().parse::<u32>()
                };
                match n {
                    Ok(n) => json!({ "stair_ids": ids, "desired_risers": n }),
                    Err(_) => {
                        error(&frame, "The number of risers must be a whole number");
                        return true;
                    }
                }
            }
            "st_tread" => json!({ "stair_ids": ids, "tread_depth": text }),
            "st_width" => json!({ "stair_ids": ids, "width": text }),
            _ => return false,
        };
        if let Err(e) = self.run("stair.set_params", args) {
            error(&frame, &format!("The stair was not changed:\n{e}"));
        }
        self.refresh_scene(&frame);
        true
    }
}
