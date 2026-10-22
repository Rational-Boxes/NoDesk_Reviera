// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Air terminals and mechanical equipment (MEP-DUCT-009): three 100 L/s diffusers in a room make
//! its actual supply airflow 300 L/s (and its schedule says so); a terminal on a duct goes on
//! its bottom face joined to a tap; equipment categories map to their IFC classes; in a
//! duty/standby pump set only the duty pump's flow counts in the system's totals; Connect Into
//! routes from a terminal's and an air handler's connectors into mains.

use std::collections::HashMap;

use reviera_core::mep_equipment::{FlowTotals, SpaceAirflow};
use reviera_core::mep_runs::segment_recipe;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

#[test]
fn terminals_equipment_and_sets() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("eq.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A room 8 × 6 m.
    let c = [[0.0, 0.0], [8.0, 0.0], [8.0, 6.0], [0.0, 6.0]];
    for k in 0..4 {
        ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": c[k], "end": c[(k + 1) % 4] } }, "base_level": l1.0, "top_constraint": { "unconnected": 4.0 } }));
    }
    let room = id(&ok(&r, &mut doc, "room.place", json!({ "level_id": l1.0, "point": [4.0, 3.0] })));
    let t = ok(
        &r,
        &mut doc,
        "mep_type.load_starters",
        json!({ "names": ["Supply Diffuser 600", "Return Grille 600", "Air Handling Unit", "Circulation Pump", "Exhaust Fan", "Radiator"] }),
    );

    // Three 100 L/s diffusers and a 50 L/s grille in the room, one diffuser outside it.
    let terminal = |doc: &mut Document, ty: &str, x: f64, y: f64, flow: f64| {
        let rep = ok(&r, doc, "air_terminal.place", json!({ "type_id": t[ty], "point": [x, y], "level": l1.0, "offset": 2.7, "flow": flow }));
        (id(&rep["id"]), rep["room"].clone())
    };
    for x in [2.0, 4.0, 6.0] {
        let (_, at) = terminal(&mut doc, "Supply Diffuser 600", x, 2.0, 0.1);
        assert_eq!(at, json!(room.0));
    }
    terminal(&mut doc, "Return Grille 600", 4.0, 5.0, 0.05);
    let (_, outside) = terminal(&mut doc, "Supply Diffuser 600", 12.0, 2.0, 0.1);
    assert!(outside.is_null());
    let a: Vec<SpaceAirflow> = serde_json::from_value(ok(&r, &mut doc, "space.airflow", json!({ "room_ids": [room.0] }))).unwrap();
    assert!((a[0].supply - 0.3).abs() < 1e-9 && (a[0].return_air - 0.05).abs() < 1e-9 && a[0].exhaust == 0.0, "{a:?}");
    assert_eq!(a[0].terminals.len(), 4);
    let parents = HashMap::new();
    let fields = reviera_core::schedules::FieldReader::new(doc.store(), &parents);
    let cell = fields.cell(doc.store().get(room).unwrap(), "room.supply_airflow");
    assert_eq!(cell.text, "300.0 L/s");
    // A diffuser is not mechanical equipment, nor an air handler a terminal.
    assert!(run(&r, &mut doc, "mech_equipment.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [1, 1], "level": l1.0 })).is_err());
    assert!(run(&r, &mut doc, "air_terminal.place", json!({ "type_id": t["Air Handling Unit"], "point": [1, 1], "level": l1.0 })).is_err());

    // On a 400 mm duct at 3.2 m: on its bottom face, joined to a new tap port.
    let duct = id(&ok(&r, &mut doc, "duct.create", json!({ "points": [[0.5, 4, 3.2], [7.5, 4, 3.2]], "level": l1.0, "shape": "round", "diameter": 0.4 }))["segments"][0]);
    let rep = ok(&r, &mut doc, "air_terminal.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [3, 4.3], "level": l1.0, "flow": 0.1, "duct_id": duct.0 }));
    let on_duct = id(&rep["id"]);
    assert_eq!(rep["duct"], json!(duct.0));
    let port = reviera_core::mep::port(doc.store(), on_duct, 0).unwrap();
    assert!((port.position[0] - 3.0).abs() < 1e-9 && (port.position[1] - 4.0).abs() < 1e-9 && (port.position[2] - 3.0).abs() < 1e-9, "{port:?}");
    let partners = reviera_core::mep::partners(doc.store());
    assert_eq!(partners.get(&(on_duct, 0)), Some(&vec![(duct, 2)]), "on the duct's tap");
    let a: Vec<SpaceAirflow> = serde_json::from_value(ok(&r, &mut doc, "space.airflow", json!({}))).unwrap();
    assert!((a[0].supply - 0.4).abs() < 1e-9);

    // Categories: an exhaust fan is an IfcFan, a radiator an IfcSpaceHeater RADIATOR, an air
    // handler AIRHANDLER.
    for (ty, class, pre) in [("Exhaust Fan", "IfcFan", None), ("Radiator", "IfcSpaceHeater", Some("RADIATOR")), ("Air Handling Unit", "IfcUnitaryEquipment", Some("AIRHANDLER"))] {
        let e = id(&ok(&r, &mut doc, "mech_equipment.place", json!({ "type_id": t[ty], "point": [20, -10], "level": l1.0, "rotation": "90°" })));
        let el = doc.store().get(e).unwrap();
        assert_eq!(el.class, class);
        if let Some(pre) = pre {
            assert_eq!(el.param("@PredefinedType").and_then(Value::as_str), Some(pre));
        }
    }

    // Two pumps, duty and standby: only the duty pump's flow counts.
    let pump = |doc: &mut Document, y: f64, flow: f64| {
        let p = id(&ok(&r, doc, "mech_equipment.place", json!({ "type_id": t["Circulation Pump"], "point": [20, y], "level": l1.0 })));
        ok(&r, doc, "connector.set", json!({ "element_id": p.0, "connector_index": 1, "flow_config": "preset", "flow": flow }));
        p
    };
    let (p1, p2) = (pump(&mut doc, 0.0, 0.002), pump(&mut doc, 2.0, 0.003));
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [p1.0, p2.0], "system_type_id": "hydronic_supply" })));
    let totals = |doc: &mut Document| -> FlowTotals { serde_json::from_value(ok(&r, doc, "system.flow_totals", json!({ "system_id": sys.0 }))).unwrap() };
    assert!((totals(&mut doc).equipment_flow - 0.005).abs() < 1e-12, "both count before the set");
    let set = id(&ok(&r, &mut doc, "equipment_set.create", json!({ "equipment_ids": [p1.0, p2.0], "duty_ids": [p1.0] })));
    let tt = totals(&mut doc);
    assert!((tt.equipment_flow - 0.002).abs() < 1e-12, "{tt:?}");
    assert!(tt.equipment.iter().any(|x| x.element == p2 && x.standby));
    assert_eq!(doc.store().get(set).unwrap().param("@ObjectType").and_then(Value::as_str), Some("EquipmentSet"));
    ok(&r, &mut doc, "equipment_set.set_duty", json!({ "set_id": set.0, "duty_ids": [p2.0] }));
    assert!((totals(&mut doc).equipment_flow - 0.003).abs() < 1e-12);
    assert!(run(&r, &mut doc, "equipment_set.remove", json!({ "set_id": set.0, "equipment_ids": [p2.0] })).is_err(), "no duty unit left");
    assert!(run(&r, &mut doc, "equipment_set.create", json!({ "equipment_ids": [p1.0, p2.0], "duty_ids": [p1.0] })).is_err(), "already in a set");

    // Connect Into: a diffuser below and beside a main goes up, then across into it.
    let main = id(&ok(&r, &mut doc, "duct.create", json!({ "points": [[0.5, 1, 3.2], [7.5, 1, 3.2]], "level": l1.0, "shape": "round", "diameter": 0.5 }))["segments"][0]);
    let (d, _) = terminal(&mut doc, "Supply Diffuser 600", 5.0, 2.5, 0.1);
    let rep = ok(&r, &mut doc, "mep.connect_into", json!({ "element_id": d.0, "connector_index": 0, "target_id": main.0 }));
    assert_eq!(rep["segments"].as_array().unwrap().len(), 3, "two legs and the main's second half past the tee: {rep}");
    assert_eq!(rep["fittings"].as_array().unwrap().len(), 2, "an elbow and the junction: {rep}");
    let partners = reviera_core::mep::partners(doc.store());
    assert!(partners.contains_key(&(d, 0)), "joined to the diffuser");
    let legs: Vec<f64> = rep["segments"].as_array().unwrap().iter().map(|s| segment_recipe(doc.store().get(id(s)).unwrap()).unwrap().size.diameter).collect();
    assert_eq!(legs.iter().filter(|d| (**d - 0.25).abs() < 1e-9).count(), 2, "the legs at the connector's size: {legs:?}");
    // An air handler's side connector: out, up, then square into a main along y.
    let ahu = id(&ok(&r, &mut doc, "mech_equipment.place", json!({ "type_id": t["Air Handling Unit"], "point": [20, 10], "level": l1.0 })));
    let ymain = id(&ok(&r, &mut doc, "duct.create", json!({ "points": [[30, 5, 3.2], [30, 15, 3.2]], "level": l1.0, "shape": "round", "diameter": 0.6 }))["segments"][0]);
    let rep = ok(&r, &mut doc, "mep.connect_into", json!({ "element_id": ahu.0, "connector_index": 0, "target_id": ymain.0 }));
    assert_eq!(rep["segments"].as_array().unwrap().len(), 4, "three legs and the main's second half: {rep}");
    assert!(reviera_core::mep::partners(doc.store()).contains_key(&(ahu, 0)));
    // Facing away: refused.
    let e = run(&r, &mut doc, "mep.connect_into", json!({ "element_id": ahu.0, "connector_index": 1, "target_id": ymain.0 }));
    assert!(e.is_err() || e.is_ok_and(|o| !o.warnings.is_empty()));
}
