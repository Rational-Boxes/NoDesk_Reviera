// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `profiler.*`: the built-in performance profiler's commands (spec Ch 2 §2.18, DM-PERF-001),
//! over `reviera_core::profiler`. They are application commands, so a report can state the
//! open model's size. Reports hold operation names, counts and times only.

use std::path::PathBuf;

use reviera_core::profiler::{self, Context, Live, Report, Sort};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

#[derive(Debug, Serialize, JsonSchema)]
pub struct Status {
    pub enabled: bool,
    /// Seconds recorded so far.
    pub seconds: f64,
    pub events: usize,
    pub slow_ms: f64,
}

fn status() -> Status {
    let r = profiler::report(Some(0), Sort::Name);
    Status { enabled: profiler::enabled(), seconds: r.seconds, events: r.events, slow_ms: r.slow_ms }
}

/// This machine and build, with the active model's size (its IFC entities only when `full`:
/// counting them walks the file).
fn context(app: &App, full: bool) -> Context {
    let mut c = Context::here();
    if let Some(d) = app.active().and_then(|id| app.document(id)) {
        c.elements = Some(d.doc.store().len());
        if full {
            c.ifc_entities = Some(d.project.ifc().entity_count());
        }
    }
    c
}

macro_rules! host {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, [$($eff:ident),*], $params:ty, $out:ty, |$app:ident, $p:ident| $body:block) => {
        pub(crate) struct $ty;
        impl HostCommand for $ty {
            type Host = App;
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta { name: $name, version: 1, summary: $summary, description: $desc, effects: &[$(Effect::$eff),*], transaction_name: "" };
            #[allow(unused_variables)]
            fn run(&self, $app: &mut App, _: &Caller, $p: $params) -> Result<$out> $body
        }
    };
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartParams {
    /// Operations at or over this many milliseconds are listed as slow (default 100).
    pub slow_ms: Option<f64>,
}

host!(ProfilerStart, "profiler.start", "Start the performance profiler",
    "Starts recording (DM-PERF-001), forgetting what was recorded before: every command, commit, \
     regeneration, scene build, IFC open and save, live-store write, merge, BIMserver call and viewport \
     frame is timed; operations at or over `slow_ms` (default 100) are listed as slow.",
    [Settings], Option<StartParams>, Status, |app, p| {
    let slow = p.and_then(|p| p.slow_ms).unwrap_or(profiler::SLOW_MS);
    if !(0.0..=600_000.0).contains(&slow) {
        return Err(Error::new("profiler.bad_value", "the slow threshold is 0 to 600,000 ms"));
    }
    profiler::start(slow);
    Ok(status())
});

host!(ProfilerStop, "profiler.stop", "Stop the performance profiler",
    "Stops recording (DM-PERF-001); what was recorded stays for the report and the trace.",
    [Settings], Option<serde_json::Value>, Status, |app, p| {
    profiler::stop();
    Ok(status())
});

host!(ProfilerStatus, "profiler.status", "Tell whether the profiler is recording",
    "Whether the profiler is recording, for how long, the events kept and the slow threshold.",
    [Read], Option<serde_json::Value>, Status, |app, p| { Ok(status()) });

#[derive(Debug, Serialize, JsonSchema)]
pub struct LiveOut {
    #[serde(flatten)]
    pub live: Live,
    pub elements: Option<usize>,
    pub ifc_entities: Option<usize>,
}

host!(ProfilerLive, "profiler.live", "Read the live performance metrics",
    "The live metrics (DM-PERF-001): mean and worst frame time of the last frames, frames per second over \
     the last second, the latest operations and slow operations with their durations, memory in use, and \
     the active model's size.",
    [Read], Option<serde_json::Value>, LiveOut, |app, p| {
    let elements = app.active().and_then(|id| app.document(id)).map(|d| d.doc.store().len());
    Ok(LiveOut { live: profiler::live(), elements, ifc_entities: None })
});

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// How many scopes and call paths (all when absent).
    pub top: Option<usize>,
    /// self_time (default), total, calls, max or name.
    #[serde(default)]
    pub sort: Sort,
    /// Also count the model's IFC entities for the context (slower on a large file).
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportOut {
    #[serde(flatten)]
    pub report: Report,
    pub context: Context,
}

host!(ProfilerReport, "profiler.report", "Report what the profiler recorded",
    "What was recorded (DM-PERF-001): per scope its calls, total and self time, mean, median, 95th percentile \
     and maximum; the call paths by self time; the slow operations; the counters; and the context (build, \
     platform, threads, memory, model size).",
    [Read], Option<ReportParams>, ReportOut, |app, p| {
    let p = p.unwrap_or_default();
    Ok(ReportOut { report: profiler::report(p.top, p.sort), context: context(app, p.full) })
});

host!(ProfilerReset, "profiler.reset", "Forget what the profiler recorded",
    "Clears the recorded statistics, slow operations, counters and timeline (DM-PERF-001); recording goes on \
     if it was on.",
    [Settings], Option<serde_json::Value>, (), |app, p| {
    profiler::reset();
    Ok(())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathParams {
    pub path: PathBuf,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Written {
    pub bytes: u64,
    /// Timeline events written (traces).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<usize>,
}

host!(ProfilerExportReport, "profiler.export_report", "Write the performance report for development",
    "Writes the report for development (DM-PERF-001) as Markdown to `path`: context, the hottest scopes by \
     self time with their share, the heaviest call paths, the slow operations, the counters, and how to \
     open the trace. It holds operation names, counts and times only.",
    [FileWrite], PathParams, Written, |app, p| {
    let text = profiler::markdown(&profiler::report(None, Sort::SelfTime), &context(app, true));
    std::fs::write(&p.path, &text).map_err(|e| Error::new("file.write_failed", format!("{}: {e}", p.path.display())))?;
    Ok(Written { bytes: text.len() as u64, events: None })
});

host!(ProfilerExportTrace, "profiler.export_trace", "Write the profiler's timeline as a trace",
    "Writes the recorded timeline to `path` as a Chrome trace (Trace Event JSON), for https://ui.perfetto.dev, \
     chrome://tracing or Speedscope: every scope as a complete event, per thread (DM-PERF-001).",
    [FileWrite], PathParams, Written, |app, p| {
    let t = profiler::chrome_trace();
    let n = t["traceEvents"].as_array().map_or(0, Vec::len);
    let bytes = serde_json::to_vec(&t).expect("serialises");
    std::fs::write(&p.path, &bytes).map_err(|e| Error::new("file.write_failed", format!("{}: {e}", p.path.display())))?;
    Ok(Written { bytes: bytes.len() as u64, events: Some(n) })
});
