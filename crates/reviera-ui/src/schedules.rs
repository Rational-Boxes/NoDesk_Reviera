// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Schedule views in the window (Ch 5 §5.14, DOC-SCH-001, 002, 005, 010, 012).
//!
//! A schedule opens in a tab as a grid: one column per field, one row per element (or group),
//! and the grand totals.
//! - **Editing.** Editing a cell writes the parameter (undoable); computed cells are read-only
//!   and grey.
//! - **Selection.** Selecting a row selects its elements.
//! - **Commands.** View › Schedules creates one for a category. While a schedule is open, the
//!   Modify Schedule tab sets its fields and sorting, itemizes, and exports CSV.

use reviera_core::ElementId;
use reviera_core::schedules;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

/// A schedule view: its grid and what each row and column is.
pub struct ScheduleState {
    pub grid: Grid,
    /// The elements of each data row.
    pub rows: Vec<Vec<ElementId>>,
    pub fields: Vec<String>,
    /// The grid is being filled by the window, not edited by the user.
    pub quiet: bool,
}

impl std::fmt::Debug for ScheduleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduleState")
            .field("rows", &self.rows.len())
            .field("fields", &self.fields)
            .finish()
    }
}

/// Categories that can be scheduled from the window, with their names.
const CATEGORIES: &[(&str, &str)] = &[
    ("cat.doors", "Doors"),
    ("cat.windows", "Windows"),
    ("cat.rooms", "Rooms"),
    ("cat.walls", "Walls"),
    ("cat.floors", "Floors"),
    ("cat.ceilings", "Ceilings"),
    ("cat.roofs", "Roofs"),
];

impl Gui {
    /// Whether a model element is a schedule.
    pub(crate) fn is_schedule(
        &self,
        doc: reviera_app::DocumentId,
        element: Option<ElementId>,
    ) -> bool {
        element
            .and_then(|e| {
                self.app
                    .document(doc)?
                    .doc
                    .store()
                    .get(e)
                    .map(schedules::is_schedule)
            })
            .unwrap_or(false)
    }

    /// The grid of a new schedule view in `panel`.
    pub(crate) fn schedule_grid(panel: &Panel) -> ScheduleState {
        let grid = Grid::builder(panel).build();
        grid.create_grid(0, 0, GridSelectionMode::Rows);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&grid, 1, SizerFlag::Expand | SizerFlag::All, 0);
        panel.set_sizer(sizer, true);
        ScheduleState {
            grid,
            rows: Vec::new(),
            fields: Vec::new(),
            quiet: false,
        }
    }

    /// Fills a schedule view's grid from its table.
    pub(crate) fn fill_schedule(&mut self, view_id: u32) {
        let Some(v) = self.view(view_id) else { return };
        let (doc, Some(element)) = (v.doc, v.element) else {
            return;
        };
        let table = match self.app.schedule_table(doc, element) {
            Ok(t) => t,
            Err(e) => return self.frame.set_status_text(&e.message, 0),
        };
        let Some(v) = self.views.iter_mut().find(|v| v.id == view_id) else {
            return;
        };
        let Some(s) = &mut v.schedule else { return };
        s.quiet = true;
        let g = &s.grid;
        g.begin_batch();
        let (rows, cols) = (g.get_number_rows(), g.get_number_cols());
        if rows > 0 {
            g.delete_rows(0, rows, true);
        }
        if cols > 0 {
            g.delete_cols(0, cols, true);
        }
        let n_rows = table.rows.len() + usize::from(table.totals.is_some());
        g.append_cols(table.headings.len() as i32, true);
        g.append_rows(n_rows as i32, true);
        for (c, h) in table.headings.iter().enumerate() {
            g.set_col_label_value(c as i32, h);
        }
        let grey = Colour::new(110, 110, 110, 255);
        for (r, row) in table.rows.iter().enumerate() {
            g.set_row_label_value(r as i32, &(r + 1).to_string());
            for (c, cell) in row.cells.iter().enumerate() {
                let (r, c) = (r as i32, c as i32);
                g.set_cell_value(r, c, &cell.text);
                g.set_read_only(r, c, !cell.editable);
                if !cell.editable {
                    g.set_cell_text_colour(r, c, &grey);
                }
            }
        }
        if let Some(totals) = &table.totals {
            let r = table.rows.len() as i32;
            g.set_row_label_value(r, "");
            for (c, text) in totals.iter().enumerate() {
                g.set_cell_value(r, c as i32, text);
                g.set_read_only(r, c as i32, true);
            }
        }
        g.auto_size_columns(false);
        g.end_batch();
        s.rows = table.rows.iter().map(|r| r.elements.clone()).collect();
        s.fields = table.fields;
        s.quiet = false;
    }

    /// Refills every open schedule (the model changed).
    pub(crate) fn refresh_schedules(&mut self) {
        let ids: Vec<u32> = self
            .views
            .iter()
            .filter(|v| v.schedule.is_some())
            .map(|v| v.id)
            .collect();
        for id in ids {
            self.fill_schedule(id);
        }
    }

    /// A cell was edited in view `view_id`: writes it to the model.
    pub(crate) fn schedule_cell_changed(&mut self, view_id: u32, row: i32, col: i32) {
        let frame = self.frame;
        let Some(s) = self.view(view_id).and_then(|v| v.schedule.as_ref()) else {
            return;
        };
        if s.quiet {
            return;
        }
        let (Some(elements), Some(field)) = (
            s.rows.get(row as usize).cloned(),
            s.fields.get(col as usize).cloned(),
        ) else {
            return;
        };
        let value = s.grid.get_cell_value(row, col);
        if let Err(e) = self.run(
            "schedule.set_cell",
            json!({ "element_ids": elements, "field": field, "value": value }),
        ) {
            error(&frame, &format!("The value was not changed:\n{e}"));
        }
        // Every view, including this schedule, shows the change.
        self.refresh_scene(&frame);
    }

    /// A row was selected: its elements become the selection.
    pub(crate) fn schedule_row_selected(&mut self, view_id: u32, row: i32) {
        let Some(elements) = self
            .view(view_id)
            .and_then(|v| v.schedule.as_ref())
            .filter(|s| !s.quiet)
            .and_then(|s| s.rows.get(row as usize).cloned())
        else {
            return;
        };
        if self
            .run("selection.set", json!({ "ids": elements }))
            .is_ok()
        {
            self.properties_stale = true;
            self.update_ribbon_context();
        }
    }

    /// View › Schedules: a new schedule of a chosen category, opened.
    pub(crate) fn new_schedule(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let names: Vec<&str> = CATEGORIES.iter().map(|c| c.1).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Category:", "New Schedule", &names).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(&(key, _)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| CATEGORIES.get(i))
        else {
            return;
        };
        match self.run("schedule.create", json!({ "category": key })) {
            Ok(v) => {
                self.browser_stale = true;
                self.open_view(doc, v.as_u64().map(ElementId));
            }
            Err(e) => error(&frame, &format!("The schedule was not created:\n{e}")),
        }
    }

    /// The active schedule view's schedule and definition.
    fn active_schedule(&self) -> Option<(ElementId, schedules::ScheduleDef)> {
        let v = self.active_view().filter(|v| v.schedule.is_some())?;
        let e = v.element?;
        let def = schedules::schedule_def(self.app.document(v.doc)?.doc.store().get(e)?)?;
        Some((e, def))
    }

    /// Modify Schedule › Fields: choose the columns.
    pub(crate) fn schedule_fields(&mut self) {
        let frame = self.frame;
        let Some((id, def)) = self.active_schedule() else {
            return frame.set_status_text("Open a schedule to choose its fields", 0);
        };
        let Ok(list) = self.run("schedule.fields", json!({ "category": def.category })) else {
            return;
        };
        let fields: Vec<schedules::FieldInfo> = serde_json::from_value(list).unwrap_or_default();
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        let dialog = MultiChoiceDialog::builder(
            &frame,
            "Fields to show (columns keep their order; new ones are added at the end):",
            "Schedule Fields",
            &names,
        )
        .build();
        let current: Vec<i32> = fields
            .iter()
            .enumerate()
            .filter(|(_, f)| def.fields.iter().any(|d| d.key == f.key))
            .map(|(i, _)| i as i32)
            .collect();
        dialog.set_selections(&current);
        if dialog.show_modal() != ID_OK {
            return;
        }
        let chosen: Vec<String> = dialog
            .get_selections()
            .into_iter()
            .filter_map(|i| fields.get(i as usize).map(|f| f.key.clone()))
            .collect();
        // Kept fields keep their order; new ones follow.
        let mut keys: Vec<String> = def
            .fields
            .iter()
            .filter(|f| chosen.contains(&f.key))
            .map(|f| f.key.clone())
            .collect();
        for k in chosen {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        if let Err(e) = self.run(
            "schedule.set_fields",
            json!({ "schedule_id": id, "fields": keys }),
        ) {
            return error(&frame, &format!("The fields were not changed:\n{e}"));
        }
        self.refresh_schedules();
    }

    /// Modify Schedule › Sort: sort by one field, ascending or descending.
    pub(crate) fn schedule_sort(&mut self) {
        let frame = self.frame;
        let Some((id, def)) = self.active_schedule() else {
            return frame.set_status_text("Open a schedule to sort it", 0);
        };
        let table = self
            .active_view()
            .and_then(|v| self.app.schedule_table(v.doc, id).ok());
        let Some(table) = table else { return };
        let mut choices: Vec<String> = Vec::new();
        for h in &table.headings {
            choices.push(format!("{h} (ascending)"));
            choices.push(format!("{h} (descending)"));
        }
        let names: Vec<&str> = choices.iter().map(String::as_str).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Sort by:", "Schedule Sorting", &names).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Ok(k) = usize::try_from(dialog.get_selection()) else {
            return;
        };
        let Some(field) = def.fields.get(k / 2) else {
            return;
        };
        let args = json!({
            "schedule_id": id,
            "levels": [{ "field": field.key, "descending": k % 2 == 1 }],
        });
        if let Err(e) = self.run("schedule.set_sort", args) {
            return error(&frame, &format!("The sorting was not changed:\n{e}"));
        }
        self.refresh_schedules();
    }

    /// Modify Schedule › Itemize: one row per element, or one per group of the sorted values.
    pub(crate) fn schedule_itemize(&mut self) {
        let Some((id, def)) = self.active_schedule() else {
            return self
                .frame
                .set_status_text("Open a schedule to itemize it", 0);
        };
        let levels = serde_json::to_value(&def.sort).unwrap_or_default();
        let args = json!({ "schedule_id": id, "levels": levels, "itemize": !def.itemize });
        if let Err(e) = self.run("schedule.set_sort", args) {
            return error(&self.frame, &format!("The schedule was not changed:\n{e}"));
        }
        self.frame.set_status_text(
            if def.itemize {
                "Rows with the same sorted values are grouped (Count shows how many)"
            } else {
                "One row per element"
            },
            0,
        );
        self.refresh_schedules();
    }

    /// File › Export COBie: the facility-management data set as a COBie 2.4 workbook. Asks for
    /// the creating contact the first time.
    pub(crate) fn export_cobie(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let Some(store) = self.app.document(doc).map(|d| d.doc.store().clone()) else {
            return;
        };
        let mut settings = reviera_core::cobie::settings(&store);
        if settings.contacts.is_empty() {
            let email = TextEntryDialog::builder(
                &frame,
                "E-mail address of the company creating the COBie data set:",
                "COBie Contact",
            )
            .build();
            if email.show_modal() != ID_OK {
                return;
            }
            let email = email.get_value().unwrap_or_default();
            let company =
                TextEntryDialog::builder(&frame, "Company name:", "COBie Contact").build();
            let company = if company.show_modal() == ID_OK {
                company.get_value().unwrap_or_default()
            } else {
                String::new()
            };
            settings.contacts.push(reviera_core::cobie::Contact {
                email,
                company,
                ..Default::default()
            });
            if let Err(e) = self.run("cobie.set_settings", json!({ "settings": settings })) {
                return error(&frame, &format!("The contact was not saved:\n{e}"));
            }
        }
        let name = self
            .current_path()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Project".into());
        let dialog = FileDialog::builder(&frame)
            .with_message("Export COBie")
            .with_wildcard("Excel workbook (*.xlsx)|*.xlsx")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file(&format!("{name} COBie.xlsx"))
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        match self.run("export.cobie", json!({ "path": path, "overwrite": true })) {
            Ok(r) => {
                let count = |t: &str| {
                    r["tables"]
                        .as_array()
                        .and_then(|a| a.iter().find(|x| x["name"] == t))
                        .and_then(|x| x["rows"].as_u64())
                        .unwrap_or(0)
                };
                let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
                let mut text = format!(
                    "Exported {} floors, {} spaces, {} types and {} components to\n{path}",
                    count("Floor"),
                    count("Space"),
                    count("Type"),
                    count("Component")
                );
                if !warnings.is_empty() {
                    text.push_str(&format!("\n\n{} problems to review:", warnings.len()));
                    for w in warnings.iter().take(12) {
                        text.push_str(&format!(
                            "\n• {} \"{}\": {}",
                            w["table"].as_str().unwrap_or(""),
                            w["record"].as_str().unwrap_or(""),
                            w["message"].as_str().unwrap_or("")
                        ));
                    }
                }
                MessageDialog::builder(&frame, &text, "COBie Export")
                    .with_style(MessageDialogStyle::OK)
                    .build()
                    .show_modal();
            }
            Err(e) => error(&frame, &format!("COBie was not exported:\n{e}")),
        }
    }

    /// Modify Schedule › Export: the schedule as CSV.
    pub(crate) fn export_schedule(&mut self) {
        let frame = self.frame;
        let Some((id, _)) = self.active_schedule() else {
            return frame.set_status_text("Open a schedule to export it", 0);
        };
        let name = self
            .active_view()
            .map(|v| self.view_name(v))
            .unwrap_or_else(|| "Schedule".into());
        let dialog = FileDialog::builder(&frame)
            .with_message("Export Schedule")
            .with_wildcard("CSV files (*.csv)|*.csv|Tab-separated text (*.txt)|*.txt")
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_file(&format!("{name}.csv"))
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        let format = if path.to_ascii_lowercase().ends_with(".txt") {
            "tsv"
        } else {
            "csv"
        };
        // The dialog asked before replacing a file.
        match self.run(
            "schedule.export",
            json!({ "schedule_id": id, "path": path, "format": format, "overwrite": true }),
        ) {
            Ok(r) => frame.set_status_text(
                &format!(
                    "Exported {} rows to {path}",
                    r["rows"].as_u64().unwrap_or(0)
                ),
                0,
            ),
            Err(e) => error(&frame, &format!("The schedule was not exported:\n{e}")),
        }
    }
}
