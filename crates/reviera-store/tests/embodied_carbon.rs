// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embodied carbon (ANA-CARB-001), the spec's cases: a 10 m³ concrete wall at 2 400 kg/m³ and
//! 0.10 kg CO₂e/kg reports 2 400 kg CO₂e for A1–A3; 60 years over a 25-year service life is 2
//! replacements; an unmatched material is listed and its share of the volume stated; module D
//! and biogenic carbon are apart and never netted; changing a wall's thickness makes the run
//! stale for that wall alone, and running again changes only its result.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

/// A run as the compute job does it: distilled, calculated by the runner's function, recorded.
fn run_carbon(r: &CommandRegistry, doc: &mut Document, options: Value) -> Value {
    let o: reviera_core::embodied_carbon::Options = serde_json::from_value(options).unwrap();
    let d = reviera_core::embodied_carbon::distill(doc.store(), &o);
    let res = reviera_core::embodied_carbon::compute(&d.input);
    ok(r, doc, "embodied_carbon.record", json!({ "results": res, "key_map": d.key_map, "inputs": d.inputs, "factors": d.input.factors, "options": o }))
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

#[test]
fn embodied_carbon() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("carbon.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 10 m by 4 m wall of 250 mm concrete: 10 m³.
    let w1 = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [10, 0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 4.0 } })));
    let t = doc.store().iter().find(|e| reviera_core::compound::of_wall_type(e).is_some()).unwrap().id();
    let concrete = id(&ok(&r, &mut doc, "type.duplicate", json!({ "type_id": t.0, "new_name": "Concrete 250" })));
    ok(&r, &mut doc, "compound.set_layers", json!({ "type_id": concrete.0, "layers": [{ "function": "structure", "material": "Concrete C30/37", "thickness": 0.25 }] }));
    ok(&r, &mut doc, "element.set_type", json!({ "element_ids": [w1.0], "type_id": concrete.0 }));
    // A second wall, of a lined type: concrete and an unmatched plaster.
    let lined = id(&ok(&r, &mut doc, "type.duplicate", json!({ "type_id": t.0, "new_name": "Lined" })));
    ok(&r, &mut doc, "compound.set_layers", json!({ "type_id": lined.0, "layers": [
        { "function": "structure", "material": "Concrete C30/37", "thickness": 0.2 },
        { "function": "finish1", "material": "Gypsum plaster", "thickness": 0.05 }
    ] }));
    let w2 = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0, 5], "end": [5, 5] } }, "base_level": l1.0, "top_constraint": { "unconnected": 4.0 }, "type_id": lined.0 })));

    let rows = ok(&r, &mut doc, "embodied_carbon.materials", json!({}));
    let concrete_row = rows.as_array().unwrap().iter().find(|m| m["material"] == "Concrete C30/37").unwrap().clone();
    assert!((concrete_row["volume"].as_f64().unwrap() - (10.0 + 5.0 * 4.0 * 0.2)).abs() < 1e-6, "{concrete_row}");
    assert_eq!(concrete_row["status"], "unmatched");

    // Factors: concrete per kg with D and biogenic; nothing for plaster.
    let lib = ok(&r, &mut doc, "embodied_carbon.add_library", json!({ "name": "Test factors", "factors": [
        { "id": "c30", "name": "Concrete C30/37 generic", "class": "concrete", "unit": "kg",
          "stages": { "a1_a3": 0.10, "c1_c4": 0.01, "d": -0.005, "biogenic": 0.0 }, "density": 2400.0, "service_life": 25.0 },
        { "id": "clt", "name": "Cross laminated timber", "class": "timber", "unit": "m3",
          "stages": { "a1_a3": 150.0, "biogenic": -700.0 } }
    ] }));
    let props = ok(&r, &mut doc, "embodied_carbon.auto_match", json!({}));
    assert_eq!(props["Concrete C30/37"], format!("{}/c30", lib.as_str().unwrap()), "proposed by name, not applied");
    assert_eq!(ok(&r, &mut doc, "embodied_carbon.materials", json!({}))[1]["status"], "unmatched");
    ok(&r, &mut doc, "embodied_carbon.match", json!({ "material": "Concrete C30/37", "factor_ref": format!("{}/c30", lib.as_str().unwrap()) }));

    // A 60-year study: 2 replacements of A1–A3 + C1–C4.
    let res = run_carbon(&r, &mut doc, json!({ "stages": ["b4", "c1_c4"], "study_period": 60.0 }));
    let w1_guid = doc.store().get(w1).unwrap().guid().to_string();
    let e1 = &res["by_element"][&w1_guid];
    assert!((e1["a1_a3"].as_f64().unwrap() - 2400.0).abs() < 1e-6, "10 m³ × 2400 kg/m³ × 0.10: {e1}");
    assert!((e1["c1_c4"].as_f64().unwrap() - 240.0).abs() < 1e-6);
    assert!((e1["b4"].as_f64().unwrap() - 2.0 * (2400.0 + 240.0)).abs() < 1e-6, "two replacements: {e1}");
    // Module D apart, not netted.
    assert!((e1["d"].as_f64().unwrap() + 120.0).abs() < 1e-6);
    assert!((e1["total"].as_f64().unwrap() - (2400.0 + 240.0 + 5280.0)).abs() < 1e-6, "D not subtracted: {e1}");
    // Plaster unmatched, with its share of the volume.
    let plaster = 5.0 * 4.0 * 0.05;
    let all = 10.0 + 4.0 + plaster;
    assert!((res["unmatched_share"].as_f64().unwrap() - plaster / all).abs() < 1e-9);
    assert!(res["excluded"].as_array().unwrap().iter().any(|x| x["material"] == "Gypsum plaster" && x["reason"] == "unmatched"));
    let first_total = res["totals"]["total"].as_f64().unwrap();

    // Thicker concrete in the lined wall: the run is stale for that wall alone.
    ok(&r, &mut doc, "compound.set_layers", json!({ "type_id": lined.0, "layers": [
        { "function": "structure", "material": "Concrete C30/37", "thickness": 0.3 },
        { "function": "finish1", "material": "Gypsum plaster", "thickness": 0.05 }
    ] }));
    let now = ok(&r, &mut doc, "embodied_carbon.results", json!({}));
    let w2_guid = doc.store().get(w2).unwrap().guid().to_string();
    assert_eq!(now["stale"], true);
    assert_eq!(now["stale_elements"], json!([w2_guid]));
    let again = run_carbon(&r, &mut doc, json!({ "stages": ["b4", "c1_c4"], "study_period": 60.0 }));
    assert_eq!(again["by_element"][&w1_guid], res["by_element"][&w1_guid], "the first wall's result is unchanged");
    assert_ne!(again["by_element"][&w2_guid], res["by_element"][&w2_guid]);
    assert!(again["totals"]["total"].as_f64().unwrap() > first_total);
    assert_eq!(ok(&r, &mut doc, "embodied_carbon.results", json!({}))["stale"], false);
}
