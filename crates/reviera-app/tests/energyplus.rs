// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The EnergyPlus provider end to end (ANA-ENV-007) with a stand-in `energyplus` program (a
//! shell script answering `--version`, keeping the epJSON and weather file it was given and
//! writing an `eplusout.sql`): energy.providers finds it, energy.run with provider energyplus
//! writes zones, paired surfaces, the window, gains, schedules and ideal loads with the
//! installed version, the job reads the SQL back into monthly results recorded as an
//! EnergyPlus run, and a run EnergyPlus fails passes its severe errors on.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use reviera_core::energyplus::{COOLING, COOLING_RATE, HEATING, HEATING_RATE, LIGHTS, Row};
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

/// A synthetic weather year (the values matter little: the stand-in does not simulate).
fn epw() -> String {
    let mut s = String::from("LOCATION,Testville,TS,Testland,Synthetic,123456,52.0,0.0,0.0,40.0\n");
    for h in ["DESIGN CONDITIONS,0", "TYPICAL/EXTREME PERIODS,0", "GROUND TEMPERATURES,0", "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0", "COMMENTS 1,synthetic", "COMMENTS 2,", "DATA PERIODS,1,1,Data,Monday, 1/ 1,12/31"] {
        s.push_str(h);
        s.push('\n');
    }
    for (m, d) in [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31].iter().enumerate() {
        for day in 1..=*d {
            for hour in 1..=24 {
                let t = if (5..=8).contains(&m) { 22.0 } else { 2.0 };
                s.push_str(&format!("2001,{},{day},{hour},60,A7A7,{t:.1},{:.1},50,101325,0,1300,300,0,0,0,0,0,0,0,180,3.0,5,5,15.0,77777,9,999999999,0,0.1,0,88,0.0,0.0,0.0\n", m + 1, t - 6.0));
            }
        }
    }
    s
}

/// The stand-in: answers `--version`; otherwise keeps its inputs in `keep` and writes `sql`
/// as `out/eplusout.sql`, or fails with a severe error when `keep/fail` exists.
fn stand_in(dir: &Path, keep: &Path, sql: &Path) -> PathBuf {
    let p = dir.join("energyplus");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'EnergyPlus, Version 26.2.0-4bd7a1f26f'; exit 0; fi\n\
         cp in.epJSON '{k}/in.epJSON'; cp in.epw '{k}/in.epw'; echo \"$@\" > '{k}/args'\n\
         if [ -e '{k}/fail' ]; then printf '   ** Severe  ** GetSurfaceData: bad vertices\\n   **  Fatal  ** Errors found\\n' > out/eplusout.err; exit 1; fi\n\
         printf '   ** Warning ** a stand-in warning\\n' > out/eplusout.err\ncp '{s}' out/eplusout.sql\n",
        k = keep.display(),
        s = sql.display()
    );
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn wait(app: &mut App, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(300);
    loop {
        let s = ok(app, "compute.status", json!({ "job_id": id }));
        if ["succeeded", "failed", "cancelled"].contains(&s["state"].as_str().unwrap()) {
            return s;
        }
        assert!(Instant::now() < end, "{s:#}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn energyplus_provider() {
    let (host, runner) = programs();
    let base = if std::env::temp_dir().as_os_str().len() > 40 { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = tempfile::Builder::new().prefix("rvp").tempdir_in(&base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let keep = dir.path().join("keep");
    std::fs::create_dir_all(&keep).unwrap();
    // EnergyPlus's results for the four rooms: 1000 kWh heating in January each, 500 kWh
    // cooling in July, lights, and hourly peaks.
    let sql = dir.path().join("fixture.sql");
    let mut rows = Vec::new();
    for k in 1..=4 {
        let key = format!("S{k} IDEAL LOADS");
        rows.push(Row { key: key.clone(), name: HEATING.into(), frequency: "Monthly".into(), month: 1, time: 1, value: 3.6e9 });
        rows.push(Row { key: key.clone(), name: COOLING.into(), frequency: "Monthly".into(), month: 7, time: 7, value: 1.8e9 });
        rows.push(Row { key: format!("S{k}"), name: LIGHTS.into(), frequency: "Monthly".into(), month: 1, time: 1, value: 3.6e8 });
        rows.push(Row { key: key.clone(), name: HEATING_RATE.into(), frequency: "Hourly".into(), month: 1, time: 100, value: 1500.0 });
        rows.push(Row { key, name: COOLING_RATE.into(), frequency: "Hourly".into(), month: 7, time: 5000 + k, value: 900.0 });
    }
    reviera_compute::energyplus::write_sql(&sql, &rows).unwrap();
    let program = stand_in(dir.path(), &keep, &sql);
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_COMPUTE_STATE", state.path());
        std::env::set_var("REVIERA_COMPUTE_HOST", &host);
        std::env::set_var("REVIERA_COMPUTE_RUNNERS", &runner);
        std::env::set_var("REVIERA_COMPUTE_HOST_IDLE_S", "5");
        std::env::set_var("REVIERA_COMPUTE_RUNNER_IDLE_S", "5");
        std::env::set_var("REVIERA_ENERGYPLUS", &program);
    }
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    let providers = ok(&mut app, "energy.providers", json!({}));
    assert_eq!(providers[1]["name"], "energyplus");
    assert_eq!(providers[1]["available"], true, "{providers}");
    assert_eq!(providers[1]["version"], "26.2");

    ok(&mut app, "file.new", json!({}));
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("eplus.ifc") }));
    let epw_path = dir.path().join("year.epw");
    std::fs::write(&epw_path, epw()).unwrap();
    ok(&mut app, "weather.import", json!({ "path": epw_path }));
    ok(&mut app, "location.set", json!({ "lat": 52.0, "lon": 0.0 }));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let mut walls = Vec::new();
    for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
        walls.push(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })));
    }
    let spaces = ok(&mut app, "space.place_auto", json!({ "level_id": level.0 }));
    ok(&mut app, "window.place", json!({ "host_wall_id": walls[0], "point_along": 2.0 }));
    ok(&mut app, "space_type.create", json!({ "name": "Office", "values": { "lighting_load_density": 8.0, "power_load_density": 10.0, "area_per_person": 10.0, "sensible_heat_per_person": 75.0, "latent_heat_per_person": 55.0, "outside_air_per_person": 10.0, "heating_set_point": 20.0, "cooling_set_point": 24.0 } }));
    ok(&mut app, "space.set_type", json!({ "element_ids": spaces, "type_id": "office" }));

    let run = ok(&mut app, "energy.run", json!({ "provider": "energyplus", "options": { "heating": "heat_pump", "grid_factor": 0.25 } }));
    assert_eq!(run["provider"], "energyplus");
    let id = run["job_id"].as_str().unwrap().to_owned();
    let st = wait(&mut app, &id);
    assert_eq!(st["state"], "succeeded", "{st:#}");
    let r = ok(&mut app, "energy.collect", json!({ "job_id": id }));
    let f = |v: &Value| v.as_f64().unwrap();
    assert_eq!(r["provider"], "energyplus");
    assert!((f(&r["months"][0]["heating_load"]) - 4000.0).abs() < 1e-6, "{}", r["months"][0]);
    assert!((f(&r["months"][6]["cooling_load"]) - 2000.0).abs() < 1e-6);
    assert!((f(&r["year"]["lighting"]) - 400.0).abs() < 1e-6);
    assert_eq!(f(&r["peak_heating"]), 6000.0);
    assert_eq!(f(&r["peak_cooling"]), 900.0, "the cooling peaks fall in different hours");
    // A heat pump at COP 3: a third of the heat as electricity; carbon from the grid factor.
    assert!((f(&r["year"]["heating"]) - 4000.0 / 3.0).abs() < 1e-6);
    assert!((f(&r["year"]["carbon"]) - 0.25 * f(&r["year"]["electricity"])).abs() < 1e-6);
    assert_eq!(r["spaces"].as_array().unwrap().len(), 4);
    assert!(r["spaces"].as_array().unwrap().iter().all(|s| s["space"].is_u64()), "zones map back to the spaces");
    assert_eq!(ok(&mut app, "energy.results", json!({}))["provider"], "energyplus");
    assert_eq!(ok(&mut app, "energy.status", json!({}))["stale"], false);

    // What EnergyPlus was given.
    assert_eq!(std::fs::read_to_string(keep.join("args")).unwrap().trim(), "-w in.epw -d out in.epJSON");
    assert_eq!(std::fs::read_to_string(keep.join("in.epw")).unwrap(), epw());
    let m: Value = serde_json::from_str(&std::fs::read_to_string(keep.join("in.epJSON")).unwrap()).unwrap();
    assert_eq!(m["Version"]["Version 1"]["version_identifier"], "26.2");
    let zones: Vec<&String> = m["Zone"].as_object().unwrap().keys().collect();
    assert_eq!(zones, ["s1", "s2", "s3", "s4"]);
    let surfaces = m["BuildingSurface:Detailed"].as_object().unwrap();
    assert_eq!(surfaces.len(), 24);
    let mut by_bc: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, s) in surfaces {
        *by_bc.entry(s["outside_boundary_condition"].as_str().unwrap()).or_default() += 1;
        assert!(zones.iter().any(|z| *z == s["zone_name"].as_str().unwrap()));
        assert!(m["Construction"][s["construction_name"].as_str().unwrap()].is_object(), "{name}");
        if s["outside_boundary_condition"] == "Surface" {
            let other = &surfaces[s["outside_boundary_condition_object"].as_str().unwrap()];
            assert_eq!(other["outside_boundary_condition_object"], name.as_str());
            assert_ne!(other["zone_name"], s["zone_name"]);
        }
        assert!(s["vertices"].as_array().unwrap().len() >= 3);
    }
    assert_eq!(by_bc, BTreeMap::from([("Ground", 4), ("Outdoors", 12), ("Surface", 8)]));
    let windows = m["FenestrationSurface:Detailed"].as_object().unwrap();
    assert_eq!(windows.len(), 1);
    let w = windows.values().next().unwrap();
    assert_eq!(w["surface_type"], "Window");
    assert!(surfaces.contains_key(w["building_surface_name"].as_str().unwrap()));
    let glazing = m["Construction"][w["construction_name"].as_str().unwrap()]["outside_layer"].as_str().unwrap();
    assert!(m["WindowMaterial:SimpleGlazingSystem"][glazing]["u_factor"].as_f64().unwrap() > 0.0);
    assert_eq!(m["Lights"]["s1 Lights"]["watts_per_floor_area"], 8.0);
    assert_eq!(m["ZoneHVAC:IdealLoadsAirSystem"].as_object().unwrap().len(), 4);
    assert_eq!(m["ZoneControl:Thermostat"]["s2 Thermostat"]["control_1_name"], "s2 Set Points");
    assert!(m["Output:SQLite"].is_object());
    assert_eq!(m["RunPeriod"]["Year"]["day_of_week_for_start_day"], "Monday");
    let sp: Vec<Value> = m["Schedule:Compact"]["s1 Heating Set Point"]["data"].as_array().unwrap().iter().map(|d| d["field"].clone()).collect();
    assert!(sp.contains(&json!(20.0)) && sp.contains(&json!(16.0)), "{sp:?}");

    // EnergyPlus fails: its severe errors are passed on.
    std::fs::write(keep.join("fail"), "").unwrap();
    let id = ok(&mut app, "energy.run", json!({ "provider": "energyplus" }))["job_id"].as_str().unwrap().to_owned();
    let st = wait(&mut app, &id);
    assert_eq!(st["state"], "failed");
    assert!(st.to_string().contains("bad vertices"), "{st:#}");
    let e = app.execute("energy.run", json!({ "provider": "trnsys" }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "energy.bad_provider");
}
