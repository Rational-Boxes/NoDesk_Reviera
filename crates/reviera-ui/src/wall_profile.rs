// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Edit Profile (ARC-WALL-009): a straight wall's elevation outline as points in a grid.
//!
//! Each row is a point of a loop: loop 1 is the outline, higher loops are holes. `Along` is
//! measured from the wall's start and `Height` up from its base. Gable and Step fill the grid
//! with a common outline to adjust; Reset Profile restores the rectangle.

use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const COLS: [&str; 3] = ["Loop", "Along", "Height"];
const LOOP: i32 = 0;
const ALONG: i32 = 1;
const HEIGHT: i32 = 2;

fn fill(grid: &Grid, gui: &mut Gui, loops: &[Vec<[f64; 2]>]) {
    let rows: Vec<(usize, [f64; 2])> = loops
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.iter().map(move |p| (i + 1, *p)))
        .collect();
    let n = grid.get_number_rows();
    if n > 0 {
        grid.delete_rows(0, n, true);
    }
    grid.append_rows(rows.len() as i32, true);
    for (r, (k, p)) in rows.iter().enumerate() {
        let r = r as i32;
        grid.set_cell_value(r, LOOP, &k.to_string());
        grid.set_cell_value(r, ALONG, &gui.format_length(p[0]));
        grid.set_cell_value(r, HEIGHT, &gui.format_length(p[1]));
    }
}

impl Gui {
    /// Modify | Walls › Edit Profile.
    pub(crate) fn edit_wall_profile(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(wall) = self.selection().into_iter().find(|id| {
            store
                .get(*id)
                .and_then(reviera_core::walls::wall_recipe)
                .is_some_and(|r| r.through.is_empty())
        }) else {
            return frame.set_status_text("Select a straight wall to edit its profile", 0);
        };
        let loops: Vec<Vec<[f64; 2]>> =
            match self.run("wall.get_profile", json!({ "wall_id": wall })) {
                Ok(v) => serde_json::from_value(v).unwrap_or_default(),
                Err(e) => return frame.set_status_text(&e, 0),
            };
        let g = reviera_core::walls::regenerate(&store, store.get(wall).expect("selected"));
        let Ok(g) = g else { return };
        let (length, height) = (g.length(), g.height);

        let dialog = Dialog::builder(&frame, "Edit Profile")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let header = StaticText::builder(&dialog)
            .with_label(&format!(
                "The wall is {} long and {} high. Loop 1 is the outline; loops 2 and up are \
                 holes. Along is from the wall's start, Height up from its base. Points on the \
                 top or at the ends follow them when the wall changes.",
                self.format_length(length),
                self.format_length(height)
            ))
            .build();
        header.wrap(460);
        sizer.add(&header, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let grid = Grid::builder(&dialog).build();
        grid.create_grid(0, COLS.len() as i32, GridSelectionMode::Cells);
        for (i, h) in COLS.iter().enumerate() {
            grid.set_col_label_value(i as i32, h);
        }
        grid.set_col_size(LOOP, 70);
        grid.set_col_size(ALONG, 130);
        grid.set_col_size(HEIGHT, 130);
        grid.set_min_size(Size::new(420, 280));
        fill(&grid, self, &loops);
        sizer.add(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);

        let row_buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let button = |label: &str| {
            let b = Button::builder(&dialog).with_label(label).build();
            row_buttons.add(&b, 0, SizerFlag::All, 4);
            b
        };
        let insert = button("Insert");
        let delete = button("Delete");
        let gable = button("Gable");
        let step = button("Step");
        let rect = button("Rectangle");
        insert.on_click(move |_| {
            let r = grid
                .get_grid_cursor_row()
                .max(0)
                .min(grid.get_number_rows());
            let k = if r > 0 {
                grid.get_cell_value(r - 1, LOOP)
            } else {
                "1".to_owned()
            };
            grid.insert_rows(r, 1, true);
            grid.set_cell_value(r, LOOP, &k);
            grid.set_grid_cursor(r, ALONG);
        });
        delete.on_click(move |_| {
            if grid.get_number_rows() > 3 {
                grid.delete_rows(grid.get_grid_cursor_row().max(0), 1, true);
            }
        });
        // The presets' points, in the project's units.
        let (l, h) = (length, height);
        let mut text = |pts: &[[f64; 2]]| -> Vec<(String, String)> {
            pts.iter()
                .map(|p| (self.format_length(p[0]), self.format_length(p[1])))
                .collect()
        };
        let gable_pts = text(&[
            [0.0, 0.0],
            [l, 0.0],
            [l, h],
            [l / 2.0, h + l / 4.0],
            [0.0, h],
        ]);
        let step_pts = text(&[
            [0.0, 0.0],
            [l, 0.0],
            [l, h * 2.0 / 3.0],
            [l / 2.0, h * 2.0 / 3.0],
            [l / 2.0, h],
            [0.0, h],
        ]);
        let rect_pts = text(&[[0.0, 0.0], [l, 0.0], [l, h], [0.0, h]]);
        let presets = move |pts: &[(String, String)]| {
            let n = grid.get_number_rows();
            if n > 0 {
                grid.delete_rows(0, n, true);
            }
            grid.append_rows(pts.len() as i32, true);
            for (r, (a, z)) in pts.iter().enumerate() {
                grid.set_cell_value(r as i32, LOOP, "1");
                grid.set_cell_value(r as i32, ALONG, a);
                grid.set_cell_value(r as i32, HEIGHT, z);
            }
        };
        gable.on_click(move |_| presets(&gable_pts));
        step.on_click(move |_| presets(&step_pts));
        rect.on_click(move |_| presets(&rect_pts));
        sizer.add_sizer(&row_buttons, 0, SizerFlag::Left | SizerFlag::Right, 6);

        let ok = Button::builder(&dialog)
            .with_id(ID_OK)
            .with_label("OK")
            .build();
        let reset = Button::builder(&dialog).with_label("Reset Profile").build();
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("Cancel")
            .build();
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add(&reset, 0, SizerFlag::All, 6);
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        const RESET: i32 = 7301;
        reset.on_click(move |_| dialog.end_modal(RESET));

        loop {
            let answer = dialog.show_modal();
            if answer == RESET {
                match self.run("wall.reset_profile", json!({ "wall_ids": [wall] })) {
                    Ok(_) => self.refresh_scene(&frame),
                    Err(e) => frame.set_status_text(&e, 0),
                }
                break;
            }
            if answer != ID_OK {
                break;
            }
            grid.save_edit_control_value();
            match self.profile_loops(&grid).and_then(|loops| {
                self.run(
                    "wall.edit_profile",
                    json!({ "wall_id": wall, "loops": loops }),
                )
            }) {
                Ok(_) => {
                    self.properties_stale = true;
                    self.refresh_scene(&frame);
                    frame.set_status_text("Profile changed", 0);
                    break;
                }
                Err(e) => super::error(&frame, &format!("The profile was not changed:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// The grid's rows as loops.
    fn profile_loops(&mut self, grid: &Grid) -> Result<Vec<Vec<[f64; 2]>>, String> {
        let mut loops: std::collections::BTreeMap<u32, Vec<[f64; 2]>> = Default::default();
        for r in 0..grid.get_number_rows() {
            let cell = |c: i32| grid.get_cell_value(r, c);
            let k: u32 = cell(LOOP)
                .trim()
                .parse()
                .map_err(|_| format!("row {}: the loop is a number (1 for the outline)", r + 1))?;
            let mut p = [0.0; 2];
            for (i, c) in [ALONG, HEIGHT].into_iter().enumerate() {
                p[i] = self
                    .run(
                        "units.parse",
                        json!({ "text": cell(c), "data_type": "length" }),
                    )
                    .ok()
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| format!("row {}: `{}` is not a length", r + 1, cell(c)))?;
            }
            loops.entry(k).or_default().push(p);
        }
        if !loops.contains_key(&1) {
            return Err("loop 1, the outline, has no points".to_owned());
        }
        Ok(loops.into_values().collect())
    }
}
