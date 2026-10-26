// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View › Coordination › Issues (COL-BCF-001, first build of COL-BCF-004): *New Issue* makes a BCF
//! topic from the 3D view and the selection; *Issues* lists the model's topics (Show its
//! viewpoint, Edit, Comment, Add View, Delete); *Import BCF* and *Export BCF* read and write
//! `.bcf` archives.

use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// Same as the list dialog's double-click code.
const EDIT: i32 = 7202;
const SHOW: i32 = 7601;
const COMMENT: i32 = 7602;
const ADD_VIEW: i32 = 7603;
const DELETE: i32 = 7604;

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

fn list_of(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

impl Gui {
    /// The issue form: its fields from `topic` (empty for a new one); `None` when cancelled.
    fn issue_form(&mut self, title: &str, topic: &Value) -> Option<Value> {
        let lists = self.run("bcf.lists", json!({})).unwrap_or_default();
        let f = super::reinforcement_ui::Form::new(&self.frame, title);
        let name = f.text("Title", &text(&topic["title"]), "");
        let description = f.lines("Description", &text(&topic["description"]), "");
        let kind = f.combo("Type", &list_of(&lists["topic_types"]), topic["topic_type"].as_str().unwrap_or("Issue"), "Or type a new one");
        let status = f.combo("Status", &list_of(&lists["statuses"]), topic["status"].as_str().unwrap_or("Open"), "Or type a new one");
        let priority = f.combo("Priority", &list_of(&lists["priorities"]), topic["priority"].as_str().unwrap_or(""), "");
        let labels = f.text("Labels", &list_of(&topic["labels"]).join(", "), "Separated by commas");
        let assigned = f.combo("Assigned to", &list_of(&lists["users"]), topic["assigned_to"].as_str().unwrap_or(""), "A name or email");
        let due = f.text("Due date", &text(&topic["due_date"]), "YYYY-MM-DD");
        let stage = f.combo("Stage", &list_of(&lists["stages"]), topic["stage"].as_str().unwrap_or(""), "");
        if !f.show("", "OK") {
            f.dialog.destroy();
            return None;
        }
        let labels: Vec<String> = labels.get_value().split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
        let fields = json!({
            "title": name.get_value().trim(),
            "description": description.get_value(),
            "topic_type": kind.get_value().trim(),
            "status": status.get_value().trim(),
            "priority": priority.get_value().trim(),
            "labels": labels,
            "assigned_to": assigned.get_value().trim(),
            "due_date": due.get_value().trim(),
            "stage": stage.get_value().trim(),
        });
        f.dialog.destroy();
        Some(fields)
    }

    /// View › Coordination › New Issue: a topic from the 3D view and the selection.
    pub(crate) fn bcf_new_issue(&mut self) {
        let frame = self.frame;
        let Some(mut fields) = self.issue_form("New Issue", &json!({})) else { return };
        let title = fields["title"].as_str().unwrap_or("").to_owned();
        if title.is_empty() {
            return frame.set_status_text("An issue needs a title", 0);
        }
        let description = fields["description"].take();
        if let Some(f) = fields.as_object_mut() {
            f.remove("title");
            f.remove("description");
        }
        match self.run("bcf.create_topic", json!({ "title": title, "description": description, "fields": fields })) {
            Ok(_) => frame.set_status_text(&format!("Issue \u{201c}{title}\u{201d} made, with a viewpoint of the 3D view when one is active"), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// View › Coordination › Issues: the model's topics.
    pub(crate) fn bcf_issues(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("bcf.list", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|t| {
                    vec![
                        text(&t["title"]),
                        text(&t["topic_type"]),
                        text(&t["status"]),
                        text(&t["priority"]),
                        text(&t["assigned_to"]),
                        text(&t["due_date"]),
                        text(&t["modified"]).replace('T', " ").trim_end_matches('Z').to_owned(),
                        format!("{} / {}", t["comments"], t["viewpoints"]),
                    ]
                })
                .collect();
            let intro = if list.is_empty() {
                "No issues yet: New Issue makes one from the 3D view, or Import BCF reads them from a file."
            } else {
                "BCF issues kept with the model. Show opens an issue's viewpoint in the 3D view."
            };
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Issues",
                intro,
                &[("Title", 200), ("Type", 70), ("Status", 80), ("Priority", 60), ("Assigned to", 120), ("Due", 80), ("Modified", 120), ("Comments / views", 100)],
                &rows,
                &[
                    (SHOW, "Show", "Open its first viewpoint in the 3D view: camera, cut, hidden or isolated elements and selection"),
                    (EDIT, "Edit…", "Its title, description, type, status, priority, labels, assignee, due date and stage"),
                    (COMMENT, "Comment…", "Add a comment"),
                    (ADD_VIEW, "Add View", "Add the active 3D view as another viewpoint"),
                    (DELETE, "Delete", "Delete the issue"),
                ],
                selected,
            );
            selected = at;
            let Some(t) = at.and_then(|k| list.get(k)).cloned() else {
                if matches!(code, SHOW | EDIT | COMMENT | ADD_VIEW | DELETE) {
                    continue;
                }
                break;
            };
            let guid = t["guid"].clone();
            let result = match code {
                SHOW => match self.run("bcf.apply_viewpoint", json!({ "topic_id": guid })) {
                    Ok(r) => {
                        if let (Some(v), Some(doc)) = (r["view_id"].as_u64(), self.doc) {
                            self.open_view(doc, Some(ElementId(v)));
                        }
                        self.selection_changed();
                        self.refresh_active();
                        let missing = r["not_found"].as_array().map_or(0, Vec::len);
                        if missing > 0 {
                            frame.set_status_text(&format!("{missing} of the issue's elements are not in this model"), 0);
                        }
                        return;
                    }
                    Err(e) => Err(e),
                },
                EDIT => {
                    let full = self.run("bcf.get", json!({ "topic_id": guid })).unwrap_or_default();
                    match self.issue_form("Edit Issue", &full) {
                        Some(fields) => self.run("bcf.update_topic", json!({ "topic_id": guid, "fields": fields })).map(|_| ()),
                        None => continue,
                    }
                }
                COMMENT => {
                    let f = super::reinforcement_ui::Form::new(&frame, "Comment");
                    let c = f.lines("Comment", "", "");
                    let ok = f.show("", "Add");
                    let value = c.get_value();
                    f.dialog.destroy();
                    if !ok || value.trim().is_empty() {
                        continue;
                    }
                    self.run("bcf.add_comment", json!({ "topic_id": guid, "text": value })).map(|_| ())
                }
                ADD_VIEW => self.run("bcf.add_viewpoint", json!({ "topic_id": guid })).map(|_| ()),
                DELETE => self.run("bcf.delete_topic", json!({ "topic_ids": [guid] })).map(|_| ()),
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
        }
    }

    /// File › Import BCF…: topics from a .bcf archive, merged by GUID.
    pub(crate) fn bcf_import(&mut self) {
        let frame = self.frame;
        let fd = FileDialog::builder(&frame).with_message("Import BCF").with_wildcard("BCF (*.bcf;*.bcfzip)|*.bcf;*.bcfzip").with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist).build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        match self.run("bcf.import", json!({ "path": path })) {
            Ok(r) => {
                let mut msg = format!(
                    "Read {} issues (BCF {}): {} new, {} updated, {} unchanged; {} viewpoints, {} snapshots.",
                    r["topics"],
                    r["version"].as_str().unwrap_or("?"),
                    r["added"],
                    r["updated"],
                    r["unchanged"],
                    r["viewpoints"],
                    r["snapshots"]
                );
                for p in r["problems"].as_array().into_iter().flatten() {
                    msg.push_str(&format!("\n• {}", p.as_str().unwrap_or("")));
                }
                MessageDialog::builder(&frame, &msg, "Import BCF").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// File › Export BCF…: every topic as a .bcf archive, BCF 3.0 or 2.1.
    pub(crate) fn bcf_export(&mut self) {
        let frame = self.frame;
        let versions = ["BCF 3.0".to_owned(), "BCF 2.1".to_owned()];
        let f = super::reinforcement_ui::Form::new(&frame, "Export BCF");
        let v = f.choice("Version", &versions, 0);
        let ok = f.show("2.1 for tools that do not read 3.0 yet.", "Next…");
        let version = if v.get_selection() == Some(1) { "2.1" } else { "3.0" };
        f.dialog.destroy();
        if !ok {
            return;
        }
        let fd = FileDialog::builder(&frame).with_message("Export BCF").with_wildcard("BCF (*.bcf)|*.bcf|BCF zip (*.bcfzip)|*.bcfzip").with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt).build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        match self.run("bcf.export", json!({ "path": path, "version": version })) {
            Ok(r) => frame.set_status_text(&format!("Wrote {} issues, {} comments, {} viewpoints and {} snapshots (BCF {version}) to {path}", r["topics"], r["comments"], r["viewpoints"], r["snapshots"]), 0),
            Err(e) => error(&frame, &e),
        }
    }
}
