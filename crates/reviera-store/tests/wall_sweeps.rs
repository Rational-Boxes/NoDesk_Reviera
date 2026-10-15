// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall sweeps and reveals (ARC-WALL-010).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

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

fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| m.0[t[k] as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn a_cornice_runs_round_and_a_reveal_cuts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A closed rectangle drawn clockwise, so the exterior (left) faces out.
    let corners = [[0.0, 0.0], [0.0, 6.0], [10.0, 6.0], [10.0, 0.0]];
    let mut walls = Vec::new();
    for k in 0..4 {
        walls.push(id(&run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "curve": { "line": { "start": corners[k], "end": corners[(k + 1) % 4] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
        )
        .unwrap()));
    }
    // A 50 × 100 mm cornice placed on one wall goes round all four, mitred.
    let s = id(&run(
        &r,
        &mut doc,
        "wallsweep.create",
        json!({ "wall_ids": [walls[0].0], "offset_from_level": 2.8, "width": 0.05, "height": 0.1 }),
    )
    .unwrap());
    let e = doc.store().get(s).unwrap();
    assert_eq!(reviera_core::wall_sweeps::recipe(e).unwrap().walls.len(), 4);
    let meshes = reviera_core::wall_sweeps::regenerate(doc.store(), e).unwrap();
    assert_eq!(meshes.len(), 1, "one closed run");
    // The faces are 0.1 m out from the axes: the profile's centroid runs round a 10.25 × 6.25 m
    // rectangle.
    let v = volume(&meshes[0]).abs();
    let expected = 0.05 * 0.1 * 2.0 * (10.25 + 6.25);
    assert!((v - expected).abs() < 1e-6, "{v} vs {expected}");
    let b = meshes[0].0.iter().fold([f64::MAX, f64::MIN], |a, p| {
        [a[0].min(p[0]), a[1].max(p[0])]
    });
    assert!(
        (b[0] + 0.15).abs() < 1e-9 && (b[1] - 10.15).abs() < 1e-9,
        "{b:?}"
    );

    // A 20 mm reveal on one wall only: it cuts that wall (a boolean subtraction, hiding the tool).
    let rv = id(&run(
        &r,
        &mut doc,
        "wallreveal.create",
        json!({ "wall_ids": [walls[1].0], "offset_from_level": 1.0, "chain": false }),
    )
    .unwrap());
    let rel = reviera_core::csg::resolved(doc.store());
    assert_eq!(rel.len(), 1);
    assert_eq!((rel[0].0, rel[0].1, rel[0].3), (walls[1], rv, true));
    let mut bodies = reviera_core::bodies::all(doc.store());
    let wall_before: f64 = bodies
        .iter()
        .find(|b| b.element == walls[1])
        .unwrap()
        .parts
        .iter()
        .map(|p| volume(&p.mesh))
        .sum();
    reviera_geom::csg::apply(&Default::default(), doc.store(), &mut bodies);
    assert!(
        bodies.iter().all(|b| b.element != rv),
        "the reveal itself is not drawn"
    );
    let wall_after: f64 = bodies
        .iter()
        .find(|b| b.element == walls[1])
        .unwrap()
        .parts
        .iter()
        .map(|p| volume(&p.mesh))
        .sum();
    // 20 mm deep, 20 mm high, along the 10 m wall's face (its mitred ends make it 10.2 m).
    let groove = wall_before - wall_after;
    assert!((groove - 0.02 * 0.02 * 10.2).abs() < 2e-4, "{groove}");
    // Removing its only wall deletes it, and the boolean goes with it.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".RECESS.") && text.contains(".MOLDING."));
    run(
        &r,
        &mut doc,
        "wallsweep.remove_walls",
        json!({ "sweep_id": rv.0, "wall_ids": [walls[1].0] }),
    )
    .unwrap();
    assert!(doc.store().get(rv).is_none());
    assert!(reviera_core::csg::resolved(doc.store()).is_empty());
    project.save(&doc).unwrap();
}
