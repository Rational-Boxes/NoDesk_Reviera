// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe sizing (MEP-CALC-001, MEP-PIPE-005): a three-branch supply air system sized by
//! Equal Friction at 1 Pa/m, size locks, branch rules, and a domestic hot water pipe sized for the
//! converted flow of its fixture units.

use reviera_core::mep::{Domain, Shape};
use reviera_core::mep_runs::{Size, segment_recipe};
use reviera_core::mep_sizing::{AIR, Equation, flow_in};
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

/// The smallest round duct of the default series whose friction at `q` is at most 1 Pa/m.
fn smallest(q: f64) -> f64 {
    [0.1, 0.125, 0.15, 0.16, 0.2, 0.25, 0.315, 0.355, 0.4, 0.45, 0.5, 0.56, 0.63]
        .into_iter()
        .find(|d| flow_in(q, &Size { shape: Shape::Round, diameter: *d, width: 0.0, height: 0.0 }, 9.0e-5, AIR, Equation::Colebrook).1 <= 1.0)
        .unwrap()
}

#[test]
fn supply_air_sized_by_equal_friction() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("size.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
    let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [0, 0], "level": l1.0 })));
    // Three diffusers (0.1 m³/s each, Ø250 necks at 2.85 m) under the branches.
    let xs = [6.0, 9.0, 12.0];
    let diffusers: Vec<ElementId> =
        xs.iter().map(|x| id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [x, 3.0], "level": l1.0, "offset": 2.7 })))).collect();
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": diffusers.iter().map(|d| d.0).collect::<Vec<_>>() })));
    // Sizing needs the equipment.
    assert_eq!(run(&r, &mut doc, "mep.size", json!({ "system_id": sys.0 })).unwrap_err().code, "mep_size.no_equipment");
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
    // The main from the AHU's supply (1.5, 0, 0.9), up to 3.6 m and along; Ø400.
    let main = ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 0, 0.9], [3, 0, 0.9], [3, 0, 3.6], [14, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
    assert_eq!(main["connected_to"], json!([ahu.0]));
    // Branches from the main's side to each diffuser, Ø250.
    let mut branches = Vec::new();
    for x in xs {
        let b = ok(&r, &mut doc, "duct.create", json!({ "points": [[x, 0, 3.6], [x, 3, 3.6], [x, 3, 2.85]], "level": l1.0, "shape": "round", "diameter": 0.25, "system_type_id": "supply_air" }));
        // The branch's own segments (a split part of the main comes first).
        branches.extend(b["segments"].as_array().unwrap().iter().map(id).filter(|s| segment_recipe(doc.store().get(*s).unwrap()).unwrap().start[1] > 0.1));
    }
    // Lock one branch segment.
    let locked = branches[0];
    ok(&r, &mut doc, "mep_run.lock_size", json!({ "element_ids": [locked.0], "locked": true }));

    let rep = ok(&r, &mut doc, "mep.size", json!({ "system_id": sys.0, "method": "equal_friction" }));
    assert_eq!(rep["skipped"], json!([locked.0]));
    let sized = rep["sized"].as_array().unwrap();
    assert!(sized.len() >= 8, "{rep}");
    // Every segment: the smallest size whose friction is at or below 1 Pa/m at its flow.
    let mut flows: Vec<f64> = Vec::new();
    for s in sized {
        let seg = id(&s["segment"]);
        let q = s["flow"].as_f64().unwrap();
        flows.push((q * 1000.0).round() / 1000.0);
        let d = segment_recipe(doc.store().get(seg).unwrap()).unwrap().size.diameter;
        assert!((d - smallest(q)).abs() < 1e-9, "segment {} at {q} m³/s is Ø{d}", seg.0);
        assert!(s["friction"].as_f64().unwrap() <= 1.0 + 1e-9);
    }
    // The main carries 0.3 near the AHU, then 0.2 and 0.1 past the branches; branches 0.1.
    for want in [0.3, 0.2, 0.1] {
        assert!(flows.contains(&want), "{flows:?}");
    }
    assert_eq!(segment_recipe(doc.store().get(locked).unwrap()).unwrap().size.diameter, 0.25);
    // Branch rule: match the diffusers' Ø250 necks.
    let rep = ok(&r, &mut doc, "mep.size", json!({ "system_id": sys.0, "branch_rule": "match_connector" }));
    let last_branch = *branches.last().unwrap();
    assert!(rep["sized"].as_array().unwrap().iter().any(|s| id(&s["segment"]) == last_branch));
    assert_eq!(segment_recipe(doc.store().get(last_branch).unwrap()).unwrap().size.diameter, 0.25);
    // Velocity: 3 m/s needs larger ducts than 1 Pa/m does for the main.
    let rep = ok(&r, &mut doc, "mep.size", json!({ "system_id": sys.0, "method": "velocity", "targets": { "velocity": 3.0 } }));
    assert!(rep["sized"].as_array().unwrap().iter().all(|s| s["velocity"].as_f64().unwrap() <= 3.0 + 1e-9));
    let _ = Domain::Duct;
}

#[test]
fn domestic_water_by_fixture_units() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("dhw.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Water Heater", "Lavatory"] }));
    // The lavatory's hot connector: 40 fixture units, a flow of its own of 0.1 l/s, Ø22.
    ok(&r, &mut doc, "connector.set", json!({ "element_id": t["Lavatory"], "connector_index": 1, "fixture_units": 40.0, "flow": 0.0001, "diameter": 0.022 }));
    let heater = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Water Heater"], "point": [0, 10], "level": l1.0 })));
    let lav = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Lavatory"], "point": [4, 10], "level": l1.0 })));
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [lav.0], "system_type_id": "domestic_hot_water" })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": heater.0 }));
    let pipe = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0.1, 10, 1.5], [0.1, 10, 2.5], [4.08, 10, 2.5], [4.08, 11, 2.5], [4.08, 11, 0.55], [4.08, 10.2, 0.55]], "level": l1.0, "diameter": 0.022, "system_type_id": "domestic_hot_water" }));
    assert_eq!(pipe["connected_to"].as_array().unwrap().len(), 2, "{pipe}");

    // Without a table, connector flows are summed and the report says so.
    let rep = ok(&r, &mut doc, "mep.size", json!({ "system_id": sys.0, "method": "velocity" }));
    assert!(rep["conversion"].as_str().unwrap().starts_with("summed connector flows"), "{rep}");
    assert!((rep["sized"][0]["flow"].as_f64().unwrap() - 0.0001).abs() < 1e-12);
    // A table (illustrative values) refusing a row out of order, then accepted.
    let e = run(&r, &mut doc, "fixture_unit_method.set", json!({ "name": "Test", "csv": "fixture_units,tank,valve\n10,0.5,1.0\n5,0.6,1.1\n" })).unwrap_err();
    assert!(e.message.starts_with("line 3"), "{}", e.message);
    ok(&r, &mut doc, "fixture_unit_method.set", json!({ "name": "Test", "source": "illustrative", "csv": "fixture_units,tank,valve\n10,0.5,1.0\n40,1.2,2.0\n100,2.0,3.0\n" }));
    assert_eq!(ok(&r, &mut doc, "fixture_unit_method.evaluate", json!({ "fixture_units": 40.0 })), 1.2);
    // Each segment is sized for the converted flow of the 40 units beyond it, not the sum of the
    // fixtures' own flows.
    let rep = ok(&r, &mut doc, "mep.size", json!({ "system_id": sys.0, "method": "velocity" }));
    for s in rep["sized"].as_array().unwrap() {
        assert_eq!(s["fixture_units"], 40.0);
        assert!((s["flow"].as_f64().unwrap() - 0.0012).abs() < 1e-12, "{s}");
        assert!(s["velocity"].as_f64().unwrap() <= 1.5 + 1e-9);
    }
    assert!(rep["conversion"].as_str().unwrap().starts_with("Test (illustrative), flush tank"));
    // Sanitary systems are not sized by flow; a water temperature outside the table is refused.
    ok(&r, &mut doc, "system_type.set", json!({ "type_id": "domestic_hot_water", "fluid_temperature": 120.0 }));
    assert_eq!(run(&r, &mut doc, "mep.size", json!({ "system_id": sys.0 })).unwrap_err().message, "Temperature outside fluid table range");
}

#[test]
fn sizing_calculator() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("calc.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // 1 000 l/s in rectangular duct at 1 Pa/m ± 10 %: every candidate in 0.9–1.1 Pa/m.
    let c = ok(&r, &mut doc, "sizing_calculator.candidates", json!({ "flow": 1.0, "domain": "duct", "shape": "rectangular", "friction": { "target": 1.0, "tolerance": 0.1 } }));
    let list = c["candidates"].as_array().unwrap();
    assert!(!list.is_empty() && c["note"].is_null());
    assert!(list.iter().all(|x| (0.9..=1.1).contains(&x["friction"].as_f64().unwrap())), "{c}");
    // Closest to the target first; wider than high unless swapped.
    let dev = |x: &Value| x["friction_deviation"].as_f64().unwrap().abs();
    assert!(list.windows(2).all(|w| dev(&w[0]) <= dev(&w[1]) + 1e-12));
    assert!(list.iter().all(|x| x["width"].as_f64() >= x["height"].as_f64()));
    // With an aspect ratio of at most 1:3 too.
    let c = ok(&r, &mut doc, "sizing_calculator.candidates", json!({ "flow": 1.0, "domain": "duct", "shape": "rectangular", "friction": { "target": 1.0 }, "aspect": { "max": 3.0 }, "swap": true }));
    let list = c["candidates"].as_array().unwrap();
    assert!(!list.is_empty() && list.iter().all(|x| x["aspect"].as_f64().unwrap() <= 3.0 + 1e-9 && x["height"].as_f64() >= x["width"].as_f64()));
    // Nothing within: the three nearest, noted.
    let c = ok(&r, &mut doc, "sizing_calculator.candidates", json!({ "flow": 1.0, "domain": "duct", "friction": { "target": 1.0, "tolerance": 0.0 }, "velocity": { "target": 2.0, "tolerance": 0.0 } }));
    assert_eq!(c["note"], "No size within tolerance");
    assert_eq!(c["candidates"].as_array().unwrap().len(), 3);
    assert!(c["candidates"].as_array().unwrap().iter().all(|x| x["within"] == false));

    // A duct selected: its flow typed in, Apply resizes it; size-locked, Apply is refused.
    let d = ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 0], [6, 0]], "level": l1.0, "shape": "rectangular", "width": 0.4, "height": 0.3 }));
    let seg = id(&d["segments"][0]);
    let c = ok(&r, &mut doc, "sizing_calculator.candidates", json!({ "element_id": seg.0, "flow": 1.0 }));
    assert_eq!((c["shape"].as_str(), c["current"].as_str(), c["size_locked"].as_bool()), (Some("rectangular"), Some("400×300"), Some(false)), "{c}");
    let best = &c["candidates"][0];
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [seg.0], "width": best["width"], "height": best["height"] }));
    let z = segment_recipe(doc.store().get(seg).unwrap()).unwrap().size;
    assert_eq!((z.width, z.height), (best["width"].as_f64().unwrap(), best["height"].as_f64().unwrap()));
    ok(&r, &mut doc, "mep_run.lock_size", json!({ "element_ids": [seg.0], "locked": true }));
    let e = run(&r, &mut doc, "duct.set_size", json!({ "element_ids": [seg.0], "width": 0.5, "height": 0.5 })).unwrap_err();
    assert_eq!(e.message, "Element is size-locked");
    assert_eq!(segment_recipe(doc.store().get(seg).unwrap()).unwrap().size, z);
    // No flow: refused until one is typed.
    assert_eq!(run(&r, &mut doc, "sizing_calculator.candidates", json!({ "element_id": seg.0 })).unwrap_err().code, "sizing_calculator.flow");
}
