// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The analytical model in the window (STR-AM-001, 006): Structure › Analytical › Physical to
//! Analytical, Analytical to Physical, Disconnect and Merge Nodes, and Show Analytical, which draws
//! analytical members (red) and nodes (blue crosses) over 3D views.

use std::cell::RefCell;
use std::rc::Rc;

use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error, with};

const MEMBER: [f32; 4] = [0.85, 0.1, 0.1, 1.0];
const NODE: [f32; 4] = [0.1, 0.3, 0.9, 1.0];
const SUPPORT: [f32; 4] = [0.1, 0.6, 0.2, 1.0];
const LOAD: [f32; 4] = [0.95, 0.5, 0.05, 1.0];

impl Gui {
    /// The analytical model's lines for a 3D view, when shown.
    pub(crate) fn analytical_lines(&self, index: usize) -> Vec<Lines> {
        if !self.show_analytical || self.views[index].is_2d() {
            return Vec::new();
        }
        let Some(d) = self.app.document(self.views[index].doc) else {
            return Vec::new();
        };
        let store = d.doc.store();
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
        let members: Vec<[[f32; 3]; 2]> = reviera_core::analytical::members(store)
            .into_iter()
            .map(|(_, m)| [f(m.start), f(m.end)])
            .collect();
        let s = 0.15;
        let nodes: Vec<[[f32; 3]; 2]> = reviera_core::analytical::nodes(store)
            .into_iter()
            .flat_map(|(_, p)| {
                let p = f(p);
                [
                    [[p[0] - s, p[1], p[2]], [p[0] + s, p[1], p[2]]],
                    [[p[0], p[1] - s, p[2]], [p[0], p[1] + s, p[2]]],
                    [[p[0], p[1], p[2] - s], [p[0], p[1], p[2] + s]],
                ]
            })
            .collect();
        // Supports: a small triangle under each supported node.
        let mut supports = Vec::new();
        for e in store.iter() {
            if reviera_core::analytical::node_recipe(e).is_some_and(|r| r.support.is_some())
                && let Some(p) = reviera_core::analytical::node_point(store, e)
            {
                let p = f(p);
                let (a, b, c) = (
                    [p[0] - 0.25, p[1], p[2] - 0.35],
                    [p[0] + 0.25, p[1], p[2] - 0.35],
                    [p[0], p[1], p[2]],
                );
                supports.extend([[a, b], [b, c], [c, a]]);
            }
        }
        // Loads: an arrow of fixed length in each force's direction, ending at its point.
        let members_by_guid: std::collections::HashMap<String, ([f64; 3], [f64; 3])> =
            reviera_core::analytical::members(store)
                .into_iter()
                .filter_map(|(id, m)| Some((store.get(id)?.guid().to_string(), (m.start, m.end))))
                .collect();
        let point_of = |host: &str, at: f64| -> Option<[f64; 3]> {
            if let Some((a, b)) = members_by_guid.get(host) {
                return Some([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * at));
            }
            let id = reviera_core::GlobalId::parse(host).and_then(|g| store.find_by_guid(&g))?;
            reviera_core::analytical::node_point(store, store.get(id)?)
        };
        let mut loads = Vec::new();
        let mut arrow = |p: [f64; 3], force: [f64; 3]| {
            let m = (force[0].powi(2) + force[1].powi(2) + force[2].powi(2)).sqrt();
            if m < 1e-9 {
                return;
            }
            let d = force.map(|v| v / m);
            let tail = [0, 1, 2].map(|k| (p[k] - d[k] * 1.0) as f32);
            let head = f(p);
            loads.push([tail, head]);
            // Two barbs.
            let side = if d[2].abs() > 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            };
            for s in [-1.0, 1.0] {
                let barb = [0, 1, 2].map(|k| (p[k] - d[k] * 0.2 + side[k] * 0.1 * s) as f32);
                loads.push([barb, head]);
            }
        };
        for e in store.iter() {
            if let Some(l) = reviera_core::loads::point_load(e)
                && let Some(p) = point_of(&l.host, l.at)
            {
                arrow(p, [l.forces[0], l.forces[1], l.forces[2]]);
            }
            if let Some(l) = reviera_core::loads::line_load(e) {
                for k in 0..=4 {
                    let t = k as f64 / 4.0;
                    if let Some(p) = point_of(&l.host, t) {
                        let v = [0, 1, 2].map(|i| l.start[i] + (l.end[i] - l.start[i]) * t);
                        arrow(p, v);
                    }
                }
            }
        }
        vec![
            Lines {
                color: MEMBER,
                width: 0.5,
                segments: members,
            },
            Lines {
                color: NODE,
                width: 0.35,
                segments: nodes,
            },
            Lines {
                color: SUPPORT,
                width: 0.5,
                segments: supports,
            },
            Lines {
                color: LOAD,
                width: 0.35,
                segments: loads,
            },
        ]
    }

    /// Structure › Analytical › Show Analytical.
    pub(crate) fn toggle_analytical(&mut self) {
        self.show_analytical = !self.show_analytical;
        for i in 0..self.views.len() {
            self.update_inspection(i);
            self.views[i].panel.refresh(false, None);
        }
        self.frame.set_status_text(
            if self.show_analytical {
                "Analytical model shown in 3D views"
            } else {
                "Analytical model hidden"
            },
            0,
        );
    }

    /// Runs an analytical automation command on the selection (or everything that applies) and
    /// reports what it did.
    pub(crate) fn analytical_automation(&mut self, command: &str) {
        let frame = self.frame;
        let ids = self.selection();
        let args = match command {
            "boundary_condition.at_column_bases" => json!({}),
            "load_combination.generate" => json!({ "add": true }),
            _ if ids.is_empty() => json!({}),
            _ => json!({ "element_ids": ids }),
        };
        match self.run(command, args) {
            Ok(v) => {
                let n = |k: &str| v[k].as_array().map_or(0, Vec::len);
                let text = match command {
                    "boundary_condition.at_column_bases" => format!(
                        "{} column feet supported (pinned)",
                        v.as_array().map_or(0, Vec::len)
                    ),
                    "load_combination.generate" => format!(
                        "{} combinations made from the code template; {} cases flagged",
                        n("created"),
                        n("flagged")
                    ),
                    "analytical_automation.disconnect" => {
                        format!(
                            "{} analytical members disconnected",
                            v.as_array().map_or(0, Vec::len)
                        )
                    }
                    "analytical_node.merge" => format!(
                        "{} nodes merged, {} unused nodes removed",
                        v["merged"], v["removed_unused"]
                    ),
                    _ => format!(
                        "{} created, {} updated, {} skipped",
                        n("created"),
                        n("updated"),
                        n("skipped")
                    ),
                };
                frame.set_status_text(&text, 0);
                if !self.show_analytical && command.ends_with("physical_to_analytical") {
                    self.toggle_analytical();
                    frame.set_status_text(&text, 0);
                }
                self.refresh_scene(&frame);
                for i in 0..self.views.len() {
                    self.update_inspection(i);
                }
            }
            Err(e) => error(&frame, &format!("The run did not complete:\n{e}")),
        }
    }
}

/// A running structural analysis: its job and the timer that follows it.
pub struct AnalysisJob {
    id: String,
    timer: Timer<Frame>,
}

impl Gui {
    /// Analyze › Run Analysis: the analytical model, its supports, cases and loads analysed out of
    /// process by the built-in linear static solver; collected when it finishes.
    pub(crate) fn start_structural_analysis(gui: &Rc<RefCell<Gui>>) {
        let Ok(mut g) = gui.try_borrow_mut() else {
            return;
        };
        if g.analysis_job.is_some() {
            return g.frame.set_status_text("An analysis is already running", 0);
        }
        let frame = g.frame;
        match g.run("structural_analysis.run", json!({})) {
            Ok(v) => {
                let id = v["job_id"].as_str().unwrap_or_default().to_owned();
                let timer = Timer::new(&frame);
                {
                    let gg = gui.clone();
                    timer.on_tick(move |_| with(&gg, Gui::poll_structural_analysis));
                }
                timer.start(250, false);
                frame.set_status_text(
                    &format!(
                        "Analysing {} members and {} nodes in {} cases out of process…",
                        v["members"], v["nodes"], v["cases"]
                    ),
                    0,
                );
                g.analysis_job = Some(AnalysisJob { id, timer });
            }
            Err(e) => error(&frame, &format!("The analysis did not start:\n{e}")),
        }
    }

    fn poll_structural_analysis(&mut self) {
        let Some(id) = self.analysis_job.as_ref().map(|j| j.id.clone()) else {
            return;
        };
        let Ok(s) = self.run("compute.status", json!({ "job_id": id })) else {
            return;
        };
        let state = s["state"].as_str().unwrap_or_default().to_owned();
        if !matches!(state.as_str(), "succeeded" | "failed" | "cancelled") {
            return;
        }
        if let Some(j) = self.analysis_job.take() {
            j.timer.stop();
        }
        let frame = self.frame;
        if state != "succeeded" {
            let why = s["failure"]["message"]
                .as_str()
                .unwrap_or(&state)
                .to_owned();
            return error(&frame, &format!("The analysis {state}:\n{why}"));
        }
        match self.run("structural_analysis.collect", json!({ "job_id": id })) {
            Ok(r) => self.show_analysis_report(&r),
            Err(e) => error(&frame, &format!("Collecting the analysis failed:\n{e}")),
        }
    }

    /// Analyze › Results: the last analysis's summary.
    pub(crate) fn structural_results(&mut self) {
        match self.run("structural_analysis.results", json!({})) {
            Ok(r) => self.show_analysis_report(&r),
            Err(e) => error(&self.frame, &e),
        }
    }

    fn show_analysis_report(&mut self, r: &serde_json::Value) {
        // Displacements are small: millimetres to a tenth.
        let length = |v: f64| format!("{:.1} mm", v * 1000.0);
        let mut text = String::new();
        if r["stale"] == true {
            text.push_str("The model has changed since this analysis: run it again.\n\n");
        }
        for c in r["cases"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "{}{}: max moment {:.1} kN·m, max displacement {}, vertical reactions {:.1} kN\n",
                c["name"].as_str().unwrap_or_default(),
                if c["combination"] == true {
                    " (combination)"
                } else {
                    ""
                },
                c["max_moment"].as_f64().unwrap_or(0.0) / 1000.0,
                length(c["max_displacement"].as_f64().unwrap_or(0.0)),
                c["total_vertical_reaction"].as_f64().unwrap_or(0.0) / 1000.0,
            ));
        }
        let warnings: Vec<&str> = r["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str())
            .collect();
        if !warnings.is_empty() {
            text.push_str(&format!("\nWarnings:\n{}", warnings.join("\n")));
        }
        self.frame
            .set_status_text("Structural analysis collected", 0);
        let d = MessageDialog::builder(&self.frame, &text, "Structural Analysis Results")
            .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
            .build();
        d.show_modal();
    }
}
