// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Heating and cooling loads end to end (ANA-HCL-001, ANA-ENV-004): four office rooms analysed
//! out of process as analysis.loads jobs; changing the infiltration class from medium to tight
//! lowers the heating load and the report states the class used; the peaks are written to the
//! spaces' Pset_SpaceThermalDesign; changing a U-value makes the run stale.

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

fn run_loads(app: &mut App, args: Value) -> Value {
    let job = ok(app, "hc_loads.run", args);
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
    ok(app, "hc_loads.collect", json!({ "job_id": id }))
}

#[test]
fn heating_and_cooling_loads() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvl").tempdir_in(base).unwrap();
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
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("loads.ifc") }));
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

    let medium = run_loads(&mut app, json!({ "infiltration": "medium" }));
    assert_eq!(medium["infiltration"], "medium", "the report states the class");
    assert_eq!(medium["spaces"].as_array().unwrap().len(), 4);
    let s0 = &medium["spaces"][0]["load"];
    assert!(s0["heating"].as_f64().unwrap() > 0.0 && s0["cooling_total"].as_f64().unwrap() > s0["cooling_sensible"].as_f64().unwrap());
    let tight = run_loads(&mut app, json!({ "infiltration": "tight" }));
    assert_eq!(tight["infiltration"], "tight");
    assert!(tight["heating_total"].as_f64().unwrap() < medium["heating_total"].as_f64().unwrap());

    // The peaks are on the spaces.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let sp = store.get(reviera_core::ElementId(spaces[0].as_u64().unwrap())).unwrap();
    assert!(sp.param("Pset_SpaceThermalDesign.TotalHeatLoss").is_some() && sp.param("Pset_SpaceThermalDesign.CoolingDesignAirflow").is_some());
    assert_eq!(ok(&mut app, "hc_loads.status", json!({}))["stale"], false);
    // A better wall: the run is stale.
    let wall_type = store.iter().find(|e| reviera_core::compound::of_wall_type(e).is_some()).unwrap().id();
    ok(&mut app, "energy_model.set_thermal", json!({ "type_id": wall_type.0, "u_value": 0.12 }));
    assert_eq!(ok(&mut app, "hc_loads.status", json!({}))["stale"], true);
}
