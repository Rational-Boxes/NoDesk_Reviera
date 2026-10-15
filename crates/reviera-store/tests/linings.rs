// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Walls by room and segment, Auto Join and hosted walls (ARC-WALL-014/015/016).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, walls};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

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

fn geometry(doc: &Document, w: ElementId) -> walls::WallGeometry {
    walls::regenerate(doc.store(), doc.store().get(w).unwrap()).unwrap()
}

fn setup() -> (
    tempfile::TempDir,
    LocalProject,
    Document,
    CommandRegistry,
    ElementId,
) {
    let tmp = tempfile::tempdir().unwrap();
    let (p, doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("l.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (tmp, p, doc, r, l1)
}

#[test]
fn a_hosted_lining_follows_and_is_cut_by_the_hosts_door() {
    let (_t, mut project, mut doc, r, l1) = setup();
    let core = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let made = run(
        &r,
        &mut doc,
        "wall.create_by_segment",
        json!({ "target_id": core.0, "side": "interior", "hosted": true }),
    )
    .unwrap();
    let lining = id(&made[0]);
    let (c, l) = (geometry(&doc, core), geometry(&doc, lining));
    // Against the interior face (−y): its axis 0.2 m from the core's.
    assert!(
        (l.start[1] + 0.2).abs() < 1e-9 && (l.end[1] + 0.2).abs() < 1e-9,
        "{l:?}"
    );
    assert!((l.length() - 10.0).abs() < 1e-9 && (l.height - c.height).abs() < 1e-9);
    // Hosted, so Auto Join is on: a door in the core cuts the lining too.
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": core.0, "point_along": 4.0 }),
    )
    .unwrap();
    let cuts = reviera_core::inserts::cuts_by_host(doc.store());
    let lc = cuts.get(&lining).expect("the lining is cut");
    assert_eq!(lc.len(), 1);
    let mid = (lc[0].x[0] + lc[0].x[1]) / 2.0;
    // The lining runs the same way as the core, so the door is 4 m along it too.
    assert!((mid - 4.0).abs() < 1e-6, "{:?}", lc[0]);
    // Rotating the host 15° turns the lining with it, still against its face.
    run(
        &r,
        &mut doc,
        "element.rotate",
        json!({ "element_ids": [core.0], "center": [0.0, 0.0], "angle": "15°" }),
    )
    .unwrap();
    let (c, l) = (geometry(&doc, core), geometry(&doc, lining));
    let cross = c.direction()[0] * l.direction()[1] - c.direction()[1] * l.direction()[0];
    assert!(cross.abs() < 1e-9);
    let gap = ((l.start[0] - c.start[0]) * c.normal()[0]
        + (l.start[1] - c.start[1]) * c.normal()[1])
        .abs();
    assert!((gap - 0.2).abs() < 1e-9, "{gap}");
    assert_eq!(
        run(
            &r,
            &mut doc,
            "wall.select_main_host",
            json!({ "wall_id": lining.0 })
        )
        .unwrap(),
        json!(core.0)
    );
    // Removing the relationship leaves it exactly in place.
    run(
        &r,
        &mut doc,
        "wall.remove_host",
        json!({ "wall_ids": [lining.0] }),
    )
    .unwrap();
    assert_eq!(geometry(&doc, lining), l);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [core.0], "vector": [0.0, 1.0, 0.0] }),
    )
    .unwrap();
    assert_eq!(geometry(&doc, lining), l);
    // A host cannot be hosted on its own hosted wall.
    run(
        &r,
        &mut doc,
        "wall.set_host",
        json!({ "wall_id": lining.0, "host_wall_id": core.0 }),
    )
    .unwrap();
    assert_eq!(
        run(
            &r,
            &mut doc,
            "wall.set_host",
            json!({ "wall_id": core.0, "host_wall_id": lining.0 })
        )
        .unwrap_err()
        .code,
        "wall.host_cycle"
    );
    project.save(&doc).unwrap();
}

#[test]
fn join_and_lock_moves_both() {
    let (_t, _p, mut doc, r, l1) = setup();
    let core = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    // A lining drawn against the exterior face with Auto Join & Lock.
    let lining = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.2], "end": [10.0, 0.2] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "auto_join": "join_and_lock" }),
    )
    .unwrap());
    let joins = walls::linings::geometry_joined(doc.store(), doc.store().get(core).unwrap());
    assert_eq!(joins, vec![lining]);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [core.0], "vector": [0.0, -1.0, 0.0] }),
    )
    .unwrap();
    assert!(
        (geometry(&doc, lining).start[1] + 0.8).abs() < 1e-6,
        "{:?}",
        geometry(&doc, lining)
    );
    // Unjoined, a door in the core no longer cuts the lining.
    run(
        &r,
        &mut doc,
        "geometry.unjoin",
        json!({ "element_ids": [core.0, lining.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": core.0, "point_along": 4.0 }),
    )
    .unwrap();
    assert!(!reviera_core::inserts::cuts_by_host(doc.store()).contains_key(&lining));
}

#[test]
fn a_room_is_lined_all_round() {
    let (_t, _p, mut doc, r, l1) = setup();
    let corners = [[0.0, 0.0], [8.0, 0.0], [8.0, 5.0], [0.0, 5.0]];
    for k in 0..4 {
        run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "curve": { "line": { "start": corners[k], "end": corners[(k + 1) % 4] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
        )
        .unwrap();
    }
    let room = id(&run(
        &r,
        &mut doc,
        "room.place",
        json!({ "level_id": l1.0, "point": [4.0, 2.5] }),
    )
    .unwrap());
    let made: Vec<ElementId> = run(
        &r,
        &mut doc,
        "wall.create_by_room",
        json!({ "room_id": room.0, "height": 2.4 }),
    )
    .unwrap()
    .as_array()
    .unwrap()
    .iter()
    .map(id)
    .collect();
    assert_eq!(made.len(), 4);
    // The linings' axes run round a rectangle 0.2 m inside the walls' faces (0.1 + 0.1).
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for w in &made {
        let g = geometry(&doc, *w);
        assert!((g.height - 2.4).abs() < 1e-9);
        for p in [g.start, g.end] {
            xs.push(p[0]);
            ys.push(p[1]);
        }
    }
    let (x0, x1) = xs
        .iter()
        .fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    let (y0, y1) = ys
        .iter()
        .fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    assert!(
        (x0 - 0.2).abs() < 1e-6 && (x1 - 7.8).abs() < 1e-6,
        "{x0} {x1}"
    );
    assert!(
        (y0 - 0.2).abs() < 1e-6 && (y1 - 4.8).abs() < 1e-6,
        "{y0} {y1}"
    );
    // Each lining's end is the next one's start: they join at the corners.
    let ends: Vec<([f64; 2], [f64; 2])> = made
        .iter()
        .map(|w| {
            let g = geometry(&doc, *w);
            (g.start, g.end)
        })
        .collect();
    for (_, e) in &ends {
        assert!(
            ends.iter()
                .any(|(s, _)| (s[0] - e[0]).hypot(s[1] - e[1]) < 1e-6)
        );
    }
}
