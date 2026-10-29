// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Geodesic domes (ARC-GLZ-006): made, reported for fabrication, changed, and saved to IFC.

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

#[test]
fn a_3v_dome_reports_its_struts_and_comes_back_from_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dome.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 3V dome of 5 m radius at 5/8.
    let id = run(&r, &mut doc, "roof.create_geodesic", json!({ "level_id": level.0, "center": [10.0, 10.0], "radius": "5 m", "frequency": 3, "truncation": "5/8" })).unwrap();
    let info = run(&r, &mut doc, "roof.geodesic", json!({ "roof_id": id })).unwrap();
    let classes = info["strut_classes"].as_array().unwrap();
    assert_eq!(classes.len(), 3, "{info}");
    // The published 3V 5/8 dome: 61 hubs, 105 panels, 165 struts of 30 A, 55 B and 80 C at
    // 0.34862, 0.40355 and 0.41241 × the radius; its base steps between rings.
    assert_eq!(classes.iter().map(|c| c["count"].as_u64().unwrap()).collect::<Vec<_>>(), vec![30, 55, 80]);
    for (c, k) in classes.iter().zip([0.34862, 0.40355, 0.41241]) {
        assert!((c["length"].as_f64().unwrap() - 5.0 * k).abs() < 2e-4, "{c}");
    }
    assert_eq!((info["hubs"].as_u64(), info["panels"].as_u64(), info["level_base"].as_bool()), (Some(61), Some(105), Some(false)));
    let counted: u64 = classes.iter().map(|c| c["count"].as_u64().unwrap()).sum();
    assert_eq!(counted, info["struts"].as_u64().unwrap());
    // Euler: hubs − struts + panels = 1 for a dome open at its base.
    let (v, e, f) = (info["hubs"].as_i64().unwrap(), info["struts"].as_i64().unwrap(), info["panels"].as_i64().unwrap());
    assert_eq!(v - e + f, 1, "{info}");
    let height = info["height"].as_f64().unwrap();
    assert!(height > 5.0 && height < 7.5, "{info}");
    // Its body: a panel per triangle and a strut per edge, glass and metal.
    let store = doc.store();
    let body = reviera_core::bodies::all(store).into_iter().find(|b| b.element == ElementId(id.as_u64().unwrap())).expect("a body");
    assert_eq!(body.parts.len() as i64, f + e);
    let e = store.get(ElementId(id.as_u64().unwrap())).unwrap();
    assert_eq!((e.class.as_str(), e.param("@PredefinedType").and_then(Value::as_str)), ("IfcRoof", Some("DOME_ROOF")));
    assert!(reviera_core::glazing_library::of_type(e).is_some() && reviera_core::frame_library::of_type(e).is_some());
    // A 2V hemisphere instead: level on its base.
    run(&r, &mut doc, "roof.set_geodesic", json!({ "roof_id": id, "frequency": 2, "truncation": 0.5, "glazing": "Twin-Wall Polycarbonate 10 mm" })).unwrap();
    let info = run(&r, &mut doc, "roof.geodesic", json!({ "roof_id": id })).unwrap();
    assert_eq!((info["hubs"].as_u64(), info["panels"].as_u64(), info["struts"].as_u64(), info["level_base"].as_bool()), (Some(26), Some(40), Some(65), Some(true)), "{info}");
    assert!(run(&r, &mut doc, "roof.set_geodesic", json!({ "roof_id": id, "frequency": 40 })).is_err());
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("DOME_ROOF") && text.contains("IFCFACETEDBREP"));
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| reviera_core::geodesic::recipe(e).is_some()).expect("the dome comes back");
    let rec = reviera_core::geodesic::recipe(back).unwrap();
    assert_eq!((rec.frequency, rec.glazing.as_deref()), (2, Some("Twin-Wall Polycarbonate 10 mm")));
    assert!((reviera_core::glazing_library::of_type(back).unwrap().u - 3.2).abs() < 1e-9);
}
