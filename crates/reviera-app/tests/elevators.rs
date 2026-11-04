// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Elevators (ARC-VT-001 to ARC-VT-004): sized from the packs the building code chooses, made of
//! a car, a shaft that cuts the floors, hoistway walls and an entrance per level; written as an
//! IfcTransportElement; its hoistway and landings kept clear; checked for car area, accessible
//! size and the stretcher; and deleted as one.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") })
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A model of four levels 3.5 m apart, each with a 12 × 10 m floor; returns the levels and floors.
fn building(app: &mut App) -> (Vec<Value>, Vec<u64>) {
    ok(app, "file.new", json!({}));
    let mut levels = vec![ok(app, "level.list", json!({}))[0]["id"].clone()];
    for k in 1..4 {
        levels.push(ok(app, "level.create", json!({ "elevation": 3.5 * f64::from(k) })));
    }
    let c = [[0.0, 0.0], [12.0, 0.0], [12.0, 10.0], [0.0, 10.0]];
    let curves: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    let floors = levels.iter().map(|l| ok(app, "floor.create", json!({ "loops": [curves], "level_id": l })).as_u64().unwrap()).collect();
    (levels, floors)
}

fn intrusions(app: &mut App) -> Vec<Value> {
    ok(app, "clearance.check", json!({})).as_array().cloned().unwrap_or_default()
}

#[test]
fn an_elevator_is_sized_made_written_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (levels, _) = building(&mut app);

    // The US packs (the default code is the IBC): four storeys need the stretcher, so the
    // 3500 lb car with its MRL hoistway, pit and overhead.
    let packs = ok(&mut app, "vt.rule_packs", json!({}));
    let used: Vec<&str> = packs.as_array().unwrap().iter().filter(|p| p["in_use"] == true).map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(used, ["us_ibc_ada", "us_a17", "practice_us"]);
    let made = ok(&mut app, "elevator.create", json!({ "point": [6.0, 4.0], "base_level": levels[0], "top_level": levels[3] }));
    assert_eq!(made["load"], 1588.0);
    assert_eq!(made["persons"], 21);
    assert_eq!(made["walls"].as_array().unwrap().len(), 4);
    assert_eq!(made["entrances"].as_array().unwrap().len(), 4);
    let list = ok(&mut app, "elevator.list", json!({}));
    assert_eq!(list[0]["levels"].as_array().unwrap().len(), 4);

    // Nothing in the hoistway or the landings, nothing overlapping, and nothing to report.
    assert_eq!(intrusions(&mut app), Vec::<Value>::new());
    let overlaps = ok(&mut app, "overlap.find", json!({}));
    assert_eq!(overlaps["overlaps"], json!([]), "{overlaps:#}");
    assert_eq!(ok(&mut app, "elevator.check", json!({})), json!([]));

    // A partition run across the hoistway on level 2 is reported.
    let wall = ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [4.0, 5.0], "end": [8.0, 5.0] } }, "base_level": levels[1], "top_constraint": { "unconnected": 3.0 } }));
    let found = intrusions(&mut app);
    assert!(found.iter().any(|i| i["kind"] == "hoistway" && i["other"] == wall), "{found:#?}");
    ok(&mut app, "element.delete", json!({ "element_ids": [wall] }));

    // Written as an IfcTransportElement with its property sets.
    let path = dir.path().join("elevator.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCTRANSPORTELEMENT(") && text.contains(".ELEVATOR."), "no elevator in the file");
    assert!(text.contains("'Pset_TransportElementElevator'") && text.contains("'Reviera_Elevator'"));

    // Deleted as one: car, shaft, four walls and four entrances.
    let car = made["elevator"].clone();
    assert_eq!(ok(&mut app, "elevator.delete", json!({ "elevator": made["walls"][2] })), 10);
    assert_eq!(ok(&mut app, "elevator.list", json!({})), json!([]));
    assert!(run(&mut app, "elevator.delete", json!({ "elevator": car })).is_err());
}

#[test]
fn a_small_car_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (levels, _) = building(&mut app);
    // 1500 × 1200 mm, 800 mm door, for 1134 kg: too small for the ADA and the stretcher.
    let made = ok(&mut app, "elevator.create", json!({ "point": [6.0, 4.0], "base_level": levels[0], "top_level": levels[3], "load": 1134, "car_width": 1.5, "car_depth": 1.2, "door_width": 0.8, "walls": false }));
    let f = ok(&mut app, "elevator.check", json!({}));
    let rules: Vec<(&str, &str)> = f.as_array().unwrap().iter().map(|x| (x["rule"].as_str().unwrap(), x["severity"].as_str().unwrap())).collect();
    assert!(rules.contains(&("ADA 2010 407.4.1", "error")), "{f:#}");
    assert!(rules.contains(&("IBC 3002.4", "error")), "{f:#}");
    assert!(f.as_array().unwrap().iter().all(|x| x["elevator"] == made["elevator"]), "{f:#}");

    // Under England's packs: EN 81-20's area table is advisory, so an oversized car is a note.
    ok(&mut app, "vt.set_rule_packs", json!({ "packs": ["en81", "england_adm", "practice_en"] }));
    let made = ok(&mut app, "elevator.create", json!({ "point": [2.0, 4.0], "base_level": levels[0], "top_level": levels[1], "load": 630, "car_width": 1.4, "car_depth": 1.6 }));
    let f = ok(&mut app, "elevator.check", json!({ "elevators": [made["elevator"]] }));
    let area: Vec<&Value> = f.as_array().unwrap().iter().filter(|x| x["kind"] == "car_area").collect();
    assert_eq!(area.len(), 1, "{f:#}");
    assert_eq!(area[0]["severity"], "note");
    assert!((area[0]["required"].as_f64().unwrap() - 1.66).abs() < 1e-9);
    // Its 1500 × 1500 mm landings are virtual volumes in front of each entrance.
    let zones = ok(&mut app, "clearance.list", json!({ "element_ids": [made["elevator"]] }));
    let landings: Vec<&Value> = zones.as_array().unwrap().iter().filter(|c| c["kind"] == "elevator_landing").collect();
    assert_eq!(landings.len(), 2);
    let (min, max) = (&landings[0]["min"], &landings[0]["max"]);
    assert!(((max[0].as_f64().unwrap() - min[0].as_f64().unwrap()) - 1.5).abs() < 1e-6, "{landings:#?}");
    assert!((max[1].as_f64().unwrap() - (4.0 - 0.2)).abs() < 1e-6, "{landings:#?}");
}

