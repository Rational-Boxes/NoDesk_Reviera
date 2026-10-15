// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Edit Stack (ARC-STKW-001): the rows of a stacked wall type in a grid, top row first.
//!
//! Each row is a basic wall type (by name), its height (`Variable` for the one row that takes
//! the rest of the wall's height), its offset from the offset reference (toward the exterior)
//! and whether it is flipped. On a wall whose type is not stacked yet, OK makes a new stacked
//! wall type from the rows and gives it to the selected walls. Break Up turns stacked walls into
//! basic walls.

use reviera_core::ElementId;
use reviera_core::walls::stacked::{self, OffsetReference};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

/// A row as shown: type name, height (`None` for Variable), offset, flip.
type RowView = (String, Option<f64>, f64, bool);

const COLS: [&str; 4] = ["Type", "Height", "Offset", "Flip"];
const TYPE: i32 = 0;
const HEIGHT: i32 = 1;
const OFFSET: i32 = 2;
const FLIP: i32 = 3;

const REFERENCES: [(OffsetReference, &str); 6] = [
    (OffsetReference::WallCenterline, "Wall Centerline"),
    (OffsetReference::CoreCenterline, "Core Centerline"),
    (OffsetReference::FinishFaceExterior, "Finish Face: Exterior"),
    (OffsetReference::FinishFaceInterior, "Finish Face: Interior"),
    (OffsetReference::CoreFaceExterior, "Core Face: Exterior"),
    (OffsetReference::CoreFaceInterior, "Core Face: Interior"),
];

fn swap_rows(grid: &Grid, a: i32, b: i32) {
    for c in 0..COLS.len() as i32 {
        let (va, vb) = (grid.get_cell_value(a, c), grid.get_cell_value(b, c));
        grid.set_cell_value(a, c, &vb);
        grid.set_cell_value(b, c, &va);
    }
}

impl Gui {
    /// Modify | Walls › Edit Stack.
    pub(crate) fn edit_stack(&mut self) {
        let frame = self.frame;
        let walls: Vec<ElementId> = self.selection();
        let Some(store) = self.store_now() else {
            return;
        };
        let Some((wall, type_id)) = walls.iter().find_map(|&id| {
            let e = store.get(id)?;
            reviera_core::walls::wall_recipe(e)?;
            let g = reviera_core::GlobalId::parse(e.param("@type")?.as_str()?)?;
            Some((id, store.find_by_guid(&g)?))
        }) else {
            return frame.set_status_text("Select a wall to edit its stack", 0);
        };
        let _ = wall;
        let t = store.get(type_id).expect("found");
        let current = stacked::of_type(t);
        let type_name = t.name.clone().unwrap_or_default();
        // Basic wall types by name.
        let basic: Vec<(String, ElementId)> = store
            .iter()
            .filter(|e| {
                e.class == "IfcWallType"
                    && reviera_core::walls::type_width(e).is_some()
                    && stacked::of_type(e).is_none()
            })
            .map(|e| (e.name.clone().unwrap_or_default(), e.id()))
            .collect();
        let name_of = |guid: &str| {
            reviera_core::GlobalId::parse(guid)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|id| store.get(id))
                .and_then(|e| e.name.clone())
                .unwrap_or_default()
        };
        let (reference, rows): (OffsetReference, Vec<RowView>) = match &current {
            Some(s) => (
                s.offset_reference,
                s.rows
                    .iter()
                    .map(|r| (name_of(&r.wall_type), r.height, r.offset, r.flip))
                    .collect(),
            ),
            None => (
                OffsetReference::WallCenterline,
                vec![(type_name.clone(), None, 0.0, false)],
            ),
        };
        let title = match &current {
            Some(_) => format!("Edit Stack — {type_name}"),
            None => "New Stacked Wall Type".to_owned(),
        };
        let dialog = Dialog::builder(&frame, &title)
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let names: Vec<&str> = basic.iter().map(|(n, _)| n.as_str()).collect();
        let header = StaticText::builder(&dialog)
            .with_label(&format!(
                "Rows from the top. Type is a basic wall type ({}). Height is a length, or \
                 Variable for the one row that takes the rest of the wall's height. Offset is \
                 from the offset reference, positive toward the exterior.",
                names.join(", ")
            ))
            .build();
        header.wrap(560);
        sizer.add(&header, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let line = BoxSizer::builder(Orientation::Horizontal).build();
        let mut name_box = None;
        if current.is_none() {
            let l = StaticText::builder(&dialog).with_label("Name").build();
            let t = TextCtrl::builder(&dialog)
                .with_value(&format!("{type_name} Stacked"))
                .build();
            line.add(&l, 0, SizerFlag::AlignCenterVertical | SizerFlag::All, 4);
            line.add(&t, 1, SizerFlag::All, 4);
            name_box = Some(t);
        }
        let l = StaticText::builder(&dialog).with_label("Offset").build();
        let offset = Choice::builder(&dialog)
            .with_choices(REFERENCES.iter().map(|r| r.1.to_owned()).collect())
            .build();
        offset.set_selection(
            REFERENCES
                .iter()
                .position(|r| r.0 == reference)
                .unwrap_or(0) as u32,
        );
        line.add(&l, 0, SizerFlag::AlignCenterVertical | SizerFlag::All, 4);
        line.add(&offset, 1, SizerFlag::All, 4);
        sizer.add_sizer(
            &line,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            6,
        );

        let grid = Grid::builder(&dialog).build();
        grid.create_grid(
            rows.len() as i32,
            COLS.len() as i32,
            GridSelectionMode::Cells,
        );
        for (i, h) in COLS.iter().enumerate() {
            grid.set_col_label_value(i as i32, h);
        }
        grid.set_col_format_bool(FLIP);
        for (r, (name, height, off, flip)) in rows.iter().enumerate() {
            let r = r as i32;
            grid.set_cell_value(r, TYPE, name);
            grid.set_cell_value(
                r,
                HEIGHT,
                &height.map_or("Variable".to_owned(), |h| self.format_length(h)),
            );
            grid.set_cell_value(r, OFFSET, &self.format_length(*off));
            grid.set_cell_value(r, FLIP, if *flip { "1" } else { "" });
        }
        grid.set_col_size(TYPE, 200);
        grid.set_col_size(HEIGHT, 110);
        grid.set_col_size(OFFSET, 110);
        grid.set_col_size(FLIP, 60);
        grid.set_min_size(Size::new(620, 200));
        sizer.add(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);

        let row_buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let button = |label: &str| {
            let b = Button::builder(&dialog).with_label(label).build();
            row_buttons.add(&b, 0, SizerFlag::All, 4);
            b
        };
        let insert = button("Insert");
        let delete = button("Delete");
        let up = button("Up");
        let down = button("Down");
        let variable = button("Variable");
        let first = names.first().map(|s| s.to_string()).unwrap_or_default();
        let current_row = move |g: &Grid| g.get_grid_cursor_row().max(0);
        insert.on_click(move |_| {
            let r = current_row(&grid).min(grid.get_number_rows());
            grid.insert_rows(r, 1, true);
            grid.set_cell_value(r, TYPE, &first);
            grid.set_cell_value(r, HEIGHT, "1000");
            grid.set_cell_value(r, OFFSET, "0");
            grid.set_grid_cursor(r, TYPE);
        });
        delete.on_click(move |_| {
            if grid.get_number_rows() > 1 {
                grid.delete_rows(current_row(&grid), 1, true);
            }
        });
        up.on_click(move |_| {
            let r = current_row(&grid);
            if r > 0 {
                swap_rows(&grid, r, r - 1);
                grid.set_grid_cursor(r - 1, grid.get_grid_cursor_col().max(0));
            }
        });
        down.on_click(move |_| {
            let r = current_row(&grid);
            if r + 1 < grid.get_number_rows() {
                swap_rows(&grid, r, r + 1);
                grid.set_grid_cursor(r + 1, grid.get_grid_cursor_col().max(0));
            }
        });
        // Variable: this row takes the rest; the row that was variable gets a height.
        variable.on_click(move |_| {
            let r = current_row(&grid);
            for k in 0..grid.get_number_rows() {
                if k != r
                    && grid
                        .get_cell_value(k, HEIGHT)
                        .trim()
                        .eq_ignore_ascii_case("variable")
                {
                    grid.set_cell_value(k, HEIGHT, "1000");
                }
            }
            grid.set_cell_value(r, HEIGHT, "Variable");
        });
        sizer.add_sizer(&row_buttons, 0, SizerFlag::Left | SizerFlag::Right, 6);

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

        loop {
            if dialog.show_modal() != ID_OK {
                break;
            }
            grid.save_edit_control_value();
            let reference = REFERENCES
                .get(offset.get_selection().unwrap_or(0) as usize)
                .map_or(OffsetReference::WallCenterline, |r| r.0);
            let result = self.stack_rows(&grid, &basic).and_then(|rows| {
                let reference = serde_json::to_value(reference).expect("serialises");
                match (&current, &name_box) {
                    (Some(_), _) => self
                        .run(
                            "stackedwall.set_rows",
                            json!({ "type_id": type_id, "offset_reference": reference, "rows": rows }),
                        )
                        .map(|_| ()),
                    (None, n) => {
                        let name = n.as_ref().map(|t| t.get_value()).unwrap_or_default();
                        let new = self.run(
                            "stackedwall.create_type",
                            json!({ "name": name, "offset_reference": reference, "rows": rows }),
                        )?;
                        self.run(
                            "element.set_type",
                            json!({ "element_ids": walls, "type_id": new }),
                        )
                        .map(|_| ())
                    }
                }
            });
            match result {
                Ok(()) => {
                    self.properties_stale = true;
                    self.refresh_scene(&frame);
                    frame.set_status_text("Stack changed", 0);
                    break;
                }
                Err(e) => super::error(&frame, &format!("The stack was not changed:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// The grid's rows as `stackedwall.set_rows` rows.
    fn stack_rows(
        &mut self,
        grid: &Grid,
        basic: &[(String, ElementId)],
    ) -> Result<Vec<serde_json::Value>, String> {
        let mut out = Vec::new();
        for r in 0..grid.get_number_rows() {
            let cell = |c: i32| grid.get_cell_value(r, c);
            let name = cell(TYPE);
            let ty = basic
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
                .map(|(_, id)| *id)
                .ok_or_else(|| format!("row {}: `{name}` is not a basic wall type", r + 1))?;
            let length = |gui: &mut Gui, text: &str| {
                gui.run(
                    "units.parse",
                    json!({ "text": text, "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
                .ok_or_else(|| format!("row {}: `{text}` is not a length", r + 1))
            };
            let h = cell(HEIGHT);
            let height = if h.trim().eq_ignore_ascii_case("variable") {
                None
            } else {
                Some(length(self, &h)?)
            };
            let offset = match cell(OFFSET).trim() {
                "" => 0.0,
                t => length(self, t)?,
            };
            let flip = matches!(cell(FLIP).trim(), "1" | "true" | "yes");
            out.push(
                json!({ "wall_type_id": ty, "height": height, "offset": offset, "flip": flip }),
            );
        }
        Ok(out)
    }

    /// Modify | Walls › Break Up.
    pub(crate) fn break_up(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let ids: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .is_some_and(|e| stacked::of_wall(&store, e).is_some())
            })
            .collect();
        if ids.is_empty() {
            return frame.set_status_text("Select stacked walls to break up", 0);
        }
        match self.run("wall.break_up", json!({ "stacked_wall_ids": ids })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Broken up into basic walls", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
