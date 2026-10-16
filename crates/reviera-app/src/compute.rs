// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `compute.*`: running job kinds out of process on the local compute host (spec Ch 2 §2.17,
//! DM-RPC-001 … 009), starting with the project-wide clash test `analysis.clash`
//! (ANA-CLASH-001): the whole model's bodies are distilled and measured by the built-in runner,
//! on the same overlap engine as the live checks, without loading Reviera's process.

use std::collections::BTreeMap;

use prost::Message;
use reviera_compute::client::{ComputeClient, LocalJob, state_name};
use reviera_compute::distill::{self, Distilled};
use reviera_compute::kinds::CLASH;
use reviera_compute::proto::analysis::v1 as an;
use reviera_compute::proto::to_any;
use reviera_core::{Caller, CommandMeta, Effect, ElementId, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

impl App {
    /// The compute client, created on first use.
    fn compute(&mut self) -> &ComputeClient {
        self.compute.get_or_insert_with(ComputeClient::local)
    }
}

/// A contract reason code as an error code.
fn failed(e: reviera_compute::ComputeError) -> Error {
    let code = match e.reason.as_str() {
        "invalid_input" => "compute.invalid_input",
        "unsupported_kind" => "compute.unsupported_kind",
        "version_mismatch" => "compute.version_mismatch",
        "too_large" => "compute.too_large",
        "limit_exceeded" => "compute.limit_exceeded",
        "memory_limit" => "compute.memory_limit",
        "cancelled" => "compute.cancelled",
        "runner_failed" => "compute.runner_failed",
        "unauthorised" => "compute.unauthorised",
        "negotiation_failed" => "compute.negotiation_failed",
        "host_restarted" => "compute.host_restarted",
        "not_found" => "compute.not_found",
        _ => "compute.unavailable",
    };
    Error::new(code, e.message)
}

/// Which elements a distillation takes.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Only elements of these IFC classes (for example `IfcWall`); default all.
    pub classes: Option<Vec<String>>,
    /// Include pairs that belong together by design (a window and its wall); default false.
    #[serde(default)]
    pub include_related: bool,
    /// Include overlaps accepted as intended; default false.
    #[serde(default)]
    pub include_accepted: bool,
}

/// The clash distillation of the open model.
fn distil_clash(app: &App, scope: &Scope) -> Result<Distilled> {
    let doc = app
        .active()
        .ok_or_else(|| Error::new("document.none", "no document is open"))?;
    let store = app.get(doc)?.doc.store();
    let bodies: Vec<_> = app
        .bodies()?
        .into_iter()
        .filter(|b| {
            scope.classes.as_ref().is_none_or(|cs| {
                store
                    .get(b.element)
                    .is_some_and(|e| cs.iter().any(|c| c.eq_ignore_ascii_case(&e.class)))
            })
        })
        .collect();
    let excluded: Vec<(ElementId, ElementId)> = distill::candidate_pairs(&bodies, 0.001)
        .into_iter()
        .filter(|&(a, b)| {
            (!scope.include_related && reviera_core::bodies::related(store, a, b))
                || (!scope.include_accepted
                    && reviera_core::overlap_accept::is_accepted(store, a, b))
        })
        .collect();
    let guid = |id: ElementId| store.get(id).map(|e| e.guid().to_string());
    Ok(distill::clash_bodies(&bodies, &guid, &excluded, ""))
}

fn distil(app: &App, kind: &str, scope: &Scope) -> Result<Distilled> {
    match kind {
        CLASH => distil_clash(app, scope),
        other => Err(Error::new(
            "compute.unsupported_kind",
            format!("no distiller for {other} in Reviera"),
        )),
    }
}

// ---- compute.kinds ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct KindOut {
    pub kind: String,
    pub version: u32,
    pub runner: String,
    pub runner_version: String,
    pub licence: String,
    pub capabilities: Vec<String>,
}

pub(crate) struct ComputeKinds;

impl HostCommand for ComputeKinds {
    type Host = App;
    type Params = Option<serde_json::Value>;
    type Output = Vec<KindOut>;
    const META: CommandMeta = CommandMeta {
        name: "compute.kinds",
        version: 1,
        summary: "List the job kinds the compute host offers",
        description: "Lists the job kinds that can run out of process (DM-RPC-002), with their runner and its \
                      licence. Starts the local compute host if it is not running; it exits again when idle.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<serde_json::Value>) -> Result<Vec<KindOut>> {
        let kinds = app.compute().kinds().map_err(failed)?;
        Ok(kinds
            .into_iter()
            .filter(|k| !k.kind.starts_with("test."))
            .map(|k| KindOut {
                kind: k.kind,
                version: k.version,
                runner: k.runner,
                runner_version: k.runner_version,
                licence: k.licence,
                capabilities: k.capabilities,
            })
            .collect())
    }
}

// ---- compute.distill ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DistillParams {
    /// The job kind, for example `analysis.clash`.
    pub kind: String,
    pub scope: Option<Scope>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DistillOut {
    pub distiller: String,
    pub version: u32,
    pub detail: String,
    pub anonymised: bool,
    pub blobs: usize,
    pub size_bytes: u64,
    pub elements: usize,
    pub triangles: usize,
    pub snapshot_hash: String,
}

fn distill_out(d: &Distilled) -> DistillOut {
    DistillOut {
        distiller: d.distiller.clone(),
        version: d.version,
        detail: d.detail.clone(),
        anonymised: d.anonymised,
        blobs: d.blobs.len(),
        size_bytes: d.size_bytes(),
        elements: d.elements,
        triangles: d.triangles,
        snapshot_hash: d.snapshot_hash.clone(),
    }
}

pub(crate) struct ComputeDistill;

impl HostCommand for ComputeDistill {
    type Host = App;
    type Params = DistillParams;
    type Output = DistillOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.distill",
        version: 1,
        summary: "Show what a job would send",
        description: "Makes the distillation a job of this kind would send (DM-RPC-003) and reports its size, \
                      chunks and contents, without sending anything. For `analysis.clash`: the bodies of the \
                      chosen elements with booleans applied, under opaque keys, and the pairs left out.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: DistillParams) -> Result<DistillOut> {
        Ok(distill_out(&distil(
            app,
            &p.kind,
            &p.scope.unwrap_or_default(),
        )?))
    }
}

// ---- compute.submit ----

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClashOptions {
    /// Overlaps thinner than this are contact (metres; default 1 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
    /// Smallest clash reported, cubic metres (default 1 cm³).
    pub min_volume: Option<f64>,
    /// Measuring lines across each clash, each way (default 48).
    pub resolution: Option<u32>,
    /// At most this many clashes, largest first (default all).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitParams {
    /// The job kind, for example `analysis.clash`.
    pub kind: String,
    pub scope: Option<Scope>,
    /// The kind's options.
    pub options: Option<ClashOptions>,
    /// Where to run it; only `local` for now.
    pub location: Option<String>,
    /// Submitting the same key again returns the existing job.
    pub client_job_key: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SubmitOut {
    pub job_id: String,
    pub distillation: DistillOut,
    /// Bytes uploaded for this job (what the host lacked).
    pub uploaded_bytes: u64,
}

pub(crate) struct ComputeSubmit;

impl HostCommand for ComputeSubmit {
    type Host = App;
    type Params = SubmitParams;
    type Output = SubmitOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.submit",
        version: 1,
        summary: "Run a job out of process",
        description: "Distils the model for a job kind, uploads only what the compute host lacks, and submits the \
                      job (DM-RPC-001); returns its ID at once. The job runs in a separate process under memory \
                      and CPU limits and outlives Reviera; follow it with `compute.status` and collect it with \
                      `compute.results`. `analysis.clash` is the project-wide clash test (ANA-CLASH-001).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: SubmitParams) -> Result<SubmitOut> {
        if let Some(l) = p.location.as_deref()
            && l != "local"
        {
            return Err(Error::new(
                "compute.unsupported_location",
                "only local compute is available for now",
            ));
        }
        let d = distil(app, &p.kind, &p.scope.unwrap_or_default())?;
        let options = match p.kind.as_str() {
            CLASH => {
                let o = p.options.unwrap_or_default();
                Some(to_any(&an::ClashOptions {
                    tolerance: o.tolerance.unwrap_or(0.001),
                    min_volume: o.min_volume.unwrap_or(1e-6),
                    resolution: o.resolution.unwrap_or(48),
                    limit: o.limit.unwrap_or(0),
                    against_models: Vec::new(),
                }))
            }
            _ => None,
        };
        let project = String::new();
        let client = app.compute();
        let before = client.uploaded_bytes();
        let job_id = client
            .submit(&p.kind, options, &d, &project, p.client_job_key)
            .map_err(failed)?;
        Ok(SubmitOut {
            job_id,
            distillation: distill_out(&d),
            uploaded_bytes: client.uploaded_bytes() - before,
        })
    }
}

// ---- compute.status, compute.jobs, compute.reattach, compute.cancel, compute.forget_job ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobParams {
    pub job_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct JobOut {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: f32,
    pub phase: String,
    pub failure: Option<FailureOut>,
    pub submitted_ms: i64,
    pub results_fetched: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FailureOut {
    pub reason: String,
    pub message: String,
    pub runner_output: String,
}

fn job_out(j: LocalJob) -> JobOut {
    JobOut {
        job_id: j.job_id,
        kind: j.kind,
        state: j.state,
        progress: j.progress,
        phase: j.phase,
        failure: j.failure.map(|f| FailureOut {
            reason: f.reason,
            message: f.message,
            runner_output: f.runner_output,
        }),
        submitted_ms: j.submitted_ms,
        results_fetched: j.results_fetched,
    }
}

pub(crate) struct ComputeStatus;

impl HostCommand for ComputeStatus {
    type Host = App;
    type Params = JobParams;
    type Output = JobOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.status",
        version: 1,
        summary: "A job's state and progress",
        description: "A job's state (queued, starting, running, succeeded, failed, cancelled), progress and \
                      phase, and for a failure its reason and the runner's last output (DM-RPC-001).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: JobParams) -> Result<JobOut> {
        app.compute()
            .status(&p.job_id)
            .map(job_out)
            .ok_or_else(|| Error::new("compute.not_found", format!("no job {}", p.job_id)))
    }
}

pub(crate) struct ComputeJobs;

impl HostCommand for ComputeJobs {
    type Host = App;
    type Params = Option<serde_json::Value>;
    type Output = Vec<JobOut>;
    const META: CommandMeta = CommandMeta {
        name: "compute.jobs",
        version: 1,
        summary: "List compute jobs",
        description: "The jobs this user has submitted, newest last, from the client's record (DM-RPC-008).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<serde_json::Value>) -> Result<Vec<JobOut>> {
        let mut jobs = app.compute().jobs();
        jobs.sort_by_key(|j| j.submitted_ms);
        Ok(jobs.into_iter().map(job_out).collect())
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReattachParams {
    /// One job; default every outstanding job.
    pub job_id: Option<String>,
}

pub(crate) struct ComputeReattach;

impl HostCommand for ComputeReattach {
    type Host = App;
    type Params = Option<ReattachParams>;
    type Output = Vec<JobOut>;
    const META: CommandMeta = CommandMeta {
        name: "compute.reattach",
        version: 1,
        summary: "Follow outstanding jobs again",
        description: "Reconnects to jobs that have not finished, or whose results have not been collected, and \
                      follows them from their last event (DM-RPC-009). Jobs keep running while Reviera is closed.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<ReattachParams>) -> Result<Vec<JobOut>> {
        let id = p.and_then(|p| p.job_id);
        Ok(app
            .compute()
            .reattach(id.as_deref())
            .into_iter()
            .map(job_out)
            .collect())
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CancelOut {
    pub state: String,
}

pub(crate) struct ComputeCancel;

impl HostCommand for ComputeCancel {
    type Host = App;
    type Params = JobParams;
    type Output = CancelOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.cancel",
        version: 1,
        summary: "Cancel a job",
        description: "Cancels a queued or running job; its runner stops and frees its memory (DM-RPC-001).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: JobParams) -> Result<CancelOut> {
        let state = app.compute().cancel(&p.job_id).map_err(failed)?;
        Ok(CancelOut { state })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForgetOut {
    pub forgotten: bool,
}

pub(crate) struct ComputeForgetJob;

impl HostCommand for ComputeForgetJob {
    type Host = App;
    type Params = JobParams;
    type Output = ForgetOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.forget_job",
        version: 1,
        summary: "Forget a job",
        description: "Removes a job from this client's record (DM-RPC-009). A job still running is cancelled \
                      first.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: JobParams) -> Result<ForgetOut> {
        let c = app.compute();
        if c.status(&p.job_id).is_some_and(|j| !j.terminal()) {
            let _ = c.cancel(&p.job_id);
        }
        Ok(ForgetOut {
            forgotten: c.forget(&p.job_id),
        })
    }
}

// ---- compute.results ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultsParams {
    pub job_id: String,
    /// Tell the host the results are kept, so it may release them (default true).
    pub acknowledge: Option<bool>,
}

/// One clash, with its elements.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ClashOut {
    pub a: Option<ElementId>,
    pub b: Option<ElementId>,
    pub a_label: String,
    pub b_label: String,
    pub volume: f64,
    pub depth: f64,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub centroid: Vec<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Provenance {
    pub kind: String,
    pub kind_version: u32,
    pub runner: String,
    pub runner_version: String,
    pub distiller: String,
    pub snapshot_hash: String,
    pub host: String,
    pub started_ms: i64,
    pub finished_ms: i64,
    pub schema: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResultsOut {
    pub provenance: Provenance,
    /// For `analysis.clash`: the clashes, largest first.
    pub clashes: Option<Vec<ClashOut>>,
    /// Bodies the clash test checked.
    pub bodies: Option<u32>,
}

pub(crate) struct ComputeResults;

impl HostCommand for ComputeResults {
    type Host = App;
    type Params = ResultsParams;
    type Output = ResultsOut;
    const META: CommandMeta = CommandMeta {
        name: "compute.results",
        version: 1,
        summary: "Collect a job's results",
        description: "Downloads a finished job's results (resuming if the connection breaks), checks them, and \
                      attaches them to the model's elements through the keys kept locally. Returns them with \
                      their provenance (DM-RPC-001). For `analysis.clash`, the clashes with their elements.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ResultsParams) -> Result<ResultsOut> {
        let job = app
            .compute()
            .status(&p.job_id)
            .ok_or_else(|| Error::new("compute.not_found", format!("no job {}", p.job_id)))?;
        if job.state != state_name(reviera_compute::proto::pb::JobState::Succeeded as i32) {
            return Err(Error::new(
                "compute.not_ready",
                format!("the job is {}", job.state),
            ));
        }
        let (m, raw) = app.compute().results(&p.job_id).map_err(failed)?;
        let provenance = Provenance {
            kind: m.kind.clone(),
            kind_version: m.kind_version,
            runner: m.runner.clone(),
            runner_version: m.runner_version.clone(),
            distiller: m
                .distillation
                .as_ref()
                .map(|d| format!("{} {}", d.distiller, d.distiller_version))
                .unwrap_or_default(),
            snapshot_hash: m.snapshot_hash.clone(),
            host: m.host.clone(),
            started_ms: m.started_ms,
            finished_ms: m.finished_ms,
            schema: m.schema.clone(),
            sha256: m.sha256.clone(),
            size: m.size,
        };
        let mut out = ResultsOut {
            provenance,
            clashes: None,
            bodies: None,
        };
        if m.schema == reviera_compute::kinds::CLASH_RESULT {
            let r = an::ClashResult::decode(raw.as_slice())
                .map_err(|e| Error::new("compute.invalid_input", e.to_string()))?;
            let doc = app
                .active()
                .ok_or_else(|| Error::new("document.none", "no document is open"))?;
            let store = app.get(doc)?.doc.store();
            let element = |key: &str| -> Option<ElementId> {
                let guid = job.key_map.get(key)?;
                let g = reviera_core::GlobalId::parse(guid)?;
                store.find_by_guid(&g)
            };
            let label = |id: Option<ElementId>, key: &str| match id
                .and_then(|i| store.get(i).map(|e| (i, e)))
            {
                Some((i, e)) => match &e.name {
                    Some(n) => format!("{} {} ({n})", e.class, i.0),
                    None => format!("{} {}", e.class, i.0),
                },
                None => format!("(no longer in the model: {key})"),
            };
            let by_key: BTreeMap<&str, Option<ElementId>> = r
                .clashes
                .iter()
                .flat_map(|c| [c.a.as_str(), c.b.as_str()])
                .map(|k| (k, element(k)))
                .collect();
            out.bodies = Some(r.bodies);
            out.clashes = Some(
                r.clashes
                    .iter()
                    .map(|c| {
                        let (a, b) = (by_key[c.a.as_str()], by_key[c.b.as_str()]);
                        ClashOut {
                            a,
                            b,
                            a_label: label(a, &c.a),
                            b_label: label(b, &c.b),
                            volume: c.volume,
                            depth: c.depth,
                            min: c.min.clone(),
                            max: c.max.clone(),
                            centroid: c.centroid.clone(),
                        }
                    })
                    .collect(),
            );
        }
        if p.acknowledge.unwrap_or(true) {
            app.compute().ack(&p.job_id).map_err(failed)?;
        }
        Ok(out)
    }
}
