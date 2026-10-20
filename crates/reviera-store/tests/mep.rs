// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP connectors, connections, systems and checks (MEP-CONN-001, MEP-CONN-002, MEP-SYS-001,
//! MEP-SYS-002, MEP-SYS-005): a VAV box type's two ports in IFC, connections refused across
//! domains and undone exactly, a supply air system of eight diffusers and an AHU, Divide System,
//! a domestic cold water system in IFC, type colours, duplicate names, and the checks.

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

fn level(doc: &Document) -> ElementId {
    doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id()
}

#[test]
fn connectors_connections_and_ports_in_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mep.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = level(&doc);
    // A VAV box: one inlet and one outlet duct connector.
    let vav = id(&ok(&r, &mut doc, "mep_type.create", json!({
        "name": "VAV Box", "category": "mechanical_equipment", "size": [0.8, 0.5, 0.4],
        "connectors": [
            { "domain": "duct", "name": "Inlet", "origin": [-0.4, 0.0, 0.2], "direction": [-1, 0, 0], "diameter": 0.25, "classification": "supply_air", "flow_direction": "in" },
            { "domain": "duct", "name": "Outlet", "origin": [0.4, 0.0, 0.2], "direction": [1, 0, 0], "diameter": 0.25, "classification": "supply_air", "flow_direction": "out" }
        ]
    })));
    let listed = ok(&r, &mut doc, "connector.list", json!({ "element_id": vav.0 }));
    assert_eq!(listed.as_array().unwrap().len(), 2);
    // Two copies, the second's inlet on the first's outlet: they connect as placed.
    let a = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": vav.0, "point": [0.0, 0.0], "level": l1.0, "offset": 3.0 })));
    let b = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": vav.0, "point": [0.8, 0.0], "level": l1.0, "offset": 3.0 })));
    let ports = ok(&r, &mut doc, "connector.list", json!({ "element_id": b.0 }));
    assert_eq!(ports[0]["connected_to"], json!([[a.0, 1]]));
    let p0 = &ports[0]["position"];
    assert!((p0[0].as_f64().unwrap() - 0.4).abs() < 1e-9 && (p0[2].as_f64().unwrap() - 3.2).abs() < 1e-9, "{p0}");
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": a.0 })), json!([a.0, b.0]));

    // A pipe connector cannot join a duct connector; nothing is written.
    let heater = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Water Heater"] }))["Water Heater"].clone();
    let w = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": heater, "point": [5.0, 0.0], "level": l1.0 })));
    let before = doc.store().iter().find(|e| e.class == "IfcProject").unwrap().param("rxd:mep_connections").cloned();
    let e = run(&r, &mut doc, "mep.connect", json!({ "port_a": { "element": a.0, "port": 0 }, "port_b": { "element": w.0, "port": 0 } })).unwrap_err();
    assert_eq!(e.code, "mep.domain");
    assert_eq!(doc.store().iter().find(|e| e.class == "IfcProject").unwrap().param("rxd:mep_connections").cloned(), before);
    // A port that has a partner takes no other.
    let c = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": vav.0, "point": [3.0, 0.0], "level": l1.0 })));
    let e = run(&r, &mut doc, "mep.connect", json!({ "port_a": { "element": a.0, "port": 1 }, "port_b": { "element": c.0, "port": 0 } })).unwrap_err();
    assert_eq!(e.code, "mep.taken");
    // Undoing a connection and redoing it gives the same network.
    ok(&r, &mut doc, "mep.connect", json!({ "port_a": { "element": b.0, "port": 1 }, "port_b": { "element": c.0, "port": 0 } }));
    let net = ok(&r, &mut doc, "mep.network_of", json!({ "element_id": c.0 }));
    assert_eq!(net.as_array().unwrap().len(), 3);
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": c.0 })), json!([c.0]));
    ok(&r, &mut doc, "edit.redo", json!({}));
    assert_eq!(ok(&r, &mut doc, "mep.network_of", json!({ "element_id": c.0 })), net);

    // A copy's flow can be its own; its size cannot.
    ok(&r, &mut doc, "connector.set", json!({ "element_id": a.0, "connector_index": 0, "flow_config": "preset", "flow": 0.3 }));
    let e = run(&r, &mut doc, "connector.set", json!({ "element_id": a.0, "connector_index": 0, "diameter": 0.3 })).unwrap_err();
    assert_eq!(e.code, "connector.type_value");
    let pa = ok(&r, &mut doc, "connector.list", json!({ "element_id": a.0 }));
    assert_eq!(pa[0]["connector"]["flow"], 0.3);
    assert_eq!(ok(&r, &mut doc, "connector.list", json!({ "element_id": b.0 }))[0]["connector"]["flow"], 0.0);

    // Deleting the middle box leaves open ports, which Show Disconnects reports.
    let open_before = ok(&r, &mut doc, "mep_check.show_disconnects", json!({ "domains": ["duct"] })).as_array().unwrap().len();
    ok(&r, &mut doc, "element.delete", json!({ "element_ids": [b.0] }));
    let open = ok(&r, &mut doc, "mep_check.show_disconnects", json!({ "domains": ["duct"] }));
    let open: Vec<(u64, u64)> = open.as_array().unwrap().iter().map(|p| (p["element"].as_u64().unwrap(), p["index"].as_u64().unwrap())).collect();
    assert!(open.contains(&(a.0, 1)) && open.contains(&(c.0, 0)), "{open:?}");
    assert_eq!(open.len(), open_before + 2);
    // Only the conduit domain: no duct ends.
    assert_eq!(ok(&r, &mut doc, "mep_check.show_disconnects", json!({ "domains": ["conduit"] })), json!([]));

    // IFC: each VAV box's two ports nested, SINK and SOURCE, and one connection (a to c).
    ok(&r, &mut doc, "mep.connect", json!({ "port_a": { "element": a.0, "port": 1 }, "port_b": { "element": c.0, "port": 0 } }));
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    // a, c (two each) and the water heater (two).
    assert_eq!(count("IFCDISTRIBUTIONPORT"), 6);
    assert_eq!(count("IFCRELNESTS"), 3);
    assert_eq!(count("IFCRELCONNECTSPORTS"), 1);
    let ports: Vec<&str> = rows.iter().filter(|r| r.class == "IFCDISTRIBUTIONPORT").map(|r| r.attributes.as_str()).collect();
    assert!(ports.iter().any(|a| a.contains(".SINK.") && a.contains(".DUCT.")), "{ports:?}");
    assert!(ports.iter().any(|a| a.contains(".SOURCE.") && a.contains(".DUCT.")));
    assert!(ports.iter().any(|a| a.contains(".PIPE.") && a.contains(".DOMESTICHOTWATER.")));
    // Saving again keeps the ports' identities.
    let ids: Vec<String> = rows.iter().filter(|r| r.class == "IFCDISTRIBUTIONPORT").filter_map(|r| r.global_id.clone()).collect();
    ok(&r, &mut doc, "mesh_object.set", json!({ "element_ids": [w.0], "point": [6.0, 0.0] }));
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let again: Vec<String> = rows.iter().filter(|r| r.class == "IFCDISTRIBUTIONPORT").filter_map(|r| r.global_id.clone()).collect();
    assert_eq!(ids.len(), again.len());
    assert!(ids.iter().all(|g| again.contains(g)));
    // Reopened: the ports are not read back as elements; the connectors and connection are kept.
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert!(!doc2.store().iter().any(|e| e.class == "IfcDistributionPort"));
    assert_eq!(reviera_core::mep::connections(doc2.store()).len(), 1);
}

#[test]
fn systems_types_and_checks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sys.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = level(&doc);
    let types = ok(&r, &mut doc, "mep_type.load_starters", json!({}));
    let place = |doc: &mut Document, t: &str, x: f64, y: f64| {
        id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": types[t], "point": [x, y], "level": l1.0, "offset": 2.7 })))
    };
    let diffusers: Vec<ElementId> = (0..8).map(|k| place(&mut doc, "Supply Diffuser 600", k as f64 * 2.0, 5.0)).collect();
    let ahu = place(&mut doc, "Air Handling Unit", 0.0, 0.0);
    // Eight diffusers, then the AHU as equipment: nine members, the AHU its source.
    let sa = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": diffusers.iter().map(|d| d.0).collect::<Vec<_>>() })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sa.0, "equipment_id": ahu.0 }));
    let s = &ok(&r, &mut doc, "system.list", json!({}))[0];
    assert_eq!(s["name"], "Supply Air 1");
    assert_eq!(s["elements"].as_array().unwrap().len(), 9);
    assert_eq!(s["equipment"], ahu.0);
    assert_eq!(s["classification"], "supply_air");
    // A return grille cannot join a supply air system.
    let grille = place(&mut doc, "Return Grille 600", 20.0, 0.0);
    let e = run(&r, &mut doc, "system.add", json!({ "system_id": sa.0, "element_ids": [grille.0] })).unwrap_err();
    assert_eq!(e.code, "system.incompatible");
    // A diffuser's connector is in one system at a time.
    let e = run(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [diffusers[0].0] })).unwrap_err();
    assert_eq!(e.code, "system.taken");
    // The new diffuser is Unassigned until it joins.
    let extra = place(&mut doc, "Supply Diffuser 600", 30.0, 5.0);
    let un = ok(&r, &mut doc, "system.unassigned", json!({}));
    assert!(un["Duct"].as_array().unwrap().contains(&json!([extra.0, 0])), "{un}");

    // Not well connected: no physical connections to the AHU yet.
    let issues = ok(&r, &mut doc, "mep_check.systems", json!({ "kind": "duct" }));
    assert!(issues.as_array().unwrap().iter().any(|i| i["kind"] == "not_well_connected" && i["message"].as_str().unwrap().contains("Supply Air 1")), "{issues}");
    let ra = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [grille.0], "name": "Return West" })));
    let issues = ok(&r, &mut doc, "mep_check.systems", json!({ "kind": "duct" }));
    assert!(issues.as_array().unwrap().iter().any(|i| i["message"] == "Return West has no equipment"), "{issues}");

    // Divide: two pumps, each in its own network, in one hydronic supply system.
    let p1 = place(&mut doc, "Circulation Pump", 40.0, 0.0);
    let p2 = place(&mut doc, "Circulation Pump", 50.0, 0.0);
    let hs = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [p1.0, p2.0], "system_type_id": "hydronic_supply", "name": "Heating" })));
    let parts = ok(&r, &mut doc, "system.divide", json!({ "system_id": hs.0 }));
    assert_eq!(parts.as_array().unwrap().len(), 2);
    let names: Vec<String> = ok(&r, &mut doc, "system.list", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["classification"] == "hydronic_supply")
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Heating 1", "Heating 2"]);
    let e = run(&r, &mut doc, "system.divide", json!({ "system_id": hs.0 })).unwrap_err();
    assert_eq!(e.code, "system.one_network");
    // Renaming to another system's name in the classification is refused.
    let e = run(&r, &mut doc, "system.set", json!({ "system_id": hs.0, "name": "heating 2" })).unwrap_err();
    assert_eq!(e.message, "A system with this name already exists");
    // Two connected Out connectors: a flow direction mismatch naming both.
    ok(&r, &mut doc, "connector.set", json!({ "element_id": p1.0, "connector_index": 0, "flow_direction": "out" }));
    ok(&r, &mut doc, "mep.connect", json!({ "port_a": { "element": p1.0, "port": 0 }, "port_b": { "element": p2.0, "port": 1 } }));
    let issues = ok(&r, &mut doc, "mep_check.systems", json!({ "kind": "pipe" }));
    let dir_issues: Vec<&Value> = issues.as_array().unwrap().iter().filter(|i| i["kind"] == "flow_direction").collect();
    assert_eq!(dir_issues.len(), 1, "{issues}");
    assert_eq!(dir_issues[0]["element_ids"], json!([p1.0, p2.0]));

    // A domestic cold water system of the water heater and two lavatories.
    let wh = place(&mut doc, "Water Heater", 60.0, 0.0);
    let lav1 = place(&mut doc, "Lavatory", 62.0, 0.0);
    let lav2 = place(&mut doc, "Lavatory", 64.0, 0.0);
    let dcw = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [lav1.0, lav2.0, wh.0], "system_type_id": "domestic_cold_water" })));
    // A type's colour recolours its systems' members.
    ok(&r, &mut doc, "system_type.set", json!({ "type_id": "domestic_cold_water", "color": [0.0, 1.0, 0.0, 1.0] }));
    let colors = reviera_core::mep_systems::element_colors(doc.store());
    assert_eq!(colors[&lav1], [0.0, 1.0, 0.0, 1.0]);
    // A made type, and a duplicate name refused.
    let t = ok(&r, &mut doc, "system_type.create", json!({ "classification": "supply_air", "name": "Supply Air - Lab", "color": [1.0, 0.5, 0.0, 1.0] }));
    assert_eq!(t, "t1");
    let e = run(&r, &mut doc, "system_type.create", json!({ "classification": "supply_air", "name": "supply air - lab" })).unwrap_err();
    assert_eq!(e.code, "system_type.duplicate");
    ok(&r, &mut doc, "system.set", json!({ "system_id": sa.0, "system_type_id": "t1" }));
    let e = run(&r, &mut doc, "system.set", json!({ "system_id": ra.0, "system_type_id": "t1" })).unwrap_err();
    assert_eq!(e.code, "system.classification");

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let dcw_row = rows
        .iter()
        .find(|r| r.class == "IFCDISTRIBUTIONSYSTEM" && r.attributes.contains(".DOMESTICCOLDWATER."))
        .expect("a DCW system");
    let groups: Vec<&reviera_ifc::StepRow> = rows.iter().filter(|r| r.class == "IFCRELASSIGNSTOGROUP" && r.attributes.contains(&format!("#{})", dcw_row.id.0))).collect();
    assert_eq!(groups.len(), 1, "{groups:?}");
    // Its three members.
    let members = groups[0].attributes.matches('#').count() - 1;
    assert_eq!(members, 3, "{}", groups[0].attributes);
    assert!(rows.iter().any(|r| r.class == "IFCRELSERVICESBUILDINGS"));
    let _ = dcw;
}
