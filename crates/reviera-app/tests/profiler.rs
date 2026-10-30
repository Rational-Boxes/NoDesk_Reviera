// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The performance profiler (DM-PERF-001): commands timed with what happens inside them, the
//! live metrics, the report for development and the trace.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

// The profiler is process-wide: one test drives it.
#[test]
fn a_session_is_timed_reported_and_traced() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    assert_eq!(run(&mut app, "profiler.status", json!({})).unwrap()["enabled"], false);
    assert!(run(&mut app, "profiler.start", json!({ "slow_ms": -1 })).is_err());
    let s = run(&mut app, "profiler.start", json!({ "slow_ms": 0 })).unwrap();
    assert_eq!((s["enabled"].as_bool(), s["slow_ms"].as_f64()), (Some(true), Some(0.0)));

    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"].clone();
    for k in 0..3 {
        run(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, k * 5], "end": [6, k * 5] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    }
    let id = app.active().unwrap();
    app.render_scene(id).unwrap();
    run(&mut app, "file.save_as", json!({ "path": dir.path().join("p.ifc") })).unwrap();

    let r = run(&mut app, "profiler.report", json!({})).unwrap();
    let row = |n: &str| r["rows"].as_array().unwrap().iter().find(|x| x["name"] == n).cloned();
    assert_eq!(row("command:wall.create").expect("the command")["calls"], 3, "{r}");
    for scope in ["commit", "render scene", "IFC save", "command:file.save_as"] {
        assert!(row(scope).is_some(), "{scope} timed: {r}");
    }
    let paths: Vec<&str> = r["paths"].as_array().unwrap().iter().filter_map(|p| p["path"].as_str()).collect();
    assert!(paths.contains(&"command:wall.create › commit"), "{paths:?}");
    assert!(paths.iter().any(|p| p.starts_with("command:file.save_as › IFC save")), "{paths:?}");
    // With the threshold at 0 every operation is slow.
    assert!(!r["slow"].as_array().unwrap().is_empty());
    assert_eq!(r["context"]["elements"].as_u64().unwrap(), app.document(id).unwrap().doc.store().len() as u64);

    let live = run(&mut app, "profiler.live", json!({})).unwrap();
    assert!(live["recent"].as_array().unwrap().iter().any(|e| e["name"] == "command:file.save_as"), "{live}");
    assert!(live["elements"].as_u64().is_some());

    let md = dir.path().join("report.md");
    run(&mut app, "profiler.export_report", json!({ "path": md })).unwrap();
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.starts_with("# BIM Reviera performance report") && text.contains("`command:wall.create › commit`") && text.contains("## Slow operations"), "{text}");
    let trace = dir.path().join("trace.json");
    let t = run(&mut app, "profiler.export_trace", json!({ "path": trace })).unwrap();
    let parsed: Value = serde_json::from_slice(&std::fs::read(&trace).unwrap()).unwrap();
    assert_eq!(parsed["traceEvents"].as_array().unwrap().len() as u64, t["events"].as_u64().unwrap());
    assert!(parsed["traceEvents"].as_array().unwrap().iter().all(|e| e["ph"] == "X"));

    run(&mut app, "profiler.stop", json!({})).unwrap();
    run(&mut app, "level.list", json!({})).unwrap();
    let after = run(&mut app, "profiler.report", json!({})).unwrap();
    assert!(after["rows"].as_array().unwrap().iter().all(|x| x["name"] != "command:level.list" || x["calls"] == 1), "nothing recorded once stopped");
    run(&mut app, "profiler.reset", json!({})).unwrap();
    assert!(run(&mut app, "profiler.report", json!({})).unwrap()["rows"].as_array().unwrap().is_empty());
}
