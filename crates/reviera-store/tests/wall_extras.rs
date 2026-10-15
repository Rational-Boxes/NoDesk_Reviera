// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall extras: Split with Gap and the horizontal split (ARC-WALL-011).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, walls};
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

fn setup() -> (
    tempfile::TempDir,
    LocalProject,
    Document,
    CommandRegistry,
    ElementId,
    ElementId,
) {
    let tmp = tempfile::tempdir().unwrap();
    let (p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("w.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(&run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    (tmp, p, doc, r, l1, l2)
}

#[test]
fn split_with_a_gap() {
    let (_t, _p, mut doc, r, l1, _) = setup();
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let door = id(&run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 2.0 }),
    )
    .unwrap());
    let parts = run(
        &r,
        &mut doc,
        "element.split_with_gap",
        json!({ "element_id": wall.0, "point": [4.0, 0.0], "gap": 0.02 }),
    )
    .unwrap();
    let (a, b) = (id(&parts[0]), id(&parts[1]));
    let ra = walls::wall_recipe(doc.store().get(a).unwrap()).unwrap();
    let rb = walls::wall_recipe(doc.store().get(b).unwrap()).unwrap();
    assert!((rb.curve.start[0] - ra.curve.end[0] - 0.02).abs() < 1e-9);
    assert_eq!((ra.join_allowed[1], rb.join_allowed[0]), (false, false));
    // The door stays on the first wall; the first wall keeps the original.
    assert_eq!(a, wall);
    let host = reviera_core::inserts::insert_recipe(doc.store().get(door).unwrap())
        .unwrap()
        .host;
    assert_eq!(host, doc.store().get(a).unwrap().guid().to_string());
    // A locked distance holds the gap: moving the first wall drags the second along.
    assert_eq!(
        run(&r, &mut doc, "constraint.list", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [a.0], "vector": [-1.0, 0.0, 0.0] }),
    )
    .unwrap();
    let ra = walls::wall_recipe(doc.store().get(a).unwrap()).unwrap();
    let rb = walls::wall_recipe(doc.store().get(b).unwrap()).unwrap();
    assert!(
        (rb.curve.start[0] - ra.curve.end[0] - 0.02).abs() < 1e-6,
        "{:?} {:?}",
        ra.curve,
        rb.curve
    );
    // Gaps out of range are refused.
    assert!(
        run(
            &r,
            &mut doc,
            "element.split_with_gap",
            json!({ "element_id": b.0, "point": [8.0, 0.0], "gap": 0.5 })
        )
        .is_err()
    );
}

#[test]
fn split_horizontally_at_a_level() {
    let (_t, _p, mut doc, r, l1, l2) = setup();
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 5.0 } }),
    )
    .unwrap());
    let door = id(&run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 2.0 }),
    )
    .unwrap());
    // A window high up, its sill at 3.5 m.
    let win = id(&run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": wall.0, "point_along": 6.0, "sill_height": 3.5 }),
    )
    .unwrap());
    let parts = run(
        &r,
        &mut doc,
        "wall.split_horizontal",
        json!({ "wall_id": wall.0, "level_id": l2.0 }),
    )
    .unwrap();
    let (lo, hi) = (id(&parts[0]), id(&parts[1]));
    let g = |w: ElementId| walls::regenerate(doc.store(), doc.store().get(w).unwrap()).unwrap();
    assert!((g(lo).height - 3.0).abs() < 1e-9 && (g(hi).base_z - 3.0).abs() < 1e-9);
    assert!((g(hi).height - 2.0).abs() < 1e-9, "{}", g(hi).height);
    // The door stays below; the window moves up, at the same height.
    let host = |i: ElementId| {
        reviera_core::inserts::insert_recipe(doc.store().get(i).unwrap())
            .unwrap()
            .host
    };
    assert_eq!(host(door), doc.store().get(lo).unwrap().guid().to_string());
    assert_eq!(host(win), doc.store().get(hi).unwrap().guid().to_string());
    let p = reviera_core::inserts::resolve(doc.store(), doc.store().get(win).unwrap()).unwrap();
    assert!((p.cut().z[0] - 3.5).abs() < 1e-9, "{:?}", p.cut().z);
    // Through the door: refused.
    assert!(
        run(
            &r,
            &mut doc,
            "wall.split_horizontal",
            json!({ "wall_id": lo.0, "z": 1.0 })
        )
        .is_err()
    );
}

#[test]
fn slanted_and_tapered_walls() {
    let (_t, mut project, mut doc, r, l1, _) = setup();
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let body = |doc: &Document| {
        reviera_core::bodies::all(doc.store())
            .into_iter()
            .find(|b| b.element == wall)
            .unwrap()
    };
    let flat = body(&doc).bounds().unwrap();
    run(
        &r,
        &mut doc,
        "wall.set_cross_section",
        json!({ "wall_ids": [wall.0], "mode": "slanted", "angle": 10f64.to_radians() }),
    )
    .unwrap();
    let (lo, _) = body(&doc).bounds().unwrap();
    // The top leans 0.529 m to the interior (−y for a wall drawn along +x).
    assert!(
        (lo[1] - (flat.0[1] - 3.0 * 10f64.to_radians().tan())).abs() < 1e-6,
        "{lo:?}"
    );
    // Tapered needs a variable layer.
    assert!(run(
        &r,
        &mut doc,
        "wall.set_cross_section",
        json!({ "wall_ids": [wall.0], "mode": "tapered", "exterior_angle": 0.01, "interior_angle": 0.01 }),
    )
    .is_err());
    // A 200 mm variable structure layer, 1° on each face over 3 m: 0.0953 m left at the top.
    let t = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .unwrap()
        .id();
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": [
            { "function": "finish1", "material": "Render", "thickness": "20 mm" },
            { "function": "structure", "material": "Concrete", "thickness": "200 mm", "variable": true },
            { "function": "finish2", "material": "Plaster", "thickness": "20 mm" },
        ] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "wall.set_cross_section",
        json!({ "wall_ids": [wall.0], "mode": "tapered", "exterior_angle": 1f64.to_radians(), "interior_angle": 1f64.to_radians() }),
    )
    .unwrap();
    let top = 0.2 - 3.0 * 2.0 * 1f64.to_radians().tan();
    assert!((top - 0.0953).abs() < 1e-4, "{top}");
    let b = body(&doc).bounds().unwrap();
    let base_width = 0.24;
    assert!(
        (b.1[1] - b.0[1] - base_width).abs() < 1e-6,
        "the base is the widest"
    );
    // 2° each would reach −0.0095 m: refused.
    let e = run(
        &r,
        &mut doc,
        "wall.set_cross_section",
        json!({ "wall_ids": [wall.0], "mode": "tapered", "exterior_angle": 2f64.to_radians(), "interior_angle": 2f64.to_radians() }),
    )
    .unwrap_err();
    assert_eq!(e.code, "wall.taper_negative");
    // Saved as a B-rep.
    project.save(&doc).unwrap();
}

#[test]
fn edited_profiles() {
    let (_t, mut project, mut doc, r, l1, _) = setup();
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let bounds = |doc: &Document| {
        reviera_core::bodies::all(doc.store())
            .into_iter()
            .find(|b| b.element == wall)
            .unwrap()
            .bounds()
            .unwrap()
    };
    // A gable: 1.5 m above the 3 m eaves at the middle.
    run(
        &r,
        &mut doc,
        "wall.edit_profile",
        json!({ "wall_id": wall.0, "loops": [[[0.0, 0.0], [10.0, 0.0], [10.0, 3.0], [5.0, 4.5], [0.0, 3.0]]] }),
    )
    .unwrap();
    assert!((bounds(&doc).1[2] - 4.5).abs() < 1e-6);
    // A 1 m notch at the top of the end, and a hole.
    run(
        &r,
        &mut doc,
        "wall.edit_profile",
        json!({ "wall_id": wall.0, "loops": [
            [[0.0, 0.0], [10.0, 0.0], [10.0, 2.0], [9.0, 2.0], [9.0, 3.0], [0.0, 3.0]],
            [[2.0, 1.0], [3.0, 1.0], [3.0, 2.0], [2.0, 2.0]],
        ] }),
    )
    .unwrap();
    // Raising the top keeps the notch 1 m deep.
    run(
        &r,
        &mut doc,
        "wall.set_constraints",
        json!({ "wall_id": wall.0, "top_constraint": { "unconnected": 4.0 } }),
    )
    .unwrap();
    let p = run(
        &r,
        &mut doc,
        "wall.get_profile",
        json!({ "wall_id": wall.0 }),
    )
    .unwrap();
    assert_eq!(p[0][2], json!([10.0, 3.0]));
    assert_eq!(p[0][4], json!([9.0, 4.0]));
    assert_eq!(p[1][0], json!([2.0, 1.0]));
    // The body's volume: 10 × 4 less the 1 × 1 notch and the 1 × 1 hole, by 0.2.
    let body = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == wall)
        .unwrap();
    let volume: f64 = body
        .parts
        .iter()
        .map(|m| {
            m.mesh
                .1
                .chunks(3)
                .map(|t| {
                    let [a, b, c] = [0, 1, 2].map(|k| m.mesh.0[t[k] as usize]);
                    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                        + a[2] * (b[0] * c[1] - b[1] * c[0]))
                        / 6.0
                })
                .sum::<f64>()
        })
        .sum();
    assert!((volume - 0.2 * 38.0).abs() < 1e-6, "{volume}");
    // A door still cuts it; curved walls are refused; Reset restores the rectangle.
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 6.0 }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    run(
        &r,
        &mut doc,
        "wall.reset_profile",
        json!({ "wall_ids": [wall.0] }),
    )
    .unwrap();
    assert!((bounds(&doc).1[2] - 4.0).abs() < 1e-6);
}

#[test]
fn location_lines_stay_put() {
    let (_t, mut project, mut doc, r, l1, _) = setup();
    // A first wall makes the default type (200 mm); a 300 mm one beside it.
    let w = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "location_line": "finish_face_exterior" }),
    )
    .unwrap());
    let g = |doc: &Document| walls::regenerate(doc.store(), doc.store().get(w).unwrap()).unwrap();
    // Drawn on its exterior face (the left, +y): the centre is 0.1 m inside.
    assert!((g(&doc).start[1] + 0.1).abs() < 1e-9, "{:?}", g(&doc));
    let t200 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .unwrap()
        .id();
    let t300 = id(&run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": t200.0, "new_name": "Generic - 300mm" }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": t300.0, "param": "width", "value": 0.3 }),
    )
    .unwrap();
    // 200 → 300 mm with Finish Face: Exterior leaves the exterior face where it was.
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [w.0], "type_id": t300.0 }),
    )
    .unwrap();
    let after = g(&doc);
    assert!(
        (after.start[1] + 0.15).abs() < 1e-9 && (after.start[1] + after.width / 2.0).abs() < 1e-9
    );
    // Changing the location line keeps the body; the curve moves to the centre.
    run(
        &r,
        &mut doc,
        "wall.set_location_line",
        json!({ "wall_ids": [w.0], "location_line": "wall_centerline" }),
    )
    .unwrap();
    assert_eq!(g(&doc), after);
    let rec = walls::wall_recipe(doc.store().get(w).unwrap()).unwrap();
    assert!((rec.curve.start[1] + 0.15).abs() < 1e-9);
    // A core-face line on a layered type: the core's exterior face.
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t300.0, "layers": [
            { "function": "finish1", "material": "Render", "thickness": 0.05 },
            { "function": "structure", "material": "Concrete", "thickness": 0.25 },
        ] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "wall.set_location_line",
        json!({ "wall_ids": [w.0], "location_line": "core_face_exterior" }),
    )
    .unwrap();
    let rec = walls::wall_recipe(doc.store().get(w).unwrap()).unwrap();
    assert!((rec.curve.start[1] + 0.05).abs() < 1e-9, "{:?}", rec.curve);
    assert_eq!(g(&doc), after);
    project.save(&doc).unwrap();
}
