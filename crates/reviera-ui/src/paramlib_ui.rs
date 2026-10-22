// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Settings › Parameter Libraries (DM-SPAR-001): the libraries open in this session
//! (Open, New, Close) and, for one, its definitions (New, Edit, Deprecate, Add to Project, Import
//! Legacy, Export IFC Templates). Changes are written to the library file at once; they are not
//! project changes.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::reinforcement_ui::Form;
use super::{Gui, error};

const OPEN: i32 = 7601;
const NEW: i32 = 7602;
const CLOSE: i32 = 7603;
const DEFS: i32 = 7604;
const D_NEW: i32 = 7611;
const D_EDIT: i32 = 7612;
const D_DEPRECATE: i32 = 7613;
const D_PROJECT: i32 = 7614;
const D_IMPORT: i32 = 7615;
const D_EXPORT: i32 = 7616;

fn file(frame: &Frame, message: &str, wildcard: &str, save: bool) -> Option<String> {
    let style = if save { FileDialogStyle::Save | FileDialogStyle::OverwritePrompt } else { FileDialogStyle::Open | FileDialogStyle::FileMustExist };
    let d = FileDialog::builder(frame).with_message(message).with_wildcard(wildcard).with_style(style).build();
    (d.show_modal() == ID_OK).then(|| d.get_path()).flatten()
}

impl Gui {
    /// Manage › Settings › Parameter Libraries.
    pub(crate) fn parameter_libraries(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = self.run("paramlib.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|l| {
                    vec![
                        format!("{}{}", l["name"].as_str().unwrap_or(""), if l["read_only"].as_bool() == Some(true) { " 🔒" } else { "" }),
                        l["version"].as_str().unwrap_or("").to_owned(),
                        l["definitions"].as_u64().unwrap_or(0).to_string(),
                        l["path"].as_str().unwrap_or("").to_owned(),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Parameter Libraries",
                "Shared parameter definitions in open .rplib.json files. Changes are written to the library file at once; the project keeps its own copy of what it uses.",
                &[("Library", 180), ("Version", 70), ("Definitions", 80), ("File", 260)],
                &rows,
                &[(OPEN, "Open…", "Open a library file"), (NEW, "New…", "Make a library file"), (CLOSE, "Close", "Close the library"), (DEFS, "Definitions…", "Its definitions")],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| list.get(k)).cloned();
            let result = match (code, chosen) {
                (OPEN, _) => match file(&frame, "Open Parameter Library", "Parameter libraries (*.rplib.json)|*.rplib.json|All files (*.*)|*.*", false) {
                    Some(p) => self.run("paramlib.open", json!({ "path_or_url": p })).map(|_| ()),
                    None => continue,
                },
                (NEW, _) => {
                    let Some(p) = file(&frame, "New Parameter Library", "Parameter libraries (*.rplib.json)|*.rplib.json", true) else { continue };
                    let p = if p.ends_with(".rplib.json") { p } else { format!("{}.rplib.json", p.trim_end_matches(".json")) };
                    let d = TextEntryDialog::builder(&frame, "Library name:", "New Parameter Library").build();
                    if d.show_modal() != ID_OK {
                        continue;
                    }
                    let name = d.get_value().unwrap_or_default();
                    self.run("paramlib.create", json!({ "path": p, "name": name })).map(|_| ())
                }
                (CLOSE, Some(l)) => self.run("paramlib.close", json!({ "library_id": l["library_id"] })).map(|_| ()),
                (DEFS, Some(l)) => {
                    self.library_definitions(&l);
                    Ok(())
                }
                (CLOSE | DEFS, None) => continue,
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }

    fn library_definitions(&mut self, l: &Value) {
        let frame = self.frame;
        let id = l["library_id"].as_str().unwrap_or("").to_owned();
        let mut selected = None;
        loop {
            let found = self.run("paramlib.search", json!({ "deprecated": true })).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let defs: Vec<Value> = found.into_iter().filter(|f| f["library_id"] == json!(id)).collect();
            let rows: Vec<Vec<String>> = defs
                .iter()
                .map(|f| {
                    let d = &f["definition"];
                    vec![
                        d["name"].as_str().unwrap_or("").to_owned(),
                        d["data_type"].as_str().unwrap_or("").to_owned(),
                        d["labels"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                        if d["deprecated"].as_bool() == Some(true) { "Deprecated".into() } else { String::new() },
                        if f["in_project"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                &format!("{} — Definitions", l["name"].as_str().unwrap_or("")),
                "Definitions are identified by GUID; they are deprecated, never removed, and their data type never changes.",
                &[("Name", 180), ("Data type", 100), ("Labels", 150), ("Status", 90), ("In project", 70)],
                &rows,
                &[
                    (D_NEW, "New…", "Add a definition"),
                    (D_EDIT, "Edit…", "Change its name, tooltip, labels or IFC home"),
                    (D_DEPRECATE, "Deprecate", "Mark it deprecated"),
                    (D_PROJECT, "Add to Project…", "Bind it to categories of the project as a shared parameter"),
                    (D_IMPORT, "Import Legacy…", "Import a legacy shared-parameter text file"),
                    (D_EXPORT, "Export IFC Templates…", "Write the library as IFC property set templates"),
                ],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| defs.get(k)).map(|f| f["definition"].clone());
            let result = match (code, chosen) {
                (D_NEW, _) => match self.definition_form(None) {
                    Some(def) => self.run("paramlib.definition.add", json!({ "library_id": id, "def": def })).map(|_| ()),
                    None => continue,
                },
                (D_EDIT, Some(d)) => match self.definition_form(Some(&d)) {
                    Some(changes) => self.run("paramlib.definition.edit", json!({ "library_id": id, "guid": d["guid"], "changes": changes })).map(|_| ()),
                    None => continue,
                },
                (D_DEPRECATE, Some(d)) => self.run("paramlib.definition.deprecate", json!({ "library_id": id, "guid": d["guid"] })).map(|_| ()),
                (D_PROJECT, Some(d)) => {
                    let f = Form::new(&frame, "Add to Project");
                    let cats = f.text("Categories", "cat.walls", "Category keys, comma separated (cat.walls, cat.doors, …)");
                    let inst = f.check("Binding", "Instance parameter (else type)", true);
                    let ok = f.show("", "Add");
                    let (c, i) = (cats.get_value(), inst.get_value());
                    f.dialog.destroy();
                    if !ok {
                        continue;
                    }
                    let categories: Vec<String> = c.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
                    let r = self.run("paramlib.add_to_project", json!({ "library_id": id, "guid": d["guid"], "categories": categories, "is_instance": i }));
                    match r {
                        Err(e) if e.contains("keep the project's") => {
                            let ask = MessageDialog::builder(&frame, &format!("{e}\n\nUpdate the project's copy from the library?"), "Add to Project")
                                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                                .build();
                            let choice = if ask.show_modal() == ID_YES { "update" } else { "keep" };
                            self.run("paramlib.add_to_project", json!({ "library_id": id, "guid": d["guid"], "categories": categories, "is_instance": i, "on_conflict": choice })).map(|_| ())
                        }
                        other => other.map(|_| ()),
                    }
                }
                (D_IMPORT, _) => match file(&frame, "Import Shared Parameters", "Text files (*.txt)|*.txt|All files (*.*)|*.*", false) {
                    Some(p) => self.run("paramlib.import_legacy", json!({ "path": p, "target_library_id": id })).map(|v| {
                        frame.set_status_text(&format!("{} imported, {} already there", v["added"], v["skipped"]), 0);
                    }),
                    None => continue,
                },
                (D_EXPORT, _) => match file(&frame, "Export IFC Templates", "IFC files (*.ifc)|*.ifc", true) {
                    Some(p) => self.run("paramlib.export_ifc_templates", json!({ "library_id": id, "path": p })).map(|n| {
                        frame.set_status_text(&format!("{n} property templates written"), 0);
                    }),
                    None => continue,
                },
                (D_EDIT | D_DEPRECATE | D_PROJECT, None) => continue,
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }

    /// A definition's fields: a new one's (`paramlib.definition.add` def) or changes.
    fn definition_form(&mut self, d: Option<&Value>) -> Option<Value> {
        const TYPES: [&str; 10] = ["text", "multiline_text", "integer", "number", "yes_no", "url", "length", "area", "volume", "angle"];
        let f = Form::new(&self.frame, if d.is_some() { "Edit Definition" } else { "New Definition" });
        let name = f.text("Name", d.and_then(|d| d["name"].as_str()).unwrap_or(""), "Free of [ ] { } | ; < >");
        let types: Vec<String> = TYPES.iter().map(|t| (*t).to_owned()).collect();
        let ty = d.is_none().then(|| f.choice("Data type", &types, 0));
        let tooltip = f.text("Tooltip", d.and_then(|d| d["tooltip"].as_str()).unwrap_or(""), "");
        let labels = f.text("Labels", &d.and_then(|d| d["labels"].as_array()).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(), "Comma separated");
        let ifc = f.text(
            "IFC home",
            &d.map(|d| &d["ifc"]).filter(|i| i.is_object()).map(|i| format!("{}.{}", i["pset"].as_str().unwrap_or(""), i["property"].as_str().unwrap_or(""))).unwrap_or_default(),
            "PropertySet.Property; blank: none",
        );
        let ok = f.show("", "OK");
        let mut out = json!({
            "name": name.get_value().trim(),
            "tooltip": tooltip.get_value(),
            "labels": labels.get_value().split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect::<Vec<_>>(),
        });
        if let Some(t) = &ty {
            out["data_type"] = json!(TYPES[t.get_selection().map_or(0, |i| i as usize)]);
        }
        if let Some((p, q)) = ifc.get_value().trim().split_once('.') {
            out["ifc"] = json!({ "pset": p, "property": q });
        }
        f.dialog.destroy();
        ok.then_some(out)
    }
}
