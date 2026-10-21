// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plumbing fixtures (MEP-PIPE-006): six basins on a wall, their connectors against it, in one
//! sanitary system; the wall moved 200 mm moves them; a hot water system from a basin and a WC
//! leaves the WC out and says so; the IFC types; the wall deleted.

use reviera_core::mesh_objects::{mesh_object_recipe, regenerate};
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
fn six_basins_on_a_wall() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixtures.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let w = reviera_core::walls::regenerate(doc.store(), doc.store().get(wall).unwrap()).unwrap().width;
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Lavatory", "Water Closet"] }));
    // Six basins clicked just off the wall's interior face, 1.2 m apart.
    let basins: Vec<ElementId> = (0..6)
        .map(|k| id(&ok(&r, &mut doc, "plumbing_fixture.place", json!({ "type_id": t["Lavatory"], "point": [1.0 + 1.2 * k as f64, -w / 2.0 - 0.1], "level": l1.0 }))))
        .collect();
    let e = doc.store().get(basins[0]).unwrap();
    assert_eq!((e.class.as_str(), e.param("@PredefinedType").and_then(Value::as_str)), ("IfcSanitaryTerminal", Some("WASHHANDBASIN")));
    assert!(mesh_object_recipe(e).unwrap().face.is_some());
    // Their back, with the connectors, against the face.
    let waste = reviera_core::mep::ports(doc.store(), e).into_iter().find(|p| p.connector.name == "Waste").unwrap();
    assert!((waste.position[1] + w / 2.0).abs() < 0.03, "{:?}", waste.position);
    assert!(waste.direction[1] > 0.99, "{:?}", waste.direction);
    // Not a fixture: refused.
    let th = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Thermostat"] }));
    assert_eq!(run(&r, &mut doc, "plumbing_fixture.place", json!({ "type_id": th["Thermostat"], "point": [1.0, -1.0], "level": l1.0 })).unwrap_err().code, "plumbing_fixture.not_a_fixture");

    // One sanitary system of the six.
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": basins.iter().map(|b| b.0).collect::<Vec<_>>(), "system_type_id": "sanitary" })));
    let list = ok(&r, &mut doc, "system.list", json!({}));
    let sanitary: Vec<&Value> = list.as_array().unwrap().iter().filter(|s| s["classification"] == "sanitary").collect();
    assert_eq!(sanitary.len(), 1);
    assert_eq!(sanitary[0]["id"], sys.0);
    assert!(basins.iter().all(|b| sanitary[0]["elements"].as_array().unwrap().contains(&json!(b.0))));

    // The wall moves 200 mm: the basins with it.
    let at = |doc: &Document, b: ElementId| regenerate(doc.store(), doc.store().get(b).unwrap()).unwrap().placement.origin;
    let before: Vec<[f64; 3]> = basins.iter().map(|b| at(&doc, *b)).collect();
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [wall.0], "vector": [0.0, -0.2, 0.0] }));
    for (b, p0) in basins.iter().zip(&before) {
        let p = at(&doc, *b);
        assert!((p[0] - p0[0]).abs() < 1e-9 && (p[1] - p0[1] + 0.2).abs() < 1e-9, "{p:?} {p0:?}");
    }

    // Hot water from a basin and a WC: the WC is left out and listed.
    let wc = id(&ok(&r, &mut doc, "plumbing_fixture.place", json!({ "type_id": t["Water Closet"], "point": [7.5, -3.0], "level": l1.0, "work_plane": true })));
    assert_eq!(doc.store().get(wc).unwrap().param("@PredefinedType").and_then(Value::as_str), Some("TOILETPAN"));
    let out = run(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [basins[0].0, wc.0], "system_type_id": "domestic_hot_water", "skip_incompatible": true })).unwrap();
    let skipped = out.warnings.iter().find(|w| w.id == "system.skipped").expect("the WC listed");
    assert_eq!(skipped.element_ids, vec![wc]);
    let dhw = ok(&r, &mut doc, "system.list", json!({}));
    let s = dhw.as_array().unwrap().iter().find(|s| s["classification"] == "domestic_hot_water").unwrap();
    assert_eq!(s["elements"], json!([basins[0].0]));
    // Without the option it is refused, as before.
    assert_eq!(run(&r, &mut doc, "system.create", json!({ "kind": "piping", "element_ids": [basins[1].0, wc.0], "system_type_id": "domestic_hot_water" })).unwrap_err().code, "system.incompatible");

    // IFC: sanitary terminals of their types, in the storey.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCSANITARYTERMINAL" && r.attributes.contains(".WASHHANDBASIN.")).count(), 6);

    // The wall deleted: the basins stay, unhosted, with a warning.
    let p = at(&doc, basins[2]);
    let out = run(&r, &mut doc, "element.delete", json!({ "element_ids": [wall.0] })).unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "mep.unhosted" && w.element_ids.contains(&basins[2])));
    assert_eq!(at(&doc, basins[2]), p);
}
