// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Modify tools: move, copy, rotate, mirror, offset, split, trim/extend, align and pin
//! (spec Ch 3 §3.12, ARC-MOD-001 … 013).

use reviera_core::walls::{self, Line};
use reviera_core::{Caller, CommandRegistry, Document, ElementId, grids, inserts};
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

fn ids(v: Value) -> Vec<ElementId> {
    serde_json::from_value(v).unwrap()
}

fn project() -> (tempfile::TempDir, Document, ElementId) {
    let dir = tempfile::tempdir().unwrap();
    let (_, doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("m.ifc")).unwrap();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (dir, doc, level)
}

fn wall(
    r: &CommandRegistry,
    doc: &mut Document,
    level: ElementId,
    s: [f64; 2],
    e: [f64; 2],
) -> ElementId {
    let v = run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": s, "end": e } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap();
    ElementId(v.as_u64().unwrap())
}

fn curve(doc: &Document, id: ElementId) -> Line {
    walls::wall_recipe(doc.store().get(id).unwrap())
        .unwrap()
        .curve
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn copies_carry_their_doors_and_get_new_marks_and_names() {
    let (_d, mut doc, level) = project();
    let r = registry();
    let w = wall(&r, &mut doc, level, [0.0, 0.0], [6.0, 0.0]);
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": w.0, "point_along": 2 }),
    )
    .unwrap();
    let g = ElementId(
        run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [0, -2], "end": [0, 8] } } }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let copies = run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [w.0, g.0], "vectors": [[0, 5, 0], [0, 10, 0]] }),
    )
    .unwrap();
    let copies: Vec<Vec<ElementId>> = serde_json::from_value(copies).unwrap();
    assert_eq!(copies.len(), 2);
    let w2 = copies[0][0];
    assert!(close(curve(&doc, w2).start, [0.0, 5.0]));
    // The copied wall has its own door, with the next Mark.
    let doors: Vec<(ElementId, String)> = inserts::placed(doc.store())
        .into_iter()
        .map(|p| {
            let mark = doc
                .store()
                .get(p.id)
                .unwrap()
                .param("@Tag")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            (p.host, mark)
        })
        .collect();
    assert_eq!(doors.len(), 3);
    assert!(doors.iter().any(|(h, m)| *h == w2 && m == "2"));
    let names: Vec<String> = grids::list(doc.store())
        .into_iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(names, ["1", "2", "3"]);
    // A door copied alone stays in its wall.
    let door = inserts::placed(doc.store())
        .into_iter()
        .find(|p| p.host == w)
        .unwrap()
        .id;
    let c = run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [door.0], "vectors": [[2.5, 3, 0]] }),
    )
    .unwrap();
    let c: Vec<Vec<ElementId>> = serde_json::from_value(c).unwrap();
    let p = inserts::resolve(doc.store(), doc.store().get(c[0][0]).unwrap()).unwrap();
    assert_eq!(p.host, w);
    assert!((p.recipe.along - 4.5).abs() < 1e-9);
}

#[test]
fn rotate_and_mirror_keep_walls_joined_and_turn_doors() {
    let (_d, mut doc, level) = project();
    let r = registry();
    let a = wall(&r, &mut doc, level, [0.0, 0.0], [4.0, 0.0]);
    let b = wall(&r, &mut doc, level, [4.0, 0.0], [4.0, 3.0]);
    let door = ElementId(
        run(
            &r,
            &mut doc,
            "door.place",
            json!({ "host_wall_id": a.0, "point_along": 2 }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let joins_before = walls::solve(doc.store()).joins.len();
    run(
        &r,
        &mut doc,
        "element.rotate",
        json!({ "element_ids": [a.0, b.0], "center": [0, 0], "angle": "90°" }),
    )
    .unwrap();
    assert!(close(curve(&doc, a).end, [0.0, 4.0]));
    assert!(close(curve(&doc, b).end, [-3.0, 4.0]));
    assert_eq!(walls::solve(doc.store()).joins.len(), joins_before);
    // A door cannot be rotated away from its wall.
    let e = run(
        &r,
        &mut doc,
        "element.rotate",
        json!({ "element_ids": [door.0], "center": [0, 0], "angle": 1 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.cannot_rotate");

    // Mirror (copy) about the y axis: the copy's exterior and door face the mirrored side.
    let before = inserts::resolve(doc.store(), doc.store().get(door).unwrap()).unwrap();
    let m = ids(run(
        &r,
        &mut doc,
        "element.mirror",
        json!({ "element_ids": [a.0], "axis": { "start": [1, -5], "end": [1, 5] } }),
    )
    .unwrap());
    let ma = m[0];
    assert!(close(curve(&doc, ma).start, [2.0, 0.0]));
    let ra = walls::wall_recipe(doc.store().get(ma).unwrap()).unwrap();
    assert!(ra.flip);
    let copy = inserts::placed(doc.store())
        .into_iter()
        .find(|p| p.host == ma)
        .unwrap();
    assert_ne!(copy.recipe.flip_facing, before.recipe.flip_facing);
    // The door's swing side is the mirror image of the original's.
    let side = |p: &inserts::Placed| {
        let s = p.swing()[0][1];
        s[0]
    };
    assert!((side(&copy) - (2.0 - side(&before))).abs() < 1e-9);
}

#[test]
fn offset_split_trim_and_extend_walls() {
    let (_d, mut doc, level) = project();
    let r = registry();
    let a = wall(&r, &mut doc, level, [0.0, 0.0], [10.0, 0.0]);
    let o = ids(run(
        &r,
        &mut doc,
        "element.offset",
        json!({ "element_ids": [a.0], "distance": "2 m", "side_point": [5, 7] }),
    )
    .unwrap());
    assert!(close(curve(&doc, o[0]).start, [0.0, 2.0]));

    // Split with a door beyond the point: it moves to the new wall.
    let door = ElementId(
        run(
            &r,
            &mut doc,
            "door.place",
            json!({ "host_wall_id": a.0, "point_along": 8 }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let parts = ids(run(
        &r,
        &mut doc,
        "element.split",
        json!({ "element_id": a.0, "point": [6, 0.3] }),
    )
    .unwrap());
    assert!(close(curve(&doc, parts[0]).end, [6.0, 0.0]));
    assert!(close(curve(&doc, parts[1]).start, [6.0, 0.0]));
    let p = inserts::resolve(doc.store(), doc.store().get(door).unwrap()).unwrap();
    assert_eq!(p.host, parts[1]);
    assert!((p.recipe.along - 2.0).abs() < 1e-9);
    let e = run(
        &r,
        &mut doc,
        "element.split",
        json!({ "element_id": parts[1].0, "point": [8, 0] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "split.through_insert");

    // Trim to corner: two walls that do not touch meet in an L.
    let v = wall(&r, &mut doc, level, [12.0, 1.0], [12.0, 6.0]);
    let h = wall(&r, &mut doc, level, [13.0, 8.0], [18.0, 8.0]);
    run(
        &r,
        &mut doc,
        "element.trim_corner",
        json!({ "a": { "element_id": v.0, "keep_point": [12, 3] }, "b": { "element_id": h.0, "keep_point": [16, 8] } }),
    )
    .unwrap();
    assert!(close(curve(&doc, v).end, [12.0, 8.0]));
    assert!(close(curve(&doc, h).start, [12.0, 8.0]));
    assert!(
        walls::solve(doc.store())
            .joins_of(v)
            .iter()
            .any(|j| j.other == h || j.wall == h)
    );
    let e = run(
        &r,
        &mut doc,
        "element.trim_corner",
        json!({ "a": { "element_id": h.0, "keep_point": [16, 8] }, "b": { "element_id": o[0].0, "keep_point": [3, 2] } }),
    )
    .unwrap_err();
    assert_eq!(e.code, "trim.no_intersection");

    // Trim to a grid: the part beyond it goes, and a door left outside is deleted with a warning.
    let g = run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [7, -5], "end": [7, 5] } } }),
    )
    .unwrap();
    let out = r
        .execute(
            &mut doc,
            "element.trim_extend",
            json!({ "boundary": g, "target": { "element_id": parts[1].0, "keep_point": [6.5, 0] } }),
            &Caller::user(),
        )
        .unwrap();
    assert!(close(curve(&doc, parts[1]).end, [7.0, 0.0]));
    assert!(doc.store().get(door).is_none());
    assert_eq!(out.warnings.len(), 1);
}

#[test]
fn align_moves_a_wall_face_onto_a_grid_and_pins_refuse_edits() {
    let (_d, mut doc, level) = project();
    let r = registry();
    let w = wall(&r, &mut doc, level, [0.0, 0.0], [0.0, 5.0]);
    let g = run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [3, -2], "end": [3, 8] } } }),
    )
    .unwrap();
    // The wall runs +y, so its exterior face (left) is at x = -0.1.
    run(
        &r,
        &mut doc,
        "element.align",
        json!({
            "reference": { "element": g, "sub": "axis", "point": [3, 1] },
            "targets": [{ "element": w.0, "sub": "face:ext", "point": [-0.1, 1] }],
        }),
    )
    .unwrap();
    assert!(close(curve(&doc, w).start, [3.1, 0.0]));

    run(&r, &mut doc, "element.pin", json!({ "element_ids": [w.0] })).unwrap();
    for (cmd, args) in [
        (
            "element.move",
            json!({ "element_ids": [w.0], "vector": [1, 0, 0] }),
        ),
        ("element.delete", json!({ "element_ids": [w.0] })),
        ("wall.flip", json!({ "wall_ids": [w.0] })),
    ] {
        assert_eq!(
            run(&r, &mut doc, cmd, args).unwrap_err().code,
            "element.pinned",
            "{cmd}"
        );
    }
    // Copies of a pinned element are allowed, and are not pinned.
    let c: Vec<Vec<ElementId>> = serde_json::from_value(
        run(
            &r,
            &mut doc,
            "element.copy",
            json!({ "element_ids": [w.0], "vectors": [[2, 0, 0]] }),
        )
        .unwrap(),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c[0][0].0], "vector": [1, 0, 0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.unpin",
        json!({ "element_ids": [w.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [w.0], "vector": [1, 0, 0] }),
    )
    .unwrap();
}

#[test]
fn reshaping_a_wall_keeps_its_doors_in_place() {
    let (_d, mut doc, level) = project();
    let r = registry();
    // Drawn right to left: its start is the right end.
    let w = wall(&r, &mut doc, level, [10.0, 0.0], [0.0, 0.0]);
    let door = ElementId(
        run(
            &r,
            &mut doc,
            "door.place",
            json!({ "host_wall_id": w.0, "point_along": 3 }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let at = |doc: &Document| {
        let p = inserts::resolve(doc.store(), doc.store().get(door).unwrap()).unwrap();
        let c = p.world([0.0, 0.0, 0.0]);
        [c[0], c[1]]
    };
    assert!(close(at(&doc), [7.0, 0.0]));
    // Dragging the start end out by 2 m.
    run(
        &r,
        &mut doc,
        "wall.set_location",
        json!({ "wall_id": w.0, "curve": { "line": { "start": [12, 0], "end": [0, 0] } } }),
    )
    .unwrap();
    assert!(close(at(&doc), [7.0, 0.0]));
    // Trimming the start back to a grid at x = 9.
    let g = run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [9, -3], "end": [9, 3] } } }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.trim_extend",
        json!({ "boundary": g, "target": { "element_id": w.0, "keep_point": [4, 0] } }),
    )
    .unwrap();
    assert!(close(curve(&doc, w).start, [9.0, 0.0]));
    assert!(close(at(&doc), [7.0, 0.0]));
}
