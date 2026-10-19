// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structure › Steel cuts and modifiers (STR-STL-008): Cope, Miter, Saw Cut, Cut Through and
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
