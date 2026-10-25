// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! System-zones (MEP-ZONE-001): a zone drawn round 6 of 7 spaces lists those 6; one that
//! crosses a space takes it; a whole-level zone takes all 7; zones are served by equipment (not
//! by a wall); set points are checked; in IFC each zone is an IfcSpatialZone with a derived
//! IfcZone grouping its spaces and equipment, Pset_ZoneCommon and the set points on the spaces,
//! rewritten on each save and not read back.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
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

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

/// A 4 m square cell of walls at (x, y) with a space in it.
fn cell(r: &CommandRegistry, doc: &mut Document, level: ElementId, x: f64, y: f64) -> Value {
    for (a, b) in [([x, y], [x + 4.0, y]), ([x + 4.0, y], [x + 4.0, y + 4.0]), ([x + 4.0, y + 4.0], [x, y + 4.0]), ([x, y + 4.0], [x, y])] {
        ok(r, doc, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }));
    }
    ok(r, doc, "space.place", json!({ "level_id": level.0, "point": [x + 2.0, y + 2.0] }))
}

#[test]
fn system_zones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zones.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // Six spaces in a 3 × 2 block, a seventh apart.
    let mut block = Vec::new();
    for j in 0..2 {
        for k in 0..3 {
            block.push(cell(&r, &mut doc, l1, k as f64 * 5.0, j as f64 * 5.0));
        }
    }
    let apart = cell(&r, &mut doc, l1, 20.0, 0.0);

    // Drawn round the block: those six, in number order.
    let z1 = id(&ok(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0, "boundary": [[-1, -1], [15, -1], [15, 10], [-1, 10]] })));
    let info = &ok(&r, &mut doc, "system_zone.info", json!({ "zone_ids": [z1.0] }))[0];
    assert_eq!(info["name"], "System-Zone 1");
    assert_eq!(info["spaces"], json!(block), "{info}");
    assert!((info["area"].as_f64().unwrap() - 176.0).abs() < 1e-9);
    assert!((info["height"].as_f64().unwrap() - 3.0).abs() < 1e-9, "the top level: 3 m");
    // Clockwise corners are turned round; one crossing the seventh space takes it.
    let z2 = id(&ok(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0, "boundary": [[19, 1], [19, 3], [21, 3], [21, 1]], "name": "East" })));
    assert_eq!(ok(&r, &mut doc, "system_zone.info", json!({ "zone_ids": [z2.0] }))[0]["spaces"], json!([apart]));
    // By Level: all seven.
    let z3 = id(&ok(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0, "whole_level": true })));
    let i3 = &ok(&r, &mut doc, "system_zone.info", json!({ "zone_ids": [z3.0] }))[0];
    assert_eq!(i3["spaces"].as_array().unwrap().len(), 7);
    assert_eq!(i3["whole_level"], true);
    assert_eq!(i3["name"], "System-Zone 2");

    // Bad boundaries and arguments are refused.
    let e = run(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0, "boundary": [[0, 0], [4, 4], [4, 0], [0, 4]] })).unwrap_err();
    assert!(e.message.contains("crosses itself"), "{}", e.message);
    assert!(run(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0, "boundary": [[0, 0], [4, 0]] })).is_err());
    assert!(run(&r, &mut doc, "system_zone.create", json!({ "level_id": l1.0 })).is_err());

    // Served by a VAV box, not by a wall.
    let vav = ok(&r, &mut doc, "mep_type.create", json!({
        "name": "VAV Box", "category": "mechanical_equipment", "size": [0.8, 0.5, 0.4],
        "connectors": [{ "domain": "duct", "name": "Inlet", "origin": [-0.4, 0.0, 0.2], "direction": [-1, 0, 0], "diameter": 0.25, "classification": "supply_air", "flow_direction": "in" }]
    }));
    let unit = ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": vav, "point": [2.0, 2.0], "level": l1.0, "offset": 2.8 }));
    ok(&r, &mut doc, "system_zone.assign", json!({ "zone_id": z1.0, "equipment_or_system_id": unit }));
    let wall = doc.store().iter().find(|e| reviera_core::walls::wall_recipe(e).is_some()).unwrap().id();
    let e = run(&r, &mut doc, "system_zone.assign", json!({ "zone_id": z1.0, "equipment_or_system_id": wall.0 })).unwrap_err();
    assert_eq!(e.code, "system_zone.cannot_serve");
    let info = &ok(&r, &mut doc, "system_zone.info", json!({ "zone_ids": [z1.0] }))[0];
    assert_eq!((info["served_by"].clone(), info["served_by_kind"].as_str()), (unit.clone(), Some("equipment")));
    assert_eq!(info["served_by_name"], "VAV Box");

    // Set points: heating below cooling.
    ok(&r, &mut doc, "system_zone.set", json!({ "zone_ids": [z1.0], "heating_set_point": 21.0, "cooling_set_point": 24.0, "humidity_max": 60.0 }));
    let e = run(&r, &mut doc, "system_zone.set", json!({ "zone_ids": [z1.0], "heating_set_point": 25.0 })).unwrap_err();
    assert!(e.message.contains("below the cooling"), "{}", e.message);
    ok(&r, &mut doc, "system_zone.set", json!({ "zone_ids": [z1.0], "humidity_max": null }));
    let pr = &ok(&r, &mut doc, "system_zone.info", json!({ "zone_ids": [z1.0] }))[0]["props"];
    assert_eq!((pr["heating_set_point"].as_f64(), pr["humidity_max"].as_f64()), (Some(21.0), None));
    // One undo takes a change back.
    ok(&r, &mut doc, "system_zone.set", json!({ "zone_ids": [z2.0], "name": "East wing" }));
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(doc.store().get(z2).unwrap().name.as_deref(), Some("East"));

    // IFC: three spatial zones, three derived IfcZones; the first groups six spaces and the box.
    project.save(&doc).unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCSPATIALZONE"), 3);
    assert!(rows.iter().filter(|r| r.class == "IFCSPATIALZONE").all(|r| r.attributes.contains(".THERMAL.")));
    assert_eq!(count("IFCZONE"), 3, "rewritten, not added to");
    assert!(rows.iter().filter(|r| r.class == "IFCZONE").all(|r| r.attributes.contains("'SystemZone'")));
    assert_eq!(count("IFCRELREFERENCEDINSPATIALSTRUCTURE"), 3);
    let groups: Vec<&str> = rows.iter().filter(|r| r.class == "IFCRELASSIGNSTOGROUP").map(|r| r.attributes.as_str()).collect();
    assert!(groups.iter().any(|g| g.matches('#').count() == 6 + 1 + 1), "six spaces, the box and the zone: {groups:?}");
    let psets: Vec<&str> = rows.iter().filter(|r| r.class == "IFCPROPERTYSET").map(|r| r.attributes.as_str()).collect();
    assert_eq!(psets.iter().filter(|p| p.contains("'Pset_ZoneCommon'")).count(), 3);
    assert_eq!(psets.iter().filter(|p| p.contains("'Pset_SpaceThermalRequirements'")).count(), 1);
    assert!(rows.iter().any(|r| r.attributes.contains("'SpaceTemperatureMin'") && r.attributes.contains("(294.1")), "21 °C in kelvin");

    // Read back: the zones and their spaces, no IfcZone elements, no set points on the spaces.
    let (_, doc2, _) = LocalProject::open(&path).unwrap();
    let s = doc2.store();
    assert_eq!(s.iter().filter(|e| e.class == "IfcZone").count(), 0);
    assert_eq!(reviera_core::system_zones::all(s).len(), 3);
    assert_eq!(reviera_core::system_zones::spaces(s, z1).len(), 6);
    let sp = s.get(id(&block[0])).unwrap();
    assert!(!sp.params.keys().any(|k| k.starts_with("Pset_SpaceThermalRequirements")), "{:?}", sp.params.keys().collect::<Vec<_>>());

    // Deleting a zone removes its body; the others stay.
    ok(&r, &mut doc, "element.delete", json!({ "element_ids": [z3.0] }));
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCSPATIALZONE").count(), 2);
    assert_eq!(rows.iter().filter(|r| r.class == "IFCZONE").count(), 2);
}
