// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Clash Detection pane (COL-CLASH-001, COL-CLASH-002): runs a clash test of set A against
//! set B out of process, follows its progress, collects its results into the model, and
//! navigates them: each clash selected and framed in 3D, previous and next, isolated or boxed,
//! given a status and a comment, resolved with the overlap tools, and exported.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, with};

const FILTERS: [(&str, &[&str]); 7] = [
    ("Open (new and active)", &["new", "active"]),
    ("All", &[]),
    ("New", &["new"]),
    ("Active", &["active"]),
    ("Reviewed", &["reviewed"]),
    ("Approved", &["approved"]),
    ("Resolved", &["resolved"]),
];

const STATUSES: [&str; 5] = ["new", "active", "reviewed", "approved", "resolved"];

pub(crate) struct ClashPane {
    list: ListCtrl,
    filter: Choice,
    status: Choice,
    comment: TextCtrl,
    info: StaticText,
    gauge: Gauge,
    timer: Timer<Frame>,
    /// The results shown, in list order.
    items: Vec<Value>,
    /// The clash job being followed.
    job: Option<String>,
    /// The result shown in the view.
    current: Option<usize>,
    /// Isolate is on: each clash shown is isolated in turn.
    isolating: bool,
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn status_colour(status: &str) -> Colour {
    match status {
        "new" => Colour::new(200, 40, 40, 255),
        "active" => Colour::new(200, 110, 0, 255),
        "reviewed" => Colour::new(40, 90, 190, 255),
        "approved" => Colour::new(30, 140, 60, 255),
        _ => Colour::new(140, 140, 140, 255),
    }
}

fn button(panel: &Panel, label: &str, tip: &str) -> Button {
    let b = Button::builder(panel).with_label(label).build();
    b.set_tooltip(tip);
    b
}

impl Gui {
    /// Shows or hides the Clash Detection pane, building it the first time.
    pub(crate) fn toggle_clash_pane(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().clash_pane.is_some() {
            with(gui, |g| {
                g.toggle_pane("clash");
                g.refresh_clash_list();
            });
            return;
        }
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let run = button(
            &panel,
            "Run Test…",
            "Choose set A and set B and test them against each other, out of process",
        );
        let refresh = button(&panel, "Run Again", "Run the last clash test again");
        let export = button(&panel, "Export…", "Write the clash report as CSV or HTML");
        let gauge = Gauge::builder(&panel).with_range(100).build();
        let info = StaticText::builder(&panel)
            .with_label("No clash test has run yet.")
            .build();
        let filter = Choice::builder(&panel).build();
        for (label, _) in FILTERS {
            filter.append(label);
        }
        filter.set_selection(0);
        let list = ListCtrl::builder(&panel)
            .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
            .build();
        for (k, (heading, width)) in [
            ("#", 36),
            ("Status", 72),
            ("Element A", 150),
            ("Element B", 150),
            ("Group", 140),
            ("Volume", 72),
            ("Depth", 64),
        ]
        .into_iter()
        .enumerate()
        {
            let format = if k >= 5 {
                ListColumnFormat::Right
            } else {
                ListColumnFormat::Left
            };
            list.insert_column(k as i64, heading, format, width);
        }
        let prev = button(&panel, "◀ Previous", "Show the previous clash");
        let next = button(&panel, "Next ▶", "Show the next clash");
        let isolate = button(
            &panel,
            "Isolate",
            "Show only the two elements of this clash",
        );
        let section = button(&panel, "Section Box", "Box the 3D view around this clash");
        let reset = button(&panel, "Reset View", "Undo Isolate in the view");
        let status = Choice::builder(&panel).build();
        for s in STATUSES {
            status.append(&capitalised(s));
        }
        let comment = TextCtrl::builder(&panel).build();
        comment.set_tooltip("A comment kept with the clash, with your name and the time");
        let apply = button(
            &panel,
            "Apply",
            "Set this clash's status, and add the comment",
        );
        let resolve = button(
            &panel,
            "Resolve…",
            "Resolve the clash with the overlap tools: cut, join, move apart or accept",
        );

        // The list on the left, taking the pane's height; the controls beside it.
        let left = BoxSizer::builder(Orientation::Vertical).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&run, 1, SizerFlag::Right, 2);
        row.add(&refresh, 1, SizerFlag::Right, 2);
        row.add(&export, 1, SizerFlag::Right, 8);
        let show = StaticText::builder(&panel).with_label("Show").build();
        row.add(
            &show,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            4,
        );
        row.add(&filter, 1, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(
            &gauge,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            8,
        );
        row.add(&info, 2, SizerFlag::AlignCenterVertical, 0);
        left.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        left.add(&list, 1, SizerFlag::Expand, 0);
        let right = BoxSizer::builder(Orientation::Vertical).build();
        for (x, y) in [(&prev, &next), (&isolate, &section), (&reset, &resolve)] {
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            row.add(x, 1, SizerFlag::Right, 2);
            row.add(y, 1, SizerFlag::empty(), 0);
            right.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 3);
        }
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&status, 1, SizerFlag::Right, 2);
        row.add(&apply, 1, SizerFlag::empty(), 0);
        right.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 3);
        right.add(&comment, 0, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Horizontal).build();
        sizer.add_sizer(&left, 1, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(&right, 0, SizerFlag::Expand | SizerFlag::All, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("clash")
                .with_caption("Clash Detection")
                .bottom()
                .layer(1)
                .best_size(1000, 330)
                .min_size(400, 200)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        let timer = Timer::new(&frame);
        {
            let g = gui.clone();
            timer.on_tick(move |_| with(&g, Gui::poll_clash_job));
        }
        gui.borrow_mut().clash_pane = Some(ClashPane {
            list,
            filter,
            status,
            comment,
            info,
            gauge,
            timer,
            items: Vec::new(),
            job: None,
            current: None,
            isolating: false,
        });
        {
            let g = gui.clone();
            run.on_click(move |_| with(&g, Gui::run_clash_test));
        }
        {
            let g = gui.clone();
            refresh.on_click(move |_| with(&g, |g| g.start_clash_job("clash.refresh", json!({}))));
        }
        {
            let g = gui.clone();
            export.on_click(move |_| with(&g, Gui::export_clash_report));
        }
        {
            let g = gui.clone();
            filter.on_selection_changed(move |_| with(&g, Gui::refresh_clash_list));
        }
        {
            let g = gui.clone();
            list.on_item_selected(move |e| {
                let i = e.get_item_index();
                with(&g, |g| g.show_clash(i.max(0) as usize));
            });
        }
        {
            let g = gui.clone();
            prev.on_click(move |_| with(&g, |g| g.step_clash(-1)));
        }
        {
            let g = gui.clone();
            next.on_click(move |_| with(&g, |g| g.step_clash(1)));
        }
        {
            let g = gui.clone();
            isolate.on_click(move |_| {
                with(&g, |g| {
                    if let Some(p) = g.clash_pane.as_mut() {
                        p.isolating = true;
                    }
                    g.temp_action("inspect.isolate");
                })
            });
        }
        {
            let g = gui.clone();
            section.on_click(move |_| with(&g, Gui::selection_box));
        }
        {
            let g = gui.clone();
            reset.on_click(move |_| {
                with(&g, |g| {
                    if let Some(p) = g.clash_pane.as_mut() {
                        p.isolating = false;
                    }
                    g.temp_reset();
                })
            });
        }
        {
            let g = gui.clone();
            apply.on_click(move |_| with(&g, Gui::apply_clash_status));
        }
        {
            let g = gui.clone();
            resolve.on_click(move |_| with(&g, Gui::resolve_clash));
        }
        with(gui, Gui::refresh_clash_list);
    }

    /// Lists the model's clash results under the chosen filter.
    pub(crate) fn refresh_clash_list(&mut self) {
        let report = self.run("clash.last_report", json!({})).ok();
        let Some(p) = self.clash_pane.as_mut() else {
            return;
        };
        let wanted = FILTERS[p.filter.get_selection().unwrap_or(0) as usize].1;
        p.items = report
            .as_ref()
            .and_then(|r| r["items"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|i| wanted.is_empty() || wanted.contains(&i["status"].as_str().unwrap_or("")))
            .collect();
        p.list.delete_all_items();
        for (k, i) in p.items.iter().enumerate() {
            let status = i["status"].as_str().unwrap_or_default();
            let volume = i["volume"].as_f64().unwrap_or(0.0);
            p.list.insert_item(k as i64, &(k + 1).to_string(), None);
            let cells = [
                capitalised(status),
                i["a_label"].as_str().unwrap_or_default().to_owned(),
                i["b_label"].as_str().unwrap_or_default().to_owned(),
                i["group"].as_str().unwrap_or_default().to_owned(),
                if volume < 0.01 {
                    format!("{:.2} L", volume * 1000.0)
                } else {
                    format!("{volume:.3} m³")
                },
                format!("{:.0} mm", i["depth"].as_f64().unwrap_or(0.0) * 1000.0),
            ];
            for (c, text) in cells.iter().enumerate() {
                p.list.set_item_text_by_column(k as i64, c as i32 + 1, text);
            }
            p.list
                .set_item_text_colour(k as i64, &status_colour(status));
        }
        p.current = None;
        if p.job.is_none() {
            let text = match report
                .as_ref()
                .and_then(|r| r.get("test"))
                .filter(|t| !t.is_null())
            {
                Some(t) => {
                    let counts = &report.as_ref().unwrap()["counts"];
                    let n = |k: &str| counts[k].as_u64().unwrap_or(0);
                    format!(
                        "Run {} ({} bodies): {} open, {} reviewed, {} approved, {} resolved.",
                        t["runs"],
                        t["bodies"],
                        n("new") + n("active"),
                        n("reviewed"),
                        n("approved"),
                        n("resolved")
                    )
                }
                None => "No clash test has run in this model yet.".to_owned(),
            };
            p.info.set_label(&text);
            p.gauge.set_value(0);
        }
    }

    /// Asks for set A and set B and starts the test.
    fn run_clash_test(&mut self) {
        let frame = self.frame;
        let Ok(bodies) = self.app.bodies() else {
            return self.frame.set_status_text("Open a model first", 0);
        };
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let mut classes: Vec<String> = bodies
            .iter()
            .filter_map(|b| store.get(b.element).map(|e| e.class.clone()))
            .collect();
        classes.sort();
        classes.dedup();
        let labels: Vec<&str> = classes.iter().map(String::as_str).collect();
        let mut sets = Vec::new();
        for (name, hint) in [
            ("Set A", "Elements of set A (tick none for every element):"),
            (
                "Set B",
                "Tested against set B (tick none for every element):",
            ),
        ] {
            let d =
                MultiChoiceDialog::builder(&frame, hint, &format!("Clash Test: {name}"), &labels)
                    .build();
            if d.show_modal() != ID_OK {
                return;
            }
            let picked: Vec<String> = d
                .get_selections()
                .into_iter()
                .filter_map(|i| classes.get(i as usize).cloned())
                .collect();
            sets.push(picked);
        }
        self.start_clash_job(
            "clash.run_basic",
            json!({ "set_a": { "classes": sets[0] }, "set_b": { "classes": sets[1] } }),
        );
    }

    /// Starts a clash job (`clash.run_basic` or `clash.refresh`) and follows it.
    fn start_clash_job(&mut self, command: &str, args: Value) {
        if self.clash_pane.as_ref().is_some_and(|p| p.job.is_some()) {
            return self
                .frame
                .set_status_text("A clash test is already running", 0);
        }
        match self.run(command, args) {
            Ok(v) => {
                let id = v["job_id"].as_str().unwrap_or_default().to_owned();
                if let Some(p) = self.clash_pane.as_mut() {
                    p.info.set_label(&format!(
                        "Testing {} elements out of process…",
                        v["elements"]
                    ));
                    p.gauge.set_value(0);
                    p.job = Some(id);
                    p.timer.start(200, false);
                }
            }
            Err(e) => error(&self.frame, &format!("The clash test did not start:\n{e}")),
        }
    }

    /// Follows the running clash job; collects it when it finishes.
    fn poll_clash_job(&mut self) {
        let Some(id) = self.clash_pane.as_ref().and_then(|p| p.job.clone()) else {
            if let Some(p) = &self.clash_pane {
                p.timer.stop();
            }
            return;
        };
        let Ok(s) = self.run("compute.status", json!({ "job_id": id })) else {
            return;
        };
        let state = s["state"].as_str().unwrap_or_default().to_owned();
        let progress = (s["progress"].as_f64().unwrap_or(0.0) * 100.0).round() as i32;
        if let Some(p) = &self.clash_pane {
            p.gauge.set_value(progress);
            if !matches!(state.as_str(), "succeeded" | "failed" | "cancelled") {
                p.info.set_label(&format!(
                    "Clash test {state}: {}",
                    s["phase"].as_str().unwrap_or_default()
                ));
                return;
            }
            p.timer.stop();
        }
        if let Some(p) = self.clash_pane.as_mut() {
            p.job = None;
        }
        if state != "succeeded" {
            let why = s["failure"]["message"]
                .as_str()
                .unwrap_or(&state)
                .to_owned();
            if let Some(p) = &self.clash_pane {
                p.info.set_label(&format!("The clash test {state}: {why}"));
            }
            return;
        }
        match self.run("clash.collect", json!({ "job_id": id })) {
            Ok(r) => {
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.refresh_clash_list();
                self.frame.set_status_text(
                    &format!(
                        "Clash test run {}: {} new, {} found again, {} resolved",
                        r["run"], r["new"], r["found_again"], r["resolved"]
                    ),
                    0,
                );
            }
            Err(e) => error(
                &self.frame,
                &format!("Collecting the clash test failed:\n{e}"),
            ),
        }
    }

    /// Selects and frames clash `i` of the list.
    fn show_clash(&mut self, i: usize) {
        let Some(item) = self
            .clash_pane
            .as_ref()
            .and_then(|p| p.items.get(i).cloned())
        else {
            return;
        };
        if let Some(p) = self.clash_pane.as_mut() {
            p.current = Some(i);
            if let Some(k) = STATUSES
                .iter()
                .position(|s| *s == item["status"].as_str().unwrap_or_default())
            {
                p.status.set_selection(k as u32);
            }
        }
        let ids: Vec<Value> = [item["a_id"].clone(), item["b_id"].clone()]
            .into_iter()
            .filter(Value::is_u64)
            .collect();
        if ids.is_empty() {
            return self
                .frame
                .set_status_text("Both elements of this clash have been deleted", 0);
        }
        let _ = self.run("selection.set", json!({ "ids": ids }));
        self.selection_changed();
        // While isolating, the new pair is isolated in its turn.
        if self.clash_pane.as_ref().is_some_and(|p| p.isolating) {
            self.temp_reset();
            self.temp_action("inspect.isolate");
        }
        let corner = |k: &str| -> Option<[f64; 3]> {
            let v = item[k].as_array()?;
            Some([
                v.first()?.as_f64()?,
                v.get(1)?.as_f64()?,
                v.get(2)?.as_f64()?,
            ])
        };
        match (corner("min"), corner("max")) {
            (Some(lo), Some(hi)) => self.zoom_to_box(lo, hi),
            _ => self.zoom_to_fit(),
        }
        let comments = item["comments"].as_array().map(|c| c.len()).unwrap_or(0);
        self.frame.set_status_text(
            &format!(
                "Clash {}: {} × {} — {}{}",
                i + 1,
                item["a_label"].as_str().unwrap_or_default(),
                item["b_label"].as_str().unwrap_or_default(),
                item["status"].as_str().unwrap_or_default(),
                if comments > 0 {
                    format!(", {comments} comment(s)")
                } else {
                    String::new()
                }
            ),
            0,
        );
    }

    /// Shows the previous (-1) or next (+1) clash.
    fn step_clash(&mut self, by: i64) {
        let Some(p) = &self.clash_pane else { return };
        let n = p.items.len() as i64;
        if n == 0 {
            return;
        }
        let at = p.current.map_or(if by > 0 { -1 } else { n }, |c| c as i64);
        let to = (at + by).rem_euclid(n);
        p.list
            .set_item_state(to, ListItemState::Selected, ListItemState::Selected);
        p.list.ensure_visible(to);
        // Selecting the row shows it (on_item_selected); make sure even if it was selected.
        self.show_clash(to as usize);
    }

    fn current_clash(&self) -> Option<Value> {
        let p = self.clash_pane.as_ref()?;
        p.current.and_then(|i| p.items.get(i).cloned())
    }

    /// Sets the shown clash's status and adds the comment.
    fn apply_clash_status(&mut self) {
        let Some(item) = self.current_clash() else {
            return self.frame.set_status_text("Pick a clash first", 0);
        };
        let (status, comment) = {
            let p = self.clash_pane.as_ref().expect("built");
            (
                p.status.get_selection().map(|k| STATUSES[k as usize]),
                p.comment.get_value(),
            )
        };
        let args = json!({
            "result_ids": [item["id"]],
            "status": status,
            "comment": (!comment.trim().is_empty()).then_some(comment),
        });
        match self.run("clash.set_status", args) {
            Ok(_) => {
                let frame = self.frame;
                self.refresh_scene(&frame);
                let at = self.clash_pane.as_ref().and_then(|p| p.current);
                if let Some(p) = &self.clash_pane {
                    p.comment.set_value("");
                }
                self.refresh_clash_list();
                if let Some(i) = at {
                    let n = self.clash_pane.as_ref().map_or(0, |p| p.items.len());
                    if n > 0 {
                        self.step_clash_to(i.min(n - 1));
                    }
                }
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn step_clash_to(&mut self, i: usize) {
        if let Some(p) = &self.clash_pane {
            p.list
                .set_item_state(i as i64, ListItemState::Selected, ListItemState::Selected);
            p.list.ensure_visible(i as i64);
        }
        self.show_clash(i);
    }

    /// Resolves the shown clash with the overlap tools (ARC-MOD-032).
    fn resolve_clash(&mut self) {
        let Some(item) = self.current_clash() else {
            return self.frame.set_status_text("Pick a clash first", 0);
        };
        let (a, b) = (item["a_id"].clone(), item["b_id"].clone());
        if !a.is_u64() || !b.is_u64() {
            return self
                .frame
                .set_status_text("An element of this clash has been deleted", 0);
        }
        let (al, bl) = (
            item["a_label"].as_str().unwrap_or_default().to_owned(),
            item["b_label"].as_str().unwrap_or_default().to_owned(),
        );
        let actions: Vec<(String, &str)> = vec![
            ("Accept as intended".to_owned(), "accept"),
            (format!("Cut {al} by {bl}"), "cut_a"),
            (format!("Cut {bl} by {al}"), "cut_b"),
            ("Join (by precedence)".to_owned(), "join"),
            (format!("Move {bl} apart"), "move_b"),
            (format!("Move {al} apart"), "move_a"),
        ];
        let labels: Vec<&str> = actions.iter().map(|x| x.0.as_str()).collect();
        let d = SingleChoiceDialog::builder(
            &self.frame,
            &format!("{al} × {bl}:"),
            "Resolve Clash",
            &labels,
        )
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(action) = usize::try_from(d.get_selection())
            .ok()
            .and_then(|i| actions.get(i))
            .map(|x| x.1)
        else {
            return;
        };
        if self.resolve_overlap(&a, &b, &al, &bl, action) {
            // Recorded on the clash: the next run will find it resolved.
            let _ = self.run(
                "clash.set_status",
                json!({ "result_ids": [item["id"]], "comment": format!("Resolved in Reviera: {}", actions.iter().find(|x| x.1 == action).map_or("", |x| x.0.as_str())) }),
            );
            self.refresh_clash_list();
        }
    }

    fn export_clash_report(&mut self) {
        let dialog = FileDialog::builder(&self.frame)
            .with_message("Export Clash Report")
            .with_wildcard("CSV files (*.csv)|*.csv|HTML pages (*.html)|*.html")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file("clash report.csv")
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        let format = if dialog.get_filter_index() == 1 || path.ends_with(".html") {
            "html"
        } else {
            "csv"
        };
        match self.run(
            "clash.export_report",
            json!({ "path": path, "format": format }),
        ) {
            Ok(n) => self
                .frame
                .set_status_text(&format!("Exported {n} clash result(s) to {path}"), 0),
            Err(e) => error(&self.frame, &e),
        }
    }
}

fn error(frame: &Frame, message: &str) {
    super::error(frame, message);
}
