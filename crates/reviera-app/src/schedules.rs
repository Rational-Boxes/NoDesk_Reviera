// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Schedule tables and export (DOC-SCH-001, DOC-SCH-012). A table needs the model tree (to find
//! each element's level), which the application session holds.

use std::path::PathBuf;

use reviera_core::schedules::{self, Table};
use reviera_core::{
    Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

impl App {
    /// The table of a schedule as it is now.
    pub fn schedule_table(&self, doc: DocumentId, schedule: ElementId) -> Result<Table> {
        self.schedule_table_on(doc, schedule, None)
    }

    /// The table of a schedule as it shows on `sheet` (a keynote legend that filters by sheet
    /// lists only the keynotes of the sheet's views, DOC-VIEW-010); `None`, on its own.
    pub fn schedule_table_on(&self, doc: DocumentId, schedule: ElementId, sheet: Option<ElementId>) -> Result<Table> {
        let parents = self.model_parents(doc)?;
        let store = self.get(doc)?.doc.store();
        let e = store
            .get(schedule)
            .ok_or_else(|| Error::new("schedule.not_found", format!("no schedule {schedule}")))?;
        let views: Option<Vec<String>> = sheet
            .and_then(|s| store.get(s))
            .and_then(reviera_core::sheets::sheet_data)
            .map(|d| d.viewports.into_iter().map(|vp| vp.view).collect());
        schedules::table_on_sheet(store, e, &parents, views.as_deref())
    }
}

fn resolve(app: &App, r: &ElementRef) -> Result<(DocumentId, ElementId)> {
    let doc = app
        .active()
        .ok_or_else(|| Error::new("document.none", "no document is open"))?;
    let id = app
        .get(doc)?
        .doc
        .store()
        .resolve(r)
        .ok_or_else(|| Error::new("schedule.not_found", "no such schedule"))?;
    Ok((doc, id))
}

const fn meta(
    name: &'static str,
    summary: &'static str,
    effects: &'static [Effect],
) -> CommandMeta {
    CommandMeta {
        name,
        version: 1,
        summary,
        description: summary,
        effects,
        transaction_name: "",
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableParams {
    pub schedule_id: ElementRef,
    /// As placed on this sheet (a keynote legend that filters by sheet lists that sheet's
    /// keynotes).
    pub sheet_id: Option<ElementRef>,
}

pub(crate) struct ScheduleGetTable;

impl HostCommand for ScheduleGetTable {
    type Host = App;
    type Params = TableParams;
    type Output = Table;
    const META: CommandMeta = meta(
        "schedule.get_table",
        "A schedule's table as it shows: headings, rows with their elements and cell text, and grand totals",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, p: TableParams) -> Result<Table> {
        let (doc, id) = resolve(app, &p.schedule_id)?;
        let sheet = match &p.sheet_id {
            Some(r) => Some(app.get(doc)?.doc.store().resolve(r).ok_or_else(|| Error::new("sheet.not_found", "no such sheet"))?),
            None => None,
        };
        app.schedule_table_on(doc, id, sheet)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Csv,
    Tsv,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub schedule_id: ElementRef,
    pub path: PathBuf,
    #[serde(default)]
    pub format: Format,
    /// Replace an existing file (required to overwrite).
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    pub path: PathBuf,
    pub rows: usize,
}

pub(crate) struct ScheduleExport;

impl HostCommand for ScheduleExport {
    type Host = App;
    type Params = ExportParams;
    type Output = ExportResult;
    const META: CommandMeta = meta(
        "schedule.export",
        "Export a schedule as delimited text (CSV or TSV, UTF-8), as it shows, with formulas escaped",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<ExportResult> {
        let (doc, id) = resolve(app, &p.schedule_id)?;
        if p.path.exists() && !p.overwrite {
            return Err(Error::new(
                "file.exists",
                format!("{} exists; pass overwrite to replace it", p.path.display()),
            ));
        }
        let table = app.schedule_table(doc, id)?;
        let delimiter = match p.format {
            Format::Csv => ',',
            Format::Tsv => '\t',
        };
        std::fs::write(&p.path, schedules::delimited(&table, delimiter))
            .map_err(|e| Error::new("file.write_failed", e.to_string()))?;
        Ok(ExportResult {
            path: p.path,
            rows: table.rows.len(),
        })
    }
}
