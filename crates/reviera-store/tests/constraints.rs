// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Constraints (ARC-CNST-001/002): EQ keeps three walls evenly spaced, a locked dimension drags
//! the other wall along, an alignment lock to a grid, an unsatisfiable edit fails and changes
//! nothing, constraints survive their dimension and go with their elements, and they are saved.

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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

/// A wall along y at `x`, from y = 0 to 6.
fn wall_at(r: &CommandRegistry, doc: &mut Document, level: ElementId, x: f64) -> ElementId {
    id(&run(
        r,
        doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [x, 0.0], "end": [x, 6.0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap())
}

fn x_of(doc: &Document, wall: ElementId) -> f64 {
    reviera_core::walls::wall_recipe(doc.store().get(wall).unwrap())
        .unwrap()
        .curve
        .start[0]
}

fn axis(wall: ElementId, x: f64) -> Value {
    json!({ "element": wall.0, "sub": "axis", "point": [x, 3.0] })
}

#[test]
fn eq_locks_and_alignments() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("constraints.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let plan = id(&run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l1.0] }),
    )
    .unwrap()[0]);
    let (a, b, c) = (
        wall_at(&r, &mut doc, l1, 0.0),
        wall_at(&r, &mut doc, l1, 3.0),
        wall_at(&r, &mut doc, l1, 7.0),
    );

    // EQ across the three: the middle wall goes to 3.5 and stays midway when the outer moves.
    let dim = id(&run(
        &r,
        &mut doc,
        "dim.create_aligned",
        json!({ "view_id": plan.0, "refs": [axis(a, 0.0), axis(b, 3.0), axis(c, 7.0)], "line_point": [0.0, 8.0] }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "constraint.set_equality",
        json!({ "dimension_id": dim.0, "on": true }),
    )
    .unwrap();
    assert!((x_of(&doc, b) - 3.5).abs() < 1e-6, "{}", x_of(&doc, b));
    assert!(x_of(&doc, a).abs() < 1e-9 && (x_of(&doc, c) - 7.0).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c.0], "vector": [2.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert!((x_of(&doc, b) - 4.5).abs() < 1e-6, "{}", x_of(&doc, b));
    run(
        &r,
        &mut doc,
        "constraint.set_equality",
        json!({ "dimension_id": dim.0, "on": false }),
    )
    .unwrap();

    // A locked 3000 between a and a new wall d at x = -3: moving a drags d.
    let d = wall_at(&r, &mut doc, l1, -3.0);
    let lock = id(&run(
        &r,
        &mut doc,
        "dim.create_aligned",
        json!({ "view_id": plan.0, "refs": [axis(d, -3.0), axis(a, 0.0)], "line_point": [0.0, 9.0] }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "constraint.lock_dimension",
        json!({ "dimension_id": lock.0 }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [a.0], "vector": [-0.5, 0.0, 0.0] }),
    )
    .unwrap();
    assert!((x_of(&doc, a) + 0.5).abs() < 1e-9);
    assert!((x_of(&doc, d) + 3.5).abs() < 1e-6, "{}", x_of(&doc, d));

    // Pinned, d cannot follow: the move fails and nothing changes.
    run(&r, &mut doc, "element.pin", json!({ "element_ids": [d.0] })).unwrap();
    let err = run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [a.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap_err();
    assert_eq!(err.code, "constraint.unsatisfied", "{err:?}");
    assert!((x_of(&doc, a) + 0.5).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "element.unpin",
        json!({ "element_ids": [d.0] }),
    )
    .unwrap();

    // An alignment lock between a grid and wall b: moving the grid moves the wall.
    let bx = x_of(&doc, b);
    let g = id(&run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [bx, -2.0], "end": [bx, 8.0] } } }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "constraint.lock_alignment",
        json!({ "ref_a": { "element": g.0, "sub": "axis", "point": [bx, 0.0] }, "ref_b": axis(b, bx) }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [g.0], "vector": [0.25, 0.0, 0.0] }),
    )
    .unwrap();
    assert!((x_of(&doc, b) - bx - 0.25).abs() < 1e-6);

    // Deleting the locked dimension keeps its constraint; deleting wall d removes it.
    let before = run(&r, &mut doc, "constraint.list", json!({})).unwrap();
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [lock.0] }),
    )
    .unwrap();
    let after = run(&r, &mut doc, "constraint.list", json!({})).unwrap();
    assert_eq!(
        before.as_array().unwrap().len(),
        after.as_array().unwrap().len()
    );
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [d.0] }),
    )
    .unwrap();
    let left = run(&r, &mut doc, "constraint.list", json!({})).unwrap();
    assert_eq!(
        left.as_array().unwrap().len(),
        after.as_array().unwrap().len() - 1
    );

    // Saved and reopened, the alignment still holds and still drags.
    project.save(&doc).unwrap();
    drop(project);
    let (_p2, mut doc2, _) = LocalProject::open(&path).unwrap();
    let listed = run(&r, &mut doc2, "constraint.list", json!({})).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["kind"], "Alignment");
    assert_eq!(listed[0]["satisfied"], true);
}

#[test]
fn temporary_dimensions_drive_walls_and_doors() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("t.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let plan = id(&run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l1.0] }),
    )
    .unwrap()[0]);
    let (a, b, c) = (
        wall_at(&r, &mut doc, l1, 0.0),
        wall_at(&r, &mut doc, l1, 3.0),
        wall_at(&r, &mut doc, l1, 7.0),
    );
    let _ = (a, c);
    // The middle wall measures 3 to the left and 4 to the right.
    let dims = run(
        &r,
        &mut doc,
        "dim.temporary_query",
        json!({ "element_id": b.0 }),
    )
    .unwrap();
    let mut values: Vec<f64> = dims
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["value"].as_f64().unwrap())
        .collect();
    values.sort_by(f64::total_cmp);
    assert_eq!(values, vec![3.0, 4.0]);
    let left = dims
        .as_array()
        .unwrap()
        .iter()
        .position(|d| (d["value"].as_f64().unwrap() - 3.0).abs() < 1e-9)
        .unwrap();
    // Typed 3500: the wall moves to x = 3.5, the other wall stays.
    run(
        &r,
        &mut doc,
        "dim.temporary_set",
        json!({ "element_id": b.0, "index": left, "value": 3.5 }),
    )
    .unwrap();
    assert!((x_of(&doc, b) - 3.5).abs() < 1e-9);
    assert!(x_of(&doc, a).abs() < 1e-9);

    // A door in a wall along x from (0, 6) to (7, 6), at 2 m: 2 to wall a, 1.5 to wall b.
    let host = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 6.0], "end": [7.0, 6.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let door = id(&run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": host.0, "point_along": 2.0 }),
    )
    .unwrap());
    let dims = run(
        &r,
        &mut doc,
        "dim.temporary_query",
        json!({ "element_id": door.0 }),
    )
    .unwrap();
    let mut values: Vec<f64> = dims
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["value"].as_f64().unwrap())
        .collect();
    values.sort_by(f64::total_cmp);
    assert!(
        (values[0] - 1.5).abs() < 1e-9 && (values[1] - 2.0).abs() < 1e-9,
        "{values:?}"
    );
    let to_a = dims
        .as_array()
        .unwrap()
        .iter()
        .position(|d| d["other"] == a.0)
        .unwrap();
    run(
        &r,
        &mut doc,
        "dim.temporary_set",
        json!({ "element_id": door.0, "index": to_a, "value": 1.2 }),
    )
    .unwrap();
    let along = reviera_core::inserts::insert_recipe(doc.store().get(door).unwrap())
        .unwrap()
        .along;
    assert!((along - 1.2).abs() < 1e-9, "{along}");
    // Made permanent: an aligned dimension of 1.2 in the plan.
    let dim = id(&run(
        &r,
        &mut doc,
        "dim.make_permanent",
        json!({ "view_id": plan.0, "element_id": door.0, "index": to_a }),
    )
    .unwrap());
    let listed = run(&r, &mut doc, "anno.list", json!({ "view_id": plan.0 })).unwrap();
    let d = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == dim.0)
        .unwrap();
    assert!((d["measured"]["values"][0].as_f64().unwrap() - 1.2).abs() < 1e-9);
}
