// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Array along a path (ARC-MOD-033): a temple colonnade round a stylobate's edges with a column at
//! every corner, a hedge along a spline with seeded variation, staying parametric, and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, path_arrays};
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

/// A box mesh (metres) as an OBJ file.
fn write_box(path: &std::path::Path, w: f64, d: f64, h: f64) {
    let mut obj = String::new();
    for z in [0.0, h] {
        for (x, y) in [(0.0, 0.0), (w, 0.0), (w, d), (0.0, d)] {
            obj.push_str(&format!("v {x} {y} {z}\n"));
        }
    }
    obj.push_str("f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n");
    std::fs::write(path, obj).unwrap();
}

fn members(r: &CommandRegistry, doc: &mut Document, a: ElementId) -> Vec<Value> {
    run(r, doc, "array.members", json!({ "array_id": a.0 }))
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn temple_colonnade_round_the_stylobate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("temple.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    write_box(&dir.path().join("doric.obj"), 1.0, 1.0, 6.0);
    write_box(&dir.path().join("ionic.obj"), 0.8, 0.8, 7.0);
    let doric = id(run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": dir.path().join("doric.obj"), "category": "column" }),
    )
    .unwrap());
    let ionic = id(run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": dir.path().join("ionic.obj"), "category": "column" }),
    )
    .unwrap());
    // The stylobate: a 30 × 14 m floor whose top is 0.6 m up.
    let edge = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
    let stylobate = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[edge([0.0, 0.0], [30.0, 0.0]), edge([30.0, 0.0], [30.0, 14.0]), edge([30.0, 14.0], [0.0, 14.0]), edge([0.0, 14.0], [0.0, 0.0])]], "level_id": l1.0, "offset": 0.6 }),
    )
    .unwrap());

    // 13 columns along each long side and 6 along each short side, corners shared: 34.
    let a = id(run(
        &r,
        &mut doc,
        "array.along_path",
        json!({
            "sources": [{ "type_id": doric.0 }],
            "path": { "host_edges": stylobate.0 },
            "level": l1.0,
            "distribution": { "mode": "number", "value": 13, "counts": [13, 6] },
            "corners": "every_corner",
            "orientation": "outward",
            "name": "Peristyle"
        }),
    )
    .unwrap());
    let m = members(&r, &mut doc, a);
    assert_eq!(m.len(), 34);
    let at = |x: f64, y: f64| {
        m.iter()
            .find(|v| {
                (v["point"][0].as_f64().unwrap() - x).abs() < 1e-6
                    && (v["point"][1].as_f64().unwrap() - y).abs() < 1e-6
            })
            .cloned()
    };
    for corner in [(0.0, 0.0), (30.0, 0.0), (30.0, 14.0), (0.0, 14.0)] {
        assert!(at(corner.0, corner.1).is_some(), "no column at {corner:?}");
    }
    // On the stylobate's top; the front row (y = 0) faces outward, to -y; spaced 2.5 m.
    let front = at(2.5, 0.0).expect("a column 2.5 m along the front");
    assert!((front["z"].as_f64().unwrap() - 0.6).abs() < 1e-9);
    assert!(
        (front["rotation"].as_f64().unwrap() + std::f64::consts::FRAC_PI_2).abs() < 1e-6,
        "{front}"
    );
    // The array's body is its columns.
    let bodies = reviera_core::bodies::all(doc.store());
    let body = bodies.iter().find(|b| b.element == a).unwrap();
    assert_eq!(body.parts.len(), 34);
    let (lo, hi) = body.bounds().unwrap();
    assert!((lo[2] - 0.6).abs() < 1e-6 && (hi[2] - 6.6).abs() < 1e-6);

    // Parametric: the long sides get 15; an entrance gap; Ionic columns instead; all one step each.
    run(&r, &mut doc, "array.set", json!({ "array_id": a.0, "distribution": { "mode": "number", "value": 15, "counts": [15, 6] } })).unwrap();
    assert_eq!(members(&r, &mut doc, a).len(), 38);
    let mid = members(&r, &mut doc, a)
        .into_iter()
        .find(|v| {
            (v["point"][0].as_f64().unwrap() - 15.0).abs() < 1e-6
                && v["point"][1].as_f64().unwrap().abs() < 1e-6
        })
        .unwrap();
    run(
        &r,
        &mut doc,
        "array.exclude_member",
        json!({ "array_id": a.0, "index": mid["index"] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "array.set",
        json!({ "array_id": a.0, "sources": [{ "type_id": ionic.0 }] }),
    )
    .unwrap();
    let m = members(&r, &mut doc, a);
    assert_eq!(m.len(), 37);
    assert!(
        m.iter()
            .all(|v| (v["point"][0].as_f64().unwrap() - 15.0).abs() > 1e-6
                || v["point"][1].as_f64().unwrap().abs() > 1e-6)
    );
    // Raising the level raises the stylobate and its columns.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l1.0, "elevation": 1.0 }),
    )
    .unwrap();
    assert!((members(&r, &mut doc, a)[0]["z"].as_f64().unwrap() - 1.6).abs() < 1e-9);
    // Undo the elevation change.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l1.0, "elevation": 0.0 }),
    )
    .unwrap();

    // IFC: one Ionic map, 37 IfcColumn members aggregated by the array (an IfcElementAssembly).
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCELEMENTASSEMBLY"), 1);
    assert_eq!(count("IFCCOLUMN"), 37);
    assert_eq!(count("IFCMAPPEDITEM"), 37);
    let meshes = IfcModel::open(&path).unwrap().tessellate(1).unwrap();
    let cols: Vec<_> = meshes.iter().filter(|m| m.class == "IfcColumn").collect();
    assert_eq!(cols.len(), 37);
    let zmax = cols
        .iter()
        .flat_map(|m| m.verts.chunks(3).map(|v| v[2]))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((zmax - 7.6).abs() < 1e-3, "{zmax}");
    // Saved again unchanged, the members are not rewritten.
    let ids = |rows: &[reviera_ifc::StepRow]| {
        rows.iter()
            .filter(|r| r.class == "IFCCOLUMN")
            .map(|r| r.id.0)
            .collect::<Vec<_>>()
    };
    run(&r, &mut doc, "level.create", json!({ "elevation": 8.0 })).unwrap();
    project.save(&doc).unwrap();
    let again = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(ids(&again), ids(&rows));
    drop(project);

    // Reopened: one array again (its members are not loaded as elements), 37 members.
    let (mut p2, mut doc2, _) = LocalProject::open(&path).unwrap();
    let arrays: Vec<_> = doc2
        .store()
        .iter()
        .filter(|e| path_arrays::path_array_recipe(e).is_some())
        .map(|e| e.id())
        .collect();
    assert_eq!(arrays.len(), 1);
    assert!(!doc2.store().iter().any(|e| e.class == "IfcColumn"));
    assert_eq!(members(&r, &mut doc2, arrays[0]).len(), 37);

    // Unarray: 37 independent columns, the array gone; saved, the derived members are replaced.
    let cols = run(
        &r,
        &mut doc2,
        "array.unarray",
        json!({ "array_id": arrays[0].0 }),
    )
    .unwrap();
    assert_eq!(cols.as_array().unwrap().len(), 37);
    assert!(
        doc2.store()
            .iter()
            .all(|e| path_arrays::path_array_recipe(e).is_none())
    );
    p2.save(&doc2).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCCOLUMN"), 37);
    assert_eq!(count("IFCELEMENTASSEMBLY"), 0);
}

#[test]
fn hedge_along_a_spline_with_seeded_variation() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("h.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    write_box(&dir.path().join("shrub_a.obj"), 0.6, 0.6, 0.8);
    write_box(&dir.path().join("shrub_b.obj"), 0.5, 0.5, 1.0);
    let a = id(run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": dir.path().join("shrub_a.obj"), "category": "planting" }),
    )
    .unwrap());
    let b = id(run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": dir.path().join("shrub_b.obj"), "category": "planting" }),
    )
    .unwrap());
    let hedge = id(run(
        &r,
        &mut doc,
        "array.along_path",
        json!({
            "sources": [{ "type_id": a.0, "weight": 2 }, { "type_id": b.0 }],
            "path": { "points": [[0, 0], [4, 2], [8, 0], [12, 3]], "shape": "spline" },
            "level": l1.0,
            "distribution": { "mode": "max", "value": 0.6 },
            "variation": { "rotation": [0.0, std::f64::consts::TAU], "scale": [0.85, 1.15], "seed": 42 }
        }),
    )
    .unwrap());
    let m1 = members(&r, &mut doc, hedge);
    assert!(m1.len() > 20);
    // Spaced no more than 0.6 m apart along the curve (so no more in a straight line either).
    for w in m1.windows(2) {
        let d = (w[1]["point"][0].as_f64().unwrap() - w[0]["point"][0].as_f64().unwrap())
            .hypot(w[1]["point"][1].as_f64().unwrap() - w[0]["point"][1].as_f64().unwrap());
        assert!(d <= 0.6 + 1e-6, "{d}");
    }
    // Both shrubs appear; scales stay in range; the arrangement is stable.
    let types: std::collections::HashSet<String> = m1
        .iter()
        .map(|v| v["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(types.len(), 2);
    assert!(
        m1.iter()
            .all(|v| (0.85..=1.15).contains(&v["scale"].as_f64().unwrap()))
    );
    assert_eq!(members(&r, &mut doc, hedge), m1);
    // Reshaping the spline keeps each shrub's look (rotation, scale, model by index).
    run(&r, &mut doc, "array.set", json!({ "array_id": hedge.0, "path": { "points": [[0, 0], [4, 3], [8, 0], [12, 3]], "shape": "spline" } })).unwrap();
    let m2 = members(&r, &mut doc, hedge);
    for (x, y) in m1.iter().zip(&m2).take(10) {
        assert_eq!(x["scale"], y["scale"]);
        assert_eq!(x["type"], y["type"]);
    }
    // Reseed: a different arrangement.
    run(
        &r,
        &mut doc,
        "array.set",
        json!({ "array_id": hedge.0, "reseed": true }),
    )
    .unwrap();
    let m3 = members(&r, &mut doc, hedge);
    assert_ne!(m3[0]["scale"], m2[0]["scale"]);
    // Moved as a whole (a drawn path moves with it).
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [hedge.0], "vector": [10.0, 0.0, 0.0] }),
    )
    .unwrap();
    let m4 = members(&r, &mut doc, hedge);
    assert!(
        (m4[0]["point"][0].as_f64().unwrap() - m3[0]["point"][0].as_f64().unwrap() - 10.0).abs()
            < 1e-9
    );
}
