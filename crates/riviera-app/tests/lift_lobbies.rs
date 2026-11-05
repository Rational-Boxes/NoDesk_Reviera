// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Evacuation elevator lobbies (ANA-ROUTE-007): the room in front of the landing, its size for a
//! quarter of the floor's load and its wheelchair spaces, and direct access to an exit stair.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn a_lobby_is_sized_for_a_quarter_of_the_floor_and_reaches_a_stair() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let l1 = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let l2 = ok(&mut app, "level.create", json!({ "elevation": 3.5 }));
    let made = ok(&mut app, "elevator.create", json!({ "point": [10.0, 10.0], "base_level": l1, "top_level": l2, "equipment": false }));
    let lift = made["elevator"].clone();
    ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": lift, "role": "occupant_evacuation" }));
    // A lobby on Level 2 only: three walls against the hoistway's front wall (its outer face 0.2 m
    // in front of the entrance line), 3 m deep.
    let w = f(&made["hoistway"][0]);
    let (x0, x1, face) = (10.0 - w / 2.0, 10.0 + w / 2.0, 9.8);
    for (a, b) in [([x0, face], [x0, face - 3.0]), ([x0, face - 3.0], [x1, face - 3.0]), ([x1, face - 3.0], [x1, face])] {
        ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": l2, "top_constraint": { "unconnected": 3.0 } }));
    }
    let room = ok(&mut app, "room.place", json!({ "level_id": l2, "point": [10.0, 8.0] }));
    let check = |app: &mut App, occupants: f64| ok(app, "elevator.check_lobbies", json!({ "occupants": occupants }));

    // 40 on the floor: 10 waiting at 0.28 m² and one wheelchair space, 3.72 m².
    let c = check(&mut app, 40.0);
    let rows = c.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{c:#}");
    let (on1, on2) = (rows.iter().find(|r| r["level"] == l1).unwrap(), rows.iter().find(|r| r["level"] == l2).unwrap());
    assert!(on1["room"].is_null() && on1["problems"][0].as_str().unwrap().contains("no enclosed lobby"), "{on1:#}");
    assert!(on2["room"] == room || on2["room"].is_number(), "{on2:#}");
    assert!((f(&on2["required"]) - (10.0 * 0.2787 + 0.762 * 1.219)).abs() < 1e-9 && f(&on2["area"]) > f(&on2["required"]), "{on2:#}");
    // No stair at all: no direct access.
    assert_eq!(on2["stair_access"], false);
    assert!(on2["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("exit stair")), "{on2:#}");
    // 200 on the floor: 50 waiting and four wheelchair spaces, 17.6 m²: too small.
    let c = check(&mut app, 200.0);
    let on2 = c.as_array().unwrap().iter().find(|r| r["level"] == l2).unwrap().clone();
    assert_eq!(on2["wheelchair_spaces"], 4);
    assert!(on2["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("IBC 3008.6.3")), "{on2:#}");
    // A stair arriving at Level 2 just beside the lobby: direct access.
    ok(&mut app, "stair.create", json!({ "base_level": l1, "width": 1.2, "railings": false, "runs": [{ "start": [x1 + 6.5, 7.5], "end": [x1 + 1.0, 7.5] }] }));
    let c = check(&mut app, 40.0);
    let on2 = c.as_array().unwrap().iter().find(|r| r["level"] == l2).unwrap().clone();
    assert_eq!(on2["stair_access"], true, "{on2:#}");
    assert!(on2["problems"].as_array().unwrap().is_empty(), "{on2:#}");
    // Not an evacuation elevator: not checked.
    ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": lift, "role": "none" }));
    assert_eq!(check(&mut app, 40.0), json!([]));
}
