// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Opening models in the background: the Open dialog closes at once, and a tab shows the
//! progress (reading the file, then building its geometry) while a worker thread does the work.
//! The finished model is attached on the window's thread, with the usual recovery and lock
//! prompts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use reviera_app::LoadStage;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, ask, error, user, with};

/// A model being opened.
pub struct Loading {
    pub path: PathBuf,
    pub panel: Panel,
    stage: Arc<AtomicU8>,
    done: Receiver<reviera_core::Result<reviera_app::Loaded>>,
    started: Instant,
}

impl Gui {
    /// Starts opening `path` in the background (File › Open, a file on the command line).
    pub(crate) fn open(&mut self, path: &Path, _frame: &Frame) {
        // Already open: show it.
        if let Some(doc) = self.app.document_at(path) {
            if let Some(v) = self.views.iter().find(|v| v.doc == doc).map(|v| v.id) {
                self.activate_view(v);
            }
            return;
        }
        if let Some(l) = self.loading.iter().find(|l| l.path == path)
            && let Some(i) = super::wx::page_index(&self.notebook, &l.panel)
        {
            self.notebook.set_selection(i);
            return;
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let panel = Panel::builder(&self.notebook).build();
        let stage = Arc::new(AtomicU8::new(LoadStage::Reading as u8));
        if let Some(gui) = self.this.upgrade() {
            let p = panel;
            panel.on_paint(move |_| {
                let dc = PaintDC::new(&p);
                with(&gui, |g| g.paint_loading(&p, &dc));
            });
        }
        self.notebook
            .add_page(&panel, &format!("Opening {name}…"), true);
        let (tx, done) = mpsc::channel();
        let (worker_path, worker_stage) = (path.to_owned(), stage.clone());
        std::thread::Builder::new()
            .name("reviera-load".into())
            .spawn(move || {
                let result = reviera_app::load(&worker_path, &|s| {
                    worker_stage.store(s as u8, Ordering::Relaxed)
                });
                // The window may have closed the tab (cancelled): nobody listens then.
                let _ = tx.send(result);
            })
            .expect("the loading thread starts");
        self.loading.push(Loading {
            path: path.to_owned(),
            panel,
            stage,
            done,
            started: Instant::now(),
        });
        self.frame
            .set_status_text(&format!("Opening {}…", path.display()), 0);
        if let Some(t) = &self.load_timer
            && !t.is_running()
        {
            t.start(50, false);
        }
    }

    /// Draws a loading tab: the file name, the stage, and a moving progress bar.
    fn paint_loading(&self, panel: &Panel, dc: &PaintDC) {
        let Some(l) = self
            .loading
            .iter()
            .find(|l| l.panel.get_handle() == panel.get_handle())
        else {
            return;
        };
        let size = panel.get_client_size();
        let (w, h) = (size.width, size.height);
        dc.set_background(Colour::new(204, 214, 224, 255));
        dc.clear();
        let name = l
            .path
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let stage = match l.stage.load(Ordering::Relaxed) {
            0 => "Reading the IFC file",
            _ => "Building geometry",
        };
        let elapsed = l.started.elapsed().as_secs_f32();
        let lines = [
            format!("Opening {name}"),
            format!("{stage}… {elapsed:.0} s"),
        ];
        dc.set_text_foreground(Colour::new(40, 44, 52, 255));
        let bar_w = (w / 3).clamp(200, 480);
        let (bx, by) = ((w - bar_w) / 2, h / 2);
        for (k, line) in lines.iter().enumerate() {
            let (tw, th) = dc.get_text_extent(line);
            dc.draw_text(line, (w - tw) / 2, by - 16 - (2 - k as i32) * (th + 6));
        }
        // The bar: the trough, the finished stages, and a block sweeping across the current one.
        let bar_h = 10;
        dc.set_pen(Colour::new(140, 150, 165, 255), 1, PenStyle::Solid);
        dc.set_brush(Colour::new(240, 243, 247, 255), BrushStyle::Solid);
        dc.draw_rounded_rectangle(bx, by, bar_w, bar_h, 4.0);
        let stages = 2;
        let current = i32::from(l.stage.load(Ordering::Relaxed).min(1));
        let span = bar_w / stages;
        dc.set_pen(Colour::new(40, 110, 220, 255), 1, PenStyle::Transparent);
        dc.set_brush(Colour::new(40, 110, 220, 255), BrushStyle::Solid);
        if current > 0 {
            dc.draw_rounded_rectangle(bx, by, span * current, bar_h, 4.0);
        }
        let block = span / 3;
        let phase = (elapsed * 0.8).fract();
        let x = bx
            + span * current
            + ((span - block) as f32 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos())) as i32;
        dc.draw_rounded_rectangle(x, by, block, bar_h, 4.0);
    }

    /// Animates the loading tabs and attaches finished models; returns whether any is still
    /// loading.
    pub(crate) fn poll_loading(&mut self) -> bool {
        let mut finished = Vec::new();
        for (i, l) in self.loading.iter().enumerate() {
            match l.done.try_recv() {
                Ok(result) => finished.push((i, Some(result))),
                Err(TryRecvError::Empty) => l.panel.refresh(false, None),
                Err(TryRecvError::Disconnected) => finished.push((i, None)),
            }
        }
        for (i, result) in finished.into_iter().rev() {
            let l = self.loading.remove(i);
            if let Some(page) = super::wx::page_index(&self.notebook, &l.panel) {
                self.notebook.delete_page(page);
            }
            match result {
                Some(r) => self.finish_open(&l.path, r),
                None => error(
                    &self.frame,
                    &format!("Opening {} stopped unexpectedly", l.path.display()),
                ),
            }
        }
        !self.loading.is_empty()
    }

    /// Cancels opening the model shown in notebook page `index`; returns whether it was one.
    pub(crate) fn cancel_loading(&mut self, index: usize) -> bool {
        let Some(i) = self
            .loading
            .iter()
            .position(|l| super::wx::page_index(&self.notebook, &l.panel) == Some(index))
        else {
            return false;
        };
        let l = self.loading.remove(i);
        // The worker finishes and its result is dropped.
        self.notebook.delete_page(index);
        self.frame
            .set_status_text(&format!("Cancelled opening {}", l.path.display()), 0);
        true
    }

    /// Attaches a loaded model, with the recovery and lock prompts of `file.open`.
    fn finish_open(&mut self, path: &Path, result: reviera_core::Result<reviera_app::Loaded>) {
        let frame = self.frame;
        let mut opened = result.and_then(|loaded| self.app.open_loaded(loaded, false));
        // Unsaved work from a session that ended unexpectedly: offer to restore it (DM-LIVE-004).
        if let Err(e) = &opened
            && e.code == "recovery.pending"
        {
            let key = e.details["store_key"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let message = format!(
                "{} has unsaved work from a session that ended unexpectedly.\n\nRestore it? (No discards it.)",
                path.display()
            );
            let id = |v: serde_json::Value| v.as_u64().unwrap_or_default();
            opened = if ask(&frame, &message) {
                self.app
                    .execute("recovery.restore", json!({ "store_key": key }), &user())
                    .map(|o| id(o.result["model_id"].clone()))
            } else {
                let _ = self
                    .app
                    .execute("recovery.discard", json!({ "store_key": key }), &user());
                self.app
                    .execute("file.open", json!({ "path": path }), &user())
                    .map(|o| id(o.result))
            };
        }
        // Open in another session: open read-only.
        if opened.as_ref().is_err_and(|e| e.code == "file.locked") {
            opened = self
                .app
                .execute(
                    "file.open",
                    json!({ "path": path, "read_only": true }),
                    &user(),
                )
                .map(|o| o.result.as_u64().unwrap_or_default());
        }
        match opened {
            Ok(id) => self.show_document(id, &frame),
            Err(e) => error(&frame, &format!("Could not open {}:\n{e}", path.display())),
        }
    }
}
