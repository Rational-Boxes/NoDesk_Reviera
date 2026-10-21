// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Risers (MEP-DUCT-011): a 200 mm pipe risen from Level 1 to Level 3 + 300 mm with one elbow,
//! bound to Level 3 as it moves (the run beyond following), the rise and drop symbols in plans by
//! detail level, and a riser refused when no elbow fits.

use reviera_core::mep_risers::{Kind, symbols};
use reviera_core::mep_runs::segment_recipe;
use reviera_core::views::DetailLevel;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<reviera_core::command::Outcome> {
    r.execute(doc, name, args, &Caller::user())
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

#[test]
fn a_riser_bound_to_level_3() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("riser.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l1z = reviera_core::levels::elevation(doc.store().get(l1).unwrap());
    let l2 = id(&ok(&r, &mut doc, "level.create", json!({ "elevation": l1z + 3.0, "make_plan_view": false })));
    let l3 = id(&ok(&r, &mut doc, "level.create", json!({ "elevation": l1z + 6.0, "make_plan_view": false })));
    // A 200 mm pipe at 2.4 m along x, its end open.
    let pipe = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 0], [5, 0]], "level": l1.0, "diameter": 0.2, "system_type_id": "domestic_cold_water" }));
    let seg = id(&pipe["segments"][0]);
    let count = |doc: &Document| doc.store().iter().count();

    // To Level 3 + 300 mm: one elbow, one vertical pipe whose top is there.
    let rep = ok(&r, &mut doc, "mep_run.create_riser", json!({ "element_id": seg.0, "connector_index": 1, "top_level": l3.0, "top_offset": 0.3 }));
    assert_eq!((rep["fittings"].as_array().unwrap().len(), rep["segments"].as_array().unwrap().len()), (1, 1), "{rep}");
    let riser = id(&rep["segments"][0]);
    let s = segment_recipe(doc.store().get(riser).unwrap()).unwrap();
    assert!((s.end[2] - (l1z + 6.3)).abs() < 1e-9 && (s.start[0] - s.end[0]).abs() < 1e-12, "{s:?}");
    assert_eq!(s.size.diameter, 0.2);
    let elbow = id(&rep["fittings"][0]);
    let net = reviera_core::mep::network(doc.store(), seg);
    assert!(net.contains(&elbow) && net.contains(&riser));
    // Carrying the pipe's system classification.
    assert_eq!(s.classification, segment_recipe(doc.store().get(seg).unwrap()).unwrap().classification);
    assert!(s.classification.is_some());

    // A run on from its top (up, then along), then Level 3 raised 500 mm: the riser lengthens by
    // 500 mm and the run moves with it, still connected.
    let top = s.end;
    let on = ok(&r, &mut doc, "pipe.create", json!({ "points": [[top[0], top[1], 6.3], [top[0], top[1], 7.3], [top[0], 4.0, 7.3]], "level": l1.0, "diameter": 0.2 }));
    let on_seg = id(&on["segments"][0]);
    assert!(on["connected_to"].as_array().unwrap().contains(&json!(riser.0)), "{on}");
    let before = segment_recipe(doc.store().get(riser).unwrap()).unwrap();
    let on_before = segment_recipe(doc.store().get(on_seg).unwrap()).unwrap();
    ok(&r, &mut doc, "level.set_elevation", json!({ "level_id": l3.0, "elevation": l1z + 6.5 }));
    let after = segment_recipe(doc.store().get(riser).unwrap()).unwrap();
    let len = |s: &reviera_core::mep_runs::SegmentRecipe| (s.end[2] - s.start[2]).abs();
    assert!((len(&after) - len(&before) - 0.5).abs() < 1e-9);
    let on_after = segment_recipe(doc.store().get(on_seg).unwrap()).unwrap();
    assert!((on_after.start[2] - on_before.start[2] - 0.5).abs() < 1e-9 && (on_after.end[2] - on_before.end[2] - 0.5).abs() < 1e-9);
    assert!(reviera_core::mep::network(doc.store(), seg).contains(&on_seg));
    // Undone, everything is back.
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(segment_recipe(doc.store().get(riser).unwrap()).unwrap(), before);

    // Symbols. A Level 1 plan (cut 1.2 m, up to Level 2): a rise; single-line at Coarse, two-line
    // at Fine.
    let coarse = symbols(doc.store(), l1z, l1z + 1.2, l1z + 3.0, DetailLevel::Coarse, 100.0);
    let s1: Vec<_> = coarse.iter().filter(|x| x.element == riser).collect();
    assert_eq!(s1.len(), 1);
    assert!(s1[0].kind == Kind::Rise && s1[0].single_line);
    let fine = symbols(doc.store(), l1z, l1z + 1.2, l1z + 3.0, DetailLevel::Fine, 100.0);
    let s2 = fine.iter().find(|x| x.element == riser).unwrap();
    assert!(s2.kind == Kind::Rise && !s2.single_line);
    // Level 2: it passes through; nothing. Level 3 cut above its top: a drop.
    assert!(!symbols(doc.store(), l1z + 3.0, l1z + 4.2, l1z + 6.0, DetailLevel::Fine, 100.0).iter().any(|x| x.element == riser));
    let s3 = symbols(doc.store(), l1z + 6.0, l1z + 7.2, l1z + 9.0, DetailLevel::Fine, 100.0);
    assert_eq!(s3.iter().find(|x| x.element == riser).map(|x| x.kind), Some(Kind::Drop));
    // A Level 3 plan cut below its top (at 0.2 m): it is cut, nothing extra.
    assert!(!symbols(doc.store(), l1z + 6.0, l1z + 6.2, l1z + 9.0, DetailLevel::Fine, 100.0).iter().any(|x| x.element == riser));

    // Unbound, the level moves alone.
    ok(&r, &mut doc, "mep_run.bind_riser", json!({ "element_id": riser.0 }));
    let fixed = segment_recipe(doc.store().get(riser).unwrap()).unwrap();
    ok(&r, &mut doc, "level.set_elevation", json!({ "level_id": l3.0, "elevation": l1z + 7.0 }));
    assert_eq!(segment_recipe(doc.store().get(riser).unwrap()).unwrap().end, fixed.end);
    let _ = l2;

    // Refusals create nothing: no elbow for 200 mm, a connector already joined, zero length.
    let t = doc.store().iter().find(|e| reviera_core::mep_runs::segment_type(e).is_some_and(|t| t.domain == reviera_core::mep::Domain::Pipe)).unwrap().id();
    ok(&r, &mut doc, "routing_preferences.set", json!({ "type_id": t.0, "routing": { "elbow": [{ "part": "elbow", "min": 0.0, "max": 0.1 }] } }));
    let other = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 10], [5, 10]], "level": l1.0, "diameter": 0.2 }));
    let o = id(&other["segments"][0]);
    let n = count(&doc);
    let e = run(&r, &mut doc, "mep_run.create_riser", json!({ "element_id": o.0, "connector_index": 1, "top_level": l3.0 })).unwrap_err();
    assert_eq!(e.message, "No Elbow in routing preferences for 200 mm");
    assert_eq!(count(&doc), n);
    assert_eq!(run(&r, &mut doc, "mep_run.create_riser", json!({ "element_id": seg.0, "connector_index": 1, "top_level": l3.0 })).unwrap_err().code, "riser.not_open");
    let e = run(&r, &mut doc, "mep_run.create_riser", json!({ "element_id": o.0, "connector_index": 0, "top_level": l1.0, "top_offset": 2.4 })).unwrap_err();
    assert_eq!(e.message, "Riser length is zero");
    assert_eq!(count(&doc), n);
}
