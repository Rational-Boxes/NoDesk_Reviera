// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The volume overlap engine over a model (ARC-MOD-030): a well-built room has none (joined
//! corners, a door in its wall, a floor and an attached roof touch but do not overlap), and
//! real overlaps are found with their volume.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn pairs(v: &Value) -> Vec<(u64, u64, f64)> {
    v["overlaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["a"].as_u64().unwrap(),
                o["b"].as_u64().unwrap(),
                o["volume"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_room_has_no_overlaps_until_something_crosses_it() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let rect = [[0.0, 0.0], [8.0, 0.0], [8.0, 5.0], [0.0, 5.0]];
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(
            run(
                &mut app,
                "wall.create",
                json!({
                    "curve": { "line": { "start": rect[(i + 1) % 4], "end": rect[i] } },
                    "base_level": level, "top_constraint": { "unconnected": 3 },
                }),
            )
            .unwrap()
            .as_u64()
            .unwrap(),
        );
    }
    run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": walls[0], "point_along": 4.0 }),
    )
    .unwrap();
    // A floor under the walls' inside faces, its top at the level.
    let inner = [[0.1, 0.1], [7.9, 0.1], [7.9, 4.9], [0.1, 4.9]];
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": inner[i], "end": inner[(i + 1) % 4] } }))
        .collect();
    run(
        &mut app,
        "floor.create",
        json!({ "loops": [curves], "level_id": level }),
    )
    .unwrap();
    // A gable roof on the walls' exterior faces, the walls attached.
    let outer = [[-0.1, -0.1], [8.1, -0.1], [8.1, 5.1], [-0.1, 5.1]];
    let curves: Vec<Value> = (0..4)
        .map(|i| {
            json!({ "line": { "start": outer[i], "end": outer[(i + 1) % 4] },
                    "defines_slope": i % 2 == 0, "overhang": 0.4 })
        })
        .collect();
    let roof = run(
        &mut app,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": level, "base_offset": 3.0 }),
    )
    .unwrap();
    run(&mut app, "roof.attach_walls", json!({ "roof_id": roof })).unwrap();

    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    assert_eq!(pairs(&found), vec![], "{found:#}");
    assert!(found["bodies"].as_u64().unwrap() >= 7);

    // A wall run through the room's side, and a railing through the opposite wall.
    let cross = run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [3.0, 5.5], "end": [3.0, 3.0] } },
            "base_level": level, "top_constraint": { "unconnected": 2 },
        }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let rail = run(
        &mut app,
        "railing.create",
        json!({ "path": [[6.0, -1.0], [6.0, 1.0]], "base_level": level }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let found = run(&mut app, "overlap.find", json!({})).unwrap();
    let p = pairs(&found);
    let has = |x: u64, y: u64| p.iter().any(|(a, b, _)| (*a, *b) == (x.min(y), x.max(y)));
    assert!(has(cross, walls[2]), "{found:#}");
    assert!(has(rail, walls[0]), "{found:#}");
    // The crossing wall (200 mm) through the 200 mm wall, 2 m high: 0.2 × 0.2 × 2 = 0.08 m³,
    // less where the two may have joined; and the crossing wall over the floor touches only.
    let v = p
        .iter()
        .find(|(a, b, _)| (*a, *b) == (cross.min(walls[2]), cross.max(walls[2])))
        .unwrap()
        .2;
    assert!(v > 0.01 && v < 0.081, "{v}");
    // Checking just the railing: only its overlap.
    let one = run(&mut app, "overlap.find", json!({ "element_ids": [rail] })).unwrap();
    assert_eq!(pairs(&one).len(), 1, "{one:#}");
    // A door and its wall belong together, unless asked.
    let all = run(&mut app, "overlap.find", json!({ "include_related": true })).unwrap();
    assert!(pairs(&all).len() >= pairs(&found).len());
}

/// Resolving overlaps (ARC-MOD-032): accepting one takes it out of the checks; moving apart
/// separates the pair in one undo step; the boolean modes wait for the kernel.
#[test]
fn overlaps_are_accepted_or_moved_apart() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let wall = |app: &mut App, a: [f64; 2], b: [f64; 2]| {
        run(
            app,
            "wall.create",
            json!({ "curve": { "line": { "start": a, "end": b } },
                    "base_level": level, "top_constraint": { "unconnected": 3 } }),
        )
        .unwrap()
        .as_u64()
        .unwrap()
    };
    let long = wall(&mut app, [0.0, 0.0], [8.0, 0.0]);
    let cross = wall(&mut app, [3.0, 0.6], [3.0, -2.0]);
    let rail = run(
        &mut app,
        "railing.create",
        json!({ "path": [[6.0, -1.0], [6.0, 1.0]], "base_level": level }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    assert_eq!(
        pairs(&run(&mut app, "overlap.find", json!({})).unwrap()).len(),
        2
    );

    // Accept the crossing wall: no longer reported, unless asked; and listed.
    run(
        &mut app,
        "overlap.resolve",
        json!({ "a": long, "b": cross, "mode": "accept", "note": "a buttress" }),
    )
    .unwrap();
    let left = pairs(&run(&mut app, "overlap.find", json!({})).unwrap());
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(
        pairs(
            &run(
                &mut app,
                "overlap.find",
                json!({ "include_accepted": true })
            )
            .unwrap()
        )
        .len(),
        2
    );
    // Still accepted after both move.
    for (id, v) in [(cross, [0.5, 0.0, 0.0]), (long, [0.0, 0.05, 0.0])] {
        run(
            &mut app,
            "element.move",
            json!({ "element_ids": [id], "vector": v }),
        )
        .unwrap();
    }
    let left = pairs(&run(&mut app, "overlap.find", json!({})).unwrap());
    assert!(
        !left
            .iter()
            .any(|(a, b, _)| (*a, *b) == (long.min(cross), long.max(cross))),
        "{left:?}"
    );
    let accepted = run(&mut app, "overlap.accepted", json!({})).unwrap();
    assert_eq!(accepted[0]["note"], "a buttress");
    assert!(accepted[0]["at"].as_str().unwrap().ends_with('Z'));
    run(
        &mut app,
        "overlap.accept",
        json!({ "a": cross, "b": long, "accepted": false }),
    )
    .unwrap();
    assert_eq!(
        pairs(&run(&mut app, "overlap.find", json!({})).unwrap()).len(),
        2
    );

    // Move the railing out of the wall: the shorter way is across it.
    let out = run(
        &mut app,
        "overlap.resolve",
        json!({ "a": long, "b": rail, "mode": "move_apart" }),
    )
    .unwrap();
    let moved = out["moved"].as_array().unwrap();
    assert_eq!(moved[0], 0.0);
    assert!(
        (moved[1].as_f64().unwrap().abs() - 1.0).abs() < 0.2,
        "{out}"
    );
    let now = pairs(&run(&mut app, "overlap.find", json!({})).unwrap());
    assert!(
        !now.iter().any(|(a, b, _)| *a == rail || *b == rail),
        "{now:?}"
    );
    // One undo puts it back.
    run(&mut app, "edit.undo", json!({})).unwrap();
    assert_eq!(
        pairs(&run(&mut app, "overlap.find", json!({})).unwrap()).len(),
        2
    );

    // Cut the long wall by the crossing one (ARC-MOD-028): it loses the shared volume, and the
    // pair is no longer an overlap.
    let volume = |app: &App, id: u64| -> f64 {
        app.bodies()
            .unwrap()
            .iter()
            .find(|b| b.element.0 == id)
            .map_or(0.0, |b| {
                b.parts
                    .iter()
                    .map(|p| {
                        let (pts, idx) = &p.mesh;
                        idx.chunks(3)
                            .map(|t| {
                                let (a, b, c) =
                                    (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
                                (a[0] * (b[1] * c[2] - b[2] * c[1])
                                    - a[1] * (b[0] * c[2] - b[2] * c[0])
                                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                                    / 6.0
                            })
                            .sum::<f64>()
                    })
                    .sum()
            })
    };
    let before = volume(&app, long);
    run(
        &mut app,
        "overlap.resolve",
        json!({ "a": long, "b": cross, "mode": "cut_a" }),
    )
    .unwrap();
    let after = volume(&app, long);
    assert!(
        (before - after - 0.2 * 0.2 * 3.0).abs() < 1e-6,
        "{before} → {after}"
    );
    let left = pairs(&run(&mut app, "overlap.find", json!({})).unwrap());
    assert!(
        !left
            .iter()
            .any(|(a, b, _)| (*a, *b) == (long.min(cross), long.max(cross))),
        "{left:?}"
    );
    // Saved, the file's body is the cut result (tessellated by IfcOpenShell).
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cut.ifc");
        run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
        let guid = {
            let d = app.document(app.active().unwrap()).unwrap();
            d.doc
                .store()
                .get(reviera_core::ElementId(long))
                .unwrap()
                .guid()
                .to_string()
        };
        let ifc = reviera_ifc::IfcModel::open(&path).unwrap();
        let mesh = ifc
            .tessellate(1)
            .unwrap()
            .into_iter()
            .find(|m| m.global_id.as_ref().is_some_and(|g| g.as_str() == guid))
            .expect("the wall's body");
        let p = |k: u32| {
            let k = 3 * k as usize;
            [mesh.verts[k], mesh.verts[k + 1], mesh.verts[k + 2]]
        };
        let v: f64 = mesh
            .faces
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum();
        assert!((v.abs() - after).abs() < 1e-3, "{v} in the file vs {after}");
    }
    // The cut follows the crossing wall when it moves.
    run(
        &mut app,
        "element.move",
        json!({ "element_ids": [cross], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert!((volume(&app, long) - after).abs() < 1e-6);
    // Removing the boolean restores the wall.
    run(
        &mut app,
        "geometry.remove_boolean",
        json!({ "target_id": long }),
    )
    .unwrap();
    assert!((volume(&app, long) - before).abs() < 1e-6);
    // A union fuses the tool in and hides its own body.
    run(
        &mut app,
        "geometry.boolean",
        json!({ "target_id": long, "tool_ids": [cross], "op": "union" }),
    )
    .unwrap();
    assert!(app.bodies().unwrap().iter().all(|b| b.element.0 != cross));
    let fused = volume(&app, long);
    let cross_volume = 0.2 * 2.6 * 3.0;
    assert!(
        (fused - (before + cross_volume - 0.12)).abs() < 1e-6,
        "{fused}"
    );
}
