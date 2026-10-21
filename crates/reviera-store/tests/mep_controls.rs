// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mechanical control devices (MEP-DUCT-012): a thermostat on a wall at 1 500 mm following the
//! wall, assigned to a VAV box, its IFC (class, predefined type, storey, host and control
//! relationships), the refusals, and the wall deleted under it.

use reviera_core::mesh_objects::{mesh_object_recipe, regenerate};
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
fn a_thermostat_on_a_wall_controls_a_vav_box() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controls.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let z0 = reviera_core::levels::elevation(doc.store().get(l1).unwrap());
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let width = reviera_core::walls::regenerate(doc.store(), doc.store().get(wall).unwrap()).unwrap().width;
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Thermostat", "CO2 Sensor", "VAV Box"] }));
    // Clicked 50 mm off the wall's interior (right-hand, −y) face, 2 m along: it goes on that face.
    let th = id(&ok(&r, &mut doc, "mech_control_device.place", json!({ "type_id": t["Thermostat"], "point": [2.0, -width / 2.0 - 0.05], "level": l1.0, "offset": 1.5 })));
    let e = doc.store().get(th).unwrap();
    assert_eq!(e.class, "IfcUnitaryControlElement");
    assert_eq!(e.param("@PredefinedType").and_then(Value::as_str), Some("THERMOSTAT"));
    let face = mesh_object_recipe(e).unwrap().face.expect("hosted");
    assert_eq!(face.side, -1.0);
    let at = |doc: &Document| regenerate(doc.store(), doc.store().get(th).unwrap()).unwrap().placement;
    let p = at(&doc);
    // Its back (+y) on the face (half its 25 mm depth out from it), 1.5 m up, facing away (−y).
    assert!((p.origin[0] - 2.0).abs() < 1e-9 && (p.origin[1] + width / 2.0 + 0.0125).abs() < 1e-9, "{:?}", p.origin);
    assert!((p.origin[2] - z0 - 1.5).abs() < 1e-9);
    assert!(p.rotation.abs() < 1e-9, "{}", p.rotation);
    // The wall moves 1 m: the thermostat with it.
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [wall.0], "vector": [0.0, 1.0, 0.0] }));
    assert!((at(&doc).origin[1] - (1.0 - width / 2.0 - 0.0125)).abs() < 1e-9, "{:?}", at(&doc).origin);
    // Moved along the wall, it stays on the face.
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [th.0], "vector": [1.0, 0.5, 0.0] }));
    let q = at(&doc);
    assert!((q.origin[0] - 3.0).abs() < 1e-9 && (q.origin[1] - (1.0 - width / 2.0 - 0.0125)).abs() < 1e-9, "{:?}", q.origin);

    // Far from walls, on the work plane.
    let co2 = id(&ok(&r, &mut doc, "mech_control_device.place", json!({ "type_id": t["CO2 Sensor"], "point": [3.0, 5.0], "level": l1.0 })));
    assert!(mesh_object_recipe(doc.store().get(co2).unwrap()).unwrap().face.is_none());
    assert_eq!(doc.store().get(co2).unwrap().class, "IfcSensor");
    // Not a control device type.
    let e = run(&r, &mut doc, "mech_control_device.place", json!({ "type_id": t["VAV Box"], "point": [3.0, 5.0], "level": l1.0 })).unwrap_err();
    assert_eq!(e.code, "mech_control_device.not_a_device");

    // Assigned to a VAV box; not to itself or another device.
    let vav = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["VAV Box"], "point": [3.0, 3.0], "level": l1.0, "offset": 2.8 })));
    assert_eq!(doc.store().get(vav).unwrap().class, "IfcAirTerminalBox");
    ok(&r, &mut doc, "mech_control_device.assign", json!({ "device_id": th.0, "controlled_ids": [vav.0] }));
    assert_eq!(run(&r, &mut doc, "mech_control_device.assign", json!({ "device_id": th.0, "controlled_ids": [th.0] })).unwrap_err().code, "mech_control_device.self");
    assert_eq!(run(&r, &mut doc, "mech_control_device.assign", json!({ "device_id": th.0, "controlled_ids": [co2.0] })).unwrap_err().code, "mech_control_device.device");
    let list = ok(&r, &mut doc, "mech_control_device.list", json!({}));
    let row = list.as_array().unwrap().iter().find(|d| d["device"] == th.0).unwrap();
    assert_eq!((row["host"].as_u64(), row["controlled"].clone()), (Some(wall.0), json!([vav.0])));

    // IFC: the class and predefined type, in the storey, hosted to the wall, controlling the box.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let dev = rows.iter().find(|r| r.class == "IFCUNITARYCONTROLELEMENT").expect("the thermostat");
    assert!(dev.attributes.contains(".THERMOSTAT."), "{}", dev.attributes);
    let refs = |r: &reviera_ifc::StepRow, id: u32| r.attributes.contains(&format!("#{id},")) || r.attributes.contains(&format!("#{id})"));
    assert!(rows.iter().any(|r| r.class == "IFCRELCONTAINEDINSPATIALSTRUCTURE" && refs(r, dev.id.0)));
    assert!(rows.iter().any(|r| r.class == "IFCRELCONNECTSELEMENTS" && refs(r, dev.id.0)));
    let box_row = rows.iter().find(|r| r.class == "IFCAIRTERMINALBOX").expect("the box");
    let rel = rows.iter().find(|r| r.class == "IFCRELFLOWCONTROLELEMENTS").expect("the control relationship");
    assert!(refs(rel, dev.id.0) && refs(rel, box_row.id.0), "{}", rel.attributes);
    // Saved again, not doubled.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCRELFLOWCONTROLELEMENTS").count(), 1);

    // The wall deleted: the thermostat stays where it was, unhosted, with a warning.
    let before = at(&doc);
    let out = run(&r, &mut doc, "element.delete", json!({ "element_ids": [wall.0] })).unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "mep.unhosted"), "{:?}", out.warnings);
    assert!(doc.store().get(th).is_some());
    assert!(mesh_object_recipe(doc.store().get(th).unwrap()).unwrap().face.is_none());
    let after = at(&doc);
    assert!((0..3).all(|k| (after.origin[k] - before.origin[k]).abs() < 1e-9) && (after.rotation - before.rotation).abs() < 1e-9);
}
