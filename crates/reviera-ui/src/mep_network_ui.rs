// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Pressure Drop (MEP-CALC-002, MEP-CALC-004): Calculate Networks, the Analytical
//! Segments of a system (flow, velocity, friction, loss, static pressure, the critical path;
//! select, override a loss, recalculate) and the Pressure Loss Report (systems, sections,
//! presets, CSV or HTML).

use reviera_core::ElementId;
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
