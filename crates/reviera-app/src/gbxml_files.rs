// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! gbXML files (ANA-EM-004, ANA-ENV-009): `gbxml.export(path)` writes the energy model as
//! gbXML 8.01 (`reviera_core::gbxml::export`); `gbxml.import_loads(path)` reads the design loads
//! another tool computed and writes them onto the matching spaces (`gbxml.apply_loads`).

use reviera_core::gbxml;
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub path: String,
    /// `energy_model` (the default) or `spaces`; both write the energy model's spaces so far.
    pub source: Option<String>,
}

pub(crate) struct GbxmlExport;

impl HostCommand for GbxmlExport {
    type Host = App;
    type Params = ExportParams;
    type Output = gbxml::ExportReport;
    const META: CommandMeta = CommandMeta {
        name: "gbxml.export",
        version: 1,
        summary: "Export gbXML",
        description: "Writes the energy model (ANA-EM-003) to `path` as gbXML 8.01 (ANA-EM-004): campus and location, building, storeys, spaces (ids `sp-` + GlobalId with $ as -; area, volume, people, lighting and equipment), surfaces with their adjacent spaces (shared interior surfaces once), windows and doors as openings, constructions and window types with U-values and SHGC. Returns the counts and warnings (spaces left out, default U-values).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<gbxml::ExportReport> {
        if p.source.as_deref().is_some_and(|s| s != "energy_model" && s != "spaces") {
            return Err(bad("gbxml.bad_source", "source is energy_model or spaces"));
        }
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let (text, report) = gbxml::export(&store)?;
        std::fs::write(&p.path, text).map_err(|e| bad("gbxml.write", format!("{}: {e}", p.path)))?;
        Ok(report)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    pub path: String,
}

pub(crate) struct GbxmlImportLoads;

impl HostCommand for GbxmlImportLoads {
    type Host = App;
    type Params = ImportParams;
    type Output = gbxml::ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "gbxml.import_loads",
        version: 1,
        summary: "Import loads from gbXML",
        description: "Reads the result values another tool wrote into a gbXML file at `path` (Results elements naming a space: HeatLoad, CoolingLoad, SensibleCoolingLoad, HeatingAirflow, CoolingAirflow, in W, kW, Btu/h, kBtu/h, tons, m3/s, m3/h or cfm; geometry ignored) and writes them onto the matching spaces' Pset_SpaceThermalDesign (TotalHeatLoss, TotalHeatGain, TotalSensibleHeatGain, airflows), matching by id (sp- + GlobalId), then by name and number; one undoable step. Returns the matched spaces, the unmatched ids and matched spaces with no values.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ImportParams) -> Result<gbxml::ImportReport> {
        let text = std::fs::read_to_string(&p.path).map_err(|e| bad("gbxml.read", format!("{}: {e}", p.path)))?;
        let spaces = gbxml::read_loads(&text)?;
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let out = app.execute("gbxml.apply_loads", json!({ "file": p.path, "spaces": spaces, "at": at }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("gbxml.internal", e.to_string()))
    }
}
