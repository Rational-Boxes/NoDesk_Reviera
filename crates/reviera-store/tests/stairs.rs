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
    assert_eq!(ifc.entities_of_type("IfcFacetedBrep").unwrap().len(), 3);
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
