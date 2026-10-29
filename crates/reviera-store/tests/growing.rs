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

#[test]
fn high_wire_gutters_hang_in_rows_under_the_crop_wire() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("hw.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A Venlo range, gutters at 5.5 m: high-wire gutters along it, the wire at 3.25 m.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "venlo", "level_id": level.0, "span": 8.0, "spans": 2, "length": 24.0 })).unwrap();
    let layout = run(&r, &mut doc, "growing.layout", json!({ "host_id": gh, "system": "high_wire" })).unwrap();
    let s = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    // Rows 1.25 m apart across each truss, split by the main aisle and the truss posts.
    assert!(s["pieces"].as_u64().unwrap() >= 20, "{s}");
    let store = doc.store();
    let parts: Vec<_> = store.iter().filter_map(reviera_core::growing::part_recipe).collect();
    assert!(parts.iter().all(|p| p.wire == Some(3.25) && p.top[1] < p.top[0]), "falling to their drain ends under the wire");
    // A gutter, its wire and its hangers.
    let (_, class, meshes) = reviera_core::growing::all(store).into_iter().next().unwrap();
    assert_eq!(class, "IfcBuildingElementProxy");
    assert!(meshes.len() >= 4);
    // A wire too low for the gutter is refused.
    assert!(run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "wire": 0.8 })).is_err());
}

#[test]
fn a_layout_fills_a_room() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("room.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 12 × 7 m grow room.
    let pts = [[0.0, 0.0], [12.0, 0.0], [12.0, 7.0], [0.0, 7.0]];
    for i in 0..4 {
        run(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": pts[i], "end": pts[(i + 1) % 4] } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    }
    let room = run(&r, &mut doc, "room.place", json!({ "level_id": level.0, "point": [6.0, 3.5], "limit_offset": 3.0 })).unwrap();
    let layout = run(&r, &mut doc, "growing.layout", json!({ "host_id": room, "system": "benches", "direction": "along" })).unwrap();
    let s = run(&r, &mut doc, "growing.summary", json!({ "layout_id": layout })).unwrap();
    // Benches along the 12 m, split by the main aisle: rows of 1.8 m in the room's 7 m less the
    // walls and the clearance.
    assert!(s["pieces"].as_u64().unwrap() >= 4 && s["share"].as_f64().unwrap() > 40.0, "{s}");
    // Every piece inside the room, clear of the walls (their inner faces 100 mm in).
    let store = doc.store();
    for p in store.iter().filter_map(reviera_core::growing::part_recipe) {
        for q in [p.start, p.end] {
            assert!(q[0] > 0.29 && q[0] < 11.71 && q[1] > 0.29 + p.width / 2.0 - 1e-6 && q[1] < 6.71 - p.width / 2.0 + 1e-6, "{p:?}");
        }
    }
    // High-wire gutters in a 3 m room keep their wire under its top.
    run(&r, &mut doc, "growing.set_layout", json!({ "layout_id": layout, "system": "high_wire" })).unwrap();
    let store = doc.store();
    assert!(store.iter().filter_map(reviera_core::growing::part_recipe).all(|p| p.wire.is_some_and(|w| w <= 2.7 + 1e-9)));
}
