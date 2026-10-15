// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reference planes (ARC-REFP-001), work planes (ARC-WP-001) and model lines (ARC-MLIN-001):
//! lines drawn on a level and on a named reference plane follow their planes, a view's work
//! plane, dissociating, deleting the host, and the IFC annotations.

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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn line_points(r: &CommandRegistry, doc: &mut Document, line: ElementId) -> Vec<[f64; 3]> {
    let all = run(r, doc, "modelline.list", json!({})).unwrap();
    let l = all
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == line.0)
        .cloned()
        .expect("listed");
    serde_json::from_value(l["points"].clone()).unwrap()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn lines_on_levels_and_reference_planes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("planes.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A model line on Level 1 from (0, 0) to (4, 0): at the level's elevation.
    let lines = run(
        &r,
        &mut doc,
        "modelline.create",
        json!({ "curves": [{ "line": { "start": [0.0, 0.0], "end": [4.0, 0.0] } }], "level": l1.0, "line_style": "Wide Lines" }),
    )
    .unwrap();
    let flat = id(&lines[0]);
    assert!(near(line_points(&r, &mut doc, flat)[0], [0.0, 0.0, 0.0]));
    // Raising the level raises the line.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l1.0, "elevation": 0.5 }),
    )
    .unwrap();
    assert!(near(line_points(&r, &mut doc, flat)[1], [4.0, 0.0, 0.5]));

    // A named vertical reference plane along x = 10, drawn in plan from (10, 0) to (10, 6).
    let rp = id(&run(
        &r,
        &mut doc,
        "refplane.create",
        json!({ "start": [10.0, 0.0], "end": [10.0, 6.0], "name": "East Face" }),
    )
    .unwrap());
    let named = run(&r, &mut doc, "workplane.list", json!({})).unwrap();
    assert!(
        named
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["label"] == "Reference Plane: East Face"),
        "{named}"
    );
    assert!(
        run(
            &r,
            &mut doc,
            "refplane.create",
            json!({ "start": [0.0, 0.0], "end": [0.0, 1.0], "name": "East Face" })
        )
        .is_err()
    );
    // An arc on it: 2 m along its line, 1 m up, to 4 m along, through (3, 2).
    let on_plane = id(&run(
        &r,
        &mut doc,
        "modelline.create",
        json!({ "curves": [{ "line": { "start": [2.0, 1.0], "end": [4.0, 1.0] }, "through": [[3.0, 2.0]] }], "ref_plane": rp.0 }),
    )
    .unwrap()[0]);
    let pts = line_points(&r, &mut doc, on_plane);
    assert!(near(pts[0], [10.0, 2.0, 1.0]), "{pts:?}");
    assert!(pts.iter().all(|p| (p[0] - 10.0).abs() < 1e-9));
    assert!(pts.iter().any(|p| (p[2] - 2.0).abs() < 0.05), "{pts:?}");

    // Moving the reference plane 2 m east moves the line on it (it follows its host).
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [rp.0, on_plane.0], "vector": [2.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert!(near(
        line_points(&r, &mut doc, on_plane)[0],
        [12.0, 2.0, 1.0]
    ));
    // Moving the line alone in plan keeps its place on a plane fixed in space.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [flat.0], "vector": [0.0, 1.0, 0.0] }),
    )
    .unwrap();
    assert!(near(line_points(&r, &mut doc, flat)[0], [0.0, 1.0, 0.5]));

    // A plan's work plane is its level until one is set.
    let plan = id(&run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l1.0] }),
    )
    .unwrap()[0]);
    let wp = run(&r, &mut doc, "workplane.get", json!({ "view_id": plan.0 })).unwrap();
    assert_eq!(wp["label"], "Level: Level 1", "{wp}");
    let wp = run(
        &r,
        &mut doc,
        "workplane.set",
        json!({ "view_id": plan.0, "ref_plane": rp.0 }),
    )
    .unwrap();
    assert_eq!(wp["label"], "Reference Plane: East Face");
    assert!((wp["resolved"]["origin"][0].as_f64().unwrap() - 12.0).abs() < 1e-9);

    // Saved: two IfcAnnotation model lines (styled) and one reference plane.
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let rows = ifc.step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCANNOTATION"), 3);
    assert_eq!(count("IFCCURVESTYLE"), 2);
    assert!(
        rows.iter()
            .any(|r| r.class == "IFCANNOTATION" && r.attributes.contains("'ReferencePlane'"))
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCANNOTATION" && r.attributes.contains("'ModelLine'"))
            .count(),
        2
    );
    // Saved again unchanged, the curves are not rewritten.
    let polylines = |rows: &[reviera_ifc::StepRow]| {
        rows.iter()
            .filter(|r| r.class == "IFCPOLYLINE")
            .map(|r| r.id.0)
            .collect::<Vec<_>>()
    };
    run(&r, &mut doc, "level.create", json!({ "elevation": 6.0 })).unwrap();
    project.save(&doc).unwrap();
    let again = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(polylines(&again), polylines(&rows));
    drop(project);

    // Reopened intact.
    let (_p2, mut doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        run(&r, &mut doc2, "modelline.list", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let rp2 = id(&run(&r, &mut doc2, "refplane.list", json!({})).unwrap()[0]["id"]);
    // Dissociated, the arc stays when its plane moves; then deleting the plane leaves it.
    let arc2 = run(&r, &mut doc2, "modelline.list", json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["work_plane"] == "Reference Plane: East Face")
        .map(|l| id(&l["id"]))
        .unwrap();
    run(
        &r,
        &mut doc2,
        "element.dissociate_work_plane",
        json!({ "element_ids": [arc2.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc2,
        "refplane.set",
        json!({ "ref_plane_id": rp2.0, "start": [20.0, 0.0], "end": [20.0, 6.0] }),
    )
    .unwrap();
    assert!(near(line_points(&r, &mut doc2, arc2)[0], [12.0, 2.0, 1.0]));
    let deleted = run(
        &r,
        &mut doc2,
        "element.delete",
        json!({ "element_ids": [rp2.0] }),
    )
    .unwrap();
    assert_eq!(deleted.as_array().unwrap().len(), 1);

    // Back on the level, a line deleted with its level's lines when the level goes.
    let level1 = doc2
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey" && e.name.as_deref() == Some("Level 1"))
        .unwrap()
        .id();
    run(
        &r,
        &mut doc2,
        "element.set_work_plane",
        json!({ "element_ids": [arc2.0], "level": level1.0 }),
    )
    .unwrap();
    // Projected onto the level's plane (z = 0.5).
    assert!(near(line_points(&r, &mut doc2, arc2)[0], [12.0, 2.0, 0.5]));
}
