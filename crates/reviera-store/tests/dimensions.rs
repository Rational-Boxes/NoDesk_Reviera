// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Aligned dimensions (spec Ch 5 §5.13, DOC-ANN-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn values(r: &CommandRegistry, doc: &mut Document, view: &Value) -> Vec<f64> {
    let list = run(r, doc, "anno.list", json!({"view_id": view})).unwrap();
    list[0]["measured"]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_f64().unwrap() * 1e6).round() / 1e6)
        .collect()
}

#[test]
fn a_dimension_between_wall_faces_follows_the_walls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dims.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = reviera_core::plans::levels(doc.store())[0].2;
    let plan = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({"level_ids": [level.0]}),
    )
    .unwrap()[0]
        .clone();
    let wall = |doc: &mut Document, y: f64| -> u64 {
        run(
            &r,
            doc,
            "wall.create",
            json!({"curve": {"line": {"start": [0, y], "end": [6, y]}}, "base_level": level.0,
                   "top_constraint": {"unconnected": 3}}),
        )
        .unwrap()
        .as_u64()
        .unwrap()
    };
    let (a, b) = (wall(&mut doc, 0.0), wall(&mut doc, 4.0));
    // A's exterior face (y = 0.1) to B's interior face (y = 3.9), and on to B's centreline.
    let dim = run(
        &r,
        &mut doc,
        "dim.create_aligned",
        json!({"view_id": plan,
               "refs": [{"element": a, "sub": "face:ext", "point": [2, 0.1]},
                        {"element": b, "sub": "face:int", "point": [2, 3.9]},
                        {"element": b, "sub": "axis", "point": [2, 4]}],
               "line_point": [7, 2]}),
    )
    .unwrap();
    assert_eq!(values(&r, &mut doc, &plan), [3.8, 0.1]);

    // Moving wall B moves the dimension's references.
    run(
        &r,
        &mut doc,
        "wall.set_location",
        json!({"wall_id": b, "curve": {"line": {"start": [0, 5], "end": [6, 5]}}}),
    )
    .unwrap();
    assert_eq!(values(&r, &mut doc, &plan), [4.8, 0.1]);

    // Non-parallel references and single references are refused.
    let c = run(
        &r,
        &mut doc,
        "wall.create",
        json!({"curve": {"line": {"start": [8, 0], "end": [8, 5]}}, "base_level": level.0,
               "top_constraint": {"unconnected": 3}}),
    )
    .unwrap();
    let bad = run(
        &r,
        &mut doc,
        "dim.create_aligned",
        json!({"view_id": plan,
               "refs": [{"element": a, "sub": "axis", "point": [2, 0]},
                        {"element": c, "sub": "axis", "point": [8, 2]}],
               "line_point": [7, 2]}),
    )
    .unwrap_err();
    assert_eq!(bad.code, "dim.not_parallel");

    // Saved with the model, and orphaned (not deleted) when a wall goes.
    project.save(&doc).unwrap();
    let (_, mut reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(values(&r, &mut reopened, &plan), [4.8, 0.1]);
    run(&r, &mut doc, "element.delete", json!({"element_ids": [b]})).unwrap();
    let list = run(&r, &mut doc, "anno.list", json!({"view_id": plan})).unwrap();
    assert_eq!(list[0]["measured"]["orphaned"], json!([false, true, true]));
    assert_eq!(list[0]["id"], dim);

    // Deleting the view deletes its annotation.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({"element_ids": [plan]}),
    )
    .unwrap();
    assert!(deleted.as_array().unwrap().contains(&dim));
    assert!(doc.store().get(ElementId(dim.as_u64().unwrap())).is_none());
}
