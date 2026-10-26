// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annual energy end to end (ANA-ENV-005): four office rooms simulated through a synthetic year
//! out of process as analysis.energy jobs; monthly heating peaks in winter and cooling in summer;
//! a heat pump moves heating from gas to electricity; the grid factor gives the operational
//! carbon; a better wall lowers the heating and makes the run stale.

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

/// A synthetic year: −8 °C winter nights to 31 °C summer afternoons, clear days.
fn epw() -> String {
    let mut s = String::from("LOCATION,Testville,TS,Testland,Synthetic,123456,52.0,0.0,0.0,40.0\n");
    for h in ["DESIGN CONDITIONS,0", "TYPICAL/EXTREME PERIODS,0", "GROUND TEMPERATURES,0", "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0", "COMMENTS 1,synthetic", "COMMENTS 2,", "DATA PERIODS,1,1,Data,Sunday, 1/ 1,12/31"] {
        s.push_str(h);
        s.push('\n');
    }
    let mut n = 0;
    for (m, d) in [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31].iter().enumerate() {
        for day in 1..=*d {
            for hour in 1..=24 {
                let doy = n as f64 / 24.0;
                let season = -(std::f64::consts::TAU * (doy + 10.0) / 365.0).cos();
                let daily = -(std::f64::consts::TAU * (hour as f64 - 3.0) / 24.0).cos();
                let t = 11.5 + 14.5 * season + 5.0 * daily;
                let up = (7..=18).contains(&hour);
                let (dni, dhi, ext) = if up { (650, 110, 900) } else { (0, 0, 0) };
                s.push_str(&format!("2001,{},{day},{hour},60,A7A7,{t:.1},{:.1},50,101325,{ext},1300,300,{},{dni},{dhi},0,0,0,0,180,3.0,5,5,15.0,77777,9,999999999,0,0.1,0,88,0.0,0.0,0.0\n", m + 1, t - 6.0, dhi + 300));
                n += 1;
            }
        }
    }
    s
}

fn run_energy(app: &mut App, args: Value) -> Value {
    let job = ok(app, "energy.run", args);
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
    ok(app, "energy.collect", json!({ "job_id": id }))
}

#[test]
fn annual_energy() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rve").tempdir_in(base).unwrap();
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
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("energy.ifc") }));
    let epw_path = dir.path().join("year.epw");
    std::fs::write(&epw_path, epw()).unwrap();
    let w = ok(&mut app, "weather.import", json!({ "path": epw_path }));
    assert!(w["weather"]["derived"]["heating_dry_bulb"].as_f64().unwrap() < -1.0);
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
        ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }));
    }
    let spaces = ok(&mut app, "space.place_auto", json!({ "level_id": level.0 }));
    assert_eq!(spaces.as_array().unwrap().len(), 4);
    ok(&mut app, "space_type.create", json!({ "name": "Office", "values": { "lighting_load_density": 8.0, "power_load_density": 10.0, "area_per_person": 10.0, "sensible_heat_per_person": 75.0, "latent_heat_per_person": 55.0, "outside_air_per_person": 10.0, "heating_set_point": 20.0, "cooling_set_point": 24.0 } }));
    ok(&mut app, "space.set_type", json!({ "element_ids": spaces, "type_id": "office" }));

    let f = |v: &Value| v.as_f64().unwrap();
    let gas = run_energy(&mut app, json!({}));
    assert_eq!(gas["months"].as_array().unwrap().len(), 12);
    assert_eq!(gas["spaces"].as_array().unwrap().len(), 4);
    let (jan, jul) = (&gas["months"][0], &gas["months"][6]);
    assert!(f(&jan["heating_load"]) > f(&jul["heating_load"]) && f(&jul["cooling_load"]) > f(&jan["cooling_load"]), "{jan} {jul}");
    assert!(f(&gas["year"]["gas"]) > 0.0 && gas["year"]["carbon"].is_null(), "no grid factor, no carbon");
    assert!(f(&gas["eui"]) > 0.0 && (50.0..64.0).contains(&f(&gas["floor_area"])), "{}", gas["floor_area"]);
    let hp = run_energy(&mut app, json!({ "options": { "heating": "heat_pump", "grid_factor": 0.25 } }));
    assert_eq!(f(&hp["year"]["gas"]), 0.0);
    assert!((f(&hp["year"]["heating_load"]) - f(&gas["year"]["heating_load"])).abs() < 1e-6);
    assert!(f(&hp["year"]["electricity"]) > f(&gas["year"]["electricity"]));
    assert!((f(&hp["year"]["carbon"]) - 0.25 * f(&hp["year"]["electricity"])).abs() < 1e-3);
    assert_eq!(ok(&mut app, "energy.status", json!({}))["stale"], false);
    assert_eq!(ok(&mut app, "energy.results", json!({ "run_id": gas["id"] }))["id"], gas["id"]);

    // A better wall: less heating, and the last run is stale.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let wall_type = store.iter().find(|e| reviera_core::compound::of_wall_type(e).is_some()).unwrap().id();
    ok(&mut app, "energy_model.set_thermal", json!({ "type_id": wall_type.0, "u_value": 0.12 }));
    assert_eq!(ok(&mut app, "energy.status", json!({}))["stale"], true);
    let better = run_energy(&mut app, json!({ "options": { "heating": "heat_pump", "grid_factor": 0.25 } }));
    assert!(f(&better["year"]["heating_load"]) < f(&hp["year"]["heating_load"]));
    let e = app.execute("energy.run", json!({ "options": { "cooling_cop": 0.2 } }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "energy.bad_option");
}
