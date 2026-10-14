// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Railings (ARC-RAIL-001/002): along a sketched path, and guarding the open edges of a deck
//! (a floor) and a flat roof, following their outlines.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, railings};
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

fn ids(v: Value) -> Vec<ElementId> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| id(x.clone()))
        .collect()
}

fn geometry(doc: &Document, id: ElementId) -> railings::RailingGeometry {
    railings::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array(
        (0..4)
            .map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } }))
            .collect(),
    )
}

#[test]
fn railings_guard_decks_and_flat_roofs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("railings.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A free railing: the specified 3.05 m path has 24 balusters and two posts.
    let free = id(run(
        &r,
        &mut doc,
        "railing.create",
        json!({ "path": [[20.0, 0.0], [23.05, 0.0]], "base_level": level.0 }),
    )
    .unwrap());
    let g = geometry(&doc, free);
    assert_eq!((g.posts, g.balusters), (2, 24));
    assert_eq!(
        doc.store()
            .get(free)
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "GUARDRAIL"
    );

    // A 4 × 3 m deck (a floor 0.6 m up) against a house wall along its north edge.
    run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0.0, 3.1], "end": [4.0, 3.1] } },
            "base_level": level.0, "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap();
    let deck = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [rect([0.0, 0.0], [4.0, 3.0])], "level_id": level.0, "offset": 0.6 }),
    )
    .unwrap());
    // Guard all open edges: the three away from the house, in one chain.
    let made = ids(run(
        &r,
        &mut doc,
        "railing.create_on_edges",
        json!({ "host_id": deck.0 }),
    )
    .unwrap());
    assert_eq!(made.len(), 1);
    let g = geometry(&doc, made[0]);
    assert!(!g.closed);
    assert_eq!(g.path.len(), 4);
    // Set in 50 mm, on the deck's top.
    assert!(
        (g.length - (2.95 + 3.9 + 2.95)).abs() < 1e-9,
        "{}",
        g.length
    );
    assert!((g.z - 0.6).abs() < 1e-9);
    assert_eq!(g.posts, 4);
    // The deck's outline moves; the railing follows. Curve 2 is the east edge (x = 4).
    run(
        &r,
        &mut doc,
        "sketch.move_curve",
        json!({ "element_id": deck.0, "key": 2, "vector": [1.0, 0.0] }),
    )
    .unwrap();
    let g = geometry(&doc, made[0]);
    assert!(
        (g.length - (2.95 + 4.9 + 2.95)).abs() < 1e-9,
        "{}",
        g.length
    );

    // A flat roof's perimeter: one closed railing, a post at every corner.
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": rect([10.0, 0.0], [16.0, 5.0]).as_array().unwrap().iter().map(|c| {
                let mut c = c.clone();
                c["defines_slope"] = json!(false);
                c
            }).collect::<Vec<_>>(),
            "level_id": level.0, "base_offset": 3.0,
        }),
    )
    .unwrap());
    let made = ids(run(
        &r,
        &mut doc,
        "railing.create_on_edges",
        json!({ "host_id": roof.0 }),
    )
    .unwrap());
    let g = geometry(&doc, made[0]);
    assert!(g.closed);
    assert_eq!(g.posts, 4);
    assert!((g.z - 3.3).abs() < 1e-9, "{}", g.z);
    // A sloped roof is refused.
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": roof.0, "shape": "hip" }),
    )
    .unwrap();
    let err = run(
        &r,
        &mut doc,
        "railing.create_on_edges",
        json!({ "host_id": roof.0 }),
    )
    .unwrap_err();
    assert_eq!(err.code, "railing.sloped_host");

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcRailing").unwrap().len() >= 2);
    assert_eq!(ifc.entities_of_type("IfcRailingType").unwrap().len(), 1);

    // Deleting the deck deletes its railing.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [deck.0] }),
    )
    .unwrap();
    assert!(
        doc.store()
            .iter()
            .filter(|e| railings::railing_recipe(e).is_some())
            .all(|e| e.id() == free || e.id() == made[0])
    );
}

/// ARC-RAIL-003: openings and gates added to a laid-out railing.
#[test]
fn openings_and_gates_in_a_railing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gates.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let rail = id(run(
        &r,
        &mut doc,
        "railing.create",
        json!({ "path": [[0.0, 0.0], [6.0, 0.0], [6.0, 3.0]], "base_level": level.0 }),
    )
    .unwrap());
    let before = geometry(&doc, rail);
    assert_eq!(before.posts, 3);

    // A 900 mm opening at 1.5 m: a post each side, nothing between them.
    let key = run(
        &r,
        &mut doc,
        "railing.add_opening",
        json!({ "railing_id": rail.0, "at": 1.5 }),
    )
    .unwrap();
    assert_eq!(key, json!(1));
    let g = geometry(&doc, rail);
    assert_eq!(g.posts, 5);
    let in_gap = |g: &railings::RailingGeometry, x0: f64, x1: f64| {
        g.parts.iter().any(|(pts, _)| {
            let c = pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64;
            let y = pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64;
            y.abs() < 0.1 && c > x0 && c < x1
        })
    };
    // Post faces at 1.05 and 1.95: no rail or baluster between.
    assert!(!in_gap(&g, 1.06, 1.94));
    assert!(in_gap(&before, 1.06, 1.94));

    // A double gate of 1.2 m by a point near the path: two leaves of about 585 mm.
    let gate = id(run(
        &r,
        &mut doc,
        "railing.add_gate",
        json!({ "railing_id": rail.0, "point": [4.0, 0.3], "leaves": 2 }),
    )
    .unwrap());
    let e = doc.store().get(gate).unwrap();
    assert_eq!(e.class, "IfcDoor");
    assert_eq!(e.param("@PredefinedType").unwrap(), "GATE");
    assert_eq!(
        railings::gate_operation(doc.store(), e).unwrap().0,
        "DOUBLE_DOOR_SINGLE_SWING"
    );
    let g = geometry(&doc, rail);
    assert_eq!(g.posts, 7);
    let parts = railings::gate_geometry(doc.store(), doc.store().get(gate).unwrap()).unwrap();
    let stiles: Vec<f64> = parts
        .iter()
        .map(|(pts, _)| pts.iter().map(|p| p[0]).fold(f64::MIN, f64::max))
        .collect();
    let (lo, hi) = (
        parts
            .iter()
            .flat_map(|(p, _)| p.iter().map(|q| q[0]))
            .fold(f64::MAX, f64::min),
        stiles.iter().copied().fold(f64::MIN, f64::max),
    );
    assert!(
        (lo - 3.41).abs() < 1e-9 && (hi - 4.59).abs() < 1e-9,
        "{lo} {hi}"
    );
    let features: Vec<railings::RailFeature> = serde_json::from_value(
        run(
            &r,
            &mut doc,
            "railing.features",
            json!({ "railing_id": rail.0 }),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(features.len(), 2);
    assert!((features[1].at - 4.0).abs() < 1e-9 && (features[1].width - 1.2).abs() < 1e-9);

    // Refused: across the corner, and overlapping the gate.
    let err = run(
        &r,
        &mut doc,
        "railing.add_opening",
        json!({ "railing_id": rail.0, "at": 6.0 }),
    )
    .unwrap_err();
    assert_eq!(err.code, "railing.feature_off_segment");
    let err = run(
        &r,
        &mut doc,
        "railing.move_feature",
        json!({ "railing_id": rail.0, "key": 1, "at": 3.2 }),
    )
    .unwrap_err();
    assert_eq!(err.code, "railing.feature_overlap");
    // On the second run, by distance: 6 m + 1.5 m.
    run(
        &r,
        &mut doc,
        "railing.move_feature",
        json!({ "railing_id": rail.0, "key": 1, "at": 7.5 }),
    )
    .unwrap();
    let g = geometry(&doc, rail);
    assert!(in_gap(&g, 1.06, 1.94));
    assert!(!g.parts.iter().any(|(pts, _)| {
        let x = pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64;
        let y = pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64;
        (x - 6.0).abs() < 0.1 && y > 1.06 && y < 1.94
    }));

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let doors = ifc.entities_of_type("IfcDoor").unwrap();
    assert_eq!(doors.len(), 1);
    assert_eq!(
        ifc.attribute(doors[0].id, "OperationType").unwrap(),
        reviera_ifc::AttrValue::Enum("DOUBLE_DOOR_SINGLE_SWING".into())
    );

    // A path change keeps each feature's distance; one that no longer fits is left out until
    // the path makes room again.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [rail.0], "vector": [0.0, 1.0, 0.0] }),
    )
    .unwrap();
    let f: Vec<railings::RailFeature> = serde_json::from_value(
        run(
            &r,
            &mut doc,
            "railing.features",
            json!({ "railing_id": rail.0 }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!((f[1].at - 4.0).abs() < 1e-9);
    let mut short = doc.store().get(rail).unwrap().clone();
    let mut recipe = railings::railing_recipe(&short).unwrap();
    if let railings::RailPath::Free { points, .. } = &mut recipe.path {
        *points = vec![[0.0, 1.0], [3.0, 1.0]];
    }
    short
        .params
        .insert("rxd:recipe".into(), serde_json::to_value(&recipe).unwrap());
    let g = railings::regenerate(doc.store(), &short).unwrap();
    assert_eq!(g.posts, 2, "the features no longer fit and are left out");

    // Removing the gate removes its element; deleting the railing would delete it too.
    run(
        &r,
        &mut doc,
        "railing.remove_feature",
        json!({ "railing_id": rail.0, "key": 2 }),
    )
    .unwrap();
    assert!(doc.store().get(gate).is_none());
    let gate = id(run(
        &r,
        &mut doc,
        "railing.add_gate",
        json!({ "railing_id": rail.0, "at": 4.0 }),
    )
    .unwrap());
    assert_eq!(
        railings::gate_operation(doc.store(), doc.store().get(gate).unwrap())
            .unwrap()
            .0,
        "SINGLE_SWING_LEFT"
    );
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [rail.0] }),
    )
    .unwrap();
    assert!(doc.store().get(gate).is_none());
}
