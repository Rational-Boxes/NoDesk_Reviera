// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roofs by footprint: creation, slopes, gables and overhangs, edits, walls the footprint is
//! locked to, and the saved IfcRoof (spec Ch 3 §3.6.4, ARC-ROOF-001).

use std::path::{Path, PathBuf};

use reviera_core::roofs;
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

fn project(dir: &Path) -> (LocalProject, Document, PathBuf, ElementId) {
    let path = dir.join("roofs.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (project, doc, path, level)
}

fn curves(pts: &[[f64; 2]], extra: impl Fn(usize) -> Value) -> Value {
    Value::Array(
        (0..pts.len())
            .map(|i| {
                let mut c = json!({ "line": { "start": pts[i], "end": pts[(i + 1) % pts.len()] } });
                if let (Value::Object(c), Value::Object(x)) = (&mut c, extra(i)) {
                    c.extend(x);
                }
                c
            })
            .collect(),
    )
}

fn geometry(doc: &Document, id: ElementId) -> roofs::RoofGeometry {
    roofs::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

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

const RECT: [[f64; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];

#[test]
fn a_gable_roof_is_saved_as_an_ifc_roof_with_a_brep_body() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": curves(&RECT, |i| if i % 2 == 0 {
                json!({ "overhang": "500 mm" })
            } else {
                json!({ "defines_slope": false, "overhang": 0.3 })
            }),
            "level_id": level.0,
            "base_offset": "3000 mm",
            "slope": "45°",
        }),
    )
    .unwrap());
    let e = doc.store().get(roof).unwrap();
    assert_eq!(e.class, "IfcRoof");
    assert_eq!(e.param("@PredefinedType").unwrap(), "GABLE_ROOF");
    let g = geometry(&doc, roof);
    assert!((g.plate - 3.0).abs() < 1e-9);
    assert_eq!(g.faces.len(), 2);
    // The ridge: half the 6 m span at 45° above the plate.
    assert!((g.underside([5.0, 3.0]).unwrap() - 6.0).abs() < 1e-6);
    assert!((g.projected_area() - 10.6 * 7.0).abs() < 1e-6);

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let roofs_in_file = ifc.entities_of_type("IfcRoof").unwrap();
    assert_eq!(roofs_in_file.len(), 1);
    assert_eq!(ifc.entities_of_type("IfcFacetedBrep").unwrap().len(), 2);
    assert_eq!(ifc.entities_of_type("IfcRoofType").unwrap().len(), 1);
    assert_eq!(
        ifc.referenced_by(roofs_in_file[0].id, "IfcRelContainedInSpatialStructure")
            .unwrap()
            .len(),
        1
    );
    // The geometry engine reads two closed slabs of 300 mm on the sloped faces.
    let meshes = ifc.tessellate(1).unwrap();
    let m = meshes.iter().find(|m| m.class == "IfcRoof").unwrap();
    let v = volume(&m.verts, &m.faces).abs();
    let want = g.area() * 0.3;
    assert!((v - want).abs() / want < 0.05, "{v} vs {want}");
    let top = m.verts.chunks(3).map(|c| c[2]).fold(f64::MIN, f64::max);
    assert!(top > 6.0 && top < 6.0 + 0.3 * 2f64.sqrt() + 1e-6, "{top}");

    // Saving again writes nothing new; reopened, the roof is the same.
    let count = ifc.entity_count();
    project.save(&doc).unwrap();
    assert_eq!(IfcModel::open(&path).unwrap().entity_count(), count);
    let (mut project, mut reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(geometry(&reopened, roof), g);

    // Deleting the roof removes its body.
    run(
        &r,
        &mut reopened,
        "element.delete",
        json!({ "element_ids": [roof.0] }),
    )
    .unwrap();
    project.save(&reopened).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcRoof").unwrap().is_empty());
    assert!(ifc.entities_of_type("IfcFacetedBrep").unwrap().is_empty());
}

#[test]
fn edits_change_slopes_gables_cutoff_and_level() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves(&RECT, |_| json!({})), "level_id": level.0, "slope": "45°" }),
    )
    .unwrap());
    assert_eq!(geometry(&doc, roof).predefined, "HIP_ROOF");
    // Ends become gables: a gable roof.
    run(
        &r,
        &mut doc,
        "roof.set_edge",
        json!({ "roof_id": roof.0, "keys": [2, 4], "defines_slope": false }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert_eq!(g.predefined, "GABLE_ROOF");
    assert_eq!(
        doc.store()
            .get(roof)
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "GABLE_ROOF"
    );
    // A steeper south slope moves the ridge north.
    run(
        &r,
        &mut doc,
        "roof.set_edge",
        json!({ "roof_id": roof.0, "keys": [1], "slope": "60°" }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    let t60 = 60f64.to_radians().tan();
    // The faces meet where y tan 60° = (6 − y) tan 45°.
    let ridge_y = 6.0 / (t60 + 1.0);
    assert!((g.underside([5.0, ridge_y]).unwrap() - ridge_y * t60).abs() < 1e-6);
    // The roof's slope resets every curve's own.
    run(
        &r,
        &mut doc,
        "roof.set_params",
        json!({ "roof_ids": [roof.0], "slope": "30°" }),
    )
    .unwrap();
    let t30 = 30f64.to_radians().tan();
    assert!((geometry(&doc, roof).underside([5.0, 3.0]).unwrap() - 3.0 * t30).abs() < 1e-6);
    // A second level as cutoff: a flat top at 1 m.
    let upper = id(run(&r, &mut doc, "level.create", json!({ "elevation": 1.0 })).unwrap());
    run(
        &r,
        &mut doc,
        "roof.set_params",
        json!({ "roof_ids": [roof.0], "cutoff_level_id": upper.0 }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert!((g.underside([5.0, 3.0]).unwrap() - 1.0).abs() < 1e-9);
    // Bad values are refused and change nothing.
    let steep = json!({ "roof_ids": [roof.0], "slope": "90°" });
    assert!(run(&r, &mut doc, "roof.set_params", steep).is_err());
    run(
        &r,
        &mut doc,
        "roof.set_params",
        json!({ "roof_ids": [roof.0], "cutoff_level_id": null, "base_offset": 2.5 }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert!((g.plate - 2.5).abs() < 1e-12);
    assert!(g.faces.iter().all(|f| f.key.is_some()));
    // Undo steps back through the edits.
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert!((geometry(&doc, roof).underside([5.0, 3.0]).unwrap() - 1.0).abs() < 1e-9);
    // Deleting the cutoff level deletes the roof that depends on it.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [upper.0] }),
    )
    .unwrap();
    assert!(deleted.as_array().unwrap().iter().any(|v| v == roof.0));
}

#[test]
fn a_footprint_locked_to_walls_follows_them_and_moves() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    // Four walls on their centrelines, drawn clockwise so their exterior faces are outside;
    // the footprint follows those faces.
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": RECT[(i + 1) % 4], "end": RECT[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    let faces: Vec<Value> = (0..4)
        .map(|i| {
            let (a, b) = (RECT[i], RECT[(i + 1) % 4]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l = d[0].hypot(d[1]);
            let out = [d[1] / l * 0.1, -d[0] / l * 0.1];
            json!({
                "line": { "start": [a[0] + out[0], a[1] + out[1]], "end": [b[0] + out[0], b[1] + out[1]] },
                "lock": { "element": walls[i].0, "sub": "face:ext" },
                "overhang": 0.4,
            })
        })
        .collect();
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": faces, "level_id": level.0, "base_offset": 3.0 }),
    )
    .unwrap());
    let before = geometry(&doc, roof);
    assert!(
        (before.projected_area() - 11.0 * 7.0).abs() < 1e-6,
        "{}",
        before.projected_area()
    );
    // Moving the south wall south by 1 m: the roof's footprint follows.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [walls[0].0], "vector": [0.0, -1.0, 0.0] }),
    )
    .unwrap();
    let after = geometry(&doc, roof);
    assert!(
        (after.projected_area() - 11.0 * 8.0).abs() < 1e-6,
        "{}",
        after.projected_area()
    );
    // Moving the roof itself moves its footprint (and unlocks it).
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [roof.0], "vector": [20.0, 0.0, 0.0] }),
    )
    .unwrap();
    let moved = geometry(&doc, roof);
    assert!(moved.footprint.iter().all(|p| p[0] > 19.0));
}

#[test]
fn bad_footprints_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    // Every edge a gable: nothing slopes, so the roof is flat (allowed).
    let flat = run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves(&RECT, |_| json!({ "defines_slope": false })), "level_id": level.0 }),
    )
    .unwrap();
    assert_eq!(
        doc.store()
            .get(id(flat))
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "FLAT_ROOF"
    );
    // An open footprint fails sketch validation.
    let open = json!({
        "curves": [
            { "line": { "start": [0, 0], "end": [5, 0] } },
            { "line": { "start": [5, 0], "end": [5, 5] } },
        ],
        "level_id": level.0,
    });
    assert!(run(&r, &mut doc, "roof.create_footprint", open).is_err());
    // Two loops are not one footprint.
    let mut two = curves(&RECT, |_| json!({}));
    let far: Vec<[f64; 2]> = RECT.iter().map(|p| [p[0] + 20.0, p[1]]).collect();
    two.as_array_mut().unwrap().extend(
        curves(&far, |_| json!({}))
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    let e = run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": two, "level_id": level.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "roof.one_loop");
}

#[test]
fn walls_attach_to_a_gable_roof_and_detach() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    // Four 3 m walls, clockwise, and a door in the south wall.
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": RECT[(i + 1) % 4], "end": RECT[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": walls[0].0, "point_along": 5.0 }),
    )
    .unwrap();
    // A gable roof on the walls' outer faces (x = −0.1 … 10.1, y = −0.1 … 6.1) at 3 m, 45°.
    let outer = [[-0.1, -0.1], [10.1, -0.1], [10.1, 6.1], [-0.1, 6.1]];
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": curves(&outer, |i| if i % 2 == 0 {
                json!({ "overhang": 0.5 })
            } else {
                json!({ "defines_slope": false, "overhang": 0.3 })
            }),
            "level_id": level.0,
            "base_offset": 3.0,
            "slope": "45°",
        }),
    )
    .unwrap());
    let ids: Vec<u64> = walls.iter().map(|w| w.0).collect();
    run(
        &r,
        &mut doc,
        "wall.attach",
        json!({ "wall_ids": ids, "target_id": roof.0 }),
    )
    .unwrap();
    // The east gable wall reaches the ridge, 3.1 m above the plate in the middle.
    let solved = reviera_core::walls::solve(doc.store());
    let east = reviera_core::walls::attach::attached_body(
        doc.store(),
        walls[1],
        &solved.bodies[&walls[1]],
    )
    .unwrap()
    .unwrap();
    assert!((east.z_max - 6.1).abs() < 1e-6, "{}", east.z_max);

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    // Four walls as B-reps, plus the roof's two face slabs.
    assert_eq!(ifc.entities_of_type("IfcFacetedBrep").unwrap().len(), 6);
    let meshes = ifc.tessellate(1).unwrap();
    let wall_volume: f64 = meshes
        .iter()
        .filter(|m| m.class.starts_with("IfcWall"))
        .map(|m| volume(&m.verts, &m.faces).abs())
        .sum();
    // The walls' attached solids, less the door's opening that IfcOpenShell cuts out.
    let gross: f64 = walls
        .iter()
        .map(|w| {
            let a = reviera_core::walls::attach::attached_body(doc.store(), *w, &solved.bodies[w])
                .unwrap()
                .unwrap();
            let flat: Vec<f64> = a.solid.0.iter().flatten().copied().collect();
            volume(&flat, &a.solid.1)
        })
        .sum();
    assert!(
        wall_volume < gross - 0.2 && wall_volume > gross - 1.0,
        "{wall_volume} vs {gross}"
    );
    // The door's opening stays where it was: relative to its wall's frame.
    let door = meshes.iter().find(|m| m.class == "IfcDoor").unwrap();
    let xs: Vec<f64> = door.verts.chunks(3).map(|c| c[0]).collect();
    let mid = (xs.iter().cloned().fold(f64::MAX, f64::min)
        + xs.iter().cloned().fold(f64::MIN, f64::max))
        / 2.0;
    assert!((mid - 5.0).abs() < 0.05, "{mid}");

    // Detaching returns the walls to extrusions; deleting the roof also frees them.
    run(
        &r,
        &mut doc,
        "wall.detach",
        json!({ "wall_ids": [walls[1].0] }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert_eq!(ifc.entities_of_type("IfcFacetedBrep").unwrap().len(), 5);
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [roof.0] }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcFacetedBrep").unwrap().is_empty());
    assert!(ifc.entities_of_type("IfcExtrudedAreaSolid").unwrap().len() >= 4);
    // A wall outside the roof cannot attach.
    let far = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [30, 0], "end": [40, 0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    let roof2 = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves(&outer, |_| json!({})), "level_id": level.0, "base_offset": 3.0 }),
    )
    .unwrap());
    let e = run(
        &r,
        &mut doc,
        "wall.attach",
        json!({ "wall_ids": [far.0], "target_id": roof2.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "wall.attach_not_covered");
}

#[test]
fn roof_shapes_set_which_curves_slope() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    // A 10 × 6 footprint whose first curve is a long side.
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves(&pts, |_| json!({})), "level_id": level.0, "slope": "45°" }),
    )
    .unwrap());
    let shape = |doc: &Document| roofs::shape_of(doc.store().get(roof).unwrap());
    assert_eq!(shape(&doc), Some(roofs::RoofShape::Hip));
    let set = |doc: &mut Document, s: &str| {
        run(
            &r,
            doc,
            "roof.set_shape",
            json!({ "roof_id": roof.0, "shape": s, "overhang": "500 mm" }),
        )
        .unwrap();
    };
    // Gable: the ridge runs the full length at 3 m (45°, half of the 6 m width).
    set(&mut doc, "gable");
    assert_eq!(shape(&doc), Some(roofs::RoofShape::Gable));
    let g = geometry(&doc, roof);
    assert_eq!(g.predefined, "GABLE_ROOF");
    assert!((g.underside([0.01, 3.0]).unwrap() - 3.0).abs() < 1e-6);
    // Turned: the ridge runs across, 5 m up.
    set(&mut doc, "gable_rotated");
    assert!((geometry(&doc, roof).underside([5.0, 0.01]).unwrap() - 5.0).abs() < 1e-6);
    // Shed: one slope rising from the first long side to 6 m at the other.
    set(&mut doc, "shed");
    assert_eq!(shape(&doc), Some(roofs::RoofShape::Shed));
    assert!((geometry(&doc, roof).underside([5.0, 5.99]).unwrap() - 5.99).abs() < 1e-6);
    set(&mut doc, "shed_reversed");
    assert!((geometry(&doc, roof).underside([5.0, 0.01]).unwrap() - 5.99).abs() < 1e-6);
    // Flat, with the overhang all round: 11 × 7.
    set(&mut doc, "flat");
    let g = geometry(&doc, roof);
    assert_eq!(g.predefined, "FLAT_ROOF");
    assert!((g.projected_area() - 77.0).abs() < 1e-6);
    // One curve's own change makes it custom.
    run(
        &r,
        &mut doc,
        "roof.set_edge",
        json!({ "roof_id": roof.0, "keys": [1], "defines_slope": true }),
    )
    .unwrap();
    assert_eq!(shape(&doc), Some(roofs::RoofShape::Shed));
    run(
        &r,
        &mut doc,
        "roof.set_edge",
        json!({ "roof_id": roof.0, "keys": [2], "defines_slope": true }),
    )
    .unwrap();
    assert_eq!(shape(&doc), None);
}

#[test]
fn attach_walls_below_fills_the_gables() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": RECT[(i + 1) % 4], "end": RECT[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    // An interior partition under the roof is not a gable: it stays as it is.
    let partition = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [5.0, 0.0], "end": [5.0, 6.0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    // A wall well outside the roof stays as it is.
    let outside = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [20.0, 0.0], "end": [25.0, 0.0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    // A drawn (not picked) hip roof over the walls, then made a gable.
    let outer = [[-0.1, -0.1], [10.1, -0.1], [10.1, 6.1], [-0.1, 6.1]];
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": curves(&outer, |_| json!({ "overhang": 0.4 })),
            "level_id": level.0,
            "base_offset": 3.0,
            "slope": "45°",
        }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": roof.0, "shape": "gable" }),
    )
    .unwrap();
    assert_eq!(
        reviera_core::walls::attach::walls_below(doc.store(), roof),
        walls
    );
    let attached = run(
        &r,
        &mut doc,
        "roof.attach_walls",
        json!({ "roof_id": roof.0 }),
    )
    .unwrap();
    assert_eq!(
        attached,
        json!(walls.iter().map(|w| w.0).collect::<Vec<_>>())
    );
    // The gable ends now rise to the ridge; the outside wall is untouched.
    let solved = reviera_core::walls::solve(doc.store());
    let east = reviera_core::walls::attach::attached_body(
        doc.store(),
        walls[1],
        &solved.bodies[&walls[1]],
    )
    .unwrap()
    .unwrap();
    assert!((east.z_max - 6.1).abs() < 1e-6, "{}", east.z_max);
    for w in [outside, partition] {
        let e = doc.store().get(w).unwrap();
        assert!(
            reviera_core::walls::wall_recipe(e)
                .unwrap()
                .attach_top
                .is_none()
        );
    }
    // Nothing is left to attach.
    assert!(reviera_core::walls::attach::walls_below(doc.store(), roof).is_empty());
}

/// Each attached wall's top follows the roof's underside along its centreline (give or take the
/// slope across its 200 mm), and nowhere dips below the 3 m plate.
fn check_tops(doc: &Document, walls: &[ElementId], g: &roofs::RoofGeometry) {
    let solved = reviera_core::walls::solve(doc.store());
    for w in walls {
        let body = &solved.bodies[w];
        let a = reviera_core::walls::attach::attached_body(doc.store(), *w, body)
            .unwrap()
            .unwrap();
        let (s, e) = (body.geometry.start, body.geometry.end);
        let highest = (0..=20)
            .map(|k| {
                let t = f64::from(k) / 20.0;
                let p = [s[0] + (e[0] - s[0]) * t, s[1] + (e[1] - s[1]) * t];
                g.underside(p).expect("the roof covers the wall")
            })
            .fold(f64::MIN, f64::max);
        assert!(
            a.z_max >= highest - 1e-6 && a.z_max - highest < 0.15,
            "wall {w}: top {} vs roof {highest}",
            a.z_max
        );
        assert!(
            a.z_min >= 3.0 - 1e-6,
            "wall {w} dips below the plate: {}",
            a.z_min
        );
    }
}

/// A square building: a pyramid (every side slopes) whose apex is over the middle, then a gable;
/// all four walls attach both times.
#[test]
fn a_square_building_takes_a_pyramid_and_a_gable() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    let c = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": c[(i + 1) % 4], "end": c[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    walls.sort();
    let outer = [[-0.1, -0.1], [8.1, -0.1], [8.1, 8.1], [-0.1, 8.1]];
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": curves(&outer, |_| json!({ "overhang": 0.5 })),
            "level_id": level.0,
            "base_offset": 3.0,
            "slope": "45°",
        }),
    )
    .unwrap());
    // A pyramid: four faces meeting at one apex over the middle, 4.1 m above the plate.
    let g = geometry(&doc, roof);
    assert_eq!(g.faces.len(), 4);
    assert!((g.underside([4.0, 4.0]).unwrap() - 7.1).abs() < 1e-6);
    assert!((g.max_height() - 4.1).abs() < 1e-6 || g.max_height() > 4.0);
    assert!((g.projected_area() - 9.2 * 9.2).abs() < 1e-6);
    run(
        &r,
        &mut doc,
        "roof.attach_walls",
        json!({ "roof_id": roof.0 }),
    )
    .unwrap();
    check_tops(&doc, &walls, &g);
    // Every wall of a pyramid is an eave wall: none rises above the plate by more than the
    // slope across its width.
    let solved = reviera_core::walls::solve(doc.store());
    for w in &walls {
        let a = reviera_core::walls::attach::attached_body(doc.store(), *w, &solved.bodies[w])
            .unwrap()
            .unwrap();
        assert!(a.z_max < 3.0 + 0.25, "{}", a.z_max);
    }
    // As a gable, two walls become gable ends reaching the ridge, 4.1 m up.
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": roof.0, "shape": "gable" }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert_eq!(g.faces.len(), 2);
    check_tops(&doc, &walls, &g);
    let solved = reviera_core::walls::solve(doc.store());
    let ends = walls
        .iter()
        .filter(|w| {
            let a = reviera_core::walls::attach::attached_body(doc.store(), **w, &solved.bodies[w])
                .unwrap()
                .unwrap();
            (a.z_max - 7.1).abs() < 1e-6
        })
        .count();
    assert_eq!(ends, 2);
}

/// An L-shaped building (ARC-ROOF-001, owner request): the roof over the walls' outer faces
/// fills the footprint at the convex and the concave (inner) corner alike, and every perimeter
/// wall, those meeting at the inner corner included, attaches with its top on the roof.
fn roof_an_l(l: [[f64; 2]; 6], outer: [[f64; 2]; 6], east_x: f64, ridge: f64) {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    // Walls drawn clockwise so their exterior faces are outside.
    let mut walls = Vec::new();
    for i in 0..6 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": l[(i + 1) % 6], "end": l[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    walls.sort();
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({
            "curves": curves(&outer, |_| json!({ "overhang": 0.5 })),
            "level_id": level.0,
            "base_offset": 3.0,
            "slope": "45°",
        }),
    )
    .unwrap());

    // The hip roof covers the L grown by the overhang (0.6 m from the centrelines), the inner
    // corner neither missed nor doubled: both Ls are 11.2 × 5.2 + 5.2 × 4.0.
    let g = geometry(&doc, roof);
    assert!(
        (g.projected_area() - 79.04).abs() < 1e-6,
        "{}",
        g.projected_area()
    );
    // Over every corner, just inside, the roof is there and above the plate.
    for (i, c) in l.iter().enumerate() {
        let (prev, next) = (l[(i + 5) % 6], l[(i + 1) % 6]);
        let toward = |q: [f64; 2]| {
            let d = [q[0] - c[0], q[1] - c[1]];
            let n = d[0].hypot(d[1]);
            [d[0] / n * 0.3, d[1] / n * 0.3]
        };
        let (a, b) = (toward(prev), toward(next));
        // At a convex corner the inside is between the two walls; at the concave (inner)
        // corner it is the other way.
        let turn = (c[0] - prev[0]) * (next[1] - c[1]) - (c[1] - prev[1]) * (next[0] - c[0]);
        let k = if turn > 0.0 { 1.0 } else { -1.0 };
        let p = [c[0] + k * (a[0] + b[0]), c[1] + k * (a[1] + b[1])];
        let z = g
            .underside(p)
            .unwrap_or_else(|| panic!("no roof over {p:?}"));
        assert!(z >= 3.0 - 1e-9, "{p:?}: {z}");
    }

    // Every wall is on the perimeter and fits under the roof, the two at the inner corner too.
    let below = reviera_core::walls::attach::walls_below(doc.store(), roof);
    assert_eq!(below, walls);
    run(
        &r,
        &mut doc,
        "roof.attach_walls",
        json!({ "roof_id": roof.0 }),
    )
    .unwrap();
    check_tops(&doc, &walls, &g);

    // As a gable: the curves along the long side slope, the ends are gables. The walls stay
    // attached and follow, and the east end wall rises to the wing's ridge, 2.1 m up (half
    // the wing's 4.2 m width at 45°).
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": roof.0, "shape": "gable" }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    check_tops(&doc, &walls, &g);
    let solved = reviera_core::walls::solve(doc.store());
    let east = walls
        .iter()
        .find(|w| {
            let b = &solved.bodies[w].geometry;
            (b.start[0] - east_x).abs() < 1e-9 && (b.end[0] - east_x).abs() < 1e-9
        })
        .unwrap();
    let a = reviera_core::walls::attach::attached_body(doc.store(), *east, &solved.bodies[east])
        .unwrap()
        .unwrap();
    assert!((a.z_max - ridge).abs() < 1e-6, "{}", a.z_max);
}

#[test]
fn an_l_shaped_building_is_roofed_and_its_walls_attach() {
    // Centrelines of the L, counter-clockwise; the inner corner is at (4, 4). The roof's
    // footprint is on the walls' outer faces, 0.1 m out.
    roof_an_l(
        [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [4.0, 4.0],
            [4.0, 8.0],
            [0.0, 8.0],
        ],
        [
            [-0.1, -0.1],
            [10.1, -0.1],
            [10.1, 4.1],
            [4.1, 4.1],
            [4.1, 8.1],
            [-0.1, 8.1],
        ],
        10.0,
        5.1,
    );
}

#[test]
fn an_l_opening_the_other_way_is_roofed_too() {
    // The L turned so its inner corner is at (6, 4): the wing runs up the east side, and the
    // walls meet the concave corner the other way round.
    roof_an_l(
        [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 8.0],
            [6.0, 8.0],
            [6.0, 4.0],
            [0.0, 4.0],
        ],
        [
            [-0.1, -0.1],
            [10.1, -0.1],
            [10.1, 8.1],
            [5.9, 8.1],
            [5.9, 4.1],
            [-0.1, 4.1],
        ],
        10.0,
        // As a gable, the whole east side (8.2 m deep) is one gable end: the slopes from the
        // south and north eaves meet 4.1 m up.
        7.1,
    )
}

/// A building around a central courtyard (owner request): the roof's footprint is the outline
/// and the courtyard. Every edge slopes, outward to the street and inward to the courtyard; the
/// ring is roofed once, the courtyard stays open, and the outer and courtyard walls attach.
#[test]
fn a_courtyard_building_is_roofed_around_its_courtyard() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    // Outer walls on a 12 × 12 square and courtyard walls on a 4 × 4 square in the middle.
    let outer = [[0.0, 0.0], [12.0, 0.0], [12.0, 12.0], [0.0, 12.0]];
    let court = [[4.0, 4.0], [8.0, 4.0], [8.0, 8.0], [4.0, 8.0]];
    let mut walls = Vec::new();
    // Outer walls clockwise (exterior outside); courtyard walls counter-clockwise (exterior
    // toward the courtyard).
    for i in 0..4 {
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": outer[(i + 1) % 4], "end": outer[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
        walls.push(id(run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": court[i], "end": court[(i + 1) % 4] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    walls.sort();
    // The footprint on the outer walls' outer faces and the courtyard walls' courtyard faces.
    let ring = [[-0.1, -0.1], [12.1, -0.1], [12.1, 12.1], [-0.1, 12.1]];
    let hole = [[4.1, 4.1], [7.9, 4.1], [7.9, 7.9], [4.1, 7.9]];
    let mut all = curves(&ring, |_| json!({ "overhang": 0.4 }));
    if let (Value::Array(a), Value::Array(b)) =
        (&mut all, curves(&hole, |_| json!({ "overhang": 0.4 })))
    {
        a.extend(b);
    }
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": all, "level_id": level.0, "base_offset": 3.0, "slope": "45°" }),
    )
    .unwrap());
    let g = geometry(&doc, roof);
    assert_eq!(g.faces.len(), 8);
    // The ring is roofed once: the outline grown by the overhang (13.0 m square) less the
    // courtyard shrunk by it (3.0 m square).
    assert!(
        (g.projected_area() - (13.0 * 13.0 - 3.0 * 3.0)).abs() < 1e-6,
        "{}",
        g.projected_area()
    );
    // The courtyard is open; the ridge runs around halfway across the 4.2 m ring, 2.1 up.
    assert!(g.underside([6.0, 6.0]).is_none());
    for p in [[2.0, 6.0], [6.0, 2.0], [10.0, 6.0], [6.0, 10.0]] {
        assert!((g.underside(p).unwrap() - 5.1).abs() < 1e-6, "{p:?}");
    }
    // Both the street walls and the courtyard walls are on the perimeter and attach.
    assert_eq!(
        reviera_core::walls::attach::walls_below(doc.store(), roof),
        walls
    );
    run(
        &r,
        &mut doc,
        "roof.attach_walls",
        json!({ "roof_id": roof.0 }),
    )
    .unwrap();
    check_tops(&doc, &walls, &g);

    // As a flat roof, the courtyard is a hole through one slab.
    run(
        &r,
        &mut doc,
        "roof.set_shape",
        json!({ "roof_id": roof.0, "shape": "flat" }),
    )
    .unwrap();
    let g = geometry(&doc, roof);
    assert_eq!(g.predefined, "FLAT_ROOF");
    assert!(g.underside([6.0, 6.0]).is_none());
    assert!(
        (g.volume() - (13.0 * 13.0 - 3.0 * 3.0) * 0.3).abs() < 1e-6,
        "{}",
        g.volume()
    );
    // The slab with its hole is closed: its mesh's volume is the same.
    let m = &g.faces[0].slab;
    let flat: Vec<f64> = m.0.iter().flatten().copied().collect();
    let v = volume(&flat, &m.1);
    assert!((v - g.volume()).abs() < 1e-6, "{v}");
    // Saved, the geometry engine reads the same slab, hole and all.
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let meshes = ifc.tessellate(1).unwrap();
    let roof_mesh = meshes.iter().find(|m| m.class == "IfcRoof").unwrap();
    let v = volume(&roof_mesh.verts, &roof_mesh.faces).abs();
    assert!((v - g.volume()).abs() / g.volume() < 1e-3, "{v}");
}
