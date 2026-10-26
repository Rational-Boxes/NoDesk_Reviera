// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Daylight factor studies (ANA-LIGHT-002, ANA-ENV-006) as compute jobs: `daylight.run`
//! distils the spaces' sensor grids and room values and the model's obstructions and glazing,
//! and submits an `analysis.daylight` job; `daylight.collect` records it (`daylight.record`);
//! `daylight.status` tells whether the latest study is stale.

use reviera_core::daylight::{self as dl, DaylightOptions};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
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
    pub options: Option<DaylightOptions>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub spaces: usize,
    pub sensors: usize,
}

pub(crate) struct DaylightRun;

impl HostCommand for DaylightRun {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "daylight.run",
        version: 1,
        summary: "Run a daylight factor study",
        description: "Starts a daylight factor study (ANA-LIGHT-002, ANA-ENV-006) as an analysis.daylight compute job: a sensor grid in each enclosed space (`options`: height 0.85 m, spacing 0.5 m, border 0.5 m from the walls, level_ids, every level when empty), the sky component under the CIE overcast sky by ray casting through the windows (visible transmittance from the window type's Pset_DoorWindowGlazingType.VisibleLightTransmittance, else glazing_transmittance 0.7) against walls, slabs, roofs and other building elements, plus the internally reflected component from the room's reflectances (ceiling 0.7, wall 0.5, floor 0.2). `threshold` (2%) counts the sensors that reach it. Returns the job; follow it with compute.status, then daylight.collect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let o = p.and_then(|p| p.options).unwrap_or_default();
        o.check()?;
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let (input, key_map) = dl::distill(&store, &o);
        if input.spaces.is_empty() {
            return Err(bad("daylight.no_spaces", "place enclosed MEP spaces first: the sensors are by space (Analyze › Spaces & Zones)"));
        }
        let hash = reviera_core::solar::hash_of(&input);
        let sensors = input.spaces.iter().map(|s| s.points.len()).sum();
        let distilled = reviera_compute::distill::daylight(&input, &key_map);
        let client = app.compute();
        let job_id = client.submit(reviera_compute::kinds::DAYLIGHT, None, &distilled, "", None).map_err(crate::compute::failed)?;
        client.set_context(&job_id, json!({ "daylight": { "options": o, "inputs_hash": hash } }));
        Ok(RunOut { job_id, spaces: input.spaces.len(), sensors })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    pub job_id: Option<String>,
}

pub(crate) struct DaylightCollect;

impl HostCommand for DaylightCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = dl::Report;
    const META: CommandMeta = CommandMeta {
        name: "daylight.collect",
        version: 1,
        summary: "Collect a daylight study",
        description: "Fetches a finished analysis.daylight job and keeps it with the model (daylight.record, one undoable step; the last five studies). Returns the report (daylight.results).",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<CollectParams>) -> Result<dl::Report> {
        use reviera_compute::kinds::{DAYLIGHT, DAYLIGHT_RESULTS};
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client.jobs().into_iter().filter(|j| j.kind == DAYLIGHT && j.state == "succeeded" && !j.results_fetched).collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| bad("daylight.no_job", "there is no finished daylight study to collect"))?;
        if job.state != "succeeded" {
            let why = job.failure.as_ref().map(|f| format!(": {}", f.message)).unwrap_or_default();
            return Err(bad("compute.not_ready", format!("the run is {}{why}", job.state)));
        }
        let (m, raw) = client.results(&job.job_id).map_err(crate::compute::failed)?;
        if m.schema != DAYLIGHT_RESULTS {
            return Err(bad("daylight.not_a_daylight_run", m.schema));
        }
        let results: dl::DaylightResults = serde_json::from_slice(&raw).map_err(|e| bad("compute.invalid_input", e.to_string()))?;
        let ctx = job.context.clone().unwrap_or_default();
        let c = &ctx["daylight"];
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        app.execute("daylight.record", json!({ "job_id": job.job_id, "options": c["options"], "results": results, "key_map": job.key_map, "inputs_hash": c["inputs_hash"], "at": at }), caller)?;
        let out = app.execute("daylight.results", json!({ "study_id": job.job_id }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("daylight.internal", e.to_string()))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub study_id: String,
    pub stale: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {
    pub study_id: Option<String>,
}

pub(crate) struct DaylightStatus;

impl HostCommand for DaylightStatus {
    type Host = App;
    type Params = Option<StatusParams>;
    type Output = StatusOut;
    const META: CommandMeta = CommandMeta {
        name: "daylight.status",
        version: 1,
        summary: "Whether a daylight study is stale",
        description: "Whether a daylight study (`study_id`, else the latest) is stale: the spaces, walls, windows, roofs or other obstructions, or the glazing's transmittance changed since it ran.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<StatusParams>) -> Result<StatusOut> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let st = dl::study(&store, p.and_then(|p| p.study_id).as_deref())?;
        let (input, _) = dl::distill(&store, &st.options);
        Ok(StatusOut { stale: reviera_core::solar::hash_of(&input) != st.inputs_hash, study_id: st.id })
    }
}
