// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structure › Framing › Automate Connections (STR-STL-004): the rule set, what to do with joins
//! that have a connection and whether to preview; then the log, a line per join. A preview marks
//! each join in 3D views, coloured by the type it would get (grey where skipped), and Apply places
//! them as previewed. The log saves as CSV.

use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// A join's marker colour: by type when placed, grey when skipped, red when failed.
fn colour(j: &Value) -> [f32; 4] {
    match (j["status"].as_str(), j["type_id"].as_str()) {
        (Some("failed"), _) => [0.85, 0.1, 0.1, 1.0],
        (Some("skipped"), _) => [0.55, 0.55, 0.55, 1.0],
        (_, Some("base_plate")) => [0.1, 0.35, 0.85, 1.0],
        (_, Some("shear_plate")) => [0.1, 0.6, 0.2, 1.0],
        (_, Some("end_plate")) => [0.55, 0.2, 0.75, 1.0],
        (_, Some("gusset")) => [0.9, 0.5, 0.0, 1.0],
        _ => [0.2, 0.2, 0.2, 1.0],
    }
}

fn type_name(id: &str) -> String {
    reviera_core::steel_fab::connection_type(id).map_or_else(|| id.to_owned(), |t| t.name.to_owned())
}

impl Gui {
    /// Structure › Framing › Automate Connections.
    pub(crate) fn automate_connections(&mut self) {
        let frame = self.frame;
        let info = match self.run("structural_connection.rule_sets", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let names: Vec<String> = info["rule_sets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["name"].as_str().map(str::to_owned))
            .collect();
        let selected = self.selection();
        let dialog = Dialog::builder(&frame, "Automate Connections").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let scope = if selected.is_empty() {
            "Every join in the model".to_owned()
        } else {
            format!("The joins of the {} selected element(s)", selected.len())
        };
        let text = StaticText::builder(&dialog)
            .with_label(&format!("{scope}: column feet, beam ends and brace ends.\nRule set:"))
            .build();
        sizer.add(&text, 0, SizerFlag::All, 10);
        let rules = Choice::builder(&dialog).with_choices(names.clone()).build();
        rules.set_selection(0);
        sizer.add(&rules, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, 10);
        let label = StaticText::builder(&dialog).with_label("Joins that already have a connection:").build();
        sizer.add(&label, 0, SizerFlag::All, 10);
        let existing = Choice::builder(&dialog)
            .with_choices(vec![
                "Skip them".to_owned(),
                "Give generic connections the matched type".to_owned(),
            ])
            .build();
        existing.set_selection(0);
        sizer.add(&existing, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, 10);
        let preview = CheckBox::builder(&dialog)
            .with_label("Preview first (marks each join in 3D views; nothing changes until Apply)")
            .build();
        preview.set_value(true);
        sizer.add(&preview, 0, SizerFlag::All, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Run").build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let go = dialog.show_modal() == ID_OK;
        let rule_set = rules.get_selection().and_then(|i| names.get(i as usize).cloned());
        let replace = existing.get_selection() == Some(1);
        let preview = preview.get_value();
        dialog.destroy();
        if !go {
            return;
        }
        let mut args = json!({
            "rule_set": rule_set,
            "existing": if replace { "replace_generic" } else { "skip" },
            "preview": preview,
        });
        if !selected.is_empty() {
            args["element_ids"] = json!(selected);
        }
        let report = match self.run("structural_connection.automate", args) {
            Ok(r) => r,
            Err(e) => return error(&frame, &format!("Automation did not run:\n{e}")),
        };
        if preview {
            self.auto_preview = Some(report.clone());
            self.redraw_inspection();
            let apply = self.automation_log(&report, true);
            self.auto_preview = None;
            if apply {
                match self.run("structural_connection.apply_automation", json!({ "report": report })) {
                    Ok(done) => {
                        self.automation_status(&done);
                        self.refresh_scene(&frame);
                    }
                    Err(e) => error(&frame, &format!("The connections were not placed:\n{e}")),
                }
            }
            self.redraw_inspection();
        } else {
            self.refresh_scene(&frame);
            self.automation_status(&report);
            self.automation_log(&report, false);
        }
    }

    /// Draws the views now: a modal dialog follows, during which the views cannot draw.
    fn redraw_inspection(&mut self) {
        let ids: Vec<u32> = self.views.iter().map(|v| v.id).collect();
        for id in ids {
            self.draw(id);
        }
    }

    fn automation_status(&self, r: &Value) {
        let n = |k: &str| r[k].as_u64().unwrap_or(0);
        self.frame.set_status_text(
            &format!(
                "Automate Connections: {} placed, {} skipped, {} failed (Edit › Undo undoes the run)",
                n("placed"),
                n("skipped"),
                n("failed")
            ),
            0,
        );
    }

    /// The log: a line per join. With `preview`, Apply returns true.
    fn automation_log(&mut self, report: &Value, preview: bool) -> bool {
        let frame = self.frame;
        let csv = self
            .run("structural_connection.automation_csv", json!({ "report": report }))
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let n = |k: &str| report[k].as_u64().unwrap_or(0);
        let title = if preview { "Automate Connections: Preview" } else { "Automate Connections: Log" };
        let dialog = Dialog::builder(&frame, title)
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let verb = if preview { "would be placed" } else { "placed" };
        let summary = StaticText::builder(&dialog)
            .with_label(&format!(
                "Rule set {}: {} {verb}, {} skipped, {} failed.",
                report["rule_set"].as_str().unwrap_or(""),
                n("placed"),
                n("skipped"),
                n("failed")
            ))
            .build();
        sizer.add(&summary, 0, SizerFlag::All, 10);
        let list = ListCtrl::builder(&dialog)
            .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
            .with_size(Size::new(if preview { 640 } else { 820 }, if preview { 220 } else { 380 }))
            .build();
        for (k, (heading, width)) in
            [("Status", 64), ("Join", 96), ("Type", 150), ("Members", 100), ("Reason", 300)].into_iter().enumerate()
        {
            list.insert_column(k as i64, heading, ListColumnFormat::Left, width);
        }
        let empty = Vec::new();
        for (k, j) in report["joins"].as_array().unwrap_or(&empty).iter().enumerate() {
            let k = k as i64;
            let status = j["status"].as_str().unwrap_or("");
            list.insert_item(k, status, None);
            let kind = j["kind"].as_str().unwrap_or("").replace('_', " ");
            let ty = j["type_id"].as_str().map(type_name).unwrap_or_default();
            let members: Vec<String> = j["members"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .map(|m| m.to_string())
                .collect();
            for (c, text) in [kind, ty, members.join(", "), j["reason"].as_str().unwrap_or("").to_owned()]
                .iter()
                .enumerate()
            {
                list.set_item_text_by_column(k, c as i32 + 1, text);
            }
            let [r, g, b, _] = colour(j).map(|v| (v * 255.0) as u8);
            list.set_item_text_colour(k, &Colour::rgb(r, g, b));
        }
        sizer.add(&list, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let save = Button::builder(&dialog).with_label("Save CSV…").build();
        save.on_click(move |_| {
            let fd = FileDialog::builder(&dialog)
                .with_message("Save the Automation Log")
                .with_wildcard("CSV files (*.csv)|*.csv")
                .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                .with_default_file("connection automation.csv")
                .build();
            if fd.show_modal() == ID_OK
                && let Some(path) = fd.get_path()
                && let Err(e) = std::fs::write(&path, &csv)
            {
                error(&frame, &format!("The log was not saved:\n{e}"));
            }
        });
        buttons.add(&save, 0, SizerFlag::All, 6);
        buttons.add_stretch_spacer(1);
        let close = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label(if preview { "Cancel" } else { "Close" })
            .build();
        buttons.add(&close, 0, SizerFlag::All, 6);
        if preview {
            let apply = Button::builder(&dialog).with_id(ID_OK).with_label("Apply").build();
            buttons.add(&apply, 0, SizerFlag::All, 6);
            dialog.set_affirmative_id(ID_OK);
        }
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        if preview {
            // In the frame's lower left, over the side panes, so the marked joins stay in sight.
            let (fs, ds) = (frame.get_client_size(), dialog.get_size());
            let at = frame.client_to_screen(Point::new(4, (fs.height - ds.height - 30).max(0)));
            dialog.move_window(at.x, at.y);
        }
        let applied = dialog.show_modal() == ID_OK && preview;
        dialog.destroy();
        applied
    }

    /// A preview's markers in 3D views: a cross and a small box at each join, by type.
    pub(crate) fn automation_lines(&self, index: usize) -> Vec<Lines> {
        let Some(r) = &self.auto_preview else { return Vec::new() };
        if self.views[index].is_2d() {
            return Vec::new();
        }
        let mut out: Vec<Lines> = Vec::new();
        for j in r["joins"].as_array().into_iter().flatten() {
            let Some(p) = j["point"].as_array().map(|p| p.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>())
            else {
                continue;
            };
            if p.len() != 3 {
                continue;
            }
            let (s, b) = (0.35f32, 0.12f32);
            let mut seg = vec![
                [[p[0] - s, p[1], p[2]], [p[0] + s, p[1], p[2]]],
                [[p[0], p[1] - s, p[2]], [p[0], p[1] + s, p[2]]],
                [[p[0], p[1], p[2] - s], [p[0], p[1], p[2] + s]],
            ];
            if j["status"] == "placed" {
                for (dx, dy) in [(-b, -b), (b, -b), (b, b), (-b, b)] {
                    seg.push([[p[0] + dx, p[1] + dy, p[2] - b], [p[0] + dx, p[1] + dy, p[2] + b]]);
                }
                for z in [-b, b] {
                    let c = |dx: f32, dy: f32| [p[0] + dx, p[1] + dy, p[2] + z];
                    seg.push([c(-b, -b), c(b, -b)]);
                    seg.push([c(b, -b), c(b, b)]);
                    seg.push([c(b, b), c(-b, b)]);
                    seg.push([c(-b, b), c(-b, -b)]);
                }
            }
            let color = colour(j);
            match out.iter_mut().find(|l| l.color == color) {
                Some(l) => l.segments.extend(seg),
                None => out.push(Lines { color, width: 0.8, segments: seg }),
            }
        }
        out
    }
}
