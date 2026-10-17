// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural Settings (STR-SET-001) and Coarse framing (DOC-GFX-010): the settings' rules,
//! point-to-point steel, view detail levels, and single lines for beams and braces.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, beams};
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn settings_detail_levels_and_coarse_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ss.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(
        &r,
        &mut doc,
        "level.create",
        json!({ "elevation": 4.0 }),
    ));

    // Defaults, and the dialog's rules.
    let s = run(&r, &mut doc, "structural_settings.get", Value::Null);
    assert_eq!(s["coarse_cutback"], 3.0);
    assert_eq!(s["brace_symbol"], "parallel_line");
    assert_eq!(s["support_distance"], 0.3);
    let refuse = |doc: &mut Document, args: Value| {
        r.execute(doc, "structural_settings.set", args, &Caller::user())
            .unwrap_err()
    };
    let e = refuse(&mut doc, json!({ "key": "coarse_cutback", "value": -1 }));
    assert_eq!(e.code, "structural_settings.out_of_range");
    assert_eq!(e.details["key"], "coarse_cutback");
    // Node merge 400 mm with support distance 300 mm is refused, naming the rule.
    let e = refuse(
        &mut doc,
        json!({ "key": "node_merge_distance", "value": "400 mm" }),
    );
    assert!(
        e.message.contains("smaller than the support distance"),
        "{e:?}"
    );
    let e = refuse(&mut doc, json!({ "key": "no_such", "value": 1 }));
    assert_eq!(e.code, "structural_settings.unknown_key");
    // Several values in one step.
    run(
        &r,
        &mut doc,
        "structural_settings.set",
        json!({ "values": { "coarse_cutback": 5, "support_distance": "500 mm", "brace_offset": 4 } }),
    );
    let s = run(&r, &mut doc, "structural_settings.get", Value::Null);
    assert_eq!(s["coarse_cutback"], 5.0);
    assert!(near(s["support_distance"].as_f64().unwrap(), 0.5));
    run(&r, &mut doc, "edit.undo", json!({}));
    let s = run(&r, &mut doc, "structural_settings.get", Value::Null);
    assert_eq!(s["coarse_cutback"], 3.0);

    // A girder between two columns, and a brace from one column's foot to the other's head.
    for x in [0.0, 6.0] {
        run(
            &r,
            &mut doc,
            "column.place",
            json!({ "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        );
    }
    let g = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));
    let b = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "start_point": [0.0, 0.0], "start_level": l1.0, "end_point": [6.0, 0.0], "end_level": l2.0 }),
    ));
    let length = |doc: &mut Document| {
        run(&r, doc, "beam.info", json!({ "beam_ids": [g.0] }))[0]["length"]
            .as_f64()
            .unwrap()
    };
    let joined = length(&mut doc);
    // Point-to-point steel: steel members are not shortened at joins.
    run(
        &r,
        &mut doc,
        "structural_settings.set",
        json!({ "key": "point_to_point_steel", "value": true }),
    );
    assert!(near(length(&mut doc), 6.0));
    run(
        &r,
        &mut doc,
        "structural_settings.set",
        json!({ "key": "point_to_point_steel", "value": false }),
    );
    assert!(near(length(&mut doc), joined));

    // Coarse lines at 1:100 for the Level 2 range: the girder along its location line, less the
    // coarse cutback (3 mm × 100 = 300 mm) at each joined end; the brace offset 2 mm × 100.
    let lines = beams::coarse_lines(doc.store(), 3.0, 4.0, 100.0);
    let girder = lines.iter().find(|l| l.element == g).unwrap();
    let len = |l: &beams::CoarseLine| {
        let [a, b] = l.segment;
        ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
    };
    assert!(near(len(girder), joined - 0.6), "{}", len(girder));
    assert!(near(girder.segment[0][2], 4.0));
    let brace = lines.iter().find(|l| l.element == b).unwrap();
    assert!(near(brace.segment[0][1].abs(), 0.2), "{:?}", brace.segment);
    // No brace symbol: braces are left out.
    run(
        &r,
        &mut doc,
        "structural_settings.set",
        json!({ "key": "brace_symbol", "value": "none" }),
    );
    let lines = beams::coarse_lines(doc.store(), 3.0, 4.0, 100.0);
    assert!(lines.iter().all(|l| l.element != b));
    // Out of the range: nothing.
    assert!(beams::coarse_lines(doc.store(), 10.0, 12.0, 100.0).is_empty());

    // A plan's detail level: Medium by default, Coarse when set.
    let views = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l2.0], "allow_duplicates": true }),
    );
    let plan = id(views[0].clone());
    let level = |doc: &Document| reviera_core::views::detail_level(doc.store().get(plan).unwrap());
    assert_eq!(level(&doc), reviera_core::views::DetailLevel::Medium);
    run(
        &r,
        &mut doc,
        "view.set_detail_level",
        json!({ "view_id": plan.0, "level": "coarse" }),
    );
    assert_eq!(level(&doc), reviera_core::views::DetailLevel::Coarse);
}
