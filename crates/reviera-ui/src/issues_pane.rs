// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Issues pane (COL-BCF-004, COL-FE-009, COL-BCF-002): the active model's issues, in a pane
//! docked at the bottom that can be floated or docked elsewhere like the others. They are
//! wherever the model keeps them (`issues.*`): a project on a BCF issue server it is bound to
//! (Issue Server…), FileEngine for a model opened from there, or its own BCF topics; the model's
//! own topics not on the server yet are listed too, marked, with Publish to Server. Every issue
//! is listed, open and resolved alike: resolved ones greyed and marked ✓, with a status filter
//! (all, open, resolved) and a search box. Choosing one shows its snapshot, description,
//! comments and fields; Show opens its viewpoint in the 3D view; Reply…, Resolve/Reopen,
//! Edit…, Add View, Delete and New Issue… write to where it is kept; Import BCF… and Export
//! BCF… exchange files. A server's issues are read when the model opens (the pane opens when
//! there are any) and every minute while the pane is shown; the status bar says what changed.

use base64::Engine;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error, with};

/// The snapshot preview's width.
const PICTURE_W: u32 = 300;
const PICTURE_H: u32 = 188;

/// How often a server's issues are read again while the pane is shown.
const POLL_MS: i32 = 60_000;

/// The status filter's choices.
const FILTERS: &[&str] = &["All issues", "Open", "Resolved"];

/// One issue as the pane shows it (an `issues.list` entry).
#[derive(Clone)]
struct Row {
    id: String,
    title: String,
    status: String,
    resolved: bool,
    issue: Value,
    /// Kept with the model only (not on the server).
    local: bool,
}

impl Row {
    fn of(i: &Value) -> Self {
        Row {
            id: i["id"].as_str().unwrap_or("").to_owned(),
            title: i["title"].as_str().unwrap_or("").to_owned(),
            status: i["status"].as_str().unwrap_or("").to_owned(),
            resolved: i["resolved"] == true,
            local: i["kept"] == "model",
            issue: i.clone(),
        }
    }

    fn comments(&self) -> usize {
        self.issue["comments"].as_array().map_or(0, Vec::len)
    }

    /// The words the search box looks in.
    fn haystack(&self) -> String {
        let i = &self.issue;
        let mut h = String::new();
        for k in ["title", "description", "topic_type", "priority", "assigned_to", "opened_by", "status", "stage"] {
            h.push_str(i[k].as_str().unwrap_or(""));
            h.push(' ');
        }
        for l in i["labels"].as_array().into_iter().flatten() {
            h.push_str(l.as_str().unwrap_or(""));
            h.push(' ');
        }
        for c in i["comments"].as_array().into_iter().flatten() {
            h.push_str(c["text"].as_str().unwrap_or(""));
            h.push(' ');
            h.push_str(c["author"].as_str().unwrap_or(""));
            h.push(' ');
        }
        h.to_lowercase()
    }
}

pub(crate) struct IssuesPane {
    list: ListCtrl,
    filter: Choice,
    search: TextCtrl,
    #[allow(dead_code)]
    timer: Timer<Frame>,
    /// Every issue read; `rows` are the ones the filters show.
    all: Vec<Row>,
    info: StaticText,
    picture: StaticBitmap,
    detail: TextCtrl,
    show: Button,
    resolve: Button,
    edit: Button,
    add_view: Button,
    delete: Button,
    publish: Button,
    rows: Vec<Row>,
    /// The document shown.
    doc: Option<u64>,
    /// `model`, `fileengine` or `bcf_api`.
    source: String,
    /// The last `issues.list` answer (lists, what the server allows, counts).
    last: Value,
    current: Option<usize>,
}

fn date(s: &str) -> String {
    s.get(..16).unwrap_or(s).replace('T', " ")
}

fn scaled(rgba: &[u8], w: u32, h: u32) -> Option<Bitmap> {
    // Fitted into the preview, nearest pixel, on a dark ground.
    let k = (PICTURE_W as f64 / w as f64).min(PICTURE_H as f64 / h as f64).min(1.0);
    let (sw, sh) = (((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1));
    let (ox, oy) = ((PICTURE_W - sw) / 2, (PICTURE_H - sh) / 2);
    let mut out = vec![0u8; (PICTURE_W * PICTURE_H * 4) as usize];
    for px in out.chunks_mut(4) {
        px.copy_from_slice(&[40, 40, 40, 255]);
    }
    for y in 0..sh {
        for x in 0..sw {
            let (sx, sy) = (((x as f64 / k) as u32).min(w - 1), ((y as f64 / k) as u32).min(h - 1));
            let s = ((sy * w + sx) * 4) as usize;
            let d = (((y + oy) * PICTURE_W + x + ox) * 4) as usize;
            let a = rgba[s + 3] as u32;
            for c in 0..3 {
                out[d + c] = ((rgba[s + c] as u32 * a + 40 * (255 - a)) / 255) as u8;
            }
        }
    }
    Bitmap::from_rgba(&out, PICTURE_W, PICTURE_H)
}

fn blank() -> Option<Bitmap> {
    let mut out = vec![0u8; (PICTURE_W * PICTURE_H * 4) as usize];
    for px in out.chunks_mut(4) {
        px.copy_from_slice(&[40, 40, 40, 255]);
    }
    Bitmap::from_rgba(&out, PICTURE_W, PICTURE_H)
}

fn button(panel: &Panel, label: &str, tip: &str) -> Button {
    let b = Button::builder(panel).with_label(label).build();
    b.set_tooltip(tip);
    b
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

impl Gui {
    /// Window › Issues: shows or hides the pane.
    pub(crate) fn toggle_issues_pane(&mut self) {
        if self.issues_pane.is_some() {
            let shown = self.aui.is_pane_shown("issues");
            self.aui.set_pane_shown("issues", !shown);
            if !shown {
                self.refresh_issues(true);
            }
            return;
        }
        self.show_issues_pane();
    }

    /// Shows the pane (building it the first time) with the active model's issues.
    pub(crate) fn show_issues_pane(&mut self) {
        if self.issues_pane.is_none() {
            self.build_issues_pane();
        } else if !self.aui.is_pane_shown("issues") {
            self.aui.set_pane_shown("issues", true);
        }
        self.refresh_issues(false);
    }

    /// After a model opens: a server's issues read now, and the pane shown when there are any
    /// (open or resolved).
    pub(crate) fn issues_after_open(&mut self) {
        let on_server = self.run("coordination.status", json!({})).ok().is_some_and(|s| s["source"] != "model");
        if !on_server {
            return self.refresh_issues(false);
        }
        match self.run("issues.list", json!({ "refresh": true })) {
            Ok(v) if v["total"].as_u64().unwrap_or(0) > 0 => self.show_issues_pane(),
            Ok(_) => self.refresh_issues(false),
            Err(e) => self.frame.set_status_text(&format!("The model's issues could not be read: {e}"), 0),
        }
    }

    fn build_issues_pane(&mut self) {
        let Some(gui) = self.this.upgrade() else { return };
        let (frame, aui) = (self.frame, self.aui);
        let panel = Panel::builder(&frame).build();
        let info = StaticText::builder(&panel).with_label("").build();
        let list = ListCtrl::builder(&panel).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
        for (k, (h, w)) in [("Issue", 300), ("Status", 100), ("Type", 70), ("Priority", 65), ("Assigned to", 140), ("Raised by", 150), ("Changed", 120), ("Replies", 55), ("Kept", 90)].into_iter().enumerate() {
            list.insert_column(k as i64, h, ListColumnFormat::Left, w);
        }
        let picture = StaticBitmap::builder(&panel).with_bitmap(blank()).build();
        picture.set_min_size(Size::new(PICTURE_W as i32, PICTURE_H as i32));
        let detail = TextCtrl::builder(&panel).with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly).build();
        detail.set_min_size(Size::new(340, PICTURE_H as i32));
        let show = button(&panel, "Show", "Open the issue's viewpoint in the 3D view: camera, cut, hidden elements, and the elements it is about selected");
        let reply = button(&panel, "Reply…", "Add a comment to the issue, where it is kept");
        let resolve = button(&panel, "Resolve", "Resolve (close) the issue, or open it again");
        let edit = button(&panel, "Edit…", "Its title, description, type, status, priority, labels, assignee, due date and stage");
        let add_view = button(&panel, "Add View", "Add the 3D view as it is now (with a snapshot) as another viewpoint of the issue");
        let delete = button(&panel, "Delete", "Delete the issue where it is kept");
        let new = button(&panel, "New Issue…", "Raise an issue from the 3D view: its camera, cut, hidden elements, selection and a snapshot, where the model's issues are kept");
        let publish = button(&panel, "Publish to Server", "Put the issues kept only with this model (from clash checks, imports …) on the server the model's issues are kept on");
        let refresh = button(&panel, "Refresh", "Read the issues again");
        let server = button(&panel, "Issue Server…", "Keep this model's issues on a BCF issue server (OpenConstructionERP or another BCF API server), or with the model");
        let import = button(&panel, "Import BCF…", "Read issues from a .bcf file into the model (then Publish to Server to share them)");
        let export = button(&panel, "Export BCF…", "Write the issues shown, wherever they are kept, to a .bcf file");
        let filter = Choice::builder(&panel).build();
        for f in FILTERS {
            filter.append(f);
        }
        filter.set_selection(0);
        filter.set_tooltip("Show all issues, only the open ones, or only the resolved (closed) ones");
        let search = TextCtrl::builder(&panel).build();
        search.set_min_size(Size::new(180, -1));
        search.set_tooltip("Find issues by words in their title, description, comments, type, priority, assignee or labels");
        let search_label = StaticText::builder(&panel).with_label("Find").build();

        let top = BoxSizer::builder(Orientation::Horizontal).build();
        for b in [&show, &reply, &resolve, &edit, &add_view, &delete, &new, &publish, &refresh] {
            top.add(b, 0, SizerFlag::Right, 4);
        }
        top.add_stretch_spacer(1);
        for b in [&import, &export, &server] {
            top.add(b, 0, SizerFlag::Left, 4);
        }
        let second = BoxSizer::builder(Orientation::Horizontal).build();
        second.add(&filter, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 8);
        second.add(&search_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 4);
        second.add(&search, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 12);
        second.add(&info, 1, SizerFlag::AlignCenterVertical, 0);
        let body = BoxSizer::builder(Orientation::Horizontal).build();
        body.add(&list, 3, SizerFlag::Expand | SizerFlag::Right, 6);
        body.add(&picture, 0, SizerFlag::Right, 6);
        body.add(&detail, 2, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add_sizer(&top, 0, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(&second, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 4);
        sizer.add_sizer(&body, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("issues")
                .with_caption("Issues")
                .bottom()
                .layer(1)
                .best_size(1200, 300)
                .min_size(520, 200)
                .close_button(true)
                .floatable(true)
                .dockable(true)
                .movable(true),
        );
        aui.update();
        let timer = Timer::new(&frame);
        {
            let g = gui.clone();
            timer.on_tick(move |_| with(&g, Gui::poll_issues));
        }
        timer.start(POLL_MS, false);
        {
            let g = gui.clone();
            filter.on_selection_changed(move |_| with(&g, Gui::show_issue_rows));
        }
        {
            let g = gui.clone();
            search.on_text_updated(move |_| with(&g, Gui::show_issue_rows));
        }
        self.issues_pane = Some(IssuesPane {
            list,
            filter,
            search,
            timer,
            all: Vec::new(),
            info,
            picture,
            detail,
            show,
            resolve,
            edit,
            add_view,
            delete,
            publish,
            rows: Vec::new(),
            doc: None,
            source: String::new(),
            last: Value::Null,
            current: None,
        });
        {
            let g = gui.clone();
            list.on_item_selected(move |e| {
                let i = e.get_item_index();
                with(&g, |g| g.issue_chosen(i.max(0) as usize));
            });
        }
        {
            let g = gui.clone();
            list.on_item_activated(move |_| with(&g, Gui::show_issue));
        }
        let actions: [(&Button, fn(&mut Gui)); 12] = [
            (&show, Gui::show_issue),
            (&reply, Gui::reply_issue),
            (&resolve, Gui::resolve_issue),
            (&edit, Gui::edit_issue),
            (&add_view, Gui::add_view_to_issue),
            (&delete, Gui::delete_issue),
            (&new, Gui::new_issue_here),
            (&publish, Gui::publish_issues),
            (&refresh, Gui::refresh_issues_now),
            (&server, Gui::issue_server),
            (&import, Gui::import_bcf_issues),
            (&export, Gui::export_bcf_issues),
        ];
        for (b, f) in actions {
            let g = gui.clone();
            b.on_click(move |_| with(&g, f));
        }
    }

    fn refresh_issues_now(&mut self) {
        self.refresh_issues(true);
    }

    /// Reads the active model's issues into the pane (`ask` asks the server again).
    pub(crate) fn refresh_issues(&mut self, ask: bool) {
        if self.issues_pane.is_none() {
            return;
        }
        let (rows, note, last) = if self.app.active().is_some() {
            match self.run("issues.list", json!({ "refresh": ask })) {
                Ok(v) => {
                    let rows: Vec<Row> = v["issues"].as_array().into_iter().flatten().map(Row::of).collect();
                    let (open, resolved, local) = (v["open"].as_u64().unwrap_or(0), v["resolved"].as_u64().unwrap_or(0), v["local"].as_u64().unwrap_or(0));
                    let place = match v["source"].as_str() {
                        Some("bcf_api") => format!("On {} · {}", v["server"].as_str().unwrap_or("the issue server"), v["project"].as_str().unwrap_or("")),
                        Some("fileengine") => format!("On FileEngine · {}", v["project"].as_str().unwrap_or("this model")),
                        _ => "Kept with the model".to_owned(),
                    };
                    let mut note = format!("{place}: {} open, {} resolved", open, resolved);
                    if local > 0 {
                        note.push_str(&format!("; {} only in this model (Publish to Server)", plural(local, "issue", "issues")));
                    }
                    if v["offline"] == true {
                        note.push_str(" — offline: as last read");
                    }
                    (rows, note, v)
                }
                Err(e) => (Vec::new(), format!("The issues could not be read: {e}"), Value::Null),
            }
        } else {
            (Vec::new(), "Open a model to see its issues.".into(), Value::Null)
        };
        let doc = self.doc;
        let Some(p) = self.issues_pane.as_mut() else { return };
        p.doc = doc;
        p.source = last["source"].as_str().unwrap_or("").to_owned();
        p.publish.enable(p.source != "model" && last["local"].as_u64().unwrap_or(0) > 0);
        p.info.set_label(&note);
        p.all = rows;
        p.last = last;
        self.show_issue_rows();
    }

    /// Fills the list with the issues the filters show, keeping the one chosen.
    fn show_issue_rows(&mut self) {
        let Some(p) = self.issues_pane.as_mut() else { return };
        let keep = p.current.and_then(|k| p.rows.get(k)).map(|r| r.id.clone());
        let filter = p.filter.get_selection().unwrap_or(0);
        let words: Vec<String> = p.search.get_value().to_lowercase().split_whitespace().map(str::to_owned).collect();
        p.rows = p
            .all
            .iter()
            .filter(|r| match filter {
                1 => !r.resolved,
                2 => r.resolved,
                _ => true,
            })
            .filter(|r| words.is_empty() || {
                let h = r.haystack();
                words.iter().all(|w| h.contains(w.as_str()))
            })
            .cloned()
            .collect();
        p.list.delete_all_items();
        let grey = Colour::new(128, 128, 128, 255);
        let local = Colour::new(40, 90, 190, 255);
        for (k, r) in p.rows.iter().enumerate() {
            let i = &r.issue;
            let k = k as i64;
            p.list.insert_item(k, &r.title, None);
            let status = if r.resolved { format!("✓ {}", r.status) } else { format!("● {}", r.status) };
            let cells = [
                status,
                i["topic_type"].as_str().unwrap_or("").to_owned(),
                i["priority"].as_str().unwrap_or("").to_owned(),
                i["assigned_to"].as_str().unwrap_or("").to_owned(),
                i["opened_by"].as_str().unwrap_or("").to_owned(),
                date(i["updated"].as_str().unwrap_or("")),
                r.comments().to_string(),
                if p.source == "model" { "Model".into() } else if r.local { "This model only".into() } else { "Server".into() },
            ];
            for (c, text) in cells.iter().enumerate() {
                p.list.set_item_text_by_column(k, c as i32 + 1, text);
            }
            if r.resolved {
                p.list.set_item_text_colour(k, &grey);
            } else if r.local && p.source != "model" {
                p.list.set_item_text_colour(k, &local);
            }
        }
        p.current = None;
        let at = keep.and_then(|id| p.rows.iter().position(|r| r.id == id)).or((!p.rows.is_empty()).then_some(0));
        match at {
            Some(k) => {
                p.list.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
                self.issue_chosen(k);
            }
            None => {
                p.detail.set_value(if p.all.is_empty() { "" } else { "No issue matches the filter." });
                if let Some(b) = blank() {
                    p.picture.set_bitmap(&b);
                }
                for b in [&p.show, &p.resolve, &p.edit, &p.add_view, &p.delete] {
                    b.enable(false);
                }
            }
        }
    }

    /// Every minute: a server's issues read again while the pane is shown; the status bar says
    /// what changed (new issues, replies, resolved or reopened).
    fn poll_issues(&mut self) {
        let on_server = self.issues_pane.as_ref().is_some_and(|p| matches!(p.source.as_str(), "fileengine" | "bcf_api") && p.doc == self.doc);
        if !on_server || !self.aui.is_pane_shown("issues") || self.app.active().is_none() {
            return;
        }
        let before: Vec<(String, bool, usize)> = self.issues_pane.as_ref().map(|p| p.all.iter().map(|r| (r.id.clone(), r.resolved, r.comments())).collect()).unwrap_or_default();
        self.refresh_issues(true);
        let Some(p) = self.issues_pane.as_ref() else { return };
        let (mut new, mut replies, mut changed) = (0, 0, 0);
        for r in &p.all {
            match before.iter().find(|b| b.0 == r.id) {
                None => new += 1,
                Some(b) => {
                    replies += r.comments().saturating_sub(b.2);
                    changed += usize::from(b.1 != r.resolved);
                }
            }
        }
        let mut what = Vec::new();
        if new > 0 {
            what.push(plural(new as u64, "new issue", "new issues"));
        }
        if replies > 0 {
            what.push(plural(replies as u64, "new reply", "new replies"));
        }
        if changed > 0 {
            what.push(format!("{changed} resolved or reopened"));
        }
        if !what.is_empty() {
            self.frame.set_status_text(&format!("Issues: {}", what.join(", ")), 0);
        }
    }

    /// After the active model changed: the pane shows the new one's issues (as last read).
    pub(crate) fn issues_follow_document(&mut self) {
        if self.issues_pane.as_ref().is_some_and(|p| p.doc != self.doc) && self.aui.is_pane_shown("issues") {
            self.refresh_issues(false);
        }
    }

    fn issue_chosen(&mut self, k: usize) {
        let Some(row) = self.issues_pane.as_ref().and_then(|p| p.rows.get(k)).cloned() else { return };
        let i = &row.issue;
        let picture = if i["has_snapshot"] == true {
            self.run("issues.snapshot", json!({ "id": row.id }))
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
                .and_then(|png| super::help::decode_png(&png))
                .and_then(|(rgba, w, h)| scaled(&rgba, w, h))
        } else {
            None
        };
        let Some(p) = self.issues_pane.as_mut() else { return };
        p.current = Some(k);
        if let Some(b) = picture.or_else(blank) {
            p.picture.set_bitmap(&b);
        }
        let mut text = String::new();
        text.push_str(&format!("{}{}\n", if row.resolved { "✓ RESOLVED · " } else { "" }, row.title));
        let mut fields = Vec::new();
        for (label, key) in [("Type", "topic_type"), ("Priority", "priority"), ("Assigned to", "assigned_to"), ("Due", "due_date"), ("Stage", "stage")] {
            if let Some(v) = i[key].as_str().filter(|v| !v.is_empty()) {
                fields.push(format!("{label}: {v}"));
            }
        }
        let labels = strings(&i["labels"]);
        if !labels.is_empty() {
            fields.push(format!("Labels: {}", labels.join(", ")));
        }
        if !fields.is_empty() {
            text.push_str(&format!("{}\n", fields.join(" · ")));
        }
        if let Some(d) = i["description"].as_str().filter(|d| !d.trim().is_empty()) {
            text.push_str(&format!("\n{d}\n"));
        }
        text.push('\n');
        for c in i["comments"].as_array().into_iter().flatten() {
            text.push_str(&format!("{} · {}\n{}\n\n", c["author"].as_str().unwrap_or(""), date(c["date"].as_str().unwrap_or("")), c["text"].as_str().unwrap_or("")));
        }
        let ms = strings(&i["measurements"]);
        if !ms.is_empty() {
            text.push_str(&format!("Measured: {}\n", ms.join(", ")));
        }
        if let Some(n) = i["objects"].as_array().map(Vec::len).filter(|n| *n > 0) {
            text.push_str(&format!("About {}\n", plural(n as u64, "element", "elements")));
        }
        if let Some(r) = i["resolved_by"].as_str() {
            text.push_str(&format!("Resolved by {r}\n"));
        }
        if i["stale"] == true {
            text.push_str("The model has changed since this viewpoint was taken.\n");
        }
        if let (Some(on), Some(here)) = (i["version"].as_str().filter(|s| !s.is_empty()), p.last["version"].as_str())
            && on != here
        {
            text.push_str(&format!("Raised on version {}\n", super::fileengine_ui::stamp(on)));
        }
        if row.local && p.source != "model" {
            text.push_str("Kept only with this model: Publish to Server puts it on the server.\n");
        }
        p.detail.set_value(text.trim_end());
        p.resolve.set_label(if row.resolved { "Reopen" } else { "Resolve" });
        let own = row.local || p.source == "model";
        let can = |k: &str| own || p.last["server_can"][k] == true;
        p.show.enable(i["viewpoints"].as_u64().unwrap_or(0) > 0);
        p.resolve.enable(true);
        p.edit.enable(can("edit_fields"));
        p.add_view.enable(can("add_view"));
        p.delete.enable(can("delete"));
    }

    fn current_issue(&self) -> Option<Row> {
        let p = self.issues_pane.as_ref()?;
        p.rows.get(p.current?).cloned()
    }

    fn issue_done(&mut self, r: Result<Value, String>, ok: &str) {
        match r {
            Ok(_) => {
                self.refresh_issues(false);
                if !ok.is_empty() {
                    self.frame.set_status_text(ok, 0);
                }
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn show_issue(&mut self) {
        let frame = self.frame;
        let Some(row) = self.current_issue() else { return };
        if row.issue["viewpoints"].as_u64().unwrap_or(0) == 0 {
            return frame.set_status_text("This issue has no viewpoint (a comment on the file)", 0);
        }
        match self.run("issues.show", json!({ "id": row.id })) {
            Ok(r) => {
                if let (Some(v), Some(doc)) = (r["view_id"].as_u64(), self.doc) {
                    self.open_view(doc, Some(reviera_core::ElementId(v)));
                }
                self.selection_changed();
                self.refresh_active();
                let missing = r["not_found"].as_array().map_or(0, Vec::len);
                frame.set_status_text(&if missing > 0 { format!("{}: {missing} of its elements are not in this model", row.title) } else { row.title.clone() }, 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    fn reply_issue(&mut self) {
        let frame = self.frame;
        let Some(row) = self.current_issue() else { return };
        let on_server = !row.local && self.issues_pane.as_ref().is_some_and(|p| p.source != "model");
        let f = super::reinforcement_ui::Form::new(&frame, &format!("Reply: {}", row.title));
        let c = f.lines("Comment", "", "Mention someone with @name@example.com");
        let ok = f.show(if on_server { "Posted to the issue on the server, as you." } else { "" }, "Reply");
        let text = c.get_value();
        f.dialog.destroy();
        if !ok || text.trim().is_empty() {
            return;
        }
        let r = self.run("issues.comment", json!({ "id": row.id, "text": text }));
        self.issue_done(r, "");
    }

    fn resolve_issue(&mut self) {
        let Some(row) = self.current_issue() else { return };
        let r = self.run("issues.set_status", json!({ "id": row.id, "resolved": !row.resolved }));
        self.issue_done(r, if row.resolved { "Opened again" } else { "Resolved" });
    }

    /// The issue form's fields (title, description and, where kept, type, status …), or `None`.
    fn issue_fields(&mut self, title: &str, issue: &Value, full: bool, note: &str, ok_label: &str, in_model: Option<&mut bool>) -> Option<Value> {
        let lists = self.issues_pane.as_ref().map(|p| p.last["lists"].clone()).unwrap_or(Value::Null);
        let f = super::reinforcement_ui::Form::new(&self.frame, title);
        let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let name = f.text("Title", &text(&issue["title"]), "Optional for a new issue: the description's first line otherwise");
        let description = f.lines("Description", &text(&issue["description"]), "What is wrong, or what is asked; mention someone with @name@example.com");
        let extra = full.then(|| {
            (
                f.combo("Type", &strings(&lists["topic_types"]), issue["topic_type"].as_str().unwrap_or("Issue"), "Or type a new one"),
                f.combo("Status", &strings(&lists["statuses"]), issue["status"].as_str().unwrap_or("Open"), "Or type a new one"),
                f.combo("Priority", &strings(&lists["priorities"]), issue["priority"].as_str().unwrap_or(""), ""),
                f.text("Labels", &strings(&issue["labels"]).join(", "), "Separated by commas"),
                f.combo("Assigned to", &strings(&lists["users"]), issue["assigned_to"].as_str().unwrap_or(""), "A name or e-mail"),
                f.text("Due date", &text(&issue["due_date"]), "YYYY-MM-DD"),
                f.combo("Stage", &strings(&lists["stages"]), issue["stage"].as_str().unwrap_or(""), ""),
            )
        });
        let keep = in_model.as_ref().map(|_| f.check("", "Keep it only with this model (not on the server)", false));
        if !f.show(note, ok_label) {
            f.dialog.destroy();
            return None;
        }
        let mut out = json!({ "title": name.get_value().trim(), "description": description.get_value() });
        if let Some((kind, status, priority, labels, assigned, due, stage)) = extra {
            let labels: Vec<String> = labels.get_value().split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
            out["fields"] = json!({
                "topic_type": kind.get_value().trim(),
                "status": status.get_value().trim(),
                "priority": priority.get_value().trim(),
                "labels": labels,
                "assigned_to": assigned.get_value().trim(),
                "due_date": due.get_value().trim(),
                "stage": stage.get_value().trim(),
            });
        }
        if let (Some(flag), Some(k)) = (in_model, keep) {
            *flag = k.get_value();
        }
        f.dialog.destroy();
        Some(out)
    }

    fn edit_issue(&mut self) {
        let Some(row) = self.current_issue() else { return };
        let Some(v) = self.issue_fields("Edit Issue", &row.issue.clone(), true, "", "OK", None) else { return };
        if v["title"].as_str().is_some_and(str::is_empty) {
            return self.frame.set_status_text("An issue needs a title", 0);
        }
        let mut fields = v["fields"].clone();
        fields["title"] = v["title"].clone();
        fields["description"] = v["description"].clone();
        let r = self.run("issues.update", json!({ "id": row.id, "fields": fields }));
        self.issue_done(r, "");
    }

    fn add_view_to_issue(&mut self) {
        let Some(row) = self.current_issue() else { return };
        let r = self.run("issues.add_view", json!({ "id": row.id }));
        self.issue_done(r, "The 3D view was added to the issue");
    }

    fn delete_issue(&mut self) {
        let Some(row) = self.current_issue() else { return };
        let on_server = !row.local && self.issues_pane.as_ref().is_some_and(|p| p.source == "bcf_api");
        let msg = if on_server { format!("Delete \u{201c}{}\u{201d} on the issue server, for everyone?", row.title) } else { format!("Delete \u{201c}{}\u{201d}?", row.title) };
        if !super::ask(&self.frame, &msg) {
            return;
        }
        let r = self.run("issues.delete", json!({ "id": row.id }));
        self.issue_done(r, "Deleted");
    }

    /// New Issue… (and View › Coordination › New Issue): where the model's issues are kept.
    pub(crate) fn new_issue_here(&mut self) {
        if self.app.active().is_none() {
            return;
        }
        if self.issues_pane.is_none() || self.issues_pane.as_ref().is_some_and(|p| p.doc != self.doc) {
            self.show_issues_pane();
        }
        let source = self.issues_pane.as_ref().map(|p| p.source.clone()).unwrap_or_default();
        let (title, note) = match source.as_str() {
            "fileengine" => ("New Issue on FileEngine", "Kept with the 3D view as it is now: camera, section box, hidden elements, the selection and a snapshot."),
            "bcf_api" => ("New Issue on the Issue Server", "Kept with the 3D view as it is now: camera, section box, hidden elements, the selection and a snapshot."),
            _ => ("New Issue", "Kept with the model, with the 3D view as it is now and a snapshot."),
        };
        let mut in_model = false;
        let keep_choice = (source != "model").then_some(&mut in_model);
        let Some(v) = self.issue_fields(title, &json!({}), source != "fileengine", note, "Raise", keep_choice) else { return };
        let text = v["description"].as_str().unwrap_or("").to_owned();
        if text.trim().is_empty() && v["title"].as_str().unwrap_or("").is_empty() {
            return self.frame.set_status_text("Say what the issue is", 0);
        }
        let mut args = json!({ "title": v["title"], "text": text, "in_model": in_model });
        if v["fields"].is_object() {
            args["fields"] = v["fields"].clone();
        }
        self.frame.set_status_text("Raising the issue…", 0);
        let r = self.run("issues.new", args);
        self.frame.set_status_text("", 0);
        match r {
            Ok(i) => {
                self.refresh_issues(false);
                self.frame.set_status_text(&format!("Raised: {}", i["title"].as_str().unwrap_or("")), 0);
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn publish_issues(&mut self) {
        let frame = self.frame;
        frame.set_status_text("Putting the model's issues on the server…", 0);
        let r = self.run("issues.publish", json!({}));
        frame.set_status_text("", 0);
        match r {
            Ok(v) => {
                let n = v["published"].as_array().map_or(0, Vec::len) as u64;
                let failed = v["failed"].as_array().cloned().unwrap_or_default();
                self.refresh_issues(false);
                if failed.is_empty() {
                    frame.set_status_text(&format!("{} put on the server", plural(n, "issue", "issues")), 0);
                } else {
                    let why: Vec<String> = failed.iter().map(|f| format!("• {}", f[1].as_str().unwrap_or(""))).collect();
                    error(&frame, &format!("{} put on the server; {} not:\n{}", plural(n, "issue", "issues"), failed.len(), why.join("\n")));
                }
            }
            Err(e) => error(&frame, &e),
        }
    }

    fn import_bcf_issues(&mut self) {
        self.bcf_import();
        self.refresh_issues(false);
    }

    fn export_bcf_issues(&mut self) {
        let frame = self.frame;
        let versions = ["BCF 3.0".to_owned(), "BCF 2.1".to_owned()];
        let f = super::reinforcement_ui::Form::new(&frame, "Export BCF");
        let v = f.choice("Version", &versions, 0);
        let shown = f.check("", "Only the issues the filter shows", false);
        let ok = f.show("Every issue of the model, wherever it is kept, with its viewpoints, snapshots and comments. 2.1 for tools that do not read 3.0 yet.", "Next…");
        let version = if v.get_selection() == Some(1) { "2.1" } else { "3.0" };
        let only_shown = shown.get_value();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let fd = FileDialog::builder(&frame).with_message("Export BCF").with_wildcard("BCF (*.bcf)|*.bcf|BCF zip (*.bcfzip)|*.bcfzip").with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt).build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        let mut args = json!({ "path": path, "version": version });
        if only_shown && let Some(p) = &self.issues_pane {
            args["ids"] = json!(p.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
        }
        match self.run("issues.export_bcf", args) {
            Ok(r) => frame.set_status_text(&format!("Wrote {} issues, {} comments, {} viewpoints and {} snapshots (BCF {version}) to {path}", r["topics"], r["comments"], r["viewpoints"], r["snapshots"]), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Issue Server…: keep the model's issues on a BCF issue server's project, or with the model.
    pub(crate) fn issue_server(&mut self) {
        let frame = self.frame;
        if self.app.active().is_none() {
            return frame.set_status_text("Open a model first", 0);
        }
        loop {
            let servers = self.run("coordination.servers", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let status = self.run("coordination.status", json!({})).unwrap_or_default();
            let now = match status["source"].as_str() {
                Some("bcf_api") => format!("This model's issues are kept on {} · {}.", status["server"].as_str().unwrap_or(""), status["project"].as_str().unwrap_or("")),
                Some("fileengine") => "This model's issues are kept on FileEngine (it was opened from there).".to_owned(),
                _ => "This model's issues are kept with the model.".to_owned(),
            };
            let mut items: Vec<String> = servers.iter().map(|s| format!("{} ({}, BCF API {})", s["name"].as_str().unwrap_or(""), s["url"].as_str().unwrap_or(""), s["version"].as_str().unwrap_or(""))).collect();
            let (add, remove, sign_in, unbind) = (items.len(), items.len() + 1, items.len() + 2, items.len() + 3);
            items.push("Add a server…".into());
            items.push("Remove a server…".into());
            items.push("Sign in to a server again…".into());
            items.push(if status["source"] == "fileengine" { "Not on an issue server (keep them on FileEngine)".into() } else { "Not on an issue server (keep them with the model)".into() });
            let f = super::reinforcement_ui::Form::new(&frame, "Issue Server");
            let at = status["binding"]["server_id"].as_str().and_then(|id| servers.iter().position(|s| s["id"] == id)).unwrap_or(if servers.is_empty() { add } else { 0 });
            let c = f.choice("Keep issues on", &items, at);
            let ok = f.show(&format!("{now}\nA BCF API server (OpenConstructionERP, or another BCF API 3.0 or 2.1 server) holds issues by project; the model is bound by its file, which is not changed."), "Next…");
            let k = c.get_selection().unwrap_or(0) as usize;
            f.dialog.destroy();
            if !ok {
                return;
            }
            if k == add {
                self.add_issue_server();
                continue;
            }
            if k == remove || k == sign_in {
                let names: Vec<String> = servers.iter().map(|s| s["name"].as_str().unwrap_or("").to_owned()).collect();
                if names.is_empty() {
                    continue;
                }
                let f = super::reinforcement_ui::Form::new(&frame, if k == remove { "Remove a Server" } else { "Sign In Again" });
                let c = f.choice("Server", &names, 0);
                let pw = (k == sign_in).then(|| f.password("Password or token", "Kept in the system keychain"));
                let ok = f.show(if k == remove { "Its stored password or token is deleted, and models bound to it keep their issues with them again. Issues on the server are not touched." } else { "" }, if k == remove { "Remove" } else { "Sign In" });
                let s = servers.get(c.get_selection().unwrap_or(0) as usize).cloned();
                let secret = pw.map(|p| p.get_value());
                f.dialog.destroy();
                let (true, Some(s)) = (ok, s) else { continue };
                let r = if k == remove { self.run("coordination.remove_server", json!({ "server_id": s["id"] })) } else { self.run("coordination.sign_in", json!({ "server_id": s["id"], "secret": secret.unwrap_or_default() })) };
                if let Err(e) = r {
                    error(&frame, &e);
                }
                continue;
            }
            if k == unbind {
                if let Err(e) = self.run("coordination.unbind", json!({})) {
                    error(&frame, &e);
                }
                break;
            }
            let Some(s) = servers.get(k) else { return };
            let projects = match self.run("coordination.projects", json!({ "server_id": s["id"] })) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => {
                    error(&frame, &format!("The projects could not be read:\n{e}"));
                    continue;
                }
            };
            if projects.is_empty() {
                error(&frame, "You may see no projects on this server.");
                continue;
            }
            let names: Vec<String> = projects.iter().map(|p| p["name"].as_str().unwrap_or("").to_owned()).collect();
            let f = super::reinforcement_ui::Form::new(&frame, "Issue Server Project");
            let at = projects.iter().position(|p| p["project_id"] == status["binding"]["project_id"]).unwrap_or(0);
            let c = f.choice("Project", &names, at);
            let ok = f.show("The model's issues are read from and written to this project. Its own BCF topics stay with it until Publish to Server.", "Use This Project");
            let p = projects.get(c.get_selection().unwrap_or(0) as usize).cloned();
            f.dialog.destroy();
            let (true, Some(p)) = (ok, p) else { continue };
            if let Err(e) = self.run("coordination.bind", json!({ "server_id": s["id"], "project_id": p["project_id"] })) {
                error(&frame, &e);
                continue;
            }
            break;
        }
        self.show_issues_pane();
        self.refresh_issues(true);
    }

    fn add_issue_server(&mut self) {
        let frame = self.frame;
        let kinds = ["E-mail and password (OpenConstructionERP)".to_owned(), "Access token".to_owned(), "User name and password (HTTP Basic)".to_owned(), "None".to_owned()];
        let f = super::reinforcement_ui::Form::new(&frame, "Add an Issue Server");
        let name = f.text("Name", "", "How it is listed here");
        let url = f.text("Address", "https://", "The server's address, or its BCF API's (…/bcf/3.0)");
        let kind = f.choice("Sign in with", &kinds, 0);
        let user = f.text("E-mail or user name", "", "");
        let secret = f.password("Password or token", "Kept in the system keychain, never shown again");
        let login = f.text("Login address", "", "Optional: where the e-mail and password go (OpenConstructionERP's by default)");
        let remember = f.check("", "Remember the password or token", true);
        let ok = f.show("Reviera finds the BCF API (3.0, or 2.1) from the address and signs in to check.", "Add");
        let auth = ["login", "token", "basic", "none"][kind.get_selection().unwrap_or(0).min(3) as usize];
        let args = json!({
            "name": name.get_value().trim(),
            "url": url.get_value().trim(),
            "auth": auth,
            "user": user.get_value().trim(),
            "secret": secret.get_value(),
            "login_url": login.get_value().trim(),
            "remember": remember.get_value(),
        });
        f.dialog.destroy();
        if !ok {
            return;
        }
        frame.set_status_text("Looking for the BCF API…", 0);
        let r = self.run("coordination.add_server", args);
        frame.set_status_text("", 0);
        match r {
            Ok(s) => frame.set_status_text(&format!("Added {} (BCF API {})", s["name"].as_str().unwrap_or(""), s["version"].as_str().unwrap_or("")), 0),
            Err(e) => error(&frame, &format!("The server was not added:\n{e}")),
        }
    }
}
