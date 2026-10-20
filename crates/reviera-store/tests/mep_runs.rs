// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ducts, pipes and fittings (MEP-DUCT-001, MEP-DUCT-002, MEP-DUCT-003, MEP-PIPE-001): an
//! L-shaped run's elbow, a riser's two elbows, taps and tees chosen by junction rows, Reapply
//! Type, a sloped pipe's Gradient, a run from an AHU joining its system, caps and resizing, and
//! the IFC of it all.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
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

fn count(doc: &Document, class: &str) -> usize {
    doc.store().iter().filter(|e| e.class == class).count()
}

#[test]
fn duct_runs_fittings_and_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // An L-shaped 600 × 300 run: one elbow, two segments.
    let rep = ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 0], [5, 0], [5, 4]], "level": l1.0, "middle_elevation": 3.0, "width": 0.6, "height": 0.3 }));
    assert_eq!(rep["segments"].as_array().unwrap().len(), 2);
    assert_eq!(rep["fittings"].as_array().unwrap().len(), 1);
    let elbow = id(&rep["fittings"][0]);
    assert_eq!(doc.store().get(elbow).unwrap().name.as_deref(), Some("Elbow"));
    // The elbow's two legs are connected to the two segments.
    let legs = ok(&r, &mut doc, "connector.list", json!({ "element_id": elbow.0 }));
    assert!(legs.as_array().unwrap().iter().all(|l| l["connected_to"].as_array().unwrap().len() == 1), "{legs}");
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": elbow.0 })).as_array().unwrap().len(), 3);

    // From 3.0 m to 3.6 m mid-run: two elbows and a vertical segment.
    let rep = ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 10, 3.0], [4, 10, 3.0], [4, 10, 3.6], [8, 10, 3.6]], "level": l1.0, "width": 0.6, "height": 0.3 }));
    assert_eq!(rep["fittings"].as_array().unwrap().len(), 2);
    let segs = rep["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 3);
    let riser = reviera_core::mep_runs::segment_recipe(doc.store().get(id(&segs[1])).unwrap()).unwrap();
    assert!((riser.start[0] - riser.end[0]).abs() < 1e-9 && riser.end[2] > riser.start[2], "{riser:?}");

    // Junction rows "Tap ≤ 300 mm, Tee > 300 mm": a 250 mm branch gets a tap, a 400 mm one a tee.
    let t = id(&ok(&r, &mut doc, "mep_segment_type.create", json!({
        "domain": "duct", "name": "Main Duct", "shape": "rectangular",
        "routing": { "junction": [{ "part": "tap", "max": 0.3 }, { "part": "tee", "min": 0.3 }], "elbow": [{ "part": "elbow" }], "cap": [{ "part": "cap" }] }
    })));
    let main = ok(&r, &mut doc, "duct.create", json!({ "type_id": t.0, "points": [[0, 20], [10, 20]], "level": l1.0, "width": 0.8, "height": 0.4 }));
    let host = id(&main["segments"][0]);
    let tap = ok(&r, &mut doc, "duct.create", json!({ "type_id": t.0, "points": [[3, 24], [3, 20]], "level": l1.0, "shape": "round", "diameter": 0.25 }));
    let tap_fitting = id(&tap["fittings"][0]);
    assert_eq!(doc.store().get(tap_fitting).unwrap().name.as_deref(), Some("Tap"));
    assert_eq!(reviera_core::mep_runs::segment_recipe(doc.store().get(host).unwrap()).unwrap().taps.len(), 1);
    let tee = ok(&r, &mut doc, "duct.create", json!({ "type_id": t.0, "points": [[7, 24], [7, 20]], "level": l1.0, "width": 0.4, "height": 0.3 }));
    let tee_fitting = id(&tee["fittings"][0]);
    assert_eq!(doc.store().get(tee_fitting).unwrap().name.as_deref(), Some("Tee"));
    // The main is now two segments round the tee, the tap kept on the first; all one network.
    assert_eq!(tee["segments"].as_array().unwrap().len(), 2, "{tee}");
    let net = ok(&r, &mut doc, "mep.network_of", json!({ "element_id": host.0 }));
    assert_eq!(net.as_array().unwrap().len(), 6, "{net}");
    // A branch that is not square to the main is refused.
    let e = run(&r, &mut doc, "duct.create", json!({ "type_id": t.0, "points": [[1, 24], [2, 20]], "level": l1.0, "width": 0.4, "height": 0.3 })).unwrap_err();
    assert_eq!(e.code, "mep_run.branch");

    // Without the tap row, Reapply Type removes the tap and warns.
    ok(&r, &mut doc, "routing_preferences.set", json!({ "type_id": t.0, "routing": {
        "junction": [{ "part": "tee", "min": 0.3 }], "elbow": [{ "part": "elbow" }], "cap": [{ "part": "cap" }]
    } }));
    let out = run(&r, &mut doc, "mep_run.reapply_type", json!({ "element_ids": [tap_fitting.0, tee_fitting.0] })).unwrap();
    assert_eq!(out.result["removed"], json!([tap_fitting.0]));
    assert!(out.warnings.iter().any(|w| w.message.contains("no junction") && w.message.contains("250 mm")), "{:?}", out.warnings);
    assert!(doc.store().get(tap_fitting).is_none());

    // No elbow row fits: the corner is left open, with a warning.
    let small = id(&ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "duct", "name": "Small Only", "shape": "round", "routing": { "elbow": [{ "part": "elbow", "max": 0.2 }] } })));
    let out = run(&r, &mut doc, "duct.create", json!({ "type_id": small.0, "points": [[0, 40], [4, 40], [4, 44]], "level": l1.0, "diameter": 0.3 })).unwrap();
    assert_eq!(out.result["fittings"], json!([]));
    assert!(out.warnings.iter().any(|w| w.message.contains("no elbow") && w.message.contains("Ø300")), "{:?}", out.warnings);

    // Resizing a segment resizes the elbow leg joined to it.
    let l_seg = id(&rep_segment(&doc, elbow));
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [l_seg.0], "width": 0.8 }));
    let f = reviera_core::mep_runs::fitting_recipe(doc.store().get(elbow).unwrap()).unwrap();
    assert!(f.legs.iter().any(|l| (l.size.width - 0.8).abs() < 1e-9), "{f:?}");

    // Moving the whole L-run keeps it connected; moving one segment alone separates it, with a
    // warning; a copy of the run is connected within itself.
    let run_ids: Vec<u64> = ok(&r, &mut doc, "mep.network_of", json!({ "element_id": elbow.0 })).as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let out = run(&r, &mut doc, "element.move", json!({ "element_ids": run_ids, "vector": [0, -5, 0] })).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": elbow.0 })).as_array().unwrap().len(), 3);
    let copies = ok(&r, &mut doc, "element.copy", json!({ "element_ids": run_ids, "vectors": [[0, -10, 0]] }));
    let copy0 = copies[0][0].as_u64().unwrap();
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": copy0 })).as_array().unwrap().len(), 3);
    let out = run(&r, &mut doc, "element.move", json!({ "element_ids": [l_seg.0], "vector": [0, 0, 1] })).unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "mep.connection_dropped"), "{:?}", out.warnings);
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": l_seg.0 })), json!([l_seg.0]));
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": l_seg.0 })).as_array().unwrap().len(), 3);

    // Caps on the L-run's open ends.
    let caps = ok(&r, &mut doc, "duct.cap_open_ends", json!({ "element_ids": [l_seg.0] }));
    assert_eq!(caps.as_array().unwrap().len(), 1);

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCDUCTSEGMENT"), count(&doc, "IfcDuctSegment"));
    assert_eq!(n("IFCDUCTFITTING"), count(&doc, "IfcDuctFitting"));
    assert!(n("IFCDUCTSEGMENTTYPE") >= 3);
    assert!(n("IFCRELCONNECTSPORTS") >= 10, "{}", n("IFCRELCONNECTSPORTS"));
    assert!(rows.iter().any(|r| r.class == "IFCDUCTFITTING" && r.attributes.contains(".BEND.")));
    assert!(rows.iter().any(|r| r.class == "IFCDUCTFITTING" && r.attributes.contains(".JUNCTION.")));
    assert!(rows.iter().any(|r| r.class == "IFCDUCTFITTING" && r.attributes.contains(".OBSTRUCTION.")));
    // Each segment has a body.
    let meshes = IfcModel::open(&path).unwrap().tessellate(1).unwrap();
    assert!(meshes.len() >= count(&doc, "IfcDuctSegment"));
}

/// A segment joined to the elbow.
fn rep_segment(doc: &Document, elbow: ElementId) -> Value {
    let p = reviera_core::mep::partners(doc.store());
    json!(p[&(elbow, 0)][0].0.0)
}

#[test]
fn pipes_slope_and_runs_join_systems() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipes.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A 50 mm pipe with an elbow, falling 1 %.
    let rep = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 0], [10, 0], [10, 5]], "level": l1.0, "middle_elevation": 2.0, "diameter": 0.05, "slope": 0.01, "system_type_id": "sanitary" }));
    assert_eq!(rep["fittings"].as_array().unwrap().len(), 1);
    let first = reviera_core::mep_runs::segment_recipe(doc.store().get(id(&rep["segments"][0])).unwrap()).unwrap();
    let last = reviera_core::mep_runs::segment_recipe(doc.store().get(id(&rep["segments"][1])).unwrap()).unwrap();
    assert!((first.start[2] - 2.0).abs() < 1e-9 && (last.end[2] - (2.0 - 0.15)).abs() < 1e-9, "{first:?} {last:?}");
    // Rectangular pipes are refused; a duct cannot carry sanitary.
    assert_eq!(run(&r, &mut doc, "pipe.create", json!({ "points": [[0, 3], [2, 3]], "level": l1.0, "shape": "rectangular", "width": 0.1, "height": 0.1 })).unwrap_err().code, "mep_run.shape");
    assert_eq!(run(&r, &mut doc, "duct.create", json!({ "points": [[0, 3], [2, 3]], "level": l1.0, "shape": "round", "diameter": 0.2, "system_type_id": "sanitary" })).unwrap_err().code, "mep_run.classification");

    // A supply duct drawn from an AHU's supply connector connects to it and joins its system.
    let types = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
    let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": types["Air Handling Unit"], "point": [0, 30], "level": l1.0 })));
    let diffuser = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": types["Supply Diffuser 600"], "point": [20, 30], "level": l1.0, "offset": 2.7 })));
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [diffuser.0] })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
    let duct = ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 30, 0.9], [6, 30, 0.9]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
    assert_eq!(duct["connected_to"], json!([ahu.0]));
    assert_eq!(duct["system"], sys.0);
    let s = &ok(&r, &mut doc, "system.list", json!({}))[0];
    assert!(s["elements"].as_array().unwrap().contains(&duct["segments"][0]));

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCPIPESEGMENT"));
    assert!(rows.iter().any(|r| r.class == "IFCPIPEFITTING" && r.attributes.contains(".BEND.")));
    let gradient = rows.iter().find(|r| r.class == "IFCPROPERTYSINGLEVALUE" && r.attributes.contains("'Gradient'")).expect("Gradient");
    assert!(gradient.attributes.contains("0.01"), "{}", gradient.attributes);
}
