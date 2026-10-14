// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall openings (ARC-WALL-012, ARC-OPEN-003), shafts (ARC-OPEN-004) and vertical openings in
//! roofs (ARC-OPEN-002): their cuts, how they follow levels and hosts, and their IFC.

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

fn level1(doc: &Document) -> ElementId {
    doc.store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id()
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        { "line": { "start": [x0, y0], "end": [x1, y0] } },
        { "line": { "start": [x1, y0], "end": [x1, y1] } },
        { "line": { "start": [x1, y1], "end": [x0, y1] } },
        { "line": { "start": [x0, y1], "end": [x0, y0] } },
    ])
}

/// The volume of a closed mesh (divergence theorem).
fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

fn body_volume(doc: &Document, id: ElementId) -> f64 {
    reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == id)
        .flat_map(|b| b.parts)
        .map(|p| volume(&p.mesh))
        .sum()
}

#[test]
fn a_wall_opening_cuts_its_wall_follows_its_level_and_voids_it_in_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("o.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = level1(&doc);
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let wall = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 0], "end": [8, 0] } },
            "base_level": l1.0,
            "top_constraint": { "level": l2.0 },
        }),
    )
    .unwrap());
    let whole = body_volume(&doc, wall);
    assert!((whole - 8.0 * 3.0 * 0.2).abs() < 1e-6, "{whole}");

    // 2 m wide, from Level 1 up to 0.5 m under Level 2.
    let opening = id(run(
        &r,
        &mut doc,
        "opening.create_wall",
        json!({
            "wall_id": wall.0, "u0": 3.0, "u1": 5.0,
            "top_constraint": { "level": l2.0 }, "top_offset": -0.5,
        }),
    )
    .unwrap());
    let cut = body_volume(&doc, wall);
    assert!((whole - cut - 2.0 * 2.5 * 0.2).abs() < 1e-6, "{cut}");

    // Raising Level 2 makes it taller (ARC-OPEN-003).
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 3.5 }),
    )
    .unwrap();
    let raised = body_volume(&doc, wall);
    assert!(
        (8.0 * 3.5 * 0.2 - raised - 2.0 * 3.0 * 0.2).abs() < 1e-6,
        "{raised}"
    );

    // A door may not overlap it, nor another opening.
    let e = run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 4.5 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.overlaps", "{e:?}");
    let e = run(
        &r,
        &mut doc,
        "opening.create_wall",
        json!({ "wall_id": wall.0, "u0": 4.5, "u1": 6.0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "opening.overlaps");
    // Or leave the wall.
    let e = run(
        &r,
        &mut doc,
        "opening.create_wall",
        json!({ "wall_id": wall.0, "u0": 7.5, "u1": 8.5 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "opening.outside_host");

    // It slides along the wall by a move's component along it.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [opening.0], "vector": [1.0, 3.0, 0.0] }),
    )
    .unwrap();
    let o = reviera_core::wall_openings::resolve(doc.store(), doc.store().get(opening).unwrap())
        .unwrap();
    assert_eq!(o.recipe.u, [4.0, 6.0]);
    // Edited to a fixed height.
    run(
        &r,
        &mut doc,
        "opening.edit_wall",
        json!({ "opening_id": opening.0, "base_offset": 1.0, "top_constraint": { "unconnected": 1.2 } }),
    )
    .unwrap();
    let o = reviera_core::wall_openings::resolve(doc.store(), doc.store().get(opening).unwrap())
        .unwrap();
    assert_eq!(o.z, [1.0, 2.2]);

    // Copying the wall copies its opening.
    let copies = run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [wall.0], "vectors": [[0.0, 5.0, 0.0]] }),
    )
    .unwrap();
    let copy = id(copies[0][0].clone());
    let copy_guid = doc.store().get(copy).unwrap().guid().to_string();
    assert_eq!(
        reviera_core::wall_openings::of_host(doc.store(), &copy_guid).len(),
        1
    );

    // IFC: an IfcOpeningElement voiding each wall; the walls' own bodies stay whole.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCOPENINGELEMENT"), 2);
    assert_eq!(count("IFCRELVOIDSELEMENT"), 2);
    // Reopened, the opening is still a wall opening.
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(reviera_core::wall_openings::all(doc2.store()).len(), 2);

    // Deleting the wall deletes its opening.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [wall.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert!(doc.store().get(opening).is_none());
}

fn floor_area(doc: &Document, floor: ElementId) -> f64 {
    let e = doc.store().get(floor).unwrap();
    let f = reviera_core::floors::regenerate(doc.store(), e).unwrap();
    reviera_core::slab_openings::cut_profiles(doc.store(), e, &f.profiles)
        .iter()
        .map(|p| {
            reviera_core::polygons::area(&p.outer).abs()
                - p.holes
                    .iter()
                    .map(|h| reviera_core::polygons::area(h).abs())
                    .sum::<f64>()
        })
        .sum()
}

#[test]
fn a_shaft_cuts_every_floor_in_its_range_including_later_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = level1(&doc);
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let l3 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 6.0 })).unwrap());
    let floor = |r: &CommandRegistry, doc: &mut Document, level: ElementId| {
        id(run(
            r,
            doc,
            "floor.create",
            json!({ "loops": [square(0.0, 0.0, 10.0, 10.0)], "level_id": level.0 }),
        )
        .unwrap())
    };
    let f1 = floor(&r, &mut doc, l1);
    let f2 = floor(&r, &mut doc, l2);
    // From Level 1 to Level 3: the floors above Level 1 up to Level 3 are cut.
    let shaft = id(run(
        &r,
        &mut doc,
        "opening.create_shaft",
        json!({
            "curves": square(4.0, 4.0, 6.0, 5.5),
            "base_level": l1.0,
            "top_constraint": { "level": l3.0 },
        }),
    )
    .unwrap());
    assert!((floor_area(&doc, f1) - 100.0).abs() < 1e-6);
    assert!((floor_area(&doc, f2) - 97.0).abs() < 1e-6);
    // A floor added later in its range is cut too.
    let f3 = floor(&r, &mut doc, l3);
    assert!((floor_area(&doc, f3) - 97.0).abs() < 1e-6);
    // Lowering the top under Level 3's floor leaves it whole.
    run(
        &r,
        &mut doc,
        "opening.edit_shaft",
        json!({ "shaft_id": shaft.0, "top_offset": -0.5 }),
    )
    .unwrap();
    assert!((floor_area(&doc, f3) - 100.0).abs() < 1e-6);
    // Moving the shaft moves its holes.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [shaft.0], "vector": [3.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert!((floor_area(&doc, f2) - 97.0).abs() < 1e-6);
    let s = reviera_core::slab_openings::shaft_recipe(doc.store().get(shaft).unwrap()).unwrap();
    assert_eq!(s.loops[0][0][0], 7.0);

    // IFC: the shaft is a proxy with the whole prism; one opening voids the cut floor.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCBUILDINGELEMENTPROXY"), 1);
    assert_eq!(count("IFCOPENINGELEMENT"), 1);
    assert_eq!(count("IFCRELVOIDSELEMENT"), 1);
    // Back to Level 3: two hosts, two derived openings; saved again they are updated, not added.
    run(
        &r,
        &mut doc,
        "opening.edit_shaft",
        json!({ "shaft_id": shaft.0, "top_offset": 0.0 }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCOPENINGELEMENT"), 2);
    assert_eq!(count("IFCRELVOIDSELEMENT"), 2);
    // Reopened, the derived openings are not elements; the shaft is.
    drop(project);
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        doc.store()
            .iter()
            .filter(|e| e.class == "IfcOpeningElement")
            .count(),
        0
    );
    assert_eq!(reviera_core::slab_openings::shafts(doc.store()).len(), 1);
    // Deleting the shaft removes its openings.
    let shaft = reviera_core::slab_openings::shafts(doc.store())[0].0;
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [shaft.0], "confirm_dependents": true }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCOPENINGELEMENT"), 0);
    assert_eq!(count("IFCRELVOIDSELEMENT"), 0);
}

#[test]
fn a_vertical_opening_in_a_sloped_roof_has_plumb_sides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = level1(&doc);
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(0.0, 0.0, 10.0, 8.0), "level_id": l1.0, "base_offset": 3.0 }),
    )
    .unwrap());
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    let area = g.projected_area();
    // A skylight on the south slope, clear of the hips.
    let opening = id(run(
        &r,
        &mut doc,
        "opening.create_vertical",
        json!({ "host_id": roof.0, "loops": [[[4.0, 1.0], [6.0, 1.0], [6.0, 2.0], [4.0, 2.0]]] }),
    )
    .unwrap());
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    assert_eq!(g.uncut, 0);
    assert!(
        (area - g.projected_area() - 2.0).abs() < 1e-6,
        "{}",
        g.projected_area()
    );
    // The hole's sides are plumb: every point of the slab on the hole's edge x = 4 is at x = 4.
    let face = g.faces.iter().find(|f| !f.holes.is_empty()).unwrap();
    let on_side: Vec<&[f64; 3]> = face
        .slab
        .0
        .iter()
        .filter(|p| (p[0] - 4.0).abs() < 1e-9 && p[1] > 0.99 && p[1] < 2.01)
        .collect();
    assert_eq!(on_side.len(), 4);
    // A shaft reaching the roof cuts it as well.
    run(
        &r,
        &mut doc,
        "opening.create_shaft",
        json!({
            "loops": [[[4.0, 6.0], [5.0, 6.0], [5.0, 7.0], [4.0, 7.0]]],
            "base_level": l1.0, "top_constraint": { "unconnected": 10.0 },
        }),
    )
    .unwrap();
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    assert!((area - g.projected_area() - 3.0).abs() < 1e-6);
    // Moving the roof moves its opening.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [roof.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    let o = reviera_core::slab_openings::opening_recipe(doc.store().get(opening).unwrap()).unwrap();
    assert_eq!(o.loops[0][0], [5.0, 1.0]);
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    // The roof's own opening, and the one the shaft derives for it (still over the roof).
    assert_eq!(count("IFCOPENINGELEMENT"), 2);
}
