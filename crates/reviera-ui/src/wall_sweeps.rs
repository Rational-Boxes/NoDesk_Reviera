// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall Sweep and Wall Reveal (ARC-WALL-010) on the selected walls: a dialog for the
//! placement and the rectangular profile. A horizontal run continues around the walls joined
//! end to end with the first. The sweep's own rows in Properties change it afterwards.

use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

impl Gui {
    /// Modify | Walls › Sweep.
    pub(crate) fn wall_sweep(&mut self) {
        self.sweep_dialog(false);
    }

    /// Modify | Walls › Reveal.
    pub(crate) fn wall_reveal(&mut self) {
        self.sweep_dialog(true);
    }

    fn sweep_dialog(&mut self, reveal: bool) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let walls: Vec<_> = self
            .selection()
            .into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::walls::wall_recipe)
                    .is_some_and(|r| r.through.is_empty())
            })
            .collect();
        if walls.is_empty() {
            return frame.set_status_text("Select straight walls first", 0);
        }
        let title = if reveal { "Wall Reveal" } else { "Wall Sweep" };
        let dialog = Dialog::builder(&frame, title)
            .with_style(DialogStyle::DefaultDialogStyle)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let choice = |label: &str, choices: &[&str]| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let c = Choice::builder(&dialog)
                .with_choices(choices.iter().map(|s| s.to_string()).collect())
                .build();
            c.set_selection(0);
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&c, 1, SizerFlag::Expand, 0);
            c
        };
        let orientation = choice("Placement", &["Horizontal", "Vertical"]);
        let side = choice("Side", &["Exterior", "Interior"]);
        let text = |label: &str, value: String| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let t = TextCtrl::builder(&dialog).with_value(&value).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&t, 1, SizerFlag::Expand, 0);
            t
        };
        let at = text(
            "Offset from level (horizontal)",
            self.format_length(if reveal { 1.0 } else { 2.7 }),
        );
        let along = text("Distance along wall (vertical)", String::new());
        let width = text(
            if reveal { "Depth" } else { "Projection" },
            self.format_length(if reveal { 0.02 } else { 0.05 }),
        );
        let height = text(
            "Profile height",
            self.format_length(if reveal { 0.02 } else { 0.1 }),
        );
        let cuts = CheckBox::builder(&dialog).with_label("Cuts wall").build();
        let chain = CheckBox::builder(&dialog)
            .with_label("Continue around joined walls")
            .build();
        chain.set_value(true);
        sizer.add_sizer(&grid, 0, SizerFlag::All | SizerFlag::Expand, 10);
        if !reveal {
            sizer.add(
                &cuts,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                10,
            );
        }
        sizer.add(
            &chain,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            10,
        );
        let ok = Button::builder(&dialog)
            .with_id(ID_OK)
            .with_label("OK")
            .build();
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("Cancel")
            .build();
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        ok.set_default();
        loop {
            if dialog.show_modal() != ID_OK {
                break;
            }
            let mut length = |t: &TextCtrl| -> Result<Option<f64>, String> {
                let v = t.get_value();
                if v.trim().is_empty() {
                    return Ok(None);
                }
                self.run("units.parse", json!({ "text": v, "data_type": "length" }))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(Some)
                    .ok_or_else(|| format!("`{v}` is not a length"))
            };
            let args = (|| -> Result<serde_json::Value, String> {
                Ok(json!({
                    "wall_ids": walls,
                    "orientation": if orientation.get_selection() == Some(1) { "vertical" } else { "horizontal" },
                    "side": if side.get_selection() == Some(1) { "interior" } else { "exterior" },
                    "offset_from_level": length(&at)?,
                    "distance_along": length(&along)?,
                    "width": length(&width)?,
                    "height": length(&height)?,
                    "cuts_wall": !reveal && cuts.get_value(),
                    "chain": chain.get_value(),
                }))
            })();
            let command = if reveal {
                "wallreveal.create"
            } else {
                "wallsweep.create"
            };
            match args.and_then(|a| self.run(command, a)) {
                Ok(_) => {
                    self.refresh_scene(&frame);
                    frame.set_status_text(&format!("{title} added"), 0);
                    break;
                }
                Err(e) => super::error(&frame, &format!("Not added:\n{e}")),
            }
        }
        dialog.destroy();
    }
}
