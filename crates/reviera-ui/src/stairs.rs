// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Stair tool (ARC-STAIR-002) and stair Properties (ARC-STAIR-005).
//!
//! In a plan, each run takes two clicks: its start and toward its end. A run holds as many
//! risers as its length allows at the tread depth, at most those left; the status bar counts
//! them down. The next click starts the next run, joined by an automatic landing. When every
//! riser is placed the stair is made; Esc makes it from the runs so far (or cancels when there
//! are none), the last run extending to the top (ARC-STAIR-007).
//!
//! A run drawn to within a tread of the length that places every riser left snaps to it: the
//! stair reaches the level above and is made on that click. Once made, a stair passing under a
//! floor or ceiling that leaves it short of headroom offers to open it, under the model's
//! building code or another one chosen there.

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
/// Width of a new spiral stair's treads, metres.
const SPIRAL_WIDTH: f64 = 1.0;
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
    /// The name of the level the stair reaches, for prompts.
    top_name: String,
    start: Option<[f64; 2]>,
    cursor: Option<[f64; 2]>,
    /// A spiral: the first click is the centre, the second a point on the walking line.
    spiral: bool,
    /// An L winder: the start, the walking line's corner, then the side it turns to.
    winder: bool,
    corner: Option<[f64; 2]>,
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
        let mut k = risers_for_length(len, self.tread).min(self.left()).max(1);
        // Within a tread of the top: it snaps to reach it.
        if k + 1 >= self.left() {
            k = self.left().max(1);
        }
        let l = f64::from(k - 1) * self.tread;
        Some(([a[0] + d[0] / len * l, a[1] + d[1] / len * l], k))
    }
}

impl Gui {
    /// Architecture › Stair.
    pub(crate) fn start_stair_tool(&mut self, spiral: bool) {
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
        self.end_line_tool();
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
        let top_name = top
            .and_then(|t| levels.iter().find(|l| l.id == t))
            .map_or_else(|| "its top".to_owned(), |l| l.name.clone());
        self.stair_tool = Some(StairTool {
            level: plan.level,
            top,
            top_name,
            risers,
            tread: stairs::StairType::default().min_tread,
            runs: Vec::new(),
            start: None,
            cursor: None,
            spiral,
            winder: false,
            corner: None,
        });
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        self.stair_prompt();
    }

    /// Architecture › Winder Stair (an L winder with every riser).
    pub(crate) fn start_winder_tool(&mut self) {
        self.start_stair_tool(false);
        if let Some(t) = &mut self.stair_tool {
            t.winder = true;
        }
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
            None if t.winder => format!(
                "Winder Stair: click the start of the walking line (the middle of the first riser) · {} risers (Esc cancels)",
                t.risers
            ),
            Some(_) if t.winder && t.corner.is_none() => {
                "Winder Stair: click the corner of the walking line".to_owned()
            }
            Some(_) if t.winder => {
                "Winder Stair: click the side it turns to; it places every riser".to_owned()
            }
            None if t.spiral => format!(
                "Spiral Stair: click the centre · {} risers (Esc cancels)",
                t.risers
            ),
            Some(_) if t.spiral => {
                "Spiral Stair: click a point on the walking line (the middle of the treads); it turns counter-clockwise going up".to_owned()
            }
            None if t.runs.is_empty() => format!(
                "Stair: click the start of the first run · {} risers to place (Esc cancels)",
                t.risers
            ),
            None => format!(
                "Stair: click the start of the next run (a landing joins them) · {left} risers left · Esc completes it: the last run extends to {}",
                t.top_name
            ),
            Some(_) if now >= left && left > 0 => format!(
                "Stair: reaches {} · {now} risers · click to complete the stair",
                t.top_name
            ),
            Some(_) => format!(
                "Stair: click the end of the run · {now} risers in this run, {} left after it (it snaps to {} within a tread)",
                left.saturating_sub(now),
                t.top_name
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
            Some(_) if t.winder && t.corner.is_none() => t.corner = Some(p),
            Some(a) if t.winder => {
                let c = t.corner.expect("checked");
                return self.finish_winder(a, c, p);
            }
            Some(c) if t.spiral => return self.finish_spiral(c, p),
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
        if t.corner.take().is_some() || t.start.take().is_some() {
            self.stair_preview();
            return self.stair_prompt();
        }
        if t.runs.is_empty() {
            self.end_stair_tool();
            return self.frame.set_status_text("Stair cancelled", 0);
        }
        self.finish_stair();
    }

    /// The end of an L winder from `start` round `corner`, turning toward `side`, holding
    /// every riser; `None` when the second leg would be too short.
    fn winder_end(
        t: &StairTool,
        start: [f64; 2],
        corner: [f64; 2],
        side: [f64; 2],
    ) -> Option<[f64; 2]> {
        let d = [corner[0] - start[0], corner[1] - start[1]];
        let leg1 = d[0].hypot(d[1]);
        if leg1 < 1e-6 {
            return None;
        }
        let u = [d[0] / leg1, d[1] / leg1];
        let cross = u[0] * (side[1] - corner[1]) - u[1] * (side[0] - corner[0]);
        let v = if cross >= 0.0 {
            [-u[1], u[0]]
        } else {
            [u[1], -u[0]]
        };
        let leg2 = f64::from(t.risers.saturating_sub(1)) * t.tread - leg1;
        (leg2 >= WIDTH / 2.0 && leg1 >= WIDTH / 2.0)
            .then(|| [corner[0] + v[0] * leg2, corner[1] + v[1] * leg2])
    }

    fn finish_winder(&mut self, start: [f64; 2], corner: [f64; 2], side: [f64; 2]) {
        let frame = self.frame;
        let Some(t) = self.stair_tool.clone() else {
            return;
        };
        let Some(end) = Self::winder_end(&t, start, corner, side) else {
            return frame.set_status_text(
                "Winder Stair: the corner leaves too little (or too much) for the second leg; click the corner again",
                0,
            );
        };
        self.stair_tool = None;
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        let mut args = json!({
            "base_level": t.level,
            "width": WIDTH,
            "tread_depth": t.tread,
            "runs": [{ "start": start, "corner": corner, "end": end }],
        });
        match t.top {
            Some(l) => args["top"] = json!({ "level": l }),
            None => args["top"] = json!({ "unconnected": UNCONNECTED }),
        }
        match self.run("stair.create", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Winder stair created", 0);
                if let Some(id) = v.as_u64() {
                    let _ = self.run("selection.set", json!({ "ids": [id] }));
                    self.selection_changed();
                    self.offer_stair_openings(ElementId(id), false);
                }
            }
            Err(e) => error(&frame, &format!("The stair was not created:\n{e}")),
        }
    }

    /// A full-step spiral about `center` through `start`, with every riser.
    fn finish_spiral(&mut self, center: [f64; 2], start: [f64; 2]) {
        let frame = self.frame;
        let Some(t) = self.stair_tool.take() else {
            return;
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        let mut args = json!({
            "base_level": t.level,
            "width": SPIRAL_WIDTH,
            "tread_depth": t.tread,
            "runs": [{ "start": start, "center": center }],
        });
        match t.top {
            Some(l) => args["top"] = json!({ "level": l }),
            None => args["top"] = json!({ "unconnected": UNCONNECTED }),
        }
        match self.run("stair.create", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Spiral stair created", 0);
                if let Some(id) = v.as_u64() {
                    let _ = self.run("selection.set", json!({ "ids": [id] }));
                    self.selection_changed();
                    self.offer_stair_openings(ElementId(id), false);
                }
            }
            Err(e) => error(&frame, &format!("The stair was not created:\n{e}")),
        }
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
                let note = if t.left() > 0 {
                    format!(" — the last run extends to {}", t.top_name)
                } else {
                    String::new()
                };
                frame.set_status_text(&format!("Stair created{note}"), 0);
                if let Some(id) = v.as_u64() {
                    let _ = self.run("selection.set", json!({ "ids": [id] }));
                    self.selection_changed();
                    self.offer_stair_openings(ElementId(id), false);
                }
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
        // A winder: its walking line, to the corner and on to where every riser ends.
        if t.winder {
            if let Some(a) = t.start {
                match (t.corner, t.cursor) {
                    (None, Some(p)) => lines.segments.push(seg(a, p)),
                    (Some(c), p) => {
                        lines.segments.push(seg(a, c));
                        if let Some(e) = p.and_then(|p| Self::winder_end(&t, a, c, p)) {
                            lines.segments.push(seg(c, e));
                        }
                    }
                    _ => {}
                }
            }
            if let Some(v) = self.viewport() {
                v.set_overlay_lines(&[lines]);
            }
            return;
        }
        // A spiral: the walking line and the treads' inner and outer circles.
        if t.spiral {
            if let (Some(c), Some(p)) = (t.start, t.cursor) {
                let r = (p[0] - c[0]).hypot(p[1] - c[1]);
                for rad in [r, (r - SPIRAL_WIDTH / 2.0).max(0.0), r + SPIRAL_WIDTH / 2.0] {
                    let pts: Vec<[f64; 2]> = (0..=72)
                        .map(|k| {
                            let a = f64::from(k) / 72.0 * std::f64::consts::TAU;
                            [c[0] + rad * a.cos(), c[1] + rad * a.sin()]
                        })
                        .collect();
                    for w in pts.windows(2) {
                        lines.segments.push(seg(w[0], w[1]));
                    }
                }
                lines.segments.push(seg(c, p));
            }
            if let Some(v) = self.viewport() {
                v.set_overlay_lines(&[lines]);
            }
            return;
        }
        let mut runs = t.runs.clone();
        if let (Some(a), Some(c)) = (t.start, t.cursor)
            && let Some((end, k)) = t.run(a, c)
        {
            runs.push((a, end, k));
        } else if let Some(&(a, b, k)) = t.runs.last()
            && t.left() > 0
        {
            // What Esc makes: the last run carried on to the top.
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = d[0].hypot(d[1]);
            if len > 1e-9 {
                let k = k + t.left();
                let l = f64::from(k - 1) * t.tread;
                runs.pop();
                runs.push((a, [a[0] + d[0] / len * l, a[1] + d[1] / len * l], k));
            }
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

    /// Offers to open the floors and ceilings that leave a stair short of headroom
    /// (ARC-STAIR-007), under the model's building code (Model Properties) or another one chosen
    /// here. With `always`, says so when none is short.
    pub(crate) fn offer_stair_openings(&mut self, stair: ElementId, always: bool) {
        let frame = self.frame;
        let Ok(check) = self.run("stair.headroom_check", json!({ "stair_id": stair })) else {
            return;
        };
        let short: Vec<&serde_json::Value> = check["obstructions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| o["uncovered"].as_f64().unwrap_or(0.0) > 1e-4)
            .collect();
        if short.is_empty() {
            if always {
                let h = self.format_length(check["headroom"].as_f64().unwrap_or(0.0));
                frame.set_status_text(&format!("The stair has {h} of headroom everywhere"), 0);
            }
            return;
        }
        let names: Vec<String> = short
            .iter()
            .map(|o| {
                let clearance = o["clearance"].as_f64().unwrap_or(0.0);
                let name = o["host_name"].as_str().unwrap_or("a floor");
                if clearance > 0.0 {
                    format!(
                        "{name} (headroom down to {})",
                        self.format_length(clearance)
                    )
                } else {
                    format!("{name} (in the stair's way)")
                }
            })
            .collect();
        // The model's code first, then the others, each with its headroom and clause.
        let Ok(codes) = self.run("code.get", json!({})) else {
            return;
        };
        let current = codes["settings"]["code"]
            .as_str()
            .unwrap_or("ibc")
            .to_owned();
        let model_headroom = codes["headroom"]["stair"].as_f64().unwrap_or(2.0);
        let model_clause = codes["headroom"]["clause"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let mut choices: Vec<(String, Option<String>)> = vec![(
            format!(
                "{} — model default: {} ({model_clause})",
                codes["label"].as_str().unwrap_or(""),
                self.format_length(model_headroom)
            ),
            None,
        )];
        for c in codes["codes"].as_array().into_iter().flatten() {
            let key = c["key"].as_str().unwrap_or("").to_owned();
            if key == current || c["headroom"].is_null() {
                continue;
            }
            let h = self.format_length(c["headroom"]["stair"].as_f64().unwrap_or(0.0));
            choices.push((
                format!(
                    "{}: {h} ({})",
                    c["label"].as_str().unwrap_or(""),
                    c["headroom"]["clause"].as_str().unwrap_or("")
                ),
                Some(key),
            ));
        }
        choices.push(("Don't open the floor".to_owned(), Some(String::new())));
        let labels: Vec<&str> = choices.iter().map(|c| c.0.as_str()).collect();
        let dialog = SingleChoiceDialog::builder(
            &frame,
            &format!(
                "The stair passes under {} with less headroom than the building code asks, measured \
                 from the line of the nosings.\n\nOpen the floor over the stair's full width, for the \
                 headroom of:",
                names.join(", ")
            ),
            "Stair Opening",
            &labels,
        )
        .build();
        dialog.set_selection(0);
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some((label, code)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| choices.get(i).cloned())
        else {
            return;
        };
        if code.as_deref() == Some("") {
            return;
        }
        let mut args = json!({ "stair_id": stair });
        if let Some(c) = code {
            args["code"] = json!(c);
        }
        match self.run("stair.create_openings", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                let n = v.as_array().map_or(0, Vec::len);
                let what = label.split(" (").next().unwrap_or(&label).to_owned();
                frame.set_status_text(
                    &format!(
                        "{n} opening(s) made for the stair · {what} · select one to drag its edges"
                    ),
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The floor was not opened:\n{e}")),
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

impl Gui {
    /// Stair paths, cut marks and tread numbers in a plan (ARC-STAIR-006).
    pub(crate) fn stair_mark_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let (Some(plan), Some(view)) = (v.plan.clone(), v.element) else {
            return Vec::new();
        };
        if plan.ceiling {
            return Vec::new();
        }
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let z = plan.range.cut as f32 + 0.004;
        let p3 = |p: [f64; 2]| [p[0] as f32, p[1] as f32, z];
        let mm = plan.scale / 1000.0;
        let mut lines = Lines {
            color: [0.1, 0.1, 0.1, 1.0],
            width: 0.18,
            segments: Vec::new(),
        };
        let label = |text: &str, at: [f64; 2], height: f32| reviera_render::annotation::Label {
            origin: [
                at[0] as f32 - reviera_render::font::width(text, height) / 2.0,
                at[1] as f32,
                z,
            ],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            height,
            text: text.to_owned(),
        };
        for e in store.iter() {
            if stairs::stair_recipe(e).is_none() {
                continue;
            }
            let Ok(g) = stairs::regenerate(&store, e) else {
                continue;
            };
            // On this level, or reaching it from below.
            let on = (g.z_base - plan.elevation).abs() < 0.05
                || (g.z_base + g.height - plan.elevation).abs() < 0.05;
            if !on {
                continue;
            }
            let Some(m) =
                reviera_core::stair_marks::marks(&store, e, plan.elevation, plan.range.cut)
            else {
                continue;
            };
            for w in m.path.windows(2) {
                lines.segments.push([p3(w[0]), p3(w[1])]);
            }
            for [a, b] in m.arrow {
                lines.segments.push([p3(a), p3(b)]);
            }
            if let Some(c) = &m.cut {
                for w in c.windows(2) {
                    lines.segments.push([p3(w[0]), p3(w[1])]);
                }
            }
            lines
                .segments
                .extend(reviera_render::annotation::label_strokes(&[label(
                    m.label.0,
                    m.label.1,
                    2.5 * mm,
                )]));
        }
        if let Some(view) = store.get(view) {
            for (n, at) in reviera_core::stair_marks::numbers_in(&store, view) {
                let t = n.to_string();
                lines
                    .segments
                    .extend(reviera_render::annotation::label_strokes(&[label(
                        &t,
                        at,
                        2.0 * mm,
                    )]));
            }
        }
        vec![lines]
    }

    /// Modify | Stairs › Number Treads: the selected stairs, in this view.
    pub(crate) fn number_treads(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return frame.set_status_text("Tread numbers go in a plan or section", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let stairs: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| store.get(*id).and_then(stairs::stair_recipe).is_some())
            .collect();
        if stairs.is_empty() {
            return frame.set_status_text("Select the stairs to number", 0);
        }
        for s in stairs {
            if let Err(e) = self.run(
                "stair.number_treads",
                json!({ "view_id": view, "stair_id": s }),
            ) {
                return frame.set_status_text(&e, 0);
            }
        }
        self.refresh_scene(&frame);
        frame.set_status_text("Treads numbered", 0);
    }
}
