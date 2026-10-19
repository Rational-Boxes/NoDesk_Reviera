// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel › Cuts and Modifiers (STR-STL-008): Cope, Miter, Saw Cut, Cut Through and
//! Cut By on two selected elements (the first is cut), Uncope on the selected members, Shorten on
//! a beam or brace, Corner Cut on a plate; Contour Cut sketches like Plate
//! (`steel_detail_ui`).

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    fn cut_done(&mut self, what: &str, result: Result<Value, String>) {
        let frame = self.frame;
        match result {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{what} made (Edit › Undo undoes it)"), 0);
            }
            Err(e) => error(&frame, &format!("{what} was not made:\n{e}")),
        }
    }

    /// Cope, Miter, Saw Cut, Cut Through, Cut By: two elements selected, the first cut.
    pub(crate) fn pair_cut(&mut self, command: &'static str) {
        let frame = self.frame;
        let sel = self.selection();
        let [a, b] = sel[..] else {
            return frame.set_status_text("Select the two elements (the one to cut first), then the tool", 0);
        };
        let (what, args) = match command {
            "steel_cut.cope" => ("The cope", json!({ "member_ids": [a, b] })),
            "steel_cut.miter" => ("The miter", json!({ "a": a, "b": b })),
            "steel_cut.saw" => {
                let d = SingleChoiceDialog::builder(&frame, "Cut flush with the other member's:", "Saw Cut", &["Flange", "Web"])
                    .build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let part = if d.get_selection() == 1 { "web" } else { "flange" };
                ("The saw cut", json!({ "a": a, "b": b, "part": part }))
            }
            "steel_cut.cut_through" => {
                let d = TextEntryDialog::builder(&frame, "Gap around the cutting element (mm):", "Cut Through")
                    .with_default_value("0")
                    .build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let gap = d.get_value().unwrap_or_default().trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
                ("The cut", json!({ "cut_id": a, "cutter_id": b, "gap": gap }))
            }
            _ => ("The cut", json!({ "cut_id": a, "cutter_id": b })),
        };
        let r = self.run(command, args);
        self.cut_done(what, r);
    }

    /// Uncope: the selected members' copes.
    pub(crate) fn uncope(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        if sel.is_empty() {
            return frame.set_status_text("Uncope: select the coped members first", 0);
        }
        match self.run("steel_cut.uncope", json!({ "member_ids": sel })) {
            Ok(r) => {
                self.refresh_scene(&frame);
                let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
                let skipped = if n("skipped") > 0 {
                    format!("; {} belonging to connections left", n("skipped"))
                } else {
                    String::new()
                };
                frame.set_status_text(&format!("{} cope(s) removed{skipped}", n("removed")), 0);
            }
            Err(e) if e.contains("uncope.nothing") => frame.set_status_text("Uncope: nothing to remove", 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Shorten: the selected beam's end, length and angles.
    pub(crate) fn shorten_member(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let [member] = sel[..] else {
            return frame.set_status_text("Shorten: select one beam or brace first", 0);
        };
        let dialog = Dialog::builder(&frame, "Shorten").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        let end = Choice::builder(&dialog).with_choices(vec!["Start".into(), "End".into()]).build();
        end.set_selection(1);
        let text = |v: &str| TextCtrl::builder(&dialog).with_value(v).build();
        let (length, ah, aw) = (text("100"), text("0"), text("0"));
        macro_rules! row {
            ($l:expr, $w:expr) => {{
                grid.add(&StaticText::builder(&dialog).with_label($l).build(), 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&$w, 1, SizerFlag::Expand, 0);
            }};
        }
        row!("End", end);
        row!("Length (mm; negative extends)", length);
        row!("Angle across the height (°)", ah);
        row!("Angle across the width (°)", aw);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Shorten").build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let go = dialog.show_modal() == ID_OK;
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let args = json!({
            "member_id": member,
            "end": if end.get_selection() == Some(0) { "start" } else { "end" },
            "length": num(&length) / 1000.0,
            "angle_h": num(&ah),
            "angle_w": num(&aw),
        });
        dialog.destroy();
        if go {
            let r = self.run("steel_modifier.shorten", args);
            self.cut_done("The shortening", r);
        }
    }

    /// Corner Cut: the selected plate's corner, kind and size.
    pub(crate) fn corner_cut(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let Some(plate) = sel.first().copied() else {
            return frame.set_status_text("Corner Cut: select a plate first", 0);
        };
        let Ok(info) = self.run("plate.info", json!({ "element_ids": [plate] })) else {
            return frame.set_status_text("Corner Cut: select a plate first", 0);
        };
        let corners: Vec<String> = info[0]["contour"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(k, c)| {
                format!(
                    "Corner {k} ({:.0}, {:.0} mm)",
                    c[0].as_f64().unwrap_or(0.0) * 1000.0,
                    c[1].as_f64().unwrap_or(0.0) * 1000.0
                )
            })
            .collect();
        let dialog = Dialog::builder(&frame, "Corner Cut").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        let corner = Choice::builder(&dialog).with_choices(corners).build();
        corner.set_selection(0);
        let kind = Choice::builder(&dialog)
            .with_choices(vec!["Straight".into(), "Convex (rounded)".into(), "Concave (notch)".into()])
            .build();
        kind.set_selection(0);
        let size = TextCtrl::builder(&dialog).with_value("25").build();
        macro_rules! row {
            ($l:expr, $w:expr) => {{
                grid.add(&StaticText::builder(&dialog).with_label($l).build(), 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&$w, 1, SizerFlag::Expand, 0);
            }};
        }
        row!("Corner", corner);
        row!("Kind", kind);
        row!("Size (mm; 0 removes)", size);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Cut").build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let go = dialog.show_modal() == ID_OK;
        const KINDS: [&str; 3] = ["straight", "convex", "concave"];
        let k = KINDS[kind.get_selection().unwrap_or(0).min(2) as usize];
        let args = json!({
            "plate_id": plate,
            "corner": corner.get_selection().unwrap_or(0),
            "kind": k,
            "size": size.get_value().trim().parse::<f64>().unwrap_or(0.0) / 1000.0,
        });
        dialog.destroy();
        if go {
            let r = self.run("steel_modifier.corner_cut", args);
            self.cut_done("The corner cut", r);
        }
    }
}

impl Gui {
    /// A numbered element's part and assembly marks.
    pub(crate) fn steel_mark_rows(&mut self, grid: &PropertyGrid, id: reviera_core::ElementId) {
        let Some(store) = self.store_now() else { return };
        let Some((part, assembly)) = store.get(id).and_then(reviera_core::steel_numbering::marks_of) else {
            return;
        };
        grid.append(Property::category("Steel Marks", "c_marks"));
        grid.append(Property::string("Part mark", "mk_part", part));
        grid.set_property_read_only("mk_part", true);
        grid.append(Property::string("Assembly mark", "mk_assembly", assembly.unwrap_or_default()));
        grid.set_property_read_only("mk_assembly", true);
    }

    /// Steel › Numbering.
    pub(crate) fn steel_numbering(&mut self) {
        let frame = self.frame;
        match self.run("steel_numbering.run", json!({})) {
            Ok(r) => {
                self.properties_stale = true;
                let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
                let mut text = format!("{} part mark(s), {} assembly mark(s):\n\n", n("parts"), n("assemblies"));
                for p in r["parts"].as_array().into_iter().flatten() {
                    text.push_str(&format!(
                        "{} × {}  {}\n",
                        p["mark"].as_str().unwrap_or(""),
                        p["count"],
                        p["description"].as_str().unwrap_or("")
                    ));
                }
                text.push('\n');
                for a in r["assemblies"].as_array().into_iter().flatten() {
                    let parts: Vec<&str> = a["parts"].as_array().into_iter().flatten().filter_map(|m| m.as_str()).collect();
                    text.push_str(&format!("{} × {}  ({})\n", a["mark"].as_str().unwrap_or(""), a["count"], parts.join(", ")));
                }
                frame.set_status_text("Steel numbered: marks are in each element's Properties and its IFC Tag", 0);
                let d = MessageDialog::builder(&frame, &text, "Steel Numbering").with_style(MessageDialogStyle::OK).build();
                d.show_modal();
            }
            Err(e) => error(&frame, &format!("The steel was not numbered:\n{e}")),
        }
    }

    /// Steel › NC Export.
    pub(crate) fn steel_nc_export(&mut self) {
        let frame = self.frame;
        let d = DirDialog::builder(&frame, "Export DSTV NC1 Files To", "").build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(folder) = d.get_path() else { return };
        let r = self.run("steel_nc.export", json!({ "folder": folder, "overwrite": true }));
        match r {
            Ok(files) => {
                let n = files.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} DSTV NC1 file(s) written to {folder}"), 0);
            }
            Err(e) => error(&frame, &format!("The NC files were not written:\n{e}")),
        }
    }
}

impl Gui {
    /// The connections selected, or those whose plates, bolts or welds are.
    fn selected_connections(&mut self) -> Vec<reviera_core::ElementId> {
        let Some(store) = self.store_now() else { return Vec::new() };
        let mut out = Vec::new();
        for id in self.selection() {
            let Some(e) = store.get(id) else { continue };
            let c = if reviera_core::connections::recipe(e).is_some() {
                Some(id)
            } else {
                reviera_core::steel_fab::fab_recipe(e)
                    .and_then(|r| reviera_core::id::GlobalId::parse(&r.connection))
                    .and_then(|g| store.find_by_guid(&g))
                    .filter(|c| store.get(*c).and_then(reviera_core::connections::recipe).is_some())
            };
            if let Some(c) = c
                && !out.contains(&c)
            {
                out.push(c);
            }
        }
        out
    }

    /// The provider and code to use: asked when there is a choice.
    fn choose_provider(&mut self) -> Option<(String, String)> {
        let frame = self.frame;
        let providers = self.run("connection_check.providers", json!({})).ok()?;
        let mut choices: Vec<(String, String, String)> = Vec::new();
        for p in providers.as_array().into_iter().flatten() {
            let id = p["id"].as_str().unwrap_or("").to_owned();
            for c in p["codes"].as_array().into_iter().flatten() {
                let code = c.as_str().unwrap_or("").to_owned();
                choices.push((id.clone(), code.clone(), format!("{} — {code}", p["title"].as_str().unwrap_or(&id))));
            }
        }
        if choices.is_empty() {
            error(&frame, "There is no connection design provider (a Lua plugin with capability \"connection_check\").");
            return None;
        }
        // The region's code first.
        let us = self.store_now().is_some_and(|s| reviera_core::steel_fab::us_region(&s));
        choices.sort_by_key(|c| (c.1 != if us { "aisc360" } else { "en1993" }, c.2.clone()));
        let labels: Vec<&str> = choices.iter().map(|c| c.2.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Provider and design code:", "Check Connections", &labels).build();
        if d.show_modal() != ID_OK {
            return None;
        }
        let c = choices.get(usize::try_from(d.get_selection()).ok()?)?;
        Some((c.0.clone(), c.1.clone()))
    }

    /// Steel › Fabrication › Check (and Presize).
    pub(crate) fn check_connections(&mut self, presize: bool) {
        let frame = self.frame;
        let ids = self.selected_connections();
        if ids.is_empty() {
            return frame.set_status_text("Select the connections to check (or their plates or bolts) first", 0);
        }
        let Some((provider, code)) = self.choose_provider() else { return };
        let command = if presize { "structural_connection.presize" } else { "structural_connection.check" };
        match self.run(command, json!({ "connection_ids": ids, "provider_id": provider, "code": code })) {
            Ok(r) => {
                self.properties_stale = true;
                self.refresh_scene(&frame);
                let list = r.as_array().cloned().unwrap_or_default();
                let ok = list.iter().filter(|c| c["record"]["ok"] == true).count();
                let worst = list.iter().filter_map(|c| c["record"]["utilisation"].as_f64()).fold(0.0, f64::max);
                frame.set_status_text(
                    &format!(
                        "{} connection(s) {}: {ok} OK, {} failing; largest utilisation {worst:.2} (Properties lists each check)",
                        list.len(),
                        if presize { "presized" } else { "checked" },
                        list.len() - ok
                    ),
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The connections were not checked:\n{e}")),
        }
    }

    /// Steel › Fabrication › Check Report.
    pub(crate) fn check_report(&mut self) {
        let frame = self.frame;
        let ids = self.selected_connections();
        if ids.is_empty() {
            return frame.set_status_text("Select the connections for the report first", 0);
        }
        let d = FileDialog::builder(&frame)
            .with_message("Save the Connection Check Report")
            .with_wildcard("HTML files (*.html)|*.html")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file("connection checks.html")
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        match self.run("structural_connection.check_report", json!({ "connection_ids": ids, "path": path })) {
            Ok(_) => frame.set_status_text(&format!("Connection check report written to {path}"), 0),
            Err(e) => error(&frame, &format!("The report was not written:\n{e}")),
        }
    }
}
