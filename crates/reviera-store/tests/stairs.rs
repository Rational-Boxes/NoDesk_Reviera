// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stairs (spec Ch 3 §3.7, ARC-STAIR-001…005): riser counts from the levels, runs with an
//! automatic landing, edits, and the saved IfcStair.

use reviera_core::stairs;
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

fn geometry(doc: &Document, id: ElementId) -> stairs::StairGeometry {
    stairs::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

#[test]
fn a_stair_between_levels_is_saved_as_an_ifc_stair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stairs.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());

    // ARC-STAIR-002: 3 m at a 180 mm maximum riser is 17 risers; 9 in the first run, then a
    // landing and the rest in the second run going the other way (a U).
    let stair = id(run(
        &r,
        &mut doc,
        "stair.create",
        json!({
            "base_level": l1.0,
            "runs": [
                { "start": [0.0, 0.0], "end": [2.24, 0.0] },
                { "start": [2.24, 1.4], "end": [0.0, 1.4] },
            ],
        }),
    )
    .unwrap());
    let e = doc.store().get(stair).unwrap();
    assert_eq!(e.class, "IfcStair");
    assert_eq!(e.param("@PredefinedType").unwrap(), "HALF_TURN_STAIR");
    let g = geometry(&doc, stair);
    assert_eq!(g.risers, 17);
    assert!((g.riser_height - 3.0 / 17.0).abs() < 1e-9);
    let recipe = stairs::stair_recipe(e).unwrap();
    assert_eq!(recipe.runs[0].risers, 9);
    assert_eq!(recipe.runs[1].risers, 8);
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    assert_eq!(g.landings.len(), 1);

    // Fewer risers wanted: the last run takes the difference.
    run(
        &r,
        &mut doc,
        "stair.set_params",
        json!({ "stair_ids": [stair.0], "desired_risers": 16 }),
    )
    .unwrap();
    let recipe = stairs::stair_recipe(doc.store().get(stair).unwrap()).unwrap();
    assert_eq!(recipe.runs[1].risers, 7);
    assert!(geometry(&doc, stair).warnings[0].contains("exceeds"));

    // Moving the stair moves its runs.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [stair.0], "vector": [1.0, 2.0, 0.0] }),
    )
    .unwrap();
    let recipe = stairs::stair_recipe(doc.store().get(stair).unwrap()).unwrap();
    assert_eq!(recipe.runs[0].start, [1.0, 2.0]);

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let in_file = ifc.entities_of_type("IfcStair").unwrap();
    assert_eq!(in_file.len(), 1);
    assert_eq!(ifc.entities_of_type("IfcStairType").unwrap().len(), 1);
    // The runs and landing, and the two railings' rails, posts and balusters.
    let rails: usize = reviera_core::railings::all(doc.store())
        .iter()
        .map(|(_, g)| g.parts.len())
        .sum();
    assert_eq!(ifc.entities_of_type("IfcRailing").unwrap().len(), 2);
    assert_eq!(
        ifc.entities_of_type("IfcFacetedBrep").unwrap().len(),
        3 + rails
    );
    let meshes = ifc.tessellate(1).unwrap();
    assert!(meshes.iter().any(|m| m.class == "IfcStair"));
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(geometry(&reopened, stair).risers, 16);

    // Deleting the top level deletes the stair that reaches it.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [l2.0] }),
    )
    .unwrap();
    assert!(doc.store().get(stair).is_none());
}

#[test]
fn a_spiral_stair_by_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spiral.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap();
    // A full-step spiral about (5, 5), its walking line 1 m out, 1 m wide: every riser in one run.
    let stair = id(run(
        &r,
        &mut doc,
        "stair.create",
        json!({
            "base_level": l1.0, "width": 1.0,
            "runs": [{ "start": [6.0, 5.0], "center": [5.0, 5.0] }],
        }),
    )
    .unwrap());
    let e = doc.store().get(stair).unwrap();
    assert_eq!(e.param("@PredefinedType").unwrap(), "SPIRAL_STAIR");
    let g = geometry(&doc, stair);
    assert_eq!(g.risers, 17);
    assert_eq!(g.runs.len(), 16);
    // Too tight a spiral is refused.
    let err = run(
        &r,
        &mut doc,
        "stair.create",
        json!({
            "base_level": l1.0, "width": 1.0,
            "runs": [{ "start": [5.4, 5.0], "center": [5.0, 5.0] }],
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, "stair.spiral_too_tight");
    // Mirrored, it turns the other way about the mirrored centre.
    run(
        &r,
        &mut doc,
        "element.mirror",
        json!({ "element_ids": [stair.0], "axis": { "start": [0.0, 0.0], "end": [0.0, 1.0] }, "copy": false }),
    )
    .unwrap();
    let recipe = stairs::stair_recipe(doc.store().get(stair).unwrap()).unwrap();
    let sp = recipe.runs[0].spiral.unwrap();
    assert_eq!(sp.center, [-5.0, 5.0]);
    assert!(!sp.ccw);
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let rails: usize = reviera_core::railings::all(doc.store())
        .iter()
        .map(|(_, g)| g.parts.len())
        .sum();
    assert_eq!(ifc.entities_of_type("IfcRailing").unwrap().len(), 2);
    assert_eq!(
        ifc.entities_of_type("IfcFacetedBrep").unwrap().len(),
        16 + rails
    );
}

/// Railings on stairs (ARC-RAIL-001, Place on Stair): placed with the stair, following each side's
/// nosing line, flat across landings, round the outside of a half turn.
#[test]
fn stairs_get_railings_that_follow_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stair-rails.ifc");
    let (_, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap();
    let stair = id(run(
        &r,
        &mut doc,
        "stair.create",
        json!({
            "base_level": l1.0,
            "runs": [
                { "start": [0.0, 0.0], "end": [2.24, 0.0] },
                { "start": [2.24, 1.4], "end": [0.0, 1.4] },
            ],
        }),
    )
    .unwrap());
    let rails: Vec<(
        reviera_core::ElementId,
        reviera_core::railings::RailingGeometry,
    )> = reviera_core::railings::all(doc.store());
    assert_eq!(rails.len(), 2);
    let riser = geometry(&doc, stair).riser_height;
    let first = stairs::stair_recipe(doc.store().get(stair).unwrap())
        .unwrap()
        .runs[0]
        .risers;
    let landing = f64::from(first) * riser;
    for (_, g) in &rails {
        // From the first nosing up to the top.
        assert!((g.heights[0] - riser).abs() < 1e-9, "{:?}", g.heights);
        assert!((g.heights.last().unwrap() - 3.0).abs() < 1e-9);
        let left = g.path[0][1] > 0.0;
        if left {
            // The inside of the half turn: straight across the well.
            assert_eq!(g.path.len(), 4, "{:?}", g.path);
            assert!((g.path[0][1] - 0.55).abs() < 1e-9);
        } else {
            // The outside: round the landing, flat at its height.
            assert_eq!(g.path.len(), 6, "{:?}", g.path);
            assert!((g.path[2][0] - 3.39).abs() < 1e-9, "{:?}", g.path);
            for k in 1..4 {
                assert!((g.heights[k] - landing).abs() < 1e-9, "{:?}", g.heights);
            }
        }
        // The rails rise with the run: the top rail's highest point is the type's height above
        // the top of the stair.
        let top = g
            .parts
            .iter()
            .flat_map(|(p, _)| p.iter())
            .map(|p| p[2])
            .fold(f64::MIN, f64::max);
        assert!((top - 4.0).abs() < 1e-9, "{top}");
    }

    // An L-shaped stair: both sides turn at the landing's corner.
    let l = id(run(
        &r,
        &mut doc,
        "stair.create",
        json!({
            "base_level": l1.0,
            "runs": [
                { "start": [10.0, 0.0], "end": [12.24, 0.0] },
                { "start": [12.84, 0.6], "end": [12.84, 2.84] },
            ],
        }),
    )
    .unwrap());
    let ls: Vec<_> = reviera_core::railings::all(doc.store())
        .into_iter()
        .filter(|(id, _)| {
            reviera_core::railings::railing_recipe(doc.store().get(*id).unwrap()).is_some_and(|r| {
                matches!(r.path, reviera_core::railings::RailPath::Stair { ref stair, .. }
                    if *stair == doc.store().get(l).unwrap().guid().to_string())
            })
        })
        .collect();
    assert_eq!(ls.len(), 2);
    for (_, g) in &ls {
        assert_eq!(g.path.len(), 5, "{:?}", g.path);
    }

    // The railings follow the stair, and go with it.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [stair.0], "vector": [0.0, 5.0, 0.0] }),
    )
    .unwrap();
    let moved =
        reviera_core::railings::regenerate(doc.store(), doc.store().get(rails[0].0).unwrap())
            .unwrap();
    assert!((moved.path[0][1] - rails[0].1.path[0][1] - 5.0).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [stair.0] }),
    )
    .unwrap();
    assert!(doc.store().get(rails[0].0).is_none());
}

#[test]
fn stair_marks_and_tread_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc) =
        LocalProject::new(Schema::Ifc4, &dir.path().join("sm.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap();
    let stair = id(run(
        &r,
        &mut doc,
        "stair.create",
        json!({ "base_level": l1.0, "runs": [
            { "start": [0.0, 0.0], "end": [2.24, 0.0] },
            { "start": [2.24, 1.4], "end": [0.0, 1.4] },
        ] }),
    )
    .unwrap());
    let e = doc.store().get(stair).unwrap();
    let g = geometry(&doc, stair);
    // A plan of Level 1 cut at 1.2 m: the path through every tread, UP, and a cut mark.
    let m = reviera_core::stair_marks::marks(doc.store(), e, 0.0, 1.2).unwrap();
    assert_eq!(m.treads.len(), g.treads.len());
    assert_eq!(m.label.0, "UP");
    assert!(m.cut.is_some());
    // Numbered in the plan: 1 … n by tread; moving the stair keeps the numbers.
    let existing = doc
        .store()
        .iter()
        .find(|e| {
            reviera_core::views::view_data(e)
                .is_some_and(|d| d.kind == reviera_core::plans::KIND_PLAN)
        })
        .map(|e| e.id());
    let plan = match existing {
        Some(p) => p,
        None => id(run(
            &r,
            &mut doc,
            "view.create_plan",
            json!({ "level_ids": [l1.0] }),
        )
        .unwrap()[0]
            .clone()),
    };
    run(
        &r,
        &mut doc,
        "stair.number_treads",
        json!({ "view_id": plan.0, "stair_id": stair.0 }),
    )
    .unwrap();
    let numbers = |doc: &Document| {
        reviera_core::stair_marks::numbers_in(doc.store(), doc.store().get(plan).unwrap())
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>()
    };
    let before = numbers(&doc);
    assert_eq!(before.first(), Some(&1));
    assert_eq!(before.len(), g.treads.len());
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [stair.0], "vector": [3.0, 0.0, 0.0] }),
    )
    .unwrap();
    assert_eq!(numbers(&doc), before);
    project.save(&doc).unwrap();
    // Deleting the stair takes its numbers.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [stair.0] }),
    )
    .unwrap();
    assert!(numbers(&doc).is_empty());
    assert!(
        doc.store()
            .iter()
            .all(|e| reviera_core::stair_marks::numbers_recipe(e).is_none())
    );
}
