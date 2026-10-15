// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model In-Place and model text (ARC-INPL-001, ARC-MTXT-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| m.0[t[k] as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

fn body_volume(doc: &Document, e: ElementId) -> f64 {
    reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == e)
        .flat_map(|b| b.parts)
        .map(|p| volume(&p.mesh))
        .sum()
}

#[test]
fn in_place_forms_and_model_text() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ip.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let square = json!([[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]);
    // A 2 × 1 × 3 m extrusion on the level: 6 m³.
    let ip = id(&run(
        &r,
        &mut doc,
        "inplace.create",
        json!({ "name": "Plinth", "category": "Casework", "forms": [
            { "kind": "extrusion", "start": 0.0, "end": 3.0, "profile": square, "level": l1.0 },
        ] }),
    )
    .unwrap());
    assert_eq!(doc.store().get(ip).unwrap().class, "IfcFurniture");
    assert!(
        (body_volume(&doc, ip) - 6.0).abs() < 1e-9,
        "{}",
        body_volume(&doc, ip)
    );
    // A revolve: a 1 × 1 square 1 m off an axis, all the way round: a ring of 2π·1.5 m³.
    run(
        &r,
        &mut doc,
        "inplace.add_form",
        json!({ "element_id": ip.0, "kind": "revolve", "axis": [[10.0, 0.0], [10.0, 1.0]], "start": 0.0, "end": std::f64::consts::TAU,
                "profile": [[11.0, 0.0], [12.0, 0.0], [12.0, 1.0], [11.0, 1.0]], "level": l1.0 }),
    )
    .unwrap();
    let ring = 2.0 * std::f64::consts::PI * 1.5;
    let v = body_volume(&doc, ip);
    // Faceted at 48 steps: within 0.3 %.
    assert!((v - 6.0 - ring).abs() < ring * 0.003, "{v}");
    // A blend from a 2 × 1 base to a 1 × 0.5 top: a frustum-like solid.
    run(
        &r,
        &mut doc,
        "inplace.add_form",
        json!({ "element_id": ip.0, "kind": "blend", "start": 0.0, "end": 1.0, "top": [[20.5, 0.25], [21.5, 0.25], [21.5, 0.75], [20.5, 0.75]],
                "profile": [[20.0, 0.0], [22.0, 0.0], [22.0, 1.0], [20.0, 1.0]], "level": l1.0 }),
    )
    .unwrap();
    let blend = body_volume(&doc, ip) - v;
    // Prismatoid: h/6 (A1 + 4M + A2) = 1/6 (2 + 4·1.125 + 0.5) = 7/6.
    assert!((blend - 7.0 / 6.0).abs() < 1e-6, "{blend}");
    // A void makes an opening element subtracted from it.
    run(
        &r,
        &mut doc,
        "inplace.add_form",
        json!({ "element_id": ip.0, "kind": "extrusion", "start": 1.0, "end": 2.0, "void": true,
                "profile": [[0.5, 0.0], [1.5, 0.0], [1.5, 1.0], [0.5, 1.0]], "level": l1.0 }),
    )
    .unwrap();
    let rel = reviera_core::csg::resolved(doc.store());
    assert_eq!(rel.len(), 1);
    assert_eq!(rel[0].0, ip);
    // Copies are independent; moving carries the forms.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [ip.0], "vector": [0.0, 5.0, 0.0] }),
    )
    .unwrap();
    let b = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == ip)
        .unwrap()
        .bounds()
        .unwrap();
    assert!((b.0[1] - 5.0).abs() < 1e-6, "{b:?}");

    // Model text "ABC" 50 mm deep: three solids.
    let t = id(&run(
        &r,
        &mut doc,
        "modeltext.create",
        json!({ "text": "ABC", "point": [0.0, -3.0], "size": 0.5, "depth": 0.05, "level": l1.0 }),
    )
    .unwrap());
    let parts = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == t)
        .unwrap()
        .parts;
    assert_eq!(parts.len(), 3);
    run(
        &r,
        &mut doc,
        "modeltext.set_text",
        json!({ "element_id": t.0, "text": "AB" }),
    )
    .unwrap();
    let parts = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == t)
        .unwrap()
        .parts;
    assert_eq!(parts.len(), 2);
    project.save(&doc).unwrap();
    // Deleting the in-place element takes its void.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [ip.0] }),
    )
    .unwrap();
    assert!(
        doc.store()
            .iter()
            .all(|e| reviera_core::in_place::void_recipe(e).is_none())
    );
}
