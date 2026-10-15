// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Skylights: a roof window in a pitched roof and a ridge lantern on a flat one.

use reviera_core::skylights::{self, SkylightStyle};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        { "line": { "start": [x0, y0], "end": [x1, y0] } },
        { "line": { "start": [x1, y0], "end": [x1, y1] } },
        { "line": { "start": [x1, y1], "end": [x0, y1] } },
        { "line": { "start": [x0, y1], "end": [x0, y0] } },
    ])
}

fn area(doc: &Document, roof: ElementId) -> f64 {
    reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap())
        .unwrap()
        .projected_area()
}

#[test]
fn roof_windows_and_lanterns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A hip roof: a roof window on its south slope.
    let hip = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(0.0, 0.0, 10.0, 8.0), "level_id": l1.0, "base_offset": 3.0 }),
    )
    .unwrap());
    let before = area(&doc, hip);
    let window = id(run(
        &r,
        &mut doc,
        "skylight.create",
        json!({ "roof_id": hip.0, "point": [5.0, 1.8] }),
    )
    .unwrap());
    let s = skylights::skylight_recipe(doc.store().get(window).unwrap()).unwrap();
    assert_eq!(s.style, SkylightStyle::RoofWindow);
    // Its hole is its rectangle seen from above: narrower than its length up the slope.
    let cut = before - area(&doc, hip);
    assert!(cut > 0.5 && cut < 0.78 * 1.18, "{cut}");
    // A lantern does not go in a pitched roof.
    let e = run(
        &r,
        &mut doc,
        "skylight.create",
        json!({ "roof_id": hip.0, "point": [5.0, 6.5], "style": "ridge_lantern" }),
    )
    .unwrap_err();
    assert_eq!(e.code, "skylight.wrong_roof");

    // A flat roof: a ridge lantern, 3 m long, panes at most 0.9 m: 4 of them.
    let flat = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(20.0, 0.0, 30.0, 8.0), "level_id": l1.0, "base_offset": 3.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": flat.0, "shape": "flat" }),
    )
    .unwrap();
    let flat_before = area(&doc, flat);
    let lantern = id(run(
        &r,
        &mut doc,
        "skylight.create",
        json!({ "roof_id": flat.0, "point": [25.0, 4.0] }),
    )
    .unwrap());
    let g = skylights::regenerate(doc.store(), doc.store().get(lantern).unwrap()).unwrap();
    assert_eq!(g.panes, 4);
    assert!((flat_before - area(&doc, flat) - 1.5 * 3.0).abs() < 1e-6);
    // Lengthened, its panes are re-mapped.
    run(
        &r,
        &mut doc,
        "skylight.set",
        json!({ "skylight_ids": [lantern.0], "length": "5 m" }),
    )
    .unwrap();
    let g = skylights::regenerate(doc.store(), doc.store().get(lantern).unwrap()).unwrap();
    assert_eq!(g.panes, 6);
    // It moves with its roof.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [flat.0], "vector": [0.0, 10.0, 0.0] }),
    )
    .unwrap();
    let s = skylights::skylight_recipe(doc.store().get(lantern).unwrap()).unwrap();
    assert_eq!(s.center, [25.0, 14.0]);

    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches(".SKYLIGHT.").count(),
        2,
        "two IfcWindows as skylights"
    );
    // Deleting the roof deletes its skylight.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [flat.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert!(doc.store().get(lantern).is_none());
    let _ = IfcModel::open(&path).unwrap();
}
