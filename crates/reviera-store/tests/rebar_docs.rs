// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar documentation (STR-RBR-018): the bending schedule's mass for 10 bars of 6 m Ø16 is
//! 94.7 kg; a multi-rebar annotation on 15 Ø12 bars at 150 mm reads "15 Ø12 @ 150" and measures
//! the set's 2 100 mm extent; two sets that are not parallel are refused; a change to a set's
//! segment B shows in its bending detail and the schedule's B column at once; the Rebar Tag
//! reads number and description; the annotations go with the last set they reference.

use std::collections::HashMap;

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
    Value::Array((0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect())
}

/// The schedule as a table of cell texts, with its headings.
fn table(doc: &Document, schedule: ElementId) -> (Vec<String>, Vec<Vec<String>>, Vec<String>) {
    let st = doc.store();
    let t = reviera_core::schedules::table(st, st.get(schedule).unwrap(), &HashMap::new()).unwrap();
    (t.headings, t.rows.iter().map(|r| r.cells.iter().map(|c| c.text.clone()).collect()).collect(), t.totals.unwrap_or_default())
}

#[test]
fn tags_annotations_details_and_schedules() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("docs.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let plan = id(run(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }))[0].clone());

    // 10 bars of 6 m Ø16 in a 6.05 m slab (25 mm edge cover each end).
    let f = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([0.0, 0.0], [6.05, 3.0])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let bars = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": f.0, "shape": "00", "bar_type": "d16", "placement": { "kind": "layer", "face": "bottom" }, "layout": { "rule": "fixed_number", "n": 10 } }),
    ));
    let sched = id(run(&r, &mut doc, "rebar_schedule.create", json!({})));
    let (headings, rows, totals) = table(&doc, sched);
    let col = |name: &str| headings.iter().position(|h| h == name).unwrap_or_else(|| panic!("{name} in {headings:?}"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][col("Quantity")], "10");
    assert_eq!(rows[0][col("Bar Length")], "6000");
    assert_eq!(rows[0][col("Mass")], "94.7", "10 × 6 m × 1.578 kg/m");
    assert_eq!(totals[col("Mass")], "94.7");

    // 15 Ø12 bars at 150 mm: a 2 100 mm extent. The slab is 2.162 m across: 2.1 + 2 × 25 + 12.
    let g = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([10.0, 0.0], [16.0, 2.162])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [g.0], "structural": true }));
    let set = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": g.0, "shape": "00", "bar_type": "d12", "placement": { "kind": "layer", "face": "bottom" }, "layout": { "rule": "number_with_spacing", "n": 15, "spacing": 0.15 } }),
    ));
    let multi = id(run(&r, &mut doc, "multi_rebar_annotation.place", json!({ "rebar_ids": [set.0], "view_id": plan.0, "kind": "aligned", "line": [17.0, 1.0] })));
    let list = run(&r, &mut doc, "rebar_annotation.list", json!({ "view_id": plan.0 }));
    let m = list.as_array().unwrap().iter().find(|a| a["kind"] == "multi").unwrap();
    assert_eq!(m["drawn"]["text"], "15 Ø12 @ 150");
    assert!((m["drawn"]["extent"].as_f64().unwrap() - 2.1).abs() < 1e-9, "{}", m["drawn"]["extent"]);
    // A set at right angles is not parallel: refused.
    let cross = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": g.0, "shape": "00", "bar_type": "d12", "placement": { "kind": "layer", "face": "top", "angle": 90.0 }, "layout": { "rule": "maximum_spacing", "spacing": 0.3 } }),
    ));
    let e = r
        .execute(&mut doc, "multi_rebar_annotation.place", json!({ "rebar_ids": [set.0, cross.0], "view_id": plan.0, "line": [17.0, 1.0] }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "multi_rebar.not_parallel");

    // The Rebar Tag reads number and description.
    run(&r, &mut doc, "rebar_tag.place", json!({ "rebar_id": set.0, "view_id": plan.0 }));
    let tags = run(&r, &mut doc, "tag.list", json!({ "view_id": plan.0 }));
    let number = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [set.0] }))[0]["number"].as_u64().unwrap();
    assert_eq!(tags[0]["lines"], json!([number.to_string(), "15 Ø12 @ 150"]));

    // U bars in a column: changing B shows in the bending detail and the schedule's B column.
    let c = id(run(&r, &mut doc, "column.place", json!({ "point": [30.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } })));
    let u = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": c.0, "shape": "21", "bar_type": "d16", "placement": { "kind": "face", "face": "left" }, "layout": { "rule": "fixed_number", "n": 2 } }),
    ));
    let detail = id(run(&r, &mut doc, "bending_detail.place", json!({ "rebar_id": u.0, "view_id": plan.0, "point": [32.0, 0.0], "scale": 0.1 })));
    let b_of = |doc: &mut Document| {
        let list = run(&r, doc, "rebar_annotation.list", json!({ "view_id": plan.0 }));
        let d = list.as_array().unwrap().iter().find(|a| a["kind"] == "bending").unwrap().clone();
        let b = d["drawn"]["dimensions"].as_array().unwrap().iter().find(|p| p[0] == "B").unwrap()[1].as_f64().unwrap();
        let line = d["drawn"]["lines"].as_array().unwrap().iter().find(|l| l.as_str().unwrap().starts_with("B ")).unwrap().as_str().unwrap().to_owned();
        (b, line)
    };
    let sched_b = |doc: &Document| {
        let (h, rows, _) = table(doc, sched);
        let b = h.iter().position(|x| x == "B").unwrap();
        let shape = h.iter().position(|x| x == "Shape Code").unwrap();
        rows.iter().find(|row| row[shape] == "21").unwrap()[b].clone()
    };
    let (b0, line0) = b_of(&mut doc);
    assert_eq!(line0, format!("B {:.0}", b0 * 1000.0));
    assert_eq!(sched_b(&doc), format!("{:.0}", b0 * 1000.0));
    // A taller column: a longer B, in both at once.
    let l3 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 5.0 })));
    run(&r, &mut doc, "column.set", json!({ "column_ids": [c.0], "top_constraint": { "level": l3.0 } }));
    let (b1, line1) = b_of(&mut doc);
    assert!((b1 - b0 - 1.0).abs() < 1e-6, "{b0} → {b1}");
    assert_eq!(line1, format!("B {:.0}", b1 * 1000.0));
    assert_eq!(sched_b(&doc), format!("{:.0}", b1 * 1000.0));

    // The annotations go with the sets.
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [u.0, set.0] }));
    assert!(doc.store().get(detail).is_none() && doc.store().get(multi).is_none());
    let _ = bars;
}
