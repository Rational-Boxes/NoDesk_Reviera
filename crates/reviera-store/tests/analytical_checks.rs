// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical model checks (STR-AM-007): disconnected nodes with the measured gap, a frame with no
//! supports (S3, C2), an unsupported beam end cleared by a boundary condition (S1), a physical beam
//! moved away from its analytical member (C3), ends on a span with no node there, ignores that
//! survive save and reopen and lapse when an element moves, and checks leaving the undo stack alone.

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

fn issues<'a>(report: &'a Value, code: &str, kind: &str) -> Vec<&'a Value> {
    report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == code && i["kind"] == kind)
        .collect()
}

fn member(r: &CommandRegistry, doc: &mut Document, a: [f64; 3], b: [f64; 3], role: &str) -> u64 {
    id(run(
        r,
        doc,
        "analytical_member.create",
        json!({ "start": a, "end": b, "role": role }),
    ))
    .0
}

/// The node at the given end of a member.
fn node(r: &CommandRegistry, doc: &mut Document, m: u64, end: usize) -> u64 {
    let info = run(r, doc, "analytical_model.info", json!({}));
    info["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == m)
        .unwrap()["nodes"][end]
        .as_u64()
        .unwrap()
}

#[test]
fn supports_and_consistency_checks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checks.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    run(&r, &mut doc, "level.create", json!({ "elevation": 8.0 }));

    // A two-storey frame with no supports: S3 and C2 are errors.
    let mut cols = Vec::new();
    for x in [0.0, 6.0] {
        cols.push(member(&r, &mut doc, [x, 0.0, 0.0], [x, 0.0, 4.0], "column"));
        member(&r, &mut doc, [x, 0.0, 4.0], [x, 0.0, 8.0], "column");
    }
    member(&r, &mut doc, [0.0, 0.0, 4.0], [6.0, 0.0, 4.0], "beam");
    member(&r, &mut doc, [0.0, 0.0, 8.0], [6.0, 0.0, 8.0], "beam");
    let undo_before = doc.undo_history().count();
    let report = run(&r, &mut doc, "analytical_model.check", json!({}));
    assert_eq!(issues(&report, "S3", "no_supports").len(), 1, "{report:#}");
    assert_eq!(issues(&report, "C2", "isolated_part").len(), 1);
    assert_eq!(report["counts_by_severity"]["error"], 2);
    // Checks are read-only.
    assert_eq!(doc.undo_history().count(), undo_before);

    // Supports at the column bases clear both errors.
    for c in &cols {
        let n = node(&r, &mut doc, *c, 0);
        run(
            &r,
            &mut doc,
            "boundary_condition.create",
            json!({ "kind": "point", "host_id": n, "state": "fixed" }),
        );
    }
    let report = run(&r, &mut doc, "analytical_model.check", json!({}));
    assert!(report["counts_by_severity"].get("error").is_none(), "{report:#}");

    // A beam whose end stops 40 mm short of a column's top node: C1 with the gap measured.
    let short = member(&r, &mut doc, [6.04, 0.0, 8.0], [10.0, 0.0, 8.0], "beam");
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    let gaps = issues(&report, "C1", "disconnected_nodes");
    assert_eq!(gaps.len(), 1, "{report:#}");
    assert!((gaps[0]["measured"].as_f64().unwrap() - 0.04).abs() < 1e-9);
    assert!(gaps[0]["message"].as_str().unwrap().contains("node gap 40 mm"));
    assert!(gaps[0]["min"].is_array() && gaps[0]["max"].is_array());
    // Its far end rests on nothing: S1 (supports) and a free end (C1).
    let report = run(&r, &mut doc, "analytical_model.check_supports", json!({}));
    let s1 = issues(&report, "S1", "unsupported_member");
    assert_eq!(s1.len(), 1, "{report:#}");
    assert_eq!(s1[0]["element_ids"], json!([short]));
    // A pinned support at the free end clears S1 on re-check.
    let tip = node(&r, &mut doc, short, 1);
    run(
        &r,
        &mut doc,
        "boundary_condition.create",
        json!({ "kind": "point", "host_id": tip, "state": "pinned" }),
    );
    let report = run(&r, &mut doc, "analytical_model.check_supports", json!({}));
    assert!(issues(&report, "S1", "unsupported_member").is_empty());

    // A secondary beam framing into the lower beam's mid-span, with no node there: C1.
    member(&r, &mut doc, [3.0, 0.0, 4.0], [3.0, 5.0, 4.0], "beam");
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    assert_eq!(issues(&report, "C1", "end_on_span").len(), 1, "{report:#}");

    // Physical framing: a beam between two structural columns, then its analytical model.
    let mut doc_cols = Vec::new();
    for x in [20.0, 26.0] {
        doc_cols.push(id(run(
            &r,
            &mut doc,
            "column.place",
            json!({ "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        )));
    }
    let beam = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [20.0, 0.0], "end": [26.0, 0.0] }, "reference_level": l2.0 }),
    ));
    run(
        &r,
        &mut doc,
        "analytical_automation.physical_to_analytical",
        json!({ "element_ids": [doc_cols[0].0, doc_cols[1].0, beam.0] }),
    );
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    assert!(issues(&report, "C3", "position_mismatch").is_empty(), "{report:#}");
    // Analytical-only members are reported for information.
    let c5 = issues(&report, "C5", "no_physical");
    assert!(!c5.is_empty());
    assert_eq!(c5[0]["severity"], "info");

    // Ignore one C5 issue; it survives save and reopen.
    let key = c5[0]["key"].as_str().unwrap().to_owned();
    let ignored_member = c5[0]["element_ids"][0].as_u64().unwrap();
    run(
        &r,
        &mut doc,
        "analytical_model.ignore_issue",
        json!({ "issue_key": key, "ignore": true, "reason": "a stub for later" }),
    );
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    let again = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key.as_str())
        .unwrap();
    assert_eq!(again["ignored"], true);
    assert_eq!(again["ignore_reason"], "a stub for later");

    // Moving the physical beam 200 mm sideways: C3 at 200 mm against 150 mm.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [beam.0], "vector": [0.0, 0.2, 0.0] }),
    );
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    let c3 = issues(&report, "C3", "position_mismatch");
    assert_eq!(c3.len(), 1, "{report:#}");
    assert!((c3[0]["measured"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    assert!((c3[0]["tolerance"].as_f64().unwrap() - 0.15).abs() < 1e-9);
    assert!(c3[0]["message"].as_str().unwrap().contains("200 mm"));
    // Its axis is still within the tolerance of the beam's body; 500 mm off, it leaves it (C4).
    assert!(issues(&report, "C4", "outside_physical").is_empty(), "{report:#}");
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [beam.0], "vector": [0.0, 0.3, 0.0] }),
    );
    let report = run(&r, &mut doc, "analytical_model.check_consistency", json!({}));
    assert_eq!(issues(&report, "C4", "outside_physical").len(), 1, "{report:#}");

    // Scope: only issues touching the beam.
    let scoped = run(
        &r,
        &mut doc,
        "analytical_model.check_consistency",
        json!({ "scope": [beam.0] }),
    );
    assert!(
        scoped["issues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["code"] != "C1"),
        "{scoped:#}"
    );

    project.save(&doc).unwrap();
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let report = run(&r, &mut doc2, "analytical_model.check_consistency", json!({}));
    let again = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key.as_str())
        .unwrap();
    assert_eq!(again["ignored"], true, "survives save and reopen");

    // Moving the ignored member's node brings the issue back.
    let n = node(&r, &mut doc2, ignored_member, 1);
    run(
        &r,
        &mut doc2,
        "analytical_node.move",
        json!({ "node_ids": [n], "delta": [0.0, 0.0, -0.1] }),
    );
    let report = run(&r, &mut doc2, "analytical_model.check_consistency", json!({}));
    let again = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key.as_str())
        .unwrap();
    assert_eq!(again["ignored"], false, "lapses when the member moves");

    // C8 is off by default; turned on, members without a section are reported.
    let report = run(&r, &mut doc2, "analytical_model.check_consistency", json!({}));
    assert!(issues(&report, "C8", "missing_properties").is_empty());
    run(
        &r,
        &mut doc2,
        "structural_settings.set",
        json!({ "key": "consistency_criteria", "value": ["C1", "C8"] }),
    );
    let report = run(&r, &mut doc2, "analytical_model.check_consistency", json!({}));
    assert!(!issues(&report, "C8", "missing_properties").is_empty());
    assert!(issues(&report, "C5", "no_physical").is_empty());
    let bad = r
        .execute(
            &mut doc2,
            "structural_settings.set",
            json!({ "key": "consistency_criteria", "value": ["C9"] }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(bad.code, "structural_settings.out_of_range");
}
