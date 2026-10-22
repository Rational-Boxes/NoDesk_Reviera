// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Spaces (MEP-SPACE-001/002): Place Spaces Automatically on a level of 25 enclosed regions makes
//! 25 spaces, 20 of them occupiable under rooms; Space Naming copies "101 Office" from the room at
//! a space; each space exports with Qto_SpaceBaseQuantities.NetVolume within 0.5%; a space type's
//! values apply to its spaces and each can be overridden per space; unplaced spaces stay listed.

use std::collections::HashMap;

use reviera_core::mep_spaces::SpaceInfo;
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(|x| ElementId(x.as_u64().unwrap())).collect()
}

#[test]
fn spaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 20 × 20 m box of walls cut into a 5 × 5 grid by separation lines: 25 regions.
    let c = [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]];
    for k in 0..4 {
        ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": c[k], "end": c[(k + 1) % 4] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }));
    }
    let mut lines = Vec::new();
    for k in 1..5 {
        let v = k as f64 * 4.0;
        lines.push(json!({ "start": [v, 0.0], "end": [v, 20.0] }));
        lines.push(json!({ "start": [0.0, v], "end": [20.0, v] }));
    }
    ok(&r, &mut doc, "roomseparator.create", json!({ "level_id": l1.0, "lines": lines }));
    // Rooms in 20 of them (all but the top row), the first "101 Office".
    let mut first_room = None;
    for i in 0..5 {
        for j in 0..4 {
            let p = [i as f64 * 4.0 + 2.0, j as f64 * 4.0 + 2.0];
            let args = if first_room.is_none() { json!({ "level_id": l1.0, "point": p, "number": "101", "name": "Office" }) } else { json!({ "level_id": l1.0, "point": p }) };
            let room = ok(&r, &mut doc, "room.place", args);
            first_room.get_or_insert(room);
        }
    }

    let spaces = ids(&ok(&r, &mut doc, "space.place_auto", json!({ "level_id": l1.0 })));
    assert_eq!(spaces.len(), 25);
    let info: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({}))).unwrap();
    assert_eq!(info.len(), 25);
    assert_eq!(info.iter().filter(|s| s.occupiable).count(), 20);
    assert!(info.iter().all(|s| s.enclosed && s.area > 10.0));
    // Spaces are not rooms.
    assert_eq!(reviera_core::rooms::all(doc.store()).len(), 20);
    assert!(ids(&ok(&r, &mut doc, "space.place_auto", json!({ "level_id": l1.0 }))).is_empty(), "no region left");

    // Space Naming: the space under "101 Office" takes its number and name.
    let named = ids(&ok(&r, &mut doc, "space.name_from_rooms", json!({})));
    assert_eq!(named.len(), 20);
    let info: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({}))).unwrap();
    let office = info.iter().find(|s| s.number == "101").expect("named 101");
    assert_eq!(office.name, "Office");

    // A space type: its values apply, each overridable per space.
    ok(&r, &mut doc, "space_type.create", json!({ "name": "Office", "values": { "lighting_load_density": 9.0, "area_per_person": 10.0, "occupancy_schedule": "Office Occupancy", "lighting_schedule": "Office Lighting" } }));
    let (a, b) = (spaces[0], spaces[1]);
    ok(&r, &mut doc, "space.set_type", json!({ "element_ids": [a.0, b.0], "type_id": "Office" }));
    ok(&r, &mut doc, "space.set", json!({ "ids": [b.0], "overrides": { "lighting_load_density": 6.5 }, "condition": "heated", "ceiling_reflectance": 0.7 }));
    let two: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({ "ids": [a.0, b.0] }))).unwrap();
    assert_eq!(two[0].values.lighting_load_density, 9.0);
    assert_eq!(two[0].values.occupancy_schedule, "Office Occupancy");
    assert!(two[0].overridden.is_empty());
    assert_eq!(two[1].values.lighting_load_density, 6.5);
    assert_eq!(two[1].values.area_per_person, 10.0, "the rest is the type's");
    assert_eq!(two[1].overridden, vec!["lighting_load_density".to_string()]);
    assert!((two[1].people - two[1].area / 10.0).abs() < 1e-9);
    ok(&r, &mut doc, "space.set", json!({ "ids": [b.0], "overrides": { "lighting_load_density": null } }));
    let back: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({ "ids": [b.0] }))).unwrap();
    assert_eq!(back[0].values.lighting_load_density, 9.0, "back to the type's");
    assert!(run(&r, &mut doc, "space.set", json!({ "ids": [b.0], "overrides": { "nonsense": 1 } })).is_err());
    assert!(run(&r, &mut doc, "space.set", json!({ "ids": [b.0], "ceiling_reflectance": 1.5 })).is_err());

    // A luminaire in a space counts as its actual lighting load.
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Ceiling Luminaire", "Supply Diffuser 600"] }));
    let pa = &info.iter().find(|s| s.id == a).unwrap();
    let _ = pa;
    let at = |doc: &Document, id: ElementId| reviera_core::mep_spaces::geometry(doc.store(), doc.store().get(id).unwrap()).unwrap().tag_point;
    let p = at(&doc, a);
    ok(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": t["Ceiling Luminaire"], "point": p, "level": l1.0 }));
    ok(&r, &mut doc, "air_terminal.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [p[0] + 1.0, p[1]], "level": l1.0, "offset": 2.7, "flow": 0.05 }));
    let one: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({ "ids": [a.0] }))).unwrap();
    assert_eq!(one[0].actual_lighting_load, 36.0);
    assert!((one[0].actual_supply - 0.05).abs() < 1e-12);

    // Exported: each space an IfcSpace (MEPSpace) with its net volume within 0.5%.
    project.save(&doc).unwrap();
    let m = IfcModel::open(&path).unwrap();
    let rows = m.step_rows().unwrap();
    let by_guid: HashMap<String, &reviera_ifc::StepRow> = rows.iter().filter_map(|r| r.global_id.clone().map(|g| (g, r))).collect();
    let info: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({}))).unwrap();
    for s in &info {
        let guid = doc.store().get(s.id).unwrap().guid().to_string();
        let row = by_guid[&guid];
        assert!(row.class == "IFCSPACE" && row.attributes.contains("'MEPSpace'"), "{}", row.attributes);
        // Its quantity set's NetVolume.
        // A step reference as a whole token (#12 is not #123).
        let refers = |text: &str, id: u32| text.split(|c: char| !(c == '#' || c.is_ascii_digit())).any(|t| t == format!("#{id}"));
        let rels: Vec<_> = rows.iter().filter(|r| r.class == "IFCRELDEFINESBYPROPERTIES" && refers(&r.attributes, row.id.0)).collect();
        let vol = rels
            .iter()
            .filter_map(|rel| {
                let q = rel.attributes.rsplit('#').next()?.trim_end_matches(')').parse::<u32>().ok()?;
                let qrow = rows.iter().find(|r| r.id.0 == q && r.class == "IFCELEMENTQUANTITY" && r.attributes.contains("Qto_SpaceBaseQuantities"))?;
                rows.iter().filter(|r| r.class == "IFCQUANTITYVOLUME" && refers(&qrow.attributes, r.id.0)).find_map(|r| r.attributes.split(',').nth(3)?.trim().parse::<f64>().ok())
            })
            .next()
            .unwrap_or_else(|| panic!("NetVolume of {}", s.number));
        let vol = if vol > 1e6 { vol / 1e9 } else { vol };
        assert!((vol - s.volume).abs() <= 0.005 * s.volume, "{vol} vs {}", s.volume);
    }

    // Unplaced: kept, listed as Not Placed, placed again by a point.
    ok(&r, &mut doc, "space.unplace", json!({ "ids": [a.0] }));
    let parents = HashMap::new();
    let fields = reviera_core::schedules::FieldReader::new(doc.store(), &parents);
    assert_eq!(fields.cell(doc.store().get(a).unwrap(), "space.area").text, "Not Placed");
    ok(&r, &mut doc, "space.set", json!({ "ids": [a.0], "point": p }));
    let again: Vec<SpaceInfo> = serde_json::from_value(ok(&r, &mut doc, "space.info", json!({ "ids": [a.0] }))).unwrap();
    assert!(again[0].placed && again[0].enclosed);
    assert_eq!(reviera_core::categories::category_of(doc.store().get(a).unwrap()).map(|c| c.key), Some("cat.spaces"));
}
