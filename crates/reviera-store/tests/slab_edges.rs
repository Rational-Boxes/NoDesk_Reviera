// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Slab edges (ARC-SLED-001): a closed mitred run round a rectangular floor, its volume the
//! profile's area times the centroid path's length; an open run; flips; following the floor;
//! IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, slab_edges};
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

#[test]
fn slab_edges_round_a_floor() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("edges.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
    let edge = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
    let floor = id(&run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[edge(c[0], c[1]), edge(c[1], c[2]), edge(c[2], c[3]), edge(c[3], c[0])]], "level_id": l1.0 }),
    )
    .unwrap());
    // All round: a closed run, 0.3 × 0.45 outside the edge.
    let se = id(&run(
        &r,
        &mut doc,
        "slabedge.create",
        json!({ "host_id": floor.0 }),
    )
    .unwrap());
    let g = slab_edges::regenerate(doc.store(), doc.store().get(se).unwrap()).unwrap();
    let expected = 0.3 * 0.45 * 2.0 * (10.3 + 6.3);
    assert!((g.length - 33.2).abs() < 1e-9, "{}", g.length);
    assert!(
        (g.volume - expected).abs() / expected < 0.005,
        "{} vs {expected}",
        g.volume
    );
    // Two edges only: an open run with end caps.
    run(
        &r,
        &mut doc,
        "hostsweep.remove_segments",
        json!({ "id": se.0, "edge_refs": [3, 4] }),
    )
    .unwrap();
    let g = slab_edges::regenerate(doc.store(), doc.store().get(se).unwrap()).unwrap();
    assert!(g.volume > 0.0 && g.volume < expected * 0.6, "{}", g.volume);
    // Flipped into the slab, the profile is under it.
    run(
        &r,
        &mut doc,
        "hostsweep.flip",
        json!({ "id": se.0, "axis": "Horizontal" }),
    )
    .unwrap();
    let g = slab_edges::regenerate(doc.store(), doc.store().get(se).unwrap()).unwrap();
    assert!(
        g.mesh.0.iter().all(|p| p[1] >= -1e-9),
        "inside the slab, south edge"
    );
    // A run that is not a chain is refused.
    assert!(
        run(
            &r,
            &mut doc,
            "slabedge.set",
            json!({ "slab_edge_id": se.0, "keys": [1, 3] })
        )
        .is_err()
    );
    // The floor raised, the edge follows.
    run(
        &r,
        &mut doc,
        "floor.set_level",
        json!({ "floor_ids": [floor.0], "offset": 0.5 }),
    )
    .ok();
    // Saved as an IfcSlab SLABEDGE with a B-rep.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(
        rows.iter()
            .any(|r| r.class == "IFCSLAB" && r.attributes.contains("'SLABEDGE'"))
    );
    // Deleting the floor deletes it.
    let gone = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [floor.0] }),
    )
    .unwrap();
    assert!(gone.as_array().unwrap().iter().any(|v| v == &json!(se.0)));
}

#[test]
fn a_drain_point_warps_a_floor_and_reset_restores_it() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("shape.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let edge = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
    let floor = id(&run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[edge(c[0], c[1]), edge(c[1], c[2]), edge(c[2], c[3]), edge(c[3], c[0])]], "level_id": l1.0 }),
    )
    .unwrap());
    let body = |doc: &Document| {
        reviera_core::bodies::all(doc.store())
            .into_iter()
            .find(|b| b.element == floor)
            .unwrap()
    };
    let flat = body(&doc).bounds().unwrap();
    // A drain 50 mm down in the middle: four facets falling to it.
    let key = run(
        &r,
        &mut doc,
        "shape.add_point",
        json!({ "element_id": floor.0, "xy": [5.0, 5.0], "mode": { "along_surface": { "offset": -0.05 } } }),
    )
    .unwrap();
    assert_eq!(key, "p1");
    let warped = body(&doc).bounds().unwrap();
    assert!(
        (warped.0[2] - (flat.0[2] - 0.05)).abs() < 0.01,
        "{warped:?} vs {flat:?}"
    );
    // Saved as a B-rep.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCFACETEDBREP"));
    // Moved further, then reset: flat again, as before.
    run(
        &r,
        &mut doc,
        "shape.move",
        json!({ "element_id": floor.0, "keys": ["p1"], "dz": -0.05 }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "shape.reset",
        json!({ "element_id": floor.0 }),
    )
    .unwrap();
    assert_eq!(body(&doc).bounds().unwrap(), flat);
    // A point outside is refused.
    assert!(run(
        &r,
        &mut doc,
        "shape.add_point",
        json!({ "element_id": floor.0, "xy": [15.0, 5.0], "mode": { "absolute": { "z": 0.0 } } }),
    )
    .is_err());
}
