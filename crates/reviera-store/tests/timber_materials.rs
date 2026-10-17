// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Timber sections and the materials library (STR-SEC-001): sawn lumber and EN 336 timber as
//! column and beam types in the region's timber grade, the timber slenderness guidance, and the
//! materials listed by region.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn timber_sections_and_materials() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tm.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "irc" }));
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A 4x4 post: 3-1/2" square dressed, Douglas Fir-Larch No.2 in a US model.
    let post = id(run(
        &r,
        &mut doc,
        "column.load_section",
        json!({ "catalog": "na_lumber", "designation": "4x4" }),
    ));
    let t = reviera_core::columns::column_type(doc.store().get(post).unwrap()).unwrap();
    assert!(near(t.width, 3.5 * 0.0254) && near(t.depth, 3.5 * 0.0254));
    assert_eq!(t.material.as_deref(), Some("Wood Douglas Fir-Larch No.2"));
    assert_eq!(
        t.material_kind,
        Some(reviera_core::columns::MaterialKind::Wood)
    );
    // A 2x10 joist type: 1-1/2" × 9-1/4".
    let joist = id(run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "na_lumber", "designation": "2x10" }),
    ));
    let t = reviera_core::beams::beam_type(doc.store().get(joist).unwrap()).unwrap();
    assert!(near(t.width, 1.5 * 0.0254) && near(t.depth, 9.25 * 0.0254));

    // A 4x4 post 4.5 m tall: le/d = 4.5 / 0.0889 ≈ 50.6, over the NDS limit of 50; 3 m is not.
    let place = |doc: &mut Document, x: f64, h: f64| {
        id(run(
            &r,
            doc,
            "column.place",
            json!({ "type_id": post.0, "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": h } }),
        ))
    };
    let tall = place(&mut doc, 0.0, 4.5);
    let short = place(&mut doc, 2.0, 3.0);
    let adv = run(
        &r,
        &mut doc,
        "column.check",
        json!({ "column_ids": [tall.0, short.0] }),
    );
    assert_eq!(adv[0]["limit"], 50.0);
    assert!(adv[0]["note"].as_str().unwrap().contains("NDS"));
    assert!(adv[1]["limit"].is_null());

    // The library: US materials first; S355's yield; the region's defaults are in it.
    let all = run(&r, &mut doc, "structural.materials", json!({}));
    let list = all.as_array().unwrap();
    assert!(list.len() >= 30);
    assert_eq!(list[0]["preferred"], true);
    let wood = run(
        &r,
        &mut doc,
        "structural.materials",
        json!({ "kind": "wood" }),
    );
    assert!(wood.as_array().unwrap().iter().all(|m| m["kind"] == "wood"));
    let s355 = list
        .iter()
        .find(|m| m["name"] == "Steel S355 (EN 10025-2)")
        .unwrap();
    assert_eq!(s355["properties"]["fy_MPa"], 355.0);
    assert!(!s355["source_ids"].as_array().unwrap().is_empty());

    // A German model: EN 336 timber in C24.
    run(&r, &mut doc, "code.set", json!({ "code": "din_18065" }));
    let s = run(&r, &mut doc, "structural.standards", json!({}));
    assert_eq!(s["timber"], "Timber C24 (EN 338)");
    let beam = id(run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "eu_timber", "designation": "75x225" }),
    ));
    let t = reviera_core::beams::beam_type(doc.store().get(beam).unwrap()).unwrap();
    assert!(near(t.width, 0.075) && near(t.depth, 0.225));
    assert_eq!(t.material.as_deref(), Some("Timber C24 (EN 338)"));
    let cats = run(&r, &mut doc, "structural.catalogs", json!({}));
    assert!(
        cats.as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "eu_timber" && c["preferred"] == true)
    );
}
