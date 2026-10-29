// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazing on vaults and domes (ARC-GLZ-006): panels on the curved surface, planar or split.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, roofs};
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

fn curves(pts: &[[f64; 2]]) -> Value {
    Value::Array((0..pts.len()).map(|i| json!({ "line": { "start": pts[i], "end": pts[(i + 1) % pts.len()] } })).collect())
}

#[test]
fn a_glazed_barrel_vault_and_dome_have_panels_on_their_surface() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cg.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 9 m span barrel vault, 18 m long, rising 3 m.
    let vault = run(&r, &mut doc, "roof.create_arched", json!({ "curves": curves(&[[0.0, 0.0], [18.0, 0.0], [18.0, 9.0], [0.0, 9.0]]), "level_id": level.0, "base_offset": 3.0, "rise": 3.0 })).unwrap();
    assert!(run(&r, &mut doc, "roof.set_glazing", json!({ "roof_id": vault, "grid_u": 1.5, "grid_v": 12 })).is_err(), "not glazed yet");
    let ty = run(&r, &mut doc, "roof.create_glazing_type", json!({ "name": "Vault Glazing" })).unwrap();
    run(&r, &mut doc, "element.set_type", json!({ "element_ids": [vault], "type_id": ty })).unwrap();
    // 1.5 m bays and 12 divisions: 12 × 12 flat panels.
    let info = run(&r, &mut doc, "roof.set_glazing", json!({ "roof_id": vault, "grid_u": "1.5 m", "grid_v": 12 })).unwrap();
    assert_eq!((info["panels"].as_u64(), info["triangles"].as_u64()), (Some(144), Some(0)), "{info}");
    assert!(info["max_warp"].as_f64().unwrap() < 1e-6, "a barrel's panels are planar: {info}");
    // Their corners lie on the arch: each panel's outer face is its thickness above the surface,
    // inset by half a bar; its corners sit within that of the vault's underside.
    let store = doc.store();
    let e = store.get(ElementId(vault.as_u64().unwrap())).unwrap();
    let g = roofs::regenerate(store, e).unwrap();
    let gl = roofs::glazing::of(store, e).unwrap();
    let panels: Vec<_> = gl.parts.iter().filter(|p| !p.mullion).collect();
    assert_eq!(panels.len(), 144);
    for p in &panels {
        for q in &p.mesh.0[..p.mesh.0.len() / 2] {
            let under = g.underside([q[0], q[1]]).unwrap();
            assert!((q[2] - under).abs() < 0.03, "a corner {q:?} off the arch ({under})");
        }
    }
    // Always triangulated: twice the panels.
    let info = run(&r, &mut doc, "roof.set_glazing", json!({ "roof_id": vault, "triangulate": "always" })).unwrap();
    assert_eq!(info["triangles"].as_u64(), Some(288), "{info}");
    // A dome of 5 m radius: 24 meridians, 6 rings, its crown ring of triangles.
    let dome = run(&r, &mut doc, "roof.create_dome", json!({ "level_id": level.0, "base_offset": 3.0, "center": [40.0, 0.0], "plan": { "circle": { "radius": 5.0 } } })).unwrap();
    run(&r, &mut doc, "element.set_type", json!({ "element_ids": [dome], "type_id": ty })).unwrap();
    let info = run(&r, &mut doc, "roof.set_glazing", json!({ "roof_id": dome, "grid_u": 24, "grid_v": 6 })).unwrap();
    assert_eq!((info["panels"].as_u64(), info["triangles"].as_u64()), (Some(144), Some(24)), "{info}");
    assert!(info["max_warp"].as_f64().unwrap() < 1e-6, "a dome's ring panels are planar: {info}");
    // Its body is the panels and bars; saved and read back with its grid.
    let body = reviera_core::bodies::all(doc.store()).into_iter().find(|b| b.element.0 == dome.as_u64().unwrap()).unwrap();
    assert!(body.parts.iter().any(|p| p.class == "IfcPlate:glass") && body.parts.iter().any(|p| p.class == "IfcMember:metal"));
    project.save(&doc).unwrap();
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().get(ElementId(dome.as_u64().unwrap())).and_then(roofs::roof_recipe).and_then(|r| r.curved_glazing).expect("the grid comes back");
    assert_eq!((back.u, back.v), (Some(24.0), Some(6.0)));
}
