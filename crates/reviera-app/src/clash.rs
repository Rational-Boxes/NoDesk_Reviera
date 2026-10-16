// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Clash detection (COL-CLASH-001, COL-CLASH-002, ANA-CLASH-001): a clash test of set A against
//! set B runs out of process as an `analysis.clash` compute job (DM-RPC-001); collecting it
//! merges its results into the model's clash results (`clash.record_run`), where they keep a
//! status between runs; the report lists them with their elements for navigation.

use std::collections::BTreeMap;

use prost::Message;
use reviera_compute::distill;
use reviera_compute::kinds::{CLASH, CLASH_RESULT};
use reviera_compute::proto::analysis::v1 as an;
use reviera_compute::proto::to_any;
use reviera_core::clashes::{self, Record, Status, Test};
use reviera_core::{Caller, CommandMeta, Effect, ElementId, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;
use crate::compute::failed;

/// One set of a clash test.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Set {
    /// IFC classes (for example `IfcWall`); empty or missing: every element.
    #[serde(default)]
    pub classes: Vec<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    pub set_a: Option<Set>,
    pub set_b: Option<Set>,
    /// Touching within this is not a clash (metres; default 1 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    pub job_id: String,
    /// Elements in the test.
    pub elements: usize,
    /// Bytes uploaded (what the compute host lacked).
    pub uploaded_bytes: u64,
}

fn in_set(set: &[String], class: &str) -> bool {
    set.is_empty() || set.iter().any(|c| c.eq_ignore_ascii_case(class))
}

/// Starts a clash test of set A against set B as a compute job.
fn run_test(
    app: &mut App,
    set_a: Vec<String>,
    set_b: Vec<String>,
    tolerance: f64,
) -> Result<RunOut> {
    let doc = app
        .active()
        .ok_or_else(|| Error::new("document.none", "no document is open"))?;
    let bodies = app.bodies()?;
    let store = app.get(doc)?.doc.store();
    let class_of = |id: ElementId| store.get(id).map(|e| e.class.clone()).unwrap_or_default();
    let bodies: Vec<_> = bodies
        .into_iter()
        .filter(|b| {
            let c = class_of(b.element);
            in_set(&set_a, &c) || in_set(&set_b, &c)
        })
        .collect();
    let excluded: Vec<(ElementId, ElementId)> = distill::candidate_pairs(&bodies, tolerance)
        .into_iter()
        .filter(|&(a, b)| {
            reviera_core::bodies::related(store, a, b)
                || reviera_core::overlap_accept::is_accepted(store, a, b)
        })
        .collect();
    let guid = |id: ElementId| store.get(id).map(|e| e.guid().to_string());
    let sets = |id: ElementId| {
        let c = class_of(id);
        u32::from(in_set(&set_a, &c)) | (u32::from(in_set(&set_b, &c)) << 1)
    };
    let d = distill::clash_bodies_in_sets(&bodies, &guid, &sets, &excluded, "");
    let options = an::ClashOptions {
        tolerance,
        min_volume: 1e-6,
        resolution: 48,
        limit: 0,
        against_models: Vec::new(),
        between_sets: true,
    };
    let client = app.compute();
    let before = client.uploaded_bytes();
    let job_id = client
        .submit(CLASH, Some(to_any(&options)), &d, "", None)
        .map_err(failed)?;
    client.set_context(
        &job_id,
        json!({ "clash_test": { "set_a": set_a, "set_b": set_b, "tolerance": tolerance } }),
    );
    Ok(RunOut {
        job_id,
        elements: d.elements,
        uploaded_bytes: client.uploaded_bytes() - before,
    })
}

pub(crate) struct ClashRunBasic;

impl HostCommand for ClashRunBasic {
    type Host = App;
    type Params = Option<RunParams>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "clash.run_basic",
        version: 1,
        summary: "Run a clash test",
        description: "Tests the elements of set A against those of set B (each a list of IFC classes; empty means every \
                      element) for clashes (COL-CLASH-001). The test runs out of process on the compute host \
                      (`analysis.clash`), with elements that belong together by design and accepted overlaps left \
                      out; returns the job at once. Follow it with `compute.status`, then `clash.collect` merges its \
                      results into the model's clash results.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<RunParams>) -> Result<RunOut> {
        let p = p.unwrap_or_default();
        run_test(
            app,
            p.set_a.unwrap_or_default().classes,
            p.set_b.unwrap_or_default().classes,
            p.tolerance.unwrap_or(0.001),
        )
    }
}

pub(crate) struct ClashRefresh;

impl HostCommand for ClashRefresh {
    type Host = App;
    type Params = Option<serde_json::Value>;
    type Output = RunOut;
    const META: CommandMeta = CommandMeta {
        name: "clash.refresh",
        version: 1,
        summary: "Run the last clash test again",
        description: "Runs the model's last clash test again with the same sets and tolerance (COL-CLASH-001); \
                      `clash.collect` then updates the results' statuses.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<serde_json::Value>) -> Result<RunOut> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let t = clashes::test(app.get(doc)?.doc.store())
            .ok_or_else(|| Error::new("clash.no_test", "no clash test has run in this model"))?;
        run_test(app, t.set_a, t.set_b, t.tolerance)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectParams {
    /// The clash job; default the newest one.
    pub job_id: Option<String>,
}

pub(crate) struct ClashCollect;

impl HostCommand for ClashCollect {
    type Host = App;
    type Params = Option<CollectParams>;
    type Output = clashes::RunSummary;
    const META: CommandMeta = CommandMeta {
        name: "clash.collect",
        version: 1,
        summary: "Collect a finished clash test",
        description: "Fetches a finished clash test's results from the compute host and merges them into the model's \
                      clash results (`clash.record_run`): new clashes are *new*, ones found again *active*, and ones \
                      no longer found *resolved*. One undoable step. Returns what changed.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(
        &self,
        app: &mut App,
        caller: &Caller,
        p: Option<CollectParams>,
    ) -> Result<clashes::RunSummary> {
        let client = app.compute();
        let job = match p.and_then(|p| p.job_id) {
            Some(id) => client.status(&id),
            None => {
                let mut jobs: Vec<_> = client
                    .jobs()
                    .into_iter()
                    .filter(|j| j.kind == CLASH && j.state == "succeeded" && !j.results_fetched)
                    .collect();
                jobs.sort_by_key(|j| j.submitted_ms);
                jobs.pop()
            }
        }
        .ok_or_else(|| Error::new("clash.no_run", "there is no finished clash test to collect"))?;
        if job.state != "succeeded" {
            return Err(Error::new(
                "compute.not_ready",
                format!("the clash test is {}", job.state),
            ));
        }
        let (m, raw) = client.results(&job.job_id).map_err(failed)?;
        if m.schema != CLASH_RESULT {
            return Err(Error::new("clash.not_a_clash_run", m.schema));
        }
        let r = an::ClashResult::decode(raw.as_slice())
            .map_err(|e| Error::new("compute.invalid_input", e.to_string()))?;
        let guid = |k: &str| job.key_map.get(k).cloned();
        let found: Vec<serde_json::Value> = r
            .clashes
            .iter()
            .filter_map(|c| {
                let (a, b) = (guid(&c.a)?, guid(&c.b)?);
                let v3 = |v: &[f64]| [v[0], v[1], v[2]];
                Some(json!({
                    "a": a, "b": b, "volume": c.volume, "depth": c.depth,
                    "min": v3(&c.min), "max": v3(&c.max), "centroid": v3(&c.centroid),
                }))
            })
            .collect();
        let test = job
            .context
            .as_ref()
            .and_then(|c| c.get("clash_test"))
            .cloned()
            .unwrap_or_else(|| json!({ "set_a": [], "set_b": [], "tolerance": 0.001 }));
        let summary = app.execute(
            "clash.record_run",
            json!({
                "set_a": test["set_a"], "set_b": test["set_b"], "tolerance": test["tolerance"],
                "bodies": r.bodies, "job_id": job.job_id, "found": found,
            }),
            caller,
        )?;
        app.compute().ack(&job.job_id).map_err(failed)?;
        serde_json::from_value(summary.result)
            .map_err(|e| Error::new("clash.invalid", e.to_string()))
    }
}

/// A clash result with its elements, for the report.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReportItem {
    #[serde(flatten)]
    pub record: Record,
    /// The elements, if they are still in the model.
    pub a_id: Option<ElementId>,
    pub b_id: Option<ElementId>,
    pub a_label: String,
    pub b_label: String,
    pub a_class: String,
    pub b_class: String,
    /// The pair of IFC classes, for grouping ("IfcSlab × IfcWall").
    pub group: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Report {
    pub test: Option<Test>,
    pub items: Vec<ReportItem>,
    /// Results by status.
    pub counts: BTreeMap<String, usize>,
}

/// The report of the model's clash results.
pub fn report(app: &App) -> Result<Report> {
    let doc = app
        .active()
        .ok_or_else(|| Error::new("document.none", "no document is open"))?;
    let store = app.get(doc)?.doc.store();
    let find = |g: &str| reviera_core::GlobalId::parse(g).and_then(|g| store.find_by_guid(&g));
    let describe = |id: Option<ElementId>, g: &str| -> (String, String) {
        match id.and_then(|i| store.get(i).map(|e| (i, e))) {
            Some((i, e)) => (
                match &e.name {
                    Some(n) => format!("{} {} ({n})", e.class, i.0),
                    None => format!("{} {}", e.class, i.0),
                },
                e.class.clone(),
            ),
            None => (format!("(deleted: {g})"), String::new()),
        }
    };
    let mut counts = BTreeMap::new();
    let items = clashes::results(store)
        .into_iter()
        .map(|record| {
            *counts.entry(record.status.name().to_owned()).or_insert(0) += 1;
            let (a_id, b_id) = (find(&record.a), find(&record.b));
            let (a_label, a_class) = describe(a_id, &record.a);
            let (b_label, b_class) = describe(b_id, &record.b);
            let mut pair = [a_class.clone(), b_class.clone()];
            pair.sort();
            ReportItem {
                group: format!("{} × {}", pair[0], pair[1]),
                record,
                a_id,
                b_id,
                a_label,
                b_label,
                a_class,
                b_class,
            }
        })
        .collect();
    Ok(Report {
        test: clashes::test(store),
        items,
        counts,
    })
}

pub(crate) struct ClashLastReport;

impl HostCommand for ClashLastReport {
    type Host = App;
    type Params = Option<serde_json::Value>;
    type Output = Report;
    const META: CommandMeta = CommandMeta {
        name: "clash.last_report",
        version: 1,
        summary: "The model's clash results",
        description: "The last clash test and its results (COL-CLASH-001, COL-CLASH-002): each with its status, its \
                      elements, their category pair, volume, depth, box, centroid and comments; resolved ones last.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<serde_json::Value>) -> Result<Report> {
        report(app)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Html,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub path: std::path::PathBuf,
    pub format: Option<ExportFormat>,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) struct ClashExportReport;

impl HostCommand for ClashExportReport {
    type Host = App;
    type Params = ExportParams;
    type Output = usize;
    const META: CommandMeta = CommandMeta {
        name: "clash.export_report",
        version: 1,
        summary: "Export the clash report",
        description: "Writes the model's clash results to a CSV or HTML file (COL-CLASH-001): status, elements and \
                      their GlobalIds, category pair, volume, depth, centroid, the runs that found it, and comments. \
                      The format follows the extension unless given. Returns the number of results written.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<usize> {
        let r = report(app)?;
        let format = p.format.unwrap_or_else(|| {
            if p.path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("htm") || e.eq_ignore_ascii_case("html"))
            {
                ExportFormat::Html
            } else {
                ExportFormat::Csv
            }
        });
        let comments = |i: &ReportItem| {
            i.record
                .comments
                .iter()
                .map(|c| format!("{} ({}): {}", c.by, c.at, c.text))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let text = match format {
            ExportFormat::Csv => {
                let mut out = String::from(
                    "id,status,group,element_a,globalid_a,element_b,globalid_b,volume_m3,depth_m,centroid_x,centroid_y,centroid_z,first_run,last_run,comments\n",
                );
                for i in &r.items {
                    let c = i.record.centroid;
                    out.push_str(&format!(
                        "{},{},{},{},{},{},{},{:.6},{:.4},{:.3},{:.3},{:.3},{},{},{}\n",
                        i.record.id,
                        i.record.status.name(),
                        csv_field(&i.group),
                        csv_field(&i.a_label),
                        i.record.a,
                        csv_field(&i.b_label),
                        i.record.b,
                        i.record.volume,
                        i.record.depth,
                        c[0],
                        c[1],
                        c[2],
                        i.record.first_run,
                        i.record.last_run,
                        csv_field(&comments(i)),
                    ));
                }
                out
            }
            ExportFormat::Html => {
                let mut rows = String::new();
                for (n, i) in r.items.iter().enumerate() {
                    rows.push_str(&format!(
                        "<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.4} m³</td><td>{:.3} m</td><td>{}</td></tr>\n",
                        i.record.status.name(),
                        n + 1,
                        i.record.status.name(),
                        html_text(&i.group),
                        html_text(&i.a_label),
                        html_text(&i.b_label),
                        i.record.volume,
                        i.record.depth,
                        html_text(&comments(i)),
                    ));
                }
                let summary = r
                    .counts
                    .iter()
                    .map(|(k, v)| format!("{v} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Clash report</title>\n<style>body{{font-family:sans-serif}}table{{border-collapse:collapse}}td,th{{border:1px solid #bbb;padding:4px 8px}}tr.resolved{{color:#888}}tr.new td:nth-child(2){{color:#c00;font-weight:bold}}</style></head>\n<body><h1>Clash report</h1><p>{}</p>\n<table><tr><th>#</th><th>Status</th><th>Group</th><th>Element A</th><th>Element B</th><th>Volume</th><th>Depth</th><th>Comments</th></tr>\n{rows}</table></body></html>\n",
                    html_text(&summary)
                )
            }
        };
        std::fs::write(&p.path, text)
            .map_err(|e| Error::new("file.write_failed", format!("{}: {e}", p.path.display())))?;
        Ok(r.items.len())
    }
}

/// Status names, for parsing user input.
pub fn parse_status(s: &str) -> Option<Status> {
    serde_json::from_value(json!(s)).ok()
}
