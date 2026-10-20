// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Circuit paths, cable sizing and voltage drop (MEP-ELEC-004, MEP-ELEC-005, MEP-SET-003): a
//! 2 kW 230 V circuit sized against a hand calculation, a rating that needs a larger conductor,
//! a size given by hand kept with a warning, a fixed 5 × 2.5 mm² cable type, and path modes,
//! offsets and custom paths.

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

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{v}"))
}

/// Copper at PVC's 70 °C: Ω per metre of `area` mm².
fn r_cu70(area: f64) -> f64 {
    0.017_241 * (1.0 + 0.003_93 * 50.0) / area
}

#[test]
fn cable_sizing_and_voltage_drop() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("cables.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Distribution Board", "Receptacle"] }));
    // The receptacle type made a 2 kW load at power factor 1.
    ok(&r, &mut doc, "connector.set", json!({ "element_id": t["Receptacle"], "connector_index": 0, "electrical": { "system": "power_balanced", "voltage": 230.0, "poles": 1, "load": [2000.0], "power_factor": 1.0, "lagging": true, "load_classification": "Power", "motor": false } }));
    let place = |doc: &mut Document, ty: &str, x: f64, y: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t[ty], "point": [x, y], "level": l1.0 })));
    let db = place(&mut doc, "Distribution Board", 0.0, 0.0);
    let far = place(&mut doc, "Receptacle", 38.0, 0.0);
    let near = place(&mut doc, "Receptacle", 18.0, 0.0);
    let circuit = |doc: &mut Document, dev: ElementId| {
        let c = id(&ok(&r, doc, "circuit.create", json!({ "kind": "power", "element_ids": [dev.0] }))["circuit"]);
        ok(&r, doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": db.0 }));
        c
    };
    let c40 = circuit(&mut doc, far);
    let c20 = circuit(&mut doc, near);

    // The far circuit: the smallest IEC size whose drop is within 3 %, matching the hand
    // calculation 2 × I × L × R (power factor 1, so reactance adds nothing).
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c40.0, "rating": 16 }));
    let p = ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c40.0 }));
    let (i, l) = (f(&p["current"]), f(&p["length"]));
    assert!((i - 2000.0 / 230.0).abs() < 1e-9);
    assert!((38.0..42.0).contains(&l), "{l}");
    let pct = |area: f64| 100.0 * 2.0 * i * l * r_cu70(area) / 230.0;
    let want = [1.0, 1.5, 2.5, 4.0, 6.0].into_iter().find(|a| pct(*a) <= 3.0).unwrap();
    assert_eq!(p["cable_size"], format!("{want} mm²"));
    assert!((f(&p["voltage_drop_percent"]) - pct(want)).abs() < 0.1, "{p}");
    assert_eq!(p["cable_size_auto"], true);
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("No ampacity table")), "{p}");
    assert_eq!(p["phases"], "A");

    // Ampacities (the user's table; illustrative values): the near circuit's 16 A fits 1.5 mm²;
    // a 20 A rating needs 2.5 mm², and sizing re-runs when the rating changes.
    ok(&r, &mut doc, "conductor_ampacity.set", json!({ "rows": [
        { "material": "copper", "insulation": "pvc", "size": "1_5mm2", "amperes": 17.0 },
        { "material": "copper", "insulation": "pvc", "size": "2_5mm2", "amperes": 24.0 },
        { "material": "copper", "insulation": "pvc", "size": "4mm2", "amperes": 32.0 },
    ] }));
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c20.0, "rating": 16 }));
    assert_eq!(ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c20.0 }))["cable_size"], "1.5 mm²");
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c20.0, "rating": 20 }));
    let p = ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c20.0 }));
    assert_eq!((p["cable_size"].as_str(), f(&p["ampacity"])), (Some("2.5 mm²"), 24.0));
    // A correction factor of 0.8 makes 2.5 mm² carry 19.2 A: 4 mm² then.
    ok(&r, &mut doc, "wiring.set", json!({ "correction_factor": 0.8 }));
    assert_eq!(ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c20.0 }))["cable_size"], "4 mm²");
    ok(&r, &mut doc, "wiring.set", json!({ "correction_factor": 1.0 }));

    // A size given by hand is kept, with a warning when it fails.
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c40.0, "cable_size": "1_5mm2" }));
    let p = ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c40.0 }));
    assert_eq!((p["cable_size"].as_str(), p["cable_size_auto"].as_bool()), (Some("1.5 mm²"), Some(false)));
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("over the 3 % limit")), "{p}");
    // Size Cable drops it again, here with a 5 % limit.
    let p = ok(&r, &mut doc, "circuit.size_cable", json!({ "circuit_id": c40.0, "max_vd_percent": 5.0 }));
    assert_eq!(p["cable_size_auto"], true);
    assert!(f(&p["voltage_drop_percent"]) <= 5.0);

    // A fixed 5 × 2.5 mm² Cu/PVC cable type: chosen, and the drop uses its conductors.
    let ct = ok(&r, &mut doc, "cable_type.create", json!({ "name": "5 × 2.5 mm² Cu/PVC", "material_id": "copper", "insulation_id": "pvc", "phase_conductors": { "count": 5, "size_id": "2_5mm2" }, "neutral": true, "ground": true }));
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c40.0, "cable_type": ct }));
    let p = ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c40.0 }));
    assert_eq!((p["cable_type"].as_str(), p["cable_size"].as_str()), (Some("5 × 2.5 mm² Cu/PVC"), Some("2.5 mm²")));
    assert!((f(&p["voltage_drop_percent"]) - pct(2.5)).abs() < 1e-6);
    // Two runs halve the drop.
    ok(&r, &mut doc, "circuit.set", json!({ "circuit_id": c40.0, "runs": 2 }));
    assert!((f(&ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c40.0 }))["voltage_drop_percent"]) - pct(2.5) / 2.0).abs() < 1e-6);
    // AWG sizes by their definition: 10 AWG is about 5.26 mm².
    let awg = ok(&r, &mut doc, "conductor_size.create", json!({ "name": "10 AWG test", "gauge": "10" }));
    assert!(awg.as_str().is_some());
}

#[test]
fn circuit_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("paths.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Distribution Board", "Receptacle"] }));
    let place = |doc: &mut Document, ty: &str, x: f64, y: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t[ty], "point": [x, y], "level": l1.0 })));
    let db = place(&mut doc, "Distribution Board", 0.0, 0.0);
    let devs: Vec<ElementId> = [(4.0, 0.0), (8.0, 0.0), (8.0, 5.0), (2.0, 6.0)].iter().map(|(x, y)| place(&mut doc, "Receptacle", *x, *y)).collect();
    let c = id(&ok(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": devs.iter().map(|d| d.0).collect::<Vec<_>>() }))["circuit"]);
    // No panel: no path.
    assert_eq!(run(&r, &mut doc, "circuit.set_path", json!({ "circuit_id": c.0, "mode": "all_devices" })).unwrap_err().code, "circuit.no_panel");
    ok(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": db.0 }));
    let len = |doc: &mut Document| f(&ok(&r, doc, "circuit.path", json!({ "circuit_id": c.0 }))["length"]);
    let drop = |doc: &mut Document| f(&ok(&r, doc, "circuit.properties", json!({ "circuit_id": c.0 }))["voltage_drop"]);
    let farthest = len(&mut doc);
    let (v0, rise) = {
        let p = ok(&r, &mut doc, "circuit.path", json!({ "circuit_id": c.0 }));
        let pts = p["points"].as_array().unwrap().clone();
        let z = |k: usize| f(&pts[k][2]);
        (drop(&mut doc), (z(0) - z(pts.len() - 1)).abs())
    };
    // The farthest device is (8, 5): 8 + 5 across, plus the rise.
    assert!((farthest - (13.0 + rise)).abs() < 0.3, "{farthest} {rise}");
    // All Devices is longer, and the drop grows with it.
    ok(&r, &mut doc, "circuit.set_path", json!({ "circuit_id": c.0, "mode": "all_devices" }));
    let all = len(&mut doc);
    assert!(all > farthest + 1.0, "{all} {farthest}");
    assert!(drop(&mut doc) > v0);
    // A 1 m offset adds 2 m to the Farthest Device path, up and down.
    ok(&r, &mut doc, "circuit.set_path", json!({ "circuit_id": c.0, "mode": "farthest_device" }));
    ok(&r, &mut doc, "circuit.set_path", json!({ "circuit_id": c.0, "mode": "farthest_device", "offset": 1.0 }));
    assert!((len(&mut doc) - (farthest + 2.0)).abs() < 1e-9);
    // A custom path is flagged when a device moves; Reset Path clears it.
    ok(&r, &mut doc, "circuit.set_path", json!({ "circuit_id": c.0, "mode": "custom", "nodes": [[0.0, 3.0, 3.0], [8.0, 3.0, 3.0]] }));
    assert_eq!(ok(&r, &mut doc, "circuit.path", json!({ "circuit_id": c.0 }))["out_of_date"], false);
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [devs[1].0], "vector": [0.5, 0.0, 0.0] }));
    let p = ok(&r, &mut doc, "circuit.properties", json!({ "circuit_id": c.0 }));
    assert_eq!(p["path_out_of_date"], true);
    assert!(p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("out of date")));
    ok(&r, &mut doc, "circuit.reset_path", json!({ "circuit_id": c.0 }));
    let p = ok(&r, &mut doc, "circuit.path", json!({ "circuit_id": c.0 }));
    assert_eq!((p["out_of_date"].as_bool(), p["mode"].as_str()), (Some(false), Some("farthest_device")));
}
