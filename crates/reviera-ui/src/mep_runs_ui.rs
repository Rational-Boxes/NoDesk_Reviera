// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Duct and Pipe tools (MEP-DUCT-001, MEP-PIPE-001): Systems › HVAC & Piping › Duct (`DT`)
//! and Pipe (`PL`).
//!
//! A form takes the run's type, shape and size, its middle elevation above the level, its system
//! type and, for pipes, a slope. Then each click in a plan adds a point at the current elevation
//! (snapping as other tools do; typed coordinates work through Coordinates); **E** asks for a new
//! elevation and puts a riser at the last point; **Enter** or **Esc** makes the run (`duct.create`
//! or `pipe.create`: elbows at corners, a tee or tap where it ends on a duct or pipe) and the tool
//! stays for the next one. Cap Open Ends caps the selection's open ends.

use reviera_core::ElementId;
use reviera_core::mep::{self, Domain};
use reviera_core::mep_systems;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

/// The Duct and Pipe tools: the run's settings and the points clicked so far.
#[derive(Debug, Clone)]
pub(crate) struct RunTool {
    pub domain: Domain,
    pub level: ElementId,
    pub type_id: Option<ElementId>,
    pub shape: mep::Shape,
    /// Metres.
    pub diameter: f64,
    pub width: f64,
    pub height: f64,
    /// The centreline's height above the level now (metres).
    pub elevation: f64,
    pub system_type: Option<String>,
    /// Pipes: rise over run.
    pub slope: f64,
    /// Plan points with their height above the level.
    pub points: Vec<[f64; 3]>,
    pub cursor: Option<[f64; 2]>,
}

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

fn number(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Gui {
    /// Systems › Duct and Pipe: asks for the run's settings, then takes clicked points.
    pub(crate) fn start_run_tool(&mut self, domain: Domain) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let Some(store) = self.store_now() else { return };
        let keep = self.run_tool.as_ref().filter(|t| t.domain == domain).cloned();
        let types: Vec<(ElementId, String, mep::Shape)> = store
            .iter()
            .filter_map(|e| {
                let t = reviera_core::mep_runs::segment_type(e)?;
                (t.domain == domain).then(|| (e.id(), e.name.clone().unwrap_or_default(), t.shape))
            })
            .collect();
        let system_types: Vec<mep_systems::SystemType> = mep_systems::system_types(&store)
            .into_iter()
            .filter(|t| t.classification.fits(domain))
            .collect();
        let duct = domain == Domain::Duct;
        let title = if duct { "Duct" } else { "Pipe" };
        let f = super::reinforcement_ui::Form::new(&frame, title);
        let mut type_labels = vec![if duct { "Built-in (by shape)".to_owned() } else { "Standard Pipe (built-in)".to_owned() }];
        type_labels.extend(types.iter().map(|t| t.1.clone()));
        let type_at = keep
            .as_ref()
            .and_then(|k| k.type_id)
            .and_then(|id| types.iter().position(|t| t.0 == id))
            .map_or(0, |i| i + 1);
        let type_choice = f.choice("Type", &type_labels, type_at);
        let shapes = [mep::Shape::Rectangular, mep::Shape::Round, mep::Shape::Oval];
        let shape_choice = duct.then(|| {
            let at = keep.as_ref().and_then(|k| shapes.iter().position(|s| *s == k.shape)).unwrap_or(0);
            f.choice("Shape", &["Rectangular".to_owned(), "Round".to_owned(), "Oval".to_owned()], at)
        });
        let mm = |v: f64| format!("{:.0}", v * 1000.0);
        let k = keep.as_ref();
        let width = duct.then(|| f.text("Width (mm)", &mm(k.map_or(0.6, |k| k.width)), "Rectangular and oval ducts"));
        let height = duct.then(|| f.text("Height (mm)", &mm(k.map_or(0.3, |k| k.height)), "Rectangular and oval ducts"));
        let diameter = f.text("Diameter (mm)", &mm(k.map_or(if duct { 0.25 } else { 0.05 }, |k| k.diameter)), "Round ducts and pipes");
        let elevation = f.text(
            "Middle elevation (m)",
            &format!("{:.3}", k.map_or(if duct { 2.7 } else { 2.4 }, |k| k.elevation)),
            "The centreline's height above the level; E while drawing changes it (a riser)",
        );
        let mut st_labels = vec!["None".to_owned()];
        st_labels.extend(system_types.iter().map(|t| t.name.clone()));
        let st_at = k
            .and_then(|k| k.system_type.as_ref())
            .and_then(|id| system_types.iter().position(|t| &t.id == id))
            .map_or(0, |i| i + 1);
        let st_choice = f.choice("System type", &st_labels, st_at);
        let slope = (!duct).then(|| {
            f.text("Slope (%)", &format!("{}", k.map_or(0.0, |k| k.slope * 100.0)), "Level legs fall by this much from the first point")
        });
        if !f.show("Click the run's points; E changes the elevation (a riser); Enter or Esc finishes", "Draw") {
            f.dialog.destroy();
            return;
        }
        let read = |t: &Option<TextCtrl>, d: f64| t.as_ref().and_then(number).map_or(d, |v| v / 1000.0);
        let ti = type_choice.get_selection().map_or(0, |i| i as usize);
        let picked = ti.checked_sub(1).and_then(|i| types.get(i));
        let shape = match (&shape_choice, picked) {
            (Some(c), _) => shapes[c.get_selection().map_or(0, |i| i as usize).min(2)],
            (None, Some(t)) => t.2,
            (None, None) => mep::Shape::Round,
        };
        let tool = RunTool {
            domain,
            level,
            type_id: picked.map(|t| t.0),
            shape,
            diameter: number(&diameter).map_or(0.25, |v| v / 1000.0),
            width: read(&width, 0.6),
            height: read(&height, 0.3),
            elevation: number(&elevation).unwrap_or(if duct { 2.7 } else { 2.4 }),
            system_type: st_choice
                .get_selection()
                .and_then(|i| (i as usize).checked_sub(1))
                .and_then(|i| system_types.get(i))
                .map(|t| t.id.clone()),
            slope: slope.as_ref().and_then(number).map_or(0.0, |v| v / 100.0),
            points: Vec::new(),
            cursor: None,
        };
        f.dialog.destroy();
        self.end_tool(&frame);
        self.run_tool = Some(tool);
        frame.set_status_text(&format!("{title}: click the run's points; E changes the elevation; Enter or Esc finishes"), 0);
    }

    pub(crate) fn end_run_tool(&mut self) -> bool {
        if self.run_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    pub(crate) fn run_motion(&mut self, x: i32, y: i32) {
        let from = self.run_tool.as_ref().and_then(|t| t.points.last().map(|p| [p[0], p[1]]));
        let lines = self.snap_lines();
        let p = self.snapped_at(x, y, from, &lines).map(|s| s.point);
        if let Some(t) = &mut self.run_tool {
            t.cursor = p;
        }
        self.run_preview();
    }

    pub(crate) fn run_click(&mut self, x: i32, y: i32) {
        let from = self.run_tool.as_ref().and_then(|t| t.points.last().map(|p| [p[0], p[1]]));
        let lines = self.snap_lines();
        let Some(p) = self.snapped_at(x, y, from, &lines).map(|s| s.point) else { return };
        let Some(t) = &mut self.run_tool else { return };
        let q = [p[0], p[1], t.elevation];
        if t.points.last() != Some(&q) {
            t.points.push(q);
        }
        self.run_preview();
    }

    /// E: a new elevation (the run rises or drops there); Enter: finish.
    pub(crate) fn run_key(&mut self, key: i32) -> bool {
        if self.run_tool.is_none() {
            return false;
        }
        match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                self.finish_run();
                true
            }
            k if k == 'E' as i32 || k == 'e' as i32 => {
                let frame = self.frame;
                let now = self.run_tool.as_ref().map_or(2.7, |t| t.elevation);
                let d = TextEntryDialog::builder(&frame, "New middle elevation above the level (m):", "Elevation")
                    .with_default_value(&format!("{now:.3}"))
                    .build();
                if d.show_modal() == ID_OK
                    && let Some(v) = d.get_value().and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok())
                    && let Some(t) = &mut self.run_tool
                {
                    t.elevation = v;
                    // A riser at the last point.
                    if let Some(last) = t.points.last().copied()
                        && (last[2] - v).abs() > 1e-6
                    {
                        t.points.push([last[0], last[1], v]);
                    }
                    frame.set_status_text(&format!("Middle elevation {v:.3} m"), 0);
                }
                true
            }
            _ => false,
        }
    }

    /// Esc: a run of two or more points is made; otherwise the tool ends.
    pub(crate) fn run_escape(&mut self) {
        if self.run_tool.as_ref().is_some_and(|t| t.points.len() >= 2) {
            self.finish_run();
        } else {
            self.end_run_tool();
            self.frame.set_status_text("Ready", 0);
        }
    }

    fn finish_run(&mut self) {
        let frame = self.frame;
        let Some(t) = self.run_tool.clone() else { return };
        if t.points.len() < 2 {
            return frame.set_status_text("Click at least two points", 0);
        }
        let duct = t.domain == Domain::Duct;
        let mut args = json!({ "points": t.points, "level": t.level, "shape": t.shape });
        if t.shape == mep::Shape::Round {
            args["diameter"] = json!(t.diameter);
        } else {
            args["width"] = json!(t.width);
            args["height"] = json!(t.height);
        }
        if let Some(id) = t.type_id {
            args["type_id"] = json!(id);
        }
        if let Some(st) = &t.system_type {
            args["system_type_id"] = json!(st);
        }
        if !duct && t.slope != 0.0 {
            args["slope"] = json!(t.slope);
        }
        let command = if duct { "duct.create" } else { "pipe.create" };
        match self.app.execute(command, args, &reviera_core::Caller::user()) {
            Ok(o) => {
                if let Some(tool) = &mut self.run_tool {
                    // The next run starts afresh at the same settings.
                    tool.points.clear();
                    tool.cursor = None;
                }
                if let Some(v) = self.viewport() {
                    v.set_overlay_lines(&[]);
                }
                self.refresh_scene(&frame);
                let r = &o.result;
                let mut text = format!(
                    "{} segment(s), {} fitting(s)",
                    r["segments"].as_array().map_or(0, Vec::len),
                    r["fittings"].as_array().map_or(0, Vec::len)
                );
                if !r["system"].is_null() {
                    text.push_str("; joined its system");
                }
                if let Some(w) = o.warnings.first() {
                    text.push_str(&format!("; {}", w.message));
                }
                frame.set_status_text(&format!("{text}. Click the next run, or Esc"), 0);
            }
            Err(e) => error(&frame, &format!("The run was not made:\n{e}")),
        }
    }

    fn run_preview(&mut self) {
        let Some(t) = self.run_tool.clone() else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else { return };
        let z = plan.range.cut as f32 + 0.004;
        let mut pts: Vec<[f64; 2]> = t.points.iter().map(|p| [p[0], p[1]]).collect();
        if let Some(c) = t.cursor {
            pts.push(c);
        }
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: pts
                .windows(2)
                .map(|w| [[w[0][0] as f32, w[0][1] as f32, z], [w[1][0] as f32, w[1][1] as f32, z]])
                .collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Systems › Cap Open Ends for the selected ducts, pipes and fittings.
    pub(crate) fn cap_open_ends(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        match self.run("duct.cap_open_ends", json!({ "element_ids": ids })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} cap(s) placed", v.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("No caps were placed:\n{e}")),
        }
    }
}

fn error(frame: &Frame, message: &str) {
    let d = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}
