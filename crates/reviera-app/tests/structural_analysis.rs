// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural analysis end to end (ANA-FEA-001): a simply supported 6 m W12X26 under 10 kN/m is
//! analysed out of process by the built-in frame solver: the mid-span moment is wL²/8 = 45 kN·m
//! and each support takes 30 kN; the results are kept with the model and go stale when a load
//! changes; the neutral model exports as JSON.

use std::path::PathBuf;
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
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // Always rebuilt: the runner must know `analysis.structural`.
    assert!(
        std::process::Command::new(cargo)
            .args(["build", "-p", "reviera-compute", "--bins"])
            .status()
            .unwrap()
            .success()
    );
    (host, runner)
}

#[test]
fn a_simply_supported_beam_analysed_out_of_process() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let state = tempfile::Builder::new()
        .prefix("rvs")
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
    run(&mut app, "code.set", json!({ "code": "ibc" }));
    let w12 = run(
        &mut app,
        "beam.load_section",
        json!({ "catalog": "aisc_w", "designation": "W12X26" }),
    );
    let m = run(
        &mut app,
        "analytical_member.create",
        json!({ "start": [0.0, 0.0, 0.0], "end": [6.0, 0.0, 0.0], "section_type_id": w12 }),
    );
    // Without supports, the pre-analysis checks (STR-AM-007) hold the run: S3 and C2 are errors.
    let held = app
        .execute("structural_analysis.run", json!({}), &Caller::user())
        .unwrap_err();
    assert_eq!(held.code, "structural.check_errors", "{held:?}");
    let codes: Vec<String> = held.details["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["code"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(codes, ["C2", "S3"]);
    for end in ["start", "end"] {
        run(
            &mut app,
            "boundary_condition.create",
            json!({ "kind": "point", "host_id": m, "location": end, "state": "pinned" }),
        );
    }
    let d = run(
        &mut app,
        "load_case.create",
        json!({ "name": "D", "nature": "D" }),
    );
    let load = run(
        &mut app,
        "load.create_line",
        json!({ "host_id": m, "case_id": d, "start": [0.0, 0.0, -10000.0] }),
    );
    run(
        &mut app,
        "load_combination.create",
        json!({ "name": "U1", "formula": "1.4*D" }),
    );

    // The neutral model exports.
    let path = work.path().join("reviera-structural-model.json");
    let ex = run(
        &mut app,
        "structural_analysis.export_model",
        json!({ "path": path }),
    );
    assert_eq!(
        (ex["members"].as_u64(), ex["nodes"].as_u64()),
        (Some(1), Some(2))
    );
    let model: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(model["units"], "SI");
    assert!(model["members"][0]["e"].as_f64().unwrap() > 1.9e11);

    // Run out of process, collect.
    let job = run(&mut app, "structural_analysis.run", json!({}));
    let id = job["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let s = run(&mut app, "compute.status", json!({ "job_id": id }));
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
    let report = run(
        &mut app,
        "structural_analysis.collect",
        json!({ "job_id": id }),
    );
    assert_eq!(report["stale"], false);
    let case = &report["cases"][0];
    // wL²/8 = 45 kN·m within 0.5 %; reactions 30 kN each (60 kN in all).
    let moment = case["max_moment"].as_f64().unwrap();
    assert!((moment - 45_000.0).abs() <= 225.0, "{moment}");
    assert!((case["total_vertical_reaction"].as_f64().unwrap() - 60_000.0).abs() < 1.0);
    // The combination is 1.4 × D.
    let u1 = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "U1")
        .unwrap();
    assert!((u1["max_moment"].as_f64().unwrap() - 1.4 * moment).abs() < 1e-6);
    // Mid-span deflection 5wL⁴/384EI.
    let members = report["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["member"], m);

    // Kept with the model; editing the load makes the results stale.
    let again = run(
        &mut app,
        "structural_analysis.results",
        json!({ "case": "U1" }),
    );
    assert_eq!(again["stale"], false);
    run(&mut app, "element.delete", json!({ "element_ids": [load] }));
    let stale = run(&mut app, "structural_analysis.results", json!({}));
    assert_eq!(stale["stale"], true);
}
