// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Levels: creation, elevation changes that move walls, renaming with views, and IFC output
//! (spec Ch 3 §3.2.1, ARC-LVL-001).

use std::path::{Path, PathBuf};

use reviera_core::views;
use reviera_core::walls;
use reviera_core::{Caller, CommandRegistry, Document, Element, ElementId};
use reviera_ifc::{AttrValue, Entity, IfcModel, Schema, StepId};
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

fn level(doc: &Document, name: &str) -> ElementId {
    doc.store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey" && e.name.as_deref() == Some(name))
        .map(Element::id)
        .expect("level exists")
}

fn new_project(dir: &Path) -> (LocalProject, Document, PathBuf) {
    let path = dir.join("levels.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    (project, doc, path)
}

fn create_level(r: &CommandRegistry, doc: &mut Document, args: Value) -> ElementId {
    ElementId(run(r, doc, "level.create", args).unwrap().as_u64().unwrap())
}

fn plan_names(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = views::list(doc.store())
        .into_iter()
        .filter(|v| v.kind == reviera_core::plans::KIND_PLAN)
        .map(|v| v.name)
        .collect();
    names.sort();
    names
}

fn entity(ifc: &IfcModel, id: StepId) -> Entity {
    ifc.attributes_many(&[id]).unwrap().remove(0)
}

#[test]
fn new_levels_continue_the_name_series_and_get_a_plan() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let l2 = create_level(&r, &mut doc, json!({ "elevation": "3 m" }));
    assert_eq!(
        doc.store().get(l2).unwrap().name.as_deref(),
        Some("Level 2")
    );
    assert_eq!(
        doc.store().get(l2).unwrap().param("@Elevation"),
        Some(&json!(3.0))
    );
    let l3 = create_level(
        &r,
        &mut doc,
        json!({ "elevation": 6.0, "make_plan_view": false }),
    );
    assert_eq!(
        doc.store().get(l3).unwrap().name.as_deref(),
        Some("Level 3")
    );
    assert_eq!(plan_names(&doc), ["Level 2"]);

    let listed = run(&r, &mut doc, "level.list", json!({})).unwrap();
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Level 1", "Level 2", "Level 3"]);

    // Names are unique among levels.
    let e = run(
        &r,
        &mut doc,
        "level.create",
        json!({ "elevation": 9.0, "name": "Level 2" }),
    )
    .unwrap_err();
    assert_eq!(e.code, "level.name_taken");

    // One undo step removes the level and its plan.
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert!(doc.store().get(l2).is_none());
    assert!(plan_names(&doc).is_empty());
}

#[test]
fn changing_an_elevation_moves_the_walls_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let l1 = level(&doc, "Level 1");
    let l2 = create_level(&r, &mut doc, json!({ "elevation": 3.0 }));
    let wall = |r: &CommandRegistry, doc: &mut Document, base: ElementId, top: Value| {
        let v = run(
            r,
            doc,
            "wall.create",
            json!({
                "curve": {"line": {"start": [0, 0], "end": [5, 0]}},
                "base_level": base.0,
                "base_offset": 0.1,
                "top_constraint": top,
            }),
        )
        .unwrap();
        ElementId(v.as_u64().unwrap())
    };
    let up_to_l2 = wall(&r, &mut doc, l1, json!({ "level": l2.0 }));
    let on_l2 = wall(&r, &mut doc, l2, json!({ "unconnected": 2.5 }));
    let geometry =
        |doc: &Document, id| walls::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap();
    assert!((geometry(&doc, up_to_l2).height - 2.9).abs() < 1e-9);

    // +300 mm: the wall based on Level 2 rises, the wall up to it gets taller; offsets are kept.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": "3300 mm" }),
    )
    .unwrap();
    assert!((geometry(&doc, up_to_l2).height - 3.2).abs() < 1e-9);
    assert!((geometry(&doc, on_l2).base_z - 3.4).abs() < 1e-9);
    assert!((geometry(&doc, on_l2).height - 2.5).abs() < 1e-9);

    // A level that would put a wall's top at or below its base is refused, and nothing changes.
    let e = run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 0.05 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "level.wall_height");
    assert!((geometry(&doc, up_to_l2).height - 3.2).abs() < 1e-9);

    // The Elevation parameter is the same value, with the same check.
    let p = run(
        &r,
        &mut doc,
        "param.get",
        json!({ "element_id": l2.0, "param_ref": "Elevation" }),
    )
    .unwrap();
    assert!((p["value"].as_f64().unwrap() - 3.3).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "param.set",
        json!({ "element_ids": [l2.0], "param_ref": "Elevation", "value": "4 m" }),
    )
    .unwrap();
    assert!((geometry(&doc, on_l2).base_z - 4.1).abs() < 1e-9);
    let e = run(
        &r,
        &mut doc,
        "param.set",
        json!({ "element_ids": [l2.0], "param_ref": "Elevation", "value": 0 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "level.wall_height");

    // Deleting the level takes its walls and plan with it.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [l2.0] }),
    )
    .unwrap();
    let deleted: Vec<u64> = serde_json::from_value(deleted).unwrap();
    assert!(deleted.contains(&up_to_l2.0) && deleted.contains(&on_l2.0));
    assert!(plan_names(&doc).is_empty());
}

#[test]
fn renaming_a_level_renames_its_plans() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let l2 = create_level(&r, &mut doc, json!({ "elevation": 3.0 }));
    run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l2.0], "allow_duplicates": true }),
    )
    .unwrap();
    assert_eq!(plan_names(&doc), ["Level 2", "Level 2 (2)"]);
    let renamed = run(
        &r,
        &mut doc,
        "level.rename",
        json!({ "level_id": l2.0, "name": "First Floor" }),
    )
    .unwrap();
    assert_eq!(renamed.as_array().unwrap().len(), 2);
    assert_eq!(plan_names(&doc), ["First Floor", "First Floor (2)"]);

    run(
        &r,
        &mut doc,
        "level.rename",
        json!({ "level_id": l2.0, "name": "Mezzanine", "rename_views": false }),
    )
    .unwrap();
    assert_eq!(plan_names(&doc), ["First Floor", "First Floor (2)"]);
    let e = run(
        &r,
        &mut doc,
        "level.rename",
        json!({ "level_id": l2.0, "name": "Level 1" }),
    )
    .unwrap_err();
    assert_eq!(e.code, "level.name_taken");
}

#[test]
fn saved_levels_are_storeys_of_the_building() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    let l2 = create_level(&r, &mut doc, json!({ "elevation": 3.5 }));
    run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": {"line": {"start": [0, 0], "end": [4, 0]}},
            "base_level": l2.0,
            "top_constraint": {"unconnected": 3},
        }),
    )
    .unwrap();
    project.save(&doc).unwrap();

    let check = |path: &Path, z: f64| {
        let ifc = IfcModel::open(path).unwrap();
        let storeys = ifc.entities_of_type("IfcBuildingStorey").unwrap();
        assert_eq!(storeys.len(), 2);
        let s = storeys
            .iter()
            .map(|s| entity(&ifc, s.id))
            .zip(&storeys)
            .find(|(e, _)| e.string("Name") == Some("Level 2"))
            .expect("Level 2 is saved");
        let (e, info) = s;
        assert_eq!(e.get("Elevation"), &AttrValue::Real(z));
        assert_eq!(e.get("CompositionType"), &AttrValue::Enum("ELEMENT".into()));
        // Aggregated under the building, with Level 1.
        let rel = ifc
            .referenced_by(info.id, "IfcRelAggregates")
            .unwrap()
            .into_iter()
            .map(|r| entity(&ifc, r))
            .find(|r| r.refs("RelatedObjects").contains(&info.id))
            .expect("the storey is aggregated");
        let building = entity(&ifc, rel.refs("RelatingObject")[0]);
        assert_eq!(building.class, "IfcBuilding");
        assert_eq!(rel.refs("RelatedObjects").len(), 2);
        // Placed at its elevation.
        let placement = entity(&ifc, e.refs("ObjectPlacement")[0]);
        let axes = entity(&ifc, placement.refs("RelativePlacement")[0]);
        let location = entity(&ifc, axes.refs("Location")[0]);
        assert_eq!(
            location.get("Coordinates"),
            &AttrValue::List(vec![
                AttrValue::Real(0.0),
                AttrValue::Real(0.0),
                AttrValue::Real(z)
            ])
        );
        // The wall is contained in it, at its height.
        let wall = ifc.entities_of_type("IfcWall").unwrap()[0].id;
        let contained = ifc
            .referenced_by(wall, "IfcRelContainedInSpatialStructure")
            .unwrap();
        assert_eq!(
            entity(&ifc, contained[0]).refs("RelatingStructure"),
            [info.id]
        );
        let wp = entity(&ifc, entity(&ifc, wall).refs("ObjectPlacement")[0]);
        let wa = entity(&ifc, wp.refs("RelativePlacement")[0]);
        let wl = entity(&ifc, wa.refs("Location")[0]);
        assert_eq!(
            wl.get("Coordinates"),
            &AttrValue::List(vec![
                AttrValue::Real(0.0),
                AttrValue::Real(0.0),
                AttrValue::Real(z)
            ])
        );
    };
    check(&path, 3.5);

    // Moving the level moves the storey, its placement and the wall on the next save.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 4.0 }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    check(&path, 4.0);

    // Reopened, the level keeps its ID, name and elevation.
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    let e = reopened.store().get(l2).expect("the level keeps its ID");
    assert_eq!(e.name.as_deref(), Some("Level 2"));
    assert_eq!(e.param("@Elevation"), Some(&json!(4.0)));
}
