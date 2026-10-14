// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Arrays (ARC-MOD-026): linear and radial arrays of independent copies.

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

fn points(doc: &Document) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = doc
        .store()
        .iter()
        .filter_map(reviera_core::columns::column_recipe)
        .map(|r| r.point)
        .collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p
}

#[test]
fn linear_and_radial_arrays() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("a.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let place = |r: &CommandRegistry, doc: &mut Document, p: [f64; 2]| {
        ElementId(
            run(
                r,
                doc,
                "column.place_architectural",
                json!({ "point": p, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
            )
            .unwrap()
            .as_u64()
            .unwrap(),
        )
    };
    // Five members at 0.8 m (Move To: 2nd), one undo step.
    let c = place(&r, &mut doc, [0.0, 0.0]);
    let out = run(
        &r,
        &mut doc,
        "element.array_linear",
        json!({ "element_ids": [c.0], "vector": [0.8, 0.0, 0.0], "count": 5 }),
    )
    .unwrap();
    assert_eq!(out.as_array().unwrap().len(), 4);
    let xs: Vec<f64> = points(&doc).iter().map(|p| p[0]).collect();
    for (k, x) in xs.iter().enumerate() {
        assert!((x - 0.8 * k as f64).abs() < 1e-9, "{xs:?}");
    }
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert_eq!(points(&doc).len(), 1);
    // Move To: Last spreads them over the vector.
    run(
        &r,
        &mut doc,
        "element.array_linear",
        json!({ "element_ids": [c.0], "vector": [4.0, 0.0, 0.0], "count": 5, "move_to": "last" }),
    )
    .unwrap();
    assert!((points(&doc)[4][0] - 4.0).abs() < 1e-9);
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();

    // Eight round 360° (Move To: Last): 45° apart, the first not doubled.
    run(
        &r,
        &mut doc,
        "element.array_radial",
        json!({ "element_ids": [c.0], "center": [-5.0, 0.0], "angle": "360°", "count": 8, "move_to": "last" }),
    )
    .unwrap();
    let pts = points(&doc);
    assert_eq!(pts.len(), 8);
    for (i, a) in pts.iter().enumerate() {
        for b in &pts[i + 1..] {
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) > 1.0, "{pts:?}");
        }
        assert!(((a[0] + 5.0).hypot(a[1]) - 5.0).abs() < 1e-9);
    }
    // Group and Associate is refused until groups exist.
    let e = run(
        &r,
        &mut doc,
        "element.array_linear",
        json!({ "element_ids": [c.0], "vector": [1, 0, 0], "count": 3, "group_and_associate": true }),
    )
    .unwrap_err();
    assert_eq!(e.code, "array.group_not_supported");
}
