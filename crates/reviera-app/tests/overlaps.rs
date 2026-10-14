// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The volume overlap engine over a model (ARC-MOD-030): a well-built room has none (joined
//! corners, a door in its wall, a floor and an attached roof touch but do not overlap), and
//! real overlaps are found with their volume.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn pairs(v: &Value) -> Vec<(u64, u64, f64)> {
    v["overlaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["a"].as_u64().unwrap(),
                o["b"].as_u64().unwrap(),
                o["volume"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_room_has_no_overlaps_until_something_crosses_it() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let rect = [[0.0, 0.0], [8.0, 0.0], [8.0, 5.0], [0.0, 5.0]];
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(
            run(
                &mut app,
                "wall.create",
                json!({
                    "curve": { "line": { "start": rect[(i + 1) % 4], "end": rect[i] } },
                    "base_level": level, "top_constraint": { "unconnected": 3 },
                }),
            )
            .unwrap()
            .as_u64()
            .unwrap(),
        );
    }
    run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": walls[0], "point_along": 4.0 }),
    )
    .unwrap();
    // A floor under the walls' inside faces, its top at the level.
    let inner = [[0.1, 0.1], [7.9, 0.1], [7.9, 4.9], [0.1, 4.9]];
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": inner[i], "end": inner[(i + 1) % 4] } }))
        .collect();
    run(
        &mut app,
        "floor.create",
        json!({ "loops": [curves], "level_id": level }),
    )
    .unwrap();
    // A gable roof on the walls' exterior faces, the walls attached.
    let outer = [[-0.1, -0.1], [8.1, -0.1], [8.1, 5.1], [-0.1, 5.1]];
    let curves: Vec<Value> = (0..4)
        .map(|i| {
            json!({ "line": { "start": outer[i], "end": outer[(i + 1) % 4] },
                    "defines_slope": i % 2 == 0, "overhang": 0.4 })
        })
        .collect();
    let roof = run(
        &mut app,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": level, "base_offset": 3.0 }),
    )
    .unwrap();
    run(&mut app, "roof.attach_walls", json!({ "roof_id": roof })).unwrap();

    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    assert_eq!(pairs(&found), vec![], "{found:#}");
    assert!(found["bodies"].as_u64().unwrap() >= 7);

    // A wall run through the room's side, and a railing through the opposite wall.
    let cross = run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [3.0, 5.5], "end": [3.0, 3.0] } },
            "base_level": level, "top_constraint": { "unconnected": 2 },
        }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let rail = run(
        &mut app,
        "railing.create",
        json!({ "path": [[6.0, -1.0], [6.0, 1.0]], "base_level": level }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    let p = pairs(&found);
    let has = |x: u64, y: u64| p.iter().any(|(a, b, _)| (*a, *b) == (x.min(y), x.max(y)));
    assert!(has(cross, walls[2]), "{found:#}");
    assert!(has(rail, walls[0]), "{found:#}");
    // The crossing wall (200 mm) through the 200 mm wall, 2 m high: 0.2 × 0.2 × 2 = 0.08 m³,
    // less where the two may have joined; and the crossing wall over the floor touches only.
    let v = p
        .iter()
        .find(|(a, b, _)| (*a, *b) == (cross.min(walls[2]), cross.max(walls[2])))
        .unwrap()
        .2;
    assert!(v > 0.01 && v < 0.081, "{v}");
    // Checking just the railing: only its overlap.
    let one = run(&mut app, "overlap.find", json!({ "element_ids": [rail] })).unwrap();
    assert_eq!(pairs(&one).len(), 1, "{one:#}");
    // A door and its wall belong together, unless asked.
    let all = run(&mut app, "overlap.find", json!({ "include_related": true })).unwrap();
    assert!(pairs(&all).len() >= pairs(&found).len());
}
