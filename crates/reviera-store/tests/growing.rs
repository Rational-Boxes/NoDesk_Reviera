// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Growing layouts (ARC-GLZ-005): benches, rolling benches and NFT tables in a glazed enclosure,
//! their growing area, following the enclosure, and saved to IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user()).map(|o| o.result)
}

#[test]
fn benches_fill_a_greenhouse_and_follow_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 9.6 × 40 m gable greenhouse.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": 40.0, "span": 9.6, "bay": 4.0 })).unwrap();
    // 1.8 m benches along the length, 0.6 m aisles, a 1.2 m main aisle across the middle.
    let layout = run(&r, &mut doc, "growing.layout", json!({ "enclosure_id": gh, "system": "benches", "direction": "along" })).unwrap();
    let s = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    let share = s["share"].as_f64().unwrap();
    assert!((60.0..75.0).contains(&share), "{s}");
    assert_eq!(s["pieces"], 8, "four rows, split by the main aisle: {s}");
    // Rolling benches pack the floor tighter.
    run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "system": "rolling_benches" })).unwrap();
    let rolling = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    assert!(rolling["share"].as_f64().unwrap() > 80.0, "{rolling}");
    // Benches across the span, from the walls to a main aisle along the middle.
    run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "system": "benches", "direction": "across" })).unwrap();
    let across = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    assert!(across["share"].as_f64().unwrap() > 50.0, "{across}");
    // NFT tables: no run longer than 12 m, each falling 2 %.
    run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "system": "nft", "direction": "along" })).unwrap();
    let nft = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    assert!(nft["longest_run"].as_f64().unwrap() <= 12.0 + 1e-9, "{nft}");
    let store = doc.store();
    let part = store.iter().find_map(reviera_core::growing::part_recipe).unwrap();
    let run_length = (part.end[0] - part.start[0]).hypot(part.end[1] - part.start[1]);
    assert!(((part.top[0] - part.top[1]) - 0.02 * run_length).abs() < 1e-9, "{part:?}");
    assert_eq!(reviera_core::categories::category_of(store.iter().find(|e| reviera_core::growing::part_recipe(e).is_some()).unwrap()).map(|c| c.key), Some("cat.growing_systems"));

    // Back to benches; a longer enclosure gets more of them.
    run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "system": "benches" })).unwrap();
    let before = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap()["growing_area"].as_f64().unwrap();
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": gh, "length": 60.0 })).unwrap();
    let after = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap()["growing_area"].as_f64().unwrap();
    assert!(after > before * 1.4, "{before} → {after}");
    // A multispan's benches split round the valley posts: none meets a post.
    let ms = run(&r, &mut doc, "enclosure.create", json!({ "form": "multispan", "level_id": level.0, "origin": [0, 30], "length": 24.0, "spans": 3 })).unwrap();
    let l2 = run(&r, &mut doc, "growing.layout", json!({ "enclosure_id": ms, "system": "benches" })).unwrap();
    let store = doc.store();
    let bodies = reviera_core::bodies::all(store);
    let posts: Vec<_> = bodies.iter().filter(|b| store.get(b.element).is_some_and(|e| e.class == "IfcColumn")).collect();
    let bench_ids: Vec<ElementId> = reviera_core::growing::all(store).iter().map(|(id, _, _)| *id).collect();
    assert!(!bench_ids.is_empty());
    let bounds = |b: &reviera_core::bodies::Body| {
        let pts = b.parts.iter().flat_map(|p| p.mesh.0.iter());
        pts.fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])]))
    };
    for b in bodies.iter().filter(|b| bench_ids.contains(&b.element)) {
        let (lo, hi) = bounds(b);
        for p in &posts {
            let (pl, ph) = bounds(p);
            let apart = (0..3).any(|k| hi[k] <= pl[k] + 1e-6 || ph[k] <= lo[k] + 1e-6);
            assert!(apart, "bench {} meets post {}", b.element, p.element);
        }
    }
    let _ = l2;
    // Saved: the benches are IfcFurniture with their geometry.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCFURNITURE(") && text.contains("GrowingLayout"));
}
