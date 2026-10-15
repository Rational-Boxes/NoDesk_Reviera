// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roofs by extrusion (ARC-ROOF-002) and openings by face (ARC-OPEN-001): a gable profile on a
//! grid's plane follows the grid and its level; an arc profile is a barrel roof of uniform
//! thickness; an opening by face in a 30° roof leans with it, and is saved as a prism along the
//! roof's normal.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, roofs};
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn geometry(doc: &Document, roof: ElementId) -> roofs::RoofGeometry {
    roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap()
}

fn max_z(g: &roofs::RoofGeometry) -> f64 {
    g.faces
        .iter()
        .flat_map(|f| f.slab.0.iter().map(|p| p[2]))
        .fold(f64::NEG_INFINITY, f64::max)
}

#[test]
fn extruded_roofs_and_openings_by_face() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("roofs.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(&run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());

    // A gable profile on grid A (along x from (0, 0) to (8, 0)): eaves at 0, ridge 2 m up at
    // the middle, on Level 2; extruded 0 to 6 m across.
    let grid = id(&run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] } } }),
    )
    .unwrap());
    let line = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
    let gable = id(&run(
        &r,
        &mut doc,
        "roof.create_extrusion",
        json!({ "grid": grid.0, "profile": [line([0.0, 0.0], [4.0, 2.0]), line([4.0, 2.0], [8.0, 0.0])], "ref_level": l2.0, "start": 0.0, "end": 6.0 }),
    )
    .unwrap());
    let g = geometry(&doc, gable);
    assert_eq!(g.predefined, "GABLE_ROOF");
    assert!(
        (max_z(&g) - 5.0).abs() < 1e-6,
        "the ridge's top at 3 + 2: {}",
        max_z(&g)
    );
    // It spans 6 m across the grid's plane (y here; the grid's normal may point either way).
    let ys: Vec<f64> = g.footprint.iter().map(|p| p[1]).collect();
    let span = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - ys.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((span - 6.0).abs() < 1e-6, "{ys:?}");
    // The level moves it up; the grid moves it across.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 3.5 }),
    )
    .unwrap();
    assert!((max_z(&geometry(&doc, gable)) - 5.5).abs() < 1e-6);
    let before: f64 = geometry(&doc, gable).footprint.iter().map(|p| p[1]).sum();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [grid.0], "vector": [0.0, 1.0, 0.0] }),
    )
    .unwrap();
    let after: f64 = geometry(&doc, gable).footprint.iter().map(|p| p[1]).sum();
    assert!((after - before - 4.0).abs() < 1e-6, "{before} {after}");

    // An arc profile: a barrel roof, as thick everywhere.
    let rp = id(&run(
        &r,
        &mut doc,
        "refplane.create",
        json!({ "start": [20.0, 0.0], "end": [28.0, 0.0], "name": "Barrel" }),
    )
    .unwrap());
    let barrel = id(&run(
        &r,
        &mut doc,
        "roof.create_extrusion",
        json!({ "ref_plane": rp.0, "profile": [{ "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] }, "through": [[4.0, 2.0]] }], "ref_level": l1.0, "start": 0.0, "end": 8.0 }),
    )
    .unwrap());
    let g = geometry(&doc, barrel);
    assert_eq!(g.predefined, "BARREL_ROOF");
    // Uniform thickness: each face's top is `thickness` above its underside, square to it.
    let (pts, _) = &g.faces[0].slab;
    let n = pts.len() / 4;
    for f in &g.faces {
        let b = f.bottom[0];
        // The top points over this face: the nearest top point to its underside corner.
        let top = pts[2 * n..]
            .iter()
            .min_by(|p, q| {
                let d = |p: &[f64; 3]| {
                    (p[0] - b[0]).powi(2) + (p[1] - b[1]).powi(2) + (p[2] - b[2]).powi(2)
                };
                d(p).total_cmp(&d(q))
            })
            .unwrap();
        let d = (top[0] - b[0]) * f.normal[0]
            + (top[1] - b[1]) * f.normal[1]
            + (top[2] - b[2]) * f.normal[2];
        assert!((d - g.thickness).abs() < 1e-6, "{d}");
    }
    // A closed profile is refused.
    assert!(run(
        &r,
        &mut doc,
        "roof.create_extrusion",
        json!({ "ref_plane": rp.0, "profile": [line([0.0, 0.0], [4.0, 2.0]), line([4.0, 2.0], [0.0, 0.0])], "ref_level": l1.0 }),
    )
    .is_err());

    // A 30° footprint roof with an opening by face on its south slope.
    let rect = [[0.0, 20.0], [10.0, 20.0], [10.0, 26.0], [0.0, 26.0]];
    let curves: Vec<Value> = (0..4).map(|i| line(rect[i], rect[(i + 1) % 4])).collect();
    let roof = id(&run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": l1.0, "slope": "30°" }),
    )
    .unwrap());
    let square = [[4.5, 20.8], [5.5, 20.8], [5.5, 21.6], [4.5, 21.6]];
    let opening = id(&run(
        &r,
        &mut doc,
        "opening.create_by_face",
        json!({ "host_id": roof.0, "loops": [square] }),
    )
    .unwrap());
    let g = geometry(&doc, roof);
    assert_eq!(g.uncut, 0);
    let face = g
        .faces
        .iter()
        .find(|f| !f.holes.is_empty())
        .expect("a cut face");
    // The hole's sides are square to the face: from the hole's bottom to its top is along the
    // face's normal, which leans 30° from vertical.
    let n = face.normal;
    assert!((n[2] - 30f64.to_radians().cos()).abs() < 1e-9, "{n:?}");
    let (pts, _) = &face.slab;
    let b = face.holes[0][0];
    let top = pts
        .iter()
        .filter(|p| (p[2] - b[2]) > 1e-3)
        .min_by(|p, q| {
            let d = |p: &[f64; 3]| {
                let v = [p[0] - b[0], p[1] - b[1], p[2] - b[2]];
                let c = [
                    v[1] * n[2] - v[2] * n[1],
                    v[2] * n[0] - v[0] * n[2],
                    v[0] * n[1] - v[1] * n[0],
                ];
                c[0].powi(2) + c[1].powi(2) + c[2].powi(2)
            };
            d(p).total_cmp(&d(q))
        })
        .unwrap();
    let v = [top[0] - b[0], top[1] - b[1], top[2] - b[2]];
    let len = (v[0].powi(2) + v[1].powi(2) + v[2].powi(2)).sqrt();
    let along = (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]) / len;
    assert!(
        (along - 1.0).abs() < 1e-6,
        "the side runs along the normal: {along}"
    );
    assert!(
        (v[1].abs() / len - 30f64.to_radians().sin()).abs() < 1e-6,
        "leans 30°"
    );

    // Saved: the opening by face is a B-rep prism voiding the roof; the extruded roofs are roofs.
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let rows = ifc.step_rows().unwrap();
    let opening_guid = doc.store().get(opening).unwrap().guid().to_string();
    let row = rows
        .iter()
        .find(|r| r.global_id.as_deref() == Some(opening_guid.as_str()))
        .unwrap();
    assert_eq!(row.class, "IFCOPENINGELEMENT");
    assert!(rows.iter().filter(|r| r.class == "IFCROOF").count() >= 3);
    assert!(
        rows.iter()
            .any(|r| r.class == "IFCROOF" && r.attributes.contains(".BARREL_ROOF."))
    );
}

#[test]
fn a_slope_arrow_turns_a_gable_into_a_hip() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("arrow.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // 10 × 6 m, the long edges sloping at 30°, the short ends gables.
    let rect = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": rect[i], "end": rect[(i + 1) % 4] }, "defines_slope": i % 2 == 0 }))
        .collect();
    let roof = id(&run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": l1.0, "slope": "30°" }),
    )
    .unwrap());
    let gable_faces = geometry(&doc, roof).faces.len();
    // An arrow at 30° from the middle of the east end (curve 2) to the ridge.
    run(
        &r,
        &mut doc,
        "roof.set_slope_arrow",
        json!({ "roof_id": roof.0, "key": 2, "tail": [10.0, 3.0], "head": [7.0, 3.0], "specify": "slope", "slope": "30°" }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert!(g.faces.len() > gable_faces, "a hip plane appeared");
    // The east plane: its normal points east, 30° from vertical.
    let east = g
        .faces
        .iter()
        .find(|f| f.normal[0] > 0.1)
        .expect("an east-facing plane");
    assert!(
        (east.normal[2] - 30f64.to_radians().cos()).abs() < 1e-6,
        "{:?}",
        east.normal
    );
    // An arrow on a slope-defining edge, or with its tail off the edge, is refused.
    assert!(run(
        &r,
        &mut doc,
        "roof.set_slope_arrow",
        json!({ "roof_id": roof.0, "key": 1, "tail": [5.0, 0.0], "head": [5.0, 3.0], "specify": "slope" }),
    )
    .is_err());
    assert!(run(
        &r,
        &mut doc,
        "roof.set_slope_arrow",
        json!({ "roof_id": roof.0, "key": 4, "tail": [1.0, 3.0], "head": [3.0, 3.0], "specify": "slope" }),
    )
    .is_err());
    // Removed, the end is a gable again.
    run(
        &r,
        &mut doc,
        "roof.remove_slope_arrow",
        json!({ "roof_id": roof.0, "key": 2 }),
    )
    .unwrap();
    assert_eq!(geometry(&doc, roof).faces.len(), gable_faces);
}

#[test]
fn a_porch_roof_joined_to_the_main_roof_ends_on_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("join.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let line = |a: [f64; 2], b: [f64; 2], slopes: bool| json!({ "line": { "start": a, "end": b }, "defines_slope": slopes });
    // The main roof: 10 × 6 m, gables east and west, at 3 m, 30°.
    let main = id(&run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": [line([0.0, 0.0], [10.0, 0.0], true), line([10.0, 0.0], [10.0, 6.0], false), line([10.0, 6.0], [0.0, 6.0], true), line([0.0, 6.0], [0.0, 0.0], false)], "level_id": l1.0, "slope": "30°", "base_offset": 3.0 }),
    )
    .unwrap());
    // A shed porch south of it, at 2.5 m, rising north at 20° (only its south edge slopes).
    let porch = id(&run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": [line([2.0, -4.0], [6.0, -4.0], true), line([6.0, -4.0], [6.0, -1.0], false), line([6.0, -1.0], [2.0, -1.0], false), line([2.0, -1.0], [2.0, -4.0], false)], "level_id": l1.0, "slope": "20°", "base_offset": 2.5 }),
    )
    .unwrap());
    let max_y = |g: &roofs::RoofGeometry| {
        g.faces
            .iter()
            .flat_map(|f| f.slab.0.iter().map(|p| p[1]))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    assert!((max_y(&geometry(&doc, porch)) + 1.0).abs() < 1e-6);
    // Joined (its north edge, curve 3) to the main roof: it reaches in and ends on its top.
    run(
        &r,
        &mut doc,
        "roof.join",
        json!({ "roof_id": porch.0, "key": 3, "target_id": main.0 }),
    )
    .unwrap();
    let g = geometry(&doc, porch);
    assert!(max_y(&g) > -0.5, "{}", max_y(&g));
    let mg = geometry(&doc, main);
    let south = mg.faces.iter().find(|f| f.normal[1] < -0.1).unwrap();
    let n = south.normal;
    let q = south.bottom[0];
    let d = n[0] * (q[0] + n[0] * mg.thickness)
        + n[1] * (q[1] + n[1] * mg.thickness)
        + n[2] * (q[2] + n[2] * mg.thickness);
    let below = g
        .faces
        .iter()
        .flat_map(|f| f.slab.0.iter())
        .map(|p| n[0] * p[0] + n[1] * p[1] + n[2] * p[2] - d)
        .fold(f64::INFINITY, f64::min);
    assert!(below > -1e-6, "nothing below the main roof's top: {below}");
    // Unjoined, it ends where it did.
    run(&r, &mut doc, "roof.unjoin", json!({ "roof_id": porch.0 })).unwrap();
    assert!((max_y(&geometry(&doc, porch)) + 1.0).abs() < 1e-6);
    // Joined to a wall at y = 2 (its south face at 1.9): it ends at that face.
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 2.0], "end": [10.0, 2.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let width = reviera_core::walls::regenerate(doc.store(), doc.store().get(wall).unwrap())
        .unwrap()
        .width;
    run(
        &r,
        &mut doc,
        "roof.join",
        json!({ "roof_id": porch.0, "key": 3, "target_id": wall.0 }),
    )
    .unwrap();
    assert!((max_y(&geometry(&doc, porch)) - (2.0 - width / 2.0)).abs() < 1e-6);
}
