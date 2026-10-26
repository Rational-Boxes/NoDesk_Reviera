// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Solar studies end to end (ANA-SOLAR-001, ANA-ENV-002): a hip roof at 52° N analysed out of
//! process as an analysis.solar job from a synthetic weather year: the south face gets more than
//! the north face; a tall block to the south cuts the south face by at least half; moving the
//! block makes the study stale; a PV study reports energy, cost and payback by the spec's formula
//! and exports as CSV.

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

/// A synthetic year: clear days, 600 W/m² direct normal and 100 W/m² diffuse from 7 to 18 h.
fn epw() -> String {
    let mut s = String::from("LOCATION,Testville,TS,Testland,Synthetic,123456,52.0,0.0,0.0,40.0\n");
    for h in ["DESIGN CONDITIONS,0", "TYPICAL/EXTREME PERIODS,0", "GROUND TEMPERATURES,0", "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0", "COMMENTS 1,synthetic", "COMMENTS 2,", "DATA PERIODS,1,1,Data,Sunday, 1/ 1,12/31"] {
        s.push_str(h);
        s.push('\n');
    }
    for (m, d) in [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31].iter().enumerate() {
        for day in 1..=*d {
            for hour in 1..=24 {
                let up = (7..=18).contains(&hour);
                let (dni, dhi, ext) = if up { (600, 100, 900) } else { (0, 0, 0) };
                s.push_str(&format!("2001,{},{day},{hour},60,A7A7,10.0,5.0,50,101325,{ext},1300,300,{},{dni},{dhi},0,0,0,0,180,3.0,5,5,15.0,77777,9,999999999,0,0.1,0,88,0.0,0.0,0.0\n", m + 1, dhi + 300));
            }
        }
    }
    s
}

fn wait(app: &mut App, id: &str) {
    let end = Instant::now() + Duration::from_secs(300);
    loop {
        let s = ok(app, "compute.status", json!({ "job_id": id }));
        let st = s["state"].as_str().unwrap().to_owned();
        if st == "succeeded" {
            return;
        }
        assert!(!["failed", "cancelled"].contains(&st.as_str()) && Instant::now() < end, "{s:#}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn study(app: &mut App, args: Value) -> Value {
    let job = ok(app, "solar.run", args);
    let id = job["job_id"].as_str().unwrap().to_owned();
    wait(app, &id);
    ok(app, "solar.collect", json!({ "job_id": id }))
}

#[test]
fn solar_studies() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvo").tempdir_in(base).unwrap();
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
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("sun.ifc") }));
    let epw_path = dir.path().join("year.epw");
    std::fs::write(&epw_path, epw()).unwrap();
    ok(&mut app, "weather.import", json!({ "path": epw_path }));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // An 8 m square hip roof at 30°.
    let sq = |a: f64, b: f64| json!([{ "line": { "start": [a, a], "end": [b, a] } }, { "line": { "start": [b, a], "end": [b, b] } }, { "line": { "start": [b, b], "end": [a, b] } }, { "line": { "start": [a, b], "end": [a, a] } }]);
    ok(&mut app, "roof.create_footprint", json!({ "curves": sq(0.0, 8.0), "level_id": level.0, "base_offset": 3.0, "slope": "30°" }));
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let faces = reviera_core::solar::analysed_faces(&store, &reviera_core::solar::Surfaces::Roofs).unwrap();
    assert_eq!(faces.len(), 4);
    // The face whose outline's centre lies furthest south, and furthest north.
    let centre_y = |f: &reviera_core::solar::Face| f.outline.iter().map(|p| p[1]).sum::<f64>() / f.outline.len() as f64;
    let south = faces.iter().min_by(|a, b| centre_y(&a.1).total_cmp(&centre_y(&b.1))).unwrap().1.key.clone();
    let north = faces.iter().max_by(|a, b| centre_y(&a.1).total_cmp(&centre_y(&b.1))).unwrap().1.key.clone();
    let mean = |r: &Value, key: &str| r["faces"].as_array().unwrap().iter().find(|f| f["key"] == key).unwrap()["mean"].as_f64().unwrap();

    let open = study(&mut app, json!({ "grid": 2.0 }));
    let (s0, n0) = (mean(&open, &south), mean(&open, &north));
    assert!(s0 > n0 * 1.2, "south {s0} against north {n0}");

    // A 25 m block just south of the roof.
    let m = ok(&mut app, "mass.create", json!({ "name": "Block", "forms": [{ "kind": "extrusion", "start": 0.0, "end": 25.0, "level": level.0, "profile": [[-6.0, -6.0], [14.0, -6.0], [14.0, -3.0], [-6.0, -3.0]] }] }));
    let shaded = study(&mut app, json!({ "grid": 2.0 }));
    let s1 = mean(&shaded, &south);
    assert!(s1 < s0 * 0.5, "shaded {s1} against open {s0}");
    assert_eq!(ok(&mut app, "solar.status", json!({}))["stale"], false);
    // Moving the block makes the study stale.
    ok(&mut app, "element.move", json!({ "element_ids": [m], "vector": [0.0, -20.0, 0.0] }));
    assert_eq!(ok(&mut app, "solar.status", json!({}))["stale"], true);

    // PV: energy = mean × area × coverage × efficiency × derate; payback = cost ÷ (energy × price).
    let pv = study(&mut app, json!({ "grid": 2.0, "study_type": "pv", "pv_settings": { "coverage": 0.8, "efficiency": 0.18, "derate": 0.86, "cost_per_kwp": 1000.0, "energy_price": 0.2 } }));
    let f = pv["faces"].as_array().unwrap().iter().find(|f| f["key"] == south.as_str()).unwrap().clone();
    let (area, m_kwh) = (f["area"].as_f64().unwrap(), f["mean"].as_f64().unwrap());
    let kwh = m_kwh * area * 0.8 * 0.18 * 0.86;
    assert!((f["pv_kwh_year"].as_f64().unwrap() - kwh).abs() < 1e-6 * kwh.max(1.0), "{f}");
    let cost = area * 0.8 * 0.18 * 1000.0;
    assert!((f["pv_payback_years"].as_f64().unwrap() - cost / (kwh * 0.2)).abs() < 1e-6);
    let csv = dir.path().join("solar.csv");
    ok(&mut app, "solar.export", json!({ "path": csv }));
    assert_eq!(std::fs::read_to_string(&csv).unwrap().lines().count(), 5, "a header and four faces");
}
