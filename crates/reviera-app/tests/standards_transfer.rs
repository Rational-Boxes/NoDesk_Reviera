// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Transfer Project Standards (DM-XFER-001): a stacked wall type comes with its row types,
//! references rewritten to the copies; overwrite keeps the target's GlobalId and changes the
//! values; new-only skips and rename renames; settings documents and list items transfer; one
//! undo step reverses a transfer.

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId, Store};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id(v: Value) -> ElementId {
    serde_json::from_value(v).unwrap()
}

fn store(app: &App) -> Store {
    app.document(app.active().unwrap()).unwrap().doc.store().clone()
}

fn width(s: &Store, name: &str) -> f64 {
    let t = s.iter().find(|e| e.class == "IfcWallType" && e.name.as_deref() == Some(name)).unwrap();
    reviera_core::walls::type_width(t).unwrap_or(f64::NAN)
}

#[test]
fn transfer_project_standards() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });

    // The source: a plinth type, a stacked type over it, settings.
    ok(&mut app, "file.new", json!({}));
    let src = app.active().unwrap();
    let level = store(&app).iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [5, 0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3 } }));
    let upper = store(&app).iter().find(|e| e.class == "IfcWallType").unwrap().id();
    let plinth = id(ok(&mut app, "type.duplicate", json!({ "type_id": upper.0, "new_name": "Plinth 300" })));
    ok(&mut app, "type.set_param", json!({ "type_id": plinth.0, "param": "width", "value": 0.3 }));
    let stacked = id(ok(&mut app, "stackedwall.create_type", json!({ "name": "Stacked", "rows": [{ "wall_type_id": upper.0 }, { "wall_type_id": plinth.0, "height": 0.9 }] })));
    let stacked_guid = store(&app).get(stacked).unwrap().guid().to_string();
    ok(&mut app, "global.create", json!({ "name": "CorridorWidth", "data_type": "length", "value": 1.8 }));
    ok(&mut app, "mechanical_settings.set", json!({ "path": "duct.air_density", "value": 1.19 }));
    let kinds = ok(&mut app, "standards.list", json!({ "source_document_id": src }));
    let kind_names: Vec<&str> = kinds.as_array().unwrap().iter().map(|k| k["kind"].as_str().unwrap()).collect();
    assert!(kind_names.contains(&"IfcWallType") && kind_names.contains(&"global_parameters") && kind_names.contains(&"mep_settings"), "{kind_names:?}");

    // The target: a Plinth 300 of its own, 200 mm.
    ok(&mut app, "file.new", json!({}));
    let tgt = app.active().unwrap();
    let level2 = store(&app).iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [5, 0] } }, "base_level": level2.0, "top_constraint": { "unconnected": 3 } }));
    let t_upper = store(&app).iter().find(|e| e.class == "IfcWallType").unwrap().id();
    let t_plinth = id(ok(&mut app, "type.duplicate", json!({ "type_id": t_upper.0, "new_name": "Plinth 300" })));
    ok(&mut app, "type.set_param", json!({ "type_id": t_plinth.0, "param": "width", "value": 0.2 }));
    let t_plinth_guid = store(&app).get(t_plinth).unwrap().guid().to_string();
    let before = store(&app).iter().count();

    // The stacked type alone: its rows come along, matched by name in the target.
    let rep = ok(&mut app, "standards.transfer", json!({ "source_document_id": src, "selection": [{ "kind": "IfcWallType", "item_ids": [stacked_guid] }] }));
    assert_eq!(rep["copied"].as_array().unwrap().len(), 1, "{rep}");
    assert_eq!(rep["matched"].as_array().unwrap().len(), 2, "both row types are in the target: {rep}");
    let s = store(&app);
    let copy = s.iter().find(|e| e.class == "IfcWallType" && e.name.as_deref() == Some("Stacked")).expect("copied");
    assert_ne!(copy.guid().to_string(), stacked_guid, "a copy gets its own GlobalId");
    let rows = reviera_core::walls::stacked::of_type(copy).unwrap().rows;
    assert!(rows.iter().any(|r| r.wall_type == t_plinth_guid), "the row points at the target's plinth");
    assert!(rows.iter().all(|r| s.find_by_guid(&reviera_core::GlobalId::parse(&r.wall_type).unwrap()).is_some()));
    // One undo takes it away again.
    ok(&mut app, "edit.undo", json!({ "steps": 1 }));
    assert_eq!(store(&app).iter().count(), before);

    // New only skips the plinth; overwrite keeps the target's GlobalId and takes the 300 mm.
    let plinth_guid = app.document(src).unwrap().doc.store().get(plinth).unwrap().guid().to_string();
    let rep = ok(&mut app, "standards.transfer", json!({ "source_document_id": src, "selection": [{ "kind": "IfcWallType", "item_ids": [plinth_guid] }] }));
    assert_eq!(rep["skipped"].as_array().unwrap().len(), 1);
    assert!((width(&store(&app), "Plinth 300") - 0.2).abs() < 1e-9);
    ok(&mut app, "standards.transfer", json!({ "source_document_id": src, "selection": [{ "kind": "IfcWallType", "item_ids": [plinth_guid] }], "on_conflict": "overwrite" }));
    let s = store(&app);
    assert!((width(&s, "Plinth 300") - 0.3).abs() < 1e-9);
    assert_eq!(s.iter().find(|e| e.name.as_deref() == Some("Plinth 300")).unwrap().guid().to_string(), t_plinth_guid);
    // Rename makes a second one.
    let rep = ok(&mut app, "standards.transfer", json!({ "source_document_id": src, "selection": [{ "kind": "IfcWallType", "item_ids": [plinth_guid] }], "on_conflict": "rename" }));
    assert_eq!(rep["renamed"].as_array().unwrap().len(), 1);
    assert!(store(&app).iter().any(|e| e.name.as_deref() == Some("Plinth 300 (2)")));

    // Settings: global parameters item by item (without the source's associations) and the MEP
    // settings document.
    ok(&mut app, "standards.transfer", json!({ "source_document_id": src, "selection": [{ "kind": "global_parameters" }, { "kind": "mep_settings" }] }));
    let g = ok(&mut app, "global.list", json!({}));
    assert_eq!(g[0]["name"], "CorridorWidth");
    assert_eq!(reviera_core::mep_mech_settings::air(&store(&app)).0, 1.19);
    assert_eq!(app.active(), Some(tgt));
    assert!(run(&mut app, "standards.transfer", json!({ "source_document_id": tgt, "selection": [] })).is_err(), "not from itself");
}
