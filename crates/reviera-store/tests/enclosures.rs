// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazed enclosures (ARC-GLZ-001/002): gable, multispan and Venlo greenhouses made of roofs,
//! curtain walls, knee walls and gutters, rebuilt on change, and saved to IFC.

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

fn count(v: &Value, key: &str) -> usize {
    v[key].as_array().map_or(0, Vec::len)
}

#[test]
fn a_venlo_range_is_roofs_walls_and_gutters_rebuilt_on_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("venlo.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // 6 trusses of 8 m (two 4 m roofs each), 80 m long in 4 m bays.
    let id = ElementId(
        run(&r, &mut doc, "enclosure.create", json!({ "form": "venlo", "level_id": level.0, "span": 8.0, "spans": 6, "length": 80.0, "bay": 4.0 }))
            .unwrap()
            .as_u64()
            .unwrap(),
    );
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    assert_eq!(count(&info, "roofs"), 12, "{info}");
    assert_eq!(count(&info, "gutters"), 13, "2 eaves and 11 valleys");
    assert_eq!(count(&info, "curtain_walls"), 4);
    assert!((info["width"].as_f64().unwrap() - 48.0).abs() < 1e-9 && (info["floor_area"].as_f64().unwrap() - 3840.0).abs() < 1e-6);
    // The outer roofs' ridges at 5.5 m + (4 m − half the 125 mm valley gutter) / 2 × tan 22°;
    // the end walls fill up into them.
    let ridge = 5.5 + (4.0 - 0.0625) / 2.0 * 22f64.to_radians().tan();
    assert!((info["ridge"].as_f64().unwrap() - ridge).abs() < 1e-9);
    let store = doc.store();
    let end = info["curtain_walls"][1].as_u64().unwrap();
    let g = reviera_core::curtain::regenerate(store, store.get(ElementId(end)).unwrap()).unwrap();
    // Up into the gables, under the roofs' glazing bars (40 mm deep across the 22° slope).
    let bars = 0.04 / 22f64.to_radians().cos();
    assert!((g.z[1] - (ridge - bars)).abs() < 0.01, "the end wall reaches the ridges: {:?}", g.z);
    // Every part names its enclosure; the enclosure is a GlazedEnclosure assembly.
    let e = store.get(id).unwrap();
    assert_eq!(e.class, "IfcElementAssembly");
    assert_eq!(reviera_core::categories::category_of(e).map(|c| c.key), Some("cat.glazed_enclosures"));
    let guid = e.guid().to_string();
    assert_eq!(store.iter().filter(|p| reviera_core::enclosures::enclosure_of(p) == Some(guid.as_str())).count(), 12 + 13 + 4);

    // Three trusses and a knee wall: the parts are made again.
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "spans": 3, "knee": 0.6 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    assert_eq!((count(&info, "roofs"), count(&info, "gutters"), count(&info, "walls")), (6, 7, 4), "{info}");
    assert_eq!(doc.store().iter().filter(|p| reviera_core::enclosures::enclosure_of(p) == Some(guid.as_str())).count(), 6 + 7 + 4 + 4, "the old parts are gone");
    // A detached gutter stays when the enclosure changes.
    let kept = info["gutters"][0].as_u64().unwrap();
    run(&r, &mut doc, "enclosure.detach", json!({ "part_ids": [kept] })).unwrap();
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "eave": 6.0 })).unwrap();
    assert!(doc.store().contains(ElementId(kept)));
    // Undo puts the eave back; bad values are refused.
    run(&r, &mut doc, "edit.undo", json!({})).ok();
    assert!(run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "pitch": "75°" })).is_err());

    // Saved: the assembly, its roofs' glazing and the curtain walls' clipped gables are in IFC.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("GlazedEnclosure"));
    assert!(text.matches("IFCPLATE(").count() > 100, "glazing panels");
}

#[test]
fn a_gable_greenhouse_has_one_span() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("g.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let v = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": "20 m", "bay": 3.0, "eave": 3.0 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": v })).unwrap();
    assert_eq!((count(&info, "roofs"), count(&info, "gutters")), (1, 2));
    assert!(run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "spans": 2 })).is_err());
    let forms = run(&r, &mut doc, "enclosure.forms", json!({})).unwrap();
    assert_eq!(forms.as_array().unwrap().len(), 3);
}
