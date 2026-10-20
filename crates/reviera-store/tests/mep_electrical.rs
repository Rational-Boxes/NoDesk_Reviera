// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Electrical circuits and panels (MEP-ELEC-002, MEP-ELEC-003, MEP-ELEC-012, MEP-SET-002): six
//! 230 V receptacles in one circuit with a 400 V motor skipped, panels by distribution system,
//! slots and numbers, moving a receptacle between circuits, Disconnect Panel, three-pole
//! numbering, Check Circuits and the IFC circuit.

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
fn circuits_panels_and_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("elec.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let types = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Distribution Board", "Receptacle", "Air Handling Unit"] }));
    let place = |doc: &mut Document, t: &str, x: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": types[t], "point": [x, 0.0], "level": l1.0 })));
    let receptacles: Vec<ElementId> = (0..7).map(|k| place(&mut doc, "Receptacle", k as f64)).collect();
    let ahu = place(&mut doc, "Air Handling Unit", 20.0);
    let db = place(&mut doc, "Distribution Board", 30.0);
    let us = place(&mut doc, "Distribution Board", 32.0);

    // Six 230 V receptacles in one power circuit; the 400 V three-phase AHU is skipped.
    let mut six: Vec<u64> = receptacles[..6].iter().map(|e| e.0).collect();
    six.push(ahu.0);
    let out = run(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": six })).unwrap();
    let c1 = id(&out.result["circuit"]);
    assert_eq!(out.result["added"].as_array().unwrap().len(), 6);
    assert_eq!(out.result["skipped"][0]["element"], ahu.0);
    assert!(out.result["skipped"][0]["reason"].as_str().unwrap().contains("400 V 3 pole"), "{}", out.result);
    assert!(out.warnings.iter().any(|w| w.id == "circuit.skipped"));
    let c = &ok(&r, &mut doc, "circuit.list", json!({}))[0];
    assert_eq!(c["voltage"], 230.0);
    assert_eq!(c["load"], 6.0 * 180.0);
    assert_eq!(c["name"], "");

    // Panels: LP1 on 230/400 V, US1 on 120/208 V.
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": db.0, "name": "LP1" }));
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": us.0, "name": "US1", "distribution_system": "120_208_wye" }));
    let choices = ok(&r, &mut doc, "circuit.compatible_panels", json!({ "circuit_id": c1.0 }));
    let names: Vec<&str> = choices.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["LP1"]);
    let e = run(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c1.0, "panel_id": us.0 })).unwrap_err();
    assert_eq!(e.code, "panel.incompatible");
    assert!(e.message.contains("120 V"), "{}", e.message);
    assert_eq!(ok(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c1.0, "panel_id": db.0 })), "LP1-1");

    // A second circuit; moving a receptacle to it drops the first's load by its 180 VA.
    let out = ok(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": [receptacles[6].0] }));
    let c2 = id(&out["circuit"]);
    assert_eq!(ok(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c2.0, "panel_id": db.0 })), "LP1-2");
    let moved = ok(&r, &mut doc, "circuit.add", json!({ "circuit_id": c2.0, "element_ids": [receptacles[0].0] }));
    assert_eq!(moved["moved"], json!([[receptacles[0].0, c1.0]]));
    let loads: Vec<f64> = ok(&r, &mut doc, "circuit.list", json!({})).as_array().unwrap().iter().map(|c| c["load"].as_f64().unwrap()).collect();
    assert_eq!(loads, [5.0 * 180.0, 2.0 * 180.0]);
    // The AHU cannot join a 230 V circuit.
    let e = run(&r, &mut doc, "circuit.add", json!({ "circuit_id": c2.0, "element_ids": [ahu.0] })).unwrap_err();
    assert!(e.message.starts_with("Incompatible: "), "{}", e.message);

    // The AHU's own three-pole circuit takes slots 3, 5 and 7 (1 and 2 are used).
    let c3 = id(&ok(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": [ahu.0] }))["circuit"]);
    assert_eq!(ok(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c3.0, "panel_id": db.0 })), "LP1-3,5,7");
    // Numerical naming: the slots alone.
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": db.0, "naming": "numerical" }));
    assert_eq!(doc.store().get(c3).unwrap().name.as_deref(), Some("3,5,7"));
    // By Project again, and the project's scheme Prefixed.
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": db.0, "naming": "by_project" }));
    ok(&r, &mut doc, "electrical_settings.set", json!({ "naming": "prefixed" }));
    assert_eq!(doc.store().get(c1).unwrap().name.as_deref(), Some("C1"));
    // The panel cannot shrink below its circuits' slots.
    let e = run(&r, &mut doc, "panel.set", json!({ "panel_id": db.0, "slots": 4 })).unwrap_err();
    assert_eq!(e.code, "panel.slots");
    let p = &ok(&r, &mut doc, "panel.list", json!({}))[0];
    assert_eq!(p["used_slots"], 5);

    // Disconnect Panel: no number, the members kept, and Check Circuits reports it.
    ok(&r, &mut doc, "circuit.disconnect_panel", json!({ "circuit_id": c2.0 }));
    let c = ok(&r, &mut doc, "circuit.list", json!({ "element_id": receptacles[6].0 }));
    assert_eq!(c[0]["name"], "");
    assert_eq!(c[0]["elements"].as_array().unwrap().len(), 2);
    let issues = ok(&r, &mut doc, "mep_check.systems", json!({ "kind": "circuit" }));
    assert_eq!(issues.as_array().unwrap().len(), 1, "{issues}");
    assert!(issues[0]["message"].as_str().unwrap().contains("has no panel"));
    // Removing the last member deletes a circuit.
    ok(&r, &mut doc, "circuit.remove", json!({ "circuit_id": c3.0, "element_ids": [ahu.0] }));
    assert!(doc.store().get(c3).is_none());

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let circuit = rows.iter().find(|r| r.class == "IFCDISTRIBUTIONCIRCUIT" && r.attributes.contains("'C1'")).expect("circuit C1");
    assert!(circuit.attributes.contains(".ELECTRICAL."), "{}", circuit.attributes);
    let group = rows
        .iter()
        .find(|r| r.class == "IFCRELASSIGNSTOGROUP" && r.attributes.ends_with(&format!("#{})", circuit.id.0)))
        .expect("its members");
    assert_eq!(group.attributes.matches('#').count() - 1, 5);
}

#[test]
fn feeders_transformers_and_loads() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("feed.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let types = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Distribution Board", "Transformer"] }));
    let place = |doc: &mut Document, t: &Value, x: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t, "point": [x, 0.0], "level": l1.0 })));
    let mdb = place(&mut doc, &types["Distribution Board"], 0.0);
    let lp = place(&mut doc, &types["Distribution Board"], 2.0);
    let t1 = place(&mut doc, &types["Transformer"], 4.0);
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": mdb.0, "name": "MDB", "distribution_system": "277_480_wye", "feed_through_lugs": true }));
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": lp.0, "name": "LP1", "distribution_system": "120_208_wye" }));
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": t1.0, "name": "T1", "distribution_system": "277_480_wye" }));

    // A 120/208 V panel cannot be fed straight from a 480 V board…
    let e = run(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": lp.0, "parent_panel_id": mdb.0 })).unwrap_err();
    assert_eq!(e.code, "panel.incompatible", "{}", e.message);
    // … but can through a transformer, once its secondary is set.
    ok(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": t1.0, "parent_panel_id": mdb.0 }));
    let e = run(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": lp.0, "parent_panel_id": t1.0 })).unwrap_err();
    assert_eq!(e.message, "Transformer has no secondary distribution system");
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": t1.0, "secondary": "120_208_wye" }));
    ok(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": lp.0, "parent_panel_id": t1.0 }));
    // A loop is refused, naming the panels.
    let e = run(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": mdb.0, "parent_panel_id": lp.0 })).unwrap_err();
    assert_eq!(e.code, "panel.loop");
    assert!(e.message.contains("LP1") && e.message.contains("MDB"), "{}", e.message);

    // 12 kVA of lighting on LP1 shows on MDB, through the transformer.
    let lights = id(&ok(&r, &mut doc, "mep_type.create", json!({
        "name": "Light Bank", "category": "lighting_fixture", "size": [0.6, 0.6, 0.1],
        "connectors": [{ "domain": "electrical", "origin": [0, 0, 0.1], "direction": [0, 0, 1], "flow_direction": "in",
            "electrical": { "voltage": 120.0, "poles": 1, "load": [1000.0], "load_classification": "Lighting" } }]
    })));
    let banks: Vec<u64> = (0..12).map(|k| place(&mut doc, &json!(lights.0), 10.0 + k as f64).0).collect();
    let c = id(&ok(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": banks }))["circuit"]);
    ok(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": lp.0 }));
    let panels = ok(&r, &mut doc, "panel.list", json!({}));
    let load = |name: &str| panels.as_array().unwrap().iter().find(|p| p["name"] == name).unwrap()["connected_load"].as_f64().unwrap();
    assert_eq!(load("LP1"), 12000.0);
    assert_eq!(load("T1"), 12000.0);
    assert_eq!(load("MDB"), 12000.0);
    let tree = ok(&r, &mut doc, "panel.hierarchy", json!({}));
    assert_eq!(tree[0]["name"], "MDB");
    assert_eq!(tree[0]["children"][0]["children"][0]["name"], "LP1");

    // A feeder on MDB's feed-through lugs takes no slot; the lugs take one feeder.
    let sub = place(&mut doc, &types["Distribution Board"], 6.0);
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": sub.0, "name": "SUB", "distribution_system": "277_480_wye" }));
    let before = ok(&r, &mut doc, "panel.list", json!({})).as_array().unwrap().iter().find(|p| p["name"] == "MDB").unwrap()["used_slots"].clone();
    let f = id(&ok(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": sub.0, "parent_panel_id": mdb.0, "connection": "lugs" })));
    let after = ok(&r, &mut doc, "panel.list", json!({})).as_array().unwrap().iter().find(|p| p["name"] == "MDB").unwrap()["used_slots"].clone();
    assert_eq!(before, after);
    assert_eq!(doc.store().get(f).unwrap().name.as_deref(), Some("MDB feed-through"));
    let sub2 = place(&mut doc, &types["Distribution Board"], 8.0);
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": sub2.0, "distribution_system": "277_480_wye" }));
    let e = run(&r, &mut doc, "panel.create_feeder", json!({ "child_panel_id": sub2.0, "parent_panel_id": mdb.0, "connection": "lugs" })).unwrap_err();
    assert_eq!(e.code, "panel.lugs_taken");
}
