// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Heating and cooling loads (ANA-HCL-001, ANA-ENV-004) as compute jobs: `hc_loads.run` distils
//! the energy model's spaces (opaque keys), their loads and set points, the design conditions
//! (the weather's, with the user's overrides) and a clear design day from the weather file, and
//! submits an `analysis.loads` job; `hc_loads.collect` records the peaks on the spaces
//! (`hc_loads.record`); `hc_loads.status` tells whether the latest run is stale.

use std::path::Path;

use reviera_core::hvac_loads::{self as hl, Infiltration, LoadsInput};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result, Store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// loose, medium (default), tight or none.
    pub infiltration: Option<Infiltration>,
    /// Allow negative load components (credits).
    #[serde(default)]
    pub credits: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub spaces: usize,
}

fn distill(app: &App, store: &Store, p: &RunParams) -> Result<(LoadsInput, std::collections::BTreeMap<String, String>, u64)> {
    let loc = reviera_core::location::get(store);
    let (Some(lat), Some(lon)) = (loc.latitude, loc.longitude) else {
        return Err(bad("hc_loads.no_location", "set the project location first (Manage › Project Location)"));
    };
    let w = &loc.weather;
    let d = w.design();
    let (Some(heat), Some(cool)) = (d.heating_dry_bulb, d.cooling_dry_bulb) else {
        return Err(bad("hc_loads.no_design", "import a weather file or enter the design temperatures (Sustainability › Site › Weather)"));
    };
    // The clear design day from the weather file, in the warmest month of the hemisphere.
    let warm = if lat >= 0.0 { 7 } else { 1 };
    let design_day = match &w.file {
        Some(f) => {
            let p = Path::new(f);
            let path = if p.is_absolute() { p.to_owned() } else { app.active().and_then(|d| app.document(d)).filter(|d| !d.untitled).and_then(|d| d.project.path().parent().map(|dir| dir.join(p))).unwrap_or_else(|| p.to_owned()) };
            let text = std::fs::read_to_string(&path).map_err(|e| bad("hc_loads.weather_read", format!("{}: {e}", path.display())))?;
            hl::design_day(&reviera_core::solar::epw_hours(&text)?, warm)
        }
        None => return Err(bad("hc_loads.no_weather", "import a weather file for the design day's sun (Sustainability › Site › Weather)")),
    };
    let (spaces, key_map) = hl::spaces(store);
    if spaces.is_empty() {
        return Err(bad("hc_loads.no_spaces", "place MEP spaces first: the loads are by space (Analyze › Spaces & Zones)"));
    }
    let input = LoadsInput {
        spaces,
        heating_dry_bulb: heat,
        cooling_dry_bulb: cool,
        cooling_wet_bulb: d.cooling_wet_bulb.unwrap_or(cool - 8.0),
        daily_range: d.daily_range.unwrap_or(10.0),
        infiltration: p.infiltration.unwrap_or_default(),
        latitude: lat,
        longitude: lon,
        time_zone: loc.time_zone.unwrap_or((lon / 15.0).round()),
        true_north: loc.true_north,
        design_day,
        credits: p.credits,
    };
    Ok((input, key_map, w.revision))
}

pub(crate) struct LoadsRun;

impl HostCommand for LoadsRun {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "hc_loads.run",
        version: 1,
        summary: "Run heating and cooling loads",
        description: "Starts a design heating and cooling loads run (ANA-HCL-001, ANA-ENV-004) as an analysis.loads compute job on the energy model's spaces: heating at the heating design temperature (transmission, outdoor air, infiltration by `infiltration` class: loose 0.39, medium 0.19, tight 0.10 L/s per m² of exterior surface, or none), cooling hour by hour over a clear design day from the weather file (sol-air transmission, solar through glazing, people, lighting, equipment, outdoor air, infiltration, sensible and latent). Needs the location, the weather's design temperatures and a weather file. Returns the job; follow it with compute.status, then hc_loads.collect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let p = p.unwrap_or_default();
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let (input, key_map, rev) = distill(app, &store, &p)?;
        let hash = reviera_core::solar::hash_of(&input);
        let distilled = reviera_compute::distill::loads(&input, &key_map);
        let client = app.compute();
        let job_id = client.submit(reviera_compute::kinds::LOADS, None, &distilled, "", None).map_err(crate::compute::failed)?;
        client.set_context(&job_id, json!({ "loads": { "infiltration": input.infiltration, "heating_dry_bulb": input.heating_dry_bulb, "cooling_dry_bulb": input.cooling_dry_bulb, "weather_revision": rev, "inputs_hash": hash, "credits": p.credits } }));
        Ok(RunOut { job_id, spaces: input.spaces.len() })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    pub job_id: Option<String>,
}

pub(crate) struct LoadsCollect;

impl HostCommand for LoadsCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = hl::Report;
    const META: CommandMeta = CommandMeta {
        name: "hc_loads.collect",
        version: 1,
        summary: "Collect a loads run",
        description: "Fetches a finished analysis.loads job and keeps it (hc_loads.record, one undoable step): the run, and each space's peaks in Pset_SpaceThermalDesign (cooling and heating design airflow, total sensible heat gain, total heat gain, total heat loss). Returns the report.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<CollectParams>) -> Result<hl::Report> {
        use reviera_compute::kinds::{LOADS, LOADS_RESULTS};
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client.jobs().into_iter().filter(|j| j.kind == LOADS && j.state == "succeeded" && !j.results_fetched).collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| bad("hc_loads.no_job", "there is no finished loads run to collect"))?;
        if job.state != "succeeded" {
            let why = job.failure.as_ref().map(|f| format!(": {}", f.message)).unwrap_or_default();
            return Err(bad("compute.not_ready", format!("the run is {}{why}", job.state)));
        }
        let (m, raw) = client.results(&job.job_id).map_err(crate::compute::failed)?;
        if m.schema != LOADS_RESULTS {
            return Err(bad("hc_loads.not_a_loads_run", m.schema));
        }
        let results: hl::LoadsResults = serde_json::from_slice(&raw).map_err(|e| bad("compute.invalid_input", e.to_string()))?;
        let ctx = job.context.clone().unwrap_or_default();
        let c = &ctx["loads"];
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        app.execute(
            "hc_loads.record",
            json!({ "job_id": job.job_id, "infiltration": c["infiltration"], "heating_dry_bulb": c["heating_dry_bulb"], "cooling_dry_bulb": c["cooling_dry_bulb"], "results": results, "key_map": job.key_map, "weather_revision": c["weather_revision"], "inputs_hash": c["inputs_hash"], "at": at }),
            caller,
        )?;
        let out = app.execute("hc_loads.report", json!({ "run_id": job.job_id }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("hc_loads.internal", e.to_string()))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub run_id: String,
    pub stale: bool,
}

pub(crate) struct LoadsStatus;

impl HostCommand for LoadsStatus {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = StatusOut;
    const META: CommandMeta = CommandMeta {
        name: "hc_loads.status",
        version: 1,
        summary: "Whether a loads run is stale",
        description: "Whether a loads run (`job_id`, else the latest) is stale: the energy model, the spaces' loads or set points, the site or the weather changed since it ran.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CollectParams>) -> Result<StatusOut> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let d = hl::data(&store);
        let r = match p.and_then(|p| p.job_id) {
            Some(id) => d.runs.iter().find(|x| x.id == id).cloned(),
            None => d.runs.last().cloned(),
        }
        .ok_or_else(|| bad("hc_loads.no_run", "no loads run yet"))?;
        let credits = app.compute().status(&r.id).and_then(|j| j.context).is_some_and(|c| c["loads"]["credits"] == true);
        let stale = match distill(app, &store, &RunParams { infiltration: Some(r.infiltration), credits }) {
            Ok((input, _, _)) => reviera_core::solar::hash_of(&input) != r.inputs_hash,
            Err(_) => true,
        };
        Ok(StatusOut { run_id: r.id, stale })
    }
}
