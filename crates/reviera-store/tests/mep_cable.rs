// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Cable tray and conduit (MEP-ELEC-016, MEP-ELEC-017): a ladder tray's bend radius, a tee placed
//! along a tray, fittings refused on a type without fittings and its curved bends, "+" and "−",
//! conduit standards, splitting with a coupling, a junction box, parallel conduits with
//! concentric bends, and the IFC of it all.

use reviera_core::mep_runs::{fitting_recipe, segment_recipe};
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

/// How many connections an element has.
fn links(doc: &Document, e: ElementId) -> usize {
    reviera_core::mep::partners(doc.store()).iter().filter(|((x, _), v)| *x == e && !v.is_empty()).count()
}

#[test]
fn cable_tray_bends_fittings_and_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tray.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A 300 × 100 ladder tray around a corner: its bend's radius is the width × 1.
    let out = ok(&r, &mut doc, "cable_tray.create", json!({ "points": [[0, 0], [6, 0], [6, 4]], "level": l1.0, "width": 0.3, "height": 0.1 }));
    let segs = ids(&out["segments"]);
    let bend = id(&out["fittings"][0]);
    let f = fitting_recipe(doc.store().get(bend).unwrap()).unwrap();
    assert_eq!(f.radius, Some(0.3));
    assert!((f.legs[0].length - 0.3).abs() < 1e-9, "a 90° bend's legs are its radius");
    let e = doc.store().get(bend).unwrap();
    assert_eq!((e.class.as_str(), e.param("@PredefinedType").and_then(Value::as_str)), ("IfcCableCarrierFitting", Some("BEND")));
    let s = doc.store().get(segs[0]).unwrap();
    assert_eq!((s.class.as_str(), s.param("@PredefinedType").and_then(Value::as_str)), ("IfcCableCarrierSegment", Some("CABLELADDERSEGMENT")));
    assert_eq!(links(&doc, bend), 2);
    // A radius below half the width is refused.
    let e = run(&r, &mut doc, "cable_tray.create", json!({ "points": [[0, 10], [5, 10], [5, 14]], "level": l1.0, "width": 0.3, "height": 0.1, "bend_radius": 0.1 })).unwrap_err();
    assert_eq!(e.code, "mep_run.bend_radius");
    // Trays are rectangular.
    assert_eq!(run(&r, &mut doc, "cable_tray.create", json!({ "points": [[0, 20], [5, 20]], "level": l1.0, "shape": "round", "diameter": 0.1 })).unwrap_err().code, "mep_run.shape");

    // A tee placed along the first 300 mm tray splits it; all three are connected.
    let tee = ok(&r, &mut doc, "cable_tray_fitting.place", json!({ "part": "tee", "point": [3.0, 0.0] }));
    let (tf, half) = (id(&tee["fitting"]), id(&tee["split"]));
    assert_eq!(fitting_recipe(doc.store().get(tf).unwrap()).unwrap().legs.len(), 3);
    assert_eq!((links(&doc, tf), links(&doc, segs[0]) >= 1, links(&doc, half)), (2, true, 2), "the tee's branch is open");
    assert_eq!(doc.store().get(tf).unwrap().param("@PredefinedType").and_then(Value::as_str), Some("TEE"));

    // "+" on the bend: a tee with an open third leg; "−" takes it back.
    assert_eq!(ok(&r, &mut doc, "mep_fitting.upgrade", json!({ "fitting_id": bend.0 })), "tee");
    let f = fitting_recipe(doc.store().get(bend).unwrap()).unwrap();
    assert_eq!(f.legs.len(), 3);
    assert_eq!(links(&doc, bend), 2, "the new leg is open, the others still connected");
    assert_eq!(ok(&r, &mut doc, "mep_fitting.downgrade", json!({ "fitting_id": bend.0 })), "elbow");
    assert_eq!(links(&doc, bend), 2);
    assert_eq!(run(&r, &mut doc, "mep_fitting.downgrade", json!({ "fitting_id": bend.0 })).unwrap_err().code, "mep_fitting.downgrade");

    // A channel tray type without fittings: its bend is a curved segment, and fittings are refused.
    let t = ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "cable_tray", "name": "Channel Bent", "tray_kind": "channel", "with_fittings": false, "bend_radius_multiplier": 2.0 }));
    let out = ok(&r, &mut doc, "cable_tray.create", json!({ "type_id": t, "points": [[0, 30], [5, 30], [5, 34]], "level": l1.0, "width": 0.2, "height": 0.06 }));
    assert_eq!(out["fittings"].as_array().unwrap().len(), 0);
    let curved: Vec<ElementId> = ids(&out["segments"]).into_iter().filter(|s| segment_recipe(doc.store().get(*s).unwrap()).unwrap().corner.is_some()).collect();
    assert_eq!(curved.len(), 1);
    let c = segment_recipe(doc.store().get(curved[0]).unwrap()).unwrap();
    assert!((reviera_core::mep_runs::arc_radius(c.start, c.corner.unwrap(), c.end).unwrap() - 0.4).abs() < 1e-9);
    assert_eq!(links(&doc, curved[0]), 2);
    assert_eq!(doc.store().get(curved[0]).unwrap().param("@PredefinedType").and_then(Value::as_str), Some("CABLETRAYSEGMENT"));
    let e = run(&r, &mut doc, "cable_tray_fitting.place", json!({ "part": "tee", "point": [2.0, 30.0] })).unwrap_err();
    assert_eq!(e.code, "mep_fitting.no_fittings");
    assert!(e.message.starts_with("Channel Bent is a cable tray type without fittings"), "{}", e.message);

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |c: &str| rows.iter().filter(|r| r.class == c).count();
    assert!(n("IFCCABLECARRIERSEGMENT") >= 6 && n("IFCCABLECARRIERFITTING") == 2, "{} {}", n("IFCCABLECARRIERSEGMENT"), n("IFCCABLECARRIERFITTING"));
    assert!(rows.iter().any(|r| r.class == "IFCDISTRIBUTIONPORT" && r.attributes.contains(".CABLECARRIER.")));
}

#[test]
fn conduit_standards_boxes_and_parallel_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conduit.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A standard of 16, 20 and 25 mm: 32 mm is refused.
    let std = ok(&r, &mut doc, "conduit_standard.create", json!({ "name": "Small PVC", "sizes": [{ "diameter": 0.016 }, { "diameter": 0.020 }, { "diameter": 0.025 }] }));
    let t = ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "conduit", "name": "PVC", "standard": std }));
    let e = run(&r, &mut doc, "conduit.create", json!({ "type_id": t, "points": [[0, 0], [5, 0]], "level": l1.0, "diameter": 0.032 })).unwrap_err();
    assert_eq!(e.code, "conduit.size");
    assert!(e.message.contains("16, 20, 25 mm"), "{}", e.message);
    // 25 mm is drawn, its bend at 6 × the diameter.
    let out = ok(&r, &mut doc, "conduit.create", json!({ "type_id": t, "points": [[0, 0], [5, 0], [5, 3]], "level": l1.0, "diameter": 0.025 }));
    let segs = ids(&out["segments"]);
    let bend = fitting_recipe(doc.store().get(id(&out["fittings"][0])).unwrap()).unwrap();
    assert!((bend.radius.unwrap() - 0.15).abs() < 1e-9);
    // EMT: the standard's own radius (1" is 5.75").
    let emt = ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "conduit", "name": "EMT", "standard": "emt" }));
    let out = ok(&r, &mut doc, "conduit.create", json!({ "type_id": emt, "points": [[0, 40], [5, 40], [5, 43]], "level": l1.0, "diameter": 0.0295 }));
    let b = fitting_recipe(doc.store().get(id(&out["fittings"][0])).unwrap()).unwrap();
    assert!((b.radius.unwrap() - 5.75 * 0.0254).abs() < 1e-9);

    // A junction box along the 25 mm conduit splits it and connects to both parts.
    let bx = ok(&r, &mut doc, "conduit_fitting.place", json!({ "part": "box", "host_id": segs[0].0, "point": [2.5, 0.0] }));
    let (b, half) = (id(&bx["fitting"]), id(&bx["split"]));
    assert_eq!(doc.store().get(b).unwrap().class, "IfcJunctionBox");
    assert_eq!((links(&doc, b), links(&doc, segs[0]) >= 1, links(&doc, half)), (2, true, 2));
    assert_eq!(run(&r, &mut doc, "mep_fitting.downgrade", json!({ "fitting_id": b.0 })).unwrap_err().code, "mep_fitting.downgrade");
    // A split adds a coupling (the conduit type's union row).
    let sp = ok(&r, &mut doc, "conduit.split", json!({ "id": half.0, "point": [3.5, 0.0] }));
    assert!(!sp["coupling"].is_null());
    assert_eq!(links(&doc, id(&sp["coupling"])), 2);

    // Parallel conduits: refused concentric on a type with fittings.
    let e = run(&r, &mut doc, "conduit.create_parallel", json!({ "run_ids": [out["segments"][0]], "h_count": 3, "h_offset": 0.1, "bend": "concentric" })).unwrap_err();
    assert_eq!(e.code, "conduit.concentric");
    // A type without fittings, bent at 6 × 25 mm = 150 mm; three conduits around a right turn
    // offset 50 mm to its outside (left of a run turning right): radii 150, 200, 250 mm.
    let bent = ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "conduit", "name": "Bent PVC", "standard": std, "with_fittings": false }));
    let run0 = ok(&r, &mut doc, "conduit.create", json!({ "type_id": bent, "points": [[0, 20], [5, 20], [5, 16]], "level": l1.0, "diameter": 0.025 }));
    let made = ok(&r, &mut doc, "conduit.create_parallel", json!({ "run_ids": [run0["segments"][0]], "h_count": 3, "h_offset": 0.05, "bend": "concentric" }));
    let mut radii: Vec<f64> = ids(&run0["segments"])
        .into_iter()
        .chain(ids(&made))
        .filter_map(|s| {
            let c = segment_recipe(doc.store().get(s)?)?;
            reviera_core::mep_runs::arc_radius(c.start, c.corner?, c.end)
        })
        .collect();
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 3, "{radii:?}");
    for (r, want) in radii.iter().zip([0.15, 0.20, 0.25]) {
        assert!((r - want).abs() < 1e-9, "{radii:?}");
    }
    // Same-radius parallels of the fitted run.
    let same = ok(&r, &mut doc, "conduit.create_parallel", json!({ "run_ids": [segs[0].0], "h_count": 2, "h_offset": -0.08 }));
    assert!(!same.as_array().unwrap().is_empty());

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let jb = rows.iter().find(|r| r.class == "IFCJUNCTIONBOX").expect("the box");
    assert!(jb.attributes.contains(".POWER."), "{}", jb.attributes);
    assert!(rows.iter().any(|r| r.class == "IFCCABLECARRIERSEGMENT" && r.attributes.contains(".CONDUITSEGMENT.")));
    assert!(rows.iter().any(|r| r.class == "IFCCABLECARRIERFITTING" && r.attributes.contains("'COUPLING'")));
    // The box's two ports are connected.
    let nests: Vec<_> = rows.iter().filter(|r| r.class == "IFCRELNESTS" && r.attributes.contains(&format!("#{}", jb.id.0))).collect();
    assert!(!nests.is_empty());
}
