// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Results Explorer (STR-RES-001): a dockable pane, active once an analysis's results are
//! kept with the model. It chooses the load case or combination, the result type (member force
//! diagrams, the deformed shape, reactions), which members, the scale, deformation and labels,
//! or a preset, and shows them in the 3D view as an overlay (positive values blue, negative red,
//! the deformed shape green, values in the stroke font). Insights lists the extremes; clicking
//! one selects and frames its member. The Results Manager part exports, imports and deletes the
//! result set. A banner says when the model has changed since the analysis.

use std::cell::RefCell;
use std::rc::Rc;

use reviera_core::ElementId;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error, with};

const TYPES: [(&str, &str); 8] = [
    ("Moment My", "moment_y"),
    ("Moment Mz", "moment_z"),
    ("Shear Vz", "shear_z"),
    ("Shear Vy", "shear_y"),
    ("Axial force N", "axial"),
    ("Torsion T", "torsion"),
    ("Deformed shape", "deformed"),
    ("Reactions", "reactions"),
];

const FILTERS: [(&str, &str); 5] = [
    ("All members", "all"),
    ("Columns", "columns"),
    ("Beams", "beams"),
    ("Braces", "braces"),
    ("Transfer beams", "transfer_beams"),
];

const POSITIVE: [f32; 4] = [0.1, 0.35, 0.9, 1.0];
const NEGATIVE: [f32; 4] = [0.85, 0.15, 0.15, 1.0];
const DEFORMED: [f32; 4] = [0.1, 0.6, 0.25, 1.0];
const LABEL: [f32; 4] = [0.1, 0.1, 0.1, 1.0];

pub(crate) struct ResultsPane {
    banner: StaticText,
    group: Choice,
    kind: Choice,
    filter: Choice,
    preset: Choice,
    scale: TextCtrl,
    deformation: CheckBox,
    labels: CheckBox,
    insights: ListCtrl,
    /// The load groups and presets listed, in order.
    groups: Vec<String>,
    presets: Vec<Value>,
    /// The Insights rows' elements.
    rows: Vec<Value>,
}

fn button(panel: &Panel, label: &str, tip: &str) -> Button {
    let b = Button::builder(panel).with_label(label).build();
    b.set_tooltip(tip);
    b
}

fn label(panel: &Panel, text: &str) -> StaticText {
    StaticText::builder(panel).with_label(text).build()
}

impl Gui {
    /// Shows or hides the Results Explorer, building it the first time.
    pub(crate) fn toggle_results_pane(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().results_pane.is_some() {
            with(gui, |g| {
                g.toggle_pane("results");
                g.refresh_results_pane();
            });
            return;
        }
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let banner = label(&panel, "");
        let group = Choice::builder(&panel).build();
        group.set_tooltip("The load case or combination shown");
        let kind = Choice::builder(&panel).build();
        for (l, _) in TYPES {
            kind.append(l);
        }
        kind.set_selection(0);
        let filter = Choice::builder(&panel).build();
        for (l, _) in FILTERS {
            filter.append(l);
        }
        filter.set_selection(0);
        let preset = Choice::builder(&panel).build();
        let apply = button(
            &panel,
            "Apply Preset",
            "Show the chosen preset in the 3D view",
        );
        let scale = TextCtrl::builder(&panel).with_value("1").build();
        scale.set_tooltip(
            "A multiplier on the automatic scale: the largest value is drawn 8% of the model's size long, the largest displacement 5%",
        );
        let deformation = CheckBox::builder(&panel)
            .with_label("Deformed shape too")
            .build();
        let labels = CheckBox::builder(&panel).with_label("Values").build();
        labels.set_value(true);
        let show = button(&panel, "Show", "Show this result in the 3D view");
        let hide = button(&panel, "Hide", "Stop showing results in the 3D view");
        let save = button(
            &panel,
            "Save Preset…",
            "Keep what the 3D view shows as a preset of this project",
        );
        let export = button(
            &panel,
            "Export…",
            "Write the result set as a .reviera-results package",
        );
        let import = button(&panel, "Import…", "Read a .reviera-results package");
        let delete = button(
            &panel,
            "Delete",
            "Delete the result set kept with the model",
        );
        let insights = ListCtrl::builder(&panel)
            .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
            .build();
        for (k, (heading, width)) in [("Value", 110), ("Member", 210), ("At", 70)]
            .into_iter()
            .enumerate()
        {
            insights.insert_column(k as i64, heading, ListColumnFormat::Left, width);
        }

        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(4)
            .with_hgap(6)
            .build();
        grid.add_growable_col(1, 1);
        for (text, control) in [
            ("Load case", &group),
            ("Result", &kind),
            ("Members", &filter),
        ] {
            grid.add(&label(&panel, text), 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(control, 1, SizerFlag::Expand, 0);
        }
        grid.add(
            &label(&panel, "Scale"),
            0,
            SizerFlag::AlignCenterVertical,
            0,
        );
        grid.add(&scale, 1, SizerFlag::Expand, 0);
        let left = BoxSizer::builder(Orientation::Vertical).build();
        left.add(&banner, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        left.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&deformation, 0, SizerFlag::Right, 10);
        row.add(&labels, 0, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Bottom, 4);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&show, 1, SizerFlag::Right, 2);
        row.add(&hide, 1, SizerFlag::Right, 2);
        row.add(&save, 1, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 6);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&preset, 1, SizerFlag::Right, 2);
        row.add(&apply, 0, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Bottom, 6);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&export, 1, SizerFlag::Right, 2);
        row.add(&import, 1, SizerFlag::Right, 2);
        row.add(&delete, 1, SizerFlag::empty(), 0);
        left.add_sizer(&row, 0, SizerFlag::Expand, 0);
        let right = BoxSizer::builder(Orientation::Vertical).build();
        right.add(
            &label(&panel, "Insights: the largest values"),
            0,
            SizerFlag::Bottom,
            2,
        );
        right.add(&insights, 1, SizerFlag::Expand, 0);
        let sizer = BoxSizer::builder(Orientation::Horizontal).build();
        sizer.add_sizer(&left, 0, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(&right, 1, SizerFlag::Expand | SizerFlag::All, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("results")
                .with_caption("Results Explorer")
                .bottom()
                .layer(1)
                .best_size(1000, 360)
                .min_size(500, 330)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        gui.borrow_mut().results_pane = Some(ResultsPane {
            banner,
            group,
            kind,
            filter,
            preset,
            scale,
            deformation,
            labels,
            insights,
            groups: Vec::new(),
            presets: Vec::new(),
            rows: Vec::new(),
        });
        {
            let g = gui.clone();
            show.on_click(move |_| with(&g, Gui::show_results));
        }
        {
            let g = gui.clone();
            hide.on_click(move |_| with(&g, Gui::hide_results));
        }
        {
            let g = gui.clone();
            apply.on_click(move |_| with(&g, Gui::apply_results_preset));
        }
        {
            let g = gui.clone();
            save.on_click(move |_| with(&g, Gui::save_results_preset));
        }
        {
            let g = gui.clone();
            export.on_click(move |_| with(&g, Gui::export_results));
        }
        {
            let g = gui.clone();
            import.on_click(move |_| with(&g, Gui::import_results));
        }
        {
            let g = gui.clone();
            delete.on_click(move |_| with(&g, Gui::delete_results));
        }
        for c in [&group, &kind, &filter] {
            let g = gui.clone();
            c.on_selection_changed(move |_| with(&g, Gui::show_results));
        }
        {
            let g = gui.clone();
            insights.on_item_selected(move |e| {
                let i = e.get_item_index();
                with(&g, |g| g.show_insight(i.max(0) as usize));
            });
        }
        with(gui, |g| {
            g.refresh_results_pane();
            g.show_results();
        });
    }

    /// Lists the load groups and presets, and the banner.
    pub(crate) fn refresh_results_pane(&mut self) {
        let packages = self.run("results.packages", json!({})).ok();
        let presets = self.run("results.presets", json!({})).ok();
        let Some(p) = self.results_pane.as_mut() else {
            return;
        };
        let pkg = packages
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned();
        let text = match &pkg {
            None => {
                "No analysis results: run Analyze › Structural Analysis › Run Analysis.".to_owned()
            }
            Some(k) if k["stale"] == true => format!(
                "⚠ The model has changed since this analysis ({}): run it again.",
                k["at"].as_str().unwrap_or_default()
            ),
            Some(k) => format!(
                "{} — {}",
                k["provider"].as_str().unwrap_or_default(),
                k["at"].as_str().unwrap_or_default()
            ),
        };
        p.banner.set_label(&text);
        let stale = pkg.as_ref().is_some_and(|k| k["stale"] == true);
        p.banner.set_foreground_color(if stale {
            Colour::new(200, 40, 40, 255)
        } else {
            Colour::new(90, 90, 90, 255)
        });
        let groups: Vec<String> = pkg
            .as_ref()
            .and_then(|k| k["load_groups"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if groups != p.groups {
            let keep = p.group.get_selection().unwrap_or(0) as usize;
            p.group.clear();
            for g in &groups {
                p.group.append(g);
            }
            if !groups.is_empty() {
                p.group.set_selection(keep.min(groups.len() - 1) as u32);
            }
            p.groups = groups;
        }
        let list: Vec<Value> = presets
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        if list.len() != p.presets.len() {
            p.preset.clear();
            for x in &list {
                let mut name = x["name"].as_str().unwrap_or_default().to_owned();
                if x["unavailable"].is_string() {
                    name.push_str(" (needs panel results)");
                }
                if x["user"] == true {
                    name.push_str(" (project)");
                }
                p.preset.append(&name);
            }
            if !list.is_empty() {
                p.preset.set_selection(0);
            }
            p.presets = list;
        }
    }

    /// The 3D view results are shown in: the active one (saving an unsaved `{3D}` first).
    fn results_view(&mut self) -> Option<ElementId> {
        if self.active_view().is_none_or(|v| v.is_2d()) {
            self.frame
                .set_status_text("Results are shown in 3D views: open one first", 0);
            return None;
        }
        if self.active_view().is_some_and(|v| v.element.is_none()) {
            self.default_3d_view();
        }
        self.active_view().and_then(|v| v.element)
    }

    fn show_results(&mut self) {
        let Some(p) = &self.results_pane else { return };
        if p.groups.is_empty() {
            return;
        }
        let group = p
            .groups
            .get(p.group.get_selection().unwrap_or(0) as usize)
            .cloned()
            .unwrap_or_default();
        let kind = TYPES[p.kind.get_selection().unwrap_or(0) as usize].1;
        let filter = FILTERS[p.filter.get_selection().unwrap_or(0) as usize].1;
        let scale = p.scale.get_value().trim().parse::<f64>().unwrap_or(1.0);
        let (deformation, labels) = (p.deformation.is_checked(), p.labels.is_checked());
        let Some(view) = self.results_view() else {
            return;
        };
        match self.run(
            "results.show",
            json!({ "view_id": view, "result_type": kind, "load_group": group, "filter": filter,
                    "scale": scale, "deformation": deformation, "labels": labels }),
        ) {
            Ok(_) => self.after_results_change(),
            Err(e) => error(&self.frame, &e),
        }
    }

    fn hide_results(&mut self) {
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return;
        };
        if self.run("results.hide", json!({ "view_id": view })).is_ok() {
            self.after_results_change();
        }
    }

    fn apply_results_preset(&mut self) {
        let Some(p) = &self.results_pane else { return };
        let Some(preset) = p
            .presets
            .get(p.preset.get_selection().unwrap_or(0) as usize)
            .cloned()
        else {
            return;
        };
        let group = p
            .groups
            .get(p.group.get_selection().unwrap_or(0) as usize)
            .cloned();
        let Some(view) = self.results_view() else {
            return;
        };
        match self.run(
            "results.apply_preset",
            json!({ "view_id": view, "preset_id": preset["id"], "load_group": group }),
        ) {
            Ok(d) => {
                // The controls follow the preset.
                if let Some(p) = &self.results_pane {
                    if let Some(k) = TYPES.iter().position(|t| d["result_type"] == t.1) {
                        p.kind.set_selection(k as u32);
                    }
                    if let Some(k) = FILTERS.iter().position(|t| d["filter"] == t.1) {
                        p.filter.set_selection(k as u32);
                    }
                    p.deformation.set_value(d["deformation"] == true);
                    p.scale
                        .set_value(&format!("{}", d["scale"].as_f64().unwrap_or(1.0)));
                }
                self.after_results_change();
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn save_results_preset(&mut self) {
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return;
        };
        let d =
            TextEntryDialog::builder(&self.frame, "Preset name:", "Save Results Preset").build();
        if d.show_modal() != ID_OK {
            return;
        }
        let name = d.get_value().unwrap_or_default();
        match self.run(
            "results.save_preset",
            json!({ "name": name, "view_id": view }),
        ) {
            Ok(_) => {
                self.refresh_results_pane();
                self.frame.set_status_text("Results preset saved", 0);
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn export_results(&mut self) {
        let dialog = FileDialog::builder(&self.frame)
            .with_message("Export Results")
            .with_wildcard("Reviera results (*.reviera-results)|*.reviera-results")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file("analysis.reviera-results")
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        match self.run("results.export_package", json!({ "path": path })) {
            Ok(_) => self
                .frame
                .set_status_text(&format!("Results exported to {path}"), 0),
            Err(e) => error(&self.frame, &e),
        }
    }

    fn import_results(&mut self) {
        let dialog = FileDialog::builder(&self.frame)
            .with_message("Import Results")
            .with_wildcard("Reviera results (*.reviera-results)|*.reviera-results")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        match self.run(
            "structural_analysis.import_results",
            json!({ "path": path }),
        ) {
            Ok(v) => {
                self.refresh_results_pane();
                self.show_results();
                self.frame.set_status_text(
                    if v["stale"] == true {
                        "Results imported; they were computed for another state of the model (stale)"
                    } else {
                        "Results imported"
                    },
                    0,
                );
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    fn delete_results(&mut self) {
        let d = MessageDialog::builder(
            &self.frame,
            "Delete the analysis results kept with the model? Views stop showing them.",
            "Delete Results",
        )
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build();
        if d.show_modal() != ID_YES {
            return;
        }
        match self.run("results.delete_package", json!({})) {
            Ok(_) => {
                self.refresh_results_pane();
                self.after_results_change();
            }
            Err(e) => error(&self.frame, &e),
        }
    }

    /// Redraws 3D views and refills Insights after the display changed.
    fn after_results_change(&mut self) {
        let frame = self.frame;
        self.refresh_scene(&frame);
        for i in 0..self.views.len() {
            self.update_inspection(i);
            self.views[i].panel.refresh(false, None);
        }
        self.fill_insights();
    }

    fn fill_insights(&mut self) {
        let display = self.active_view().and_then(|v| v.element).and_then(|id| {
            self.store_now()?
                .get(id)
                .and_then(reviera_core::results::display)
        });
        let x = display.as_ref().and_then(|d| {
            self.run(
                "results.extremes",
                json!({ "result_type": d.result_type, "load_group": d.load_group, "filter": d.filter }),
            )
            .ok()
        });
        let store = self.store_now();
        let Some(p) = self.results_pane.as_mut() else {
            return;
        };
        p.insights.delete_all_items();
        p.rows = x
            .as_ref()
            .and_then(|x| x["top"].as_array().cloned())
            .unwrap_or_default();
        let unit = x
            .as_ref()
            .and_then(|x| x["unit"].as_str())
            .unwrap_or_default()
            .to_owned();
        for (k, r) in p.rows.iter().enumerate() {
            let id = ElementId(r["element_id"].as_u64().unwrap_or_default());
            let name = store
                .as_ref()
                .and_then(|s| {
                    // The physical element's name where there is one.
                    let phys = r["physical"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(Value::as_u64)
                        .map(ElementId);
                    let e = s.get(phys.unwrap_or(id))?;
                    Some(format!(
                        "{} {}",
                        e.class.trim_start_matches("Ifc"),
                        e.name.clone().unwrap_or_else(|| e.id().0.to_string())
                    ))
                })
                .unwrap_or_else(|| format!("#{}", id.0));
            p.insights.insert_item(
                k as i64,
                &format!("{:.2} {unit}", r["value"].as_f64().unwrap_or(0.0)),
                None,
            );
            p.insights.set_item_text_by_column(k as i64, 1, &name);
            p.insights.set_item_text_by_column(
                k as i64,
                2,
                &r["at"]
                    .as_f64()
                    .map(|a| format!("{a:.2} m"))
                    .unwrap_or_default(),
            );
        }
    }

    /// Selects an Insights row's member and frames it.
    fn show_insight(&mut self, i: usize) {
        let Some(r) = self
            .results_pane
            .as_ref()
            .and_then(|p| p.rows.get(i).cloned())
        else {
            return;
        };
        let mut ids: Vec<Value> = r["physical"].as_array().cloned().unwrap_or_default();
        if ids.is_empty() {
            ids.push(r["element_id"].clone());
        }
        let _ = self.run("selection.set", json!({ "ids": ids }));
        self.selection_changed();
        if let Some(p) = r["point"].as_array() {
            let q: Vec<f64> = p.iter().filter_map(Value::as_f64).collect();
            if q.len() == 3 {
                self.zoom_to_box([q[0], q[1], q[2]], [q[0], q[1], q[2]]);
            }
        }
    }

    /// The results overlay for a 3D view showing results.
    pub(crate) fn results_lines(&self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        if v.is_2d() {
            return Vec::new();
        }
        let Some(element) = v.element else {
            return Vec::new();
        };
        let Some(d) = self.app.document(v.doc) else {
            return Vec::new();
        };
        let store = d.doc.store();
        let Some(display) = store.get(element).and_then(reviera_core::results::display) else {
            return Vec::new();
        };
        let Ok(o) = reviera_core::results::overlay(store, &display) else {
            return Vec::new();
        };
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
        let seg = |s: &[[f64; 3]; 2]| [f(s[0]), f(s[1])];
        let mut text = Vec::new();
        for l in &o.labels {
            for [a, b] in reviera_render::font::strokes(
                &l.text,
                f(l.origin).into(),
                f(l.right).into(),
                f(l.up).into(),
                l.height as f32,
            ) {
                text.push([a.to_array(), b.to_array()]);
            }
        }
        vec![
            Lines {
                color: POSITIVE,
                width: 0.4,
                segments: o.positive.iter().map(seg).collect(),
            },
            Lines {
                color: NEGATIVE,
                width: 0.4,
                segments: o.negative.iter().map(seg).collect(),
            },
            Lines {
                color: DEFORMED,
                width: 0.6,
                segments: o.deformed.iter().map(seg).collect(),
            },
            Lines {
                color: LABEL,
                width: 0.3,
                segments: text,
            },
        ]
    }
}
