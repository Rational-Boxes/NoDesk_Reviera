// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! File › BIMserver… (COL-BIM-001…004, COL-MODE-002): the server profiles, with Add Server…,
//! Log In…, Log Out and Remove; the chosen server's projects and the chosen project's revisions;
//! Open Latest, Open Revision, Publish Current Project… and Check In…. Log In asks for the user
//! name and password, or a user token (masked, never shown again), and whether to remember it
//! in the system keychain.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const ADD: i32 = 7201;
const LOGIN: i32 = 7202;
const LOGOUT: i32 = 7203;
const REMOVE: i32 = 7204;
const OPEN_LATEST: i32 = 7205;
const OPEN_REVISION: i32 = 7206;
const PUBLISH: i32 = 7207;
const CHECK_IN: i32 = 7208;
const RELOAD: i32 = 7209;
const GET_LATEST: i32 = 7210;
const SYNC: i32 = 7211;
const COMPARE: i32 = 7212;
const RESTORE: i32 = 7213;

/// What the dialog shows: the servers, the chosen one's projects and the chosen project's
/// revisions (newest first), and a note under them.
struct Shown<'a> {
    servers: &'a [Value],
    server: Option<usize>,
    projects: &'a [Value],
    project: Option<usize>,
    revisions: &'a [Value],
    note: &'a str,
    /// The active document is a working copy.
    working_copy: bool,
}

/// What closed the dialog, and what was chosen.
struct Closed {
    code: i32,
    server: Option<usize>,
    project: Option<usize>,
    revision: Option<usize>,
}

fn date(ms: i64) -> String {
    // Days since the epoch to a calendar date (proleptic Gregorian), in UTC.
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
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

fn browser(frame: &Frame, s: &Shown<'_>) -> Closed {
    let dialog = Dialog::builder(frame, "BIMserver").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    let top = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&dialog).with_label("Server").build();
    let names: Vec<String> = s
        .servers
        .iter()
        .map(|x| {
            let state = if x["connected"].as_bool() == Some(true) { "connected".to_owned() } else { "not connected".to_owned() };
            format!("{} ({}, {state})", x["name"].as_str().unwrap_or(""), x["url"].as_str().unwrap_or(""))
        })
        .collect();
    let choice = Choice::builder(&dialog).with_choices(if names.is_empty() { vec!["No servers yet: Add Server…".into()] } else { names.clone() }).build();
    choice.set_selection(s.server.unwrap_or(0) as u32);
    top.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
    top.add(&choice, 1, SizerFlag::Expand, 0);
    let d = dialog;
    choice.on_selection_changed(move |_| d.end_modal(RELOAD));

    let project_rows: Vec<Vec<String>> = s
        .projects
        .iter()
        .map(|p| {
            vec![
                p["name"].as_str().unwrap_or("").to_owned(),
                p["schema"].as_str().unwrap_or("").to_uppercase(),
                p["revisions"].as_u64().unwrap_or(0).to_string(),
                if p["working_copy"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
            ]
        })
        .collect();
    let projects = list(&dialog, &[("Project", 200), ("Schema", 70), ("Revisions", 70), ("Working copy", 90)], &project_rows, s.project);
    projects.set_min_size(Size::new(440, 220));
    let d = dialog;
    let at = s.project;
    projects.on_item_selected(move |e| {
        if usize::try_from(e.get_item_index()).ok() != at {
            d.end_modal(RELOAD);
        }
    });
    let revision_rows: Vec<Vec<String>> = s
        .revisions
        .iter()
        .map(|r| {
            let comment = r["comment"].as_str().unwrap_or("").lines().next().unwrap_or("").to_owned();
            vec![r["number"].as_i64().unwrap_or(0).to_string(), date(r["date"].as_i64().unwrap_or(0)), r["user"].as_str().unwrap_or("").to_owned(), comment]
        })
        .collect();
    let revisions = list(&dialog, &[("#", 40), ("Date (UTC)", 120), ("User", 140), ("Comment", 260)], &revision_rows, (!revision_rows.is_empty()).then_some(0));
    revisions.set_min_size(Size::new(560, 220));
    let d = dialog;
    revisions.on_item_activated(move |_| d.end_modal(OPEN_REVISION));

    let lists = BoxSizer::builder(Orientation::Horizontal).build();
    lists.add(&projects, 0, SizerFlag::Expand | SizerFlag::Right, 8);
    lists.add(&revisions, 1, SizerFlag::Expand, 0);
    let note = StaticText::builder(&dialog).with_label(s.note).build();
    note.wrap(1000);

    let connected = s.server.and_then(|k| s.servers.get(k)).is_some_and(|x| x["connected"].as_bool() == Some(true));
    let has_server = s.server.is_some_and(|k| k < s.servers.len());
    let has_project = s.project.is_some_and(|k| k < s.projects.len());
    let has_revision = connected && s.working_copy && !s.revisions.is_empty();
    let row = |items: &[(i32, &str, &str, bool)]| {
        let r = BoxSizer::builder(Orientation::Horizontal).build();
        for (code, label, tip, on) in items {
            let b = Button::builder(&dialog).with_label(label).build();
            b.set_tooltip(tip);
            b.enable(*on);
            let (d, code) = (dialog, *code);
            b.on_click(move |_| d.end_modal(code));
            r.add(&b, 0, SizerFlag::Right, 4);
        }
        r
    };
    let servers = row(&[
        (ADD, "Add Server…", "Add a BIMserver by its address", true),
        (LOGIN, "Log In…", "Log in with a user name and password, or a user token", has_server),
        (LOGOUT, "Log Out", "End the session; the stored password or token can be forgotten too", connected),
        (REMOVE, "Remove Server", "Remove the server and delete its stored password or token", has_server),
        (OPEN_LATEST, "Open Latest", "Open the project's latest revision as a working copy (or its working copy here)", connected && has_project),
        (OPEN_REVISION, "Open Revision", "Open the chosen revision as a new working copy", connected && has_project && !s.revisions.is_empty()),
        (PUBLISH, "Publish Current Project…", "Check the open Local project in to the chosen project, making it that project's working copy", connected && has_project && !s.working_copy),
    ]);
    let copy = row(&[
        (GET_LATEST, "Get Latest", "Merge the newest revision into the open working copy; your changes stay local", connected && s.working_copy),
        (SYNC, "Synchronise…", "Get the latest revision, merge it, and check the working copy in", connected && s.working_copy),
        (CHECK_IN, "Check In…", "Upload the open working copy as a new revision of its project", connected && s.working_copy),
        (COMPARE, "Compare with Working Copy", "What differs between the chosen revision and the open working copy", has_revision),
        (RESTORE, "Restore…", "Check the chosen revision in again as the latest (the history is kept)", has_revision),
    ]);
    let buttons = BoxSizer::builder(Orientation::Vertical).build();
    buttons.add_sizer(&servers, 0, SizerFlag::Bottom, 6);
    let last = BoxSizer::builder(Orientation::Horizontal).build();
    last.add_sizer(&copy, 0, SizerFlag::empty(), 0);
    last.add_stretch_spacer(1);
    let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
    last.add(&close, 0, SizerFlag::Expand, 0);
    buttons.add_sizer(&last, 0, SizerFlag::Expand, 0);

    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add_sizer(&top, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&lists, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 12);
    sizer.add(&note, 0, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let code = dialog.show_modal();
    let pick = |l: &ListCtrl| usize::try_from(l.get_first_selected_item()).ok();
    let out = Closed { code, server: choice.get_selection().map(|i| i as usize).filter(|_| !names.is_empty()), project: pick(&projects), revision: pick(&revisions) };
    dialog.destroy();
    out
}

impl Gui {
    /// File › BIMserver…
    pub(crate) fn bimserver_dialog(&mut self) {
        let frame = self.frame;
        let (mut server, mut project): (Option<usize>, Option<usize>) = (Some(0), Some(0));
        let mut note = String::new();
        loop {
            let servers = match self.run("bimserver.list_servers", Value::Null) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The servers could not be read:\n{e}")),
            };
            let server_id = server.and_then(|k| servers.get(k)).and_then(|x| x["id"].as_str()).map(str::to_owned);
            let connected = server.and_then(|k| servers.get(k)).is_some_and(|x| x["connected"].as_bool() == Some(true));
            let mut projects = Vec::new();
            if let Some(id) = server_id.as_deref().filter(|_| connected) {
                match self.run("bimserver.list_projects", json!({ "server_id": id })) {
                    Ok(v) => projects = v.as_array().cloned().unwrap_or_default(),
                    Err(e) => note = format!("The projects could not be listed: {e}"),
                }
            } else if server_id.is_some() && note.is_empty() {
                note = "Log In… to see the server's projects.".into();
            }
            let poid = project.and_then(|k| projects.get(k)).and_then(|p| p["poid"].as_i64());
            let mut revisions = Vec::new();
            if let (Some(id), Some(poid)) = (server_id.as_deref(), poid) {
                match self.run("bimserver.list_revisions", json!({ "server_id": id, "poid": poid })) {
                    Ok(v) => revisions = v.as_array().cloned().unwrap_or_default(),
                    Err(e) => note = format!("The revisions could not be listed: {e}"),
                }
            }
            revisions.reverse();
            let mode = self.run("model.mode", Value::Null).ok();
            let working_copy = mode.as_ref().is_some_and(|m| m["mode"] == "bimserver");
            if note.is_empty() {
                note = match &mode {
                    Some(m) if working_copy => format!(
                        "The open project is the working copy of {} at revision {}.",
                        m["working_copy"]["project"].as_str().unwrap_or(""),
                        m["working_copy"]["base_revision"]
                    ),
                    Some(_) => "The open project is a Local file: Publish Current Project… checks it in to the chosen project.".into(),
                    None => "Open Latest downloads the chosen project as a working copy on this computer.".into(),
                };
            }
            let shown = Shown { servers: &servers, server, projects: &projects, project, revisions: &revisions, note: &note, working_copy };
            let closed = browser(&frame, &shown);
            note.clear();
            if closed.server != server {
                project = Some(0);
            } else {
                project = closed.project.or(project);
            }
            server = closed.server.or(server);
            let id = server.and_then(|k| servers.get(k)).and_then(|x| x["id"].as_str()).map(str::to_owned);
            let chosen = project.and_then(|k| projects.get(k)).cloned();
            match closed.code {
                ADD => {
                    if let Some(at) = self.add_server_form() {
                        server = Some(at);
                    }
                }
                LOGIN => {
                    if let Some(id) = id
                        && let Some(s) = servers.iter().find(|x| x["id"].as_str() == Some(id.as_str()))
                    {
                        self.login_form(s);
                    }
                }
                LOGOUT => {
                    if let Some(id) = id {
                        let forget = MessageDialog::builder(&frame, "Also forget the stored password or token?", "Log Out")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                            .build()
                            .show_modal()
                            == ID_YES;
                        if let Err(e) = self.run("bimserver.logout", json!({ "server_id": id, "forget": forget })) {
                            note = format!("Logging out: {e}");
                        }
                    }
                }
                REMOVE => {
                    if let Some(id) = id {
                        let ok = MessageDialog::builder(&frame, "Remove this server and delete its stored password or token? Working copies stay on this computer.", "Remove Server")
                            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning)
                            .build()
                            .show_modal()
                            == ID_YES;
                        if ok {
                            match self.run("bimserver.remove_server", json!({ "server_id": id })) {
                                Ok(_) => server = Some(0),
                                Err(e) => note = format!("The server was not removed: {e}"),
                            }
                        }
                    }
                }
                OPEN_LATEST | OPEN_REVISION => {
                    let (Some(id), Some(p)) = (id, chosen) else { continue };
                    let mut args = json!({ "server_id": id, "poid": p["poid"] });
                    if closed.code == OPEN_REVISION
                        && let Some(r) = closed.revision.and_then(|k| revisions.get(k))
                        && r["roid"] != p["last_roid"]
                    {
                        args["roid"] = r["roid"].clone();
                    }
                    frame.set_status_text("Downloading from BIMserver…", 0);
                    match self.run("bimserver.open", args) {
                        Ok(o) => {
                            frame.set_status_text("", 0);
                            if let Some(doc) = o["document_id"].as_u64() {
                                if self.views.iter().any(|v| v.doc == doc) {
                                    if let Some(v) = self.views.iter().find(|v| v.doc == doc).map(|v| v.id) {
                                        self.activate_view(v);
                                    }
                                } else {
                                    self.show_document(doc, &frame);
                                }
                            }
                            let msg = if o["existing"].as_bool() == Some(true) {
                                if o["newer_on_server"].as_bool() == Some(true) {
                                    format!("Opened the working copy (revision {}); the server has newer revisions.", o["base_revision"])
                                } else {
                                    format!("Opened the working copy (revision {}).", o["base_revision"])
                                }
                            } else {
                                format!("Opened revision {} of {}.", o["base_revision"], p["name"].as_str().unwrap_or(""))
                            };
                            self.update_title(&frame);
                            frame.set_status_text(&msg, 0);
                            return;
                        }
                        Err(e) => {
                            frame.set_status_text("", 0);
                            note = format!("The project was not opened: {e}");
                        }
                    }
                }
                PUBLISH => {
                    let (Some(id), Some(p)) = (id, chosen) else { continue };
                    if let Some(msg) = self.publish_form(&id, &p) {
                        note = msg;
                    }
                }
                CHECK_IN => {
                    if let Some(msg) = self.check_in_form() {
                        note = msg;
                    }
                }
                GET_LATEST => note = self.merge_latest(None),
                COMPARE => {
                    if let Some(r) = closed.revision.and_then(|k| revisions.get(k)) {
                        note = self.compare_revision(r);
                    }
                }
                RESTORE => {
                    if let Some(r) = closed.revision.and_then(|k| revisions.get(k)) {
                        let f = super::reinforcement_ui::Form::new(&frame, &format!("Restore Revision {}", r["number"]));
                        let reason = f.text("Reason", "", "Why this revision is restored (required)");
                        let ok = f.show("Checked in again as the latest revision; later ones stay in the history.", "Restore");
                        let why = reason.get_value();
                        f.dialog.destroy();
                        if ok {
                            frame.set_status_text("Restoring…", 0);
                            note = match self.run("bimserver.restore", json!({ "roid": r["roid"], "reason": why })) {
                                Ok(o) => {
                                    self.refresh_scene(&frame);
                                    self.update_title(&frame);
                                    format!("Revision {} restored as revision {}.", r["number"], o["revision"])
                                }
                                Err(e) => format!("Not restored: {e}"),
                            };
                            frame.set_status_text("", 0);
                        }
                    }
                }
                SYNC => {
                    let f = super::reinforcement_ui::Form::new(&frame, "Synchronise");
                    let comment = f.text("Comment", "", "What changed (required)");
                    let ok = f.show("Others' changes are merged into the working copy, then it is checked in.", "Synchronise");
                    let c = comment.get_value();
                    f.dialog.destroy();
                    if ok {
                        note = self.merge_latest(Some(c));
                    }
                }
                RELOAD => {}
                _ => return,
            }
        }
    }

    /// Add Server…: the new server's place in the list, once added.
    fn add_server_form(&mut self) -> Option<usize> {
        let frame = self.frame;
        let (mut name, mut url) = (String::new(), "https://".to_owned());
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, "Add BIMserver");
            let n = f.text("Name", &name, "How the server is listed (for example the office's name)");
            let u = f.text("Address", &url, "The server's address, https://… (where its /json is); http:// only for this computer");
            let ok = f.show("Reviera asks the server for its version before adding it.", "Add");
            (name, url) = (n.get_value(), u.get_value());
            f.dialog.destroy();
            if !ok {
                return None;
            }
            match self.run("bimserver.add_server", json!({ "name": name, "url": url })) {
                Ok(s) => {
                    let id = s["id"].as_str().unwrap_or("").to_owned();
                    let list = self.run("bimserver.list_servers", Value::Null).ok()?;
                    let at = list.as_array()?.iter().position(|x| x["id"].as_str() == Some(id.as_str()));
                    if let Some(s) = list.as_array().and_then(|a| at.and_then(|k| a.get(k))).cloned() {
                        self.login_form(&s);
                    }
                    return at;
                }
                Err(e) => error(&frame, &format!("The server was not added:\n{e}")),
            }
        }
    }

    /// Log In…: a user name and password, or a user token.
    fn login_form(&mut self, server: &Value) {
        let frame = self.frame;
        let id = server["id"].as_str().unwrap_or("").to_owned();
        let mut user = server["username"].as_str().unwrap_or("").to_owned();
        let mut use_token = server["login"] == "user_token";
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, &format!("Log In to {}", server["name"].as_str().unwrap_or("BIMserver")));
            let how = f.choice("Log in with", &["User name and password".to_owned(), "User token".to_owned()], usize::from(use_token));
            let u = f.text("User name", &user, "Your BIMserver user name (an e-mail address)");
            let secret = f.password("Password or token", "Your password, or the user token from your BIMserver user page; never shown again");
            let remember = f.check("", "Remember in the system keychain", true);
            let stored = server["secret"] != "none";
            let note = if stored { "Leave the password empty to use the stored one." } else { "" };
            let ok = f.show(note, "Log In");
            use_token = how.get_selection() == Some(1);
            user = u.get_value();
            let (s, keep) = (secret.get_value(), remember.get_value());
            f.dialog.destroy();
            if !ok {
                return;
            }
            let mut args = json!({ "server_id": id, "remember": keep });
            if !s.is_empty() {
                if use_token {
                    args["user_token"] = json!(s);
                } else {
                    args["username"] = json!(user);
                    args["password"] = json!(s);
                }
            }
            frame.set_status_text("Logging in…", 0);
            let r = self.run("bimserver.login", args);
            frame.set_status_text("", 0);
            match r {
                Ok(v) => {
                    frame.set_status_text(&format!("Logged in to {} as {}", server["name"].as_str().unwrap_or(""), v["name"].as_str().unwrap_or("")), 0);
                    return;
                }
                Err(e) => error(&frame, &format!("Not logged in:\n{e}")),
            }
        }
    }

    /// Publish Current Project…: a message for the dialog's note.
    fn publish_form(&mut self, server: &str, project: &Value) -> Option<String> {
        let frame = self.frame;
        let name = project["name"].as_str().unwrap_or("");
        let has = project["revisions"].as_u64().unwrap_or(0) > 0;
        let f = super::reinforcement_ui::Form::new(&frame, &format!("Publish to {name}"));
        let comment = f.text("Comment", "First issue", "What this revision is (required)");
        let replace = has.then(|| f.check("", &format!("{name} has revisions: check in a new one over them"), false));
        let ok = f.show("The open project becomes this BIMserver project's working copy on this computer.", "Publish");
        let (c, r) = (comment.get_value(), replace.as_ref().is_some_and(CheckBox::get_value));
        f.dialog.destroy();
        if !ok {
            return None;
        }
        frame.set_status_text("Checking in…", 0);
        let out = self.run("bimserver.publish_local", json!({ "server_id": server, "poid": project["poid"], "comment": c, "replace": r }));
        frame.set_status_text("", 0);
        Some(match out {
            Ok(o) => {
                self.refresh_scene(&frame);
                self.update_title(&frame);
                format!("Published as revision {} of {name}; the open project is now its working copy.", o["revision"])
            }
            Err(e) => format!("Not published: {e}"),
        })
    }

    /// Get Latest, or Synchronise with `comment`: runs it, and on conflicts lists them and asks
    /// which side wins (or stops). A message for the dialog's note.
    fn merge_latest(&mut self, comment: Option<String>) -> String {
        let frame = self.frame;
        let (name, mut args) = match &comment {
            Some(c) => ("bimserver.sync", json!({ "comment": c })),
            None => ("bimserver.get_latest", json!({})),
        };
        loop {
            frame.set_status_text("Merging the latest revision…", 0);
            let r = self.run(name, args.clone());
            frame.set_status_text("", 0);
            let r = match r {
                Ok(r) => r,
                Err(e) => return format!("Not done: {e}"),
            };
            let conflicts = r["conflicts"].as_array().cloned().unwrap_or_default();
            if r["applied"] == false && r["up_to_date"] == false && !conflicts.is_empty() {
                let lines: Vec<String> = conflicts
                    .iter()
                    .take(12)
                    .map(|c| {
                        let what = match c["kind"].as_str() {
                            Some("delete_modify") => "deleted here, changed on the server".to_owned(),
                            Some("modify_delete") => "changed here, deleted on the server".to_owned(),
                            Some("both_added") => "added on both sides".to_owned(),
                            _ => format!("both changed {}", c["facet"].as_str().unwrap_or("")),
                        };
                        format!("• {} {}: {what}", c["class"].as_str().unwrap_or(""), c["name"].as_str().unwrap_or(""))
                    })
                    .collect();
                let more = if conflicts.len() > 12 { format!("\n… and {} more", conflicts.len() - 12) } else { String::new() };
                let msg = format!(
                    "{} conflict(s) with the server's latest revision:\n\n{}{more}\n\nYes keeps your changes, No takes the server's, Cancel stops (nothing has changed).",
                    conflicts.len(),
                    lines.join("\n")
                );
                let choice = MessageDialog::builder(&frame, &msg, "Conflicts")
                    .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::Cancel | MessageDialogStyle::IconWarning)
                    .build()
                    .show_modal();
                let side = match choice {
                    ID_YES => "local",
                    ID_NO => "remote",
                    _ => return format!("Stopped at {} conflict(s); the working copy is unchanged.", conflicts.len()),
                };
                args["conflicts"] = json!(side);
                continue;
            }
            self.refresh_scene(&frame);
            self.update_title(&frame);
            return if r["up_to_date"] == true && r["checked_in"].is_null() {
                "Up to date: the working copy is based on the latest revision.".into()
            } else {
                let merged = format!("{} created, {} updated, {} deleted", r["created"], r["updated"], r["deleted"]);
                match r["checked_in"]["revision"].as_i64() {
                    Some(n) if r["up_to_date"] == true => format!("Checked in as revision {n}."),
                    Some(n) => format!("Merged the server's changes ({merged}) and checked in as revision {n}."),
                    None => format!("Merged revision {} into the working copy: {merged}.", r["base_revision"]),
                }
            };
        }
    }

    /// Compare with Working Copy: the changes from revision `r` to the open working copy, shown
    /// in a message; a summary for the dialog's note.
    fn compare_revision(&mut self, r: &Value) -> String {
        let frame = self.frame;
        frame.set_status_text("Comparing…", 0);
        let out = self.run("bimserver.compare", json!({ "roid_a": r["roid"] }));
        frame.set_status_text("", 0);
        let c = match out {
            Ok(c) => c,
            Err(e) => return format!("Not compared: {e}"),
        };
        let list = |k: &str| c[k].as_array().cloned().unwrap_or_default();
        let (added, deleted, modified) = (list("added"), list("deleted"), list("modified"));
        let summary = format!("Since revision {}: {} added, {} deleted, {} modified.", r["number"], added.len(), deleted.len(), modified.len());
        if added.is_empty() && deleted.is_empty() && modified.is_empty() {
            return format!("The working copy is the same as revision {}.", r["number"]);
        }
        let line = |e: &Value, what: &str| {
            let facets: Vec<&str> = e["facets"].as_array().into_iter().flatten().filter_map(Value::as_str).take(4).collect();
            let more = if facets.is_empty() { String::new() } else { format!(" ({})", facets.join(", ")) };
            format!("{what} {} {}{more}", e["class"].as_str().unwrap_or(""), e["name"].as_str().unwrap_or(""))
        };
        let mut lines: Vec<String> = added.iter().map(|e| line(e, "+")).chain(deleted.iter().map(|e| line(e, "−"))).chain(modified.iter().map(|e| line(e, "~"))).collect();
        let total = lines.len();
        lines.truncate(25);
        let more = if total > 25 { format!("\n… and {} more", total - 25) } else { String::new() };
        MessageDialog::builder(&frame, &format!("{summary}\n\n{}{more}", lines.join("\n")), "Compare").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
        summary
    }

    /// Check In…: a message for the dialog's note.
    fn check_in_form(&mut self) -> Option<String> {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Check In");
        let comment = f.text("Comment", "", "What changed (required)");
        let ok = f.show("The working copy is saved and uploaded as a new revision.", "Check In");
        let c = comment.get_value();
        f.dialog.destroy();
        if !ok {
            return None;
        }
        frame.set_status_text("Checking in…", 0);
        let out = self.run("bimserver.check_in", json!({ "comment": c }));
        frame.set_status_text("", 0);
        Some(match out {
            Ok(o) => {
                self.update_title(&frame);
                format!("Checked in as revision {}.", o["revision"])
            }
            Err(e) => format!("Not checked in: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_are_calendar_dates() {
        assert_eq!(super::date(0), "1970-01-01 00:00");
        assert_eq!(super::date(1_793_325_975_279), "2026-10-30 02:06");
        assert_eq!(super::date(951_782_400_000), "2000-02-29 00:00");
    }
}
