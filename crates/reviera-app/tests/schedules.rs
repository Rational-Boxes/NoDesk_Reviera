// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Schedules (DOC-SCH-001…005, 010, 012): live rows, sorting, itemize, totals, cell editing and
//! CSV export.

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    serde_json::from_value(v).unwrap()
}

fn table(app: &mut App, s: ElementId) -> Value {
    run(app, "schedule.get_table", json!({ "schedule_id": s.0 })).unwrap()
}

#[test]
fn door_and_room_schedules_stay_live_and_edit_the_model() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let doc = app.active().unwrap();
    let level = app
        .document(doc)
        .unwrap()
        .doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A 10 × 6 m house (outside faces), split in two rooms, with three doors.
    let c = [[0.1, 0.1], [9.9, 0.1], [9.9, 5.9], [0.1, 5.9]];
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &mut app,
            "wall.create",
            json!({
                "curve": { "line": { "start": c[(i + 1) % 4], "end": c[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    let partition = id(run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [5.0, 0.1], "end": [5.0, 5.9] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    for (host, along) in [(walls[0], 2.0), (walls[0], 7.0)] {
        run(
            &mut app,
            "door.place",
            json!({ "host_wall_id": host.0, "point_along": along }),
        )
        .unwrap();
    }
    let doors = id(run(
        &mut app,
        "schedule.create",
        json!({ "category": "cat.doors" }),
    )
    .unwrap());
    let t = table(&mut app, doors);
    assert_eq!(
        t["headings"],
        json!(["Mark", "Type", "Level", "Width", "Height"])
    );
    assert_eq!(t["rows"].as_array().unwrap().len(), 2);
    assert_eq!(t["rows"][0]["cells"][2]["text"], "Level 1");
    // A new door gains a row.
    run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": partition.0, "point_along": 3.0 }),
    )
    .unwrap();
    let t = table(&mut app, doors);
    assert_eq!(t["rows"].as_array().unwrap().len(), 3);
    assert_eq!(t["totals"][0], "Grand total: 3");
    // Editing a Mark changes the door; undo restores it.
    let first: Vec<u64> = serde_json::from_value(t["rows"][0]["elements"].clone()).unwrap();
    let before = t["rows"][0]["cells"][0]["text"].clone();
    run(
        &mut app,
        "schedule.set_cell",
        json!({ "element_ids": first, "field": "param:elem.mark", "value": "D-101" }),
    )
    .unwrap();
    let t = table(&mut app, doors);
    let marks: Vec<String> = t["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["cells"][0]["text"].as_str().unwrap().to_owned())
        .collect();
    assert!(marks.contains(&"D-101".to_owned()), "{marks:?}");
    run(&mut app, "edit.undo", json!({})).unwrap();
    let t = table(&mut app, doors);
    assert!(
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["cells"][0]["text"] == before)
    );
    // Computed cells are refused.
    let e = run(
        &mut app,
        "schedule.set_cell",
        json!({ "element_ids": first, "field": "insert.width", "value": "1 m" }),
    );
    assert!(e.is_err());
    // Itemize off, grouped by type: the three identical doors are one row of 3.
    run(
        &mut app,
        "schedule.set_fields",
        json!({ "schedule_id": doors.0, "fields": ["type", "count", "insert.width"] }),
    )
    .unwrap();
    run(
        &mut app,
        "schedule.set_sort",
        json!({ "schedule_id": doors.0, "levels": [{ "field": "type" }], "itemize": false }),
    )
    .unwrap();
    let t = table(&mut app, doors);
    assert_eq!(t["rows"].as_array().unwrap().len(), 1);
    assert_eq!(t["rows"][0]["cells"][1]["text"], "3");
    assert_eq!(t["rows"][0]["elements"].as_array().unwrap().len(), 3);

    // Rooms: two rooms whose areas add up in the grand total.
    run(
        &mut app,
        "room.place_automatically",
        json!({ "level_id": level.0 }),
    )
    .unwrap();
    let rooms = id(run(
        &mut app,
        "schedule.create",
        json!({ "category": "cat.rooms" }),
    )
    .unwrap());
    let t = table(&mut app, rooms);
    assert_eq!(t["rows"].as_array().unwrap().len(), 2);
    let area = |s: &Value| {
        s.as_str()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    let a: f64 = t["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| area(&r["cells"][3]["text"]))
        .sum();
    assert!(
        (area(&t["totals"][3]) - a).abs() < 0.02,
        "{} vs {a}",
        t["totals"][3]
    );
    // Renaming a room in its schedule renames the room.
    let first: Vec<u64> = serde_json::from_value(t["rows"][0]["elements"].clone()).unwrap();
    run(
        &mut app,
        "schedule.set_cell",
        json!({ "element_ids": first, "field": "room.name", "value": "Kitchen" }),
    )
    .unwrap();
    assert_eq!(
        table(&mut app, rooms)["rows"][0]["cells"][1]["text"],
        "Kitchen"
    );

    // CSV export, which will not overwrite without asking.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rooms.csv");
    run(
        &mut app,
        "schedule.export",
        json!({ "schedule_id": rooms.0, "path": path }),
    )
    .unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    assert!(
        csv.starts_with("Room Schedule\nNumber,Name,Level,Area,Perimeter,Volume\n"),
        "{csv}"
    );
    assert!(csv.contains("Kitchen"));
    assert!(
        run(
            &mut app,
            "schedule.export",
            json!({ "schedule_id": rooms.0, "path": path })
        )
        .is_err()
    );
}
