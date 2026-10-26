// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Daylight factor end to end (ANA-LIGHT-002, ANA-ENV-006): four rooms under a flat roof, one
//! window in the south wall; the study runs out of process as an analysis.daylight job; only the
//! room with the window gets daylight, more near the window than at the back; a second window
//! makes the study stale and raises that room's daylight.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn programs() -> (PathBuf, PathBuf) {
    let target = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_owned();
    let exe = |n: &str| target.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    assert!(std::process::Command::new(cargo).args(["build", "-p", "reviera-compute", "--bins"]).status().unwrap().success());
    (exe("reviera-compute-host"), exe("reviera-analysis-builtin"))
}

fn study(app: &mut App, args: Value) -> Value {
    let job = ok(app, "daylight.run", args);
    assert!(job["sensors"].as_u64().unwrap() > 20, "{job}");
    let id = job["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(300);
    loop {
        let s = ok(app, "compute.status", json!({ "job_id": id }));
        let st = s["state"].as_str().unwrap().to_owned();
        if st == "succeeded" {
            break;
        }
        assert!(!["failed", "cancelled"].contains(&st.as_str()) && Instant::now() < end, "{s:#}");
        std::thread::sleep(Duration::from_millis(100));
    }
    ok(app, "daylight.collect", json!({ "job_id": id }))
}

#[test]
fn daylight_factor() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvd").tempdir_in(base).unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_COMPUTE_STATE", state.path());
        std::env::set_var("REVIERA_COMPUTE_HOST", &host);
        std::env::set_var("REVIERA_COMPUTE_RUNNERS", &runner);
        std::env::set_var("REVIERA_COMPUTE_HOST_IDLE_S", "5");
        std::env::set_var("REVIERA_COMPUTE_RUNNER_IDLE_S", "5");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("daylight.ifc") }));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let mut walls = Vec::new();
    for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
        walls.push(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })));
    }
    let sq = json!([{ "line": { "start": [0, 0], "end": [8, 0] } }, { "line": { "start": [8, 0], "end": [8, 8] } }, { "line": { "start": [8, 8], "end": [0, 8] } }, { "line": { "start": [0, 8], "end": [0, 0] } }]);
    ok(&mut app, "roof.create_footprint", json!({ "curves": sq, "level_id": level.0, "base_offset": 3.0 }));
    let spaces = ok(&mut app, "space.place_auto", json!({ "level_id": level.0 }));
    assert_eq!(spaces.as_array().unwrap().len(), 4);
    ok(&mut app, "window.place", json!({ "host_wall_id": walls[0], "point_along": 2.0 }));

    let r = study(&mut app, json!({}));
    assert_eq!(r["spaces"].as_array().unwrap().len(), 4);
    let lit: Vec<&Value> = r["spaces"].as_array().unwrap().iter().filter(|s| s["average"].as_f64().unwrap() > 0.0).collect();
    assert_eq!(lit.len(), 1, "only the room with the window: {r:#}");
    let room = lit[0];
    assert!(room["maximum"].as_f64().unwrap() > 2.0 * room["minimum"].as_f64().unwrap(), "brighter near the window: {room}");
    assert!(room["reflected"].as_f64().unwrap() > 0.0 && room["uniformity"].as_f64().unwrap() < 1.0);
    let before = room["average"].as_f64().unwrap();
    assert_eq!(ok(&mut app, "daylight.status", json!({}))["stale"], false);

    ok(&mut app, "window.place", json!({ "host_wall_id": walls[3], "point_along": 6.0 }));
    assert_eq!(ok(&mut app, "daylight.status", json!({}))["stale"], true);
    let r2 = study(&mut app, json!({ "options": { "spacing": 0.5, "threshold": 1.0 } }));
    let room2 = r2["spaces"].as_array().unwrap().iter().find(|s| s["space"] == room["space"]).unwrap();
    assert!(room2["average"].as_f64().unwrap() > before, "{room2} vs {before}");
    assert_eq!(r2["threshold"], 1.0);
    let e = app.execute("daylight.run", json!({ "options": { "spacing": 0.01 } }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "daylight.bad_option");
}
