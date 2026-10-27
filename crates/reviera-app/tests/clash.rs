// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Clash detection end to end (COL-CLASH-001, COL-CLASH-002): a clash test runs out of process,
//! is collected into the model's results, keeps statuses between runs, resolves what a later run
//! no longer finds, exports, undoes, and survives saving and reopening.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

fn programs() -> (PathBuf, PathBuf) {
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let exe = |n: &str| target.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let (host, runner) = (exe("reviera-compute-host"), exe("reviera-analysis-builtin"));
    if !host.exists() || !runner.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        assert!(
            std::process::Command::new(cargo)
                .args(["build", "-p", "reviera-compute", "--bins"])
                .status()
                .unwrap()
                .success()
        );
    }
    (host, runner)
}

fn overlapping_manor(dir: &Path) -> PathBuf {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples");
    let script = std::fs::read_to_string(samples.join("manor.lua")).unwrap();
    let cut = script.find("-- ---- Booleans where parts meet").unwrap();
    let library = samples.join("../library").canonicalize().unwrap();
    let script = script[..cut].replace(
        "reviera.script_dir .. \"/../library\"",
        &format!("{:?}", library.to_string_lossy()),
    );
    let path = dir.join("manor_overlapping.lua");
    std::fs::write(&path, script).unwrap();
    path
}

/// Runs a clash test (or the last one again) and collects it.
fn test_and_collect(app: &mut App, command: &str, args: Value) -> Value {
    let job = run(app, command, args);
    let id = job["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let s = run(app, "compute.status", json!({ "job_id": id }));
        let state = s["state"].as_str().unwrap().to_owned();
        if state == "succeeded" {
            break;
        }
        assert!(
            !["failed", "cancelled"].contains(&state.as_str()) && Instant::now() < end,
            "{s:#}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    run(app, "clash.collect", json!({ "job_id": id }))
}

fn statuses(report: &Value) -> Vec<(String, String)> {
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["id"].as_str().unwrap().to_owned(),
                i["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn clash_tests_keep_statuses_between_runs_and_navigate_by_element() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let state = tempfile::Builder::new()
        .prefix("rvc")
        .tempdir_in(base)
        .unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_COMPUTE_STATE", state.path());
        std::env::set_var("REVIERA_COMPUTE_HOST", &host);
        std::env::set_var("REVIERA_COMPUTE_RUNNERS", &runner);
        std::env::set_var("REVIERA_COMPUTE_HOST_IDLE_S", "3");
        std::env::set_var("REVIERA_COMPUTE_RUNNER_IDLE_S", "2");
    }
    let work = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: work.path().to_owned(),
    });
    run(&mut app, "file.new", json!({}));
    run(
        &mut app,
        "script.run",
        json!({ "file": overlapping_manor(work.path()) }),
    );

    // First run: everything against everything; every result is new.
    let first = test_and_collect(&mut app, "clash.run_basic", json!({}));
    let n = first["new"].as_u64().unwrap();
    assert!(n >= 10, "{first:#}");
    assert_eq!(first["run"], 1);
    let report = run(&mut app, "clash.last_report", json!({}));
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len() as u64, n);
    assert!(items.iter().all(|i| i["status"] == "new"));
    // Every result names its elements and their category pair.
    for i in items {
        assert!(i["a_id"].is_u64() && i["b_id"].is_u64(), "{i:#}");
        assert!(i["group"].as_str().unwrap().contains(" × "));
    }
    // It matches the in-process overlap check.
    let local = run(&mut app, "overlap.find", json!({ "limit": 10000 }));
    assert_eq!(local["overlaps"].as_array().unwrap().len() as u64, n);

    // An issue per clash (COL-BCF-005): boxed round it, both elements selected, with a snapshot;
    // made again, nothing is added.
    let made = run(&mut app, "bcf.from_clash", json!({}));
    assert_eq!(made["topic_ids"].as_array().unwrap().len() as u64, n, "{made:#}");
    assert_eq!(made["snapshots"].as_u64(), Some(n));
    let t = run(&mut app, "bcf.get", json!({ "topic_id": made["topic_ids"][0] }));
    assert!(t["title"].as_str().unwrap().starts_with("Clash: ") && t["title"].as_str().unwrap().contains(" × "), "{t:#}");
    assert_eq!((t["topic_type"].as_str(), t["labels"][0].as_str()), (Some("Clash"), Some("Clash")));
    let vp = &t["viewpoints"][0];
    assert_eq!(vp["viewpoint"]["clipping_planes"].as_array().unwrap().len(), 6, "a section box");
    assert_eq!(vp["viewpoint"]["selection"].as_array().unwrap().len(), 2);
    assert!(vp["snapshot"].is_string());
    let again = run(&mut app, "bcf.from_clash", json!({}));
    assert_eq!((again["topic_ids"].as_array().unwrap().len(), again["already"].as_u64()), (0, Some(n)), "{again:#}");
    run(&mut app, "edit.undo", json!({}));
    assert_eq!(run(&mut app, "bcf.list", json!({})).as_array().unwrap().len(), 0, "one step to undo");

    // Review one, with a comment.
    let reviewed = items[0]["id"].as_str().unwrap().to_owned();
    run(
        &mut app,
        "clash.set_status",
        json!({ "result_ids": [reviewed], "status": "reviewed", "comment": "Tower wall into the eave: by design" }),
    );

    // Run again: the same clashes are active, the reviewed one stays reviewed.
    let second = test_and_collect(&mut app, "clash.refresh", json!({}));
    assert_eq!(second["run"], 2);
    assert_eq!(second["new"], 0);
    assert_eq!(second["found_again"].as_u64().unwrap(), n);
    let report = run(&mut app, "clash.last_report", json!({}));
    for (id, status) in statuses(&report) {
        let expected = if id == reviewed { "reviewed" } else { "active" };
        assert_eq!(status, expected, "{id}");
    }
    let item = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == reviewed.as_str())
        .unwrap();
    assert_eq!(
        item["comments"][0]["text"],
        "Tower wall into the eave: by design"
    );

    // A narrower test (roofs against everything) no longer finds the others: they resolve.
    let third = test_and_collect(
        &mut app,
        "clash.run_basic",
        json!({ "set_a": { "classes": ["IfcRoof"] } }),
    );
    let report = run(&mut app, "clash.last_report", json!({}));
    let resolved = report["counts"]["resolved"].as_u64().unwrap_or(0);
    assert_eq!(resolved, third["resolved"].as_u64().unwrap());
    assert!(resolved > 0 && resolved < n, "{third:#}");
    // Every remaining open clash involves a roof.
    for i in report["items"].as_array().unwrap() {
        if i["status"] != "resolved" {
            assert!(
                i["a_class"] == "IfcRoof" || i["b_class"] == "IfcRoof",
                "{i:#}"
            );
        }
    }

    // Undo takes the third run back.
    run(&mut app, "edit.undo", json!({}));
    let report = run(&mut app, "clash.last_report", json!({}));
    assert_eq!(report["counts"].get("resolved"), None);
    assert_eq!(report["test"]["runs"], 2);

    // Exports.
    let csv = work.path().join("clashes.csv");
    let html = work.path().join("clashes.html");
    assert_eq!(
        run(&mut app, "clash.export_report", json!({ "path": csv })).as_u64(),
        Some(n)
    );
    run(&mut app, "clash.export_report", json!({ "path": html }));
    let text = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(text.lines().count() as u64, n + 1);
    assert!(text.contains("Tower wall into the eave"));
    assert!(std::fs::read_to_string(&html).unwrap().contains("<table>"));

    // Saved with the model.
    let saved = work.path().join("manor-clashes.ifc");
    run(&mut app, "file.save_as", json!({ "path": saved }));
    let mut again = App::new(AppConfig {
        live: None,
        untitled_dir: work.path().to_owned(),
    });
    run(&mut again, "file.open", json!({ "path": saved }));
    let reopened = run(&mut again, "clash.last_report", json!({}));
    assert_eq!(statuses(&reopened), statuses(&report));
}
