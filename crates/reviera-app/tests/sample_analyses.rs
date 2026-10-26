// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The MEP and sustainability samples (`test-data/samples`) show what they say, from their saved
//! files: the office air system supplies 300 L/s to each of its two spaces in two system-zones;
//! the electrical sample's LP1 has its three circuits and the space a lumen-method illuminance;
//! the washroom has its three sprinklers on a wet system and its fixtures in a sanitary system;
//! the energy house runs annual energy on its own weather file (kept beside it) to a plausible
//! result and exports gbXML; the daylight studio's west room, open to the sky, is better daylit
//! than the east room behind the neighbouring block; the analysis tools' canopy is analysed
//! (its U1 combination 1.2 D + 1.6 Lr), and its route, duct pressure drop and circuits kept.

use std::path::{Path, PathBuf};
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

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples")
}

/// A fresh copy of a sample (with its resources) opened in `app`.
fn open(app: &mut App, work: &Path, name: &str) {
    let copy = work.join(format!("{name}.ifc"));
    std::fs::copy(samples().join(format!("{name}.ifc")), &copy).unwrap();
    let res = samples().join(format!("{name}.resources"));
    if res.is_dir() {
        let to = work.join(format!("{name}.resources/weather"));
        std::fs::create_dir_all(&to).unwrap();
        for f in std::fs::read_dir(res.join("weather")).unwrap() {
            let f = f.unwrap().path();
            std::fs::copy(&f, to.join(f.file_name().unwrap())).unwrap();
        }
    }
    ok(app, "file.open", json!({ "path": copy }));
}

fn finish(app: &mut App, run: &str, collect: &str, args: Value) -> Value {
    let id = ok(app, run, args)["job_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(300);
    loop {
        let s = ok(app, "compute.status", json!({ "job_id": id }));
        if s["state"] == "succeeded" {
            break;
        }
        assert!(!["failed", "cancelled"].contains(&s["state"].as_str().unwrap()) && Instant::now() < end, "{s:#}");
        std::thread::sleep(Duration::from_millis(100));
    }
    ok(app, collect, json!({ "job_id": id }))
}

#[test]
fn the_mep_and_sustainability_samples_show_their_features() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvs").tempdir_in(base).unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_COMPUTE_STATE", state.path());
        std::env::set_var("REVIERA_COMPUTE_HOST", &host);
        std::env::set_var("REVIERA_COMPUTE_RUNNERS", &runner);
        std::env::set_var("REVIERA_COMPUTE_HOST_IDLE_S", "5");
        std::env::set_var("REVIERA_COMPUTE_RUNNER_IDLE_S", "5");
    }
    let work = tempfile::tempdir().unwrap();
    let f = |v: &Value| v.as_f64().unwrap_or_else(|| panic!("{v}"));

    // Office air system: 300 L/s to each space.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u1") });
    open(&mut app, work.path(), "mep_hvac");
    let air = ok(&mut app, "space.airflow", json!({}));
    let supplies: Vec<f64> = air.as_array().unwrap().iter().map(|s| f(&s["supply"])).collect();
    assert_eq!(supplies.len(), 2, "{air}");
    assert!(supplies.iter().all(|s| (s - 0.3).abs() < 1e-9), "{supplies:?}");

    // Electrical: LP1's three circuits, and the space lit by the lumen method.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u2") });
    open(&mut app, work.path(), "mep_electrical");
    let circuits = ok(&mut app, "circuit.list", json!({}));
    // The branch circuits, not the feeder from MDB to LP1.
    let names: Vec<&str> = circuits.as_array().unwrap().iter().filter(|c| c["feeder"] != true).map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["LP1-1", "LP1-2", "LP1-3"], "{circuits:#}");
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let space = store.iter().find(|e| e.class == "IfcSpace").unwrap().id();
    let lux = ok(&mut app, "lighting.calc_average", json!({ "space_ids": [space.0] }));
    assert_eq!(lux[0]["fixtures"], 9, "{lux}");
    assert!((250.0..600.0).contains(&f(&lux[0]["illuminance"])), "{lux}");

    // Washroom: three pendent sprinklers, the fixtures in a sanitary system.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u3") });
    open(&mut app, work.path(), "mep_plumbing");
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    assert_eq!(store.iter().filter(|e| e.class == "IfcFireSuppressionTerminal").count(), 3);
    assert!(store.iter().filter(|e| e.class == "IfcSanitaryTerminal").count() >= 5);
    assert!(store.iter().any(|e| e.class == "IfcDistributionSystem" && e.name.as_deref() == Some("Sanitary")), "a sanitary system");

    // Energy house: its weather beside it, annual energy plausible, gbXML out.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u4") });
    open(&mut app, work.path(), "energy_house");
    let model = ok(&mut app, "energy_model.get", json!({}));
    let spaces = model["spaces"].as_array().unwrap();
    assert_eq!(spaces.len(), 4);
    assert!(spaces.iter().all(|s| (f(&s["height"]) - 2.7).abs() < 1e-9), "the spaces stop at the walls' tops");
    let e = finish(&mut app, "energy.run", "energy.collect", json!({ "options": { "heating": "heat_pump", "grid_factor": 0.2 } }));
    let eui = f(&e["eui"]);
    assert!((5.0..150.0).contains(&eui), "EUI {eui} kWh/m²: {}", e["year"]);
    assert!(f(&e["months"][0]["heating_load"]) > f(&e["months"][6]["heating_load"]), "more heating in January than July");
    let path = work.path().join("energy_house.xml");
    let r = ok(&mut app, "gbxml.export", json!({ "path": path }));
    assert_eq!(r["spaces"], 4);
    assert_eq!(r["openings"], 7, "six windows and the door: {r}");

    // Daylight studios: the west studio better daylit than the east one behind the block.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u5") });
    open(&mut app, work.path(), "daylight_studio");
    let d = finish(&mut app, "daylight.run", "daylight.collect", json!({}));
    assert_eq!(d["spaces"].as_array().unwrap().len(), 2, "{d:#}");
    // The west studio's sensors are at x < 6.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let st = reviera_core::daylight::study(&store, None).unwrap();
    let (west, east): (Vec<_>, Vec<_>) = st.results.spaces.iter().partition(|s| s.points[0][0] < 6.0);
    assert_eq!((west.len(), east.len()), (1, 1));
    let (w, e) = (west[0].average, east[0].average);
    assert!(w > e && e > 0.0 && w < 20.0, "west {w}% east {e}%");

    // Analysis tools: the canopy analysed from its saved model, the route to the exit, the duct
    // system's pressure drop and LP1's loads.
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().join("u6") });
    open(&mut app, work.path(), "analysis_tools");
    let r = finish(&mut app, "structural_analysis.run", "structural_analysis.collect", json!({}));
    let cases = r["cases"].as_array().unwrap();
    let case = |n: &str| cases.iter().find(|c| c["name"] == n).unwrap_or_else(|| panic!("no case {n}: {r:#}"));
    assert_eq!(cases.len(), 4, "D, Lr, W and U1");
    let u1 = f(&case("U1")["max_moment"]);
    assert!((18_000.0..23_000.0).contains(&u1), "U1 {u1} N·m");
    let (d, lr) = (f(&case("D")["total_vertical_reaction"]), f(&case("Lr")["total_vertical_reaction"]));
    assert!((f(&case("U1")["total_vertical_reaction"]) - (1.2 * d + 1.6 * lr)).abs() < 1.0, "U1 is 1.2 D + 1.6 Lr");
    let paths = ok(&mut app, "path_of_travel.list", json!({}));
    let paths = paths.as_array().unwrap();
    assert_eq!(paths.len(), 1);
    assert!((f(&paths[0]["length"]) - 13.4668).abs() < 0.01, "{paths:#?}");
    let net = ok(&mut app, "mep_network.calculate", json!({}));
    assert!((40.0..50.0).contains(&f(&net["systems"][0]["critical_loss"])), "{net:#}");
    let circuits = ok(&mut app, "circuit.list", json!({}));
    let names: Vec<&str> = circuits.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["LP1-1", "LP1-2"]);
}
