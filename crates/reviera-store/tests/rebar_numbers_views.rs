// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reinforcement numbering and partitions (STR-RBR-016) and rebar visibility and presentation
//! (STR-RBR-017): identical link sets share a number in their partition and take a new one in
//! another; Remove gaps closes 1, 2, 4, 5, 7 without changing who shares; a number held by
//! another identity is refused, naming it; renumbering keeps a locked number and skips it;
//! numbers go to the IFC Tag. Unobscured in one 3D view leaves another alone; Solid is refused
//! for a plan; Show Middle draws one bar and the extent line in that view only; First/Last
//! still counts every bar.

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

fn number(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> u64 {
    run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }))[0]["number"].as_u64().unwrap()
}

/// The rows of a partition's rebar sequence: (number, identity, element ids).
fn rows(r: &CommandRegistry, doc: &mut Document, partition: &str) -> Vec<(u64, String, Vec<u64>)> {
    let l = run(r, doc, "reinforcement_numbering.list", json!({ "partition": partition }));
    let seq = l.as_array().unwrap().iter().find(|s| s["kind"] == "rebar").unwrap().clone();
    seq["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["number"].as_u64().unwrap(),
                row["identity"].as_str().unwrap().to_owned(),
                row["element_ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect(),
            )
        })
        .collect()
}

#[test]
fn numbering_partitions_and_view_presentation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("numbers.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let col = |r: &CommandRegistry, doc: &mut Document, x: f64| {
        id(run(r, doc, "column.place", json!({ "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } })))
    };
    let (a, b) = (col(&r, &mut doc, 0.0), col(&r, &mut doc, 5.0));
    let place = |r: &CommandRegistry, doc: &mut Document, host: ElementId, shape: &str, bar: &str, placement: Value| {
        id(run(
            r,
            doc,
            "rebar.place",
            json!({ "host_id": host.0, "shape": shape, "bar_type": bar, "placement": placement, "layout": { "rule": "maximum_spacing", "spacing": 0.15 } }),
        ))
    };
    let section = json!({ "kind": "section" });
    let s1 = place(&r, &mut doc, a, "51", "d8", section.clone());
    let s2 = place(&r, &mut doc, a, "51", "d10", section.clone());
    let s3 = place(&r, &mut doc, a, "00", "d16", json!({ "kind": "face", "face": "left" }));
    let s4 = place(&r, &mut doc, a, "00", "d20", json!({ "kind": "face", "face": "right" }));
    let s5 = place(&r, &mut doc, a, "51", "d12", section.clone());
    let s6 = place(&r, &mut doc, b, "51", "d12", section.clone());
    let all = [s1, s2, s3, s4, s5, s6];
    run(&r, &mut doc, "rebar.set_partition", json!({ "ids": all.map(|s| s.0), "name": "L1" }));
    // Two identical stirrup sets in L1 share number 5.
    assert_eq!(all.map(|s| number(&r, &mut doc, s)), [1, 2, 3, 4, 5, 5]);
    // Moving one to L2 gives it a number in L2's sequence; the other keeps 5.
    run(&r, &mut doc, "rebar.set_partition", json!({ "ids": [s6.0], "name": "L2" }));
    assert_eq!(number(&r, &mut doc, s6), 1);
    assert_eq!(number(&r, &mut doc, s5), 5);
    let info = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [s6.0] }));
    assert_eq!(info[0]["partition"], "L2");

    // A number held by another identity is refused, naming it.
    let ids: Vec<String> = rows(&r, &mut doc, "L1").into_iter().map(|row| row.1).collect();
    let e = r
        .execute(&mut doc, "reinforcement_numbering.set", json!({ "partition": "L1", "kind": "rebar", "identity": ids[2], "number": 4 }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "reinforcement_numbering.taken");
    assert_eq!(e.details["identity"], ids[3]);
    // Gaps: 1, 2, 4, 5, 7 …
    run(&r, &mut doc, "reinforcement_numbering.set", json!({ "partition": "L1", "kind": "rebar", "identity": ids[3], "number": 7 }));
    run(&r, &mut doc, "reinforcement_numbering.set", json!({ "partition": "L1", "kind": "rebar", "identity": ids[2], "number": 4 }));
    let before: Vec<u64> = rows(&r, &mut doc, "L1").iter().map(|row| row.0).collect();
    assert_eq!(before, [1, 2, 4, 5, 7]);
    // … closed to 1 … 5 in number order, the same elements sharing.
    run(&r, &mut doc, "reinforcement_numbering.renumber", json!({ "partition": "L1", "kind": "rebar", "remove_gaps": true }));
    let after = rows(&r, &mut doc, "L1");
    assert_eq!(after.iter().map(|row| row.0).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert_eq!(after.iter().map(|row| row.1.clone()).collect::<Vec<_>>(), [ids[0].clone(), ids[1].clone(), ids[2].clone(), ids[4].clone(), ids[3].clone()]);
    // Renumbering with 4 locked keeps 4 and numbers the others 1, 2, 3, 5.
    let four = after[3].1.clone();
    run(&r, &mut doc, "reinforcement_numbering.set", json!({ "partition": "L1", "kind": "rebar", "identity": four, "locked": true }));
    run(&r, &mut doc, "reinforcement_numbering.renumber", json!({ "partition": "L1", "kind": "rebar" }));
    let locked = rows(&r, &mut doc, "L1");
    assert_eq!(locked.iter().find(|row| row.1 == four).unwrap().0, 4);
    let mut others: Vec<u64> = locked.iter().filter(|row| row.1 != four).map(|row| row.0).collect();
    others.sort_unstable();
    assert_eq!(others, [1, 2, 3, 5]);
    // A new identity takes the next free number and keeps it when others go.
    let s7 = place(&r, &mut doc, a, "00", "d25", json!({ "kind": "face", "face": "top" }));
    run(&r, &mut doc, "rebar.set_partition", json!({ "ids": [s7.0], "name": "L1" }));
    assert_eq!(number(&r, &mut doc, s7), 6);
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [s1.0] }));
    assert_eq!(number(&r, &mut doc, s7), 6, "numbers do not move");

    // The number is the bar's IFC Tag.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let n5 = number(&r, &mut doc, s5);
    let guid = doc.store().get(s5).unwrap().guid().to_string();
    let line = text.lines().find(|l| l.contains("IFCREINFORCINGBAR(") && l.contains(&guid)).unwrap();
    assert!(line.contains(&format!("'{n5}'")), "{line}");

    // Views: two 3D views and a plan.
    let v1 = id(run(&r, &mut doc, "view.create_3d", json!({ "kind": "named", "name": "Rebar A" })));
    let v2 = id(run(&r, &mut doc, "view.create_3d", json!({ "kind": "named", "name": "Rebar B" })));
    let plans = run(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }));
    let plan = id(plans[0].clone());
    run(&r, &mut doc, "view.set_detail_level", json!({ "view_id": v1.0, "level": "fine" }));
    run(&r, &mut doc, "rebar.set_view_visibility", json!({ "rebar_ids": [s5.0], "view_id": v1.0, "unobscured": true, "solid": true }));
    let drawn = |doc: &Document, view: ElementId, set: ElementId| {
        let st = doc.store();
        reviera_core::rebar_views::drawn(st, st.get(view), st.get(set).unwrap()).unwrap()
    };
    let d1 = drawn(&doc, v1, s5);
    assert!(d1.unobscured && d1.own_body, "unobscured and solid in Rebar A");
    let d2 = drawn(&doc, v2, s5);
    assert!(!d2.unobscured && !d2.own_body && !d2.centrelines.is_empty(), "hidden by the host, as centrelines, in Rebar B: {} {} {} {v1:?} {v2:?}", d2.unobscured, d2.own_body, d2.centrelines.len());
    // Solid cannot be set for a plan.
    let e = r
        .execute(&mut doc, "rebar.set_view_visibility", json!({ "rebar_ids": [s5.0], "view_id": plan.0, "solid": true }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.solid_3d");
    // Show Middle in the plan: one bar and the extent line, there only.
    let n = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [s5.0] }))[0]["count"].as_u64().unwrap() as usize;
    run(&r, &mut doc, "rebar.set_presentation", json!({ "rebar_ids": [s5.0], "view_id": plan.0, "mode": "middle" }));
    let dp = drawn(&doc, plan, s5);
    assert_eq!(dp.shown, vec![n / 2]);
    assert_eq!(dp.extent.len(), 3, "the extent line and its two ticks");
    assert_eq!(drawn(&doc, v2, s5).shown.len(), n, "other views show every bar");
    // First/Last still counts every bar.
    run(&r, &mut doc, "rebar.set_presentation", json!({ "rebar_ids": [s5.0], "view_id": v2.0, "mode": "first_last" }));
    assert_eq!(drawn(&doc, v2, s5).shown, vec![0, n - 1]);
    assert_eq!(run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [s5.0] }))[0]["count"].as_u64().unwrap() as usize, n);
    // Select keeps picks in range; with none left the set shows all.
    run(&r, &mut doc, "rebar.set_presentation", json!({ "rebar_ids": [s5.0], "view_id": v2.0, "mode": "select", "selected_indices": [1, 500] }));
    assert_eq!(drawn(&doc, v2, s5).shown, vec![1]);
    run(&r, &mut doc, "rebar.set_presentation", json!({ "rebar_ids": [s5.0], "view_id": v2.0, "mode": "select", "selected_indices": [500] }));
    assert_eq!(drawn(&doc, v2, s5).shown.len(), n);
    let states = run(&r, &mut doc, "rebar.view_states", json!({ "rebar_id": s5.0 }));
    let plan_state = states.as_array().unwrap().iter().find(|s| s["view_id"] == plan.0).unwrap();
    assert_eq!((plan_state["mode"].clone(), plan_state["is_3d"].clone()), (json!("middle"), json!(false)));
}
