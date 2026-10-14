// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architectural columns (ARC-COL-001): placement between levels, sections, attaching tops to a
//! sloped roof, the modify tools, and IFC.

use reviera_core::columns::{self, Justification};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
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

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn geometry(doc: &Document, c: ElementId) -> columns::ColumnGeometry {
    columns::regenerate(doc.store(), doc.store().get(c).unwrap()).unwrap()
}

#[test]
fn columns_between_levels_attached_to_a_roof_and_in_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let c = id(run(
        &r,
        &mut doc,
        "column.place_architectural",
        json!({ "point": [2.0, 2.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap());
    let g = geometry(&doc, c);
    assert_eq!((g.z0, g.top()), (0.0, 3.0));
    assert!((g.ring[0][0] - 1.8).abs() < 1e-9);
    // It follows its top level.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 3.5 }),
    )
    .unwrap();
    assert_eq!(geometry(&doc, c).top(), 3.5);

    // A round type: duplicate the default and change its section.
    let t = doc
        .store()
        .iter()
        .find(|e| columns::column_type(e).is_some())
        .unwrap()
        .id();
    let round = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": t.0, "new_name": "Round Column 500" }),
    )
    .unwrap());
    for (param, value) in [("section", json!("round")), ("diameter", json!("500 mm"))] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": round.0, "param": param, "value": value }),
        )
        .unwrap();
    }
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [c.0], "type_id": round.0 }),
    )
    .unwrap();
    let g = geometry(&doc, c);
    assert_eq!(g.ring.len(), 32);
    assert!((g.ring[0][0] - 2.25).abs() < 1e-9);

    // Under a hip roof at 6 m, unconnected 10 m high, attached: the maximum reaches the highest
    // point of the underside over it, the minimum the lowest.
    let square = |x0: f64, y0: f64, x1: f64, y1: f64| {
        json!([
            { "line": { "start": [x0, y0], "end": [x1, y0] } },
            { "line": { "start": [x1, y0], "end": [x1, y1] } },
            { "line": { "start": [x1, y1], "end": [x0, y1] } },
            { "line": { "start": [x0, y1], "end": [x0, y0] } },
        ])
    };
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(0.0, 0.0, 10.0, 10.0), "level_id": l1.0, "base_offset": 6.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "column.set",
        json!({ "column_ids": [c.0], "top_constraint": { "unconnected": 10.0 } }),
    )
    .unwrap();
    // Attach Top in the window attaches to what is directly above.
    assert_eq!(columns::target_above(doc.store(), c), Some(roof));
    run(
        &r,
        &mut doc,
        "column.attach",
        json!({ "column_ids": [c.0], "target_id": roof.0 }),
    )
    .unwrap();
    let rg = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    let g = geometry(&doc, c);
    let over: Vec<f64> = g.ring.iter().map(|p| rg.underside(*p).unwrap()).collect();
    let hi = over.iter().copied().fold(f64::MIN, f64::max);
    let lo = over.iter().copied().fold(f64::MAX, f64::min);
    assert!(hi > lo + 0.05, "{lo} {hi}");
    // Cut by the underside: each corner of the top is on it.
    for (t, u) in g.tops.iter().zip(&over) {
        assert!((t - u).abs() < 1e-6, "{t} {u}");
    }
    assert!(g.top() <= hi + 1e-6);
    run(
        &r,
        &mut doc,
        "column.attach",
        json!({ "column_ids": [c.0], "target_id": roof.0, "justification": "minimum" }),
    )
    .unwrap();
    let g = geometry(&doc, c);
    assert!(
        g.tops.iter().all(|t| (t - lo).abs() < 0.05),
        "{:?} {lo}",
        g.tops
    );
    assert_eq!(
        columns::column_recipe(doc.store().get(c).unwrap())
            .unwrap()
            .attach_top
            .unwrap()
            .justification,
        Justification::Minimum
    );

    // Moved, rotated and copied.
    run(
        &r,
        &mut doc,
        "column.detach",
        json!({ "column_ids": [c.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.rotate",
        json!({ "element_ids": [c.0], "center": [0.0, 0.0], "angle": "90°" }),
    )
    .unwrap();
    let rc = columns::column_recipe(doc.store().get(c).unwrap()).unwrap();
    assert!((rc.point[0] + 2.0).abs() < 1e-9 && (rc.point[1] - 3.0).abs() < 1e-9);
    assert!((rc.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    let copies = run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [c.0], "vectors": [[0.0, 2.0, 0.0], [0.0, 4.0, 0.0]] }),
    )
    .unwrap();
    assert_eq!(copies.as_array().unwrap().len(), 2);
    assert_eq!(columns::all(doc.store()).len(), 3);

    // IFC: three IfcColumns of two IfcColumnTypes, with bodies; reopened, still columns.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCCOLUMN"), 3);
    assert_eq!(count("IFCCOLUMNTYPE"), 2);
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(columns::all(doc2.store()).len(), 3);
    assert_eq!(
        reviera_core::categories::category_of(doc2.store().get(c).unwrap()).map(|c| c.name),
        Some("Columns")
    );

    // Deleting Level 2, which they no longer use (their tops are unconnected), leaves them.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [l2.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert_eq!(columns::all(doc.store()).len(), 3);
}
