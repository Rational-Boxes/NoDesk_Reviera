// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Global parameters (DM-GPAR-001): changing "CorridorWidth" moves the labelled walls in one
//! transaction and undo restores both; a formula follows CorridorWidth and a cycle is refused;
//! 200 doors associated with "DoorSillHeight" move together.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<reviera_core::command::Outcome> {
    r.execute(doc, name, args, &Caller::user())
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn y_of(doc: &Document, wall: ElementId) -> f64 {
    reviera_core::walls::wall_recipe(doc.store().get(wall).unwrap()).unwrap().curve.start[1]
}

fn value(r: &CommandRegistry, doc: &mut Document, name: &str) -> f64 {
    let l = ok(r, doc, "global.list", json!({}));
    l.as_array().unwrap().iter().find(|g| g["name"] == name).unwrap()["value"].as_f64().unwrap()
}

#[test]
fn global_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("globals.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let plan = id(&ok(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }))[0]);
    let wall = |doc: &mut Document, y: f64| id(&ok(&r, doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, y], "end": [10.0, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let (a, b) = (wall(&mut doc, 0.0), wall(&mut doc, 2.5));
    let axis = |w: ElementId, y: f64| json!({ "element": w.0, "sub": "axis", "point": [5.0, y] });
    let dim = id(&ok(&r, &mut doc, "dim.create_aligned", json!({ "view_id": plan.0, "refs": [axis(a, 0.0), axis(b, 2.5)], "line_point": [12.0, 0.0] })));

    // CorridorWidth labels the dimension: the second wall moves to 1.8 m.
    ok(&r, &mut doc, "global.create", json!({ "name": "CorridorWidth", "data_type": "length", "value": 1.8, "group": "Dimensions" }));
    ok(&r, &mut doc, "global.label_dimension", json!({ "global_id": "CorridorWidth", "dimension_id": dim.0 }));
    assert!(y_of(&doc, a).abs() < 1e-9);
    assert!((y_of(&doc, b) - 1.8).abs() < 1e-6, "{}", y_of(&doc, b));
    // 2000 mm in one transaction; one undo restores the value and the wall.
    ok(&r, &mut doc, "global.set", json!({ "global_id": "CorridorWidth", "value": 2.0 }));
    assert!((y_of(&doc, b) - 2.0).abs() < 1e-6);
    assert!((value(&r, &mut doc, "CorridorWidth") - 2.0).abs() < 1e-12);
    ok(&r, &mut doc, "edit.undo", json!({ "steps": 1 }));
    assert!((y_of(&doc, b) - 1.8).abs() < 1e-6, "{}", y_of(&doc, b));
    assert!((value(&r, &mut doc, "CorridorWidth") - 1.8).abs() < 1e-12);
    // Moving the first wall drags the second (still locked).
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [a.0], "vector": [0.0, -0.5, 0.0] }));
    assert!((y_of(&doc, b) - 1.3).abs() < 1e-6, "{}", y_of(&doc, b));

    // A formula follows; units must agree; a cycle is refused.
    ok(&r, &mut doc, "global.create", json!({ "name": "BayWidth", "data_type": "length", "formula": "CorridorWidth * 2 + 300 mm" }));
    assert!((value(&r, &mut doc, "BayWidth") - 3.9).abs() < 1e-9);
    ok(&r, &mut doc, "global.set", json!({ "global_id": "CorridorWidth", "value": 2.1 }));
    assert!((value(&r, &mut doc, "BayWidth") - 4.5).abs() < 1e-9);
    let e = run(&r, &mut doc, "global.set", json!({ "global_id": "CorridorWidth", "formula": "BayWidth / 2" })).unwrap_err();
    assert_eq!(&*e.code, "formula.cycle", "{}", e.message);
    assert!(run(&r, &mut doc, "global.create", json!({ "name": "Bad", "data_type": "length", "formula": "CorridorWidth + 2" })).is_err(), "a length plus a number");
    assert!(run(&r, &mut doc, "global.create", json!({ "name": "corridorwidth", "data_type": "length", "value": 1.0 })).is_err(), "names are unique ignoring case");
    assert!(run(&r, &mut doc, "global.create", json!({ "name": "A<B", "data_type": "length", "value": 1.0 })).is_err());
    assert!(run(&r, &mut doc, "global.delete", json!({ "global_id": "CorridorWidth" })).is_err(), "BayWidth's formula uses it");
    assert!((ok(&r, &mut doc, "formula.validate", json!({ "formula": "BayWidth - 1 m" })).as_f64().unwrap() - 3.5).abs() < 1e-9);

    // 200 doors follow DoorSillHeight together.
    let host = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 20.0], "end": [320.0, 20.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let doors: Vec<ElementId> = (0..200).map(|k| id(&ok(&r, &mut doc, "door.place", json!({ "host_wall_id": host.0, "point_along": 1.0 + 1.5 * k as f64 })))).collect();
    ok(&r, &mut doc, "global.create", json!({ "name": "DoorSillHeight", "data_type": "length", "value": 0.02 }));
    ok(&r, &mut doc, "global.associate", json!({ "global_id": "DoorSillHeight", "element_ids": doors, "param_ref": "insert.sill" }));
    let sill = |doc: &Document, d: ElementId| reviera_core::inserts::resolve(doc.store(), doc.store().get(d).unwrap()).unwrap().recipe.sill;
    assert!(doors.iter().all(|d| (sill(&doc, *d) - 0.02).abs() < 1e-9));
    ok(&r, &mut doc, "global.set", json!({ "global_id": "DoorSillHeight", "value": 0.05 }));
    assert!(doors.iter().all(|d| (sill(&doc, *d) - 0.05).abs() < 1e-9));
    let driven: Vec<ElementId> = serde_json::from_value(ok(&r, &mut doc, "global.select_driven", json!({ "global_id": "DoorSillHeight" }))).unwrap();
    assert_eq!(driven.len(), 200);
    ok(&r, &mut doc, "global.dissociate", json!({ "element_ids": [doors[0].0], "param_ref": "insert.sill" }));
    ok(&r, &mut doc, "global.set", json!({ "global_id": "DoorSillHeight", "value": 0.0 }));
    assert!((sill(&doc, doors[0]) - 0.05).abs() < 1e-9 && sill(&doc, doors[1]).abs() < 1e-9);
    let rep = ok(&r, &mut doc, "global.delete", json!({ "global_id": "DoorSillHeight" }));
    assert_eq!(rep["uses"], json!(199));

    // A reporting global reads its dimension.
    let c = wall(&mut doc, 6.0);
    let yb = y_of(&doc, b);
    let d2 = id(&ok(&r, &mut doc, "dim.create_aligned", json!({ "view_id": plan.0, "refs": [axis(b, yb), axis(c, 6.0)], "line_point": [14.0, 0.0] })));
    ok(&r, &mut doc, "global.create", json!({ "name": "Gap", "data_type": "length", "reporting": true }));
    ok(&r, &mut doc, "global.label_dimension", json!({ "global_id": "Gap", "dimension_id": d2.0 }));
    let expect = 6.0 - y_of(&doc, b);
    assert!((value(&r, &mut doc, "Gap") - expect).abs() < 1e-6);
    assert!(run(&r, &mut doc, "global.set", json!({ "global_id": "Gap", "value": 1.0 })).is_err());
}
