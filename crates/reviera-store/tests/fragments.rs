// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Components that are model fragments (DM-LIB-003): Create Component from a selection (with
//! what it hosts and depends on) into a Component Library, and placing it, turned, twice, with
//! new GlobalIds and reused types; levels mapped by height.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

fn count(doc: &Document, f: &dyn Fn(&reviera_core::Element) -> bool) -> usize {
    doc.store().iter().filter(|e| f(e)).count()
}

fn wall(
    r: &CommandRegistry,
    doc: &mut Document,
    level: ElementId,
    a: [f64; 2],
    b: [f64; 2],
) -> ElementId {
    id(&run(
        r,
        doc,
        "wall.create",
        json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap())
}

#[test]
fn a_room_as_a_component_placed_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("a.ifc")).unwrap();
    let r = registry();
    run(
        &r,
        &mut doc,
        "library.create",
        json!({ "path": lib, "name": "Rooms" }),
    )
    .unwrap();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A 3 × 2 m room: four walls, a door in the first, a floor.
    let c = [[0.0, 0.0], [3.0, 0.0], [3.0, 2.0], [0.0, 2.0]];
    let walls: Vec<ElementId> = (0..4)
        .map(|i| wall(&r, &mut doc, l1, c[(i + 1) % 4], c[i]))
        .collect();
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": walls[0].0, "point_along": 1.5 }),
    )
    .unwrap();
    let edge = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
    run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[edge(c[0], c[1]), edge(c[1], c[2]), edge(c[2], c[3]), edge(c[3], c[0])]], "level_id": l1.0 }),
    )
    .unwrap();
    let doors = |d: &Document| count(d, &|e| e.class == "IfcDoor");
    let walls_n = |d: &Document| count(d, &|e| reviera_core::walls::wall_recipe(e).is_some());
    assert_eq!((walls_n(&doc), doors(&doc)), (4, 1));

    // Create Component from the walls and the floor: the door (hosted) comes along, with the
    // types and the level; the model is unchanged.
    let mut ids: Vec<u64> = walls.iter().map(|w| w.0).collect();
    ids.push(
        doc.store()
            .iter()
            .find(|e| reviera_core::floors::floor_recipe(e).is_some())
            .unwrap()
            .id()
            .0,
    );
    let made = run(
        &r,
        &mut doc,
        "component.create",
        json!({ "element_ids": ids, "name": "Box Room", "library": lib, "folder": "Rooms" }),
    )
    .unwrap();
    assert_eq!(made["item"], "Rooms/Box Room.ifc");
    let brought: Vec<String> = made["brought_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert!(
        brought.iter().any(|b| b.starts_with("IfcDoor")),
        "{brought:?}"
    );
    assert!(
        brought.iter().any(|b| b.starts_with("IfcBuildingStorey")),
        "{brought:?}"
    );
    assert!(lib.join("Rooms/Box Room.ifc").is_file());
    let meta = run(
        &r,
        &mut doc,
        "library.get_meta",
        json!({ "library": lib, "item": "Rooms/Box Room.ifc" }),
    )
    .unwrap();
    assert_eq!(meta["info"]["kind"], "fragment");
    assert_eq!(meta["info"]["name"], "Box Room");
    // The file opens on its own: four walls, a door, a floor; its base point (the room's
    // bottom centre) at the origin.
    let (_fp, fdoc, _) = LocalProject::open(&lib.join("Rooms/Box Room.ifc")).unwrap();
    assert_eq!((walls_n(&fdoc), doors(&fdoc)), (4, 1));
    let xs: Vec<f64> = fdoc
        .store()
        .iter()
        .filter_map(reviera_core::walls::wall_recipe)
        .flat_map(|w| [w.curve.start[0], w.curve.end[0]])
        .collect();
    assert!(
        (xs.iter().cloned().fold(f64::INFINITY, f64::min) + 1.5).abs() < 0.2,
        "{xs:?}"
    );

    // Placed twice into a new model: once at (10, 0), once at (20, 0) turned 90°.
    let (_p2, mut doc2) = LocalProject::new(Schema::Ifc4, &tmp.path().join("b.ifc")).unwrap();
    let m1 = doc2
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // The model's own door type, to be reused by name.
    let a = run(&r, &mut doc2, "component.place_fragment", json!({ "library": lib, "item": "Rooms/Box Room.ifc", "point": [10.0, 0.0], "level": m1.0 })).unwrap();
    let b = run(&r, &mut doc2, "component.place_fragment", json!({ "library": lib, "item": "Rooms/Box Room.ifc", "point": [20.0, 0.0], "level": m1.0, "rotation": "90°" })).unwrap();
    assert_eq!((walls_n(&doc2), doors(&doc2)), (8, 2));
    assert!(!a.as_array().unwrap().is_empty() && !b.as_array().unwrap().is_empty());
    // One level (mapped onto Level 1), one door type, one wall type: types reused.
    assert_eq!(count(&doc2, &|e| e.class == "IfcBuildingStorey"), 1);
    assert_eq!(count(&doc2, &|e| e.class == "IfcDoorType"), 1);
    // Every GlobalId is new and unique; every wall regenerates where it should.
    let guids: std::collections::HashSet<String> =
        doc2.store().iter().map(|e| e.guid().to_string()).collect();
    assert_eq!(guids.len(), doc2.store().iter().count());
    let near = |x: f64, y: f64| {
        doc2.store()
            .iter()
            .filter_map(reviera_core::walls::wall_recipe)
            .any(|w| {
                [w.curve.start, w.curve.end]
                    .iter()
                    .any(|p| (p[0] - x).abs() < 0.05 && (p[1] - y).abs() < 0.05)
            })
    };
    assert!(
        near(8.5, -1.0) && near(11.5, 1.0),
        "the first room round (10, 0)"
    );
    assert!(
        near(21.0, -1.5) && near(19.0, 1.5),
        "the second turned 90° round (20, 0)"
    );
    let bodies = reviera_core::bodies::all(doc2.store());
    assert!(
        bodies
            .iter()
            .filter(|b| b.parts.iter().any(|p| p.class.starts_with("IfcDoor")))
            .count()
            >= 2
    );
    assert_eq!(reviera_core::floors::all(doc2.store()).len(), 2);
}

#[test]
fn levels_map_by_height() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("a.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(&run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    // A wall on each level: a two-level fragment.
    let a = wall(&r, &mut doc, l1, [0.0, 0.0], [4.0, 0.0]);
    let b = wall(&r, &mut doc, l2, [0.0, 0.0], [4.0, 0.0]);
    let path = tmp.path().join("core.ifc");
    run(&r, &mut doc, "component.create", json!({ "element_ids": [a.0, b.0], "name": "Core", "path": path, "base_point": [0.0, 0.0] })).unwrap();
    // Into a model with levels at 0, 3 and 6, on Level 2: its upper wall goes to Level 3.
    let (_p2, mut doc2) = LocalProject::new(Schema::Ifc4, &tmp.path().join("b.ifc")).unwrap();
    let _ = run(&r, &mut doc2, "level.create", json!({ "elevation": 3.0 })).unwrap();
    let m2 = doc2
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey" && reviera_core::levels::elevation(e) == 3.0)
        .unwrap()
        .id();
    let m3 = id(&run(&r, &mut doc2, "level.create", json!({ "elevation": 6.0 })).unwrap());
    run(
        &r,
        &mut doc2,
        "component.place_fragment",
        json!({ "path": path, "point": [0.0, 0.0], "level": m2.0 }),
    )
    .unwrap();
    let levels_used: Vec<String> = doc2
        .store()
        .iter()
        .filter_map(reviera_core::walls::wall_recipe)
        .map(|w| w.base_level)
        .collect();
    let g = |i: ElementId| doc2.store().get(i).unwrap().guid().to_string();
    assert!(
        levels_used.contains(&g(m2)) && levels_used.contains(&g(m3)),
        "{levels_used:?}"
    );
    assert_eq!(count(&doc2, &|e| e.class == "IfcBuildingStorey"), 3);
}
