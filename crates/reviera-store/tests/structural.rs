// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural walls (STR-WALL-001) and structural floors (STR-FLOOR-001): usage, IFC
//! PredefinedType and LoadBearing, span directions, and their round trip through IFC.

use reviera_core::structural::{self, WallUsage};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array(
        (0..4)
            .map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } }))
            .collect(),
    )
}

fn param<'a>(doc: &'a Document, e: ElementId, k: &str) -> Option<&'a Value> {
    doc.store().get(e).unwrap().param(k)
}

#[test]
fn structural_walls_and_floors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A structural wall: bearing by default, SOLIDWALL, load-bearing.
    let w = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    assert_eq!(
        structural::wall_usage(doc.store().get(w).unwrap()),
        WallUsage::Bearing
    );
    assert_eq!(param(&doc, w, "@PredefinedType").unwrap(), "SOLIDWALL");
    assert_eq!(
        param(&doc, w, "Pset_WallCommon.LoadBearing").unwrap()["value"],
        true
    );
    // Shear: SHEAR; structural combined keeps its exact usage in RXD.
    run(
        &r,
        &mut doc,
        "wall.set_structural_usage",
        json!({ "wall_ids": [w.0], "usage": "shear" }),
    );
    assert_eq!(param(&doc, w, "@PredefinedType").unwrap(), "SHEAR");
    run(
        &r,
        &mut doc,
        "wall.set_structural_usage",
        json!({ "wall_ids": [w.0], "usage": "structural_combined" }),
    );
    assert_eq!(
        structural::wall_usage(doc.store().get(w).unwrap()),
        WallUsage::StructuralCombined
    );
    // An ordinary wall made non-bearing explicitly: STANDARD, not load-bearing, no record.
    let plain = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 6.0], "end": [8.0, 6.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural_usage": "bearing" }),
    ));
    run(
        &r,
        &mut doc,
        "wall.set_structural_usage",
        json!({ "wall_ids": [plain.0], "usage": "non_bearing" }),
    );
    assert_eq!(param(&doc, plain, "@PredefinedType").unwrap(), "STANDARD");
    assert_eq!(
        param(&doc, plain, "Pset_WallCommon.LoadBearing").unwrap()["value"],
        false
    );
    assert!(param(&doc, plain, structural::KEY).is_none());

    // A structural floor spans along its first edge; then along its second, or a vector.
    let f = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [rect([0.0, 0.0], [8.0, 6.0])], "level_id": l1.0 }),
    ));
    run(
        &r,
        &mut doc,
        "floor.set_structural",
        json!({ "floor_ids": [f.0], "structural": true }),
    );
    let e = doc.store().get(f).unwrap();
    assert!(structural::is_structural_floor(e));
    let s = structural::span(e).unwrap();
    assert!(
        (s[0].abs() - 1.0).abs() < 1e-9 && s[1].abs() < 1e-9,
        "{s:?}"
    );
    run(
        &r,
        &mut doc,
        "floor.set_span_direction",
        json!({ "floor_id": f.0, "edge_index": 1 }),
    );
    let s = structural::span(doc.store().get(f).unwrap()).unwrap();
    assert!(
        s[0].abs() < 1e-9 && (s[1].abs() - 1.0).abs() < 1e-9,
        "{s:?}"
    );
    run(
        &r,
        &mut doc,
        "floor.set_span_direction",
        json!({ "floor_id": f.0, "vector": [3.0, 3.0] }),
    );
    let s = structural::span(doc.store().get(f).unwrap()).unwrap();
    assert!((s[0] - s[1]).abs() < 1e-9 && (s[0] - 0.5f64.sqrt()).abs() < 1e-9);
    assert_eq!(
        param(&doc, f, "Pset_SlabCommon.LoadBearing").unwrap()["value"],
        true
    );

    // IFC: the walls' PredefinedTypes and LoadBearing; reopened, usage and span kept.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".SOLIDWALL."));
    assert!(text.contains("LoadBearing"));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        structural::wall_usage(doc2.store().get(w).unwrap()),
        WallUsage::StructuralCombined
    );
    let s2 = structural::span(doc2.store().get(f).unwrap()).unwrap();
    assert!((s2[0] - s[0]).abs() < 1e-9);
    // Not structural again.
    run(
        &r,
        &mut doc,
        "floor.set_structural",
        json!({ "floor_ids": [f.0], "structural": false }),
    );
    assert!(!structural::is_structural_floor(
        doc.store().get(f).unwrap()
    ));
}
