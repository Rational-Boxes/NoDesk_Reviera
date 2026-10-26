// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embodied carbon files (ANA-CARB-001): `embodied_carbon.import_factors(path)` reads a factor
//! library in the documented Reviera CSV format (`reviera_core::embodied_carbon::parse_csv`) and
//! adds it to the project (importing the same library again replaces its factors and keeps the
//! matches); `embodied_carbon.export(run_id?, path, format)` writes a run's per-element,
//! per-category and per-material results as CSV or JSON. Data sets whose licence forbids
//! embedding stay user-imported files on the user's machine; Reviera ships none.

use reviera_core::embodied_carbon as ec;
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    pub path: String,
    /// `csv` (the default); ILCD+EPD is not read yet.
    pub format: Option<String>,
    /// The library's name; the file's name when omitted.
    pub name: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportReport {
    pub library_id: String,
    pub factors: usize,
}

pub(crate) struct ImportFactors;

impl HostCommand for ImportFactors {
    type Host = App;
    type Params = ImportParams;
    type Output = ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "embodied_carbon.import_factors",
        version: 1,
        summary: "Import emission factors",
        description: "Reads an emission factor library (`path`, `format` csv) into the project (ANA-CARB-001). The CSV has a header row and a row per factor with the columns id, name, class, unit (kg, m3, m2 or piece), a1_a3, a4, a5, c1_c4, d, biogenic (kg CO2e per unit), density (kg/m3), service_life (years), source and valid_until; id, name, unit and a1_a3 are required. Importing a library of the same `name` again replaces its factors and keeps the matches. Reviera ships no factor data. Returns the library's id and factor count.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ImportParams) -> Result<ImportReport> {
        if p.format.as_deref().is_some_and(|f| !f.eq_ignore_ascii_case("csv")) {
            return Err(bad("carbon.format", "only the Reviera CSV format is read so far; export ILCD or openLCA data as CSV"));
        }
        let path = std::path::PathBuf::from(&p.path);
        let text = std::fs::read_to_string(&path).map_err(|e| bad("carbon.read", format!("{}: {e}", path.display())))?;
        let factors = ec::parse_csv(&text)?;
        let n = factors.len();
        let name = p.name.unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Factors".into()));
        let out = app.execute("embodied_carbon.add_library", json!({ "name": name, "file": p.path, "factors": factors }), caller)?;
        Ok(ImportReport { library_id: out.result.as_str().unwrap_or_default().to_owned(), factors: n })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// The run; the latest when omitted.
    pub run_id: Option<String>,
    pub path: String,
    /// `csv` (the default) or `json`.
    pub format: Option<String>,
}

pub(crate) struct ExportRun;

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

impl HostCommand for ExportRun {
    type Host = App;
    type Params = ExportParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "embodied_carbon.export",
        version: 1,
        summary: "Export embodied carbon results",
        description: "Writes an embodied carbon run (`run_id`, else the latest) to `path` as CSV (sections by category, by material and by element, each with A1-A3, A4, A5, B4, C1-C4, the A-C total, module D and biogenic in kg CO2e) or JSON (the whole run). Returns the path written.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ExportParams) -> Result<String> {
        let out = app.execute("embodied_carbon.results", json!({ "run_id": p.run_id }), caller)?;
        let res: serde_json::Value = out.result;
        let text = if p.format.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("json")) {
            serde_json::to_string_pretty(&res["run"]).expect("serialises")
        } else {
            let run: ec::Run = serde_json::from_value(res["run"].clone()).map_err(|e| bad("carbon.internal", e.to_string()))?;
            let row = |key: &str, g: &ec::Gwp, extra: &str| format!("{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}{extra}\n", csv_cell(key), g.a1_a3, g.a4, g.a5, g.b4, g.c1_c4, g.total, g.d, g.biogenic);
            let head = "a1_a3,a4,a5,b4,c1_c4,total_a_c,d,biogenic";
            let mut s = format!("# Embodied carbon run {} (kg CO2e); study period {} years\n", run.id, run.options.study_period);
            s.push_str(&format!("section,key,{head},volume_m3\n"));
            s.push_str(&format!("total,{}", row("all", &run.totals, "")));
            for l in &run.by_category {
                s.push_str(&format!("category,{}", row(&l.key, &l.gwp, &format!(",{:.4}", l.volume))));
            }
            for l in &run.by_material {
                s.push_str(&format!("material,{}", row(&l.key, &l.gwp, &format!(",{:.4}", l.volume))));
            }
            for (guid, g) in &run.by_element {
                s.push_str(&format!("element,{}", row(guid, g, "")));
            }
            for x in &run.excluded {
                s.push_str(&format!("excluded,{},,,,,,,,,{:.4}\n", csv_cell(&format!("{} ({})", x.material, x.reason)), x.volume));
            }
            s
        };
        std::fs::write(&p.path, text).map_err(|e| bad("carbon.write", format!("{}: {e}", p.path)))?;
        Ok(p.path)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    #[serde(default)]
    pub options: Option<ec::Options>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub lines: usize,
    pub elements: usize,
    pub matched_materials: usize,
}

pub(crate) struct RunCarbon;

impl HostCommand for RunCarbon {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "embodied_carbon.run",
        version: 1,
        summary: "Run embodied carbon",
        description: "Starts an embodied carbon run (ANA-CARB-001, ANA-ENV-001) as an analysis.carbon compute job: the quantities of walls, floors, ceilings and roofs and the factors matched to their materials are distilled (element keys opaque) and calculated out of process by the built-in runner. `options`: stages (a4, a5, b4, c1_c4), study_period (years, default 50), gross_floor_area (m2). Returns the job at once; follow it with compute.status, then embodied_carbon.collect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let o = p.and_then(|p| p.options).unwrap_or_default();
        ec::check_options(&o)?;
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store();
        let d = ec::distill(store, &o);
        if d.input.lines.is_empty() {
            return Err(bad("carbon.nothing", "the model has no walls, floors, ceilings or roofs to quantify"));
        }
        let out = RunOut { job_id: String::new(), lines: d.input.lines.len(), elements: d.key_map.len(), matched_materials: d.input.factors.len() };
        let distilled = reviera_compute::distill::carbon(&d);
        let client = app.compute();
        let job_id = client.submit(reviera_compute::kinds::CARBON, None, &distilled, "", None).map_err(crate::compute::failed)?;
        client.set_context(&job_id, json!({ "carbon": { "inputs": d.inputs, "factors": d.input.factors, "options": o } }));
        Ok(RunOut { job_id, ..out })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    /// Default the latest finished carbon run not yet collected.
    pub job_id: Option<String>,
}

pub(crate) struct CollectCarbon;

impl HostCommand for CollectCarbon {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = ec::Run;
    const META: CommandMeta = CommandMeta {
        name: "embodied_carbon.collect",
        version: 1,
        summary: "Collect an embodied carbon run",
        description: "Fetches a finished analysis.carbon job's results from the compute host and keeps them with the model (embodied_carbon.record, one undoable step). Returns the run.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<CollectParams>) -> Result<ec::Run> {
        use reviera_compute::kinds::{CARBON, CARBON_RESULTS};
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client.jobs().into_iter().filter(|j| j.kind == CARBON && j.state == "succeeded" && !j.results_fetched).collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| bad("carbon.no_job", "there is no finished embodied carbon run to collect"))?;
        if job.state != "succeeded" {
            let why = job.failure.as_ref().map(|f| format!(": {}", f.message)).unwrap_or_default();
            return Err(bad("compute.not_ready", format!("the run is {}{why}", job.state)));
        }
        let (m, raw) = client.results(&job.job_id).map_err(crate::compute::failed)?;
        if m.schema != CARBON_RESULTS {
            return Err(bad("carbon.not_a_carbon_run", m.schema));
        }
        let results: ec::CarbonResults = serde_json::from_slice(&raw).map_err(|e| bad("compute.invalid_input", e.to_string()))?;
        let ctx = job.context.clone().unwrap_or_default();
        let c = &ctx["carbon"];
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let out = app.execute(
            "embodied_carbon.record",
            json!({ "job_id": job.job_id, "results": results, "key_map": job.key_map, "inputs": c["inputs"], "factors": c["factors"], "options": c["options"], "at": at }),
            caller,
        )?;
        serde_json::from_value(out.result).map_err(|e| bad("carbon.internal", e.to_string()))
    }
}
