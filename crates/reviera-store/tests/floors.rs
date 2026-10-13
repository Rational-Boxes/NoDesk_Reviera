// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Floors: sketched boundaries, holes, picked and locked walls, types, editing and IFC output
//! (spec Ch 3 §3.1.3–3.1.4, §3.6.1; ARC-SKCH-001/002, ARC-FLR-001).

use std::path::{Path, PathBuf};

use reviera_core::{Caller, CommandRegistry, Document, ElementId, floors};
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
    ElementId(v.as_u64().unwrap())
}

fn project(dir: &Path) -> (LocalProject, Document, PathBuf, ElementId) {
    let path = dir.join("floors.ifc");
    let (p, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (p, doc, path, level)
}

/// Line curves of a closed polygon.
fn ring(points: &[[f64; 2]]) -> Vec<Value> {
    (0..points.len())
        .map(|i| json!({ "line": { "start": points[i], "end": points[(i + 1) % points.len()] } }))
        .collect()
}

fn geometry(doc: &Document, f: ElementId) -> floors::FloorGeometry {
    floors::regenerate(doc.store(), doc.store().get(f).unwrap()).unwrap()
}

#[test]
fn a_floor_with_a_hole_has_its_area_volume_and_level() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    let outer = ring(&[[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]]);
    let hole = ring(&[[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]]);
    let f = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [outer, hole], "level_id": level.0, "offset": "50 mm" }),
    )
    .unwrap());
    let g = geometry(&doc, f);
    assert_eq!(g.profiles.len(), 1);
    assert_eq!(g.profiles[0].holes.len(), 1);
    assert!((g.area() - 76.0).abs() < 1e-9);
    assert!((g.top - 0.05).abs() < 1e-9 && (g.thickness - 0.3).abs() < 1e-9);
    assert!((g.volume() - 76.0 * 0.3).abs() < 1e-9);
    assert!((g.perimeter() - 44.0).abs() < 1e-9);

    // The type's thickness changes every floor of the type.
    let t = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcSlabType")
        .unwrap()
        .id();
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": t.0, "param": "thickness", "value": "200 mm" }),
    )
    .unwrap();
    assert!((geometry(&doc, f).thickness - 0.2).abs() < 1e-9);

    // An open boundary is refused with the open ends.
    let e = run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[
            { "line": { "start": [0, 0], "end": [5, 0] } },
            { "line": { "start": [5, 0], "end": [5, 5] } },
            { "line": { "start": [5, 5], "end": [0, 5] } },
        ]], "level_id": level.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "sketch.open");
    assert_eq!(e.details["points"].as_array().unwrap().len(), 2);

    // Moving and copying move the boundary; deleting the level deletes the floor.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [f.0], "vector": [1, 2, 0] }),
    )
    .unwrap();
    assert!(geometry(&doc, f).profiles[0].outer.contains(&[1.0, 2.0]));
    let c: Vec<Vec<ElementId>> = serde_json::from_value(
        run(
            &r,
            &mut doc,
            "element.copy",
            json!({ "element_ids": [f.0], "vectors": [[20, 0, 0]] }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!((geometry(&doc, c[0][0]).area() - 76.0).abs() < 1e-9);
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [level.0] }),
    )
    .unwrap();
    assert!(
        deleted
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_u64() == Some(f.0))
    );
}

#[test]
fn a_floor_picked_from_walls_follows_them() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    // A 6 × 4 room of 200 mm walls, drawn counter-clockwise (exterior on the left = inside).
    let corners = [[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]];
    let walls: Vec<ElementId> = (0..4)
        .map(|i| {
            id(run(
                &r,
                &mut doc,
                "wall.create",
                json!({
                    "curve": { "line": { "start": corners[i], "end": corners[(i + 1) % 4] } },
                    "base_level": level.0,
                    "top_constraint": { "unconnected": 3 },
                }),
            )
            .unwrap())
        })
        .collect();
    // Pick Walls: each wall's inside face (its exterior face here), locked. The picked faces
    // run past or stop short of the corners; the floor closes them.
    let faces = [
        ([0.0, 0.1], [6.0, 0.1]),
        ([5.9, 0.0], [5.9, 4.0]),
        ([6.0, 3.9], [0.0, 3.9]),
        ([0.1, 4.0], [0.1, 0.0]),
    ];
    let curves: Vec<Value> = faces
        .iter()
        .zip(&walls)
        .map(|((a, b), w)| json!({ "line": { "start": a, "end": b }, "lock": { "element": w.0, "sub": "face:ext" } }))
        .collect();
    let f = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [curves], "level_id": level.0 }),
    )
    .unwrap());
    assert!((geometry(&doc, f).area() - 5.8 * 3.8).abs() < 1e-9);

    // Moving the right-hand wall 500 mm out stretches the floor (ARC-SKCH-001).
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [walls[1].0], "vector": [0.5, 0, 0] }),
    )
    .unwrap();
    assert!(
        (geometry(&doc, f).area() - 6.3 * 3.8).abs() < 1e-9,
        "{}",
        geometry(&doc, f).area()
    );

    // Edit Boundary replaces the sketch.
    let square = ring(&[[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]]);
    run(
        &r,
        &mut doc,
        "floor.edit_boundary",
        json!({ "floor_id": f.0, "loops": [square] }),
    )
    .unwrap();
    assert!((geometry(&doc, f).area() - 4.0).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "floor.set_level",
        json!({ "floor_ids": [f.0], "offset": "-100 mm" }),
    )
    .unwrap();
    assert!((geometry(&doc, f).top + 0.1).abs() < 1e-9);
}

/// Volume of a closed triangle mesh.
fn volume(verts: &[f64], faces: &[u32]) -> f64 {
    let p = |k: u32| {
        [
            verts[3 * k as usize],
            verts[3 * k as usize + 1],
            verts[3 * k as usize + 2],
        ]
    };
    faces
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [p(t[0]), p(t[1]), p(t[2])];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn saved_floors_are_slabs_with_a_profile_and_holes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    let outer = ring(&[[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]]);
    let hole = ring(&[[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]]);
    let f = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [outer, hole], "level_id": level.0 }),
    )
    .unwrap());
    project.save(&doc).unwrap();

    let ifc = IfcModel::open(&path).unwrap();
    let slab = ifc.entities_of_type("IfcSlab").unwrap()[0].id;
    assert_eq!(
        ifc.entities_of_type("IfcArbitraryProfileDefWithVoids")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ifc.referenced_by(slab, "IfcRelContainedInSpatialStructure")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ifc.referenced_by(slab, "IfcRelDefinesByType")
            .unwrap()
            .len(),
        1
    );
    // The geometry engine sees 76 m² × 300 mm, hanging below the level.
    let meshes = ifc.tessellate(1).unwrap();
    let m = meshes.iter().find(|m| m.class == "IfcSlab").unwrap();
    assert!((volume(&m.verts, &m.faces).abs() - 76.0 * 0.3).abs() < 1e-6);
    let zs: Vec<f64> = m.verts.chunks(3).map(|c| c[2]).collect();
    let (lo, hi) = zs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(*z), b.max(*z)));
    assert!((hi - 0.0).abs() < 1e-9 && (lo + 0.3).abs() < 1e-9);

    // Saving again writes nothing new; reopened, the floor is the same.
    let count = ifc.entity_count();
    project.save(&doc).unwrap();
    assert_eq!(IfcModel::open(&path).unwrap().entity_count(), count);
    let (mut project, mut reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(geometry(&reopened, f), geometry(&doc, f));

    // Deleting the floor removes its body from the file.
    run(
        &r,
        &mut reopened,
        "element.delete",
        json!({ "element_ids": [f.0] }),
    )
    .unwrap();
    project.save(&reopened).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcSlab").unwrap().is_empty());
    assert!(
        ifc.entities_of_type("IfcArbitraryProfileDefWithVoids")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn automatic_ceilings_fill_the_room_and_follow_its_walls() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    // Two rooms of a 10 × 7 house, split at x = 5 (200 mm walls on their centrelines).
    let mut walls = Vec::new();
    for (a, b) in [
        ([0.0, 0.0], [10.0, 0.0]),
        ([10.0, 0.0], [10.0, 7.0]),
        ([10.0, 7.0], [0.0, 7.0]),
        ([0.0, 7.0], [0.0, 0.0]),
        ([5.0, 0.0], [5.0, 7.0]),
    ] {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3 } }),
        )
        .unwrap()));
    }
    // A click in the left room: its edges are the walls' finish faces (ARC-CEIL-001).
    let c = id(run(
        &r,
        &mut doc,
        "ceiling.create_auto",
        json!({ "level_id": level.0, "point": [2, 3] }),
    )
    .unwrap());
    let g = geometry(&doc, c);
    assert!((g.area() - 4.8 * 6.8).abs() < 1e-6, "{}", g.area());
    // 25 mm thick, bottom at 2600 mm.
    assert!((g.top - g.thickness - 2.6).abs() < 1e-9 && (g.thickness - 0.025).abs() < 1e-9);
    assert_eq!(doc.store().get(c).unwrap().class, "IfcCovering");
    // Moving the partition 1 m to the right widens the room's ceiling.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [walls[4].0], "vector": [1, 0, 0] }),
    )
    .unwrap();
    assert!(
        (geometry(&doc, c).area() - 5.8 * 6.8).abs() < 1e-6,
        "{}",
        geometry(&doc, c).area()
    );
    // Outside every room there is no loop.
    let e = run(
        &r,
        &mut doc,
        "ceiling.create_auto",
        json!({ "level_id": level.0, "point": [20, 3] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "ceiling.no_loop");
    // A ceiling type is not a floor type.
    let ft = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcCoveringType")
        .unwrap()
        .id();
    run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [ring(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])], "level_id": level.0 }),
    )
    .unwrap();
    let floor = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcSlab")
        .unwrap()
        .id();
    let e = run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [floor.0], "type_id": ft.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "type.wrong_family");

    // Saved as IfcCovering(CEILING) with a body, in the level.
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let cov = ifc.entities_of_type("IfcCovering").unwrap()[0].id;
    assert_eq!(
        ifc.referenced_by(cov, "IfcRelContainedInSpatialStructure")
            .unwrap()
            .len(),
        1
    );
    let meshes = ifc.tessellate(1).unwrap();
    let m = meshes.iter().find(|m| m.class == "IfcCovering").unwrap();
    assert!((volume(&m.verts, &m.faces).abs() - 5.8 * 6.8 * 0.025).abs() < 1e-6);

    // Ceiling plans look up from 2300 mm.
    let v = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "kind": "ceiling_plan", "level_ids": [level.0] }),
    )
    .unwrap();
    let range = run(
        &r,
        &mut doc,
        "view.get_view_range",
        json!({ "view_id": v[0] }),
    )
    .unwrap();
    assert_eq!(range["resolved_elevations"]["cut"], json!(2.3));
    assert_eq!(range["resolved_elevations"]["top"], json!(4.0));
}
