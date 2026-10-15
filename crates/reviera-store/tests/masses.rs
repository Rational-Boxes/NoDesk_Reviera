// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Masses, mass floors and model by face (ARC-MASS-001 … 003, ARC-ROOF-003, ARC-CW-007).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, walls};
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

#[test]
fn a_box_mass_floors_faces_and_update() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut project, mut doc) =
        LocalProject::new(Schema::Ifc4, &tmp.path().join("m.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l0 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    for z in [3.0, 6.0, 9.0, 12.0] {
        run(&r, &mut doc, "level.create", json!({ "elevation": z })).unwrap();
    }
    let m = id(&run(
        &r,
        &mut doc,
        "mass.create",
        json!({ "name": "Block", "forms": [{ "kind": "extrusion", "start": 0.0, "end": 12.0, "level": l0.0,
            "profile": [[0.0, 0.0], [20.0, 0.0], [20.0, 30.0], [0.0, 30.0]] }] }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "mass.set_floors",
        json!({ "mass_id": m.0, "all": true }),
    )
    .unwrap();
    let floors = run(&r, &mut doc, "mass.floors", json!({ "mass_id": m.0 })).unwrap();
    let floors = floors.as_array().unwrap();
    // Four floors (none at the top face), 600 m² and 1800 m³ each.
    assert_eq!(floors.len(), 4);
    for f in floors {
        assert!((f["area"].as_f64().unwrap() - 600.0).abs() < 1e-6, "{f}");
        assert!((f["volume"].as_f64().unwrap() - 1800.0).abs() < 1e-6, "{f}");
    }
    // The top floor's exterior: 300 m² of sides and the 600 m² roof.
    assert!(
        (floors[3]["exterior_area"].as_f64().unwrap() - 900.0).abs() < 1e-6,
        "{}",
        floors[3]
    );
    assert!((floors[0]["exterior_area"].as_f64().unwrap() - 300.0).abs() < 1e-6);
    // A level 3 m above the mass makes nothing until the mass reaches past it.
    let l15 = id(&run(&r, &mut doc, "level.create", json!({ "elevation": 15.0 })).unwrap());
    let all: Vec<u64> = reviera_core::levels::list(doc.store())
        .iter()
        .map(|l| l.id.0)
        .collect();
    run(
        &r,
        &mut doc,
        "mass.set_floors",
        json!({ "mass_id": m.0, "level_ids": all }),
    )
    .unwrap();
    assert_eq!(
        run(&r, &mut doc, "mass.floors", json!({ "mass_id": m.0 }))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let _ = l15;
    // Walls, a roof and a curtain system by face.
    let faces = run(&r, &mut doc, "mass.faces", json!({ "mass_id": m.0 })).unwrap();
    assert_eq!(faces.as_array().unwrap().len(), 6);
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create_by_face",
        json!({ "mass_id": m.0, "faces": ["form0.side0"] }),
    )
    .unwrap()[0]);
    let g = walls::regenerate(doc.store(), doc.store().get(wall).unwrap()).unwrap();
    assert!((g.height - 12.0).abs() < 1e-9 && (g.length() - 20.0).abs() < 1e-9);
    // On the face: the exterior finish face at y = 0, the wall inside it.
    assert!((g.start[1] - 0.1).abs() < 1e-9, "{g:?}");
    let roof = id(&run(
        &r,
        &mut doc,
        "roof.create_by_face",
        json!({ "mass_id": m.0, "faces": ["form0.top"], "thickness": 0.3 }),
    )
    .unwrap()[0]);
    let cs = id(&run(
        &r,
        &mut doc,
        "curtainsystem.create_by_face",
        json!({ "mass_id": m.0, "faces": ["form0.side1", "form0.side2"] }),
    )
    .unwrap());
    // The two faces share their corner edge: one mullion on it, not two.
    let parts = reviera_core::masses::system_parts(
        &reviera_core::masses::system_recipe(doc.store().get(cs).unwrap()).unwrap(),
    )
    .parts;
    let corner = parts
        .iter()
        .filter_map(|p| p.axis)
        .filter(|[a, b]| {
            (a[0] - 20.0).abs() < 1e-6
                && (a[1] - 30.0).abs() < 1e-6
                && (b[0] - 20.0).abs() < 1e-6
                && (b[1] - 30.0).abs() < 1e-6
        })
        .count();
    assert_eq!(corner, 1);
    // Stretched, the mass floors follow at once; the walls only with Update to Face.
    run(
        &r,
        &mut doc,
        "mass.set_form",
        json!({ "mass_id": m.0, "index": 0, "end": 15.0 }),
    )
    .unwrap();
    assert_eq!(
        run(&r, &mut doc, "mass.floors", json!({ "mass_id": m.0 }))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(
        (walls::regenerate(doc.store(), doc.store().get(wall).unwrap())
            .unwrap()
            .height
            - 12.0)
            .abs()
            < 1e-9
    );
    run(
        &r,
        &mut doc,
        "element.update_to_face",
        json!({ "element_ids": [wall.0, roof.0, cs.0] }),
    )
    .unwrap();
    assert!(
        (walls::regenerate(doc.store(), doc.store().get(wall).unwrap())
            .unwrap()
            .height
            - 15.0)
            .abs()
            < 1e-9
    );
    let rb = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == roof)
        .unwrap()
        .bounds()
        .unwrap();
    assert!((rb.0[2] - 15.0).abs() < 1e-6, "{rb:?}");
    let deps = run(
        &r,
        &mut doc,
        "mass.select_dependents",
        json!({ "mass_id": m.0 }),
    )
    .unwrap();
    assert_eq!(deps.as_array().unwrap().len(), 3);
    // Floors on the mass floors.
    let fl = run(
        &r,
        &mut doc,
        "floor.create_by_mass_floor",
        json!({ "mass_id": m.0 }),
    )
    .unwrap();
    assert_eq!(fl.as_array().unwrap().len(), 5);
    project.save(&doc).unwrap();
}

#[test]
fn a_lofted_mass() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("l.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l0 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let sq = |c: f64, h: f64| {
        json!([
            [c - h, c - h],
            [c + h, c - h],
            [c + h, c + h],
            [c - h, c + h]
        ])
    };
    let m = id(&run(
        &r,
        &mut doc,
        "mass.create",
        json!({ "name": "Tower", "forms": [{ "kind": "loft", "level": l0.0, "profile": sq(5.0, 5.0), "profiles": [
            { "offset": 0.0, "points": sq(5.0, 5.0) },
            { "offset": 5.0, "points": sq(5.0, 4.0) },
            { "offset": 10.0, "points": sq(5.0, 3.0) },
        ] }] }),
    )
    .unwrap());
    let v: f64 = reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == m)
        .flat_map(|b| b.parts)
        .map(|p| volume(&p.mesh))
        .sum();
    // Two frusta: 5/3 (100 + 64 + 80) + 5/3 (64 + 36 + 48).
    let want = 5.0 / 3.0 * (100.0 + 64.0 + 80.0) + 5.0 / 3.0 * (64.0 + 36.0 + 48.0);
    assert!((v - want).abs() < 1e-6, "{v} vs {want}");
    // The floors slice it just above the base: within 0.01 %.
    run(
        &r,
        &mut doc,
        "mass.set_floors",
        json!({ "mass_id": m.0, "all": true }),
    )
    .unwrap();
    let f = run(&r, &mut doc, "mass.floors", json!({ "mass_id": m.0 })).unwrap();
    assert!(
        (f[0]["volume"].as_f64().unwrap() - want).abs() < want * 1e-4,
        "{f}"
    );
}
