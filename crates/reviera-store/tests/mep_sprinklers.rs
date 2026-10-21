// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sprinklers (MEP-PIPE-007): a head with K = 80 and 68 °C activation exports those values in
//! Pset_FireSuppressionTerminalTypeSprinkler; pendent heads hang from the ceiling over them and
//! follow it; upright and sidewall heads; the ceiling deleted.

use reviera_core::mesh_objects::regenerate;
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

fn square(a: f64, b: f64) -> Value {
    json!([[
        { "line": { "start": [a, a], "end": [b, a] } },
        { "line": { "start": [b, a], "end": [b, b] } },
        { "line": { "start": [b, b], "end": [a, b] } },
        { "line": { "start": [a, b], "end": [a, a] } },
    ]])
}

#[test]
fn sprinkler_heads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sprinklers.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Pendent Sprinkler", "Upright Sprinkler", "Sidewall Sprinkler", "Lavatory"] }));
    let pendent = id(&t["Pendent Sprinkler"]);

    // The head: K 80, 68 °C, quick response; a red bulb.
    ok(&r, &mut doc, "sprinkler_type.set", json!({ "type_id": pendent.0, "k_factor": 80.0, "activation_temperature": 68.0, "response": "quick", "coverage_area": 12.0 }));
    let ty = doc.store().get(pendent).unwrap();
    let prop = |e: &reviera_core::Element, p: &str| e.param(&format!("Pset_FireSuppressionTerminalTypeSprinkler.{p}")).map(|v| v["value"].clone());
    assert_eq!(prop(ty, "BulbLiquidColor"), Some(json!("RED")));
    // A 93 °C rating: green.
    ok(&r, &mut doc, "sprinkler_type.set", json!({ "type_id": id(&t["Upright Sprinkler"]).0, "activation_temperature": 93.0 }));
    assert_eq!(prop(doc.store().get(id(&t["Upright Sprinkler"])).unwrap(), "BulbLiquidColor"), Some(json!("GREEN")));
    // Not a sprinkler: refused.
    let e = run(&r, &mut doc, "sprinkler_type.set", json!({ "type_id": t["Lavatory"], "k_factor": 80.0 })).unwrap_err();
    assert_eq!(e.code, "sprinkler.not_a_sprinkler");
    assert_eq!(run(&r, &mut doc, "sprinkler.place", json!({ "type_id": t["Lavatory"], "point": [1, 1], "level": l1.0 })).unwrap_err().code, "sprinkler.not_a_sprinkler");
    assert_eq!(run(&r, &mut doc, "sprinkler_type.set", json!({ "type_id": pendent.0, "k_factor": -1.0 })).unwrap_err().code, "sprinkler.bad_value");

    // Pendent heads under a ceiling at 2.6 m: their top at its underside; they follow it.
    let ceiling = id(&ok(&r, &mut doc, "ceiling.create", json!({ "loops": square(0.0, 6.0), "level_id": l1.0 })));
    let heads: Vec<ElementId> = [[1.5, 1.5], [4.5, 1.5], [1.5, 4.5], [4.5, 4.5]]
        .iter()
        .map(|p| id(&ok(&r, &mut doc, "sprinkler.place", json!({ "type_id": pendent.0, "point": p, "level": l1.0 }))))
        .collect();
    let z = |doc: &Document, h: ElementId| regenerate(doc.store(), doc.store().get(h).unwrap()).unwrap().placement.origin[2];
    assert!((z(&doc, heads[0]) - (2.6 - 0.07)).abs() < 1e-9, "{}", z(&doc, heads[0]));
    let e = doc.store().get(heads[0]).unwrap();
    assert_eq!((e.class.as_str(), e.param("@PredefinedType").and_then(Value::as_str)), ("IfcFireSuppressionTerminal", Some("SPRINKLER")));
    let inlet = &reviera_core::mep::ports(doc.store(), e)[0];
    assert!((inlet.position[2] - 2.6).abs() < 1e-9 && inlet.direction[2] > 0.99);
    ok(&r, &mut doc, "floor.set_level", json!({ "floor_ids": [ceiling.0], "offset": 2.8 }));
    assert!(heads.iter().all(|h| (z(&doc, *h) - 2.73).abs() < 1e-9));
    // Outside the ceiling: at the default ceiling height.
    let free = id(&ok(&r, &mut doc, "sprinkler.place", json!({ "type_id": pendent.0, "point": [9.0, 1.0], "level": l1.0 })));
    assert!((z(&doc, free) - 2.53).abs() < 1e-9);

    // An upright head 3 m up, piped from below; a sidewall head on a wall face.
    let up = id(&ok(&r, &mut doc, "sprinkler.place", json!({ "type_id": t["Upright Sprinkler"], "point": [9.0, 3.0], "level": l1.0 })));
    assert!((z(&doc, up) - 3.0).abs() < 1e-9);
    let p = &reviera_core::mep::ports(doc.store(), doc.store().get(up).unwrap())[0];
    assert!(p.direction[2] < -0.99);
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [10.0, 0.0], "end": [10.0, 6.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let side = id(&ok(&r, &mut doc, "sprinkler.place", json!({ "type_id": t["Sidewall Sprinkler"], "point": [9.8, 2.0], "level": l1.0 })));
    let sr = reviera_core::mesh_objects::mesh_object_recipe(doc.store().get(side).unwrap()).unwrap();
    assert!(sr.face.is_some());
    assert!((z(&doc, side) - 2.45).abs() < 1e-9);
    assert_eq!(run(&r, &mut doc, "sprinkler.place", json!({ "type_id": pendent.0, "point": [9.8, 2.0], "level": l1.0, "host_wall": wall.0 })).unwrap_err().code, "sprinkler.not_sidewall");

    // A wet system of the heads.
    ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": heads.iter().map(|h| h.0).collect::<Vec<_>>(), "system_type_id": "fire_protection_wet" }));

    // IFC: the head's values in the type's pset (68 °C in kelvin).
    project.save(&doc).unwrap();
    let model = IfcModel::open(&path).unwrap();
    let rows = model.step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCFIRESUPPRESSIONTERMINALTYPE"));
    assert_eq!(rows.iter().filter(|r| r.class == "IFCFIRESUPPRESSIONTERMINAL" && r.attributes.contains(".SPRINKLER.")).count(), 7);
    let pset = rows.iter().find(|r| r.class == "IFCPROPERTYSET" && r.attributes.contains("'Pset_FireSuppressionTerminalTypeSprinkler'")).expect("the pset");
    let refs: Vec<&str> = pset.attributes.split(['(', ')', ',']).filter(|s| s.starts_with('#')).collect();
    let props: Vec<&String> = rows.iter().filter(|r| r.class == "IFCPROPERTYSINGLEVALUE" && refs.contains(&format!("#{}", r.id.0).as_str())).map(|r| &r.attributes).collect();
    assert!(props.iter().any(|a| a.contains("'DischargeCoefficient'") && a.contains("IFCREAL(80.")), "{props:?}");
    assert!(props.iter().any(|a| a.contains("'ActivationTemperature'") && a.contains("IFCTHERMODYNAMICTEMPERATUREMEASURE(341.1")), "{props:?}");

    // The ceiling deleted: the heads stay where they are, with a warning.
    let before = z(&doc, heads[1]);
    let out = run(&r, &mut doc, "element.delete", json!({ "element_ids": [ceiling.0] })).unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "mep.unhosted" && w.element_ids.contains(&heads[1])));
    assert!((z(&doc, heads[1]) - before).abs() < 1e-9);
}
