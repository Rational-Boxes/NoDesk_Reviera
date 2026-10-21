// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct accessories, insulation and lining (MEP-DUCT-004): a fire damper splits a 400 mm duct
//! and joins both parts; insulation exports as an IfcCovering covering the duct, continues over
//! the damper, follows a resize and goes with its host; Heal rejoins the duct; lining narrows the
//! section the pressure-drop calculation uses; a silencer too long for its straight is refused.

use reviera_core::mep_coverings::CoveringInfo;
use reviera_core::mep_network::NetworkResult;
use reviera_core::mep_runs::segment_recipe;
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

fn coverings(r: &CommandRegistry, doc: &mut Document) -> Vec<CoveringInfo> {
    serde_json::from_value(ok(r, doc, "mep_covering.list", json!({}))).unwrap()
}

/// The extent of an element's solids across `axis` (0: x, 1: y, 2: z).
fn extent(doc: &Document, id: ElementId, axis: usize) -> f64 {
    let all = reviera_core::mep_runs::all(doc.store());
    let (_, _, _, solids) = all.iter().find(|a| a.0 == id).expect("drawn");
    let vs: Vec<f64> = solids.iter().flat_map(|s| s.points.iter().map(|p| p[axis])).collect();
    vs.iter().copied().fold(f64::MIN, f64::max) - vs.iter().copied().fold(f64::MAX, f64::min)
}

#[test]
fn dampers_insulation_and_lining() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accessories.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let duct = |doc: &mut Document, pts: Value, size: Value| {
        let mut a = json!({ "points": pts, "level": l1.0 });
        a.as_object_mut().unwrap().extend(size.as_object().unwrap().clone());
        ok(&r, doc, "duct.create", a)
    };

    // A 400 mm round duct, insulated with 50 mm.
    let d = id(&duct(&mut doc, json!([[0, 0, 3], [5, 0, 3]]), json!({ "shape": "round", "diameter": 0.4 }))["segments"][0]);
    let ins = ok(&r, &mut doc, "mep_insulation.add", json!({ "element_ids": [d.0], "thickness": 0.05 }));
    assert_eq!(ins.as_array().unwrap().len(), 1);
    assert!((extent(&doc, id(&ins[0]), 2) - 0.5).abs() < 1e-9);

    // A fire damper splits it; both parts connect through it.
    let rep = ok(&r, &mut doc, "duct_accessory.place", json!({ "kind": "fire_damper", "duct_id": d.0, "point": [2.5, 0] }));
    let (damper, second) = (id(&rep["accessory"]), id(&rep["split"]));
    let e = doc.store().get(damper).unwrap();
    assert_eq!(e.class, "IfcDamper");
    assert_eq!(e.param("@PredefinedType").and_then(Value::as_str), Some("FIREDAMPER"));
    let partners = reviera_core::mep::partners(doc.store());
    let joined = |k: usize| partners.get(&(damper, k)).cloned().unwrap_or_default();
    assert_eq!(joined(0).iter().map(|p| p.0).collect::<Vec<_>>(), vec![d]);
    assert_eq!(joined(1).iter().map(|p| p.0).collect::<Vec<_>>(), vec![second]);
    let (s1, s2) = (segment_recipe(doc.store().get(d).unwrap()).unwrap(), segment_recipe(doc.store().get(second).unwrap()).unwrap());
    assert!((s1.end[0] - 2.35).abs() < 1e-9 && (s2.start[0] - 2.65).abs() < 1e-9, "{s1:?} {s2:?}");
    // The insulation carries on over the damper and the second part.
    let hosts: Vec<ElementId> = coverings(&r, &mut doc).iter().map(|c| c.host).collect();
    assert!(hosts.contains(&d) && hosts.contains(&second) && hosts.contains(&damper), "{hosts:?}");

    // IFC: an IfcCovering (INSULATION) covering the duct, its thickness as a quantity.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let guid = doc.store().get(d).unwrap().guid().to_string();
    let duct_row = rows.iter().find(|r| r.class == "IFCDUCTSEGMENT" && r.attributes.contains(&guid)).expect("the duct");
    let cover_guid = doc.store().get(id(&ins[0])).unwrap().guid().to_string();
    let cover = rows.iter().find(|r| r.class == "IFCCOVERING" && r.attributes.contains(&cover_guid)).expect("the covering");
    assert!(cover.attributes.contains(".INSULATION."), "{}", cover.attributes);
    assert!(rows.iter().any(|r| r.class == "IFCRELCOVERSBLDGELEMENTS" && r.attributes.contains(&format!("#{},(#{})", duct_row.id.0, cover.id.0))), "no IfcRelCoversBldgElements");
    assert!(rows.iter().any(|r| r.class == "IFCELEMENTQUANTITY" && r.attributes.contains("Qto_CoveringBaseQuantities")));
    assert!(rows.iter().any(|r| r.class == "IFCDAMPER" && r.attributes.contains(".FIREDAMPER.")));

    // Deleting the damper takes its insulation and leaves both ends open; Heal rejoins them.
    let gone = ok(&r, &mut doc, "element.delete", json!({ "element_ids": [damper.0] }));
    assert_eq!(gone.as_array().unwrap().len(), 2, "{gone}");
    let healed = id(&ok(&r, &mut doc, "mep_segment.heal", json!({ "first_id": d.0, "second_id": second.0 })));
    assert_eq!(healed, d);
    let s = segment_recipe(doc.store().get(d).unwrap()).unwrap();
    assert!(s.start[0].abs() < 1e-9 && (s.end[0] - 5.0).abs() < 1e-9, "{s:?}");
    assert!(doc.store().get(second).is_none());
    assert_eq!(coverings(&r, &mut doc).len(), 1);
    project.save(&doc).unwrap();

    // A 600 × 300 duct, insulated 25 mm, resized to 800 × 300: the insulation follows.
    let rect = id(&duct(&mut doc, json!([[0, 4, 3], [5, 4, 3]]), json!({ "shape": "rectangular", "width": 0.6, "height": 0.3 }))["segments"][0]);
    let rins = id(&ok(&r, &mut doc, "mep_insulation.add", json!({ "element_ids": [rect.0] }))[0]);
    assert!((extent(&doc, rins, 1) - 0.65).abs() < 1e-9);
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [rect.0], "width": 0.8, "height": 0.3 }));
    assert!((extent(&doc, rins, 1) - 0.85).abs() < 1e-9);
    assert!((extent(&doc, rins, 2) - 0.35).abs() < 1e-9);
    // Edit: 40 mm; Remove.
    ok(&r, &mut doc, "mep_insulation.set", json!({ "element_ids": [rect.0], "thickness": 0.04 }));
    assert!((extent(&doc, rins, 1) - 0.88).abs() < 1e-9);
    assert_eq!(ok(&r, &mut doc, "mep_insulation.remove", json!({ "element_ids": [rect.0] })), json!([rins.0]));

    // 25 mm lining in a 400 × 200 duct: 350 × 150 free; too thick is refused; not on a pipe's
    // accessory or a pipe.
    let lined = id(&duct(&mut doc, json!([[0, 8, 3], [5, 8, 3]]), json!({ "shape": "rectangular", "width": 0.4, "height": 0.2 }))["segments"][0]);
    assert_eq!(run(&r, &mut doc, "duct_lining.add", json!({ "element_ids": [lined.0], "thickness": 0.1 })).unwrap_err().code, "mep_covering.thickness");
    ok(&r, &mut doc, "duct_lining.add", json!({ "element_ids": [lined.0] }));
    let size = segment_recipe(doc.store().get(lined).unwrap()).unwrap().size;
    let (free, roughness) = reviera_core::mep_coverings::free_size(doc.store(), lined, size);
    assert!(free.shape == size.shape && (free.width - 0.35).abs() < 1e-9 && (free.height - 0.15).abs() < 1e-9, "{free:?}");
    assert_eq!(roughness, Some(0.0015));
    let pipe = id(&ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 12, 1], [5, 12, 1]], "level": l1.0, "diameter": 0.05 }))["segments"][0]);
    assert_eq!(run(&r, &mut doc, "duct_lining.add", json!({ "element_ids": [pipe.0] })).unwrap_err().code, "mep_covering.host");

    // A 600 mm silencer in a short straight between two elbows: refused.
    let run_ = duct(&mut doc, json!([[0, 16, 3], [2, 16, 3], [2, 16.75, 3], [4, 16.75, 3]]), json!({ "shape": "round", "diameter": 0.2 }));
    let mid = ElementId(run_["segments"].as_array().unwrap().iter().map(id).find(|s| {
        let sr = segment_recipe(doc.store().get(*s).unwrap()).unwrap();
        (sr.start[0] - 2.0).abs() < 1e-9 && (sr.end[0] - 2.0).abs() < 1e-9
    }).unwrap().0);
    let sr = segment_recipe(doc.store().get(mid).unwrap()).unwrap();
    let straight = ((sr.end[1] - sr.start[1]).powi(2)).sqrt();
    assert!(straight > 0.3 && straight < 0.5, "{straight}");
    let e = run(&r, &mut doc, "duct_accessory.place", json!({ "kind": "sound_attenuator", "duct_id": mid.0, "length": 0.6 })).unwrap_err();
    assert_eq!((&*e.code, &*e.message), ("duct_accessory.short", "Not enough straight duct for this accessory"));
    // On the run's open end instead it goes on, connected.
    let rep = ok(&r, &mut doc, "duct_accessory.place", json!({ "kind": "sound_attenuator", "point": [4, 16.75], "length": 0.6 }));
    let sil = id(&rep["accessory"]);
    assert_eq!(doc.store().get(sil).unwrap().class, "IfcDuctSilencer");
    assert!(reviera_core::mep::partners(doc.store()).contains_key(&(sil, 0)));
}

/// Lining narrows the duct in the pressure-drop calculation: the lined segment's velocity rises
/// by the ratio of the sections.
#[test]
fn lining_in_the_pressure_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lined.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
    let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [0, 0], "level": l1.0 })));
    let diff = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [6.0, 3.0], "level": l1.0, "offset": 2.7 })));
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [diff.0] })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
    ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 0, 0.9], [3, 0, 0.9], [3, 0, 3.6], [9, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
    ok(&r, &mut doc, "duct.create", json!({ "points": [[6, 0, 3.6], [6, 3, 3.6], [6, 3, 2.85]], "level": l1.0, "shape": "round", "diameter": 0.25, "system_type_id": "supply_air" }));
    let velocity = |doc: &mut Document, seg: ElementId| {
        ok(&r, doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
        let n: NetworkResult = serde_json::from_value(ok(&r, doc, "analytical_segment.list", json!({ "system_id": sys.0 }))).unwrap();
        n.segments.iter().find(|a| a.element == seg).unwrap().velocity
    };
    let seg = doc.store().iter().find(|e| segment_recipe(e).is_some_and(|s| (s.start[2] - 3.6).abs() < 1e-9 && (s.end[2] - 3.6).abs() < 1e-9 && s.start[1].abs() < 1e-9 && s.size.diameter > 0.3)).unwrap().id();
    let before = velocity(&mut doc, seg);
    ok(&r, &mut doc, "duct_lining.add", json!({ "element_ids": [seg.0] }));
    let after = velocity(&mut doc, seg);
    assert!((after / before - (0.4f64 / 0.35).powi(2)).abs() < 1e-9, "{before} {after}");
}
