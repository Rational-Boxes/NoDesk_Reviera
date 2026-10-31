// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Issues pane (COL-BCF-004, COL-FE-009), docked at the bottom: the active model's issues.
//! For a model opened from FileEngine, the discussion threads people raised on it there (in its
//! 3D viewer or here), read from the server when the model opens; otherwise the BCF topics kept
//! with the model. Choosing one shows its snapshot and comments; Show opens its viewpoint in the 3D
//! view; Reply…, Resolve and New Issue… write to wherever the issues live.

use base64::Engine;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error, with};

/// The snapshot preview's width.
const PICTURE_W: u32 = 300;
const PICTURE_H: u32 = 188;

/// One issue as the pane shows it.
#[derive(Clone)]
struct Row {
    /// The thread id (FileEngine) or topic GUID (the model's).
    id: String,
    title: String,
    status: String,
    by: String,
    date: String,
    comments: Vec<(String, String, String)>,
    extra: Vec<String>,
    viewpoint: bool,
    snapshot: bool,
}

pub(crate) struct IssuesPane {
    list: ListCtrl,
    info: StaticText,
    picture: StaticBitmap,
    detail: TextCtrl,
    resolve: Button,
    rows: Vec<Row>,
    /// The document shown.
    doc: Option<u64>,
    /// The issues are a FileEngine model's.
    on_server: bool,
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

    fn build_issues_pane(&mut self) {
        let Some(gui) = self.this.upgrade() else { return };
        let (frame, aui) = (self.frame, self.aui);
        let panel = Panel::builder(&frame).build();
        let info = StaticText::builder(&panel).with_label("").build();
        let list = ListCtrl::builder(&panel).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
        for (k, (h, w)) in [("Issue", 340), ("Status", 80), ("Raised by", 200), ("Changed", 130), ("Replies", 60)].into_iter().enumerate() {
            list.insert_column(k as i64, h, ListColumnFormat::Left, w);
        }
        let picture = StaticBitmap::builder(&panel).with_bitmap(blank()).build();
        picture.set_min_size(Size::new(PICTURE_W as i32, PICTURE_H as i32));
        let detail = TextCtrl::builder(&panel).with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly).build();
        detail.set_min_size(Size::new(360, PICTURE_H as i32));
        let show = button(&panel, "Show", "Open the issue's viewpoint in the 3D view: camera, cut, hidden elements, and the elements it is about selected");
        let reply = button(&panel, "Reply…", "Add a comment to the issue");
        let resolve = button(&panel, "Resolve", "Resolve the issue (or open it again)");
        let new = button(&panel, "New Issue…", "Raise an issue from the 3D view: its camera, cut, hidden elements, selection and a snapshot");
        let refresh = button(&panel, "Refresh", "Read the issues again");

        let top = BoxSizer::builder(Orientation::Horizontal).build();
        for b in [&show, &reply, &resolve, &new, &refresh] {
            top.add(b, 0, SizerFlag::Right, 4);
        }
        top.add(&info, 1, SizerFlag::AlignCenterVertical | SizerFlag::Left, 8);
        let body = BoxSizer::builder(Orientation::Horizontal).build();
        body.add(&list, 3, SizerFlag::Expand | SizerFlag::Right, 6);
        body.add(&picture, 0, SizerFlag::Right, 6);
        body.add(&detail, 2, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add_sizer(&top, 0, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(&body, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("issues")
                .with_caption("Issues")
                .bottom()
                .layer(1)
                .best_size(1100, 260)
                .min_size(500, 180)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        self.issues_pane = Some(IssuesPane { list, info, picture, detail, resolve, rows: Vec::new(), doc: None, on_server: false, current: None });
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
        {
            let g = gui.clone();
            show.on_click(move |_| with(&g, Gui::show_issue));
        }
        {
            let g = gui.clone();
            reply.on_click(move |_| with(&g, Gui::reply_issue));
        }
        {
            let g = gui.clone();
            resolve.on_click(move |_| with(&g, Gui::resolve_issue));
        }
        {
            let g = gui.clone();
            new.on_click(move |_| with(&g, Gui::new_issue_here));
        }
        {
            let g = gui.clone();
            refresh.on_click(move |_| with(&g, |g| g.refresh_issues(true)));
        }
    }

    /// Reads the active model's issues into the pane (`ask` asks the server again).
    pub(crate) fn refresh_issues(&mut self, ask: bool) {
        if self.issues_pane.is_none() {
            return;
        }
        let linked = self.app.active().is_some() && self.run("fileengine.status", Value::Null).ok().is_some_and(|s| s["linked"] == true);
        let (rows, note) = if linked {
            match self.run("fileengine.issues", json!({ "refresh": ask })) {
                Ok(v) => {
                    let rows: Vec<Row> = v["issues"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|i| Row {
                            id: i["thread_id"].as_str().unwrap_or("").to_owned(),
                            title: i["title"].as_str().unwrap_or("").to_owned(),
                            status: i["status"].as_str().unwrap_or("").to_owned(),
                            by: i["opened_by"].as_str().unwrap_or("").to_owned(),
                            date: date(i["updated"].as_str().unwrap_or("")),
                            comments: i["comments"].as_array().into_iter().flatten().map(|c| (c["author"].as_str().unwrap_or("").to_owned(), date(c["date"].as_str().unwrap_or("")), c["text"].as_str().unwrap_or("").to_owned())).collect(),
                            extra: {
                                let mut e = Vec::new();
                                if let Some(ms) = i["measurements"].as_array().filter(|m| !m.is_empty()) {
                                    e.push(format!("Measured: {}", ms.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
                                }
                                if let Some(o) = i["objects"].as_array().filter(|o| !o.is_empty()) {
                                    e.push(format!("About {} element{}", o.len(), if o.len() == 1 { "" } else { "s" }));
                                }
                                if let Some(r) = i["resolved_by"].as_str() {
                                    e.push(format!("Resolved by {r}"));
                                }
                                if i["stale"] == true {
                                    e.push("The model has changed since this viewpoint was taken.".into());
                                }
                                if let (Some(on), Some(here)) = (i["version"].as_str().filter(|s| !s.is_empty()), v["version"].as_str())
                                    && on != here
                                {
                                    e.push(format!("Raised on version {}", super::fileengine_ui::stamp(on)));
                                }
                                e
                            },
                            viewpoint: !i["viewpoint"].is_null(),
                            snapshot: i["has_snapshot"] == true,
                        })
                        .collect();
                    let open = v["open"].as_u64().unwrap_or(0);
                    let note = format!(
                        "{} on FileEngine: {open} open, {} resolved{}",
                        v["model"].as_str().unwrap_or("This model"),
                        rows.len() as u64 - open,
                        if v["offline"] == true { " (offline: as last read)" } else { "" }
                    );
                    (rows, note)
                }
                Err(e) => (Vec::new(), format!("The issues could not be read: {e}")),
            }
        } else if self.app.active().is_some() {
            let list = self.run("bcf.list", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let rows: Vec<Row> = list
                .iter()
                .map(|t| {
                    let full = self.run("bcf.get", json!({ "topic_id": t["guid"] })).unwrap_or_default();
                    let topic = if full["topic"].is_object() { full["topic"].clone() } else { full.clone() };
                    Row {
                        id: t["guid"].as_str().unwrap_or("").to_owned(),
                        title: t["title"].as_str().unwrap_or("").to_owned(),
                        status: t["status"].as_str().unwrap_or("").to_owned(),
                        by: topic["creation_author"].as_str().unwrap_or("").to_owned(),
                        date: date(t["modified"].as_str().unwrap_or("")),
                        comments: topic["comments"].as_array().into_iter().flatten().map(|c| (c["author"].as_str().unwrap_or("").to_owned(), date(c["date"].as_str().unwrap_or("")), c["text"].as_str().unwrap_or("").to_owned())).collect(),
                        extra: topic["description"].as_str().map(|d| vec![d.to_owned()]).unwrap_or_default(),
                        viewpoint: t["viewpoints"].as_u64().unwrap_or(0) > 0,
                        snapshot: false,
                    }
                })
                .collect();
            let note = format!("{} BCF issue{} kept with the model", rows.len(), if rows.len() == 1 { "" } else { "s" });
            (rows, note)
        } else {
            (Vec::new(), "Open a model to see its issues.".into())
        };
        let doc = self.doc;
        let Some(p) = self.issues_pane.as_mut() else { return };
        p.doc = doc;
        p.on_server = linked;
        p.list.delete_all_items();
        for (k, r) in rows.iter().enumerate() {
            p.list.insert_item(k as i64, &r.title, None);
            p.list.set_item_text_by_column(k as i64, 1, &r.status);
            p.list.set_item_text_by_column(k as i64, 2, &r.by);
            p.list.set_item_text_by_column(k as i64, 3, &r.date);
            p.list.set_item_text_by_column(k as i64, 4, &r.comments.len().saturating_sub(1).to_string());
        }
        p.info.set_label(&note);
        let keep = p.current.and_then(|k| p.rows.get(k)).map(|r| r.id.clone());
        p.rows = rows;
        p.current = None;
        let at = keep.and_then(|id| p.rows.iter().position(|r| r.id == id)).or((!p.rows.is_empty()).then_some(0));
        match at {
            Some(k) => {
                p.list.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
                self.issue_chosen(k);
            }
            None => {
                p.detail.set_value("");
                if let Some(b) = blank() {
                    p.picture.set_bitmap(&b);
                }
            }
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
        let on_server = self.issues_pane.as_ref().is_some_and(|p| p.on_server);
        let picture = if on_server && row.snapshot {
            self.run("fileengine.issue_snapshot", json!({ "thread_id": row.id }))
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
        for (who, when, what) in &row.comments {
            text.push_str(&format!("{who} · {when}\n{what}\n\n"));
        }
        for e in &row.extra {
            text.push_str(&format!("{e}\n"));
        }
        p.detail.set_value(text.trim_end());
        let resolved = matches!(row.status.to_lowercase().as_str(), "resolved" | "closed");
        p.resolve.set_label(if resolved { "Reopen" } else { "Resolve" });
    }

    fn current_issue(&self) -> Option<(Row, bool)> {
        let p = self.issues_pane.as_ref()?;
        Some((p.rows.get(p.current?)?.clone(), p.on_server))
    }

    fn show_issue(&mut self) {
        let frame = self.frame;
        let Some((row, on_server)) = self.current_issue() else { return };
        if !row.viewpoint {
            return frame.set_status_text("This is a comment on the file, with no viewpoint", 0);
        }
        let r = if on_server { self.run("fileengine.show_issue", json!({ "thread_id": row.id })) } else { self.run("bcf.apply_viewpoint", json!({ "topic_id": row.id })) };
        match r {
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
        let Some((row, on_server)) = self.current_issue() else { return };
        let f = super::reinforcement_ui::Form::new(&frame, &format!("Reply: {}", row.title));
        let c = f.lines("Comment", "", "Mention someone with @name@example.com");
        let ok = f.show(if on_server { "Posted to the issue on FileEngine, as you." } else { "" }, "Reply");
        let text = c.get_value();
        f.dialog.destroy();
        if !ok || text.trim().is_empty() {
            return;
        }
        let r = if on_server { self.run("fileengine.comment_issue", json!({ "thread_id": row.id, "text": text })) } else { self.run("bcf.add_comment", json!({ "topic_id": row.id, "text": text })) };
        match r {
            Ok(_) => self.refresh_issues(false),
            Err(e) => error(&frame, &e),
        }
    }

    fn resolve_issue(&mut self) {
        let frame = self.frame;
        let Some((row, on_server)) = self.current_issue() else { return };
        let resolved = matches!(row.status.to_lowercase().as_str(), "resolved" | "closed");
        let r = if on_server {
            self.run("fileengine.resolve_issue", json!({ "thread_id": row.id, "resolved": !resolved }))
        } else {
            self.run("bcf.update_topic", json!({ "topic_id": row.id, "fields": { "status": if resolved { "Open" } else { "Resolved" } } }))
        };
        match r {
            Ok(_) => self.refresh_issues(false),
            Err(e) => error(&frame, &e),
        }
    }

    fn new_issue_here(&mut self) {
        let frame = self.frame;
        let on_server = self.issues_pane.as_ref().is_some_and(|p| p.on_server);
        if !on_server {
            self.bcf_new_issue();
            return self.refresh_issues(false);
        }
        let f = super::reinforcement_ui::Form::new(&frame, "New Issue on FileEngine");
        let title = f.text("Title", "", "Optional: the comment's first line otherwise");
        let c = f.lines("Comment", "", "What is wrong, or what is asked; mention someone with @name@example.com");
        let ok = f.show("Kept with the 3D view as it is now: camera, section box, hidden elements, the selection and a snapshot.", "Raise");
        let (t, text) = (title.get_value(), c.get_value());
        f.dialog.destroy();
        if !ok || text.trim().is_empty() {
            return;
        }
        frame.set_status_text("Raising the issue on FileEngine…", 0);
        let r = self.run("fileengine.new_issue", json!({ "title": t, "text": text }));
        frame.set_status_text("", 0);
        match r {
            Ok(i) => {
                self.refresh_issues(false);
                frame.set_status_text(&format!("Raised: {}", i["title"].as_str().unwrap_or("")), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }
}
