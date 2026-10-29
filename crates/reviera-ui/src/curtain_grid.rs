// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Edit Grid (ARC-CW-004, ARC-CW-006): a selected curtain wall's grid lines, their segments and
//! its swapped panels, in two grids.
//!
//! Lines: each grid line with its direction (U vertical, V horizontal), position, whether it was
//! placed by hand, and its segments as a row of 1s and 0s from the start or base (0 removes a
//! segment, merging the panels either side). Lines of the type cannot move; manual lines can be
//! added, moved and deleted. Panels: a cell (column along, row up, from 0) and its panel: Glazed,
//! Solid, Empty or Door (a curtain-wall door). OK applies everything in one step.

use reviera_core::curtain::{self, Dir, ManualGrid, Panel, PanelOverride, SegmentOff};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const LINE_COLS: [&str; 4] = ["Direction", "Position", "Manual", "Segments"];
const PANEL_COLS: [&str; 3] = ["Column", "Row", "Panel"];
const PANELS: [(Panel, &str); 5] = [
    (Panel::Glazed, "Glazed"),
    (Panel::Solid, "Solid"),
    (Panel::Empty, "Empty"),
    (Panel::Door, "Door"),
    (Panel::Vent, "Vent"),
];

impl Gui {
    /// Modify | Curtain Walls › Edit Grid.
    pub(crate) fn edit_curtain_grid(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let Some((id, r, g)) = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            Some((
                id,
                curtain::curtain_recipe(e)?,
                curtain::regenerate(&store, e).ok()?,
            ))
        }) else {
            return frame.set_status_text("Select a curtain wall", 0);
        };
        let dialog = Dialog::builder(&frame, "Edit Grid")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let note = StaticText::builder(&dialog)
            .with_label(&format!(
                "{} long, {} high, {} × {} cells. Lines: U vertical (position along from the start), \
                 V horizontal (up from the base); Segments 1 or 0 from the start or base, 0 removed. \
                 Panels: Column and Row from 0; Glazed, Solid, Empty or Door.",
                self.format_length(g.length()),
                self.format_length(g.z[1] - g.z[0]),
                g.u_lines.len() + 1,
                g.v_lines.len() + 1
            ))
            .build();
        note.wrap(560);
        sizer.add(&note, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let lines = Grid::builder(&dialog).build();
        lines.create_grid(0, LINE_COLS.len() as i32, GridSelectionMode::Cells);
        for (i, h) in LINE_COLS.iter().enumerate() {
            lines.set_col_label_value(i as i32, h);
        }
        lines.set_col_format_bool(2);
        let rows: Vec<(Dir, f64)> = g
            .u_lines
            .iter()
            .map(|x| (Dir::U, *x))
            .chain(g.v_lines.iter().map(|z| (Dir::V, *z)))
            .collect();
        lines.append_rows(rows.len() as i32, true);
        for (k, (dir, at)) in rows.iter().enumerate() {
            let k = k as i32;
            let n = match dir {
                Dir::U => g.v_lines.len() + 1,
                Dir::V => g.u_lines.len() + 1,
            };
            let segs: String = (0..n)
                .map(|s| {
                    let off = r
                        .removed
                        .iter()
                        .any(|o| o.dir == *dir && (o.at - at).abs() < 1e-6 && o.segment == s);
                    if off { '0' } else { '1' }
                })
                .collect();
            let manual = r
                .grids
                .iter()
                .any(|m| m.dir == *dir && (m.at - at).abs() < 1e-6);
            lines.set_cell_value(k, 0, if *dir == Dir::U { "U" } else { "V" });
            lines.set_cell_value(k, 1, &self.format_length(*at));
            lines.set_cell_value(k, 2, if manual { "1" } else { "" });
            lines.set_cell_value(k, 3, &segs);
        }
        lines.set_col_size(0, 90);
        lines.set_col_size(1, 120);
        lines.set_col_size(2, 70);
        lines.set_col_size(3, 160);
        lines.set_min_size(Size::new(540, 180));
        sizer.add(&lines, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let line_buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let add = Button::builder(&dialog).with_label("Add Line").build();
        let delete = Button::builder(&dialog).with_label("Delete Line").build();
        line_buttons.add(&add, 0, SizerFlag::All, 4);
        line_buttons.add(&delete, 0, SizerFlag::All, 4);
        sizer.add_sizer(&line_buttons, 0, SizerFlag::Left | SizerFlag::Right, 6);
        let half = self.format_length((g.z[1] - g.z[0]) / 2.0);
        add.on_click(move |_| {
            let k = lines.get_number_rows();
            lines.append_rows(1, true);
            lines.set_cell_value(k, 0, "V");
            lines.set_cell_value(k, 1, &half);
            lines.set_cell_value(k, 2, "1");
            lines.set_grid_cursor(k, 1);
        });
        delete.on_click(move |_| {
            let k = lines.get_grid_cursor_row();
            // Only manual lines can go.
            if k >= 0 && lines.get_cell_value(k, 2).trim() == "1" {
                lines.delete_rows(k, 1, true);
            }
        });
        let panels = Grid::builder(&dialog).build();
        panels.create_grid(
            r.panels.len() as i32,
            PANEL_COLS.len() as i32,
            GridSelectionMode::Cells,
        );
        for (i, h) in PANEL_COLS.iter().enumerate() {
            panels.set_col_label_value(i as i32, h);
        }
        for (k, o) in r.panels.iter().enumerate() {
            let k = k as i32;
            panels.set_cell_value(k, 0, &o.cell[0].to_string());
            panels.set_cell_value(k, 1, &o.cell[1].to_string());
            let name = PANELS
                .iter()
                .find(|p| p.0 == o.panel)
                .map_or("Glazed", |p| p.1);
            panels.set_cell_value(k, 2, name);
        }
        panels.set_min_size(Size::new(400, 110));
        sizer.add(&panels, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let panel_buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let add_panel = Button::builder(&dialog).with_label("Add Panel").build();
        let remove_panel = Button::builder(&dialog).with_label("Remove Panel").build();
        panel_buttons.add(&add_panel, 0, SizerFlag::All, 4);
        panel_buttons.add(&remove_panel, 0, SizerFlag::All, 4);
        sizer.add_sizer(&panel_buttons, 0, SizerFlag::Left | SizerFlag::Right, 6);
        add_panel.on_click(move |_| {
            let k = panels.get_number_rows();
            panels.append_rows(1, true);
            panels.set_cell_value(k, 0, "0");
            panels.set_cell_value(k, 1, "0");
            panels.set_cell_value(k, 2, "Door");
        });
        remove_panel.on_click(move |_| {
            if panels.get_number_rows() > 0 {
                panels.delete_rows(panels.get_grid_cursor_row().max(0), 1, true);
            }
        });
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
            lines.save_edit_control_value();
            panels.save_edit_control_value();
            match self.curtain_layout(&r, &lines, &panels).and_then(|args| {
                let mut args = args;
                args["curtain_id"] = json!(id);
                self.run("curtain.set_layout", args)
            }) {
                Ok(_) => {
                    self.properties_stale = true;
                    self.refresh_scene(&frame);
                    frame.set_status_text("Curtain grid changed", 0);
                    break;
                }
                Err(e) => super::error(&frame, &format!("The grid was not changed:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// The dialog's grids as `curtain.set_layout` arguments.
    fn curtain_layout(
        &mut self,
        r: &curtain::CurtainRecipe,
        lines: &Grid,
        panels: &Grid,
    ) -> Result<serde_json::Value, String> {
        let mut grids: Vec<ManualGrid> = Vec::new();
        let mut removed: Vec<SegmentOff> = Vec::new();
        let mut next = r.grids.iter().map(|g| g.key + 1).max().unwrap_or(1);
        for k in 0..lines.get_number_rows() {
            let cell = |c: i32| lines.get_cell_value(k, c);
            let dir = match cell(0).trim().to_ascii_uppercase().as_str() {
                "U" => Dir::U,
                "V" => Dir::V,
                other => return Err(format!("line {}: `{other}` is not U or V", k + 1)),
            };
            let at = self
                .run(
                    "units.parse",
                    json!({ "text": cell(1), "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
                .ok_or_else(|| format!("line {}: `{}` is not a length", k + 1, cell(1)))?;
            if cell(2).trim() == "1" {
                let key = r
                    .grids
                    .iter()
                    .find(|g| g.dir == dir && (g.at - at).abs() < 1e-6)
                    .map(|g| g.key)
                    .unwrap_or_else(|| {
                        next += 1;
                        next - 1
                    });
                grids.push(ManualGrid { key, dir, at });
            }
            for (segment, c) in cell(3).trim().chars().enumerate() {
                if c == '0' {
                    removed.push(SegmentOff { dir, at, segment });
                }
            }
        }
        let mut overrides: Vec<PanelOverride> = Vec::new();
        for k in 0..panels.get_number_rows() {
            let cell = |c: i32| panels.get_cell_value(k, c);
            let n = |c: i32| {
                cell(c)
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("panel {}: Column and Row are numbers from 0", k + 1))
            };
            let panel = PANELS
                .iter()
                .find(|p| p.1.eq_ignore_ascii_case(cell(2).trim()))
                .map(|p| p.0)
                .ok_or_else(|| format!("panel {}: `{}` is not a panel", k + 1, cell(2)))?;
            overrides.push(PanelOverride {
                cell: [n(0)?, n(1)?],
                panel,
            });
        }
        Ok(json!({ "grids": grids, "removed": removed, "panels": overrides }))
    }

    /// Modify | Roofs › Sloped Glazing: the selected roofs get a Sloped Glazing type (made if the
    /// project has none).
    pub(crate) fn sloped_glazing(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let roofs: Vec<_> = self
            .selection()
            .into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::roofs::roof_recipe)
                    .is_some()
            })
            .collect();
        if roofs.is_empty() {
            return frame.set_status_text("Select roofs to glaze", 0);
        }
        let existing = store
            .iter()
            .find(|e| reviera_core::roofs::glazing::of_type(e).is_some())
            .map(|e| e.id());
        let ty = match existing {
            Some(t) => Ok(json!(t)),
            None => self.run(
                "roof.create_glazing_type",
                json!({ "name": "Sloped Glazing" }),
            ),
        };
        match ty.and_then(|t| {
            self.run(
                "element.set_type",
                json!({ "element_ids": roofs, "type_id": t }),
            )
        }) {
            Ok(_) => {
                self.properties_stale = true;
                self.refresh_scene(&frame);
                frame.set_status_text("Sloped glazing: change the roof's type back to undo", 0);
            }
            Err(e) => super::error(&frame, &format!("Not glazed:\n{e}")),
        }
    }
}
