// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Flexible duct (MEP-DUCT-005): a flex duct drawn from a duct's centreline to a diffuser gets
//! a tap from the routing preferences and connects at both ends; its exported length is the
//! spline's; Convert to Flex with a 1.5 m limit converts only the short final segments; a bend
//! tighter than the type's minimum is warned about; flex friction uses the loss multiplier.

use reviera_core::mep_flex::ConvertReport;
use reviera_core::mep_network::NetworkResult;
use reviera_core::mep_runs::{segment_length, segment_recipe};
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

#[test]
fn flex_from_a_duct_to_a_diffuser_and_convert() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flex.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Supply Diffuser 600"] }));
    let diffuser = |doc: &mut Document, x: f64, y: f64, offset: f64| {
        id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [x, y], "level": l1.0, "offset": offset })))
    };
    // A 400 mm main at 3.6 m.
    let main = id(&ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 0, 3.6], [12, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }))["segments"][0]);

    // From the main's centreline to a diffuser: a tap on the main, connected at both ends.
    let d1 = diffuser(&mut doc, 3.0, 2.5, 2.7);
    let rep = ok(&r, &mut doc, "flex_duct.create", json!({ "points": [[3, 0, 3.6], [3, 1.5, 3.3], [3, 2.5, 2.85]], "level": l1.0 }));
    let flex = id(&rep["segments"][0]);
    let tap = id(&rep["fittings"][0]);
    let e = doc.store().get(flex).unwrap();
    assert_eq!(e.class, "IfcDuctSegment");
    assert_eq!(e.param("@PredefinedType").and_then(Value::as_str), Some("FLEXIBLESEGMENT"));
    let s = segment_recipe(e).unwrap();
    assert!((s.size.diameter - 0.25).abs() < 1e-9, "the diffuser's size: {:?}", s.size);
    assert_eq!(s.classification, segment_recipe(doc.store().get(main).unwrap()).unwrap().classification);
    let f = s.flex.as_ref().unwrap();
    assert_eq!(f.points.len(), 3, "passes through every point");
    assert!((f.points[1][1] - 1.5).abs() < 1e-9);
    let tf = reviera_core::mep_runs::fitting_recipe(doc.store().get(tap).unwrap()).unwrap();
    assert_eq!(tf.part, reviera_core::mep_runs::Part::Tap);
    let partners = reviera_core::mep::partners(doc.store());
    let to = |k: usize| partners.get(&(flex, k)).cloned().unwrap_or_default().iter().map(|p| p.0).collect::<Vec<_>>();
    assert_eq!(to(0), vec![tap]);
    assert_eq!(to(1), vec![d1]);
    assert!(partners.get(&(main, 2)).is_some_and(|v| v[0].0 == tap), "the main's tap port");
    // The spline is longer than the straight line between its ends.
    let length = segment_length(&s);
    let chord = f.points.windows(2).map(|w| (0..3).map(|k| (w[1][k] - w[0][k]).powi(2)).sum::<f64>().sqrt()).sum::<f64>();
    assert!(length >= chord - 1e-9 && length < chord * 1.2, "{length} {chord}");

    // Exported: IfcDuctSegment FLEXIBLESEGMENT with Qto_DuctSegmentBaseQuantities.Length the
    // spline's length.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let guid = doc.store().get(flex).unwrap().guid().to_string();
    let row = rows.iter().find(|r| r.class == "IFCDUCTSEGMENT" && r.attributes.contains(&guid)).expect("the flex");
    assert!(row.attributes.contains(".FLEXIBLESEGMENT."), "{}", row.attributes);
    assert!(rows.iter().any(|r| r.class == "IFCELEMENTQUANTITY" && r.attributes.contains("Qto_DuctSegmentBaseQuantities")));
    let qs: Vec<_> = rows.iter().filter(|r| r.class == "IFCQUANTITYLENGTH").collect();
    let q = qs.iter().find(|r| r.attributes.starts_with("('Length'")).expect("the length");
    let v: f64 = q.attributes.split(',').nth(3).unwrap().trim().parse().unwrap();
    let in_file = if v > 100.0 { v / 1000.0 } else { v };
    assert!((in_file - length).abs() < 0.001, "{v} {length}");

    // Convert to Flex, 1.5 m: a branch whose drop is 0.55 m converts; one of 2.25 m does not.
    let d2 = diffuser(&mut doc, 6.0, 3.0, 2.7);
    let d3 = diffuser(&mut doc, 9.0, 3.0, 1.0);
    for (x, z) in [(6.0, 2.85), (9.0, 1.15)] {
        ok(&r, &mut doc, "duct.create", json!({ "points": [[x, 0, 3.6], [x, 3, 3.6], [x, 3, z]], "level": l1.0, "shape": "round", "diameter": 0.25 }));
    }
    let rep: ConvertReport = serde_json::from_value(ok(&r, &mut doc, "duct.convert_to_flex", json!({ "terminal_ids": [d2.0, d3.0], "max_length": 1.5 }))).unwrap();
    assert_eq!(rep.ids.len(), 1, "{rep:?}");
    assert_eq!(rep.skipped.len(), 1);
    assert_eq!(rep.skipped[0].terminal, d3);
    assert!(rep.skipped[0].reason.contains("2.25 m"), "{}", rep.skipped[0].reason);
    let made = segment_recipe(doc.store().get(rep.ids[0]).unwrap()).unwrap();
    assert!(made.flex.is_some() && (made.size.diameter - 0.25).abs() < 1e-9);
    let partners = reviera_core::mep::partners(doc.store());
    assert!(partners.get(&(rep.ids[0], 1)).is_some_and(|v| v[0].0 == d2));
    let up = partners.get(&(rep.ids[0], 0)).expect("joined to the elbow above")[0].0;
    assert_eq!(reviera_core::mep_runs::fitting_recipe(doc.store().get(up).unwrap()).unwrap().part, reviera_core::mep_runs::Part::Elbow);
    // Converting again: already flex.
    let again: ConvertReport = serde_json::from_value(ok(&r, &mut doc, "duct.convert_to_flex", json!({ "terminal_ids": [d2.0], "max_length": 1.5 }))).unwrap();
    assert_eq!(again.skipped[0].reason, "already flex");
}

#[test]
fn bend_radius_and_types() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flex2.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A sharp hairpin in a 300 mm flex: drawn, with a warning.
    let o = run(&r, &mut doc, "flex_duct.create", json!({ "points": [[0, 0], [0.3, 0.05], [0, 0.1]], "level": l1.0, "diameter": 0.3 })).unwrap();
    assert!(o.warnings.iter().any(|w| w.id == "flex_duct.bend_radius"), "{:?}", o.warnings);
    // A gentle one: no warning.
    let o = run(&r, &mut doc, "flex_duct.create", json!({ "points": [[0, 3], [2, 3.2], [4, 3]], "level": l1.0, "diameter": 0.3 })).unwrap();
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    // A flexible type cannot draw rigid duct; a rigid one cannot draw flex.
    let flex_type = doc.store().iter().find(|e| e.name.as_deref() == Some("Round Flex Duct")).unwrap().id();
    assert_eq!(run(&r, &mut doc, "duct.create", json!({ "type_id": flex_type.0, "points": [[0, 6], [3, 6]], "level": l1.0, "diameter": 0.3 })).unwrap_err().code, "mep_segment_type.flexible");
    let rigid = id(&ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "duct", "name": "Spiral", "shape": "round" })));
    assert_eq!(run(&r, &mut doc, "flex_duct.create", json!({ "type_id": rigid.0, "points": [[0, 6], [3, 6]], "level": l1.0, "diameter": 0.3 })).unwrap_err().code, "mep_segment_type.flexible");
    // Starting on a duct whose routing has no tap row is refused, naming the size.
    ok(&r, &mut doc, "routing_preferences.set", json!({ "type_id": rigid.0, "routing": { "junction": [{ "part": "tee" }] } }));
    ok(&r, &mut doc, "duct.create", json!({ "type_id": rigid.0, "points": [[0, 9], [6, 9]], "level": l1.0, "diameter": 0.4 }));
    let e = run(&r, &mut doc, "flex_duct.create", json!({ "points": [[3, 9], [3, 11]], "level": l1.0, "diameter": 0.2 })).unwrap_err();
    assert_eq!(e.code, "flex_duct.no_tap");
    assert!(e.message.contains("Ø200"), "{}", e.message);
}

/// The loss multiplier scales a flex duct's friction loss: the same flex on a type with twice
/// the multiplier loses twice as much.
#[test]
fn loss_multiplier() {
    let flex_loss = |multiplier: f64| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flex3.ifc");
        let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
        let r = registry();
        let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
        let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
        let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [0, 0], "level": l1.0 })));
        let diff = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [6.0, 3.0], "level": l1.0, "offset": 2.7 })));
        let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [diff.0] })));
        ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
        ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 0, 0.9], [3, 0, 0.9], [3, 0, 3.6], [9, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
        let ty = id(&ok(&r, &mut doc, "mep_segment_type.create", json!({ "domain": "duct", "name": "Flex", "shape": "round", "flexible": true, "roughness": 0.003, "loss_multiplier": multiplier })));
        let rep = ok(&r, &mut doc, "flex_duct.create", json!({ "type_id": ty.0, "points": [[6, 0, 3.6], [6, 3, 2.85]], "level": l1.0 }));
        let flex = id(&rep["segments"][0]);
        ok(&r, &mut doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
        let n: NetworkResult = serde_json::from_value(ok(&r, &mut doc, "analytical_segment.list", json!({ "system_id": sys.0 }))).unwrap();
        n.segments.iter().find(|a| a.element == flex).expect("the flex is in the network").loss.expect("a loss")
    };
    let (one, two) = (flex_loss(1.0), flex_loss(2.0));
    assert!(one > 0.0);
    assert!((two / one - 2.0).abs() < 1e-9, "{one} {two}");
}
