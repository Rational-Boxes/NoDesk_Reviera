// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Solar analysis (ANA-SOLAR-001, ANA-ENV-002) as compute jobs: `solar.run` distils the faces to
//! analyse, the shading context, the site and the weather file's hourly irradiance into a
//! `solar.input` and submits an `analysis.solar` job; `solar.collect` records the finished
//! study (`solar.record`); `solar.status` tells whether the latest study is stale (the faces,
//! shading, site or weather changed since); `solar.export` writes the per-face results as CSV.

use std::path::{Path, PathBuf};

use reviera_core::solar::{self, PvSettings, SolarInput, Surfaces};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result, Store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// `insolation` (default) or `pv`.
    pub study_type: Option<String>,
    /// First and last (month, day); a full year by default.
    pub from: Option<[u8; 2]>,
    pub to: Option<[u8; 2]>,
    /// `roofs` (default), `masses`, or {"elements": [ids]}.
    pub surfaces: Option<Surfaces>,
    /// Metres between grid points (0.1–10, default 1).
    pub grid: Option<f64>,
    pub pv_settings: Option<PvSettings>,
    /// Category keys left out of the shading context.
    #[serde(default)]
    pub exclude_categories: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub faces: usize,
    pub shading_triangles: usize,
    pub hours: usize,
}

/// The project's weather file on disk.
fn weather_path(app: &App, file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        return p.to_owned();
    }
    app.active()
        .and_then(|d| app.document(d))
        .filter(|d| !d.untitled)
        .and_then(|d| d.project.path().parent().map(|dir| dir.join(p)))
        .unwrap_or_else(|| p.to_owned())
}

/// The study's input, distilled from the model.
fn distill(app: &App, store: &Store, p: &RunParams) -> Result<(SolarInput, u64)> {
    let loc = reviera_core::location::get(store);
    let (Some(lat), Some(lon)) = (loc.latitude, loc.longitude) else {
        return Err(bad("solar.no_location", "set the project location first (Manage › Project Location)"));
    };
    let w = &loc.weather;
    let file = w.file.as_ref().ok_or_else(|| bad("solar.no_weather", "import a weather file first (Sustainability › Site › Weather)"))?;
    let path = weather_path(app, file);
    let text = std::fs::read_to_string(&path).map_err(|e| bad("solar.weather_read", format!("{}: {e}", path.display())))?;
    let hours = solar::epw_hours(&text)?;
    let faces = solar::analysed_faces(store, p.surfaces.as_ref().unwrap_or(&Surfaces::Roofs))?;
    if faces.is_empty() {
        return Err(bad("solar.no_faces", "there are no roof or mass faces to analyse"));
    }
    let pv = (p.study_type.as_deref() == Some("pv")).then(|| p.pv_settings.clone().unwrap_or_default());
    let input = SolarInput {
        latitude: lat,
        longitude: lon,
        time_zone: loc.time_zone.unwrap_or_else(|| w.station.as_ref().map_or((lon / 15.0).round(), |s| s.time_zone)),
        true_north: loc.true_north,
        from: p.from.unwrap_or([1, 1]),
        to: p.to.unwrap_or([12, 31]),
        hours,
        faces: faces.into_iter().map(|f| f.1).collect(),
        shading: solar::shading(store, &p.exclude_categories),
        grid: p.grid.unwrap_or(1.0),
        pv,
    };
    Ok((input, w.revision))
}

pub(crate) struct SolarRun;

impl HostCommand for SolarRun {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "solar.run",
        version: 1,
        summary: "Run a solar study",
        description: "Starts a solar study (ANA-SOLAR-001, ANA-ENV-002) as an analysis.solar compute job: cumulative insolation (kWh/m²) on a grid over the analysed faces (`surfaces`: roofs, masses, or {elements: [...]}) for the period `from`–`to` (default a year) from the site and the weather file's hourly direct and diffuse irradiance, shaded by every model element (except `exclude_categories`); `study_type` pv adds PV potential per face (`pv_settings`: coverage, efficiency, derate, cost_per_kwp, energy_price). Needs the project location and a weather file. Returns the job at once; follow it with compute.status, then solar.collect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let p = p.unwrap_or(RunParams { study_type: None, from: None, to: None, surfaces: None, grid: None, pv_settings: None, exclude_categories: Vec::new() });
        if p.study_type.as_deref().is_some_and(|t| t != "insolation" && t != "pv") {
            return Err(bad("solar.study_type", "a study is insolation or pv"));
        }
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let (input, rev) = distill(app, &store, &p)?;
        if !(0.1..=10.0).contains(&input.grid) {
            return Err(bad("solar.grid", "the grid spacing runs from 0.1 to 10 m"));
        }
        let out = RunOut { job_id: String::new(), faces: input.faces.len(), shading_triangles: input.shading.len(), hours: input.hours.len() };
        let hash = solar::inputs_hash(&input, rev);
        let distilled = reviera_compute::distill::solar(&input);
        let client = app.compute();
        let job_id = client.submit(reviera_compute::kinds::SOLAR, None, &distilled, "", None).map_err(crate::compute::failed)?;
        client.set_context(&job_id, json!({ "solar": { "study_type": p.study_type.unwrap_or_else(|| "insolation".into()), "from": input.from, "to": input.to, "grid": input.grid, "pv": input.pv, "inputs_hash": hash, "params": { "surfaces": p.surfaces, "exclude_categories": p.exclude_categories } } }));
        Ok(RunOut { job_id, ..out })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    pub job_id: Option<String>,
}

pub(crate) struct SolarCollect;

impl HostCommand for SolarCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = solar::StudyReport;
    const META: CommandMeta = CommandMeta {
        name: "solar.collect",
        version: 1,
        summary: "Collect a solar study",
        description: "Fetches a finished analysis.solar job's results and keeps them with the model (solar.record, one undoable step). Returns the per-face report.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<CollectParams>) -> Result<solar::StudyReport> {
        use reviera_compute::kinds::{SOLAR, SOLAR_RESULTS};
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client.jobs().into_iter().filter(|j| j.kind == SOLAR && j.state == "succeeded" && !j.results_fetched).collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| bad("solar.no_job", "there is no finished solar study to collect"))?;
        if job.state != "succeeded" {
            let why = job.failure.as_ref().map(|f| format!(": {}", f.message)).unwrap_or_default();
            return Err(bad("compute.not_ready", format!("the study is {}{why}", job.state)));
        }
        let (m, raw) = client.results(&job.job_id).map_err(crate::compute::failed)?;
        if m.schema != SOLAR_RESULTS {
            return Err(bad("solar.not_a_solar_study", m.schema));
        }
        let results: solar::SolarResults = serde_json::from_slice(&raw).map_err(|e| bad("compute.invalid_input", e.to_string()))?;
        let ctx = job.context.clone().unwrap_or_default();
        let c = &ctx["solar"];
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        app.execute(
            "solar.record",
            json!({ "job_id": job.job_id, "study_type": c["study_type"], "from": c["from"], "to": c["to"], "grid": c["grid"], "pv": c["pv"], "results": results, "inputs_hash": c["inputs_hash"], "at": at }),
            caller,
        )?;
        let out = app.execute("solar.results", json!({ "study_id": job.job_id }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("solar.internal", e.to_string()))
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {
    pub study_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub study_id: String,
    /// The faces, shading, site or weather changed since the study ran.
    pub stale: bool,
}

pub(crate) struct SolarStatus;

impl HostCommand for SolarStatus {
    type Host = App;
    type Params = Option<StatusParams>;
    type Output = StatusOut;
    const META: CommandMeta = CommandMeta {
        name: "solar.status",
        version: 1,
        summary: "Whether a solar study is stale",
        description: "Whether a solar study (`study_id`, else the latest) is stale: the faces, shading context, site, period or weather changed since it ran.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<StatusParams>) -> Result<StatusOut> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let d = solar::data(&store);
        let st = match p.and_then(|p| p.study_id) {
            Some(id) => d.studies.iter().find(|s| s.id == id).cloned(),
            None => d.studies.last().cloned(),
        }
        .ok_or_else(|| bad("solar.no_study", "no solar study yet"))?;
        let job_params = app.compute().status(&st.id).and_then(|j| j.context).map(|c| c["solar"]["params"].clone());
        let params = RunParams {
            study_type: Some(st.study_type.clone()),
            from: Some(st.from),
            to: Some(st.to),
            surfaces: job_params.as_ref().and_then(|p| serde_json::from_value(p["surfaces"].clone()).ok()),
            grid: Some(st.grid),
            pv_settings: st.pv.clone(),
            exclude_categories: job_params.as_ref().and_then(|p| serde_json::from_value(p["exclude_categories"].clone()).ok()).unwrap_or_default(),
        };
        let stale = match distill(app, &store, &params) {
            Ok((input, rev)) => solar::inputs_hash(&input, rev) != st.inputs_hash,
            Err(_) => true,
        };
        Ok(StatusOut { study_id: st.id, stale })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub study_id: Option<String>,
    pub path: String,
}

pub(crate) struct SolarExport;

impl HostCommand for SolarExport {
    type Host = App;
    type Params = ExportParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "solar.export",
        version: 1,
        summary: "Export a solar study",
        description: "Writes a solar study's per-face results (`study_id`, else the latest) to `path` as CSV: face, element, area (m²), mean (kWh/m²), total (kWh), and for PV studies kWh a year, cost and payback years. Returns the path.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ExportParams) -> Result<String> {
        let out = app.execute("solar.results", json!({ "study_id": p.study_id }), caller)?;
        let r: solar::StudyReport = serde_json::from_value(out.result).map_err(|e| bad("solar.internal", e.to_string()))?;
        let opt = |v: Option<f64>, d: usize| v.map_or(String::new(), |x| format!("{x:.d$}"));
        let mut s = String::from("face,element,area_m2,mean_kwh_m2,total_kwh,pv_kwh_year,pv_cost,pv_payback_years\n");
        for f in &r.faces {
            s.push_str(&format!(
                "{},{},{:.3},{:.2},{:.1},{},{},{}\n",
                f.key,
                f.element.map_or(String::new(), |e| e.0.to_string()),
                f.area,
                f.mean,
                f.total,
                opt(f.pv_kwh_year, 1),
                opt(f.pv_cost, 0),
                opt(f.pv_payback_years, 1)
            ));
        }
        std::fs::write(&p.path, s).map_err(|e| bad("solar.write", format!("{}: {e}", p.path)))?;
        Ok(p.path)
    }
}
