// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! File › Open From FileEngine…, Save to FileEngine… and FileEngine Version History…
//! (COL-FE-001…004): the connections, with Add Connection…, Sign In…, Sign Out and Remove; the
//! chosen connection's folders and models, entered by double-click, with Up; Open, or (saving)
//! a name, New Folder… and Save Here. Sign In asks for the service credential FileEngine
//! generated (masked, never shown again) and whether to remember it in the system keychain. When
//! Ctrl+S finds that someone else saved first, [`Gui::fileengine_conflict`] offers a copy beside
//! it, overwriting, or opening theirs read-only.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const ADD: i32 = 7301;
const SIGN_IN: i32 = 7302;
const SIGN_OUT: i32 = 7303;
const REMOVE: i32 = 7304;
const UP: i32 = 7305;
const ENTER: i32 = 7306;
const OPEN: i32 = 7307;
const SAVE: i32 = 7308;
const NEW_FOLDER: i32 = 7309;
const RELOAD: i32 = 7310;
const RESTORE: i32 = 7311;
const COPY: i32 = 7312;
const OVERWRITE: i32 = 7313;
const THEIRS: i32 = 7314;

/// What the browser shows.
struct Shown<'a> {
    connections: &'a [Value],
    conn: Option<usize>,
    /// The folder's `fileengine.browse` answer, when signed in.
    folder: Option<&'a Value>,
    /// The folders above, top first (for the path line).
    path: &'a [String],
    note: &'a str,
    /// Saving: the name field's value.
    save_name: Option<&'a str>,
}

struct Closed {
    code: i32,
    conn: Option<usize>,
    entry: Option<usize>,
    name: String,
}

/// `20261030_195013.009` as `2026-10-30 19:50:13`.
pub(crate) fn stamp(v: &str) -> String {
    let b = v.as_bytes();
    if b.len() >= 15 && b[8] == b'_' && v.is_char_boundary(15) {
        format!("{}-{}-{} {}:{}:{}", &v[0..4], &v[4..6], &v[6..8], &v[9..11], &v[11..13], &v[13..15])
    } else {
        v.to_owned()
    }
}

/// Unix seconds as `2026-10-30 22:53` (UTC).
fn changed(t: i64) -> String {
    // Days to a civil date: Howard Hinnant's days_from_civil, inverted.
    let z = t.div_euclid(86_400) + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let secs = t.rem_euclid(86_400);
    format!("{}-{m:02}-{d:02} {:02}:{:02}", yoe + era * 400 + i64::from(m <= 2), secs / 3600, secs % 3600 / 60)
}

fn size(bytes: u64) -> String {
    match bytes {
        0 => String::new(),
        b if b < 1024 * 1024 => format!("{:.0} KB", (b as f64 / 1024.0).max(1.0)),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

fn list(dialog: &Dialog, columns: &[(&str, i32)], rows: &[Vec<String>], selected: Option<usize>) -> ListCtrl {
    let l = ListCtrl::builder(dialog).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
    for (k, (h, w)) in columns.iter().enumerate() {
        l.insert_column(k as i64, h, ListColumnFormat::Left, *w);
    }
    for (k, row) in rows.iter().enumerate() {
        l.insert_item(k as i64, &row[0], None);
        for (c, text) in row.iter().enumerate().skip(1) {
            l.set_item_text_by_column(k as i64, c as i32, text);
        }
    }
    if let Some(k) = selected.filter(|k| *k < rows.len()) {
        l.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
    }
    l
}

fn buttons(dialog: &Dialog, items: &[(i32, &str, &str, bool)]) -> BoxSizer {
    let r = BoxSizer::builder(Orientation::Horizontal).build();
    for (code, label, tip, on) in items {
        let b = Button::builder(dialog).with_label(label).build();
        b.set_tooltip(tip);
        b.enable(*on);
        let (d, code) = (*dialog, *code);
        b.on_click(move |_| d.end_modal(code));
        r.add(&b, 0, SizerFlag::Right, 4);
    }
    r
}

fn browser(frame: &Frame, s: &Shown<'_>) -> Closed {
    let title = if s.save_name.is_some() { "Save to FileEngine" } else { "Open From FileEngine" };
    let dialog = Dialog::builder(frame, title).with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    let top = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&dialog).with_label("Connection").build();
    let names: Vec<String> = s
        .connections
        .iter()
        .map(|x| {
            let who = match (x["connected"].as_bool() == Some(true), x["user"].as_str()) {
                (true, Some(u)) => format!("signed in as {u}"),
                _ => "not signed in".to_owned(),
            };
            format!("{} ({who})", x["name"].as_str().unwrap_or(""))
        })
        .collect();
    let choice = Choice::builder(&dialog).with_choices(if names.is_empty() { vec!["No connections yet: Add Connection…".into()] } else { names.clone() }).build();
    choice.set_selection(s.conn.unwrap_or(0) as u32);
    top.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
    top.add(&choice, 1, SizerFlag::Expand, 0);
    let d = dialog;
    choice.on_selection_changed(move |_| d.end_modal(RELOAD));

    let entries: &[Value] = s.folder.and_then(|f| f["entries"].as_array()).map_or(&[], Vec::as_slice);
    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            let folder = e["folder"].as_bool() == Some(true);
            vec![
                if folder { format!("📁 {}", e["name"].as_str().unwrap_or("")) } else { e["name"].as_str().unwrap_or("").to_owned() },
                if folder { String::new() } else { size(e["size"].as_u64().unwrap_or(0)) },
                e["modified_at"].as_i64().filter(|&t| t > 0).map(changed).unwrap_or_default(),
                e["modified_by"].as_str().filter(|m| !m.is_empty()).or(e["owner"].as_str()).unwrap_or("").to_owned(),
                if e["working_copy"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
            ]
        })
        .collect();
    let where_ = StaticText::builder(&dialog).with_label(&format!("Folder: /{}", s.path.join("/"))).build();
    let view = list(&dialog, &[("Name", 320), ("Size", 80), ("Changed (UTC)", 130), ("By", 200), ("Here", 50)], &rows, None);
    view.set_min_size(Size::new(760, 320));
    let d = dialog;
    view.on_item_activated(move |_| d.end_modal(ENTER));
    let note = StaticText::builder(&dialog).with_label(s.note).build();
    note.wrap(760);

    let connected = s.conn.and_then(|k| s.connections.get(k)).is_some_and(|x| x["connected"].as_bool() == Some(true));
    let has = s.conn.is_some_and(|k| k < s.connections.len());
    let writable = s.folder.is_some_and(|f| f["writable"].as_bool() == Some(true));
    let conn_row = buttons(&dialog, &[
        (ADD, "Add Connection…", "Add a FileEngine server by its address", true),
        (SIGN_IN, "Sign In…", "Sign in with the credential FileEngine generated for this app", has),
        (SIGN_OUT, "Sign Out", "End the session; the stored credential can be forgotten too", connected),
        (REMOVE, "Remove", "Remove the connection and delete its stored credential", has),
        (UP, "Up", "The folder above", connected && !s.path.is_empty()),
    ]);
    let name_field = s.save_name.map(|n| {
        let t = TextCtrl::builder(&dialog).with_value(n).build();
        t.set_min_size(Size::new(320, -1));
        t
    });
    let last = BoxSizer::builder(Orientation::Horizontal).build();
    if let Some(t) = &name_field {
        let l = StaticText::builder(&dialog).with_label("Name").build();
        last.add(&l, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 6);
        last.add(t, 0, SizerFlag::Right, 8);
        last.add_sizer(&buttons(&dialog, &[(NEW_FOLDER, "New Folder…", "Make a folder here", connected && writable), (SAVE, "Save Here", "Save the project as this model in this folder", connected && writable)]), 0, SizerFlag::empty(), 0);
    } else {
        last.add_sizer(&buttons(&dialog, &[(OPEN, "Open", "Open the chosen model (a working copy is kept on this computer)", connected && !entries.is_empty())]), 0, SizerFlag::empty(), 0);
    }
    last.add_stretch_spacer(1);
    let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
    last.add(&close, 0, SizerFlag::Expand, 0);

    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add_sizer(&top, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add(&where_, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    sizer.add(&view, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 12);
    sizer.add(&note, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&conn_row, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    sizer.add_sizer(&last, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let code = dialog.show_modal();
    let out = Closed {
        code,
        conn: choice.get_selection().map(|i| i as usize).filter(|_| !names.is_empty()),
        entry: usize::try_from(view.get_first_selected_item()).ok(),
        name: name_field.map(|t| t.get_value()).unwrap_or_default(),
    };
    dialog.destroy();
    out
}

impl Gui {
    /// File › Open From FileEngine…
    pub(crate) fn fileengine_open(&mut self) {
        self.fileengine_browser(false);
    }

    /// File › Save to FileEngine…: a new version of the open FileEngine model (with a comment),
    /// or the project saved into a FileEngine folder.
    pub(crate) fn fileengine_save(&mut self) {
        let frame = self.frame;
        let linked = self.run("fileengine.status", Value::Null).ok().filter(|s| s["linked"] == true);
        match linked {
            Some(s) if s["link"]["read_only"] != true => {
                let f = super::reinforcement_ui::Form::new(&frame, &format!("Save {} to FileEngine", s["link"]["name"].as_str().unwrap_or("")));
                let comment = f.text("Comment", "", "What changed (kept with the version; optional)");
                let ok = f.show("Uploaded as a new version, if nobody else has saved since you opened it.", "Save");
                let c = comment.get_value();
                f.dialog.destroy();
                if ok {
                    self.fileengine_upload(json!({ "comment": c }));
                }
            }
            _ => self.fileengine_browser(true),
        }
    }

    fn fileengine_browser(&mut self, save: bool) {
        let frame = self.frame;
        let mut conn: Option<usize> = Some(0);
        // The folders entered, top first: (uid, name).
        let mut stack: Vec<(String, String)> = Vec::new();
        let mut note = String::new();
        let mut name = self.current_path().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "model.ifc".into());
        loop {
            let connections = match self.run("fileengine.list_connections", Value::Null) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The connections could not be read:\n{e}")),
            };
            let chosen = conn.and_then(|k| connections.get(k)).cloned();
            let id = chosen.as_ref().and_then(|x| x["id"].as_str()).map(str::to_owned);
            let connected = chosen.as_ref().is_some_and(|x| x["connected"].as_bool() == Some(true));
            let mut folder = None;
            if let Some(id) = id.as_deref().filter(|_| connected) {
                let mut args = json!({ "connection_id": id });
                if let Some((uid, _)) = stack.last() {
                    args["folder_uid"] = json!(uid);
                }
                match self.run("fileengine.browse", args) {
                    Ok(v) => folder = Some(v),
                    Err(e) => note = format!("The folder could not be listed: {e}"),
                }
            } else if id.is_some() && note.is_empty() {
                note = "Sign In… to see the server's folders.".into();
            }
            if note.is_empty() {
                note = if save {
                    "The project is saved as a new model in this folder; Ctrl+S then saves new versions of it.".into()
                } else {
                    "Double-click a folder to open it. Open downloads the chosen model and opens it; Ctrl+S saves it back as a new version.".into()
                };
            }
            let path: Vec<String> = stack.iter().map(|(_, n)| n.clone()).collect();
            let shown = Shown { connections: &connections, conn, folder: folder.as_ref(), path: &path, note: &note, save_name: save.then_some(name.as_str()) };
            let closed = browser(&frame, &shown);
            note.clear();
            if save {
                name = closed.name.trim().to_owned();
            }
            if closed.conn != conn {
                stack.clear();
            }
            conn = closed.conn.or(conn);
            let id = conn.and_then(|k| connections.get(k)).and_then(|x| x["id"].as_str()).map(str::to_owned);
            let entry = closed.entry.and_then(|k| folder.as_ref().and_then(|f| f["entries"].get(k))).cloned();
            match closed.code {
                ADD => {
                    if let Some(at) = self.fileengine_add_connection() {
                        conn = Some(at);
                        stack.clear();
                    }
                }
                SIGN_IN => {
                    if let Some(c) = id.as_deref().and_then(|id| connections.iter().find(|x| x["id"].as_str() == Some(id))).cloned() {
                        self.fileengine_sign_in(&c);
                    }
                }
                SIGN_OUT => {
                    if let Some(id) = id {
                        let forget = MessageDialog::builder(&frame, "Also forget the stored credential?", "Sign Out")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                            .build()
                            .show_modal()
                            == ID_YES;
                        if let Err(e) = self.run("fileengine.sign_out", json!({ "connection_id": id, "forget": forget })) {
                            note = format!("Signing out: {e}");
                        }
                    }
                }
                REMOVE => {
                    if let Some(id) = id {
                        let ok = MessageDialog::builder(&frame, "Remove this connection and delete its stored credential? Working copies stay on this computer.", "Remove Connection")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning)
                            .build()
                            .show_modal()
                            == ID_YES;
                        if ok {
                            match self.run("fileengine.remove", json!({ "connection_id": id })) {
                                Ok(_) => {
                                    conn = Some(0);
                                    stack.clear();
                                }
                                Err(e) => note = format!("The connection was not removed: {e}"),
                            }
                        }
                    }
                }
                UP => {
                    stack.pop();
                }
                ENTER | OPEN => {
                    let (Some(id), Some(e)) = (id, entry) else { continue };
                    if e["folder"].as_bool() == Some(true) {
                        stack.push((e["uid"].as_str().unwrap_or("").to_owned(), e["name"].as_str().unwrap_or("").to_owned()));
                        continue;
                    }
                    if save {
                        name = e["name"].as_str().unwrap_or("").to_owned();
                        continue;
                    }
                    if self.fileengine_open_model(json!({ "connection_id": id, "uid": e["uid"] }), e["name"].as_str().unwrap_or("")) {
                        return;
                    }
                }
                NEW_FOLDER => {
                    let (Some(id), Some(f)) = (id, folder.as_ref()) else { continue };
                    if let Some(n) = super::notes::ask_text(&frame, "New Folder", "") {
                        match self.run("fileengine.make_folder", json!({ "connection_id": id, "parent_uid": f["folder_uid"], "name": n })) {
                            Ok(uid) => stack.push((uid.as_str().unwrap_or("").to_owned(), n)),
                            Err(e) => note = format!("The folder was not made: {e}"),
                        }
                    }
                }
                SAVE => {
                    let (Some(id), Some(f)) = (id, folder.as_ref()) else { continue };
                    if !name.to_ascii_lowercase().ends_with(".ifc") {
                        name.push_str(".ifc");
                    }
                    let exists = f["entries"].as_array().into_iter().flatten().any(|e| e["name"].as_str() == Some(name.as_str()));
                    let replace = exists
                        && MessageDialog::builder(&frame, &format!("{name} is already in this folder. Save as a new version of it?"), "Save to FileEngine")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                            .build()
                            .show_modal()
                            == ID_YES;
                    if exists && !replace {
                        continue;
                    }
                    frame.set_status_text("Uploading to FileEngine…", 0);
                    let r = self.run("fileengine.save_as", json!({ "connection_id": id, "folder_uid": f["folder_uid"], "name": name, "replace": replace }));
                    frame.set_status_text("", 0);
                    match r {
                        Ok(up) => {
                            self.update_title(&frame);
                            frame.set_status_text(&format!("Saved {} to FileEngine (version {})", up["name"].as_str().unwrap_or(""), stamp(up["version"].as_str().unwrap_or(""))), 0);
                            return;
                        }
                        Err(e) => note = format!("Not saved to FileEngine: {e}"),
                    }
                }
                RELOAD => {}
                _ => return,
            }
        }
    }

    /// Closes the tabs of models the app no longer has open (a model reopened at a new version
    /// is a new document).
    fn drop_closed_tabs(&mut self) {
        let open: Vec<u64> = self.app.documents().into_iter().map(|d| d.id).collect();
        let gone: Vec<u32> = self.views.iter().filter(|v| !open.contains(&v.doc)).map(|v| v.id).collect();
        for id in gone {
            self.close_tab(id);
        }
        self.scenes.retain(|d, _| open.contains(d));
    }

    /// Opens a model (`fileengine.open` args) in a view; whether it opened.
    fn fileengine_open_model(&mut self, args: Value, name: &str) -> bool {
        let frame = self.frame;
        frame.set_status_text("Downloading from FileEngine…", 0);
        let r = self.run("fileengine.open", args);
        frame.set_status_text("", 0);
        self.drop_closed_tabs();
        match r {
            Ok(o) => {
                if let Some(doc) = o["document_id"].as_u64() {
                    match self.views.iter().find(|v| v.doc == doc).map(|v| v.id) {
                        Some(v) => self.activate_view(v),
                        None => self.show_document(doc, &frame),
                    }
                }
                self.update_title(&frame);
                let msg = match (o["existing"].as_bool() == Some(true), o["link"]["read_only"].as_bool() == Some(true)) {
                    (true, _) if o["newer_on_server"].as_bool() == Some(true) => format!("Opened {name} with your work not uploaded yet; someone has saved a newer version since."),
                    (true, _) => format!("Opened {name} with your work not uploaded yet; Ctrl+S uploads it."),
                    (false, true) => format!("Opened {name} read-only (version {}).", stamp(o["link"]["version"].as_str().unwrap_or(""))),
                    (false, false) => format!("Opened {name} (version {}).", stamp(o["link"]["version"].as_str().unwrap_or(""))),
                };
                frame.set_status_text(&msg, 0);
                true
            }
            Err(e) => {
                error(&frame, &format!("{name} was not opened:\n{e}"));
                false
            }
        }
    }

    /// Add Connection…: the new connection's place in the list, once added.
    fn fileengine_add_connection(&mut self) -> Option<usize> {
        let frame = self.frame;
        let (mut name, mut url) = (String::new(), "https://".to_owned());
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, "Add FileEngine Connection");
            let n = f.text("Name", &name, "How the connection is listed (default: the server's address)");
            let u = f.text("Address", &url, "Your organisation's FileEngine address, e.g. https://acme.proximafilevault.com");
            let ok = f.show("Sign in next, with the credential FileEngine generates for this app.", "Add");
            (name, url) = (n.get_value(), u.get_value());
            f.dialog.destroy();
            if !ok {
                return None;
            }
            let mut args = json!({ "url": url });
            if !name.trim().is_empty() {
                args["name"] = json!(name);
            }
            match self.run("fileengine.connect", args) {
                Ok(c) => {
                    let id = c["id"].as_str().unwrap_or("").to_owned();
                    let list = self.run("fileengine.list_connections", Value::Null).ok()?;
                    let at = list.as_array()?.iter().position(|x| x["id"].as_str() == Some(id.as_str()));
                    self.fileengine_sign_in(&c);
                    return at;
                }
                Err(e) => error(&frame, &format!("The connection was not added:\n{e}")),
            }
        }
    }

    /// Sign In…: the service credential (key id and secret).
    fn fileengine_sign_in(&mut self, conn: &Value) {
        let frame = self.frame;
        let id = conn["id"].as_str().unwrap_or("").to_owned();
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, &format!("Sign In to {}", conn["name"].as_str().unwrap_or("FileEngine")));
            let key = f.text("Key", "", "The key id FileEngine generated for this app (fesk_…), or the whole key:secret");
            let secret = f.password("Secret", "Its secret (fesks_…); never shown again");
            let remember = f.check("", "Remember in the system keychain", true);
            let stored = conn["secret"] != "none";
            let note = if stored { "Leave both empty to use the stored credential." } else { "Generate a credential with the api scope in FileEngine's account settings." };
            let ok = f.show(note, "Sign In");
            let (k, s, keep) = (key.get_value(), secret.get_value(), remember.get_value());
            f.dialog.destroy();
            if !ok {
                return;
            }
            let mut args = json!({ "connection_id": id, "remember": keep });
            if !k.trim().is_empty() {
                args["key"] = json!(k.trim());
            }
            if !s.trim().is_empty() {
                args["secret"] = json!(s.trim());
            }
            frame.set_status_text("Signing in…", 0);
            let r = self.run("fileengine.sign_in", args);
            frame.set_status_text("", 0);
            match r {
                Ok(v) => {
                    frame.set_status_text(&format!("Signed in to {} as {} ({})", conn["name"].as_str().unwrap_or(""), v["user"].as_str().unwrap_or(""), v["tenant"].as_str().unwrap_or("")), 0);
                    return;
                }
                Err(e) => error(&frame, &format!("Not signed in:\n{e}")),
            }
        }
    }

    /// Uploads the open FileEngine model (`fileengine.save` args), offering the choices when
    /// someone else saved first.
    fn fileengine_upload(&mut self, args: Value) {
        let frame = self.frame;
        frame.set_status_text("Uploading to FileEngine…", 0);
        let r = self.app.execute("fileengine.save", args.clone(), &super::user());
        frame.set_status_text("", 0);
        match r {
            Ok(o) => {
                self.update_title(&frame);
                frame.set_status_text(&format!("Saved {} to FileEngine (version {})", o.result["name"].as_str().unwrap_or(""), stamp(o.result["version"].as_str().unwrap_or(""))), 0);
            }
            Err(e) if e.code == "fileengine.conflict" => self.fileengine_conflict(&e.details, args["comment"].as_str()),
            Err(e) => error(&frame, &format!("Not saved to FileEngine:\n{e}")),
        }
    }

    /// Ctrl+S found that someone else saved first (`details` from the conflict): a copy beside
    /// it, overwrite, or theirs opened read-only.
    pub(crate) fn fileengine_conflict(&mut self, details: &Value, comment: Option<&str>) {
        let frame = self.frame;
        let who = details["saved_by"].as_str().filter(|s| !s.is_empty()).unwrap_or("someone else");
        let when = details["their_version"].as_str().map(stamp).unwrap_or_default();
        let dialog = Dialog::builder(&frame, "Someone Else Saved First").build();
        let text = StaticText::builder(&dialog)
            .with_label(&format!("{who} saved a newer version of this model ({when} UTC) after you opened it. Your work is saved on this computer and nothing was uploaded.\n\nReviera never merges silently. What would you like to do?"))
            .build();
        text.wrap(520);
        let row = buttons(&dialog, &[
            (COPY, "Save as a Copy Beside It", "Upload your work as a new model in the same folder; theirs stays as it is", true),
            (OVERWRITE, "Overwrite", "Upload your work as the newest version; theirs stays in the version history", true),
            (THEIRS, "Open Theirs Read-Only", "Open their version beside yours to compare", true),
        ]);
        row.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Not Now").build();
        row.add(&cancel, 0, SizerFlag::empty(), 0);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&text, 0, SizerFlag::Expand | SizerFlag::All, 14);
        sizer.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 14);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_escape_id(ID_CANCEL);
        let code = dialog.show_modal();
        dialog.destroy();
        let mut args = json!({});
        if let Some(c) = comment {
            args["comment"] = json!(c);
        }
        match code {
            COPY | OVERWRITE => {
                args["on_conflict"] = json!(if code == COPY { "copy" } else { "overwrite" });
                self.fileengine_upload(args);
            }
            THEIRS => {
                let Some(link) = self.run("fileengine.status", Value::Null).ok().map(|s| s["link"].clone()) else { return };
                let args = json!({ "connection_id": link["connection_id"], "uid": link["uid"], "version": details["their_version"] });
                self.fileengine_open_model(args, &format!("{}'s version", who));
            }
            _ => frame.set_status_text("Saved on this computer; not uploaded to FileEngine yet", 0),
        }
    }

    /// File › FileEngine Version History…: the open model's versions, to open one read-only or
    /// restore it.
    pub(crate) fn fileengine_versions(&mut self) {
        let frame = self.frame;
        let Some(link) = self.run("fileengine.status", Value::Null).ok().filter(|s| s["linked"] == true).map(|s| s["link"].clone()) else {
            return error(&frame, "The open project is not a FileEngine model. Open one with File › Open From FileEngine…, or save it there with Save to FileEngine….");
        };
        let mut note = String::new();
        loop {
            let versions = match self.run("fileengine.versions", json!({ "connection_id": link["connection_id"], "uid": link["uid"] })) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The versions could not be listed:\n{e}")),
            };
            let current = self.run("fileengine.status", Value::Null).ok().map(|s| s["link"]["version"].clone());
            let dialog = Dialog::builder(&frame, &format!("Version History: {}", link["name"].as_str().unwrap_or(""))).with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
            let rows: Vec<Vec<String>> = versions
                .iter()
                .map(|v| {
                    let mark = match (v["newest"] == true, current.as_ref() == Some(&v["version"])) {
                        (true, true) => "newest, open",
                        (true, false) => "newest",
                        (false, true) => "open",
                        _ => "",
                    };
                    vec![stamp(v["version"].as_str().unwrap_or("")), v["saved_by"].as_str().unwrap_or("").to_owned(), v["comment"].as_str().unwrap_or("").to_owned(), mark.to_owned()]
                })
                .collect();
            let view = list(&dialog, &[("Saved (UTC)", 150), ("By", 220), ("Comment", 280), ("", 100)], &rows, (!rows.is_empty()).then_some(0));
            view.set_min_size(Size::new(760, 300));
            let d = dialog;
            view.on_item_activated(move |_| d.end_modal(OPEN));
            let text = StaticText::builder(&dialog).with_label(if note.is_empty() { "Every save is kept. Restoring a version saves its content as a new version, so nothing is lost." } else { &note }).build();
            text.wrap(760);
            let row = buttons(&dialog, &[
                (OPEN, "Open Read-Only", "Open the chosen version beside the model, to look at or compare", !rows.is_empty()),
                (RESTORE, "Restore…", "Make the chosen version the newest again (as a new version)", rows.len() > 1 && link["read_only"] != true),
            ]);
            row.add_stretch_spacer(1);
            let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
            row.add(&close, 0, SizerFlag::empty(), 0);
            let sizer = BoxSizer::builder(Orientation::Vertical).build();
            sizer.add(&view, 1, SizerFlag::Expand | SizerFlag::All, 12);
            sizer.add(&text, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
            sizer.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
            dialog.set_sizer_and_fit(sizer, true);
            dialog.set_escape_id(ID_CANCEL);
            let code = dialog.show_modal();
            let chosen = usize::try_from(view.get_first_selected_item()).ok().and_then(|k| versions.get(k)).cloned();
            dialog.destroy();
            note.clear();
            let Some(v) = chosen else { return };
            let label = stamp(v["version"].as_str().unwrap_or(""));
            match code {
                OPEN => {
                    let args = json!({ "connection_id": link["connection_id"], "uid": link["uid"], "version": v["version"] });
                    if self.fileengine_open_model(args, &format!("the version of {label}")) {
                        return;
                    }
                }
                RESTORE => {
                    let ok = MessageDialog::builder(&frame, &format!("Make the version of {label} the newest again? It is saved as a new version; the later ones stay in the history, and the model reopens at it."), "Restore Version")
                        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                        .build()
                        .show_modal()
                        == ID_YES;
                    if !ok {
                        continue;
                    }
                    match self.run("fileengine.restore_version", json!({ "version": v["version"] })) {
                        Ok(r) => {
                            self.drop_closed_tabs();
                            if let Some(doc) = r["document_id"].as_u64() {
                                self.show_document(doc, &frame);
                            }
                            self.update_title(&frame);
                            note = format!("Restored the version of {label} as {}.", stamp(r["version"].as_str().unwrap_or("")));
                        }
                        Err(e) => note = format!("Not restored: {e}"),
                    }
                }
                _ => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn listing_times_read_as_utc_dates() {
        assert_eq!(super::changed(0), "1970-01-01 00:00");
        assert_eq!(super::changed(1_793_400_785), "2026-10-30 22:53");
        assert_eq!(super::changed(951_782_400), "2000-02-29 00:00");
        assert_eq!(super::stamp("20261030_225305.571"), "2026-10-30 22:53:05");
    }
}
