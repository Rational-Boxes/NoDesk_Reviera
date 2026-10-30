// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The built-in performance profiler (spec Ch 2 §2.18, DM-PERF-001): named, nested timing scopes
//! on the operations that grow with model size, with per-name statistics, a slow-operation list,
//! counters, and a timeline exportable as a Chrome trace.
//!
//! - **Off by default.** [`scope`] reads one atomic flag and returns an empty guard while the
//!   profiler is off: nothing is recorded or allocated. [`start`], `profiler.start` or the
//!   environment variable `REVIERA_PROFILE=1` (with `REVIERA_PROFILE_SLOW_MS`) switch it on.
//! - **Scopes** nest per thread; each records its wall time and its **self time** (less the time
//!   of the scopes nested in it), its thread and an optional detail (a command's name).
//! - **Statistics** per name: calls, total, self, max, and the durations for the median and
//!   95th percentile, kept up to [`KEEP`] per name and sampled beyond.
//! - **Slow operations** at or over the threshold (100 ms by default) are kept, newest
//!   [`SLOW_KEEP`]; the timeline keeps the newest [`EVENTS_KEEP`] events.
//! - **Call paths:** time is also kept by the chain of enclosing scopes
//!   (`command:wall.create › commit`), so a report shows where the time goes inside an operation.
//! - **Live metrics:** the most recent top-level operations, the frame times (scopes named
//!   [`FRAME`]) and the memory in use, for a view refreshed while the profiler runs.
//! - **Privacy:** names, details, counts and times only. Callers pass command names as details,
//!   never element names, values, paths or credentials.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Durations kept per scope name for the percentiles.
pub const KEEP: usize = 100_000;
/// Slow operations kept.
pub const SLOW_KEEP: usize = 1_000;
/// Timeline events kept.
pub const EVENTS_KEEP: usize = 200_000;
/// The default slow threshold.
pub const SLOW_MS: f64 = 100.0;
/// The scope name of one viewport frame.
pub const FRAME: &str = "frame";
/// Recent top-level operations and frames kept for the live view.
const RECENT: usize = 30;
const FRAMES: usize = 120;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the profiler is recording.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[derive(Default)]
struct Stat {
    calls: u64,
    total: Duration,
    self_total: Duration,
    max: Duration,
    /// Kept durations (microseconds); every `stride`th one once full.
    kept: Vec<u32>,
    stride: u64,
}

/// One finished scope on the timeline.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Event {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Microseconds since the profiler started, and the duration.
    pub start_us: u64,
    pub dur_us: u64,
    pub thread: u64,
    pub depth: u32,
}

#[derive(Default)]
struct PathStat {
    calls: u64,
    total: Duration,
    self_total: Duration,
}

struct State {
    origin: Instant,
    since: SystemTime,
    slow: Duration,
    stats: BTreeMap<String, Stat>,
    paths: BTreeMap<String, PathStat>,
    recent: VecDeque<Event>,
    frames: VecDeque<(Instant, Duration)>,
    counters: BTreeMap<String, (u64, u64)>,
    slow_list: VecDeque<Event>,
    events: VecDeque<Event>,
    dropped: u64,
}

impl State {
    fn new(slow_ms: f64) -> Self {
        State { origin: Instant::now(), since: SystemTime::now(), slow: Duration::from_secs_f64(slow_ms.max(0.0) / 1000.0), stats: BTreeMap::new(), paths: BTreeMap::new(), recent: VecDeque::new(), frames: VecDeque::new(), counters: BTreeMap::new(), slow_list: VecDeque::new(), events: VecDeque::new(), dropped: 0 }
    }
}

fn state() -> &'static Mutex<Option<State>> {
    static S: OnceLock<Mutex<Option<State>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Starts recording (resetting what was recorded), with the slow threshold in milliseconds.
pub fn start(slow_ms: f64) {
    *state().lock().unwrap_or_else(|e| e.into_inner()) = Some(State::new(slow_ms));
    ENABLED.store(true, Ordering::Relaxed);
}

/// Stops recording; what was recorded stays for the report.
pub fn stop() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Forgets what was recorded (keeping the profiler on or off as it is).
pub fn reset() {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    let slow = s.as_ref().map_or(SLOW_MS, |x| x.slow.as_secs_f64() * 1000.0);
    *s = Some(State::new(slow));
}

/// Starts the profiler if the environment asks for it (`REVIERA_PROFILE=1`).
pub fn start_from_env() {
    if std::env::var("REVIERA_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0") {
        let slow = std::env::var("REVIERA_PROFILE_SLOW_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(SLOW_MS);
        start(slow);
    }
}

thread_local! {
    /// The open scopes' child time and path, innermost last.
    static STACK: RefCell<Vec<(Duration, String)>> = const { RefCell::new(Vec::new()) };
}

fn thread_id() -> u64 {
    thread_local! {
        static ID: u64 = {
            use std::sync::atomic::AtomicU64;
            static NEXT: AtomicU64 = AtomicU64::new(1);
            NEXT.fetch_add(1, Ordering::Relaxed)
        };
    }
    ID.with(|i| *i)
}

/// An open scope: recorded when dropped.
#[must_use = "a scope measures until it is dropped"]
#[derive(Debug)]
pub struct Scope {
    open: Option<(Cow<'static, str>, Option<String>, Instant)>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some((name, detail, began)) = self.open.take() else { return };
        let dur = began.elapsed();
        let (children, path, depth) = STACK.with(|s| {
            let mut s = s.borrow_mut();
            let (children, path) = s.pop().unwrap_or_default();
            if let Some(parent) = s.last_mut() {
                parent.0 += dur;
            }
            (children, path, s.len() as u32)
        });
        if !enabled() {
            return;
        }
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(st) = guard.as_mut() else { return };
        let start_us = began.saturating_duration_since(st.origin).as_micros() as u64;
        let stat = st.stats.entry(name.to_string()).or_default();
        stat.calls += 1;
        stat.total += dur;
        stat.self_total += dur.saturating_sub(children);
        stat.max = stat.max.max(dur);
        let us = dur.as_micros().min(u128::from(u32::MAX)) as u32;
        if stat.kept.len() < KEEP {
            stat.kept.push(us);
        } else {
            // Sampled beyond the cap: replace in turn, every stride-th call.
            stat.stride = stat.stride.max(2);
            if stat.calls % stat.stride == 0 {
                let at = (stat.calls / stat.stride) as usize % KEEP;
                stat.kept[at] = us;
            }
        }
        let p = st.paths.entry(path).or_default();
        p.calls += 1;
        p.total += dur;
        p.self_total += dur.saturating_sub(children);
        let is_frame = name == FRAME;
        let event = Event { name: name.into_owned(), detail, start_us, dur_us: dur.as_micros() as u64, thread: thread_id(), depth };
        if is_frame {
            if st.frames.len() == FRAMES {
                st.frames.pop_front();
            }
            st.frames.push_back((began, dur));
        } else if depth == 0 {
            if st.recent.len() == RECENT {
                st.recent.pop_front();
            }
            st.recent.push_back(event.clone());
        }
        if dur >= st.slow {
            if st.slow_list.len() == SLOW_KEEP {
                st.slow_list.pop_front();
            }
            st.slow_list.push_back(event.clone());
        }
        if st.events.len() == EVENTS_KEEP {
            st.events.pop_front();
            st.dropped += 1;
        }
        st.events.push_back(event);
    }
}

/// Opens a scope named `name`, recorded when the guard drops. Free while the profiler is off.
#[inline]
pub fn scope(name: impl Into<Cow<'static, str>>) -> Scope {
    if !enabled() {
        return Scope { open: None };
    }
    open(name.into(), None)
}

/// A scope with a detail, made only while the profiler is on.
#[inline]
pub fn scope_with(name: impl Into<Cow<'static, str>>, detail: impl FnOnce() -> String) -> Scope {
    if !enabled() {
        return Scope { open: None };
    }
    open(name.into(), Some(detail()))
}

fn open(name: Cow<'static, str>, detail: Option<String>) -> Scope {
    STACK.with(|s| {
        let mut s = s.borrow_mut();
        let path = match s.last() {
            Some((_, parent)) => format!("{parent} › {name}"),
            None => name.to_string(),
        };
        s.push((Duration::ZERO, path));
    });
    Scope { open: Some((name, detail, Instant::now())) }
}

/// Adds `n` to counter `name` (for example elements regenerated), and counts the call.
#[inline]
pub fn count(name: &'static str, n: u64) {
    if !enabled() {
        return;
    }
    if let Some(st) = state().lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let c = st.counters.entry(name.to_owned()).or_default();
        c.0 += n;
        c.1 += 1;
    }
}

/// One scope name's statistics, in milliseconds.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Row {
    pub name: String,
    pub calls: u64,
    pub total_ms: f64,
    pub self_ms: f64,
    pub mean_ms: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

/// A counter: its sum and how many times it was added to.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Counter {
    pub name: String,
    pub total: u64,
    pub calls: u64,
}

/// How a report is ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    SelfTime,
    Total,
    Calls,
    Max,
    Name,
}

/// One call path's time, in milliseconds.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PathRow {
    /// The scopes from the outermost, joined by ` › `.
    pub path: String,
    pub depth: usize,
    pub calls: u64,
    pub total_ms: f64,
    pub self_ms: f64,
}

/// What the profiler recorded.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Report {
    pub enabled: bool,
    /// Seconds recorded, and when it started (Unix seconds).
    pub seconds: f64,
    pub since: Option<u64>,
    pub slow_ms: f64,
    pub rows: Vec<Row>,
    /// The slowest kept operations first.
    pub slow: Vec<Event>,
    pub counters: Vec<Counter>,
    /// The call paths, heaviest self time first.
    pub paths: Vec<PathRow>,
    /// Timeline events kept, and dropped to keep within the cap.
    pub events: usize,
    pub events_dropped: u64,
}

fn percentile(sorted: &[u32], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at = ((sorted.len() - 1) as f64 * q).round() as usize;
    f64::from(sorted[at]) / 1000.0
}

/// What was recorded: the `top` rows (all when `None`) ordered by `sort`.
pub fn report(top: Option<usize>, sort: Sort) -> Report {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = guard.as_ref() else { return Report { enabled: enabled(), slow_ms: SLOW_MS, ..Report::default() } };
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    // Ordered and cut first; the percentiles (a sort of the kept durations) only for the rows kept.
    let mut rows: Vec<(&String, &Stat)> = st.stats.iter().collect();
    match sort {
        Sort::SelfTime => rows.sort_by_key(|r| std::cmp::Reverse(r.1.self_total)),
        Sort::Total => rows.sort_by_key(|r| std::cmp::Reverse(r.1.total)),
        Sort::Calls => rows.sort_by_key(|r| std::cmp::Reverse(r.1.calls)),
        Sort::Max => rows.sort_by_key(|r| std::cmp::Reverse(r.1.max)),
        Sort::Name => {}
    }
    if let Some(n) = top {
        rows.truncate(n);
    }
    let rows: Vec<Row> = rows
        .into_iter()
        .map(|(name, s)| {
            let mut k = s.kept.clone();
            k.sort_unstable();
            Row { name: name.clone(), calls: s.calls, total_ms: ms(s.total), self_ms: ms(s.self_total), mean_ms: ms(s.total) / s.calls.max(1) as f64, median_ms: percentile(&k, 0.5), p95_ms: percentile(&k, 0.95), max_ms: ms(s.max) }
        })
        .collect();
    let mut slow: Vec<Event> = st.slow_list.iter().cloned().collect();
    slow.sort_by_key(|e| std::cmp::Reverse(e.dur_us));
    Report {
        enabled: enabled(),
        seconds: st.origin.elapsed().as_secs_f64(),
        since: st.since.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs()),
        slow_ms: ms(st.slow),
        rows,
        slow,
        counters: st.counters.iter().map(|(n, (t, c))| Counter { name: n.clone(), total: *t, calls: *c }).collect(),
        paths: {
            let mut p: Vec<PathRow> = st.paths.iter().map(|(k, v)| PathRow { depth: k.matches(" › ").count(), path: k.clone(), calls: v.calls, total_ms: ms(v.total), self_ms: ms(v.self_total) }).collect();
            p.sort_by(|a, b| b.self_ms.total_cmp(&a.self_ms));
            if let Some(n) = top {
                p.truncate(n.max(20));
            }
            p
        },
        events: st.events.len(),
        events_dropped: st.dropped,
    }
}

/// The live view's metrics.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Live {
    pub enabled: bool,
    pub seconds: f64,
    /// The mean frame time over the last frames (ms), frames per second over the last second,
    /// and the slowest frame among them.
    pub frame_ms: Option<f64>,
    pub fps: Option<f64>,
    pub worst_frame_ms: Option<f64>,
    /// The latest top-level operations, newest first.
    pub recent: Vec<Event>,
    /// The latest slow operations, newest first.
    pub slow: Vec<Event>,
    pub slow_count: usize,
    /// Memory in use (resident set size), where the platform tells it.
    pub memory_mb: Option<f64>,
}

/// The memory in use (resident set size, MB), where the platform tells it.
pub fn memory_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096.0 / 1_048_576.0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The live metrics now.
pub fn live() -> Live {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = guard.as_ref() else { return Live { enabled: enabled(), memory_mb: memory_mb(), ..Live::default() } };
    let n = st.frames.len();
    let now = Instant::now();
    let (frame_ms, worst) = if n > 0 {
        let sum: f64 = st.frames.iter().map(|f| f.1.as_secs_f64()).sum();
        (Some(sum * 1000.0 / n as f64), st.frames.iter().map(|f| f.1.as_secs_f64() * 1000.0).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))))
    } else {
        (None, None)
    };
    let last_second = st.frames.iter().filter(|f| now.saturating_duration_since(f.0) <= Duration::from_secs(1)).count();
    Live {
        enabled: enabled(),
        seconds: st.origin.elapsed().as_secs_f64(),
        frame_ms,
        fps: (n > 0).then_some(last_second as f64),
        worst_frame_ms: worst,
        recent: st.recent.iter().rev().cloned().collect(),
        slow: st.slow_list.iter().rev().take(10).cloned().collect(),
        slow_count: st.slow_list.len(),
        memory_mb: memory_mb(),
    }
}

/// What a report says about where it was made.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Context {
    pub version: String,
    /// `debug` or `release`.
    pub build: String,
    pub platform: String,
    pub threads: usize,
    pub memory_mb: Option<f64>,
    pub elements: Option<usize>,
    pub ifc_entities: Option<usize>,
    pub triangles: Option<usize>,
}

impl Context {
    /// This build and machine (the model's size is the caller's to add).
    pub fn here() -> Context {
        Context {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            build: if cfg!(debug_assertions) { "debug" } else { "release" }.to_owned(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            memory_mb: memory_mb(),
            ..Context::default()
        }
    }
}

/// The report for development, as Markdown (DM-PERF-001).
pub fn markdown(r: &Report, c: &Context) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let _ = writeln!(o, "# BIM Reviera performance report\n");
    let _ = writeln!(o, "Recorded for {:.1} s with a slow threshold of {:.0} ms ({} profiler).\n", r.seconds, r.slow_ms, if r.enabled { "running" } else { "stopped" });
    let _ = writeln!(o, "## Context\n");
    let _ = writeln!(o, "| | |\n|---|---|");
    let _ = writeln!(o, "| Version | {} ({} build) |", c.version, c.build);
    let _ = writeln!(o, "| Platform | {}, {} CPU threads |", c.platform, c.threads);
    if let Some(m) = c.memory_mb {
        let _ = writeln!(o, "| Memory in use | {m:.0} MB |");
    }
    for (label, v) in [("Elements", c.elements), ("IFC entities", c.ifc_entities), ("Triangles", c.triangles)] {
        if let Some(v) = v {
            let _ = writeln!(o, "| {label} | {v} |");
        }
    }
    if c.build == "debug" {
        let _ = writeln!(o, "\n> A debug build: times are several times those of a release build.");
    }
    let total_self: f64 = r.rows.iter().map(|x| x.self_ms).sum();
    let _ = writeln!(o, "\n## Hottest operations (by self time)\n");
    let _ = writeln!(o, "| Scope | Calls | Self (ms) | Share | Total (ms) | Mean | Median | p95 | Max |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for x in r.rows.iter().take(25) {
        let share = if total_self > 0.0 { 100.0 * x.self_ms / total_self } else { 0.0 };
        let _ = writeln!(o, "| `{}` | {} | {:.1} | {share:.0} % | {:.1} | {:.2} | {:.2} | {:.2} | {:.1} |", x.name, x.calls, x.self_ms, x.total_ms, x.mean_ms, x.median_ms, x.p95_ms, x.max_ms);
    }
    let _ = writeln!(o, "\n## Heaviest call paths\n");
    let _ = writeln!(o, "Where the time goes inside operations: each path is the chain of enclosing scopes.\n");
    let _ = writeln!(o, "| Path | Calls | Self (ms) | Total (ms) |\n|---|---:|---:|---:|");
    for x in r.paths.iter().take(25) {
        let _ = writeln!(o, "| `{}` | {} | {:.1} | {:.1} |", x.path, x.calls, x.self_ms, x.total_ms);
    }
    let _ = writeln!(o, "\n## Slow operations (≥ {:.0} ms)\n", r.slow_ms);
    if r.slow.is_empty() {
        let _ = writeln!(o, "None.");
    } else {
        let _ = writeln!(o, "| Duration (ms) | Scope | Detail | At (s) | Thread |\n|---:|---|---|---:|---:|");
        for e in r.slow.iter().take(30) {
            let _ = writeln!(o, "| {:.1} | `{}` | {} | {:.2} | {} |", e.dur_us as f64 / 1000.0, e.name, e.detail.as_deref().unwrap_or(""), e.start_us as f64 / 1e6, e.thread);
        }
    }
    if !r.counters.is_empty() {
        let _ = writeln!(o, "\n## Counters\n");
        let _ = writeln!(o, "| Counter | Total | Calls | Mean |\n|---|---:|---:|---:|");
        for k in &r.counters {
            let _ = writeln!(o, "| `{}` | {} | {} | {:.1} |", k.name, k.total, k.calls, k.total as f64 / k.calls.max(1) as f64);
        }
    }
    let _ = writeln!(o, "\n## Timeline\n");
    let _ = writeln!(o, "{} events kept ({} dropped). Export the trace (`profiler.export_trace`) and open it in https://ui.perfetto.dev or chrome://tracing to see them per thread.", r.events, r.events_dropped);
    o
}

/// The timeline as a Chrome trace (Trace Event JSON: complete "X" events, microseconds), for
/// chrome://tracing, Perfetto and Speedscope.
pub fn chrome_trace() -> serde_json::Value {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let pid = std::process::id();
    let events: Vec<serde_json::Value> = guard
        .as_ref()
        .map(|st| {
            st.events
                .iter()
                .map(|e| {
                    let mut v = serde_json::json!({ "name": e.name, "cat": "reviera", "ph": "X", "ts": e.start_us, "dur": e.dur_us, "pid": pid, "tid": e.thread });
                    if let Some(d) = &e.detail {
                        v["args"] = serde_json::json!({ "detail": d });
                    }
                    v
                })
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({ "traceEvents": events, "displayTimeUnit": "ms", "otherData": { "producer": concat!("BIM Reviera ", env!("CARGO_PKG_VERSION")) } })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The profiler is process-wide: one test drives it from start to finish.
    #[test]
    fn scopes_nest_count_and_export() {
        stop();
        {
            let _s = scope("off");
        }
        start(5.0);
        {
            let _outer = scope_with("outer", || "detail".into());
            std::thread::sleep(Duration::from_millis(8));
            for _ in 0..3 {
                let _inner = scope("inner");
                std::thread::sleep(Duration::from_millis(2));
            }
            count("things", 7);
        }
        stop();
        {
            let _s = scope("after");
        }
        let r = report(None, Sort::SelfTime);
        assert!(!r.enabled);
        let row = |n: &str| r.rows.iter().find(|x| x.name == n).cloned();
        assert!(row("off").is_none() && row("after").is_none(), "nothing recorded while off");
        let (outer, inner) = (row("outer").unwrap(), row("inner").unwrap());
        assert_eq!((outer.calls, inner.calls), (1, 3));
        // The outer scope's self time leaves out its three inner ones.
        assert!(outer.total_ms >= 14.0 && outer.self_ms < outer.total_ms - 5.0, "{outer:?}");
        assert!(inner.p95_ms >= 2.0 && inner.max_ms >= inner.median_ms);
        // Only the outer scope reached the 5 ms threshold.
        assert_eq!(r.slow.len(), 1);
        assert_eq!((r.slow[0].name.as_str(), r.slow[0].detail.as_deref()), ("outer", Some("detail")));
        assert_eq!(r.counters[0].total, 7);
        let path = r.paths.iter().find(|p| p.path == "outer › inner").expect("the nested path");
        assert_eq!((path.calls, path.depth), (3, 1));
        let l = live();
        assert_eq!(l.recent.len(), 1, "one top-level operation");
        let md = markdown(&r, &Context::here());
        assert!(md.contains("`outer › inner`") && md.contains("## Slow operations") && md.contains("| `outer` |"), "{md}");
        let t = chrome_trace();
        let ev = t["traceEvents"].as_array().unwrap();
        assert_eq!(ev.len(), 4);
        assert!(ev.iter().all(|e| e["ph"] == "X" && e["dur"].as_u64().is_some()));
        reset();
        assert!(report(None, Sort::Name).rows.is_empty());
    }
}
