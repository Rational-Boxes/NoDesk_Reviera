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
    // In plan (cut at 1.2 m), the curved walls' cut edges lie on their faces' true radii
    // (4.9 and 5.1 m, within the chord of a 3° segment), all the way round but the door.
    {
        let scene = app.render_scene(app.active().unwrap()).unwrap();
        let edges = reviera_render::plan::feature_edges(&scene);
        let g = reviera_render::plan::plan_graphics(&scene, &edges, 1.2, 0.0, 100.0, &|_| {
            reviera_render::plan::Weights {
                cut: Some(0.5),
                projection: 0.25,
            }
        });
        let walls = [north, south];
        let cut: Vec<[[f32; 2]; 2]> = g
            .cut_edges
            .iter()
            .filter(|(e, _)| e.is_some_and(|e| walls.contains(&e.0)))
            .map(|(_, s)| *s)
            .collect();
        assert!(cut.len() > 100, "{}", cut.len());
        let sagitta = 5.1 * (1.0 - (1.5f64.to_radians()).cos());
        let (mut inner, mut outer) = (0, 0);
        for s in &cut {
            for p in s {
                let r = f64::from(p[0]).hypot(f64::from(p[1]));
                assert!(
                    r > 4.9 - sagitta - 1e-3 && r < 5.1 + 1e-3,
                    "{r} off the wall's faces"
                );
                if (r - 4.9).abs() < 2e-3 {
                    inner += 1;
                }
                if (r - 5.1).abs() < 2e-3 {
                    outer += 1;
                }
            }
        }
        assert!(inner > 50 && outer > 50, "{inner} {outer}");
        // The door's opening (at the top, about 0.8 m wide) is open in plan: no face edge there.
        assert!(
            !cut.iter()
                .any(|s| s.iter().all(|p| p[0].abs() < 0.3 && p[1] > 4.8))
        );
    }
    // The ends butt: no overlap; the door is cut into the wall, not overlapping it.
    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    assert_eq!(found["overlaps"], json!([]), "{found:#}");
    // A dome on the drum (overhanging its face), the walls attached: their tops follow its
    // underside, rising from √(5.4² − 5.1²) at the outer face to √(5.4² − 4.9²) at the inner.
    let dome = run(
        &mut app,
        "roof.create_dome",
        json!({ "level_id": level, "base_offset": 3.0, "center": [0.0, 0.0],
                "plan": { "circle": { "radius": 5.4 } } }),
    )
    .unwrap();
    let attached = run(&mut app, "roof.attach_walls", json!({ "roof_id": dome })).unwrap();
    let mut got: Vec<u64> = attached
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    got.sort();
    assert_eq!(got, vec![north, south]);
    let top = app
        .bodies()
        .unwrap()
        .iter()
        .filter(|b| b.element.0 == north)
        .flat_map(|b| b.parts.iter().flat_map(|p| p.mesh.0.iter().map(|q| q[2])))
        .fold(f64::MIN, f64::max);
    let inner = 3.0 + (5.4f64 * 5.4 - 4.9 * 4.9).sqrt();
    assert!((top - inner).abs() < 0.08, "{top} vs {inner}");
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

/// A curved wall meeting a straight wall at an angle joins it with a mitre (ARC-JOIN-001 through
/// a stand-in along the curve's end tangent): the corners meet and nothing overlaps.
#[test]
fn curved_walls_join_straight_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: dir.path().to_owned(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let straight = run(
        &mut app,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } },
                "base_level": level, "top_constraint": { "unconnected": 3 } }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    // An arc leaving the straight wall's end at an angle, bulging out.
    let arc = run(
        &mut app,
        "wall.create",
        json!({ "curve": { "line": { "start": [6.0, 0.0], "end": [9.0, 5.0] },
                           "through": [[8.6, 1.6]] },
                "base_level": level, "top_constraint": { "unconnected": 3 } }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let d = app.document(app.active().unwrap()).unwrap();
    let store = d.doc.store();
    let solved = reviera_core::walls::solve(store);
    let id = |n: u64| reviera_core::ElementId(n);
    // Joined: the join names the curved wall, and its start corners are the straight wall's end
    // corners.
    assert!(
        solved
            .joins
            .iter()
            .any(|j| (j.wall == id(straight) && j.other == id(arc))
                || (j.wall == id(arc) && j.other == id(straight))),
        "{:?}",
        solved.joins
    );
    let ends = solved
        .curved_ends
        .get(&(id(arc), 0))
        .copied()
        .expect("the arc's start joined");
    let f = solved.bodies[&id(straight)].footprint;
    for c in ends {
        assert!(
            [f[1], f[2]]
                .iter()
                .any(|q| (q[0] - c[0]).hypot(q[1] - c[1]) < 1e-9),
            "{c:?} not on {f:?}"
        );
    }
    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    assert_eq!(found["overlaps"], json!([]), "{found:#}");
}

#[test]
fn floors_follow_curved_wall_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: dir.path().to_owned(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
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
    // Picked inner faces (the −normal side of walls drawn clockwise), curved, locked.
    let face = |w: u64, a: [f64; 2], m: [f64; 2], b: [f64; 2]| {
        json!({
            "line": { "start": a, "end": b }, "through": [m],
            "lock": { "element": w, "sub": "face:int" },
        })
    };
    let floor = run(
        &mut app,
        "floor.create",
        json!({
            "level_id": level,
            "loops": [[
                face(north, [-4.9, 0.0], [0.0, 4.9], [4.9, 0.0]),
                face(south, [4.9, 0.0], [0.0, -4.9], [-4.9, 0.0]),
            ]],
        }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let ceiling = run(
        &mut app,
        "ceiling.create_auto",
        json!({ "level_id": level, "point": [0.0, 0.0] }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let area = |app: &App, id: u64| {
        let d = app.document(app.active().unwrap()).unwrap();
        reviera_core::floors::all(d.doc.store())
            .into_iter()
            .find(|(e, _)| e.0 == id)
            .map(|(_, g)| g.profiles.iter().map(|p| p.area()).sum::<f64>())
            .unwrap()
    };
    let disc = std::f64::consts::PI * 4.9 * 4.9;
    assert!(
        (area(&app, floor) - disc).abs() / disc < 2e-3,
        "{}",
        area(&app, floor)
    );
    // The automatic ceiling's faceted boundary keeps its pieces of the curved faces.
    assert!(
        (area(&app, ceiling) - disc).abs() / disc < 1e-2,
        "{}",
        area(&app, ceiling)
    );
    // The north wall bulges out to 6 m: the floor follows its face.
    run(
        &mut app,
        "wall.set_location",
        json!({
            "wall_id": north,
            "curve": { "line": { "start": [-5.0, 0.0], "end": [5.0, 0.0] }, "through": [[0.0, 6.0]] },
        }),
    )
    .unwrap();
    let after = area(&app, floor);
    assert!(after > disc + 3.0, "{after} vs {disc}");
}
