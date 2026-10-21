// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Pressure Drop (MEP-CALC-002, MEP-CALC-004): Calculate Networks, the Analytical
//! Segments of a system (flow, velocity, friction, loss, static pressure, the critical path;
//! select, override a loss, recalculate) and the Pressure Loss Report (systems, sections,
//! presets, CSV or HTML); the System Inspector (MEP-CALC-003), a view session drawing flow
//! arrows on every section with the critical path highlighted and a section's flow, static
//! pressure and loss shown on hover, ended by Exit or any edit to the model.

use reviera_app::DocumentId;
use reviera_core::ElementId;
use reviera_render::plan::Lines;
use reviera_core::mep_network::{self, NetworkResult};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const SELECT: i32 = 7401;
const SELECT_PATH: i32 = 7402;
const OVERRIDE: i32 = 7403;
const CLEAR: i32 = 7404;
const RECALCULATE: i32 = 7405;

fn opt(v: Option<f64>, places: usize) -> String {
    v.map(|v| format!("{v:.places$}")).unwrap_or_default()
}

impl Gui {
    /// Duct and pipe systems, the selection's first.
    fn network_systems(&mut self, store: &reviera_core::Store) -> Vec<reviera_core::mep_systems::SystemInfo> {
        use reviera_core::mep::{Classification, Domain};
        let sel = self.selection();
        let mut all: Vec<_> = reviera_core::mep_systems::systems(store)
            .into_iter()
            .filter(|s| (s.classification.fits(Domain::Duct) || s.classification.fits(Domain::Pipe)) && !matches!(s.classification, Classification::Sanitary | Classification::Vent))
            .collect();
        all.sort_by_key(|s| !(sel.contains(&s.id) || s.elements.iter().any(|e| sel.contains(e))));
        all
    }

    /// Analyze › Calculate Networks: every duct and pipe system.
    pub(crate) fn calculate_networks(&mut self) {
        let frame = self.frame;
        match self.run("mep_network.calculate", json!({})) {
            Ok(r) => {
                let systems = r["systems"].as_array().cloned().unwrap_or_default();
                let losses: Vec<String> = systems
                    .iter()
                    .map(|s| match s["critical_loss"].as_f64() {
                        Some(l) => format!("{} {l:.0} Pa", s["name"].as_str().unwrap_or("")),
                        None => format!("{} (flow only)", s["name"].as_str().unwrap_or("")),
                    })
                    .collect();
                let text = format!("{} system(s) calculated. Critical-path losses: {}", systems.len(), losses.join(", "));
                let warnings: Vec<&str> = r["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                if warnings.is_empty() {
                    frame.set_status_text(&text, 0);
                } else {
                    error(&frame, &format!("{text}.\n\n{}", warnings.join("\n")));
                }
            }
            Err(e) => error(&frame, &format!("The networks were not calculated:\n{e}")),
        }
    }

    /// Analyze › Analytical Segments: a system's results, recalculated first when stale.
    pub(crate) fn analytical_segments(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let systems = self.network_systems(&store);
        if systems.is_empty() {
            return frame.set_status_text("Analytical Segments: the model has no duct or pipe systems", 0);
        }
        let sys = if systems.len() == 1 {
            systems[0].id
        } else {
            let names: Vec<String> = systems.iter().map(|s| format!("{} ({})", s.name, s.classification.label())).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let d = SingleChoiceDialog::builder(&frame, "System:", "Analytical Segments", &refs).build();
            if d.show_modal() != ID_OK {
                return;
            }
            systems[d.get_selection().max(0) as usize].id
        };
        let mut at = None;
        loop {
            let Some(store) = self.store_now() else { return };
            let info = reviera_core::mep_systems::systems(&store).into_iter().find(|s| s.id == sys);
            let Some(info) = info else { return };
            // Stale or never calculated: calculated first.
            let r: NetworkResult = match mep_network::kept(&store, &info) {
                Some(k) if !k.stale => k,
                _ => match self.run("mep_network.calculate", json!({ "system_id": sys })).and_then(|_| self.run("analytical_segment.list", json!({ "system_id": sys }))) {
                    Ok(v) => match serde_json::from_value(v) {
                        Ok(r) => r,
                        Err(e) => return error(&frame, &e.to_string()),
                    },
                    Err(e) => return error(&frame, &format!("{} was not calculated:\n{e}", info.name)),
                },
            };
            let name = |id: ElementId| store.get(id).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("#{}", id.0));
            let rows: Vec<Vec<String>> = r
                .segments
                .iter()
                .map(|a| {
                    vec![
                        name(a.element),
                        a.part.clone().unwrap_or_else(|| "Segment".into()),
                        a.size.clone(),
                        format!("{:.1}", a.flow * 1000.0),
                        format!("{:.2}", a.velocity),
                        opt(a.friction, 2),
                        opt(a.coefficient, 2),
                        opt(a.loss, 1),
                        opt(a.pressure, 1),
                        if a.critical { "Yes".into() } else { String::new() },
                        if a.overridden { "Yes".into() } else { String::new() },
                        if a.connected { a.network_id.to_string() } else { format!("{} (not joined)", a.network_id) },
                    ]
                })
                .collect();
            let intro = format!(
                "{}: {:.1} l/s from {}. {}{}",
                r.name,
                r.total_flow * 1000.0,
                r.equipment.map(name).unwrap_or_default(),
                match r.critical_loss {
                    Some(l) => format!("Critical-path loss {l:.1} Pa over {} elements; static pressures assume the equipment supplies it.", r.critical_path.len()),
                    None => "Flow only: its system type does not calculate losses.".into(),
                },
                if r.warnings.is_empty() { String::new() } else { format!("\n{}", r.warnings.join("\n")) }
            );
            let (code, k) = super::mep_loads_ui::list_dialog(
                &frame,
                "Analytical Segments",
                &intro,
                &[("Element", 120), ("Part", 80), ("Size", 80), ("Flow (l/s)", 70), ("Velocity (m/s)", 95), ("Friction (Pa/m)", 100), ("Coefficient", 80), ("Loss (Pa)", 65), ("Pressure (Pa)", 95), ("Critical", 55), ("Overridden", 80), ("Network", 70)],
                &rows,
                &[
                    (SELECT, "Select", "Select the element"),
                    (SELECT_PATH, "Select Critical Path", "Select every element on the critical path"),
                    (OVERRIDE, "Override Loss…", "A measured or given loss (Pa), or a fitting's coefficient"),
                    (CLEAR, "Clear Override", "Back to the calculated loss"),
                    (RECALCULATE, "Recalculate", "Calculate the network again"),
                ],
                at,
            );
            at = k;
            let element = k.and_then(|k| r.segments.get(k)).map(|a| a.element);
            match code {
                SELECT => {
                    if let Some(e) = element {
                        self.select_ids(&[e], super::select::Combine::Replace);
                    }
                    return;
                }
                SELECT_PATH => {
                    self.select_ids(&r.critical_path, super::select::Combine::Replace);
                    return;
                }
                OVERRIDE => {
                    let Some(e) = element else { continue };
                    let fitting = r.segments.iter().any(|a| a.element == e && a.kind == "fitting");
                    let f = super::reinforcement_ui::Form::new(&frame, "Override Loss");
                    let loss = f.text("Loss (Pa)", "", "Empty: calculated");
                    let coefficient = fitting.then(|| f.text("Coefficient", "", "Empty: Reviera's"));
                    let ok = f.show("The network's results are recalculated with it.", "OK");
                    let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
                    let args = json!({ "element_id": e, "loss": num(&loss), "coefficient": coefficient.as_ref().and_then(num) });
                    f.dialog.destroy();
                    if ok && let Err(m) = self.run("analytical_segment.override", args) {
                        error(&frame, &m);
                    }
                }
                CLEAR => {
                    if let Some(e) = element
                        && let Err(m) = self.run("analytical_segment.override", json!({ "element_id": e, "clear": true }))
                    {
                        error(&frame, &m);
                    }
                }
                RECALCULATE => {
                    if let Err(m) = self.run("mep_network.calculate", json!({ "system_id": sys })) {
                        return error(&frame, &m);
                    }
                }
                _ => return,
            }
        }
    }

    /// Analyze › Pressure Loss Report.
    pub(crate) fn pressure_loss_report(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let systems = self.network_systems(&store);
        if systems.is_empty() {
            return frame.set_status_text("Pressure Loss Report: the model has no duct or pipe systems", 0);
        }
        let why: Vec<Option<String>> = systems.iter().map(|s| mep_network::not_reportable(&store, s)).collect();
        let labels: Vec<String> = systems
            .iter()
            .zip(&why)
            .map(|(s, w)| match w {
                Some(w) => format!("{} ({}) — cannot be reported: {w}", s.name, s.classification.label()),
                None => format!("{} ({})", s.name, s.classification.label()),
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut picked: Vec<i32> = why.iter().position(Option::is_none).map(|k| vec![k as i32]).unwrap_or_default();
        let chosen = loop {
            let d = MultiChoiceDialog::builder(&frame, "Systems to report (only systems whose type calculates All):", "Pressure Loss Report", &refs).build();
            d.set_selections(&picked);
            if d.show_modal() != ID_OK {
                return;
            }
            picked = d.get_selections();
            let refused: Vec<String> = picked.iter().filter_map(|k| why[*k as usize].as_ref().map(|w| format!("{}: {w}", systems[*k as usize].name))).collect();
            if refused.is_empty() && !picked.is_empty() {
                break picked.iter().map(|k| systems[*k as usize].id).collect::<Vec<ElementId>>();
            }
            if picked.is_empty() {
                return;
            }
            error(&frame, &format!("These systems cannot be ticked:\n{}", refused.join("\n")));
            picked.retain(|k| why[*k as usize].is_none());
        };
        let presets = reviera_core::mep_systems::settings(&store).report_presets;
        let f = super::reinforcement_ui::Form::new(&frame, "Report Settings");
        let mut preset_names = vec!["(these settings)".to_owned()];
        preset_names.extend(presets.iter().map(|p| p.name.clone()));
        let preset = f.choice("Preset", &preset_names, 0);
        let titles = ["Project information", "System information", "Total pressure loss", "Segment detail", "Fitting and accessory detail", "Critical path"];
        let checks: Vec<CheckBox> = titles.iter().enumerate().map(|(k, t)| f.check(if k == 0 { "Sections" } else { "" }, t, true)).collect();
        let format = f.choice("Format", &["CSV (SI units)".to_owned(), "HTML (project units)".to_owned()], 0);
        let save = f.text("Save as preset", "", "A name to keep these settings for the team; empty: not kept");
        let ok = f.show("Stale results are calculated before the report is written.", "Generate…");
        let preset_k = preset.get_selection().map_or(0, |k| k as usize);
        let sections: Vec<&str> = checks.iter().zip(mep_network::SECTIONS).filter(|(c, _)| c.get_value()).map(|(_, s)| s).collect();
        let html = format.get_selection() == Some(1);
        let save_name = save.get_value().trim().to_owned();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let settings = match preset_k {
            0 => json!({ "sections": sections }),
            k => json!({ "preset_id": presets[k - 1].id }),
        };
        if !save_name.is_empty()
            && preset_k == 0
            && let Err(e) = self.run("pressure_loss_report.save_preset", json!({ "name": save_name, "settings": { "sections": sections } }))
        {
            return error(&frame, &e);
        }
        let (wildcard, file) = if html { ("HTML files (*.html)|*.html", "pressure loss.html") } else { ("CSV files (*.csv)|*.csv", "pressure loss.csv") };
        let d = FileDialog::builder(&frame)
            .with_message("Pressure Loss Report")
            .with_wildcard(wildcard)
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file(file)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        let args = json!({ "system_ids": chosen, "settings": settings, "path": path, "format": if html { "html" } else { "csv" }, "overwrite": true });
        match self.run("pressure_loss_report.export", args) {
            Ok(r) => {
                let n = r["recalculated"].as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("Pressure loss report written to {path}{}", if n > 0 { format!(" ({n} system(s) recalculated first)") } else { String::new() }), 0);
            }
            Err(e) => error(&frame, &format!("The report was not written:\n{e}")),
        }
    }
}

/// The System Inspector session.
pub(crate) struct Inspector {
    pub view: u32,
    doc: DocumentId,
    version: reviera_core::ModelVersion,
    result: NetworkResult,
    /// The hovered section's index.
    hover: Option<usize>,
}

/// Flow arrows' colour off the critical path.
const FLOW: [f32; 4] = [0.15, 0.55, 0.95, 1.0];
/// The hovered section.
const HOVER: [f32; 4] = [0.10, 0.85, 0.85, 1.0];

fn f32s(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

/// A polyline with an arrowhead at the middle of each leg, pointing along it.
fn arrows(points: &[[f64; 3]]) -> Vec<[[f32; 3]; 2]> {
    let mut out = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        out.push([f32s(a), f32s(b)]);
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l < 0.3 {
            continue;
        }
        let u = d.map(|v| v / l);
        // Across: level for level legs, along x for risers (so the head shows in elevations).
        let n = if u[2].abs() < 0.9 {
            let h = (u[0] * u[0] + u[1] * u[1]).sqrt();
            [-u[1] / h, u[0] / h, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let size = (l * 0.25).min(0.25);
        let tip = [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0 + u[k] * size / 2.0);
        for side in [-1.0, 1.0] {
            let wing = [0, 1, 2].map(|k| tip[k] - u[k] * size + n[k] * side * size * 0.5);
            out.push([f32s(tip), f32s(wing)]);
        }
    }
    out
}

impl Gui {
    /// Analyze › System Inspector: inspects the selection's system (or a chosen one), calculating
    /// it first when its results are stale; again, it ends the session.
    pub(crate) fn start_inspector(&mut self) {
        let frame = self.frame;
        if self.inspector.is_some() {
            return self.stop_inspector("System Inspector closed");
        }
        let (Some(view), Some(doc)) = (self.active, self.doc) else {
            return frame.set_status_text("System Inspector: open a view first", 0);
        };
        let Some(store) = self.store_now() else { return };
        let systems = self.network_systems(&store);
        let sel = self.selection();
        let picked: Vec<_> = systems.iter().filter(|s| sel.contains(&s.id) || s.elements.iter().any(|e| sel.contains(e))).collect();
        let sys = match (picked.first(), systems.len()) {
            (Some(s), _) => (*s).clone(),
            (None, 0) => return frame.set_status_text("System Inspector: the model has no duct or pipe systems", 0),
            (None, 1) => systems[0].clone(),
            (None, _) => {
                let names: Vec<String> = systems.iter().map(|s| format!("{} ({})", s.name, s.classification.label())).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let d = SingleChoiceDialog::builder(&frame, "System to inspect:", "System Inspector", &refs).build();
                if d.show_modal() != ID_OK {
                    return;
                }
                systems[d.get_selection().max(0) as usize].clone()
            }
        };
        // The first failing condition, before anything is calculated.
        if let Some(e) = mep_network::inspect_refusal(&store, &sys) {
            return error(&frame, &format!("Inspect is not available:\n{}", e.message));
        }
        if mep_network::kept(&store, &sys).is_none_or(|k| k.stale) {
            frame.set_status_text(&format!("System Inspector: calculating {}…", sys.name), 0);
            if let Err(e) = self.run("mep_network.calculate", json!({ "system_id": sys.id })) {
                return error(&frame, &format!("{} was not calculated:\n{e}", sys.name));
            }
        }
        let r: NetworkResult = match self.run("system_inspector.start", json!({ "system_id": sys.id, "view_id": view.to_string() })) {
            Ok(v) => match serde_json::from_value(v) {
                Ok(r) => r,
                Err(e) => return error(&frame, &e.to_string()),
            },
            Err(e) => return error(&frame, &format!("Inspect is not available:\n{e}")),
        };
        let Some(version) = self.app.document(doc).map(|d| d.doc.version()) else { return };
        frame.set_status_text(
            &format!(
                "System Inspector: {}. Arrows show the flow; the critical path ({:.1} Pa) is drawn thick in the warning colour. Hover a section for its flow, static pressure and loss.",
                r.name,
                r.critical_loss.unwrap_or(0.0)
            ),
            0,
        );
        self.inspector = Some(Inspector { view, doc, version, result: r, hover: None });
        self.update_ribbon_context();
        self.refresh_active();
    }

    /// Ends the session.
    pub(crate) fn stop_inspector(&mut self, why: &str) {
        if self.inspector.take().is_none() {
            return;
        }
        self.frame.set_status_text(why, 0);
        if let Some(v) = self.views.iter().find(|v| Some(v.id) == self.active) {
            v.panel.set_tooltip("");
        }
        self.update_ribbon_context();
        self.refresh_active();
    }

    /// Ends the session when the model has been edited or its view has closed.
    pub(crate) fn inspector_idle(&mut self) {
        let Some(i) = &self.inspector else { return };
        let now = self.app.document(i.doc).map(|d| d.doc.version());
        if now != Some(i.version) {
            self.stop_inspector("System Inspector ended: the model was edited");
        } else if !self.views.iter().any(|v| v.id == i.view) {
            self.stop_inspector("System Inspector ended: its view was closed");
        }
    }

    /// The hovered section's values in a tooltip and the status bar; `true` when it changed.
    pub(crate) fn inspector_motion(&mut self, x: i32, y: i32, panel: &Panel) -> bool {
        if self.inspector.as_ref().is_none_or(|i| Some(i.view) != self.active) {
            return false;
        }
        // The section whose drawn centreline passes within 6 pixels (services above a plan's cut
        // plane cannot be picked as elements), else the element under the pointer.
        let size = panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let cam = self.camera();
        let (px, py) = (x as f32, y as f32);
        let near = |a: (f32, f32), b: (f32, f32)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let (cx, cy) = (a.0 + dx * t - px, a.1 + dy * t - py);
            (cx * cx + cy * cy).sqrt()
        };
        let sections = self.inspector.as_ref().map(|i| i.result.sections.clone()).unwrap_or_default();
        let mut best: Option<(usize, f32)> = None;
        for (k, s) in sections.iter().enumerate() {
            for seg in s.points.windows(2) {
                let pr = |p: [f64; 3]| cam.project(glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), w, h);
                if let (Some(a), Some(b)) = (pr(seg[0]), pr(seg[1])) {
                    let d = near(a, b);
                    if d <= 6.0 && best.is_none_or(|(_, e)| d < e) {
                        best = Some((k, d));
                    }
                }
            }
        }
        let hit = if best.is_none() { self.element_at(x, y, panel) } else { None };
        let Some(i) = &mut self.inspector else { return false };
        let k = best.map(|b| b.0).or_else(|| hit.and_then(|e| i.result.sections.iter().position(|s| s.elements.contains(&e))));
        if k == i.hover {
            return false;
        }
        i.hover = k;
        let text = match k.map(|k| &i.result.sections[k]) {
            Some(s) => format!(
                "Section {}{}: flow {:.1} l/s, {:.2} m/s; static pressure {} → {} Pa; loss {} Pa",
                s.id,
                if s.critical { " (critical path)" } else { "" },
                s.flow * 1000.0,
                s.velocity,
                opt(s.pressure_in, 1),
                opt(s.pressure_out, 1),
                opt(s.loss, 1)
            ),
            None => String::new(),
        };
        panel.set_tooltip(&text);
        if !text.is_empty() {
            self.frame.set_status_text(&text, 0);
        }
        true
    }

    /// The flow arrows of the session's view.
    pub(crate) fn inspector_lines(&self, index: usize) -> Vec<Lines> {
        let Some(i) = &self.inspector else { return Vec::new() };
        if self.views.get(index).is_none_or(|v| v.id != i.view) {
            return Vec::new();
        }
        let mut normal = Vec::new();
        let mut critical = Vec::new();
        let mut hovered = Vec::new();
        for (k, s) in i.result.sections.iter().enumerate() {
            let segs = arrows(&s.points);
            if Some(k) == i.hover {
                hovered.extend(segs);
            } else if s.critical {
                critical.extend(segs);
            } else {
                normal.extend(segs);
            }
        }
        vec![
            Lines { color: FLOW, width: 1.0, segments: normal },
            Lines { color: super::mep_ui::WARN, width: 2.5, segments: critical },
            Lines { color: HOVER, width: 3.0, segments: hovered },
        ]
    }

    /// System Inspector › Properties: every section's values and the terminals' excess pressure.
    pub(crate) fn inspector_properties(&mut self) {
        let frame = self.frame;
        let Some(i) = &self.inspector else {
            return frame.set_status_text("Start the System Inspector first", 0);
        };
        let r = i.result.clone();
        let hover = i.hover;
        let Some(store) = self.store_now() else { return };
        let name = |id: ElementId| store.get(id).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("#{}", id.0));
        let rows: Vec<Vec<String>> = r
            .sections
            .iter()
            .map(|s| {
                let excess = s.terminal.and_then(|t| r.terminals.iter().find(|x| x.element == t)).map(|t| format!("{:.1}", t.excess)).unwrap_or_default();
                vec![
                    s.id.to_string(),
                    s.elements.len().to_string(),
                    format!("{:.1}", s.flow * 1000.0),
                    format!("{:.2}", s.velocity),
                    opt(s.loss, 1),
                    opt(s.pressure_in, 1),
                    opt(s.pressure_out, 1),
                    if s.critical { "Yes".into() } else { String::new() },
                    s.terminal.map(name).unwrap_or_default(),
                    excess,
                ]
            })
            .collect();
        let intro = format!(
            "{}: {:.1} l/s; critical-path loss {:.1} Pa, which the equipment is taken to supply. A terminal's excess pressure is the critical-path loss less its own path's.",
            r.name,
            r.total_flow * 1000.0,
            r.critical_loss.unwrap_or(0.0)
        );
        let (code, k) = super::mep_loads_ui::list_dialog(
            &frame,
            "System Inspector",
            &intro,
            &[("Section", 60), ("Elements", 65), ("Flow (l/s)", 75), ("Velocity (m/s)", 95), ("Loss (Pa)", 70), ("Pressure in (Pa)", 105), ("Pressure out (Pa)", 110), ("Critical", 60), ("Terminal", 110), ("Excess (Pa)", 80)],
            &rows,
            &[(SELECT, "Select", "Select the section's elements")],
            hover,
        );
        if code == SELECT
            && let Some(s) = k.and_then(|k| r.sections.get(k))
        {
            // Selecting is not an edit: the session stays.
            self.select_ids(&s.elements, super::select::Combine::Replace);
        }
    }
}
