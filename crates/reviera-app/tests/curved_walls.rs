// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curved walls (ARC-WALL-002): a round room of two arc walls with a door on the curve, a room
//! bounded by them, no overlap where their ends butt, and the walls saved and reopened.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

#[test]
fn a_round_room_of_curved_walls() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: dir.path().to_owned(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    // Two semicircles of radius 5 (centreline), drawn clockwise round the room so their
    // exterior faces are outside.
    let arc = |app: &mut App, a: [f64; 2], m: [f64; 2], b: [f64; 2]| {
        run(
            app,
            "wall.create",
            json!({
                "curve": { "line": { "start": a, "end": b }, "through": [m] },
                "base_level": level, "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()
        .as_u64()
        .unwrap()
    };
    let north = arc(&mut app, [-5.0, 0.0], [0.0, 5.0], [5.0, 0.0]);
    let south = arc(&mut app, [5.0, 0.0], [0.0, -5.0], [-5.0, 0.0]);
    // A door at the top of the north arc, by a plan point beside it.
    let door = run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": north, "point": [0.0, 5.3] }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    {
        let d = app.document(app.active().unwrap()).unwrap();
        let store = d.doc.store();
        let p = reviera_core::inserts::resolve(
            store,
            store.get(reviera_core::ElementId(door)).unwrap(),
        )
        .unwrap();
        // Halfway along the arc (5π / 2), centred on the curve at the top.
        assert!(
            (p.recipe.along - 5.0 * std::f64::consts::FRAC_PI_2).abs() < 0.01,
            "{}",
            p.recipe.along
        );
        let c = p.world([0.0, 0.0, 0.0]);
        assert!(c[0].abs() < 0.01 && (c[1] - 5.0).abs() < 0.01, "{c:?}");
        // Square to the curve: the frame's x runs east at the top.
        let x = p.world([1.0, 0.0, 0.0]);
        assert!((x[1] - c[1]).abs() < 0.01, "{x:?}");
    }
    // A room inside: about the circle of the inner faces (radius 4.9).
    let room = run(
        &mut app,
        "room.place",
        json!({ "level_id": level, "point": [0.0, 0.0] }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let area = {
        let d = app.document(app.active().unwrap()).unwrap();
        let store = d.doc.store();
        reviera_core::rooms::all(store)
            .into_iter()
            .find(|r| r.id.0 == room)
            .unwrap()
            .geometry
            .area()
    };
    let exact = std::f64::consts::PI * 4.9 * 4.9;
    assert!((area - exact).abs() / exact < 0.005, "{area} vs {exact}");
    // The ends butt: no overlap; the door is cut into the wall, not overlapping it.
    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    assert_eq!(found["overlaps"], json!([]), "{found:#}");
    // Mirrored, the curve goes with the wall.
    run(
        &mut app,
        "element.mirror",
        json!({ "element_ids": [south], "axis": { "start": [-10.0, 0.0], "end": [10.0, 0.0] },
                "copy": false }),
    )
    .unwrap();
    {
        let d = app.document(app.active().unwrap()).unwrap();
        let r = reviera_core::walls::wall_recipe(
            d.doc.store().get(reviera_core::ElementId(south)).unwrap(),
        )
        .unwrap();
        assert!((r.through[0][1] - 5.0).abs() < 1e-9, "{:?}", r.through);
    }
    // Saved and reopened: the walls keep their curves, written as B-reps.
    let path = dir.path().join("round.ifc");
    run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
    let ifc = reviera_ifc::IfcModel::open(&path).unwrap();
    assert_eq!(ifc.entities_of_type("IfcWall").unwrap().len(), 2);
    assert!(!ifc.entities_of_type("IfcFacetedBrep").unwrap().is_empty());
    let mut again = App::new(AppConfig {
        live: None,
        untitled_dir: dir.path().to_owned(),
    });
    run(&mut again, "file.open", json!({ "path": path })).unwrap();
    let d = again.document(again.active().unwrap()).unwrap();
    let curved = d
        .doc
        .store()
        .iter()
        .filter(|e| reviera_core::walls::wall_recipe(e).is_some_and(|r| r.through.len() == 1))
        .count();
    assert_eq!(curved, 2);
}
