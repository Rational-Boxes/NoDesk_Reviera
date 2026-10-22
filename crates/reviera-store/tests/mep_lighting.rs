// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Electrical equipment, devices and lighting fixtures (MEP-ELEC-001): a fixture whose
//! calculation point is in Space 101 is listed under it although its body crosses into 102; a
//! fixture with an IES file exports an IfcLightSourceGoniometric referencing the file; devices
//! and equipment go on walls with their IFC classes.

use std::collections::HashMap;

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
fn fixtures_devices_and_equipment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("light.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // Spaces 101 (x 0–4) and 102 (x 4–8), the wall between them at x = 4.
    let wall = |doc: &mut Document, a: [f64; 2], b: [f64; 2]| id(&ok(&r, doc, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    for (a, b) in [([0.0, 0.0], [8.0, 0.0]), ([8.0, 0.0], [8.0, 4.0]), ([8.0, 4.0], [0.0, 4.0]), ([0.0, 4.0], [0.0, 0.0])] {
        wall(&mut doc, a, b);
    }
    let middle = wall(&mut doc, [4.0, 0.0], [4.0, 4.0]);
    let s101 = id(&ok(&r, &mut doc, "room.place", json!({ "level_id": l1.0, "point": [2.0, 2.0], "number": "101" })));
    let s102 = id(&ok(&r, &mut doc, "room.place", json!({ "level_id": l1.0, "point": [6.0, 2.0], "number": "102" })));
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Ceiling Luminaire", "Receptacle", "Light Switch", "Switchboard", "Smoke Detector"] }));

    // A 600 mm luminaire centred 0.15 m short of the wall: its body crosses into 102 (the wall is
    // in plan only, at ceiling height there is none), its insertion point is in 101.
    let a = id(&ok(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": t["Ceiling Luminaire"], "point": [3.85, 2.0], "level": l1.0 })));
    // Its calculation point 0.4 m to its -x side: a copy centred in 102 at 4.2 counts in 101 (which
    // ends at the wall face, 3.9).
    ok(&r, &mut doc, "component_type.set_calc_point", json!({ "type_id": t["Ceiling Luminaire"], "point": [-0.4, 0.0, 0.0] }));
    let b = id(&ok(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": t["Ceiling Luminaire"], "point": [4.2, 2.0], "level": l1.0 })));
    let in101: Vec<ElementId> = serde_json::from_value(ok(&r, &mut doc, "space.elements", json!({ "room_id": s101.0, "categories": ["lighting_fixture"] }))).unwrap();
    assert_eq!(in101, vec![a, b]);
    let in102: Vec<ElementId> = serde_json::from_value(ok(&r, &mut doc, "space.elements", json!({ "room_id": s102.0 }))).unwrap();
    assert!(!in102.contains(&a) && !in102.contains(&b));
    let parents = HashMap::new();
    let fields = reviera_core::schedules::FieldReader::new(doc.store(), &parents);
    assert!(fields.cell(doc.store().get(b).unwrap(), "component.room").text.starts_with("101"));
    // Under no ceiling it hangs at the default ceiling height, its top there.
    let top = reviera_core::mesh_objects::regenerate(doc.store(), doc.store().get(a).unwrap()).unwrap().placement.origin[2] + 0.08;
    assert!((top - 2.6).abs() < 1e-9, "{top}");

    // Devices on the wall, equipment against it; their IFC classes and predefined types.
    let rec = id(&ok(&r, &mut doc, "device.place", json!({ "type_id": t["Receptacle"], "point": [3.9, 1.0], "level": l1.0 })));
    let face = reviera_core::mesh_objects::mesh_object_recipe(doc.store().get(rec).unwrap()).unwrap().face.expect("on the wall");
    assert_eq!(face.wall, doc.store().get(middle).unwrap().guid().to_string());
    let sw = id(&ok(&r, &mut doc, "device.place", json!({ "type_id": t["Light Switch"], "point": [3.9, 3.0], "level": l1.0, "offset": 1.1 })));
    let det = id(&ok(&r, &mut doc, "device.place", json!({ "type_id": t["Smoke Detector"], "point": [2, 2], "level": l1.0, "work_plane": true, "offset": 2.55 })));
    let board = id(&ok(&r, &mut doc, "electrical_equipment.place", json!({ "type_id": t["Switchboard"], "point": [6, 0.4], "level": l1.0 })));
    for (e, class, pre) in [(rec, "IfcOutlet", "POWEROUTLET"), (sw, "IfcSwitchingDevice", "TOGGLESWITCH"), (det, "IfcSensor", "SMOKESENSOR"), (board, "IfcElectricDistributionBoard", "SWITCHBOARD"), (a, "IfcLightFixture", "POINTSOURCE")] {
        let el = doc.store().get(e).unwrap();
        assert_eq!(el.class, class);
        assert_eq!(el.param("@PredefinedType").and_then(Value::as_str), Some(pre));
    }
    assert!(run(&r, &mut doc, "device.place", json!({ "type_id": t["Switchboard"], "point": [1, 1], "level": l1.0 })).is_err());
    assert!(run(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": t["Receptacle"], "point": [1, 1], "level": l1.0 })).is_err());

    // An IES file: read, and exported as a goniometric light source referencing it.
    let ies = dir.path().join("downlight.ies");
    std::fs::write(&ies, "IESNA:LM-63-2002\n[TEST] Reviera test fixture\nTILT=NONE\n1 2400 1 3 1 1 2 0.6 0.6 0.1\n1 1 30\n0 45 90\n0\n900 600 20\n").unwrap();
    let ph = ok(&r, &mut doc, "lighting_fixture_type.set_photometry", json!({ "type_id": t["Ceiling Luminaire"], "file": ies, "colour_temperature": 4000 }));
    assert_eq!(ph["lumens"], json!(2400.0));
    assert_eq!(ph["max_candela"], json!(900.0));
    assert!(run(&r, &mut doc, "lighting_fixture_type.set_photometry", json!({ "type_id": t["Receptacle"], "file": ies })).is_err());
    project.save(&doc).unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let sources: Vec<_> = rows.iter().filter(|r| r.class == "IFCLIGHTSOURCEGONIOMETRIC").collect();
    assert_eq!(sources.len(), 2, "one per fixture, after two saves");
    assert!(sources[0].attributes.contains("4000.") && sources[0].attributes.contains("2400."), "{}", sources[0].attributes);
    let docs: Vec<_> = rows.iter().filter(|r| r.class == "IFCDOCUMENTREFERENCE").collect();
    assert_eq!(docs.len(), 2);
    assert!(docs[0].attributes.contains("downlight.ies"));
    assert!(rows.iter().any(|r| r.class == "IFCSHAPEREPRESENTATION" && r.attributes.contains("'Lighting'") && r.attributes.contains("'LightSource'")));
    let (_p, again, _) = LocalProject::open(&path).unwrap();
    assert!(reviera_core::mesh_objects::type_of(again.store(), again.store().get(again.store().find_by_guid(doc.store().get(a).unwrap().guid()).unwrap()).unwrap()).unwrap().1.photometry.is_some());
    // Cleared: no light sources on the next save.
    ok(&r, &mut doc, "lighting_fixture_type.set_photometry", json!({ "type_id": t["Ceiling Luminaire"], "file": null }));
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(!rows.iter().any(|r| r.class == "IFCLIGHTSOURCEGONIOMETRIC"));
}
