// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embodied carbon files (ANA-CARB-001): a factor library read from the Reviera CSV format
//! (importing it again replaces its factors and keeps the matches), a bad line refused by
//! number, a run calculated out of process as an analysis.carbon compute job (ANA-ENV-001) and
//! collected, and the run exported as CSV and JSON.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// The compute host and the built-in runner, rebuilt so the runner knows analysis.carbon.
fn programs() -> (PathBuf, PathBuf) {
    let target = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_owned();
    let exe = |n: &str| target.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    assert!(std::process::Command::new(cargo).args(["build", "-p", "reviera-compute", "--bins"]).status().unwrap().success());
    (exe("reviera-compute-host"), exe("reviera-analysis-builtin"))
}

#[test]
fn factor_import_and_export() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvc").tempdir_in(base).unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_COMPUTE_STATE", state.path());
        std::env::set_var("REVIERA_COMPUTE_HOST", &host);
        std::env::set_var("REVIERA_COMPUTE_RUNNERS", &runner);
        std::env::set_var("REVIERA_COMPUTE_HOST_IDLE_S", "3");
        std::env::set_var("REVIERA_COMPUTE_RUNNER_IDLE_S", "2");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [10, 0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }));
    let material = ok(&mut app, "embodied_carbon.materials", json!({}))[0]["material"].as_str().unwrap().to_owned();

    let csv = dir.path().join("factors.csv");
    std::fs::write(&csv, "# made-up test values\nid,name,class,unit,a1_a3,c1_c4,d\nx1,Generic structure,structure,m3,300,10,-20\n").unwrap();
    let rep = ok(&mut app, "embodied_carbon.import_factors", json!({ "path": csv }));
    assert_eq!(rep["factors"], 1);
    let lib = rep["library_id"].as_str().unwrap().to_owned();
    ok(&mut app, "embodied_carbon.match", json!({ "material": material, "factor_ref": format!("{lib}/x1") }));
    // Again, with another value: the match stays.
    std::fs::write(&csv, "id,name,class,unit,a1_a3\nx1,Generic structure,structure,m3,250\n").unwrap();
    ok(&mut app, "embodied_carbon.import_factors", json!({ "path": csv }));
    assert_eq!(ok(&mut app, "embodied_carbon.materials", json!({}))[0]["status"], "matched");
    std::fs::write(&csv, "id,name,unit,a1_a3\nx1,X,m3,abc\n").unwrap();
    let e = run(&mut app, "embodied_carbon.import_factors", json!({ "path": csv })).unwrap_err();
    assert!(e.message.starts_with("line 2"), "{}", e.message);

    let job = ok(&mut app, "embodied_carbon.run", json!({}));
    assert_eq!(job["elements"], 1);
    let id = job["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let s = ok(&mut app, "compute.status", json!({ "job_id": id }));
        let st = s["state"].as_str().unwrap().to_owned();
        if st == "succeeded" {
            break;
        }
        assert!(!["failed", "cancelled"].contains(&st.as_str()) && Instant::now() < end, "{s:#}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let r = ok(&mut app, "embodied_carbon.collect", json!({ "job_id": id }));
    assert_eq!(r["id"], id, "the run is named after its job");
    assert!(r["totals"]["a1_a3"].as_f64().unwrap() > 0.0);
    let out = dir.path().join("carbon.csv");
    ok(&mut app, "embodied_carbon.export", json!({ "path": out }));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().any(|l| l.starts_with("total,all,")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("element,")));
    let js = dir.path().join("carbon.json");
    ok(&mut app, "embodied_carbon.export", json!({ "path": js, "format": "json" }));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&js).unwrap()).unwrap();
    assert_eq!(v["id"], r["id"]);
}
