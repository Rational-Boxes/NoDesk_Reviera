// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! COBie export (DOC-OUT-015): the tables from `reviera_core::cobie`, written as a workbook
//! (one worksheet per table) or as one delimited file per table.

use std::path::PathBuf;

use reviera_core::cobie::{self, Issue};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

/// The export time as an ISO date and time (UTC).
fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let t = secs % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}",
        crate::output::today(),
        t / 3600,
        t / 60 % 60,
        t % 60
    )
}

impl App {
    /// The COBie 2.4 tables of a document and their validation problems.
    pub fn cobie_tables(
        &self,
        doc: DocumentId,
        created_on: &str,
    ) -> Result<(Vec<cobie::Table>, Vec<Issue>)> {
        let parents = self.model_parents(doc)?;
        let store = self.get(doc)?.doc.store();
        Ok(cobie::tables_2_4(store, &parents, created_on))
    }
}

/// Text a spreadsheet would read as a formula is kept as text.
fn safe(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() {
        format!("'{s}")
    } else {
        s.to_owned()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Version {
    /// The COBie 2.4 layout.
    #[default]
    V2_4,
    /// The current NBIMS-US table set (not available yet).
    Current,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// A workbook, one worksheet per table.
    #[default]
    Xlsx,
    /// One CSV file per table, in the folder `path`.
    Csv,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// The workbook to write, or the folder for CSV files.
    pub path: PathBuf,
    #[serde(default)]
    pub version: Version,
    #[serde(default)]
    pub format: Format,
    /// Replace existing files (required to overwrite).
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TableCount {
    pub name: String,
    pub rows: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    pub path: PathBuf,
    pub tables: Vec<TableCount>,
    /// Validation problems (the data set is written regardless).
    pub warnings: Vec<Issue>,
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

pub(crate) struct ExportCobie;

impl HostCommand for ExportCobie {
    type Host = App;
    type Params = ExportParams;
    type Output = ExportResult;
    const META: CommandMeta = meta(
        "export.cobie",
        "Export the model's facility-management data as COBie 2.4 (an XLSX workbook, or one CSV per table), with validation warnings",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<ExportResult> {
        if p.version == Version::Current {
            return Err(Error::new(
                "cobie.version_unavailable",
                "the current NBIMS-US COBie layout is not available yet; use version v2_4",
            ));
        }
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let (tables, warnings) = app.cobie_tables(doc, &now())?;
        let write_err =
            |e: &dyn std::fmt::Display| Error::new("export.write_failed", e.to_string());
        match p.format {
            Format::Xlsx => {
                if p.path.exists() && !p.overwrite {
                    return Err(Error::new(
                        "file.exists",
                        format!("{} exists; pass overwrite to replace it", p.path.display()),
                    ));
                }
                let mut book = rust_xlsxwriter::Workbook::new();
                let bold = rust_xlsxwriter::Format::new().set_bold();
                for t in &tables {
                    let sheet = book.add_worksheet();
                    sheet.set_name(&t.name).map_err(|e| write_err(&e))?;
                    for (c, h) in t.columns.iter().enumerate() {
                        sheet
                            .write_string_with_format(0, c as u16, h, &bold)
                            .map_err(|e| write_err(&e))?;
                    }
                    for (r, row) in t.rows.iter().enumerate() {
                        for (c, v) in row.iter().enumerate() {
                            // Text cells throughout: COBie values are text in the standard's
                            // spreadsheet form, and text cannot run as a formula.
                            sheet
                                .write_string(r as u32 + 1, c as u16, v)
                                .map_err(|e| write_err(&e))?;
                        }
                    }
                    sheet.set_freeze_panes(1, 0).map_err(|e| write_err(&e))?;
                }
                book.save(&p.path).map_err(|e| write_err(&e))?;
            }
            Format::Csv => {
                std::fs::create_dir_all(&p.path).map_err(|e| write_err(&e))?;
                for t in &tables {
                    let file = p.path.join(format!("{}.csv", t.name));
                    if file.exists() && !p.overwrite {
                        return Err(Error::new(
                            "file.exists",
                            format!("{} exists; pass overwrite to replace it", file.display()),
                        ));
                    }
                    let line = |cells: &[String]| {
                        cells
                            .iter()
                            .map(|c| {
                                let c = safe(c);
                                if c.contains([',', '"', '\n']) {
                                    format!("\"{}\"", c.replace('"', "\"\""))
                                } else {
                                    c
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    let mut text = line(&t.columns) + "\n";
                    for r in &t.rows {
                        text.push_str(&line(r));
                        text.push('\n');
                    }
                    std::fs::write(&file, text).map_err(|e| write_err(&e))?;
                }
            }
        }
        Ok(ExportResult {
            path: p.path,
            tables: tables
                .iter()
                .map(|t| TableCount {
                    name: t.name.clone(),
                    rows: t.rows.len(),
                })
                .collect(),
            warnings,
        })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateParams {}

pub(crate) struct CobieValidate;

impl HostCommand for CobieValidate {
    type Host = App;
    type Params = ValidateParams;
    type Output = Vec<Issue>;
    const META: CommandMeta = meta(
        "cobie.validate",
        "Check the model's COBie data set without writing it: contacts, unique names, and the spaces, types and floors records refer to",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: ValidateParams) -> Result<Vec<Issue>> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        Ok(app.cobie_tables(doc, &now())?.1)
    }
}
