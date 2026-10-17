// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural analysis (ANA-FEA-001): the analytical model, supports, cases, combinations and
//! loads are distilled to the neutral structural model and analysed out of process as an
//! `analysis.structural` compute job by the built-in linear static frame solver; collecting the
//! job keeps its results with the model, where `structural_analysis.results` reports them. The
//! model can also be exported as `reviera-structural-model.json` for other providers.

use reviera_compute::distill;
use reviera_compute::kinds::{STRUCTURAL, STRUCTURAL_RESULTS};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;
use crate::compute::failed;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// Only `linear_static` (the built-in provider's analysis).
    pub analysis_type: Option<String>,
    /// Run although the pre-analysis checks (STR-AM-007) found errors; they are recorded with
    /// the results.
    pub ignore_check_errors: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    pub nodes: usize,
    pub members: usize,
    pub cases: usize,
    pub combinations: usize,
    /// Check errors the run went ahead despite (issue keys).
    pub ignored_check_errors: Vec<String>,
}

pub(crate) struct StructuralAnalysisRun;

impl HostCommand for StructuralAnalysisRun {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "structural_analysis.run",
        version: 1,
        summary: "Run a structural analysis",
        description: "Analyses the analytical model (ANA-FEA-001): its members, nodes, supports, load cases, \
                      combinations and loads, distilled to the neutral structural model, run out of process on the \
                      compute host as an `analysis.structural` job by the built-in linear static 3D frame solver. \
                      Returns the job at once; follow it with `compute.status`, then `structural_analysis.collect`.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let p = p.unwrap_or_default();
        let force = p.ignore_check_errors.unwrap_or(false);
        let kind = p.analysis_type.unwrap_or_else(|| "linear_static".into());
        if kind != "linear_static" {
            return Err(Error::new(
                "structural.analysis_type",
                "the built-in provider runs linear static analyses",
            ));
        }
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store();
        // Pre-analysis checks: errors that are not ignored hold the run.
        let checks = reviera_core::analytical_checks::run(store, true, true, &None)?;
        let errors: Vec<&reviera_core::analytical_checks::Issue> = checks.errors().collect();
        if !errors.is_empty() && !force {
            let lines: Vec<String> = errors
                .iter()
                .map(|i| format!("{} {}: {}", i.code, i.title, i.message))
                .collect();
            return Err(Error::new(
                "structural.check_errors",
                format!(
                    "the analytical model checks found {} error(s); fix them, ignore them, or pass \
                     `ignore_check_errors`:\n{}",
                    errors.len(),
                    lines.join("\n")
                ),
            )
            .with_details(json!({ "issues": errors })));
        }
        let ignored_check_errors: Vec<String> = errors.iter().map(|i| i.key.clone()).collect();
        let d = reviera_core::structural_model::distill(store)?;
        let out = RunOut {
            ignored_check_errors,
            job_id: String::new(),
            nodes: d.model.nodes.len(),
            members: d.model.members.len(),
            cases: d.model.cases.len(),
            combinations: d.model.combinations.len(),
        };
        if out.cases == 0 {
            return Err(Error::new(
                "structural.no_cases",
                "the model has no load cases to analyse",
            ));
        }
        let distilled = distill::structural_model(&d);
        let client = app.compute();
        let job_id = client
            .submit(STRUCTURAL, None, &distilled, "", None)
            .map_err(failed)?;
        client.set_context(
            &job_id,
            json!({ "structural": {
                "analysis_type": kind,
                "ignored_check_errors": out.ignored_check_errors,
            } }),
        );
        Ok(RunOut { job_id, ..out })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    /// Default the latest finished structural analysis not yet collected.
    pub job_id: Option<String>,
}

pub(crate) struct StructuralAnalysisCollect;

impl HostCommand for StructuralAnalysisCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = reviera_core::structural_model::Report;
    const META: CommandMeta = CommandMeta {
        name: "structural_analysis.collect",
        version: 1,
        summary: "Collect a finished structural analysis",
        description: "Fetches a finished structural analysis's results from the compute host, keeps them with the \
                      model (`structural_analysis.record`, one undoable step) and returns their report.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(
        &self,
        app: &mut App,
        caller: &Caller,
        p: Option<CollectParams>,
    ) -> Result<reviera_core::structural_model::Report> {
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client
                    .jobs()
                    .into_iter()
                    .filter(|j| {
                        j.kind == STRUCTURAL && j.state == "succeeded" && !j.results_fetched
                    })
                    .collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| {
            Error::new(
                "structural.no_run",
                "there is no finished structural analysis to collect",
            )
        })?;
        if job.state != "succeeded" {
            let why = job
                .failure
                .as_ref()
                .map(|f| format!(": {}", f.message))
                .unwrap_or_default();
            return Err(Error::new(
                "compute.not_ready",
                format!("the analysis is {}{why}", job.state),
            ));
        }
        let (m, raw) = client.results(&job.job_id).map_err(failed)?;
        if m.schema != STRUCTURAL_RESULTS {
            return Err(Error::new("structural.not_a_structural_run", m.schema));
        }
        let results: reviera_core::frame::Results = serde_json::from_slice(&raw)
            .map_err(|e| Error::new("compute.invalid_input", e.to_string()))?;
        app.execute(
            "structural_analysis.record",
            json!({
                "job_id": job.job_id,
                "provider": "reviera-analysis-builtin (linear static 3D frame)",
                "results": results,
                "key_map": job.key_map,
                "ignored_check_errors": job
                    .context
                    .as_ref()
                    .and_then(|c| c["structural"].get("ignored_check_errors").cloned())
                    .unwrap_or_else(|| json!([])),
            }),
            caller,
        )?;
        app.compute().ack(&job.job_id).map_err(failed)?;
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        reviera_core::structural_model::report(app.get(doc)?.doc.store(), &Default::default())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub path: std::path::PathBuf,
}

pub(crate) struct StructuralAnalysisExport;

impl HostCommand for StructuralAnalysisExport {
    type Host = App;
    type Params = ExportParams;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "structural_analysis.export_model",
        version: 1,
        summary: "Export the structural model",
        description: "Writes the neutral structural model (`reviera-structural-model.json`, ANA-FEA-001) for analysis \
                      providers that do not read IFC: nodes with supports, members with their section properties, \
                      moduli, releases and local axes, load cases, combinations and loads, in SI units.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<RunOut> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let d = reviera_core::structural_model::distill(app.get(doc)?.doc.store())?;
        let text = serde_json::to_string_pretty(&d.model).expect("serialises");
        std::fs::write(&p.path, text)
            .map_err(|e| Error::new("file.write_failed", format!("{}: {e}", p.path.display())))?;
        Ok(RunOut {
            job_id: String::new(),
            nodes: d.model.nodes.len(),
            members: d.model.members.len(),
            cases: d.model.cases.len(),
            combinations: d.model.combinations.len(),
            ignored_check_errors: Vec::new(),
        })
    }
}
