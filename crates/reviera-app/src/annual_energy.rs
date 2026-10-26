// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annual energy (ANA-ENV-005) as compute jobs: `energy.run` distils the energy model's spaces
//! (opaque keys), their loads, set points and opening hours, the weather year and the options,
//! and submits an `analysis.energy` job; `energy.collect` records it (`energy.record`);
//! `energy.status` tells whether the latest run is stale.

use std::collections::BTreeMap;
use std::path::Path;

use reviera_core::annual_energy::{self as ae, EnergyInput, EnergyOptions};
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
    #[serde(default)]
    pub options: Option<EnergyOptions>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub spaces: usize,
}

fn distill(app: &App, store: &Store, options: &EnergyOptions) -> Result<(EnergyInput, BTreeMap<String, String>, u64)> {
    options.check()?;
    let loc = reviera_core::location::get(store);
    let (Some(lat), Some(lon)) = (loc.latitude, loc.longitude) else {
        return Err(bad("energy.no_location", "set the project location first (Sustainability › Site › Location)"));
    };
    let w = &loc.weather;
    let Some(f) = &w.file else {
        return Err(bad("energy.no_weather", "import a weather file: the run needs its year (Sustainability › Site › Weather)"));
    };
    let p = Path::new(f);
    let path = if p.is_absolute() { p.to_owned() } else { app.active().and_then(|d| app.document(d)).filter(|d| !d.untitled).and_then(|d| d.project.path().parent().map(|dir| dir.join(p))).unwrap_or_else(|| p.to_owned()) };
    let text = std::fs::read_to_string(&path).map_err(|e| bad("energy.weather_read", format!("{}: {e}", path.display())))?;
    let (weather, first_weekday) = ae::epw_year(&text)?;
    let (spaces, key_map) = reviera_core::hvac_loads::spaces(store);
    if spaces.is_empty() {
        return Err(bad("energy.no_spaces", "place MEP spaces first: the model is by space (Analyze › Spaces & Zones)"));
    }
    let input = EnergyInput { spaces, latitude: lat, longitude: lon, time_zone: loc.time_zone.unwrap_or((lon / 15.0).round()), true_north: loc.true_north, weather, first_weekday, options: options.clone() };
    Ok((input, key_map, w.revision))
}

pub(crate) struct EnergyRunCmd;

impl HostCommand for EnergyRunCmd {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "energy.run",
        version: 1,
        summary: "Run annual energy",
        description: "Starts an annual energy run (ANA-ENV-005) as an analysis.energy compute job: each of the energy model's spaces as a lumped hourly network (air, surfaces, thermal mass) through the weather file's year with an ideal thermostat (set points from the space types, set back 4 K when unoccupied; occupied from opening to closing time on weekdays). `options`: infiltration (loose, medium, tight, none), mass (light, medium, heavy), heating (gas, electric, heat_pump), heating_efficiency (0 = the source's default: 0.9, 1.0, COP 3.0), cooling_cop (3.0), grid_factor (kg CO2e/kWh; electricity carbon left out without it), gas_factor (0.202). Needs the location, a weather file and spaces. Returns the job; follow it with compute.status, then energy.collect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let options = p.and_then(|p| p.options).unwrap_or_default();
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let (input, key_map, rev) = distill(app, &store, &options)?;
        let hash = reviera_core::solar::hash_of(&input);
        let distilled = reviera_compute::distill::energy(&input, &key_map);
        let client = app.compute();
        let job_id = client.submit(reviera_compute::kinds::ENERGY, None, &distilled, "", None).map_err(crate::compute::failed)?;
        client.set_context(&job_id, json!({ "energy": { "options": options, "weather_revision": rev, "inputs_hash": hash } }));
        Ok(RunOut { job_id, spaces: input.spaces.len() })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    pub job_id: Option<String>,
}

pub(crate) struct EnergyCollect;

impl HostCommand for EnergyCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = ae::Report;
    const META: CommandMeta = CommandMeta {
        name: "energy.collect",
        version: 1,
        summary: "Collect an annual energy run",
        description: "Fetches a finished analysis.energy job and keeps it with the model (energy.record, one undoable step). Returns the report (energy.results): monthly and annual heating and cooling loads, delivered energy by end use, electricity and gas, operational carbon, EUI, peaks and each space's loads.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<CollectParams>) -> Result<ae::Report> {
        use reviera_compute::kinds::{ENERGY, ENERGY_RESULTS};
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client.jobs().into_iter().filter(|j| j.kind == ENERGY && j.state == "succeeded" && !j.results_fetched).collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| bad("energy.no_job", "there is no finished annual energy run to collect"))?;
        if job.state != "succeeded" {
            let why = job.failure.as_ref().map(|f| format!(": {}", f.message)).unwrap_or_default();
            return Err(bad("compute.not_ready", format!("the run is {}{why}", job.state)));
        }
        let (m, raw) = client.results(&job.job_id).map_err(crate::compute::failed)?;
        if m.schema != ENERGY_RESULTS {
            return Err(bad("energy.not_an_energy_run", m.schema));
        }
        let results: ae::EnergyResults = serde_json::from_slice(&raw).map_err(|e| bad("compute.invalid_input", e.to_string()))?;
        let ctx = job.context.clone().unwrap_or_default();
        let c = &ctx["energy"];
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        app.execute(
            "energy.record",
            json!({ "job_id": job.job_id, "options": c["options"], "results": results, "key_map": job.key_map, "weather_revision": c["weather_revision"], "inputs_hash": c["inputs_hash"], "at": at }),
            caller,
        )?;
        let out = app.execute("energy.results", json!({ "run_id": job.job_id }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("energy.internal", e.to_string()))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub run_id: String,
    pub stale: bool,
}

pub(crate) struct EnergyStatus;

impl HostCommand for EnergyStatus {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = StatusOut;
    const META: CommandMeta = CommandMeta {
        name: "energy.status",
        version: 1,
        summary: "Whether an annual energy run is stale",
        description: "Whether an annual energy run (`job_id`, else the latest) is stale: the energy model, the spaces' loads or set points, the site or the weather changed since it ran.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CollectParams>) -> Result<StatusOut> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let d = ae::data(&store);
        let r = match p.and_then(|p| p.job_id) {
            Some(id) => d.runs.iter().find(|x| x.id == id).cloned(),
            None => d.runs.last().cloned(),
        }
        .ok_or_else(|| bad("energy.no_run", "no annual energy run yet"))?;
        let stale = match distill(app, &store, &r.options) {
            Ok((input, _, _)) => reviera_core::solar::hash_of(&input) != r.inputs_hash,
            Err(_) => true,
        };
        Ok(StatusOut { run_id: r.id, stale })
    }
}
