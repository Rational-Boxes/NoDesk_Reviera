// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Performance Profiler (DM-PERF-001): a live window refreshed every second: Start /
//! Stop and the slow threshold; the live metrics (frame time, fps, memory, model size, slow
//! operations); the latest operations; the hottest scopes by self time; the heaviest call paths;
//! Reset, Export Report… and Export Trace…. A slow operation also shows on the status bar.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error, with};

/// The open window's controls.
pub(crate) struct ProfilerWindow {
    dialog: Dialog,
    timer: Timer<Dialog>,
    toggle: Button,
    slow: TextCtrl,
    live: StaticText,
    recent: ListCtrl,
    hot: ListCtrl,
    paths: ListCtrl,
    /// Slow operations already shown on the status bar.
    slow_seen: usize,
}

fn list(parent: &Dialog, columns: &[(&str, i32)]) -> ListCtrl {
    let l = ListCtrl::builder(parent).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
    for (k, (h, w)) in columns.iter().enumerate() {
        l.insert_column(k as i64, h, if k == 0 { ListColumnFormat::Left } else { ListColumnFormat::Right }, *w);
    }
    l
}

fn fill(l: &ListCtrl, rows: &[Vec<String>]) {
    l.delete_all_items();
    for (k, row) in rows.iter().enumerate() {
        l.insert_item(k as i64, &row[0], None);
        for (c, text) in row.iter().enumerate().skip(1) {
            l.set_item_text_by_column(k as i64, c as i32, text);
        }
    }
}

fn ms(v: &Value) -> String {
    v.as_f64().map_or(String::new(), |x| if x >= 100.0 { format!("{x:.0}") } else { format!("{x:.1}") })
}

impl Gui {
    /// Manage › Performance Profiler: opens (or raises) the live window.
    pub(crate) fn profiler_window(&mut self) {
        if let Some(w) = &self.profiler {
            w.dialog.raise();
            return;
        }
        let Some(gui) = self.this.upgrade() else { return };
        let dialog = Dialog::builder(&self.frame, "Performance Profiler").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
        let intro = StaticText::builder(&dialog)
            .with_label("Times every command, commit, regeneration, scene build, IFC open and save, merge, server call and viewport frame while it runs, to find slowdowns. Export Report… writes a report for development; Export Trace… a timeline for Perfetto.")
            .build();
        intro.wrap(900);
        let top = BoxSizer::builder(Orientation::Horizontal).build();
        let toggle = Button::builder(&dialog).with_label("Start").build();
        let label = StaticText::builder(&dialog).with_label("Slow at (ms)").build();
        let slow = TextCtrl::builder(&dialog).with_value("100").build();
        slow.set_min_size(Size::new(70, -1));
        top.add(&toggle, 0, SizerFlag::Right, 12);
        top.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, 6);
        top.add(&slow, 0, SizerFlag::Right, 12);
        let live = StaticText::builder(&dialog).with_label("").build();
        live.set_min_size(Size::new(900, 40));
        let recent = list(&dialog, &[("Latest operations", 380), ("ms", 90), ("Detail", 240)]);
        let hot = list(&dialog, &[("Scope (by self time)", 300), ("Calls", 70), ("Self ms", 90), ("Total ms", 90), ("p95 ms", 80), ("Max ms", 80)]);
        let paths = list(&dialog, &[("Call path (by self time)", 560), ("Calls", 70), ("Self ms", 90), ("Total ms", 90)]);
        for (l, h) in [(&recent, 150), (&hot, 200), (&paths, 160)] {
            l.set_min_size(Size::new(900, h));
        }
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let reset = Button::builder(&dialog).with_label("Reset").build();
        let report = Button::builder(&dialog).with_label("Export Report…").build();
        let trace = Button::builder(&dialog).with_label("Export Trace…").build();
        let close = Button::builder(&dialog).with_label("Close").build();
        for b in [&reset, &report, &trace] {
            buttons.add(b, 0, SizerFlag::Right, 6);
        }
        buttons.add_stretch_spacer(1);
        buttons.add(&close, 0, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&intro, 0, SizerFlag::Expand | SizerFlag::All, 10);
        sizer.add_sizer(&top, 0, SizerFlag::Left | SizerFlag::Right, 10);
        sizer.add(&live, 0, SizerFlag::Expand | SizerFlag::All, 10);
        sizer.add(&recent, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 10);
        sizer.add(&hot, 1, SizerFlag::Expand | SizerFlag::All, 10);
        sizer.add(&paths, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 10);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::All, 10);
        dialog.set_sizer_and_fit(sizer, true);

        let timer = Timer::new(&dialog);
        {
            let g = gui.clone();
            timer.on_tick(move |_| with(&g, Gui::refresh_profiler));
        }
        {
            let g = gui.clone();
            toggle.on_click(move |_| with(&g, Gui::toggle_profiler));
        }
        {
            let g = gui.clone();
            reset.on_click(move |_| {
                with(&g, |gui| {
                    let _ = gui.run("profiler.reset", Value::Null);
                    if let Some(w) = &mut gui.profiler {
                        w.slow_seen = 0;
                    }
                    gui.refresh_profiler();
                });
            });
        }
        {
            let g = gui.clone();
            report.on_click(move |_| with(&g, |gui| gui.export_profile(false)));
        }
        {
            let g = gui.clone();
            trace.on_click(move |_| with(&g, |gui| gui.export_profile(true)));
        }
        {
            let g = gui.clone();
            close.on_click(move |_| with(&g, Gui::close_profiler));
        }
        {
            let g = gui.clone();
            dialog.on_close(move |_| with(&g, Gui::close_profiler));
        }
        timer.start(1000, false);
        dialog.show(true);
        self.profiler = Some(ProfilerWindow { dialog, timer, toggle, slow, live, recent, hot, paths, slow_seen: 0 });
        self.refresh_profiler();
    }

    fn close_profiler(&mut self) {
        if let Some(w) = self.profiler.take() {
            w.timer.stop();
            w.dialog.destroy();
        }
    }

    fn toggle_profiler(&mut self) {
        let frame = self.frame;
        let running = self.run("profiler.status", Value::Null).ok().is_some_and(|s| s["enabled"] == true);
        let r = if running {
            self.run("profiler.stop", Value::Null)
        } else {
            let slow = self.profiler.as_ref().and_then(|w| w.slow.get_value().trim().parse::<f64>().ok()).unwrap_or(100.0);
            if let Some(w) = &mut self.profiler {
                w.slow_seen = 0;
            }
            self.run("profiler.start", json!({ "slow_ms": slow }))
        };
        if let Err(e) = r {
            error(&frame, &format!("The profiler did not change:\n{e}"));
        }
        self.refresh_profiler();
    }

    /// The window's figures again (every second while it is open).
    fn refresh_profiler(&mut self) {
        if self.profiler.is_none() {
            return;
        }
        let live = self.run("profiler.live", Value::Null).unwrap_or_default();
        let report = self.run("profiler.report", json!({ "top": 40 })).unwrap_or_default();
        let frame = self.frame;
        let Some(w) = &mut self.profiler else { return };
        let on = live["enabled"] == true;
        w.toggle.set_label(if on { "Stop" } else { "Start" });
        let mut parts = vec![if on { format!("Recording for {:.0} s.", live["seconds"].as_f64().unwrap_or(0.0)) } else { "Stopped.".to_owned() }];
        if let Some(f) = live["frame_ms"].as_f64() {
            // Frames per second only while frames are drawn (an idle view draws none).
            let fps = live["fps"].as_f64().filter(|x| *x > 0.0).map_or_else(|| ", idle".to_owned(), |x| format!(", {x:.0} fps"));
            parts.push(format!("Frames: {f:.1} ms mean, {:.1} ms worst{fps}.", live["worst_frame_ms"].as_f64().unwrap_or(0.0)));
        }
        if let Some(m) = live["memory_mb"].as_f64() {
            parts.push(format!("Memory {m:.0} MB."));
        }
        if let Some(n) = live["elements"].as_u64() {
            parts.push(format!("Model: {n} elements."));
        }
        let slow_count = live["slow_count"].as_u64().unwrap_or(0) as usize;
        parts.push(format!("{slow_count} slow operation(s)."));
        w.live.set_label(&parts.join("  "));
        w.live.wrap(900);
        let recent: Vec<Vec<String>> = live["recent"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| vec![e["name"].as_str().unwrap_or("").to_owned(), ms(&json!(e["dur_us"].as_f64().unwrap_or(0.0) / 1000.0)), e["detail"].as_str().unwrap_or("").to_owned()])
            .collect();
        fill(&w.recent, &recent);
        let hot: Vec<Vec<String>> = report["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| vec![r["name"].as_str().unwrap_or("").to_owned(), r["calls"].to_string(), ms(&r["self_ms"]), ms(&r["total_ms"]), ms(&r["p95_ms"]), ms(&r["max_ms"])])
            .collect();
        fill(&w.hot, &hot);
        let paths: Vec<Vec<String>> = report["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| vec![r["path"].as_str().unwrap_or("").to_owned(), r["calls"].to_string(), ms(&r["self_ms"]), ms(&r["total_ms"])])
            .collect();
        fill(&w.paths, &paths);
        // A new slow operation: on the status bar.
        if slow_count > w.slow_seen {
            w.slow_seen = slow_count;
            if let Some(e) = live["slow"].as_array().and_then(|a| a.first()) {
                let what = e["detail"].as_str().filter(|d| !d.is_empty()).map_or_else(|| e["name"].as_str().unwrap_or("").to_owned(), |d| format!("{} ({d})", e["name"].as_str().unwrap_or("")));
                frame.set_status_text(&format!("Slow: {what} took {:.0} ms", e["dur_us"].as_f64().unwrap_or(0.0) / 1000.0), 0);
            }
        }
    }

    fn export_profile(&mut self, trace: bool) {
        let frame = self.frame;
        let (title, wildcard, file, cmd) = if trace {
            ("Export Trace", "Trace files (*.json)|*.json", "reviera-trace.json", "profiler.export_trace")
        } else {
            ("Export Report", "Markdown files (*.md)|*.md", "reviera-performance.md", "profiler.export_report")
        };
        let parent = self.profiler.as_ref().map(|w| w.dialog);
        let dialog = match &parent {
            Some(p) => FileDialog::builder(p),
            None => FileDialog::builder(&frame),
        }
        .with_message(title)
        .with_wildcard(wildcard)
        .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
        .with_default_file(file)
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else { return };
        match self.run(cmd, json!({ "path": path })) {
            Ok(o) => frame.set_status_text(&format!("Wrote {path} ({} bytes)", o["bytes"]), 0),
            Err(e) => error(&frame, &format!("Not written:\n{e}")),
        }
    }
}
