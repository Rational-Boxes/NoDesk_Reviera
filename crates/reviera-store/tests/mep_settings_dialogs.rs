// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP settings dialogs (MEP-SET-004): three pages of Mechanical Settings changed in one OK are
//! one undo step; a space type used by 12 rooms cannot be deleted and the refusal names 12; a
//! demand-factor range grid with a gap between 10 and 12 kVA is refused, flagging the row at
//! 12 kVA. (`mep_settings.show` headless is tested in reviera-app.)

use reviera_core::{Caller, CommandRegistry, Document};
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

#[test]
fn settings_dialogs() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("dlg.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // Three pages in one OK: one undo step reverts all three.
    let before = ok(&r, &mut doc, "mechanical_settings.get", json!({}));
    let mut t = before.clone();
    t["hidden_line"]["inside_gap"] = json!(2.0);
    t["duct"]["air_density"] = json!(1.18);
    t["pipe"]["slopes"] = json!([0.0, 0.01]);
    ok(&r, &mut doc, "mechanical_settings.set", json!({ "path": "", "value": t }));
    let now = ok(&r, &mut doc, "mechanical_settings.get", json!({}));
    assert_eq!(now["hidden_line"]["inside_gap"], json!(2.0));
    assert_eq!(now["duct"]["air_density"], json!(1.18));
    assert_eq!(now["pipe"]["slopes"], json!([0.0, 0.01]));
    ok(&r, &mut doc, "edit.undo", json!({ "steps": 1 }));
    assert_eq!(ok(&r, &mut doc, "mechanical_settings.get", json!({})), before, "one undo reverts every page");
    assert!(run(&r, &mut doc, "mechanical_settings.set", json!({ "path": "", "value": { "duct": {} } })).is_err());

    // "Office" used by 12 rooms cannot be deleted.
    let office = ok(&r, &mut doc, "space_type.create", json!({ "name": "Office", "values": { "lighting_load_density": 8.0, "area_per_person": 10.0, "outside_air_per_person": 10.0, "opening_time": "07:30" } }));
    assert_eq!(office, json!("office"));
    let copy = ok(&r, &mut doc, "space_type.create", json!({ "name": "Open Office", "from": "Office" }));
    let list = ok(&r, &mut doc, "space_type.list", json!({ "which": "space" }));
    assert_eq!(list[1]["values"]["lighting_load_density"], json!(8.0), "duplicated: {list}");
    let mut rooms = Vec::new();
    for k in 0..12 {
        let x = k as f64 * 5.0;
        for (a, b) in [([x, 0.0], [x + 4.0, 0.0]), ([x + 4.0, 0.0], [x + 4.0, 4.0]), ([x + 4.0, 4.0], [x, 4.0]), ([x, 4.0], [x, 0.0])] {
            ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }));
        }
        rooms.push(ok(&r, &mut doc, "room.place", json!({ "level_id": l1.0, "point": [x + 2.0, 2.0] })));
    }
    ok(&r, &mut doc, "space.set_type", json!({ "element_ids": rooms, "type_id": "Office" }));
    let e = run(&r, &mut doc, "space_type.delete", json!({ "which": "space", "type_id": "office" })).unwrap_err();
    assert!(e.message.contains("12 spaces"), "{}", e.message);
    ok(&r, &mut doc, "space_type.delete", json!({ "which": "space", "type_id": copy }));
    ok(&r, &mut doc, "space_type.set", json!({ "which": "space", "type_id": "office", "name": "Offices" }));
    assert!(run(&r, &mut doc, "space_type.set", json!({ "which": "space", "type_id": "office", "values": { "heating_set_point": 25.0, "cooling_set_point": 22.0 } })).is_err());
    ok(&r, &mut doc, "space.set_type", json!({ "element_ids": rooms, "type_id": null }));
    ok(&r, &mut doc, "space_type.delete", json!({ "which": "space", "type_id": "office" }));
    let b = ok(&r, &mut doc, "building_type.create", json!({ "name": "School" }));
    let building = doc.store().iter().find(|e| e.class == "IfcBuilding").unwrap().id();
    ok(&r, &mut doc, "building.set_type", json!({ "element_ids": [building.0], "type_id": b }));
    let e = run(&r, &mut doc, "space_type.delete", json!({ "which": "building", "type_id": "school" })).unwrap_err();
    assert!(e.message.contains("1 building"), "{}", e.message);

    // A gap between 10 and 12 kVA: refused, the row at 12 kVA flagged.
    let e = run(
        &r,
        &mut doc,
        "demand_factor.create",
        json!({ "name": "Gappy", "method": "load", "ranges": [{ "from": 0, "to": 10000, "percent": 100 }, { "from": 12000, "percent": 50 }] }),
    )
    .unwrap_err();
    assert!(e.message.starts_with("row 2 (from 12 kVA)"), "{}", e.message);
    let e = run(&r, &mut doc, "demand_factor.create", json!({ "name": "Late", "method": "load", "ranges": [{ "from": 5000, "percent": 100 }] })).unwrap_err();
    assert!(e.message.starts_with("row 1"), "{}", e.message);
    let e = run(&r, &mut doc, "demand_factor.create", json!({ "name": "Over", "method": "quantity", "ranges": [{ "from": 0, "to": 10, "percent": 100 }, { "from": 8, "percent": 50 }] })).unwrap_err();
    assert!(e.message.contains("overlaps"), "{}", e.message);
    ok(&r, &mut doc, "demand_factor.create", json!({ "name": "Good", "method": "load", "ranges": [{ "from": 0, "to": 10000, "percent": 100 }, { "from": 10000, "percent": 50 }] }));
}
