// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Converting legacy zones (MEP-ZONE-001): an IfcZone written by another tool, grouping two
//! spaces with set points in Pset_SpaceThermalRequirements (kelvin), converts into a
//! system-zone named as it was, whose region envelopes those two spaces and no other, with the
//! set points in °C; the IfcZone is gone, and one undo brings it back.

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId, GlobalId, Store};
use reviera_ifc::{AttrValue, IfcModel};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn store(app: &App) -> Store {
    app.document(app.active().unwrap()).unwrap().doc.store().clone()
}

fn cell(app: &mut App, level: ElementId, x: f64) -> Value {
    for (a, b) in [([x, 0.0], [x + 4.0, 0.0]), ([x + 4.0, 0.0], [x + 4.0, 4.0]), ([x + 4.0, 4.0], [x, 4.0]), ([x, 4.0], [x, 0.0])] {
        ok(app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }));
    }
    ok(app, "space.place", json!({ "level_id": level.0, "point": [x + 2.0, 2.0] }))
}

#[test]
fn convert_legacy_zone() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = store(&app).iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let a = cell(&mut app, level, 0.0);
    let b = cell(&mut app, level, 5.0);
    let c = cell(&mut app, level, 10.0);
    assert!(run(&mut app, "system_zone.convert_legacy", json!({})).is_err(), "nothing to convert");
    let saved = dir.path().join("zones.ifc");
    ok(&mut app, "file.save_as", json!({ "path": saved }));
    let guid = |v: &Value| store(&app).get(serde_json::from_value(v.clone()).unwrap()).unwrap().guid().clone();
    let (ga, gb) = (guid(&a), guid(&b));

    // Another tool's zone: the two spaces, 20 °C to 24 °C.
    let mut ifc = IfcModel::open(&saved).unwrap();
    let s = |v: &str| AttrValue::String(v.into());
    let k = |v: f64| AttrValue::Typed { type_name: "IfcThermodynamicTemperatureMeasure".into(), value: Box::new(AttrValue::Real(v)) };
    let zone = ifc.create_rooted("IfcZone", &GlobalId::new()).unwrap();
    ifc.set_attribute(zone, "Name", &s("HVAC Zone A")).unwrap();
    let members = [ga, gb].iter().map(|g| AttrValue::Ref(ifc.find_by_guid(g).unwrap().0)).collect();
    let rel = ifc.create_rooted("IfcRelAssignsToGroup", &GlobalId::new()).unwrap();
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(members)).unwrap();
    ifc.set_attribute(rel, "RelatingGroup", &AttrValue::Ref(zone.0)).unwrap();
    let mut props = Vec::new();
    for (n, v) in [("SpaceTemperatureMin", 293.15), ("SpaceTemperatureMax", 297.15)] {
        let p = ifc.create("IfcPropertySingleValue").unwrap();
        ifc.set_attribute(p, "Name", &s(n)).unwrap();
        ifc.set_attribute(p, "NominalValue", &k(v)).unwrap();
        props.push(AttrValue::Ref(p.0));
    }
    let set = ifc.create_rooted("IfcPropertySet", &GlobalId::new()).unwrap();
    ifc.set_attribute(set, "Name", &s("Pset_SpaceThermalRequirements")).unwrap();
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(props)).unwrap();
    let def = ifc.create_rooted("IfcRelDefinesByProperties", &GlobalId::new()).unwrap();
    ifc.set_attribute(def, "RelatedObjects", &AttrValue::List(vec![AttrValue::Ref(zone.0)])).unwrap();
    ifc.set_attribute(def, "RelatingPropertyDefinition", &AttrValue::Ref(set.0)).unwrap();
    let imported = dir.path().join("imported.ifc");
    ifc.write(&imported).unwrap();

    ok(&mut app, "file.open", json!({ "path": imported }));
    let st = store(&app);
    let legacy = st.iter().find(|e| e.class == "IfcZone").expect("the zone is an element");
    assert_eq!(reviera_core::system_zones::members(&st, legacy).len(), 2);
    let legacy_guid = legacy.guid().to_string();

    let made = ok(&mut app, "system_zone.convert_legacy", json!({}));
    assert_eq!(made.as_array().unwrap().len(), 1);
    let info = &ok(&mut app, "system_zone.info", json!({ "zone_ids": made }))[0];
    assert_eq!(info["name"], "HVAC Zone A");
    let spaces = info["spaces"].as_array().unwrap();
    assert_eq!(spaces.len(), 2, "the two spaces and not the third: {info}");
    let st = store(&app);
    let num = |v: &Value| st.get(serde_json::from_value(v.clone()).unwrap()).unwrap().name.clone();
    let numbers: Vec<_> = spaces.iter().map(|v| st.get(serde_json::from_value(v.clone()).unwrap()).unwrap().name.clone()).collect();
    assert_eq!(numbers, vec![num(&a), num(&b)]);
    assert!(!numbers.contains(&num(&c)));
    let p = &info["props"];
    assert!((p["heating_set_point"].as_f64().unwrap() - 20.0).abs() < 1e-9 && (p["cooling_set_point"].as_f64().unwrap() - 24.0).abs() < 1e-9, "{p}");
    assert_eq!(p["converted_from"], json!(legacy_guid));
    assert_eq!(st.iter().filter(|e| e.class == "IfcZone").count(), 0);
    // The region envelopes both spaces.
    let outline: Vec<[f64; 2]> = serde_json::from_value(info["outline"].clone()).unwrap();
    for sp in spaces {
        let e = st.get(serde_json::from_value(sp.clone()).unwrap()).unwrap();
        let ring = reviera_core::mep_spaces::geometry(&st, e).unwrap().region.unwrap().outer_ring();
        assert!(ring.iter().all(|q| reviera_core::system_zones::overlaps_or_touches(&outline, &[*q, [q[0] + 1e-9, q[1]], [q[0], q[1] + 1e-9]])));
    }

    ok(&mut app, "edit.undo", json!({ "steps": 1 }));
    let st = store(&app);
    assert_eq!(st.iter().filter(|e| e.class == "IfcZone").count(), 1);
    assert!(reviera_core::system_zones::all(&st).is_empty());
}
