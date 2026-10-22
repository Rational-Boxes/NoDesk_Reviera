// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The justification editor (MEP-DUCT-006): a run drawn from a 600 mm duct's end at 400 mm gets
//! a transition; justified Bottom-Left its bottoms and left sides are flush; a run with a tee and
//! its branch is refused; justified Bottom-Centre, a segment's new height keeps its bottom
//! flush; the end control keeps the last segment in place.

use reviera_core::mep_runs::{fitting_recipe, segment_recipe};
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(|x| ElementId(x.as_u64().unwrap())).collect()
}

/// The lowest and highest of an element's solids along an axis.
fn span(doc: &Document, id: ElementId, axis: usize) -> (f64, f64) {
    let all = reviera_core::mep_runs::all(doc.store());
    let (_, _, _, solids) = all.iter().find(|a| a.0 == id).expect("drawn");
    let vs: Vec<f64> = solids.iter().flat_map(|s| s.points.iter().map(|p| p[axis])).collect();
    (vs.iter().copied().fold(f64::MAX, f64::min), vs.iter().copied().fold(f64::MIN, f64::max))
}

#[test]
fn justify_a_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("justify.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let duct = |doc: &mut Document, pts: Value, w: f64, h: f64| {
        ok(&r, doc, "duct.create", json!({ "points": pts, "level": l1.0, "shape": "rectangular", "width": w, "height": h }))
    };

    // 600 × 400, then 400 × 300 from its end (a transition), then 600 × 400 again.
    let a = duct(&mut doc, json!([[0, 0, 3], [4, 0, 3]]), 0.6, 0.4);
    let b = duct(&mut doc, json!([[4, 0, 3], [8, 0, 3]]), 0.4, 0.3);
    let c = duct(&mut doc, json!([[8, 0, 3], [12, 0, 3]]), 0.6, 0.4);
    let t1 = ids(&b["fittings"]);
    assert_eq!(t1.len(), 1, "a transition: {b}");
    let tf = fitting_recipe(doc.store().get(t1[0]).unwrap()).unwrap();
    assert_eq!(tf.part, reviera_core::mep_runs::Part::Transition);
    let partners = reviera_core::mep::partners(doc.store());
    let (sa, sb, sc) = (ids(&a["segments"])[0], ids(&b["segments"])[0], ids(&c["segments"])[0]);
    assert!(partners.get(&(t1[0], 0)).is_some_and(|v| v[0].0 == sa), "joined to the 600 duct");
    assert!(partners.get(&(t1[0], 1)).is_some_and(|v| v[0].0 == sb));
    let mut run_ids: Vec<ElementId> = [&a, &b, &c].iter().flat_map(|x| ids(&x["segments"]).into_iter().chain(ids(&x["fittings"]))).collect();
    run_ids.sort();

    // Bottom-Left from the start: bottoms at 2.8 m, left sides (+y travelling +x) at 0.3 m.
    let rep = ok(&r, &mut doc, "mep_run.justify", json!({ "element_ids": run_ids, "alignment": "BL" }));
    assert_eq!(rep["control"], json!(sa.0));
    for id in &run_ids {
        let (z0, _) = span(&doc, *id, 2);
        let (_, y1) = span(&doc, *id, 1);
        assert!((z0 - 2.8).abs() < 1e-9, "bottom of {id:?}: {z0}");
        assert!((y1 - 0.3).abs() < 1e-9, "left of {id:?}: {y1}");
    }
    let s = segment_recipe(doc.store().get(sb).unwrap()).unwrap();
    assert!((s.start[2] - 2.95).abs() < 1e-9 && (s.start[1] - 0.1).abs() < 1e-9, "{s:?}");
    // Still joined; the transition is eccentric.
    let partners = reviera_core::mep::partners(doc.store());
    assert!(partners.get(&(t1[0], 1)).is_some_and(|v| v[0].0 == sb));
    let pb = reviera_core::mep::port(doc.store(), sb, 0).unwrap();
    let pt = reviera_core::mep::port(doc.store(), t1[0], 1).unwrap();
    assert!((0..3).all(|k| (pb.position[k] - pt.position[k]).abs() < 1e-9), "{pb:?} {pt:?}");

    // Bottom-Centre; then the middle segment 200 mm high keeps its bottom at 2.8 m.
    ok(&r, &mut doc, "mep_run.justify", json!({ "element_ids": run_ids, "alignment": "BC" }));
    let s = segment_recipe(doc.store().get(sb).unwrap()).unwrap();
    assert!(s.start[1].abs() < 1e-9 && (s.start[2] - 2.95).abs() < 1e-9, "{s:?}");
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [sb.0], "width": 0.4, "height": 0.2 }));
    for id in &run_ids {
        assert!((span(&doc, *id, 2).0 - 2.8).abs() < 1e-9, "bottom of {id:?} after the resize");
    }
    assert!((span(&doc, sb, 2).1 - 3.0).abs() < 1e-9);

    // Top-Centre from the end: the last segment stays, the first moves.
    let before = segment_recipe(doc.store().get(sc).unwrap()).unwrap();
    let rep = ok(&r, &mut doc, "mep_run.justify", json!({ "element_ids": run_ids, "alignment": "TC", "control_end": "end" }));
    assert_eq!(rep["control"], json!(sc.0));
    assert_eq!(segment_recipe(doc.store().get(sc).unwrap()).unwrap().start, before.start);
    for id in &run_ids {
        assert!((span(&doc, *id, 2).1 - 3.2).abs() < 1e-9, "top of {id:?}");
    }

    // A main with a tee and its branch: refused.
    let main = ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 10, 3], [8, 10, 3]], "level": l1.0, "shape": "round", "diameter": 0.4 }));
    let branch = ok(&r, &mut doc, "duct.create", json!({ "points": [[4, 13, 3], [4, 10, 3]], "level": l1.0, "shape": "round", "diameter": 0.2 }));
    let all: Vec<ElementId> = [&main, &branch].iter().flat_map(|x| ids(&x["segments"]).into_iter().chain(ids(&x["fittings"]))).collect();
    let e = run(&r, &mut doc, "mep_run.justify", json!({ "element_ids": all, "alignment": "BC" })).unwrap_err();
    assert_eq!(&*e.message, "Justify needs a single run without branches");
}
