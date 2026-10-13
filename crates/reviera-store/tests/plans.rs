// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plan views and view ranges (spec Ch 5 §5.3, DOC-VIEW-002, DOC-VR-001).

use std::path::Path;

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn levels(doc: &Document) -> Vec<(f64, ElementId, String)> {
    reviera_core::plans::levels(doc.store())
        .into_iter()
        .map(|(z, _, id)| (z, id, doc.store().get(id).unwrap().name.clone().unwrap()))
        .collect()
}

#[test]
fn floor_plans_per_level_with_a_resolved_view_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plans.ifc");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc"),
        &path,
    )
    .unwrap();
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    let r = registry();
    let lv = levels(&doc);
    assert!(lv.len() >= 2);
    let ids: Vec<u64> = lv.iter().map(|l| l.1.0).collect();

    // One plan per level, named after it, in one undo step; existing plans are skipped.
    let undo_before = doc.undo_history().count();
    let plans = run(&r, &mut doc, "view.create_plan", json!({"level_ids": ids})).unwrap();
    assert_eq!(plans.as_array().unwrap().len(), lv.len());
    assert_eq!(doc.undo_history().count(), undo_before + 1);
    let again = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({"level_ids": [ids[0]]}),
    )
    .unwrap();
    assert_eq!(again, json!([]));
    let dup = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({"level_ids": [ids[0]], "allow_duplicates": true}),
    )
    .unwrap();
    let dup_name = doc
        .store()
        .get(ElementId(dup[0].as_u64().unwrap()))
        .unwrap()
        .name
        .clone();
    assert_eq!(dup_name, Some(format!("{} (2)", lv[0].2)));

    let ground = plans[0].clone();
    let range = run(
        &r,
        &mut doc,
        "view.get_view_range",
        json!({"view_id": ground}),
    )
    .unwrap();
    let z = |k: &str| range["resolved_elevations"][k].as_f64().unwrap();
    assert!((z("cut") - (lv[0].0 + 1.2)).abs() < 1e-9);
    assert!((z("top") - lv[1].0).abs() < 1e-9, "top is the level above");
    assert_eq!((z("bottom"), z("depth")), (lv[0].0, lv[0].0));
    // The top plan has no level above: 2300 mm above its level.
    let top_plan = plans.as_array().unwrap().last().unwrap().clone();
    let top_range = run(
        &r,
        &mut doc,
        "view.get_view_range",
        json!({"view_id": top_plan}),
    )
    .unwrap();
    assert!(
        (top_range["resolved_elevations"]["top"].as_f64().unwrap() - (lv.last().unwrap().0 + 2.3))
            .abs()
            < 1e-9
    );

    // Cut above top is refused and nothing changes.
    let bad = run(
        &r,
        &mut doc,
        "view.set_view_range",
        json!({"view_id": ground,
               "top": {"level_ref": "associated", "offset": "1 m"},
               "cut": {"level_ref": "associated", "offset": "1500 mm"},
               "bottom": {"level_ref": "associated"},
               "depth": {"level_ref": "unlimited"}}),
    )
    .unwrap_err();
    assert_eq!(bad.code, "view_range.invalid_order");
    let ok = json!({"view_id": ground,
               "top": {"level_ref": "unlimited"},
               "cut": {"level_ref": "associated", "offset": "900 mm"},
               "bottom": {"level_ref": "associated"},
               "depth": {"level_ref": "below"}});
    run(&r, &mut doc, "view.set_view_range", ok).unwrap();
    let range = run(
        &r,
        &mut doc,
        "view.get_view_range",
        json!({"view_id": ground}),
    )
    .unwrap();
    assert_eq!(range["resolved_elevations"]["top"], Value::Null);
    assert!((range["resolved_elevations"]["cut"].as_f64().unwrap() - (lv[0].0 + 0.9)).abs() < 1e-9);

    // Plans survive save and reopen.
    project.save(&doc).unwrap();
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    let mut reopened = reopened;
    let list = run(&r, &mut reopened, "view.list", json!({"kind": "view.plan"})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), lv.len() + 1);

    // Deleting a level deletes its plans.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({"element_ids": [ids[1]]}),
    )
    .unwrap();
    let deleted: Vec<u64> = deleted
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert!(deleted.contains(&plans[1].as_u64().unwrap()));
}
