// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Connection automation (STR-STL-004): a steel bay with columns, perimeter beams, an infill beam
//! and a brace has every kind of join found and connected by the Default rules; a preview leaves
//! the model and the undo history alone; a second run skips every join; a generic connection is
//! given its type with `replace_generic`; a project rule set with a depth range leaves shallow
//! beams unconnected; concrete columns are skipped; the report as CSV.

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

fn kinds(report: &Value, status: &str) -> Vec<String> {
    let mut k: Vec<String> = report["joins"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| j["status"] == status)
        .map(|j| format!("{}:{}", j["kind"].as_str().unwrap(), j["type_id"].as_str().unwrap_or("-")))
        .collect();
    k.sort();
    k
}

#[test]
fn connection_automation_connects_a_steel_bay() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("auto.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w12 = run(&r, &mut doc, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let w16 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W16X26" }));
    let w8 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W10X33" }));
    let hss = run(&r, &mut doc, "brace.load_section", json!({ "catalog": "aisc_hss_rect", "designation": "HSS6X6X3/8" }));
    let pts = [[0.0, 0.0], [6.0, 0.0], [6.0, 6.0], [0.0, 6.0]];
    let cols: Vec<ElementId> = pts
        .iter()
        .map(|p| {
            id(run(
                &r,
                &mut doc,
                "column.place",
                json!({ "type_id": w12, "point": p, "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
            ))
        })
        .collect();
    let mut beams = Vec::new();
    for i in 0..4 {
        beams.push(id(run(
            &r,
            &mut doc,
            "beam.create",
            json!({ "type_id": w16, "curve": { "start": pts[i], "end": pts[(i + 1) % 4] }, "reference_level": l2.0 }),
        )));
    }
    // An infill beam between the beams on y = 0 and y = 6.
    let infill = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "type_id": w8, "curve": { "start": [3.0, 0.0], "end": [3.0, 6.0] }, "reference_level": l2.0 }),
    ));
    run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "type_id": hss, "start_point": [0.0, 0.0], "start_level": l1.0, "end_point": [6.0, 0.0], "end_level": l2.0 }),
    );

    // The preview: 4 bases, 8 beam–column ends, 2 infill ends, 2 brace ends; nothing changes.
    let undo_before = doc.undo_history().count();
    let elements_before = doc.store().iter().count();
    let preview = run(&r, &mut doc, "structural_connection.automate", json!({ "preview": true }));
    assert_eq!(
        kinds(&preview, "placed"),
        [
            vec!["beam_beam:shear_plate"; 2],
            vec!["beam_column:shear_plate"; 8],
            vec!["brace:gusset"; 2],
            vec!["column_base:base_plate"; 4],
        ]
        .concat(),
        "{preview:#}"
    );
    assert_eq!(doc.undo_history().count(), undo_before);
    assert_eq!(doc.store().iter().count(), elements_before);

    // Applying the preview places all sixteen, in one undo step, and logs the run.
    let done = run(&r, &mut doc, "structural_connection.apply_automation", json!({ "report": preview }));
    assert_eq!(done["placed"], 16, "{done:#}");
    assert_eq!(doc.undo_history().count(), undo_before + 1);
    let info = run(&r, &mut doc, "structural_connection.rule_sets", json!({}));
    assert_eq!(info["runs"].as_array().unwrap().len(), 1);
    assert_eq!(info["runs"][0]["placed"], 16);

    // Again: every join skipped as already connected.
    let again = run(&r, &mut doc, "structural_connection.automate", json!({}));
    assert_eq!((again["placed"].as_u64(), again["skipped"].as_u64()), (Some(0), Some(16)), "{again:#}");
    assert!(again["joins"].as_array().unwrap().iter().all(|j| j["reason"].as_str().unwrap().starts_with("already connected")));

    // A generic connection is skipped, then given its type with `replace_generic`.
    let gone: Vec<u64> = done["joins"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| j["members"][1] == infill.0)
        .map(|j| j["connection"].as_u64().unwrap())
        .collect();
    run(&r, &mut doc, "element.delete", json!({ "element_ids": gone }));
    run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [beams[0].0, infill.0] }));
    let skip = run(&r, &mut doc, "structural_connection.automate", json!({ "element_ids": [infill.0] }));
    assert_eq!(kinds(&skip, "placed"), ["beam_beam:shear_plate"], "the other end has none: {skip:#}");
    run(&r, &mut doc, "edit.undo", json!({}));
    let replace = run(
        &r,
        &mut doc,
        "structural_connection.automate",
        json!({ "element_ids": [infill.0], "existing": "replace_generic" }),
    );
    assert_eq!(replace["placed"], 2, "{replace:#}");
    assert!(replace["joins"].as_array().unwrap().iter().any(|j| j["reason"] == "generic connection given the type"));

    // A project rule set: shear plates only for beams 12" and deeper; the W10 infill matches none.
    run(
        &r,
        &mut doc,
        "structural_connection.set_rule_set",
        json!({ "name": "Deep only", "rules": [
            { "type_id": "shear_plate", "join": "beam_beam", "sections": ["i_shape"], "depth_min": 0.3 }
        ] }),
    );
    let deep = run(
        &r,
        &mut doc,
        "structural_connection.automate",
        json!({ "element_ids": [infill.0], "rule_set": "Deep only", "preview": true }),
    );
    assert!(
        deep["joins"].as_array().unwrap().iter().all(|j| j["reason"] == "no rule matches"),
        "{deep:#}"
    );

    // Concrete columns are not connected.
    let c = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [20.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let rep = run(&r, &mut doc, "structural_connection.automate", json!({ "element_ids": [c.0], "preview": true }));
    assert_eq!(rep["joins"][0]["reason"], "not all members are steel", "{rep:#}");

    // The CSV: a header and a row per join.
    let csv = run(&r, &mut doc, "structural_connection.automation_csv", json!({ "report": done }));
    let csv = csv.as_str().unwrap();
    assert_eq!(csv.lines().count(), 17);
    assert!(csv.lines().nth(1).unwrap().contains(",placed,"));
    let _ = cols;
}
