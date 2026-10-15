// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ceilings that follow a roof's underside: vaulted under a hip roof, domed under a dome.

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

/// The top points of an element's body (the highest of each plan point).
fn tops(doc: &Document, e: ElementId) -> Vec<[f64; 3]> {
    let mut best: std::collections::HashMap<[i64; 2], [f64; 3]> = Default::default();
    for b in reviera_core::bodies::all(doc.store()) {
        if b.element != e {
            continue;
        }
        for p in b.parts.iter().flat_map(|p| p.mesh.0.iter()) {
            let k = [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64];
            let v = best.entry(k).or_insert(*p);
            if p[2] > v[2] {
                *v = *p;
            }
        }
    }
    best.into_values().collect()
}

#[test]
fn ceilings_follow_a_hip_roof_and_a_dome() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // An 8 × 6 m room, 3 m walls, under a hip roof on the walls' outer faces.
    let c = [[0.0, 0.0], [8.0, 0.0], [8.0, 6.0], [0.0, 6.0]];
    for i in 0..4 {
        run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": c[(i + 1) % 4], "end": c[i] } },
                "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
            }),
        )
        .unwrap();
    }
    let o = 0.1;
    let f = [[-o, -o], [8.0 + o, -o], [8.0 + o, 6.0 + o], [-o, 6.0 + o]];
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": f[i], "end": f[(i + 1) % 4] } }))
        .collect();
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": l1.0, "base_offset": 3.0 }),
    )
    .unwrap());
    let ceiling = id(run(
        &r,
        &mut doc,
        "ceiling.create_auto",
        json!({ "level_id": l1.0, "point": [4.0, 3.0], "follow_roof": true }),
    )
    .unwrap());
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    // Every top point of the ceiling is on the roof's underside, and the middle is well above
    // the flat ceiling's 2.6 m.
    let pts = tops(&doc, ceiling);
    // Four pieces, one under each hip face: the room's corners and the ridge's ends.
    assert_eq!(
        reviera_core::vaults::meshes(doc.store(), doc.store().get(ceiling).unwrap())
            .unwrap()
            .len(),
        4
    );
    assert_eq!(pts.len(), 6);
    let mut high = f64::MIN;
    for p in &pts {
        let u = g.underside([p[0], p[1]]).unwrap();
        assert!((p[2] - u).abs() < 1e-6, "{p:?} vs {u}");
        high = high.max(p[2]);
    }
    assert!(high > 4.0, "{high}");
    // Back to flat.
    run(
        &r,
        &mut doc,
        "ceiling.set_shape",
        json!({ "ceiling_ids": [ceiling.0], "shape": "flat" }),
    )
    .unwrap();
    assert!(
        tops(&doc, ceiling)
            .iter()
            .all(|p| (p[2] - 2.62).abs() < 0.03)
    );
    run(
        &r,
        &mut doc,
        "ceiling.set_shape",
        json!({ "ceiling_ids": [ceiling.0], "shape": "follow_roof" }),
    )
    .unwrap();

    // A round room under a dome: the ceiling is a dome inside it.
    let tower = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[
            { "line": { "start": [20.0, 0.0], "end": [24.0, 0.0] }, "through": [[22.0, 2.0]] },
            { "line": { "start": [24.0, 0.0], "end": [20.0, 0.0] }, "through": [[22.0, -2.0]] },
        ]], "level_id": l1.0 }),
    )
    .unwrap());
    let _ = tower;
    let dome = id(run(
        &r,
        &mut doc,
        "roof.create_dome",
        json!({ "level_id": l1.0, "base_offset": 3.0, "center": [22.0, 0.0], "plan": { "circle": { "radius": 2.2 } } }),
    )
    .unwrap());
    let dc = id(run(
        &r,
        &mut doc,
        "ceiling.create",
        json!({ "loops": [[
            { "line": { "start": [20.2, 0.0], "end": [23.8, 0.0] }, "through": [[22.0, 1.8]] },
            { "line": { "start": [23.8, 0.0], "end": [20.2, 0.0] }, "through": [[22.0, -1.8]] },
        ]], "level_id": l1.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "ceiling.set_shape",
        json!({ "ceiling_ids": [dc.0], "shape": "follow_roof", "roof_id": dome.0 }),
    )
    .unwrap();
    let dg = reviera_core::roofs::regenerate(doc.store(), doc.store().get(dome).unwrap()).unwrap();
    let pts = tops(&doc, dc);
    let peak = pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!(peak > 4.5, "{peak}");
    for p in &pts {
        if let Some(u) = dg.underside([p[0], p[1]]) {
            assert!((p[2] - u).abs() < 1e-6, "{p:?} vs {u}");
        }
    }

    // IFC: faceted bodies, and back as following ceilings.
    project.save(&doc).unwrap();
    drop(project);
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCFACETEDBREP"));
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    let e = doc2.store().get(ceiling).unwrap();
    assert!(reviera_core::vaults::follow_roof(e).is_some());
}
