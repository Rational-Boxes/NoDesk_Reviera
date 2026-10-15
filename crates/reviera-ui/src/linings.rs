// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Linings (ARC-WALL-014/015/016): finish walls against the selected walls' faces or round the
//! selected rooms, and hosted walls (Pick New Host, Remove Hosted, Select Main Host).

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;
use super::select::Combine;

impl Gui {
    /// Modify | Walls › Linings: the selected walls' faces, or the selected rooms all round.
    pub(crate) fn linings(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let ids = self.selection();
        let walls: Vec<ElementId> = ids
            .iter()
            .copied()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::walls::wall_recipe)
                    .is_some()
            })
            .collect();
        let rooms: Vec<ElementId> = ids
            .iter()
            .copied()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::rooms::room_recipe)
                    .is_some()
            })
            .collect();
        if walls.is_empty() && rooms.is_empty() {
            return frame.set_status_text("Select the walls or rooms to line", 0);
        }
        let types: Vec<(String, ElementId)> = store
            .iter()
            .filter(|e| {
                e.class == "IfcWallType"
                    && reviera_core::walls::type_width(e).is_some()
                    && reviera_core::walls::stacked::of_type(e).is_none()
            })
            .map(|e| (e.name.clone().unwrap_or_default(), e.id()))
            .collect();
        let dialog = Dialog::builder(&frame, "Linings").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let choice = |label: &str, choices: Vec<String>| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let c = Choice::builder(&dialog).with_choices(choices).build();
            c.set_selection(0);
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&c, 1, SizerFlag::Expand, 0);
            c
        };
        let ty = choice("Type", types.iter().map(|t| t.0.clone()).collect());
        let side = (!walls.is_empty()).then(|| {
            choice(
                "Side of the walls",
                vec!["Interior".into(), "Exterior".into()],
            )
        });
        let join = choice(
            "Auto Join",
            vec!["Off".into(), "Join".into(), "Join & Lock".into()],
        );
        sizer.add_sizer(&grid, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let hosted = CheckBox::builder(&dialog)
            .with_label("Hosted: follow the wall they line")
            .build();
        sizer.add(
            &hosted,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            10,
        );
        let note = StaticText::builder(&dialog)
            .with_label(&format!(
                "{} wall(s) and {} room(s) selected. Rooms are lined all round, inside.",
                walls.len(),
                rooms.len()
            ))
            .build();
        sizer.add(
            &note,
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
        if dialog.show_modal() == ID_OK {
            let type_id = ty
                .get_selection()
                .and_then(|i| types.get(i as usize))
                .map(|t| t.1);
            let auto_join =
                ["off", "join", "join_and_lock"][join.get_selection().unwrap_or(0) as usize % 3];
            let side = match side.as_ref().and_then(Choice::get_selection) {
                Some(1) => "exterior",
                _ => "interior",
            };
            let hosted = hosted.get_value();
            let mut made = 0;
            let mut errors = Vec::new();
            for w in &walls {
                match self.run(
                    "wall.create_by_segment",
                    json!({ "type_id": type_id, "target_id": w, "side": side, "hosted": hosted, "auto_join": auto_join }),
                ) {
                    Ok(v) => made += v.as_array().map_or(0, Vec::len),
                    Err(e) => errors.push(e),
                }
            }
            for r in &rooms {
                match self.run(
                    "wall.create_by_room",
                    json!({ "type_id": type_id, "room_id": r, "hosted": hosted, "auto_join": auto_join }),
                ) {
                    Ok(v) => made += v.as_array().map_or(0, Vec::len),
                    Err(e) => errors.push(e),
                }
            }
            self.refresh_scene(&frame);
            if errors.is_empty() {
                frame.set_status_text(&format!("{made} lining wall(s) made"), 0);
            } else {
                super::error(
                    &frame,
                    &format!("{made} made; not lined:\n{}", errors.join("\n")),
                );
            }
        }
        dialog.destroy();
    }

    /// Modify | Walls › Pick New Host: the next click picks the host.
    pub(crate) fn start_host_pick(&mut self) {
        let Some(store) = self.store_now() else {
            return;
        };
        let wall = self.selection().into_iter().find(|id| {
            store
                .get(*id)
                .and_then(reviera_core::walls::wall_recipe)
                .is_some()
        });
        let Some(wall) = wall else {
            return self
                .frame
                .set_status_text("Select the wall to host first", 0);
        };
        self.host_pick = Some(wall);
        self.frame
            .set_status_text("Pick New Host: click the wall it follows (Esc cancels)", 0);
    }

    pub(crate) fn host_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(wall) = self.host_pick.take() else {
            return;
        };
        let Some(host) = self.element_at(x, y, panel) else {
            return frame.set_status_text("No wall there", 0);
        };
        match self.run(
            "wall.set_host",
            json!({ "wall_id": wall, "host_wall_id": host }),
        ) {
            Ok(_) => frame.set_status_text("Hosted: the wall follows its new host", 0),
            Err(e) => super::error(&frame, &format!("Not hosted:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// Modify | Walls › Remove Hosted.
    pub(crate) fn remove_host(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        match self.run("wall.remove_host", json!({ "wall_ids": ids })) {
            Ok(_) => frame.set_status_text("No longer hosted", 0),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
    }

    /// Modify | Walls › Select Main Host.
    pub(crate) fn select_main_host(&mut self) {
        let frame = self.frame;
        let Some(&wall) = self.selection().first() else {
            return;
        };
        match self.run("wall.select_main_host", json!({ "wall_id": wall })) {
            Ok(v) => match v.as_u64() {
                Some(h) => self.select_ids(&[ElementId(h)], Combine::Replace),
                None => frame.set_status_text("The wall is not hosted", 0),
            },
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
