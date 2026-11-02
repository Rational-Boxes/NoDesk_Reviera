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
const OPEN_DOC: i32 = 7408;
const ENTER: i32 = 7409;
const PUBLISH: i32 = 7410;

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

fn main_dialog(frame: &Frame, materials: &[Value], at: usize, docs: &[Value], note: &str, fileengine: bool) -> Closed {
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
            format!("{} ({state})", m["label"].as_str().or(m["material"].as_str()).unwrap_or(""))
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
    let local = m.is_some_and(|m| m["link"]["path"].is_string());
    let where_ = StaticText::builder(&dialog)
        .with_label(&match m.map(|m| &m["link"]).filter(|l| l.is_object()) {
            Some(l) if l["path"].is_string() => format!("Documentation folder on this computer: {}", l["path"].as_str().unwrap_or("")),
            Some(l) => format!("Documentation folder on FileEngine: {}", l["folder_name"].as_str().unwrap_or("")),
            None => "No documentation folder: Link Folder… chooses one, on this computer or on FileEngine.".into(),
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

    let has = m.is_some();
    let add = |row: &BoxSizer, items: &[(i32, &str, &str, bool)]| {
        for (code, text, tip, on) in items {
            let b = Button::builder(&dialog).with_label(text).build();
            b.set_tooltip(tip);
            b.enable(*on);
            let (d, code) = (dialog, *code);
            b.on_click(move |_| d.end_modal(code));
            row.add(&b, 0, SizerFlag::Right, 4);
        }
    };
    // The folder and its documents…
    let folder_row = BoxSizer::builder(Orientation::Horizontal).build();
    add(&folder_row, &[
        (LINK, "Link Folder…", "Choose the material's documentation folder, on this computer or on FileEngine", has),
        (UNLINK, "Unlink", "Forget the folder (the values stay)", linked),
        (SET_TYPE, "Set Type…", "Mark the chosen document as an EPD, an HPD, a specification, …", linked && !docs.is_empty()),
        (OPEN_DOC, "Open", "Open the chosen document in the system's viewer", linked && !docs.is_empty()),
        (PUBLISH, "Publish to FileEngine…", "Put this project's local document folders on FileEngine, and optionally point the model at them", local && fileengine),
    ]);
    // …and the values.
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    add(&row, &[
        (READ_EPD, "Read EPD…", "Read the EPD's values from the folder's EPD documents with FileEngine's AI extraction, then choose which to keep", linked && !local),
        (READ_HPD, "Read HPD…", "Read the HPD's values from the folder's HPD documents with FileEngine's AI extraction, then choose which to keep", linked && !local),
        (ENTER, "Enter Values…", "Type an EPD's or HPD's values from the document", has),
        (REPORT, "Report…", "Write the material documentation and embodied carbon report: to a file, or published to FileEngine", true),
    ]);
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
    sizer.add_sizer(&folder_row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 6);
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
    // The server's prompt set it ran, why not Reviera's own if so, and the marked-up PDFs.
    let ps = &run["prompt_set"];
    let mut server = if ps.is_object() { format!("\nRead with the server's prompt set “{}”, revision {}.", ps["name"].as_str().unwrap_or(""), ps["revision"]) } else { String::new() };
    if let Some(n) = run["note"].as_str() {
        server.push_str(&format!(" Note: {n}."));
    }
    let marked: Vec<String> = docs.iter().filter(|d| d["markup"]["status"] == "written").map(|d| format!("{} ({})", d["name"].as_str().unwrap_or(""), d["markup"]["name"].as_str().unwrap_or(""))).collect();
    if !marked.is_empty() {
        server.push_str(&format!("\nMarked-up PDF with the evidence highlighted, beside the document on FileEngine: {}", marked.join("; ")));
    }
    let note = StaticText::builder(&dialog)
        .with_label(&format!(
            "Select the values to keep (Ctrl+click to add or remove one). Nothing is written until you Apply, and Undo takes it all back.{}{}{}",
            if several { " Where documents disagree, keep one of them." } else { "" },
            if skipped.is_empty() { String::new() } else { format!("\nNot read: {}", skipped.join("; ")) },
            server
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
        self.material_documents_at(None);
    }

    /// Architecture › Product Documents (ARC-BATH-006): the Material Documents dialog on the
    /// selected component's type.
    pub(crate) fn product_documents(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let key = self.selection().iter().filter_map(|id| store.get(*id)).find_map(|e| {
            let ty = if reviera_core::mesh_objects::mesh_object_type(e).is_some() { Some(e) } else { reviera_core::mesh_objects::type_of(&store, e).and_then(|(t, _)| store.get(t)) }?;
            Some(reviera_core::product_docs::type_key(ty))
        });
        match key {
            Some(k) => self.material_documents_at(Some(k)),
            None => error(&frame, "Select a placed component (a fixture, a product) first."),
        }
    }

    fn material_documents_at(&mut self, start: Option<String>) {
        let frame = self.frame;
        if self.app.active().is_none() {
            return error(&frame, "Open a project first.");
        }
        let mut at = 0usize;
        let mut start = start;
        let mut note = String::new();
        loop {
            let materials = match self.run("docs.materials", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The materials could not be read:\n{e}")),
            };
            if let Some(k) = start.take()
                && let Some(i) = materials.iter().position(|m| m["material"] == k.as_str())
            {
                at = i;
            }
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
            let fileengine = self.run("fileengine.list_connections", json!({})).ok().and_then(|v| v.as_array().map(|a| !a.is_empty())).unwrap_or(false);
            let local = m.as_ref().is_some_and(|m| m["link"]["path"].is_string());
            if note.is_empty() {
                note = if local {
                    "Enter Values… records an EPD's or HPD's values as you read them from the document. Reading them automatically is a FileEngine feature: Publish to FileEngine… puts the folder there.".into()
                } else {
                    "Read EPD… and Read HPD… read the values from the folder's documents of that type, with the words each was read from; you choose which to keep. Enter Values… types them in.".into()
                };
            }
            let closed = main_dialog(&frame, &materials, at, &docs, &note, fileengine);
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
                    // On this computer, or (with a connection) on FileEngine.
                    let on_server = fileengine && {
                        let f = super::reinforcement_ui::Form::new(&frame, "Link Folder");
                        let items = vec!["A folder on this computer".to_owned(), "A FileEngine folder".to_owned()];
                        let c = f.choice("Documents in", &items, 0);
                        let ok = f.show("", "Choose…");
                        let k = c.get_selection().unwrap_or(0);
                        f.dialog.destroy();
                        if !ok {
                            continue;
                        }
                        k == 1
                    };
                    let args = if on_server {
                        let Some(Pick { connection_id, folder_uid, .. }) = self.fileengine_pick_folder("Choose the Documentation Folder") else { continue };
                        json!({ "material": name, "connection_id": connection_id, "folder_uid": folder_uid })
                    } else {
                        let d = DirDialog::builder(&frame, "Choose the Documentation Folder", "").build();
                        if d.show_modal() != ID_OK {
                            continue;
                        }
                        let Some(path) = d.get_path() else { continue };
                        json!({ "material": name, "path": path })
                    };
                    match self.run("docs.link", args) {
                        Ok(l) => note = format!("{name} is linked to {}.", l["folder_name"].as_str().unwrap_or("")),
                        Err(e) => note = format!("Not linked: {e}"),
                    }
                }
                OPEN_DOC => {
                    let Some(d) = closed.document.and_then(|k| docs.get(k)) else {
                        note = "Choose a document first.".into();
                        continue;
                    };
                    match self.run("docs.document_path", json!({ "material": name, "file_uid": d["uid"] })) {
                        Ok(p) => {
                            launch_default_application(p.as_str().unwrap_or(""));
                        }
                        Err(e) => note = format!("Not opened: {e}"),
                    }
                }
                ENTER => {
                    if let Some(n) = self.enter_values(&name, m.as_ref(), &docs) {
                        note = n;
                    }
                }
                PUBLISH => {
                    if let Some(n) = self.publish_local_documents() {
                        note = n;
                    }
                }
                UNLINK => {
                    if let Err(e) = self.run("docs.unlink", json!({ "material": name })) {
                        note = format!("Not unlinked: {e}");
                    }
                }
                SET_TYPE => {
                    let Some(d) = closed.document.and_then(|k| docs.get(k)) else {
                        note = "Choose a document first.".into();
                        continue;
                    };
                    let items: Vec<String> = DOC_TYPES.iter().map(|s| (*s).to_owned()).collect();
                    let current = d["doc_type"].as_str().and_then(|t| DOC_TYPES.iter().position(|x| x.eq_ignore_ascii_case(t))).unwrap_or(0);
                    let f = super::reinforcement_ui::Form::new(&frame, &format!("Type of {}", d["name"].as_str().unwrap_or("")));
                    let c = f.choice("Type", &items, current);
                    let ok = f.show(if local { "Kept in the model, for the file in the folder." } else { "Kept on FileEngine as the document's doc.type." }, "Set");
                    let chosen = c.get_selection().map(|i| items[i as usize].clone());
                    f.dialog.destroy();
                    if let (true, Some(t)) = (ok, chosen)
                        && let Err(e) = self.run("docs.set_doc_type", if local { json!({ "material": name, "file_uid": d["uid"], "doc_type": t }) } else { json!({ "connection_id": conn, "file_uid": d["uid"], "doc_type": t }) })
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

    /// Writes the documentation and carbon report: to a file, or (with a connection) published to
    /// a FileEngine folder.
    fn material_report(&mut self) {
        let frame = self.frame;
        let fileengine = self.run("fileengine.list_connections", json!({})).ok().and_then(|v| v.as_array().map(|a| !a.is_empty())).unwrap_or(false);
        let publish = fileengine && {
            let f = super::reinforcement_ui::Form::new(&frame, "Material Documentation Report");
            let items = vec!["Save it to a file".to_owned(), "Publish it to FileEngine".to_owned()];
            let c = f.choice("The report", &items, 0);
            let ok = f.show("", "Continue…");
            let k = c.get_selection().unwrap_or(0);
            f.dialog.destroy();
            if !ok {
                return;
            }
            k == 1
        };
        if !publish {
            let fd = FileDialog::builder(&frame).with_message("Save the Material Documentation Report").with_wildcard("HTML (*.html)|*.html").with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt).build();
            if fd.show_modal() != ID_OK {
                return;
            }
            let Some(path) = fd.get_path() else { return };
            match self.run("docs.report", json!({ "path": path })) {
                Ok(r) => frame.set_status_text(&format!("Report written to {path} ({} things to check)", r["warnings"].as_array().map_or(0, Vec::len)), 0),
                Err(e) => error(&frame, &format!("The report was not written:\n{e}")),
            }
            return;
        }
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

    /// Enter Values…: a field set's values typed from a document, with the EPD's carbon factor.
    fn enter_values(&mut self, material: &str, m: Option<&Value>, docs: &[Value]) -> Option<String> {
        let frame = self.frame;
        let sets = self.run("docs.field_sets", json!({})).ok()?.as_array().cloned().unwrap_or_default();
        let names: Vec<String> = sets.iter().map(|s| s["title"].as_str().unwrap_or("").to_owned()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, &format!("Enter Values: {material}"));
        let c = f.choice("From", &names, 0);
        let ok = f.show("", "Next…");
        let at = c.get_selection().unwrap_or(0) as usize;
        f.dialog.destroy();
        if !ok {
            return None;
        }
        let set = sets.get(at)?;
        let id = set["id"].as_str().unwrap_or("");
        let fields = set["fields"].as_array().cloned().unwrap_or_default();
        let current = |k: &str| m.map(|m| shown(&m["values"][id][k]["value"])).unwrap_or_default();
        let f = super::reinforcement_ui::Form::new(&frame, &format!("{} values: {material}", set["title"].as_str().unwrap_or("")));
        let mut boxes = Vec::new();
        for field in &fields {
            let k = field["field"].as_str().unwrap_or("");
            let unit = if field["kind"] == "number" { " (number)" } else { "" };
            boxes.push((k.to_owned(), f.text(&format!("{}{unit}", field["label"].as_str().unwrap_or(k)), &current(k), field["description"].as_str().unwrap_or(""))));
        }
        let mut doc_names = vec!["(not recorded)".to_owned()];
        doc_names.extend(docs.iter().map(|d| d["name"].as_str().unwrap_or("").to_owned()));
        let typed = docs.iter().position(|d| d["doc_type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(set["doc_type"].as_str().unwrap_or("")))).map_or(0, |k| k + 1);
        let doc = f.choice("Read from", &doc_names, typed);
        let carbon = (id == "epd").then(|| f.check("Carbon", "Use the GWP A1–A3 as the material's embodied-carbon factor", true));
        let ok = f.show("Leave a value blank if the document does not give it.", "Enter");
        let values: serde_json::Map<String, Value> = boxes.iter().map(|(k, b)| (k.clone(), json!(b.get_value()))).collect();
        let document = doc.get_selection().filter(|k| *k > 0).and_then(|k| docs.get(k as usize - 1)).map(|d| d["uid"].clone());
        let carbon = carbon.is_some_and(|c| c.get_value());
        f.dialog.destroy();
        if !ok {
            return None;
        }
        let mut args = json!({ "material": material, "set": id, "values": values, "carbon": carbon });
        if let Some(d) = document {
            args["document"] = d;
        }
        Some(match self.run("docs.enter_values", args) {
            Ok(o) => {
                let mut n = format!("{} values written onto {material}.", o["written"].as_array().map_or(0, Vec::len));
                if let Some(per) = o["factor_per_unit"].as_f64() {
                    n.push_str(&format!(" Its embodied-carbon factor is now {per:.4} kg CO₂e per {}.", o["factor_unit"].as_str().unwrap_or("")));
                }
                n
            }
            Err(e) => format!("Not written: {e}"),
        })
    }

    /// File › Publish Local Documents to FileEngine…: the project's local document folders put on
    /// FileEngine, and the model optionally pointed at them. Offered when there are local-only
    /// collections and a FileEngine connection.
    pub(crate) fn publish_local_documents(&mut self) -> Option<String> {
        let frame = self.frame;
        let collections = self.run("docs.local_collections", json!({})).ok()?.as_array().cloned().unwrap_or_default();
        if collections.is_empty() {
            error(&frame, "No material's documents are in a folder on this computer only.");
            return None;
        }
        let connected = self.run("fileengine.list_connections", json!({})).ok().and_then(|v| v.as_array().map(|a| !a.is_empty())).unwrap_or(false);
        if !connected {
            error(&frame, "Add a FileEngine connection first (File › Open From FileEngine… › Add Connection…).");
            return None;
        }
        let Pick { connection_id, folder_uid, folder_name } = self.fileengine_pick_folder("Publish the Document Folders Into")?;
        let list = collections.iter().map(|c| format!("{} ({} documents; {})", c["folder_name"].as_str().unwrap_or(""), c["documents"], c["materials"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default())).collect::<Vec<_>>().join("\n");
        let relink = MessageDialog::builder(&frame, &format!("Each local folder becomes a folder of its name in {folder_name}:\n\n{list}\n\nAlso point the model at the FileEngine folders? (No keeps it referring to the files on this computer.)"), "Publish Local Documents")
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::Cancel | MessageDialogStyle::IconQuestion)
            .build()
            .show_modal();
        if relink == ID_CANCEL {
            return None;
        }
        frame.set_status_text("Publishing the documents to FileEngine…", 0);
        let r = self.run("docs.publish_local", json!({ "connection_id": connection_id, "parent_uid": folder_uid, "relink": relink == ID_YES }));
        frame.set_status_text("", 0);
        let msg = match r {
            Ok(v) => {
                let n: u64 = v.as_array().map(|a| a.iter().map(|c| c["uploaded"].as_u64().unwrap_or(0) + c["updated"].as_u64().unwrap_or(0)).sum()).unwrap_or(0);
                format!("{n} documents published to {folder_name}{}.", if relink == ID_YES { "; the model now refers to them on FileEngine" } else { "" })
            }
            Err(e) => {
                error(&frame, &format!("The documents were not published:\n{e}"));
                return None;
            }
        };
        self.update_ribbon_context();
        frame.set_status_text(&msg, 0);
        Some(msg)
    }
}
