// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Edit Assembly (ARC-CMPD-002): the layers of a wall type in a grid, exterior at the top.
//!
//! Each row is a layer: its function, priority, material, thickness, whether it wraps and
//! whether it is in the core (the core's rows must be together). Insert, Delete, Up and Down
//! edit the rows. OK validates the layers (`compound.set_layers`) and applies them to the type
//! in one step, so every wall of the type changes; a problem is shown and the dialog stays open.
//! Cancel changes nothing.

use reviera_core::ElementId;
use reviera_core::compound::{Compound, Function, Layer};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const COLS: [&str; 6] = [
    "Function",
    "Priority",
    "Material",
    "Thickness",
    "Wraps",
    "Core",
];
const FUNCTION: i32 = 0;
const PRIORITY: i32 = 1;
const MATERIAL: i32 = 2;
const THICKNESS: i32 = 3;
const WRAPS: i32 = 4;
const CORE: i32 = 5;

fn yes(v: &str) -> bool {
    matches!(v.trim(), "1" | "true" | "yes" | "Yes" | "x" | "X")
}

/// Swaps two rows' values.
fn swap_rows(grid: &Grid, a: i32, b: i32) {
    for c in 0..COLS.len() as i32 {
        let (va, vb) = (grid.get_cell_value(a, c), grid.get_cell_value(b, c));
        grid.set_cell_value(a, c, &vb);
        grid.set_cell_value(b, c, &va);
    }
}

impl Gui {
    /// The wall type Edit Assembly works on: the selected walls' (the first one's), else the
    /// Wall tool's.
    fn assembly_type(&mut self) -> Option<ElementId> {
        let store = self.store_now()?;
        let selected = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            reviera_core::walls::wall_recipe(e)?;
            let guid = e.param("@type")?.as_str()?.to_owned();
            store.find_by_guid(&reviera_core::GlobalId::parse(&guid)?)
        });
        selected.or_else(|| {
            self.tool.as_ref()?;
            self.wall_type_row
        })
    }

    /// Modify | Walls › Edit Assembly.
    pub(crate) fn edit_assembly(&mut self) {
        let frame = self.frame;
        let Some(type_id) = self.assembly_type() else {
            return frame.set_status_text("Select a wall to edit its type's layers", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(t) = store.get(type_id) else { return };
        let Some(c) = reviera_core::compound::of_wall_type(t) else {
            return;
        };
        let name = t.name.clone().unwrap_or_default();
        let walls = reviera_core::types::instances(&store, t).len();

        let dialog = Dialog::builder(&frame, &format!("Edit Assembly — {name}"))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let header = StaticText::builder(&dialog)
            .with_label(&format!(
                "Wall type {name} ({walls} walls) · total thickness {} · exterior at the top. \
                 Functions: Structure, Substrate, Thermal/Air, Finish 1, Finish 2, Membrane. \
                 Priority 1 is the strongest; tick Core for the core's layers.",
                self.format_length(c.thickness())
            ))
            .build();
        header.wrap(640);
        sizer.add(&header, 0, SizerFlag::All | SizerFlag::Expand, 10);

        let grid = Grid::builder(&dialog).build();
        grid.create_grid(
            c.layers.len() as i32,
            COLS.len() as i32,
            GridSelectionMode::Cells,
        );
        for (i, h) in COLS.iter().enumerate() {
            grid.set_col_label_value(i as i32, h);
        }
        grid.set_col_format_bool(WRAPS);
        grid.set_col_format_bool(CORE);
        for (r, l) in c.layers.iter().enumerate() {
            let r = r as i32;
            grid.set_cell_value(r, FUNCTION, l.function.name());
            grid.set_cell_value(r, PRIORITY, &l.priority().to_string());
            grid.set_cell_value(r, MATERIAL, &l.material);
            grid.set_cell_value(r, THICKNESS, &self.format_length(l.thickness));
            grid.set_cell_value(r, WRAPS, if l.wraps { "1" } else { "" });
            let core = (c.core[0]..c.core[1]).contains(&(r as usize));
            grid.set_cell_value(r, CORE, if core { "1" } else { "" });
        }
        grid.set_col_size(FUNCTION, 130);
        grid.set_col_size(PRIORITY, 70);
        grid.set_col_size(MATERIAL, 200);
        grid.set_col_size(THICKNESS, 100);
        grid.set_col_size(WRAPS, 60);
        grid.set_col_size(CORE, 60);
        grid.set_min_size(Size::new(700, 260));
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
        let current = move |g: &Grid| g.get_grid_cursor_row().max(0);
        insert.on_click(move |_| {
            let r = current(&grid).min(grid.get_number_rows());
            grid.insert_rows(r, 1, true);
            grid.set_cell_value(r, FUNCTION, "Finish 1");
            grid.set_cell_value(r, PRIORITY, "4");
            grid.set_cell_value(r, THICKNESS, "13");
            grid.set_grid_cursor(r, FUNCTION);
        });
        delete.on_click(move |_| {
            if grid.get_number_rows() > 1 {
                grid.delete_rows(current(&grid), 1, true);
            }
        });
        up.on_click(move |_| {
            let r = current(&grid);
            if r > 0 {
                swap_rows(&grid, r, r - 1);
                grid.set_grid_cursor(r - 1, grid.get_grid_cursor_col().max(0));
            }
        });
        down.on_click(move |_| {
            let r = current(&grid);
            if r + 1 < grid.get_number_rows() {
                swap_rows(&grid, r, r + 1);
                grid.set_grid_cursor(r + 1, grid.get_grid_cursor_col().max(0));
            }
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
            match self.apply_assembly(type_id, &grid) {
                Ok(()) => {
                    self.properties_stale = true;
                    self.refresh_scene(&frame);
                    frame.set_status_text(&format!("{name}: layers changed"), 0);
                    break;
                }
                Err(e) => super::error(&frame, &format!("The layers were not changed:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// Reads the grid's rows and gives the type those layers.
    fn apply_assembly(&mut self, type_id: ElementId, grid: &Grid) -> Result<(), String> {
        let rows = grid.get_number_rows();
        let mut layers = Vec::new();
        let mut core: Vec<usize> = Vec::new();
        for r in 0..rows {
            let cell = |c: i32| grid.get_cell_value(r, c);
            let function = Function::parse(&cell(FUNCTION))
                .ok_or_else(|| format!("row {}: `{}` is not a function", r + 1, cell(FUNCTION)))?;
            let priority = match cell(PRIORITY).trim() {
                "" => None,
                p => Some(
                    p.parse::<u8>()
                        .map_err(|_| format!("row {}: the priority is a number 1 to 9", r + 1))?,
                ),
            };
            let thickness = self
                .run(
                    "units.parse",
                    json!({ "text": cell(THICKNESS), "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
                .ok_or_else(|| format!("row {}: `{}` is not a length", r + 1, cell(THICKNESS)))?;
            if yes(&cell(CORE)) {
                core.push(r as usize);
            }
            layers.push(Layer {
                function,
                priority: (function != Function::Membrane)
                    .then_some(priority)
                    .flatten(),
                material: cell(MATERIAL).trim().to_owned(),
                thickness,
                wraps: yes(&cell(WRAPS)),
                structural_material: false,
                variable: false,
            });
        }
        // The core: the ticked rows, which must be together; none ticked is an empty core
        // (placed after the exterior layers that are not finishes).
        let core = match (core.first(), core.last()) {
            (Some(&a), Some(&b)) => {
                if b + 1 - a != core.len() {
                    return Err("the core's layers must be next to each other".to_owned());
                }
                [a, b + 1]
            }
            _ => [0, 0],
        };
        let c = Compound { layers, core };
        c.validate().map_err(|e| e.message)?;
        let layers: Vec<serde_json::Value> = c
            .layers
            .iter()
            .map(|l| {
                json!({
                    "function": l.function.name(),
                    "priority": l.priority,
                    "material": l.material,
                    "thickness": l.thickness,
                    "wraps": l.wraps,
                })
            })
            .collect();
        self.run(
            "compound.set_layers",
            json!({
                "type_id": type_id, "layers": layers,
                "core_start": c.core[0], "core_end": c.core[1],
            }),
        )
        .map(|_| ())
    }
}
