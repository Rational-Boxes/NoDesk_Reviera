// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Tags and text notes (DOC-ANN-009…011): live tag text, tags following their hosts, Tag All
//! without duplicates, orphans, room tags, text notes with leaders, and PDF output.

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    serde_json::from_value(v).unwrap()
}

fn tags(app: &mut App, plan: ElementId) -> Vec<Value> {
    run(app, "tag.list", json!({ "view_id": plan.0 }))
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn tags_and_text_notes_follow_the_model() {
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
    let plans = run(
        &mut app,
        "view.create_plan",
        json!({ "level_ids": [level.0] }),
    )
    .unwrap();
    let plan: ElementId = serde_json::from_value(plans[0].clone()).unwrap();
    // A 6 × 4 m room with a door in its south wall.
    let c = [[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]];
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
    let door = id(run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": walls[0].0, "point_along": 2.0 }),
    )
    .unwrap());
    let room = id(run(
        &mut app,
        "room.place",
        json!({ "level_id": level.0, "point": [3.0, 2.0] }),
    )
    .unwrap());

    // A door tag shows the Mark live.
    let tag = id(run(
        &mut app,
        "tag.create",
        json!({ "view_id": plan.0, "host_ref": door.0 }),
    )
    .unwrap());
    let t = &tags(&mut app, plan)[0];
    assert_eq!(t["data"]["tagType"], "Door Tag");
    let head = t["head"].clone();
    run(
        &mut app,
        "param.set",
        json!({ "element_ids": [door.0], "param_ref": "elem.mark", "value": "D101" }),
    )
    .unwrap();
    assert_eq!(tags(&mut app, plan)[0]["lines"], json!(["D101"]));

    // The tag follows its door along the wall; moved with the door, it is not moved twice.
    run(
        &mut app,
        "element.move",
        json!({ "element_ids": [door.0, tag.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    let moved = tags(&mut app, plan)[0]["head"].clone();
    let dx = moved[0].as_f64().unwrap() - head[0].as_f64().unwrap();
    assert!((dx.abs() - 1.0).abs() < 1e-6, "moved {dx}");
    // Moving the tag alone moves its head off the door.
    run(
        &mut app,
        "element.move",
        json!({ "element_ids": [tag.0], "vector": [0.0, -1.0, 0.0] }),
    )
    .unwrap();
    let t = &tags(&mut app, plan)[0];
    assert!((t["head"][1].as_f64().unwrap() - moved[1].as_f64().unwrap() + 1.0).abs() < 1e-6);
    run(
        &mut app,
        "tag.set_leader",
        json!({ "annotation_id": tag.0, "end_mode": "attached" }),
    )
    .unwrap();

    // A door tag cannot tag a wall.
    let err = run(
        &mut app,
        "tag.create",
        json!({ "view_id": plan.0, "host_ref": walls[1].0, "tag_type_id": "Door Tag" }),
    )
    .unwrap_err();
    assert_eq!(err.code, "tag.wrong_category");

    // Tag All Not Tagged: the room and the walls; the door already has its tag.
    let cats = json!(["cat.doors", "cat.rooms", "cat.walls"]);
    let made = run(
        &mut app,
        "tag.tag_all",
        json!({ "view_id": plan.0, "categories": cats }),
    )
    .unwrap();
    assert_eq!(made.as_array().unwrap().len(), 5);
    let again = run(
        &mut app,
        "tag.tag_all",
        json!({ "view_id": plan.0, "categories": cats }),
    )
    .unwrap();
    assert_eq!(again, json!([]));
    let all = tags(&mut app, plan);
    let room_tag = all.iter().find(|t| t["host"] == json!(room.0)).unwrap();
    assert_eq!(room_tag["data"]["tagType"], "Room Tag With Area");
    assert_eq!(room_tag["lines"][2], "22.04 m²");

    // A text note with a leader; emptying it deletes it.
    let note = id(run(
        &mut app,
        "text.create",
        json!({
            "view_id": plan.0, "position": [8.0, 3.0],
            "content": "CONCRETE SLAB\nSEE DETAIL 4", "type_id": "Text 3.5 mm",
        }),
    )
    .unwrap());
    run(
        &mut app,
        "text.add_leader",
        json!({ "annotation_id": note.0, "points": [[5.0, 2.0]] }),
    )
    .unwrap();
    let texts = run(&mut app, "text.list", json!({ "view_id": plan.0 })).unwrap();
    assert_eq!(texts[0]["data"]["leaders"][0]["side"], "left");
    let err = run(
        &mut app,
        "text.create",
        json!({ "view_id": plan.0, "position": [0.0, 0.0], "content": "X", "type_id": "Huge" }),
    )
    .unwrap_err();
    assert_eq!(err.code, "text.unknown_type");

    // The PDF carries the tag and note text.
    let path = std::env::temp_dir().join(format!("reviera-notes-{}.pdf", std::process::id()));
    run(
        &mut app,
        "export.pdf",
        json!({ "range": { "views": [plan.0] }, "path_or_folder": path }),
    )
    .unwrap();
    let pdf = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    std::fs::remove_file(&path).ok();
    // `22.04 m²` in WinAnsi, written in hex for its ² byte.
    for t in ["(D101)", "(SEE DETAIL 4)", "<32322E3034206DB2>"] {
        assert!(pdf.contains(t), "{t} missing");
    }

    // A deleted door leaves an orphaned tag; deleting the plan removes its annotation.
    run(
        &mut app,
        "element.delete",
        json!({ "element_ids": [door.0] }),
    )
    .unwrap();
    let t = tags(&mut app, plan)
        .into_iter()
        .find(|t| t["id"] == json!(tag.0))
        .unwrap();
    assert_eq!(t["orphaned"], true);
    run(
        &mut app,
        "text.set_content",
        json!({ "annotation_id": note.0, "content": " " }),
    )
    .unwrap();
    assert!(app.document(doc).unwrap().doc.store().get(note).is_none());
    run(&mut app, "view.delete", json!({ "view_ids": [plan.0] })).unwrap();
    let store = app.document(doc).unwrap().doc.store();
    assert!(
        store
            .iter()
            .all(|e| !reviera_core::annotations::is_annotation(e))
    );
}
