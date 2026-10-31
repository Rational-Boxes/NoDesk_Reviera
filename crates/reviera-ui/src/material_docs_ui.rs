// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! File › Material Documents… (COL-FE-006…008): the project's materials, each with its FileEngine
//! documentation folder (Link Folder…, Unlink), the folder's documents and their types (Set
//! Type…), the values taken from them, Read EPD… and Read HPD… (an extraction, then a review where
//! the user ticks what to accept), and Report to FileEngine… (the documentation and carbon report,
//! published to a folder).

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::fileengine_ui::{Pick, stamp};
use super::{Gui, error};

const LINK: i32 = 7401;
const UNLINK: i32 = 7402;
const SET_TYPE: i32 = 7403;
const READ_EPD: i32 = 7404;
const READ_HPD: i32 = 7405;
const REPORT: i32 = 7406;
const RELOAD: i32 = 7407;

const DOC_TYPES: &[&str] = &["EPD", "HPD", "specification", "certification", "installation", "O&M", "warranty", "SDS", "drawing", "other"];

fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| if f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.0}") } else { format!("{f}") }),
        Value::Bool(b) => if *b { "yes".into() } else { "no".into() },
        _ => String::new(),
    }
}

fn list(dialog: &Dialog, columns: &[(&str, i32)], rows: &[Vec<String>], single: bool) -> ListCtrl {
    let style = if single { ListCtrlStyle::Report | ListCtrlStyle::SingleSel } else { ListCtrlStyle::Report };
    let l = ListCtrl::builder(dialog).with_style(style).build();
    for (k, (h, w)) in columns.iter().enumerate() {
        l.insert_column(k as i64, h, ListColumnFormat::Left, *w);
    }
    for (k, row) in rows.iter().enumerate() {
        l.insert_item(k as i64, &row[0], None);
        for (c, text) in row.iter().enumerate().skip(1) {
            l.set_item_text_by_column(k as i64, c as i32, text);
        }
    }
    l
}

fn selected(l: &ListCtrl) -> Vec<usize> {
    let mut out = Vec::new();
    let mut k = -1;
    loop {
        k = i64::from(l.get_next_item(k, ListNextItemFlag::All, ListItemState::Selected));
        if k < 0 {
            return out;
        }
        out.push(k as usize);
    }
}

struct Closed {
    code: i32,
    material: Option<usize>,
    document: Option<usize>,
}

fn main_dialog(frame: &Frame, materials: &[Value], at: usize, docs: &[Value], note: &str) -> Closed {
    let dialog = Dialog::builder(frame, "Material Documents").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    let top = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&dialog).with_label("Material").build();
    let names: Vec<String> = materials
        .iter()
        .map(|m| {
            let state = match (m["link"].is_object(), m["values"].as_object().is_some_and(|v| !v.is_empty())) {
                (true, true) => "documented",
                (true, false) => "linked",
                (false, true) => "values, no folder",
                (false, false) if m["in_use"] == true => "in use, not documented",
                _ => "not in use",
            };
            format!("{} ({state})", m["material"].as_str().unwrap_or(""))
        })
        .collect();
    let choice = Choice::builder(&dialog).with_choices(if names.is_empty() { vec!["No materials: walls, floors, ceilings and roofs name them".into()] } else { names.clone() }).build();
    choice.set_selection(at as u32);
    top.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
    top.add(&choice, 1, SizerFlag::Expand, 0);
    let d = dialog;
    choice.on_selection_changed(move |_| d.end_modal(RELOAD));

    let m = materials.get(at);
    let linked = m.is_some_and(|m| m["link"].is_object());
    let where_ = StaticText::builder(&dialog)
        .with_label(&match m.map(|m| &m["link"]).filter(|l| l.is_object()) {
            Some(l) => format!("Documentation folder: {}", l["folder_name"].as_str().unwrap_or("")),
            None => "No documentation folder: Link Folder… chooses one on FileEngine.".into(),
        })
        .build();
    let doc_rows: Vec<Vec<String>> = docs
        .iter()
        .map(|d| vec![d["name"].as_str().unwrap_or("").to_owned(), d["doc_type"].as_str().unwrap_or("—").to_owned(), stamp(d["version"].as_str().unwrap_or("")), d["modified_by"].as_str().unwrap_or("").to_owned()])
        .collect();
    let doc_view = list(&dialog, &[("Document", 360), ("Type", 110), ("Version (UTC)", 150), ("By", 200)], &doc_rows, true);
    doc_view.set_min_size(Size::new(840, 150));

    let mut value_rows = Vec::new();
    for (set, fields) in m.and_then(|m| m["values"].as_object()).into_iter().flatten() {
        for t in fields.as_object().into_iter().flatten().map(|(_, t)| t) {
            value_rows.push(vec![
                set.to_uppercase(),
                t["label"].as_str().unwrap_or("").to_owned(),
                shown(&t["value"]),
                t["unit"].as_str().unwrap_or("").to_owned(),
                format!("{} ({})", t["source"]["file_name"].as_str().unwrap_or(""), stamp(t["source"]["version"].as_str().unwrap_or(""))),
            ]);
        }
    }
    let value_view = list(&dialog, &[("Set", 50), ("Value", 150), ("", 200), ("Unit", 110), ("From", 330)], &value_rows, true);
    value_view.set_min_size(Size::new(840, 170));
    let note_text = StaticText::builder(&dialog).with_label(note).build();
    note_text.wrap(840);

    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let has = m.is_some();
    for (code, text, tip, on) in [
        (LINK, "Link Folder…", "Choose the material's documentation folder on FileEngine", has),
        (UNLINK, "Unlink", "Forget the folder (the values stay)", linked),
        (SET_TYPE, "Set Type…", "Mark the chosen document as an EPD, an HPD, a specification, …", linked && !docs.is_empty()),
        (READ_EPD, "Read EPD…", "Read the EPD's values from the folder's EPD documents, then choose which to keep", linked),
        (READ_HPD, "Read HPD…", "Read the HPD's values from the folder's HPD documents, then choose which to keep", linked),
        (REPORT, "Report to FileEngine…", "Write the material documentation and embodied carbon report, and publish it to a FileEngine folder", true),
    ] {
        let b = Button::builder(&dialog).with_label(text).build();
        b.set_tooltip(tip);
        b.enable(on);
        let d = dialog;
        b.on_click(move |_| d.end_modal(code));
        row.add(&b, 0, SizerFlag::Right, 4);
    }
    row.add_stretch_spacer(1);
    let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
    row.add(&close, 0, SizerFlag::empty(), 0);

    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add_sizer(&top, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add(&where_, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    sizer.add(&doc_view, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 12);
    let vl = StaticText::builder(&dialog).with_label("Values taken from the documents").build();
    sizer.add(&vl, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 12);
    sizer.add(&value_view, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 12);
    sizer.add(&note_text, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let code = dialog.show_modal();
    let out = Closed { code, material: choice.get_selection().map(|i| i as usize).filter(|_| !names.is_empty()), document: usize::try_from(doc_view.get_first_selected_item()).ok() };
    dialog.destroy();
    out
}

/// The review of an extraction: which found values to accept, and where else they go.
struct Accepted {
    fields: Vec<String>,
    to_folder: bool,
    carbon: bool,
}

fn review(frame: &Frame, run: &Value) -> Option<Accepted> {
    let epd = run["set"] == "epd";
    let title = format!("{} values read from the documents", run["set"].as_str().unwrap_or("").to_uppercase());
    let dialog = Dialog::builder(frame, &title).with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    // One row per field and document it was found in; then the fields found nowhere.
    let mut rows = Vec::new();
    let mut keys: Vec<Option<String>> = Vec::new();
    let docs = run["documents"].as_array().cloned().unwrap_or_default();
    let several = docs.len() > 1;
    for d in &docs {
        for f in d["fields"].as_array().into_iter().flatten().filter(|f| f["found"] == true) {
            rows.push(vec![
                f["label"].as_str().unwrap_or("").to_owned(),
                shown(&f["value"]),
                f["unit"].as_str().unwrap_or("").to_owned(),
                format!("“{}”", f["quote"].as_str().unwrap_or("")),
                format!("{}{}", d["name"].as_str().unwrap_or(""), if d["cached"] == true { " (cached)" } else { "" }),
            ]);
            keys.push(Some(f["field"].as_str().unwrap_or("").to_owned()));
        }
    }
    let missing: Vec<String> = docs
        .first()
        .and_then(|d| d["fields"].as_array())
        .into_iter()
        .flatten()
        .filter(|f| docs.iter().all(|d| d["fields"].as_array().into_iter().flatten().any(|g| g["field"] == f["field"] && g["found"] != true)))
        .map(|f| f["label"].as_str().unwrap_or("").to_owned())
        .collect();
    for m in &missing {
        rows.push(vec![m.clone(), "not found".into(), String::new(), "No supporting quote in the documents: check by hand".into(), String::new()]);
        keys.push(None);
    }
    let view = list(&dialog, &[("Value", 150), ("Read", 180), ("Unit", 100), ("Quote", 380), ("Document", 260)], &rows, false);
    view.set_min_size(Size::new(1080, 300));
    for (k, key) in keys.iter().enumerate() {
        if key.is_some() {
            view.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
        }
    }
    let skipped: Vec<String> = run["skipped"].as_array().into_iter().flatten().map(|s| format!("{} ({})", s["name"].as_str().or(s["file_uid"].as_str()).unwrap_or(""), s["reason"].as_str().unwrap_or(""))).collect();
    let note = StaticText::builder(&dialog)
        .with_label(&format!(
            "Select the values to keep (Ctrl+click to add or remove one). Nothing is written until you Apply, and Undo takes it all back.{}{}",
            if several { " Where documents disagree, keep one of them." } else { "" },
            if skipped.is_empty() { String::new() } else { format!("\nNot read: {}", skipped.join("; ")) }
        ))
        .build();
    note.wrap(1080);
    let to_folder = CheckBox::builder(&dialog).with_label("Also write them to the documentation folder's metadata on FileEngine").build();
    let carbon = CheckBox::builder(&dialog).with_label("Use the GWP A1–A3 as the material's embodied-carbon factor").build();
    carbon.set_value(epd);
    carbon.enable(epd);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
    let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Apply").build();
    buttons.add(&cancel, 0, SizerFlag::Right, 6);
    buttons.add(&ok, 0, SizerFlag::empty(), 0);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add(&view, 1, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add(&note, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 12);
    sizer.add(&to_folder, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 12);
    sizer.add(&carbon, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 12);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::All, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let ok = dialog.show_modal() == ID_OK;
    let mut fields: Vec<String> = selected(&view).into_iter().filter_map(|k| keys.get(k).cloned().flatten()).collect();
    fields.dedup();
    let out = Accepted { fields, to_folder: to_folder.get_value(), carbon: epd && carbon.get_value() };
    dialog.destroy();
    ok.then_some(out)
}

impl Gui {
    /// File › Material Documents…
    pub(crate) fn material_documents(&mut self) {
        let frame = self.frame;
        if self.app.active().is_none() {
            return error(&frame, "Open a project first.");
        }
        let mut at = 0usize;
        let mut note = String::new();
        loop {
            let materials = match self.run("docs.materials", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The materials could not be read:\n{e}")),
            };
            at = at.min(materials.len().saturating_sub(1));
            let m = materials.get(at).cloned();
            let name = m.as_ref().and_then(|m| m["material"].as_str()).unwrap_or("").to_owned();
            let mut docs = Vec::new();
            if m.as_ref().is_some_and(|m| m["link"].is_object()) {
                match self.run("docs.list", json!({ "material": name })) {
                    Ok(v) => docs = v.as_array().cloned().unwrap_or_default(),
                    Err(e) => note = format!("The folder could not be listed: {e}"),
                }
            }
            if note.is_empty() {
                note = "Read EPD… and Read HPD… read the values from the folder's documents of that type, with the words each was read from; you choose which to keep.".into();
            }
            let closed = main_dialog(&frame, &materials, at, &docs, &note);
            note.clear();
            if let Some(k) = closed.material {
                if k != at {
                    at = k;
                    continue;
                }
            }
            let conn = m.as_ref().and_then(|m| m["link"]["connection_id"].as_str()).map(str::to_owned);
            match closed.code {
                LINK => {
                    if let Some(Pick { connection_id, folder_uid, .. }) = self.fileengine_pick_folder("Choose the Documentation Folder") {
                        match self.run("docs.link", json!({ "material": name, "connection_id": connection_id, "folder_uid": folder_uid })) {
                            Ok(l) => note = format!("{name} is linked to {}.", l["folder_name"].as_str().unwrap_or("")),
                            Err(e) => note = format!("Not linked: {e}"),
                        }
                    }
                }
                UNLINK => {
                    if let Err(e) = self.run("docs.unlink", json!({ "material": name })) {
                        note = format!("Not unlinked: {e}");
                    }
                }
                SET_TYPE => {
                    let (Some(conn), Some(d)) = (conn, closed.document.and_then(|k| docs.get(k))) else {
                        note = "Choose a document first.".into();
                        continue;
                    };
                    let items: Vec<String> = DOC_TYPES.iter().map(|s| (*s).to_owned()).collect();
                    let current = d["doc_type"].as_str().and_then(|t| DOC_TYPES.iter().position(|x| x.eq_ignore_ascii_case(t))).unwrap_or(0);
                    let f = super::reinforcement_ui::Form::new(&frame, &format!("Type of {}", d["name"].as_str().unwrap_or("")));
                    let c = f.choice("Type", &items, current);
                    let ok = f.show("Kept on FileEngine as the document's doc.type.", "Set");
                    let chosen = c.get_selection().map(|i| items[i as usize].clone());
                    f.dialog.destroy();
                    if let (true, Some(t)) = (ok, chosen)
                        && let Err(e) = self.run("docs.set_doc_type", json!({ "connection_id": conn, "file_uid": d["uid"], "doc_type": t }))
                    {
                        note = format!("Not set: {e}");
                    }
                }
                READ_EPD | READ_HPD => {
                    let set = if closed.code == READ_EPD { "epd" } else { "hpd" };
                    frame.set_status_text("Reading the documents (about half a minute each)…", 0);
                    let r = self.run("docs.extract", json!({ "material": name, "set": set }));
                    frame.set_status_text("", 0);
                    let run = match r {
                        Ok(r) => r,
                        Err(e) => {
                            note = format!("Not read: {e}");
                            continue;
                        }
                    };
                    if let Some(a) = review(&frame, &run) {
                        if a.fields.is_empty() {
                            note = "Nothing was kept.".into();
                            continue;
                        }
                        let file_uid = (run["documents"].as_array().map_or(0, Vec::len) > 1).then(|| run["documents"][0]["file_uid"].clone());
                        let mut args = json!({ "run_id": run["run_id"], "material": name, "fields": a.fields, "to_folder": a.to_folder, "carbon": a.carbon });
                        if let Some(f) = file_uid {
                            args["file_uid"] = f;
                        }
                        match self.run("docs.apply_extracted", args) {
                            Ok(o) => {
                                note = format!("{} values written onto {name}.", o["written"].as_array().map_or(0, Vec::len));
                                if let Some(per) = o["factor_per_unit"].as_f64() {
                                    note.push_str(&format!(" Its embodied-carbon factor is now {per:.4} kg CO₂e per {}.", o["factor_unit"].as_str().unwrap_or("")));
                                }
                            }
                            Err(e) => note = format!("Not written: {e}"),
                        }
                    }
                }
                REPORT => self.material_report(),
                RELOAD => {}
                _ => return,
            }
        }
    }

    /// Writes the documentation and carbon report and publishes it to a FileEngine folder.
    fn material_report(&mut self) {
        let frame = self.frame;
        let Some(Pick { connection_id, folder_uid, folder_name }) = self.fileengine_pick_folder("Publish the Report to a Folder") else { return };
        let project = self.current_path().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "model".into());
        let name = format!("{project} material documentation.html");
        let path = std::env::temp_dir().join(format!("reviera-{}-{name}", std::process::id()));
        let r = self.run("docs.report", json!({ "path": path }));
        let report = match r {
            Ok(r) => r,
            Err(e) => return error(&frame, &format!("The report was not written:\n{e}")),
        };
        let publish = |gui: &mut Gui, replace: bool| gui.run("fileengine.publish", json!({ "connection_id": connection_id, "folder_uid": folder_uid, "path": path, "name": name, "replace": replace, "metadata": { "doc.type": "report", "reviera.report": "material documentation" } }));
        let mut r = publish(self, false);
        if r.as_ref().is_err_and(|e| e.starts_with("fileengine.exists"))
            && MessageDialog::builder(&frame, &format!("{name} is already in {folder_name}. Publish this as a new version of it?"), "Report to FileEngine").with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build().show_modal() == ID_YES
        {
            r = publish(self, true);
        }
        let _ = std::fs::remove_file(&path);
        match r {
            Ok(p) => {
                let warnings = report["warnings"].as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("Published {} to {folder_name} (version {}){}", p["name"].as_str().unwrap_or(""), stamp(p["version"].as_str().unwrap_or("")), if warnings > 0 { format!("; {warnings} things to check") } else { String::new() }), 0);
            }
            Err(e) if e.starts_with("fileengine.exists") => {}
            Err(e) => error(&frame, &format!("The report was not published:\n{e}")),
        }
    }
}
