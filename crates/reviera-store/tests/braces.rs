// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Braces (STR-BRACE-001) and slanted columns (STR-COL-002): braces attached to columns and beams
//! follow them, their usage, freed ends when a member goes, slanted columns by end point and by
//! angle, and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, beams, columns};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn beam(doc: &Document, b: ElementId) -> beams::BeamGeometry {
    beams::all(doc.store())
        .into_iter()
        .find(|(id, _)| *id == b)
        .unwrap()
        .1
}

fn column(doc: &Document, c: ElementId) -> columns::ColumnGeometry {
    columns::regenerate(doc.store(), doc.store().get(c).unwrap()).unwrap()
}

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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn braces_follow_their_members_and_slanted_columns_lean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("br.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(
        &r,
        &mut doc,
        "level.create",
        json!({ "elevation": 4.0 }),
    ));
    let l3 = id(run(
        &r,
        &mut doc,
        "level.create",
        json!({ "elevation": 8.0 }),
    ));
    let place = |doc: &mut Document, x: f64, y: f64| {
        id(run(
            &r,
            doc,
            "column.place",
            json!({ "point": [x, y], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        ))
    };
    let c00 = place(&mut doc, 0.0, 0.0);
    let c60 = place(&mut doc, 6.0, 0.0);
    place(&mut doc, 0.0, 6.0);
    place(&mut doc, 6.0, 6.0);
    let girder = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));

    // A brace from the foot of one column to the head of the next: attached to both, a BRACE
    // member of the region's HSS, usage other.
    let b = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "start_point": [0.0, 0.0], "start_level": l1.0, "end_point": [6.0, 0.0], "end_level": l2.0 }),
    ));
    let e = doc.store().get(b).unwrap();
    assert_eq!(e.class, "IfcMember");
    assert_eq!(e.param("@PredefinedType").unwrap(), "BRACE");
    let recipe = beams::beam_recipe(e).unwrap();
    assert!(recipe.brace && recipe.attach.iter().all(Option::is_some));
    let i = &run(&r, &mut doc, "beam.info", json!({ "beam_ids": [b.0] }))[0];
    assert_eq!(i["usage"], "other");
    assert_eq!(i["designation"], "HSS6X6X3/8");
    assert_eq!(i["supports"][0][0], "column");
    // It stops at the columns' faces: shorter than its axis.
    let g = beam(&doc, b);
    assert!(g.length() < (36.0f64 + 16.0).sqrt() - 0.3);

    // Moving a column 1 m along x: the brace's end follows.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c60.0], "vector": [1.0, 0.0, 0.0] }),
    );
    assert!(near(beam(&doc, b).b[0], 7.0) && near(beam(&doc, b).b[2], 4.0));

    // A brace attached at mid-span of the girder stays at mid-span when the girder is stretched.
    let kicker = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "start_point": [3.0, 0.0], "start_level": l2.0, "end_point": [0.0, 0.0], "end_level": l1.0, "end_offset": 2.0 }),
    ));
    let a = &beams::beam_recipe(doc.store().get(kicker).unwrap())
        .unwrap()
        .attach;
    assert!(near(a[0].as_ref().unwrap().at, 0.5));
    run(
        &r,
        &mut doc,
        "beam.set",
        json!({ "beam_ids": [girder.0], "curve": { "start": [0.0, 0.0], "end": [8.0, 0.0] } }),
    );
    assert!(near(beam(&doc, kicker).a[0], 4.0));
    // Its other end, 2 m up the column, is attached at half its height.
    assert!(near(a[1].as_ref().unwrap().at, 0.5));

    // A horizontal brace at Level 2: horizontal bracing; with one end moved to Level 3, other.
    let flat = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "start_point": [0.0, 6.0], "start_level": l2.0, "end_point": [6.0, 6.0], "end_level": l2.0 }),
    ));
    let usage = |doc: &mut Document, b: ElementId| {
        run(&r, doc, "beam.info", json!({ "beam_ids": [b.0] }))[0]["usage"].clone()
    };
    assert_eq!(usage(&mut doc, flat), "horizontal_bracing");
    run(
        &r,
        &mut doc,
        "beam.set",
        json!({ "beam_ids": [flat.0], "end_level": l3.0 }),
    );
    assert_eq!(usage(&mut doc, flat), "other");
    assert!(near(beam(&doc, flat).b[2], 8.0));

    // A zero-length brace is refused.
    let err = r
        .execute(
            &mut doc,
            "brace.create",
            json!({ "start_point": [1.0, 1.0], "start_level": l1.0, "end_point": [1.0, 1.0], "end_level": l1.0 }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(err.code, "brace.zero_length");

    // Deleting the column at the brace's foot: the brace stays, its end freed where it was, and
    // one warning names it.
    let out = r
        .execute(
            &mut doc,
            "element.delete",
            json!({ "element_ids": [c00.0], "confirm_dependents": true }),
            &Caller::user(),
        )
        .unwrap();
    let named: Vec<_> = out
        .warnings
        .iter()
        .filter(|w| w.id == "str.brace.end_released" && w.element_ids.contains(&b))
        .collect();
    assert_eq!(named.len(), 1);
    let g = beam(&doc, b);
    assert!(near(g.a[0], 0.0) && near(g.a[2], 0.0));
    assert!(
        beams::beam_recipe(doc.store().get(b).unwrap())
            .unwrap()
            .attach[0]
            .is_none()
    );

    // A slanted column, end point driven: base at (10, 0) on Level 1, top at (12, 0) on Level 2;
    // a sheared prism with horizontal ends, so its volume is its plan area times its height.
    let s = id(run(
        &r,
        &mut doc,
        "column.place_slanted",
        json!({ "p1": [10.0, 0.0], "p1_level": l1.0, "p2": [12.0, 0.0], "p2_level": l2.0 }),
    ));
    let g = column(&doc, s);
    assert_eq!(g.shift, [2.0, 0.0]);
    assert!(near(volume(&g.mesh()), g.ty.area() * 4.0));
    assert_eq!(g.axis().1, [12.0, 0.0, 4.0]);
    // Angle driven: from atan(4/2) to 75°, the top moves and the base stays.
    run(
        &r,
        &mut doc,
        "column.set_slant",
        json!({ "column_ids": [s.0], "style": "angle" }),
    );
    assert!(near(column(&doc, s).shift[0], 2.0));
    run(
        &r,
        &mut doc,
        "column.set_slant",
        json!({ "column_ids": [s.0], "style": "angle", "angle": "75°" }),
    );
    let g = column(&doc, s);
    assert!(near(g.shift[0], 4.0 / 75f64.to_radians().tan()));
    assert_eq!(g.point, [10.0, 0.0]);
    // Raising Level 2 keeps the angle: the top moves out.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 5.0 }),
    );
    assert!(near(
        column(&doc, s).shift[0],
        5.0 / 75f64.to_radians().tan()
    ));
    // Placed top first: the ends are swapped, with a warning; the base is the lower end.
    let out = r
        .execute(
            &mut doc,
            "column.place_slanted",
            json!({ "p1": [20.0, 0.0], "p1_level": l2.0, "p2": [22.0, 0.0], "p2_level": l1.0 }),
            &Caller::user(),
        )
        .unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.id == "str.column.ends_swapped")
    );
    let g = column(&doc, id(out.result));
    assert_eq!((g.point, g.shift), ([22.0, 0.0], [-2.0, 0.0]));
    // Vertical again.
    run(
        &r,
        &mut doc,
        "column.set_slant",
        json!({ "column_ids": [s.0], "style": "vertical" }),
    );
    assert_eq!(column(&doc, s).shift, [0.0, 0.0]);

    // IFC: braces as IfcMember (BRACE) typed by an IfcMemberType; reopened, still braces.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCMEMBER"), 3);
    assert_eq!(count("IFCMEMBERTYPE"), 1);
    assert!(text.contains(".BRACE."));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        beams::all(doc2.store())
            .iter()
            .filter(|(id, _)| beams::beam_recipe(doc2.store().get(*id).unwrap())
                .unwrap()
                .brace)
            .count(),
        3
    );
}
