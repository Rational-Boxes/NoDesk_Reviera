// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Settings › Classification (DM-CLS-001): the project's classification systems (Load,
//! Remove); Classify gives the selection a code from a searchable list of a system's codes; Load
//! Keynotes reads a keynote file.

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const LOAD: i32 = 7701;
const REMOVE: i32 = 7702;

impl Gui {
    /// Manage › Settings › Classification Systems.
    pub(crate) fn classification_systems(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = self.run("classification.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|s| {
                    vec![
                        s["name"].as_str().unwrap_or("").to_owned(),
                        s["edition"].as_str().unwrap_or("").to_owned(),
                        s["codes"].as_u64().unwrap_or(0).to_string(),
                        s["used_by"].as_u64().unwrap_or(0).to_string(),
                        s["location"].as_str().unwrap_or("").to_owned(),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Classification Systems",
                "Classification tables loaded into the project, from a CSV of code, parent code and title, or from an IFC file's classification. Classify gives selected types or elements a code.",
                &[("System", 160), ("Edition", 70), ("Codes", 60), ("Classified", 70), ("From", 240)],
                &rows,
                &[(LOAD, "Load…", "Load a table from a CSV or IFC file"), (REMOVE, "Remove", "Remove the system and every code given from it")],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| list.get(k)).cloned();
            let result = match (code, chosen) {
                (LOAD, _) => {
                    let d = FileDialog::builder(&frame)
                        .with_message("Load Classification")
                        .with_wildcard("Classification tables (*.csv;*.ifc)|*.csv;*.ifc|All files (*.*)|*.*")
                        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                        .build();
                    if d.show_modal() != ID_OK {
                        continue;
                    }
                    let Some(path) = d.get_path() else { continue };
                    let stem = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
                    let n = TextEntryDialog::builder(&frame, "System name:", "Load Classification").with_default_value(&stem).build();
                    if n.show_modal() != ID_OK {
                        continue;
                    }
                    let name = n.get_value().unwrap_or(stem);
                    self.run("classification.load", json!({ "path_or_url": path, "name": name })).map(|_| ())
                }
                (REMOVE, Some(s)) => self.run("classification.delete", json!({ "system_id": s["id"] })).map(|_| ()),
                (REMOVE, None) => continue,
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }

    /// Classify: the selection takes a code of a system, found by searching its table.
    pub(crate) fn classify_selection(&mut self) {
        let frame = self.frame;
        let sel: Vec<ElementId> = self.selection();
        if sel.is_empty() {
            return frame.set_status_text("Classify: select the types or elements first", 0);
        }
        let systems = self.run("classification.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if systems.is_empty() {
            return frame.set_status_text("Classify: load a classification system first (Manage › Classification Systems)", 0);
        }
        let names: Vec<String> = systems.iter().map(|s| s["name"].as_str().unwrap_or("").to_owned()).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Classification system:", "Classify", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(sys) = usize::try_from(d.get_selection()).ok().and_then(|k| systems.get(k)).map(|s| s["id"].clone()) else { return };
        let q = TextEntryDialog::builder(&frame, "Search codes and titles (blank: all):", "Classify").build();
        if q.show_modal() != ID_OK {
            return;
        }
        let text = q.get_value().unwrap_or_default();
        let items = self.run("classification.search", json!({ "system_id": sys, "text": text })).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if items.is_empty() {
            return frame.set_status_text("Classify: no code matches", 0);
        }
        let labels: Vec<String> = items.iter().map(|i| format!("{} — {}", i["code"].as_str().unwrap_or(""), i["title"].as_str().unwrap_or(""))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let c = SingleChoiceDialog::builder(&frame, "Code:", "Classify", &refs).build();
        if c.show_modal() != ID_OK {
            return;
        }
        let Some(code) = usize::try_from(c.get_selection()).ok().and_then(|k| items.get(k)).map(|i| i["code"].clone()) else { return };
        match self.run("classification.assign", json!({ "element_ids": sel, "system_id": sys, "code": code })) {
            Ok(_) => {
                self.properties_stale = true;
                frame.set_status_text(&format!("{} element(s) classified {}", sel.len(), code.as_str().unwrap_or("")), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Manage › Settings › Load Keynotes.
    pub(crate) fn load_keynotes(&mut self) {
        let frame = self.frame;
        let d = FileDialog::builder(&frame)
            .with_message("Load Keynotes")
            .with_wildcard("Keynote files (*.txt)|*.txt|All files (*.*)|*.*")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        match self.run("keynotes.load", json!({ "path": path })) {
            Ok(v) => frame.set_status_text(&format!("{} keynotes from {}", v["entries"], v["path"].as_str().unwrap_or("")), 0),
            Err(e) => error(&frame, &e),
        }
    }
}
