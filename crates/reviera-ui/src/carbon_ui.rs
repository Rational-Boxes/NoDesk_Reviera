// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analyze › Carbon › **Embodied Carbon** (ANA-CARB-001): the project's materials with their
//! volume and match (Import Factors… reads a CSV library, Match… picks a factor, Auto-match
//! proposes by name to confirm, Unmatch), then Run: the totals by stage (module D and biogenic
//! apart), the share left unmatched, and the results by category and by material, with Export….

use reviera_core::embodied_carbon as ec;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const MATCH: i32 = 7202;
const IMPORT: i32 = 7801;
const AUTO: i32 = 7802;
const UNMATCH: i32 = 7803;
const RUN: i32 = 7804;
const RESULTS: i32 = 7805;
const EXPORT: i32 = 7806;

fn kg(v: f64) -> String {
    if v.abs() >= 1000.0 { format!("{:.2} t", v / 1000.0) } else { format!("{v:.0} kg") }
}

impl Gui {
    pub(crate) fn embodied_carbon_dialog(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let rows = match self.run("embodied_carbon.materials", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let Some(store) = self.store_now() else { return };
            let data = ec::data(&store);
            // A factor reference by its factor's name.
            let name_of = |r: &str| -> String {
                r.split_once('/')
                    .and_then(|(l, f)| data.libraries.iter().find(|x| x.id == l)?.factors.iter().find(|x| x.id == f))
                    .map_or_else(|| r.to_owned(), |f| f.name.clone())
            };
            let lines: Vec<Vec<String>> = rows
                .iter()
                .map(|m| {
                    vec![
                        m["material"].as_str().unwrap_or("").to_owned(),
                        format!("{:.3}", m["volume"].as_f64().unwrap_or(0.0)),
                        m["elements"].to_string(),
                        m["status"].as_str().unwrap_or("").to_owned(),
                        m["factor_name"].as_str().map(str::to_owned).or_else(|| m["proposal"].as_str().map(|p| format!("(proposed: {})", name_of(p)))).unwrap_or_default(),
                    ]
                })
                .collect();
            let libs = if data.libraries.is_empty() {
                "No factor library yet: import one (Reviera ships none).".to_owned()
            } else {
                format!("Libraries: {}.", data.libraries.iter().map(|l| format!("{} ({} factors)", l.name, l.factors.len())).collect::<Vec<_>>().join(", "))
            };
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Embodied Carbon",
                &format!("Walls, floors, ceilings and roofs by material, matched to emission factors. Unmatched materials are reported, never guessed. {libs}"),
                &[("Material", 180), ("Volume m³", 80), ("Elements", 70), ("Status", 80), ("Factor", 220)],
                &lines,
                &[
                    (IMPORT, "Import Factors…", "Read a factor library in the Reviera CSV format"),
                    (MATCH, "Match…", "Pick the selected material's factor"),
                    (AUTO, "Auto-match", "Propose factors by name, then confirm them"),
                    (UNMATCH, "Unmatch", "Remove the selected material's factor"),
                    (RUN, "Run", "Calculate and keep a run"),
                    (RESULTS, "Results…", "The latest run's results"),
                ],
                selected,
            );
            selected = at;
            let material = at.and_then(|k| rows.get(k)).and_then(|m| m["material"].as_str()).map(str::to_owned);
            let result: Result<(), String> = match code {
                IMPORT => {
                    let fd = FileDialog::builder(&frame)
                        .with_message("Import Emission Factors")
                        .with_wildcard("CSV files (*.csv)|*.csv;*.CSV|All files (*.*)|*.*")
                        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                        .build();
                    if fd.show_modal() != ID_OK {
                        continue;
                    }
                    let Some(path) = fd.get_path() else { continue };
                    self.run("embodied_carbon.import_factors", json!({ "path": path })).map(|v| frame.set_status_text(&format!("{} factors imported", v["factors"]), 0))
                }
                MATCH => {
                    let Some(m) = material else { continue };
                    let mut refs = Vec::new();
                    let mut labels = Vec::new();
                    for l in &data.libraries {
                        for f in &l.factors {
                            refs.push(format!("{}/{}", l.id, f.id));
                            labels.push(format!("{} — {} ({} {:?})", f.name, l.name, f.stages.a1_a3, f.unit));
                        }
                    }
                    if labels.is_empty() {
                        error(&frame, "Import a factor library first");
                        continue;
                    }
                    let refs_s: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let d = SingleChoiceDialog::builder(&frame, &format!("Factor for {m}:"), "Match Material", &refs_s).build();
                    let k = (d.show_modal() == ID_OK).then(|| usize::try_from(d.get_selection()).ok()).flatten();
                    d.destroy();
                    let Some(k) = k else { continue };
                    self.run("embodied_carbon.match", json!({ "material": m, "factor_ref": refs[k] })).map(|_| ())
                }
                AUTO => match self.run("embodied_carbon.auto_match", json!({})) {
                    Ok(p) => {
                        let list: Vec<String> = p.as_object().into_iter().flatten().map(|(m, r)| format!("{m} → {}", name_of(r.as_str().unwrap_or("")))).collect();
                        if list.is_empty() {
                            frame.set_status_text("No proposals: no factor shares a word with an unmatched material", 0);
                            continue;
                        }
                        let d = MessageDialog::builder(&frame, &format!("Apply these matches?\n\n{}", list.join("\n")), "Auto-match").with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build();
                        if d.show_modal() != ID_YES {
                            continue;
                        }
                        self.run("embodied_carbon.auto_match", json!({ "apply": true })).map(|_| ())
                    }
                    Err(e) => Err(e),
                },
                UNMATCH => {
                    let Some(m) = material else { continue };
                    self.run("embodied_carbon.match", json!({ "material": m, "factor_ref": Value::Null })).map(|_| ())
                }
                // A compute job: the results open when it is collected.
                RUN => match self.run("embodied_carbon.run", json!({})) {
                    Ok(v) => {
                        self.watch_job(v["job_id"].as_str().unwrap_or_default().to_owned(), "embodied_carbon.collect", "Embodied carbon");
                        return;
                    }
                    Err(e) => Err(e),
                },
                RESULTS => {
                    self.carbon_results();
                    continue;
                }
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }

    /// The latest run: totals, then by category and by material.
    pub(crate) fn carbon_results(&mut self) {
        let frame = self.frame;
        let res = match self.run("embodied_carbon.results", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let run: ec::Run = match serde_json::from_value(res["run"].clone()) {
            Ok(r) => r,
            Err(e) => return error(&frame, &e.to_string()),
        };
        let t = &run.totals;
        let mut intro = format!(
            "Run {} ({} years): A–C total {} (A1–A3 {}, A4 {}, A5 {}, B4 {}, C1–C4 {}). Module D {} and biogenic {} are reported apart, not netted. Unmatched: {:.1} % of the volume.",
            run.id,
            run.options.study_period,
            kg(t.total),
            kg(t.a1_a3),
            kg(t.a4),
            kg(t.a5),
            kg(t.b4),
            kg(t.c1_c4),
            kg(t.d),
            kg(t.biogenic),
            run.unmatched_share * 100.0
        );
        if let Some(p) = run.per_m2 {
            intro.push_str(&format!(" {p:.1} kg CO₂e/m²."));
        }
        if res["stale"] == true {
            intro.push_str(&format!(" Stale: {} element(s) changed since; run again.", res["stale_elements"].as_array().map_or(0, Vec::len)));
        }
        let mut rows: Vec<Vec<String>> = Vec::new();
        for (what, lines) in [("Category", &run.by_category), ("Material", &run.by_material)] {
            for l in lines {
                rows.push(vec![what.into(), l.key.clone(), kg(l.gwp.a1_a3), kg(l.gwp.total), kg(l.gwp.d), kg(l.gwp.biogenic), format!("{:.3}", l.volume)]);
            }
        }
        for x in &run.excluded {
            rows.push(vec!["Left out".into(), format!("{} ({})", x.material, x.reason), String::new(), String::new(), String::new(), String::new(), format!("{:.3}", x.volume)]);
        }
        let (code, _) = super::mep_loads_ui::list_dialog(
            &frame,
            "Embodied Carbon Results",
            &intro,
            &[("By", 80), ("Name", 180), ("A1–A3", 80), ("A–C", 80), ("D", 70), ("Biogenic", 70), ("m³", 70)],
            &rows,
            &[(EXPORT, "Export…", "Write the run as CSV or JSON")],
            None,
        );
        if code == EXPORT {
            let fd = FileDialog::builder(&frame)
                .with_message("Export Embodied Carbon")
                .with_wildcard("CSV (*.csv)|*.csv|JSON (*.json)|*.json")
                .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                .build();
            if fd.show_modal() != ID_OK {
                return;
            }
            let Some(path) = fd.get_path() else { return };
            let format = if path.to_lowercase().ends_with(".json") { "json" } else { "csv" };
            match self.run("embodied_carbon.export", json!({ "run_id": run.id, "path": path, "format": format })) {
                Ok(_) => frame.set_status_text("Embodied carbon exported", 0),
                Err(e) => error(&frame, &e),
            }
        }
    }
}
