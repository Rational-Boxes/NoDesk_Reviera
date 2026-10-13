// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Walls: creation, regeneration and IFC output (spec Ch 3 §3.3).

use std::path::{Path, PathBuf};

use reviera_core::walls::{self, RECIPE_KEY};
use reviera_core::{Caller, CommandRegistry, Document, Element, ElementId};
use reviera_ifc::{AttrValue, IfcModel, Schema};
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

fn create_wall(
    r: &CommandRegistry,
    doc: &mut Document,
    level: ElementId,
    start: [f64; 2],
    end: [f64; 2],
) -> ElementId {
    let v = run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": {"line": {"start": start, "end": end}},
            "base_level": level.0,
            "top_constraint": {"unconnected": "3 m"},
        }),
    )
    .unwrap();
    ElementId(v.as_u64().unwrap())
}

fn new_project(dir: &Path) -> (LocalProject, Document, PathBuf) {
    let path = dir.join("walls.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    (project, doc, path)
}

#[test]
fn a_new_wall_regenerates_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let l1 = level(&doc, "Level 1");
    let id = create_wall(&r, &mut doc, l1, [0.0, 0.0], [5.0, 0.0]);

    let wall = doc.store().get(id).unwrap();
    assert_eq!(wall.class, "IfcWall");
    let g = walls::regenerate(doc.store(), wall).unwrap();
    assert_eq!(
        (g.length(), g.height, g.width, g.base_z),
        (5.0, 3.0, 0.2, 0.0)
    );
    let (verts, tris) = g.mesh();
    assert_eq!((verts.len(), tris.len()), (8, 36));

    // The default type was created once and is reused.
    create_wall(&r, &mut doc, l1, [5.0, 0.0], [5.0, 4.0]);
    let types: Vec<_> = doc
        .store()
        .iter()
        .filter(|e| e.class == "IfcWallType")
        .collect();
    assert_eq!(types.len(), 1);

    // Moving the wall regenerates it.
    run(
        &r,
        &mut doc,
        "wall.set_location",
        json!({"wall_id": id.0, "curve": {"line": {"start": [0.0, 0.0], "end": [0.0, 7.5]}}}),
    )
    .unwrap();
    let g = walls::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap();
    assert_eq!(g.length(), 7.5);
    assert_eq!(g.direction(), [0.0, 1.0]);
}

#[test]
fn invalid_walls_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let l1 = level(&doc, "Level 1");
    let short = run(
        &r,
        &mut doc,
        "wall.create",
        json!({"curve": {"line": {"start": [0, 0], "end": [0.1, 0]}}, "base_level": l1.0, "top_constraint": {"unconnected": 3}}),
    )
    .unwrap_err();
    assert_eq!(short.code, "wall.too_short");
    let flat = run(
        &r,
        &mut doc,
        "wall.create",
        json!({"curve": {"line": {"start": [0, 0], "end": [4, 0]}}, "base_level": l1.0, "top_constraint": {"unconnected": 0}}),
    )
    .unwrap_err();
    assert_eq!(flat.code, "wall.non_positive_height");
    // Nothing was left behind by the failed commands (not even the default type).
    assert!(
        doc.store()
            .iter()
            .all(|e| e.class != "IfcWall" && e.class != "IfcWallType")
    );
}

fn entity(ifc: &IfcModel, id: reviera_ifc::StepId) -> reviera_ifc::Entity {
    ifc.attributes_many(&[id]).unwrap().remove(0)
}

#[test]
fn saved_walls_are_standard_ifc_and_reopen_with_their_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    let l1 = level(&doc, "Level 1");
    let id = create_wall(&r, &mut doc, l1, [1.0, 2.0], [6.0, 2.0]);
    project.save(&doc).unwrap();

    let ifc = IfcModel::open(&path).unwrap();
    let wall = ifc.entities_of_type("IfcWall").unwrap();
    assert_eq!(wall.len(), 1);
    let w = entity(&ifc, wall[0].id);
    assert_eq!(w.get("PredefinedType"), &AttrValue::Enum("STANDARD".into()));

    // Placement at the start, X along the wall.
    let placement = entity(&ifc, w.refs("ObjectPlacement")[0]);
    let axes = entity(&ifc, placement.refs("RelativePlacement")[0]);
    let location = entity(&ifc, axes.refs("Location")[0]);
    assert_eq!(
        location.get("Coordinates"),
        &AttrValue::List(vec![
            AttrValue::Real(1.0),
            AttrValue::Real(2.0),
            AttrValue::Real(0.0)
        ])
    );
    // Axis and Body representations.
    let shape = entity(&ifc, w.refs("Representation")[0]);
    let reps: Vec<_> = shape
        .refs("Representations")
        .into_iter()
        .map(|r| entity(&ifc, r))
        .collect();
    let ids: Vec<_> = reps
        .iter()
        .map(|r| r.string("RepresentationIdentifier").unwrap())
        .collect();
    assert_eq!(ids, ["Axis", "Body"]);
    let solid = entity(&ifc, reps[1].refs("Items")[0]);
    assert_eq!(solid.class, "IfcExtrudedAreaSolid");
    assert_eq!(solid.get("Depth"), &AttrValue::Real(3.0));
    let profile = entity(&ifc, solid.refs("SweptArea")[0]);
    assert_eq!(
        (profile.get("XDim"), profile.get("YDim")),
        (&AttrValue::Real(5.0), &AttrValue::Real(0.2))
    );

    // Contained in the level and typed.
    let storey = ifc.entities_of_type("IfcBuildingStorey").unwrap()[0].id;
    let contained = ifc
        .referenced_by(wall[0].id, "IfcRelContainedInSpatialStructure")
        .unwrap();
    assert_eq!(
        entity(&ifc, contained[0]).refs("RelatingStructure"),
        [storey]
    );
    let typed = ifc
        .referenced_by(wall[0].id, "IfcRelDefinesByType")
        .unwrap();
    let wall_type = entity(&ifc, entity(&ifc, typed[0]).refs("RelatingType")[0]);
    assert_eq!(wall_type.class, "IfcWallType");
    assert_eq!(wall_type.string("Name"), Some(walls::DEFAULT_TYPE_NAME));

    // The geometry engine sees a box of 5 × 0.2 × 3 m.
    let meshes = ifc.tessellate(1).unwrap();
    let mesh = meshes
        .iter()
        .find(|m| m.class == "IfcWall")
        .expect("the wall has geometry");
    let xs = mesh.verts.chunks(3).map(|v| v[0]);
    let (min, max) = xs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    assert!(
        (min - 1.0).abs() < 1e-6 && (max - 6.0).abs() < 1e-6,
        "{min} {max}"
    );

    // Reopened: the recipe and type come back, and the wall regenerates the same.
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    let before = doc.store().get(id).unwrap();
    let after = reopened.store().get(id).expect("the wall keeps its ID");
    assert_eq!(after.param(RECIPE_KEY), before.param(RECIPE_KEY));
    assert_eq!(after.param("@type"), before.param("@type"));
    assert_eq!(
        walls::regenerate(reopened.store(), after).unwrap(),
        walls::regenerate(doc.store(), before).unwrap()
    );
}

#[test]
fn moving_and_deleting_walls_updates_the_file_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    let l1 = level(&doc, "Level 1");
    let id = create_wall(&r, &mut doc, l1, [0.0, 0.0], [5.0, 0.0]);
    project.save(&doc).unwrap();
    let count = IfcModel::open(&path).unwrap().entity_count();

    run(
        &r,
        &mut doc,
        "wall.set_location",
        json!({"wall_id": id.0, "curve": {"line": {"start": [0, 0], "end": [0, 8]}}}),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert_eq!(ifc.entity_count(), count, "the shape is updated, not added");
    let solid = &ifc.entities_of_type("IfcRectangleProfileDef").unwrap()[0];
    assert_eq!(entity(&ifc, solid.id).get("XDim"), &AttrValue::Real(8.0));

    run(
        &r,
        &mut doc,
        "element.delete",
        json!({"element_ids": [id.0]}),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert!(ifc.entities_of_type("IfcWall").unwrap().is_empty());
    assert!(
        ifc.entities_of_type("IfcExtrudedAreaSolid")
            .unwrap()
            .is_empty()
    );
    assert!(ifc.entities_of_type("IfcPolyline").unwrap().is_empty());
}

#[test]
fn walls_in_an_existing_model_start_at_their_level() {
    let dir = tempfile::tempdir().unwrap();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc");
    let path = dir.path().join("fzk.ifc");
    std::fs::copy(source, &path).unwrap();
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    let r = registry();
    let upper = doc
        .store()
        .iter()
        .filter(|e| e.class == "IfcBuildingStorey")
        .max_by(|a, b| {
            let z = |e: &Element| e.param("@Elevation").and_then(Value::as_f64).unwrap();
            z(a).total_cmp(&z(b))
        })
        .unwrap();
    let (upper, elevation) = (
        upper.id(),
        upper.param("@Elevation").and_then(Value::as_f64).unwrap(),
    );
    assert!(elevation > 0.0);
    let id = create_wall(&r, &mut doc, upper, [20.0, 20.0], [24.0, 20.0]);
    let g = walls::regenerate(doc.store(), doc.store().get(id).unwrap()).unwrap();
    assert_eq!(g.base_z, elevation);
    project.save(&doc).unwrap();

    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    assert!(reopened.store().get(id).is_some());
    let ifc = IfcModel::open(&path).unwrap();
    let mesh = ifc
        .tessellate(1)
        .unwrap()
        .into_iter()
        .find(|m| reopened.store().find_by_guid(m.global_id.as_ref().unwrap()) == Some(id))
        .expect("the new wall has geometry");
    let zmin = mesh.verts.chunks(3).map(|v| v[2]).fold(f64::MAX, f64::min);
    assert!((zmin - elevation).abs() < 1e-6);
}
