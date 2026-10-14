// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Doors and windows: placing, marks, fit, moving with the host, types, and IFC openings and
//! fills (spec Ch 3 §3.4.2–3.4.3, ARC-HOST-001; DM-IFC-007).

use std::path::{Path, PathBuf};

use reviera_core::inserts;
use reviera_core::walls::{self, openings};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{AttrValue, Entity, IfcModel, Schema, StepId};
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

fn new_project(dir: &Path) -> (LocalProject, Document, PathBuf) {
    let path = dir.join("inserts.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    (project, doc, path)
}

/// A 3 m high, 200 mm wall from (0, 0) to (`length`, 0) on Level 1.
fn wall(r: &CommandRegistry, doc: &mut Document, length: f64) -> ElementId {
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    id(run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 0], "end": [length, 0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap())
}

fn place(
    r: &CommandRegistry,
    doc: &mut Document,
    cmd: &str,
    host: ElementId,
    along: f64,
) -> ElementId {
    id(run(
        r,
        doc,
        cmd,
        json!({ "host_wall_id": host.0, "point_along": along }),
    )
    .unwrap())
}

fn placed(doc: &Document, id: ElementId) -> inserts::Placed {
    inserts::resolve(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

fn mark(doc: &Document, id: ElementId) -> String {
    doc.store()
        .get(id)
        .unwrap()
        .param("@Tag")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

fn entity(ifc: &IfcModel, id: StepId) -> Entity {
    ifc.attributes_many(&[id]).unwrap().remove(0)
}

/// Volume of a closed triangle mesh.
fn volume(verts: &[[f64; 3]], faces: &[u32]) -> f64 {
    faces
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [
                verts[t[0] as usize],
                verts[t[1] as usize],
                verts[t[2] as usize],
            ];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn doors_are_numbered_and_cut_their_wall() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let w = wall(&r, &mut doc, 30.0);
    let doors: Vec<ElementId> = (0..10)
        .map(|k| place(&r, &mut doc, "door.place", w, 1.0 + 2.5 * f64::from(k)))
        .collect();
    let marks: Vec<String> = doors.iter().map(|d| mark(&doc, *d)).collect();
    let expected: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
    assert_eq!(marks, expected);
    // Windows have their own series, and sit at the type's default sill.
    let win = place(&r, &mut doc, "window.place", w, 27.0);
    assert_eq!(mark(&doc, win), "1");
    let p = placed(&doc, win);
    assert!((p.z0 - 0.9).abs() < 1e-9 && (p.ty.height - 1.2).abs() < 1e-9);

    // Each insert has an opening, and the wall's body loses their volume.
    let openings_n = doc
        .store()
        .iter()
        .filter(|e| inserts::opening_of(e).is_some())
        .count();
    assert_eq!(openings_n, 11);
    let cuts = inserts::cuts_by_host(doc.store());
    let body = walls::solve(doc.store()).bodies[&w];
    let (v, i) = openings::cut_mesh(&body, &cuts[&w]);
    let expected = 30.0 * 3.0 * 0.2 - 0.2 * (10.0 * 0.9 * 2.1 + 1.2 * 1.2);
    assert!(
        (volume(&v, &i) - expected).abs() < 1e-6,
        "{}",
        volume(&v, &i)
    );
}

#[test]
fn inserts_must_fit_and_follow_their_wall() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let w = wall(&r, &mut doc, 6.0);
    let door = place(&r, &mut doc, "door.place", w, 1.0);
    // Overlapping, past the end, above the top: refused.
    for (cmd, args, code) in [
        (
            "door.place",
            json!({ "host_wall_id": w.0, "point_along": 1.5, "fit": false }),
            "insert.overlaps",
        ),
        (
            "door.place",
            json!({ "host_wall_id": w.0, "point_along": 5.8, "fit": false }),
            "insert.outside_host",
        ),
        (
            "window.place",
            json!({ "host_wall_id": w.0, "point_along": 4.0, "sill_height": 2.5, "fit": false }),
            "insert.outside_host",
        ),
    ] {
        assert_eq!(run(&r, &mut doc, cmd, args).unwrap_err().code, code);
    }
    // A plan point is projected onto the wall.
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": w.0, "point": [4.0, 3.0] }),
    )
    .unwrap());
    assert!((placed(&doc, win).recipe.along - 4.0).abs() < 1e-9);

    // Moving the wall moves its door; moving the door slides it along the wall.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [w.0], "vector": [0, 2, 0] }),
    )
    .unwrap();
    let p = placed(&doc, door);
    assert_eq!(p.world([0.0, 0.0, 0.0]), [1.0, 2.0, 0.0]);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [door.0], "vector": [0.5, 7, 0] }),
    )
    .unwrap();
    assert!((placed(&doc, door).recipe.along - 1.5).abs() < 1e-9);
    let e = run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [door.0], "vector": [2, 0, 0] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.overlaps");
    // Sill height moves the window without resizing it.
    run(
        &r,
        &mut doc,
        "insert.set_sill_height",
        json!({ "element_ids": [win.0], "sill_height": "1100 mm" }),
    )
    .unwrap();
    let p = placed(&doc, win);
    assert!((p.z0 - 1.1).abs() < 1e-9 && (p.ty.height - 1.2).abs() < 1e-9);

    // Flips: hand moves the hinge, facing the swing side.
    let swing = placed(&doc, door).swing();
    run(
        &r,
        &mut doc,
        "insert.flip_hand",
        json!({ "element_ids": [door.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "insert.flip_facing",
        json!({ "element_ids": [door.0] }),
    )
    .unwrap();
    let p = placed(&doc, door);
    assert!(p.recipe.flip_hand && p.recipe.flip_facing);
    // The open panel now runs to the interior side (−y) from the other jamb.
    let [a, b] = p.swing()[0];
    assert!(b[1] < a[1] && (a[0] - swing[0][0][0]).abs() > 0.5);

    // Deleting the wall takes its doors, windows and openings.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [w.0] }),
    )
    .unwrap();
    assert_eq!(deleted.as_array().unwrap().len(), 5);
    assert!(inserts::placed(doc.store()).is_empty());
}

#[test]
fn door_and_window_types_resize_and_swap() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let w = wall(&r, &mut doc, 8.0);
    let d1 = place(&r, &mut doc, "door.place", w, 1.0);
    let d2 = place(&r, &mut doc, "door.place", w, 2.2);
    let default = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcDoorType")
        .unwrap()
        .id();
    assert_eq!(
        doc.store().get(default).unwrap().name.as_deref(),
        Some(inserts::DEFAULT_DOOR_TYPE)
    );
    let wide = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": default.0, "new_name": "Double Door 1800 x 2100" }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": wide.0, "param": "width", "value": "1800 mm" }),
    )
    .unwrap();
    // Swapping d1 to it would run into d2.
    let e = run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [d1.0], "type_id": wide.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.overlaps");
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [d2.0], "vector": [1.3, 0, 0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [d2.0], "type_id": wide.0 }),
    )
    .unwrap();
    assert!((placed(&doc, d2).ty.width - 1.8).abs() < 1e-9);
    // A window type is not a door type; widening the default into d2 is refused.
    let win_type = {
        place(&r, &mut doc, "window.place", w, 7.0);
        doc.store()
            .iter()
            .find(|e| e.class == "IfcWindowType")
            .unwrap()
            .id()
    };
    let e = run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [d1.0], "type_id": win_type.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "type.wrong_family");
    let e = run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": default.0, "param": "width", "value": 6 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.outside_host");
    let e = run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": default.0, "param": "default_sill", "value": 1 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "type.unknown_param");
    // A window type's default sill applies to new windows only (ARC-WIN-001).
    let w1 = inserts::placed(doc.store())
        .into_iter()
        .find(|p| p.kind == inserts::Kind::Window)
        .unwrap();
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": win_type.0, "param": "default_sill", "value": "600 mm" }),
    )
    .unwrap();
    assert!((placed(&doc, w1.id).z0 - 0.9).abs() < 1e-9);
}

#[test]
fn saved_inserts_void_and_fill_their_wall() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    let w = wall(&r, &mut doc, 6.0);
    let door = place(&r, &mut doc, "door.place", w, 1.0);
    let win = place(&r, &mut doc, "window.place", w, 4.0);
    project.save(&doc).unwrap();

    let ifc = IfcModel::open(&path).unwrap();
    let wall_step = ifc.entities_of_type("IfcWall").unwrap()[0].id;
    let voids = ifc.referenced_by(wall_step, "IfcRelVoidsElement").unwrap();
    assert_eq!(voids.len(), 2);
    for (class, width, height) in [("IfcDoor", 0.9, 2.1), ("IfcWindow", 1.2, 1.2)] {
        let e = ifc.entities_of_type(class).unwrap()[0].id;
        let ent = entity(&ifc, e);
        assert_eq!(ent.get("OverallWidth"), &AttrValue::Real(width));
        assert_eq!(ent.get("OverallHeight"), &AttrValue::Real(height));
        assert_eq!(ent.string("Tag"), Some("1"));
        let fills = ifc.referenced_by(e, "IfcRelFillsElement").unwrap();
        let opening = entity(&ifc, fills[0]).refs("RelatingOpeningElement")[0];
        assert_eq!(entity(&ifc, opening).class, "IfcOpeningElement");
        let void = ifc.referenced_by(opening, "IfcRelVoidsElement").unwrap();
        assert_eq!(
            entity(&ifc, void[0]).refs("RelatingBuildingElement"),
            [wall_step]
        );
        // Contained in the level and typed.
        assert_eq!(
            ifc.referenced_by(e, "IfcRelContainedInSpatialStructure")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            ifc.referenced_by(e, "IfcRelDefinesByType").unwrap().len(),
            1
        );
    }
    let door_e = entity(&ifc, ifc.entities_of_type("IfcDoor").unwrap()[0].id);
    assert_eq!(
        door_e.get("OperationType"),
        &AttrValue::Enum("SINGLE_SWING_LEFT".into())
    );

    // The geometry engine cuts the wall with the openings, and sees the door and window.
    let meshes = ifc.tessellate(1).unwrap();
    let wall_mesh = meshes.iter().find(|m| m.class == "IfcWall").unwrap();
    let verts: Vec<[f64; 3]> = wall_mesh
        .verts
        .chunks(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let expected = 6.0 * 3.0 * 0.2 - 0.2 * (0.9 * 2.1 + 1.2 * 1.2);
    assert!(
        (volume(&verts, &wall_mesh.faces).abs() - expected).abs() < 1e-3,
        "{}",
        volume(&verts, &wall_mesh.faces)
    );
    for class in ["IfcDoor", "IfcWindow"] {
        let m = meshes
            .iter()
            .find(|m| m.class == class)
            .expect("has geometry");
        let xs: Vec<f64> = m.verts.chunks(3).map(|c| c[0]).collect();
        let (lo, hi) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        let centre = if class == "IfcDoor" { 1.0 } else { 4.0 };
        assert!(((lo + hi) / 2.0 - centre).abs() < 1e-6, "{class} {lo} {hi}");
    }

    // Saving again changes nothing; reopened, the inserts come back.
    let count = ifc.entity_count();
    project.save(&doc).unwrap();
    assert_eq!(IfcModel::open(&path).unwrap().entity_count(), count);
    let (mut project, mut reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(placed(&reopened, door), placed(&doc, door));
    assert_eq!(placed(&reopened, win), placed(&doc, win));

    // Deleting the door removes its opening and relationships from the file.
    run(
        &r,
        &mut reopened,
        "element.delete",
        json!({ "element_ids": [door.0] }),
    )
    .unwrap();
    project.save(&reopened).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcDoor").unwrap().is_empty());
    assert_eq!(ifc.entities_of_type("IfcOpeningElement").unwrap().len(), 1);
    assert_eq!(ifc.entities_of_type("IfcRelVoidsElement").unwrap().len(), 1);
    assert_eq!(ifc.entities_of_type("IfcRelFillsElement").unwrap().len(), 1);
}

/// Windows fit their wall where they are placed: they slide off the wall's ends, shrink to a
/// short wall or the gap between neighbours, and on a dormer rise onto its front wall's open
/// face and shrink under the dormer roof.
#[test]
fn windows_fit_their_wall() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id()
        .0;
    // A 1 m wall: the 1.2 m default window shrinks to it.
    let short = wall(&r, &mut doc, 1.0);
    let win = place(&r, &mut doc, "window.place", short, 0.5);
    let p = placed(&doc, win);
    assert!((p.ty.width - 1.0).abs() < 1e-9, "{}", p.ty.width);
    assert_eq!(p.recipe.width, Some(p.ty.width));
    // A window clicked near a wall's end slides inside it, at the type's size.
    let long = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 5], "end": [3, 5] } },
            "base_level": level,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    let a = place(&r, &mut doc, "window.place", long, 0.1);
    let pa = placed(&doc, a);
    assert!((pa.recipe.along - 0.6).abs() < 1e-9 && pa.recipe.width.is_none());
    // The next one fits where it is clicked, in the 1.8 m left beside it.
    let b = place(&r, &mut doc, "window.place", long, 2.0);
    let pb = placed(&doc, b);
    assert!((pb.cut().x[0] - 1.4).abs() < 1e-9, "{:?}", pb.cut());
    assert!(pb.recipe.width.is_none());
    let err = run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": long.0, "point_along": 2.0, "fit": false }),
    )
    .unwrap_err();
    assert_eq!(err.code, "insert.overlaps");
    // Clicked on another window: no room there.
    let err = run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": long.0, "point_along": 0.6 }),
    )
    .unwrap_err();
    assert_eq!(err.code, "insert.no_room");

    // Resized by command: its own size, centred; null returns to the type's.
    run(
        &r,
        &mut doc,
        "insert.set_size",
        json!({ "element_ids": [b.0], "width": 0.8, "height": 0.6 }),
    )
    .unwrap();
    let pb2 = placed(&doc, b);
    assert!((pb2.ty.width - 0.8).abs() < 1e-9 && (pb2.ty.height - 0.6).abs() < 1e-9);
    assert!((pb2.recipe.along - pb.recipe.along).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "insert.set_size",
        json!({ "element_ids": [b.0], "width": null, "height": null }),
    )
    .unwrap();
    assert_eq!(placed(&doc, b).recipe.width, None);
    // Too wide to fit beside its neighbour without `fit`: refused; with it, shrunk.
    let err = run(
        &r,
        &mut doc,
        "insert.set_size",
        json!({ "element_ids": [b.0], "width": 2.5 }),
    )
    .unwrap_err();
    assert!(err.code == "insert.overlaps" || err.code == "insert.outside_host");
    run(
        &r,
        &mut doc,
        "insert.set_size",
        json!({ "element_ids": [b.0], "width": 2.5, "fit": true }),
    )
    .unwrap();
    let pb3 = placed(&doc, b);
    assert!((pb3.ty.width - 1.8).abs() < 1e-9, "{}", pb3.ty.width);
}
