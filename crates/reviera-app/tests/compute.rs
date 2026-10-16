// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `compute.*` against a real model: the project-wide clash test run out of process
//! (`analysis.clash`) finds exactly what the in-process overlap engine finds.

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

/// The host and runner programs, built if they are not there yet.
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
        let ok = std::process::Command::new(cargo)
            .args(["build", "-p", "reviera-compute", "--bins"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "building the compute programs failed");
    }
    (host, runner)
}

/// The manor sample before its boolean pass: towers, gallery and house still overlap.
fn overlapping_manor(dir: &Path) -> PathBuf {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples");
    let script = std::fs::read_to_string(samples.join("manor.lua")).unwrap();
    let cut = script.find("-- ---- Booleans where parts meet").unwrap();
    let library = samples.join("../library").canonicalize().unwrap();
    let script = script[..cut].replace(
        "reviera.script_dir .. \"/../library\"",
        &format!("{:?}", library.to_string_lossy()),
    ) + "\nprint(\"done\")\n";
    let path = dir.join("manor_overlapping.lua");
    std::fs::write(&path, script).unwrap();
    path
}

#[test]
fn the_clash_test_out_of_process_matches_overlap_find() {
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
    // SAFETY: this test binary has one test, so nothing else reads the environment meanwhile.
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

    let kinds = run(&mut app, "compute.kinds", json!({}));
    assert!(
        kinds
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["kind"] == "analysis.clash"),
        "{kinds:#}"
    );
    let d = run(
        &mut app,
        "compute.distill",
        json!({ "kind": "analysis.clash" }),
    );
    assert!(d["elements"].as_u64().unwrap() > 100, "{d:#}");

    let local = run(&mut app, "overlap.find", json!({ "limit": 10000 }));
    let submitted = run(
        &mut app,
        "compute.submit",
        json!({ "kind": "analysis.clash" }),
    );
    let id = submitted["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(120);
    let status = loop {
        let s = run(&mut app, "compute.status", json!({ "job_id": id }));
        if ["succeeded", "failed", "cancelled"].contains(&s["state"].as_str().unwrap())
            || Instant::now() > end
        {
            break s;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status["state"], "succeeded", "{status:#}");
    let results = run(&mut app, "compute.results", json!({ "job_id": id }));
    assert_eq!(results["provenance"]["runner"], "reviera-analysis-builtin");
    assert_eq!(results["provenance"]["distiller"], "clash.bodies 1");

    let pairs = |v: &Value, a: &str, b: &str| -> Vec<(u64, u64, u64)> {
        let mut out: Vec<(u64, u64, u64)> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let (x, y) = (o[a].as_u64().unwrap(), o[b].as_u64().unwrap());
                (x.min(y), x.max(y), o["volume"].as_f64().unwrap().to_bits())
            })
            .collect();
        out.sort();
        out
    };
    let expected = pairs(&local["overlaps"], "a", "b");
    let remote = pairs(&results["clashes"], "a", "b");
    assert!(expected.len() >= 10, "{} overlaps", expected.len());
    assert_eq!(remote, expected);
    assert_eq!(
        results["bodies"].as_u64().unwrap(),
        d["elements"].as_u64().unwrap()
    );

    // The job is in the client's record, with its results collected.
    let jobs = run(&mut app, "compute.jobs", json!({}));
    let job = jobs
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["job_id"] == id.as_str())
        .unwrap();
    assert_eq!(job["results_fetched"], true);
}
