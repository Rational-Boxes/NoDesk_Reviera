// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ramps (ARC-RAMP-001) and Measure (ARC-MOD-023).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

#[test]
fn a_ramp_with_landings_its_warning_ifc_and_measures() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let exec =
        |doc: &mut Document, name: &str, args: Value| r.execute(doc, name, args, &Caller::user());
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // 0.6 m at 1:12 needs 7.2 m: drawn so, with a 6 m maximum incline, it gets one landing.
    let out = exec(
        &mut doc,
        "ramp.create",
        json!({
            "path": [[0, 0], [7.2, 0]], "base_level": l1.0,
            "top_constraint": { "unconnected": 0.6 },
        }),
    )
    .unwrap();
    assert!(out.warnings.is_empty());
    let ramp = ElementId(out.result.as_u64().unwrap());
    let t = doc
        .store()
        .iter()
        .find(|e| reviera_core::ramps::ramp_type(e).is_some())
        .unwrap()
        .id();
    exec(
        &mut doc,
        "type.set_param",
        json!({ "type_id": t.0, "param": "max_incline_length", "value": "6 m" }),
    )
    .unwrap();
    let g = reviera_core::ramps::regenerate(doc.store(), doc.store().get(ramp).unwrap()).unwrap();
    assert_eq!(g.pieces.iter().filter(|p| p.landing).count(), 1);
    assert!(!g.too_steep());
    // Drawn too short, it is still made, with a warning.
    let out = exec(
        &mut doc,
        "ramp.set",
        json!({ "ramp_ids": [ramp.0], "path": [[0, 0], [5, 0]] }),
    )
    .unwrap();
    assert_eq!(out.warnings[0].id, "arc.ramp.too_steep");
    // Measured along its path, and between two points.
    let along = exec(&mut doc, "measure.along", json!({ "element_id": ramp.0 })).unwrap();
    assert_eq!(along.result, json!(5.0));
    let between = exec(
        &mut doc,
        "measure.between",
        json!({ "a": [0, 0], "b": [3, 4] }),
    )
    .unwrap();
    assert_eq!(between.result["distance"], json!(5.0));

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCRAMP").count(), 1);
    assert_eq!(rows.iter().filter(|r| r.class == "IFCRAMPTYPE").count(), 1);
}
