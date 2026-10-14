// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Virtual volumes (ARC-MOD-031): a door's swing is kept clear of other elements but not of
//! its own wall; two doors that would hit each other are found; ADA 2010 maneuvering
//! clearances flag a wall too close to the latch side; and the volumes are drawn only by a
//! view's choice.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn wall(app: &mut App, level: u64, a: [f64; 2], b: [f64; 2]) -> u64 {
    run(
        app,
        "wall.create",
        json!({ "curve": { "line": { "start": a, "end": b } },
                "base_level": level, "top_constraint": { "unconnected": 3 } }),
    )
    .unwrap()
    .as_u64()
    .unwrap()
}

#[test]
fn door_swings_and_accessibility_clearances() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    // A 6 m wall along x, its exterior (the door's facing side) toward +y.
    let w = wall(&mut app, level, [0.0, 0.0], [6.0, 0.0]);
    let door = run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": w, "point_along": 3.0 }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let swing = &run(&mut app, "clearance.list", json!({})).unwrap()[0];
    assert_eq!(swing["kind"], "door_swing");
    assert_eq!(swing["element"], door);
    // Only its swing (no rule set yet); nothing intrudes, its own wall allowed.
    assert_eq!(
        run(&mut app, "clearance.list", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        run(&mut app, "clearance.check", json!({})).unwrap(),
        json!([])
    );
    // A short wall standing in the swing, on the swing side.
    let side = if swing["max"][1].as_f64().unwrap() > 0.5 {
        1.0
    } else {
        -1.0
    };
    let blocker = wall(&mut app, level, [3.0, side * 0.5], [3.0, side * 1.5]);
    let found = run(&mut app, "clearance.check", json!({})).unwrap();
    let hits: Vec<&Value> = found.as_array().unwrap().iter().collect();
    assert_eq!(hits.len(), 1, "{found:#}");
    assert_eq!(hits[0]["other"], blocker);
    assert_eq!(hits[0]["kind"], "door_swing");
    run(
        &mut app,
        "element.delete",
        json!({ "element_ids": [blocker] }),
    )
    .unwrap();

    // A second door opposite the first, swinging into the same space: the swings collide.
    let w2 = wall(&mut app, level, [0.0, side * 1.6], [6.0, side * 1.6]);
    let d2 = run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": w2, "point_along": 3.0, "flip_facing": side > 0.0 }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let found = run(&mut app, "clearance.check", json!({})).unwrap();
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["other_is_swing"] == true
                && ((i["element"] == door && i["other"] == d2)
                    || (i["element"] == d2 && i["other"] == door))),
        "{found:#}"
    );
    run(
        &mut app,
        "element.delete",
        json!({ "element_ids": [d2, w2] }),
    )
    .unwrap();

    // ADA 2010: a wall 300 mm past the latch on the pull side intrudes the 455 mm clearance.
    run(
        &mut app,
        "clearance.set_rule_set",
        json!({ "standard": "ada_2010" }),
    )
    .unwrap();
    let list = run(&mut app, "clearance.list", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3, "{list:#}");
    assert_eq!(
        run(&mut app, "clearance.check", json!({})).unwrap(),
        json!([])
    );
    let pull = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "pull_side")
        .unwrap();
    // The latch side is where the pull clearance reaches farther from the door's centre.
    let (x0, x1) = (
        pull["min"][0].as_f64().unwrap(),
        pull["max"][0].as_f64().unwrap(),
    );
    let latch = if (x1 - 3.0) > (3.0 - x0) { 1.0 } else { -1.0 };
    let half = 0.45 + 0.3; // the door's half width and 300 mm past the latch
    let x = 3.0 + latch * half;
    let side_wall = wall(&mut app, level, [x, side * 0.1], [x, side * 2.0]);
    let found = run(&mut app, "clearance.check", json!({})).unwrap();
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "pull_side" && i["other"] == side_wall),
        "{found:#}"
    );

    // Drawn only by a view's choice.
    let scene = app.render_scene(app.active().unwrap()).unwrap();
    assert_eq!(
        scene
            .meshes
            .iter()
            .filter(|m| reviera_render::is_virtual(&m.class))
            .count(),
        3
    );
}
