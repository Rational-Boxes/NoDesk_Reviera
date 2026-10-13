// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rooms: placement, boundaries from wall faces, islands, automatic placement, warnings and the
//! saved IfcSpace (spec Ch 3 §3.9.1–3.9.2, ARC-ROOM-001/002).

use std::path::{Path, PathBuf};

use reviera_core::rooms;
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
    let path = dir.join("rooms.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (project, doc, path, level)
}

fn wall(
    r: &CommandRegistry,
    doc: &mut Document,
    level: ElementId,
    a: [f64; 2],
    b: [f64; 2],
) -> ElementId {
    id(run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": a, "end": b } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap())
}

/// Walls on the centrelines of a 5.2 × 4.2 rectangle, 200 mm thick: 5 × 4 m inside.
fn house(r: &CommandRegistry, doc: &mut Document, level: ElementId) -> Vec<ElementId> {
    let c = [[-0.1, -0.1], [5.1, -0.1], [5.1, 4.1], [-0.1, 4.1]];
    (0..4)
        .map(|i| wall(r, doc, level, c[i], c[(i + 1) % 4]))
        .collect()
}

fn geometry(doc: &Document, room: ElementId) -> rooms::RoomGeometry {
    rooms::regenerate(doc.store(), doc.store().get(room).unwrap()).unwrap()
}

#[test]
fn a_room_inside_walls_measures_to_their_finish_faces() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    house(&r, &mut doc, level);
    let room = id(run(
        &r,
        &mut doc,
        "room.place",
        json!({ "level_id": level.0, "point": [2.0, 2.0] }),
    )
    .unwrap());
    let g = geometry(&doc, room);
    assert!(g.enclosed());
    assert!((g.area() - 20.0).abs() < 1e-9, "{}", g.area());
    assert!((g.perimeter() - 18.0).abs() < 1e-9);
    assert!((g.unbounded_height() - 4.0).abs() < 1e-12);
    assert!((g.volume() - 80.0).abs() < 1e-9);
    let e = doc.store().get(room).unwrap();
    assert_eq!(
        rooms::number_and_name(e),
        ("1".to_owned(), "Room".to_owned())
    );
    // The room follows its walls: moving the east wall away opens the corners, and the room is
    // no longer enclosed; undoing the move closes it again.
    let east = doc
        .store()
        .iter()
        .filter(|e| reviera_core::walls::wall_recipe(e).is_some())
        .map(|e| e.id())
        .nth(1)
        .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [east.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert!(!geometry(&doc, room).enclosed());
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert!((geometry(&doc, room).area() - 20.0).abs() < 1e-9);
    // A point inside a wall is refused.
    let e = run(
        &r,
        &mut doc,
        "room.place",
        json!({ "level_id": level.0, "point": [-0.05, 2.0] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "room.inside_element");
}

#[test]
fn doors_do_not_join_rooms_and_islands_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _, level) = project(dir.path());
    let r = registry();
    house(&r, &mut doc, level);
    // A partition at x = 2.5 with a door in it.
    let partition = wall(&r, &mut doc, level, [2.5, -0.1], [2.5, 4.1]);
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": partition.0, "point_along": 2.0 }),
    )
    .unwrap();
    let placed = run(
        &r,
        &mut doc,
        "room.place_automatically",
        json!({ "level_id": level.0 }),
    )
    .unwrap();
    let ids: Vec<ElementId> = serde_json::from_value(placed).unwrap();
    assert_eq!(ids.len(), 2, "one room each side of the door");
    let areas: Vec<f64> = ids.iter().map(|i| geometry(&doc, *i).area()).collect();
    assert!(
        areas.iter().all(|a| (a - 2.4 * 4.0).abs() < 1e-9),
        "{areas:?}"
    );
    let numbers: Vec<String> = ids
        .iter()
        .map(|i| rooms::number_and_name(doc.store().get(*i).unwrap()).0)
        .collect();
    assert_eq!(numbers, ["1", "2"]);
    // Again: every region has a room, so nothing more is placed.
    let again = run(
        &r,
        &mut doc,
        "room.place_automatically",
        json!({ "level_id": level.0 }),
    )
    .unwrap();
    assert_eq!(again.as_array().unwrap().len(), 0);
    // A free-standing 300 mm "column" of wall in the west room: an island.
    wall(&r, &mut doc, level, [1.0, 2.0], [1.3, 2.0]);
    let west = ids
        .iter()
        .copied()
        .find(|i| geometry(&doc, *i).tag_point[0] < 2.5)
        .unwrap();
    let g = geometry(&doc, west);
    assert!(
        (g.area() - (2.4 * 4.0 - 0.3 * 0.2)).abs() < 1e-9,
        "{}",
        g.area()
    );
    assert!((g.perimeter() - (2.0 * (2.4 + 4.0) + 2.0 * (0.3 + 0.2))).abs() < 1e-9);
    // A second room in the same region is redundant, with a warning.
    let out = r
        .execute(
            &mut doc,
            "room.place",
            json!({ "level_id": level.0, "point": [0.5, 0.5] }),
            &Caller::user(),
        )
        .unwrap();
    assert!(
        out.warnings.iter().any(|w| w.id == "arc.room.redundant"),
        "{:?}",
        out.warnings
    );
    // Outside the house: not enclosed.
    let out = r
        .execute(
            &mut doc,
            "room.place",
            json!({ "level_id": level.0, "point": [20.0, 20.0] }),
            &Caller::user(),
        )
        .unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "arc.room.not_enclosed"));
}

#[test]
fn rooms_are_saved_as_spaces_under_their_storey() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path, level) = project(dir.path());
    let r = registry();
    house(&r, &mut doc, level);
    let room = id(run(
        &r,
        &mut doc,
        "room.place",
        json!({ "level_id": level.0, "point": [2.0, 2.0], "name": "Kitchen", "number": "101", "limit_offset": "2700 mm" }),
    )
    .unwrap());
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let spaces = ifc.entities_of_type("IfcSpace").unwrap();
    assert_eq!(spaces.len(), 1);
    let s = ifc.attributes_many(&[spaces[0].id]).unwrap().remove(0);
    assert_eq!(s.string("Name"), Some("101"));
    assert_eq!(s.string("LongName"), Some("Kitchen"));
    assert_eq!(
        ifc.referenced_by(spaces[0].id, "IfcRelAggregates")
            .unwrap()
            .len(),
        1
    );
    assert!(
        ifc.referenced_by(spaces[0].id, "IfcRelContainedInSpatialStructure")
            .unwrap()
            .is_empty()
    );
    let meshes = ifc.tessellate(1).unwrap();
    let m = meshes.iter().find(|m| m.class == "IfcSpace").unwrap();
    let top = m.verts.chunks(3).map(|c| c[2]).fold(f64::MIN, f64::max);
    assert!((top - 2.7).abs() < 1e-9, "{top}");
    // Reopened, the room is the same; renaming it updates the file.
    let (mut project, mut reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(geometry(&reopened, room), geometry(&doc, room));
    run(
        &r,
        &mut reopened,
        "room.set",
        json!({ "room_ids": [room.0], "name": "Dining" }),
    )
    .unwrap();
    project.save(&reopened).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let s = ifc.attributes_many(&[spaces[0].id]).unwrap().remove(0);
    assert_eq!(s.string("LongName"), Some("Dining"));
    // Deleting the level deletes its rooms.
    let deleted = run(
        &r,
        &mut reopened,
        "element.delete",
        json!({ "element_ids": [level.0] }),
    )
    .unwrap();
    assert!(deleted.as_array().unwrap().iter().any(|v| v == room.0));
}
