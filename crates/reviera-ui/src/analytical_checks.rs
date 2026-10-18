// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Analytical Checks pane (STR-AM-007): Analyze › Checks › Check Supports and Check
//! Consistency run over the whole model or the selection; the issues are listed by severity with
//! the measured distance against its tolerance, and each can be selected, framed, isolated or
//! ignored with a reason. While the pane is open, issue locations are marked in 3D views. With
//! automatic checks on in Structural Settings, the checks run after each change and new issues are
//! reported.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error, with};

const SEVERITIES: [(&str, &[&str]); 5] = [
    ("All", &[]),
    ("Errors", &["error"]),
    ("Warnings", &["warning"]),
    ("Hints", &["hint"]),
    ("Information", &["info"]),
];

const MARK: [f32; 4] = [0.9, 0.1, 0.8, 1.0];
const MARK_CURRENT: [f32; 4] = [1.0, 0.85, 0.0, 1.0];

pub(crate) struct ChecksPane {
    list: ListCtrl,
    scope: Choice,
    filter: Choice,
    show_ignored: CheckBox,
    reason: TextCtrl,
    info: StaticText,
    /// The last report and the checks it ran (supports, consistency).
    report: Option<Value>,
    last: (bool, bool),
    /// The issues shown, in list order.
    items: Vec<Value>,
    current: Option<usize>,
}

fn button(panel: &Panel, label: &str, tip: &str) -> Button {
    let b = Button::builder(panel).with_label(label).build();
    b.set_tooltip(tip);
    b
}

fn colour(severity: &str) -> Colour {
    match severity {
        "error" => Colour::new(200, 30, 30, 255),
        "warning" => Colour::new(190, 110, 0, 255),
        "hint" => Colour::new(40, 90, 190, 255),
        _ => Colour::new(110, 110, 110, 255),
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn corner(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ])
}

impl Gui {
    /// Shows or hides the Analytical Checks pane, building it the first time.
    pub(crate) fn toggle_checks_pane(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().checks_pane.is_some() {
            with(gui, |g| {
                g.toggle_pane("analytical_checks");
                g.refresh_check_marks();
            });
            return;
        }
        Gui::build_checks_pane(gui);
    }

    fn build_checks_pane(gui: &Rc<RefCell<Gui>>) {
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let scope = Choice::builder(&panel).build();
        scope.append("Whole model");
        scope.append("Selection");
        scope.set_selection(0);
        scope.set_tooltip("Check the whole model, or only issues touching the selected elements");
        let supports = button(
            &panel,
            "Check Supports",
            "S1 unsupported members, S2 circular support, S3 no supports",
        );
        let consistency = button(
            &panel,
            "Check Consistency",
            "C1–C8: disconnected nodes, isolated parts, physical/analytical mismatch, counterparts, \
             instability hints, missing properties (criteria in Structural Settings)",
        );
        let recheck = button(&panel, "Re-check", "Run the last check again");
        let filter = Choice::builder(&panel).build();
        for (label, _) in SEVERITIES {
            filter.append(label);
        }
        filter.set_selection(0);
        let show_ignored = CheckBox::builder(&panel).with_label("Show ignored").build();
        let export = button(&panel, "Export CSV…", "Write the issues as CSV");
        let info = StaticText::builder(&panel)
            .with_label("No check has run yet.")
            .build();
        let list = ListCtrl::builder(&panel)
            .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
            .build();
        for (k, (heading, width)) in [
            ("Severity", 70),
            ("Code", 44),
            ("Issue", 170),
            ("Message", 520),
            ("Measured", 110),
        ]
        .into_iter()
        .enumerate()
        {
            list.insert_column(k as i64, heading, ListColumnFormat::Left, width);
        }
        let select = button(&panel, "Select", "Select the elements of this issue");
        let zoom = button(&panel, "Zoom To", "Frame this issue in the 3D view");
        let isolate = button(
            &panel,
            "Isolate",
            "Show only this issue's elements in the view",
        );
        let reset = button(&panel, "Reset View", "Undo Isolate in the view");
        let reason = TextCtrl::builder(&panel).build();
        reason.set_tooltip("Why the issue is ignored (optional)");
        let ignore = button(
            &panel,
            "Ignore",
            "Ignore this issue until one of its elements changes geometry",
        );
        let unignore = button(&panel, "Un-ignore", "Report this issue again");

        let left = BoxSizer::builder(Orientation::Vertical).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&scope, 0, SizerFlag::Right, 4);
        row.add(&supports, 0, SizerFlag::Right, 2);
        row.add(&consistency, 0, SizerFlag::Right, 2);
        row.add(&recheck, 0, SizerFlag::Right, 8);
        row.add(&filter, 0, SizerFlag::Right, 4);
        row.add(
            &show_ignored,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            8,
        );
        row.add(&export, 0, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        left.add(&info, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        left.add(&list, 1, SizerFlag::Expand, 0);
        let right = BoxSizer::builder(Orientation::Vertical).build();
        for (x, y) in [(&select, &zoom), (&isolate, &reset)] {
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            row.add(x, 1, SizerFlag::Right, 2);
            row.add(y, 1, SizerFlag::empty(), 0);
            right.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 3);
        }
        let label = StaticText::builder(&panel).with_label("Reason").build();
        right.add(&label, 0, SizerFlag::Top, 6);
        right.add(&reason, 0, SizerFlag::Expand | SizerFlag::Bottom, 3);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&ignore, 1, SizerFlag::Right, 2);
        row.add(&unignore, 1, SizerFlag::empty(), 0);
        right.add_sizer(&row, 0, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Horizontal).build();
        sizer.add_sizer(&left, 1, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(&right, 0, SizerFlag::Expand | SizerFlag::All, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("analytical_checks")
                .with_caption("Analytical Checks")
                .bottom()
                .layer(1)
                .best_size(1100, 300)
                .min_size(400, 180)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        gui.borrow_mut().checks_pane = Some(ChecksPane {
            list,
            scope,
            filter,
            show_ignored,
            reason,
            info,
            report: None,
            last: (true, true),
            items: Vec::new(),
            current: None,
        });
        {
            let g = gui.clone();
            supports.on_click(move |_| with(&g, |g| g.analytical_check(true, false)));
        }
        {
            let g = gui.clone();
            consistency.on_click(move |_| with(&g, |g| g.analytical_check(false, true)));
        }
        {
            let g = gui.clone();
            recheck.on_click(move |_| {
                with(&g, |g| {
                    let (s, c) = g.checks_pane.as_ref().map_or((true, true), |p| p.last);
                    g.analytical_check(s, c);
                })
            });
        }
        {
            let g = gui.clone();
            filter.on_selection_changed(move |_| with(&g, Gui::fill_check_list));
        }
        {
            let g = gui.clone();
            show_ignored.on_toggled(move |_| with(&g, Gui::fill_check_list));
        }
        {
            let g = gui.clone();
            export.on_click(move |_| with(&g, Gui::export_checks));
        }
        {
            let g = gui.clone();
            list.on_item_selected(move |e| {
                let i = e.get_item_index();
                with(&g, |g| g.show_check_issue(i.max(0) as usize, false));
            });
        }
        {
            let g = gui.clone();
            select.on_click(move |_| {
                with(&g, |g| {
                    if let Some(i) = g.checks_pane.as_ref().and_then(|p| p.current) {
                        g.show_check_issue(i, false);
                    }
                })
            });
        }
        {
            let g = gui.clone();
            zoom.on_click(move |_| {
                with(&g, |g| {
                    if let Some(i) = g.checks_pane.as_ref().and_then(|p| p.current) {
                        g.show_check_issue(i, true);
                    }
                })
            });
        }
        {
            let g = gui.clone();
            isolate.on_click(move |_| with(&g, |g| g.temp_action("inspect.isolate")));
        }
        {
            let g = gui.clone();
            reset.on_click(move |_| with(&g, Gui::temp_reset));
        }
        {
            let g = gui.clone();
            ignore.on_click(move |_| with(&g, |g| g.ignore_check_issue(true)));
        }
        {
            let g = gui.clone();
            unignore.on_click(move |_| with(&g, |g| g.ignore_check_issue(false)));
        }
    }

    /// Analyze › Checks › Check Supports / Check Consistency: opens the pane and runs the check.
    pub(crate) fn run_analytical_check(gui: &Rc<RefCell<Gui>>, supports: bool, consistency: bool) {
        if gui.borrow().checks_pane.is_none() {
            Gui::build_checks_pane(gui);
        } else {
            with(gui, |g| {
                if !g.aui.is_pane_shown("analytical_checks") {
                    g.toggle_pane("analytical_checks");
                }
            });
        }
        with(gui, |g| g.analytical_check(supports, consistency));
    }

    /// Runs a check over the chosen scope and lists its issues.
    fn analytical_check(&mut self, supports: bool, consistency: bool) {
        let selection = self.selection();
        let Some(p) = self.checks_pane.as_mut() else {
            return;
        };
        p.last = (supports, consistency);
        let scope = if p.scope.get_selection() == Some(1) {
            if selection.is_empty() {
                return self
                    .frame
                    .set_status_text("Select elements to check, or check the whole model", 0);
            }
            json!(selection)
        } else {
            json!("model")
        };
        let command = match (supports, consistency) {
            (true, false) => "analytical_model.check_supports",
            (false, true) => "analytical_model.check_consistency",
            _ => "analytical_model.check",
        };
        match self.run(command, json!({ "scope": scope })) {
            Ok(r) => {
                if let Some(p) = self.checks_pane.as_mut() {
                    p.report = Some(r);
                }
                // The issues are drawn over the analytical model.
                if !self.show_analytical {
                    self.toggle_analytical();
                }
                self.fill_check_list();
            }
            Err(e) => error(&self.frame, &format!("The check did not run:\n{e}")),
        }
    }

    /// Lists the last report's issues under the severity filter.
    fn fill_check_list(&mut self) {
        let Some(p) = self.checks_pane.as_mut() else {
            return;
        };
        let wanted = SEVERITIES[p.filter.get_selection().unwrap_or(0) as usize].1;
        let ignored = p.show_ignored.is_checked();
        let Some(r) = p.report.clone() else {
            return;
        };
        p.items = r["issues"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|i| wanted.is_empty() || wanted.contains(&i["severity"].as_str().unwrap_or("")))
            .filter(|i| ignored || i["ignored"] != true)
            .collect();
        p.list.delete_all_items();
        for (k, i) in p.items.iter().enumerate() {
            let severity = i["severity"].as_str().unwrap_or_default();
            let measured = match (i["measured"].as_f64(), i["tolerance"].as_f64()) {
                (Some(m), Some(t)) => format!("{:.0} / {:.0} mm", m * 1000.0, t * 1000.0),
                (Some(m), None) => format!("{:.0} mm", m * 1000.0),
                _ => String::new(),
            };
            let mut title = i["title"].as_str().unwrap_or_default().to_owned();
            if i["ignored"] == true {
                title.push_str(" (ignored)");
            }
            p.list.insert_item(k as i64, &capitalised(severity), None);
            let cells = [
                i["code"].as_str().unwrap_or_default().to_owned(),
                title,
                i["message"].as_str().unwrap_or_default().to_owned(),
                measured,
            ];
            for (c, text) in cells.iter().enumerate() {
                p.list.set_item_text_by_column(k as i64, c as i32 + 1, text);
            }
            p.list.set_item_text_colour(k as i64, &colour(severity));
        }
        p.current = None;
        let counts = &r["counts_by_severity"];
        let n = |k: &str| counts[k].as_u64().unwrap_or(0);
        let mut text = format!(
            "{} error(s), {} warning(s), {} hint(s), {} for information; {} ignored. Tolerances: \
             support {:.0} mm, analytical-to-physical {:.0} mm, node merge {:.0} mm.",
            n("error"),
            n("warning"),
            n("hint"),
            n("info"),
            r["ignored"].as_u64().unwrap_or(0),
            r["tolerances"]["support_distance"].as_f64().unwrap_or(0.0) * 1000.0,
            r["tolerances"]["analytical_distance"]
                .as_f64()
                .unwrap_or(0.0)
                * 1000.0,
            r["tolerances"]["node_merge_distance"]
                .as_f64()
                .unwrap_or(0.0)
                * 1000.0,
        );
        for nc in r["not_checked"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                " Not checked: {}.",
                nc.as_str().unwrap_or_default()
            ));
        }
        p.info.set_label(&text);
        self.refresh_check_marks();
    }

    /// Selects issue `i` (and, with `zoom`, frames it).
    fn show_check_issue(&mut self, i: usize, zoom: bool) {
        let Some(item) = self
            .checks_pane
            .as_ref()
            .and_then(|p| p.items.get(i).cloned())
        else {
            return;
        };
        if let Some(p) = self.checks_pane.as_mut() {
            p.current = Some(i);
            p.reason
                .set_value(item["ignore_reason"].as_str().unwrap_or_default());
        }
        let ids: Vec<Value> = item["element_ids"].as_array().cloned().unwrap_or_default();
        if !ids.is_empty() {
            let _ = self.run("selection.set", json!({ "ids": ids }));
            self.selection_changed();
        }
        if zoom {
            match (corner(&item["min"]), corner(&item["max"])) {
                (Some(lo), Some(hi)) => self.zoom_to_box(lo, hi),
                _ => self.zoom_to_fit(),
            }
        }
        self.frame.set_status_text(
            &format!(
                "{} {}: {}",
                item["code"].as_str().unwrap_or_default(),
                item["title"].as_str().unwrap_or_default(),
                item["message"].as_str().unwrap_or_default()
            ),
            0,
        );
        self.refresh_check_marks();
    }

    fn ignore_check_issue(&mut self, ignore: bool) {
        let Some((key, reason)) = self.checks_pane.as_ref().and_then(|p| {
            let item = p.items.get(p.current?)?;
            Some((item["key"].as_str()?.to_owned(), p.reason.get_value()))
        }) else {
            return self.frame.set_status_text("Choose an issue first", 0);
        };
        let reason = (!reason.trim().is_empty()).then_some(reason);
        match self.run(
            "analytical_model.ignore_issue",
            json!({ "issue_key": key, "ignore": ignore, "reason": reason }),
        ) {
            Ok(_) => {
                let (s, c) = self.checks_pane.as_ref().map_or((true, true), |p| p.last);
                self.analytical_check(s, c);
                self.frame.set_status_text(
                    if ignore {
                        "Issue ignored until one of its elements changes"
                    } else {
                        "Issue reported again"
                    },
                    0,
                );
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn export_checks(&mut self) {
        let Some(r) = self.checks_pane.as_ref().and_then(|p| p.report.clone()) else {
            return self.frame.set_status_text("Run a check first", 0);
        };
        let dialog = FileDialog::builder(&self.frame)
            .with_message("Export Analytical Checks")
            .with_wildcard("CSV files (*.csv)|*.csv")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file("analytical checks.csv")
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = String::from(
            "severity,code,title,message,elements,measured_mm,tolerance_mm,ignored,key\n",
        );
        let issues = r["issues"].as_array().cloned().unwrap_or_default();
        for i in &issues {
            let ids: Vec<String> = i["element_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect();
            let mm = |v: &Value| {
                v.as_f64()
                    .map(|x| format!("{:.1}", x * 1000.0))
                    .unwrap_or_default()
            };
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                i["severity"].as_str().unwrap_or_default(),
                i["code"].as_str().unwrap_or_default(),
                quote(i["title"].as_str().unwrap_or_default()),
                quote(i["message"].as_str().unwrap_or_default()),
                quote(&ids.join(" ")),
                mm(&i["measured"]),
                mm(&i["tolerance"]),
                i["ignored"] == true,
                quote(i["key"].as_str().unwrap_or_default()),
            ));
        }
        match std::fs::write(&path, out) {
            Ok(()) => self
                .frame
                .set_status_text(&format!("Exported {} issue(s) to {path}", issues.len()), 0),
            Err(e) => error(&self.frame, &format!("{path}: {e}")),
        }
    }

    /// Redraws 3D views so the issue marks follow the list.
    fn refresh_check_marks(&mut self) {
        for i in 0..self.views.len() {
            self.update_inspection(i);
            self.views[i].panel.refresh(false, None);
        }
    }

    /// Marks for the listed issues while the pane is shown: a box around each issue's location,
    /// the chosen one in yellow.
    pub(crate) fn check_marks(&self) -> Vec<Lines> {
        let Some(p) = &self.checks_pane else {
            return Vec::new();
        };
        if !self.aui.is_pane_shown("analytical_checks") {
            return Vec::new();
        }
        let cube = |lo: [f64; 3], hi: [f64; 3], pad: f64| -> Vec<[[f32; 3]; 2]> {
            let lo = lo.map(|v| (v - pad) as f32);
            let hi = hi.map(|v| (v + pad) as f32);
            let c = |x: bool, y: bool, z: bool| {
                [
                    if x { hi[0] } else { lo[0] },
                    if y { hi[1] } else { lo[1] },
                    if z { hi[2] } else { lo[2] },
                ]
            };
            let mut s = Vec::new();
            for a in [false, true] {
                for b in [false, true] {
                    s.push([c(false, a, b), c(true, a, b)]);
                    s.push([c(a, false, b), c(a, true, b)]);
                    s.push([c(a, b, false), c(a, b, true)]);
                }
            }
            s
        };
        let mut marks = Vec::new();
        let mut current = Vec::new();
        for (k, i) in p.items.iter().enumerate() {
            let (Some(lo), Some(hi)) = (corner(&i["min"]), corner(&i["max"])) else {
                continue;
            };
            if Some(k) == p.current {
                current.extend(cube(lo, hi, 0.4));
            } else if (0..3).all(|a| hi[a] - lo[a] < 3.0) {
                // Local issues (a gap, a free end); a whole part is marked only when chosen.
                marks.extend(cube(lo, hi, 0.25));
            }
        }
        vec![
            Lines {
                color: MARK,
                width: 0.4,
                segments: marks,
            },
            Lines {
                color: MARK_CURRENT,
                width: 0.8,
                segments: current,
            },
        ]
    }

    /// Automatic checks (STR-SET-001): with *Member supports* or *Analytical/physical
    /// consistency* on, the checks run after each change; issues not seen before are reported,
    /// and the pane's list is refreshed when it is open.
    pub(crate) fn auto_analytical_check(&mut self) {
        let Ok(s) = self.run("structural_settings.get", json!({})) else {
            return;
        };
        let (supports, consistency) = (
            s["member_supports_check"] == true,
            s["consistency_check"] == true,
        );
        if !supports && !consistency {
            self.auto_check_keys = None;
            return;
        }
        let command = match (supports, consistency) {
            (true, false) => "analytical_model.check_supports",
            (false, true) => "analytical_model.check_consistency",
            _ => "analytical_model.check",
        };
        let Ok(r) = self.run(command, json!({})) else {
            return;
        };
        let keys: HashSet<String> = r["issues"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| i["ignored"] != true && i["severity"] != "info")
            .filter_map(|i| i["key"].as_str().map(str::to_owned))
            .collect();
        let new = self
            .auto_check_keys
            .as_ref()
            .map_or(0, |before| keys.difference(before).count());
        self.auto_check_keys = Some(keys);
        if let Some(p) = self.checks_pane.as_mut() {
            p.last = (supports, consistency);
            p.report = Some(r);
            self.fill_check_list();
        }
        if new > 0 {
            self.frame.set_status_text(
                &format!(
                    "Analytical checks: {new} new issue(s) — see Analyze › Checks › Analytical Checks"
                ),
                0,
            );
        }
    }
}
