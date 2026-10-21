// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical pipe connections (MEP-PIPE-009): a fan coil joined to the supply and return mains
//! only analytically contributes its flow to the loop pump; a projection beyond a pipe's end
//! attaches to the end; a return connector is not joined to a supply pipe, and the skip is
//! listed; a physical pipe on the connector replaces the connection; deleting the equipment
//! deletes it; the IFC virtual port.

use reviera_core::mep_analytical::Resolved;
use reviera_core::mep_hydronic::HydronicReport;
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

#[test]
fn fan_coil_on_the_mains() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytical.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Chiller", "Circulation Pump", "Fan Coil Unit"] }));
    let place = |doc: &mut Document, name: &str, x: f64, y: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t[name], "point": [x, y], "level": l1.0 })));
    place(&mut doc, "Chiller", 0.0, 0.0);
    let pump = place(&mut doc, "Circulation Pump", 3.0, 0.0);
    let z = 0.15;
    let pipe = |doc: &mut Document, pts: &[[f64; 2]], st: Option<&str>| {
        let mut a = json!({ "points": pts.iter().map(|p| [p[0], p[1], z]).collect::<Vec<_>>(), "level": l1.0, "diameter": 0.05 });
        if let Some(st) = st {
            a["system_type_id"] = json!(st);
        }
        ok(&r, doc, "pipe.create", a)
    };
    // Chiller → pump → a supply main ending open at x = 10; a return main from x = 10 back round
    // into the chiller.
    pipe(&mut doc, &[[1.0, 0.0], [2.8, 0.0]], None);
    let supply = ids(&pipe(&mut doc, &[[3.2, 0.0], [10.0, 0.0]], Some("hydronic_supply"))["segments"])[0];
    let ret = pipe(&mut doc, &[[10.0, 3.0], [-2.0, 3.0], [-2.0, 0.0], [-1.0, 0.0]], Some("hydronic_return"));
    let ret_main = ids(&ret["segments"])[0];
    // A fan coil between the mains, piped to neither.
    let fcu = place(&mut doc, "Fan Coil Unit", 6.0, 1.0);
    ok(&r, &mut doc, "hydronic.set_enabled", json!({ "enabled": true }));
    let rep: HydronicReport = serde_json::from_value(ok(&r, &mut doc, "hydronic.analyze", json!({}))).unwrap();
    assert!(rep.loops.is_empty(), "{rep:?}");

    // A return connector to the supply pipe: refused, naming both.
    let e = run(&r, &mut doc, "analytical_pipe_connection.create", json!({ "equipment_id": fcu.0, "connector_index": 1, "pipe_id": supply.0 })).unwrap_err();
    assert_eq!(e.message, "Connector 2: Hydronic Return, pipe is Hydronic Supply");
    // Reviera's choice within 1.6 m: only the supply main is near enough; the return is skipped.
    let out = run(&r, &mut doc, "analytical_pipe_connection.create_auto", json!({ "equipment_ids": [fcu.0], "radius": 1.6 })).unwrap();
    assert_eq!(out.result["created"].as_array().unwrap().len(), 1, "{}", out.result);
    let skipped = out.result["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].as_str().unwrap().contains("Connector 2: Hydronic Return, pipe is Hydronic Supply"), "{skipped:?}");
    assert!(out.warnings.iter().any(|w| w.id == "analytical_connection.skipped"));
    // The return by hand.
    let rc = id(&ok(&r, &mut doc, "analytical_pipe_connection.create", json!({ "equipment_id": fcu.0, "connector_index": 1, "pipe_id": ret_main.0 })));
    let list: Vec<Resolved> = serde_json::from_value(ok(&r, &mut doc, "analytical_pipe_connection.list", json!({}))).unwrap();
    assert_eq!(list.len(), 2);
    let sc = list.iter().find(|c| c.connector == 0).unwrap();
    assert_eq!(sc.pipe, supply);
    assert!((sc.point[0] - 5.5).abs() < 1e-9 && sc.point[1].abs() < 1e-9 && !sc.at_end, "{sc:?}");
    // Connected for the checks: no open fan coil connectors.
    let open = reviera_core::mep::open_ports(doc.store(), &[reviera_core::mep::Domain::Pipe]);
    assert!(open.iter().all(|p| p.element != fcu), "{open:?}");

    // Its flow reaches the pump.
    let rep: HydronicReport = serde_json::from_value(ok(&r, &mut doc, "hydronic.analyze", json!({}))).unwrap();
    assert!(rep.problems.is_empty(), "{:?}", rep.problems);
    let l = rep.loops.iter().find(|l| l.pump == pump).expect("the loop");
    assert!((l.flow - 0.0002).abs() < 1e-12, "{}", l.flow);
    assert_eq!(l.critical, Some(fcu));
    // A fixed loss on a connection adds to the head.
    let before = l.head;
    ok(&r, &mut doc, "analytical_pipe_connection.set", json!({ "connection_id": rc.0, "loss": 5000.0 }));
    let rep: HydronicReport = serde_json::from_value(ok(&r, &mut doc, "hydronic.analyze", json!({}))).unwrap();
    assert!((rep.loops[0].head - before - 5000.0).abs() < 1e-6, "{} {}", rep.loops[0].head, before);

    // Beyond the pipe's end: at the end.
    let far = place(&mut doc, "Fan Coil Unit", 12.0, 1.0);
    ok(&r, &mut doc, "analytical_pipe_connection.create", json!({ "equipment_id": far.0, "connector_index": 0, "pipe_id": supply.0 }));
    let list: Vec<Resolved> = serde_json::from_value(ok(&r, &mut doc, "analytical_pipe_connection.list", json!({}))).unwrap();
    let c = list.iter().find(|c| c.equipment == far).unwrap();
    assert!(c.at_end && (c.point[0] - 10.0).abs() < 1e-9 && c.point[1].abs() < 1e-9, "{c:?}");

    // A physical pipe from the fan coil's supply connector replaces its analytical connection.
    pipe(&mut doc, &[[5.5, 1.0], [4.5, 1.0]], None);
    let list: Vec<Resolved> = serde_json::from_value(ok(&r, &mut doc, "analytical_pipe_connection.list", json!({}))).unwrap();
    assert!(!list.iter().any(|c| c.equipment == fcu && c.connector == 0), "{list:?}");
    assert!(list.iter().any(|c| c.id == rc));
    // Deleting the far fan coil deletes its connection.
    let gone = ids(&ok(&r, &mut doc, "element.delete", json!({ "element_ids": [far.0] })));
    assert_eq!(gone.len(), 2, "{gone:?}");
    let list: Vec<Resolved> = serde_json::from_value(ok(&r, &mut doc, "analytical_pipe_connection.list", json!({}))).unwrap();
    assert_eq!(list.len(), 1);

    // IFC: a virtual port in the return main, connected to the fan coil's return port.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let port = rows.iter().find(|r| r.class == "IFCDISTRIBUTIONPORT" && r.attributes.contains("'Analytical connection'")).expect("the virtual port");
    let tag = format!("#{}", port.id.0);
    assert!(rows.iter().any(|r| r.class == "IFCRELCONNECTSPORTS" && r.attributes.contains(&format!("{tag},")) || r.class == "IFCRELCONNECTSPORTS" && r.attributes.contains(&format!("{tag})"))));
    assert!(rows.iter().any(|r| r.class == "IFCRELNESTS" && r.attributes.contains(&tag)));
}
