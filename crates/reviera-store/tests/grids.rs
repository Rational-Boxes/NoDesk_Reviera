// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Grids: naming, editing, dimensions to grids, and the IFC grid system (spec Ch 3 §3.2.2,
//! ARC-GRID-001; DM-IFC-006).

use std::path::{Path, PathBuf};

use reviera_core::grids;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn new_project(dir: &Path) -> (LocalProject, Document, PathBuf) {
    let path = dir.join("grids.ifc");
    let (project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    (project, doc, path)
}

fn grid(r: &CommandRegistry, doc: &mut Document, start: [f64; 2], end: [f64; 2]) -> ElementId {
    let v = run(
        r,
        doc,
        "grid.create",
        json!({ "curve": { "line": { "start": start, "end": end } } }),
    )
    .unwrap();
    ElementId(v.as_u64().unwrap())
}

fn names(doc: &Document) -> Vec<String> {
    grids::list(doc.store())
        .into_iter()
        .map(|g| g.name)
        .collect()
}

fn entity(ifc: &IfcModel, id: StepId) -> Entity {
    ifc.attributes_many(&[id]).unwrap().remove(0)
}

#[test]
fn grids_are_numbered_and_a_letter_starts_a_letter_series() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    for k in 0..10 {
        let x = f64::from(k) * 6.0;
        grid(&r, &mut doc, [x, 0.0], [x, 20.0]);
    }
    let expected: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
    assert_eq!(names(&doc), expected);

    // A new run renamed "A" continues with B, C.
    let a = grid(&r, &mut doc, [-2.0, 0.0], [70.0, 0.0]);
    run(
        &r,
        &mut doc,
        "grid.rename",
        json!({ "grid_id": a.0, "name": "A" }),
    )
    .unwrap();
    grid(&r, &mut doc, [-2.0, 6.0], [70.0, 6.0]);
    grid(&r, &mut doc, [-2.0, 12.0], [70.0, 12.0]);
    assert_eq!(names(&doc)[10..], ["A", "B", "C"]);

    // Names are unique; too-short grids are refused.
    let e = run(
        &r,
        &mut doc,
        "grid.rename",
        json!({ "grid_id": a.0, "name": "B" }),
    )
    .unwrap_err();
    assert_eq!(e.code, "grid.name_taken");
    let e = run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [0, 0], "end": [0, 0.0001] } } }),
    )
    .unwrap_err();
    assert_eq!(e.code, "grid.too_short");

    // One grid system holds them all.
    assert_eq!(
        doc.store().iter().filter(|e| grids::is_system(e)).count(),
        1
    );
    let listed = run(&r, &mut doc, "grid.list", json!({})).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 13);
}

#[test]
fn dimensions_follow_a_moved_grid_and_the_system_goes_with_the_last_line() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc, _) = new_project(dir.path());
    let r = registry();
    let g1 = grid(&r, &mut doc, [0.0, 0.0], [0.0, 10.0]);
    let g2 = grid(&r, &mut doc, [6.0, 0.0], [6.0, 10.0]);
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let plan = run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [level.0] }),
    )
    .unwrap();
    let plan = plan[0].as_u64().unwrap();
    run(
        &r,
        &mut doc,
        "dim.create_aligned",
        json!({
            "view_id": plan,
            "refs": [
                { "element": g1.0, "sub": "axis", "point": [0, 5] },
                { "element": g2.0, "sub": "axis", "point": [6, 5] },
            ],
            "line_point": [3, 12],
        }),
    )
    .unwrap();
    let value = |r: &CommandRegistry, doc: &mut Document| {
        let dims = run(r, doc, "anno.list", json!({ "view_id": plan })).unwrap();
        dims[0]["measured"]["values"][0].as_f64().unwrap()
    };
    assert!((value(&r, &mut doc) - 6.0).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "grid.set_curve",
        json!({ "grid_id": g2.0, "curve": { "line": { "start": [7.5, 0], "end": [7.5, 10] } } }),
    )
    .unwrap();
    assert!((value(&r, &mut doc) - 7.5).abs() < 1e-9);
    // Renamed, the grid keeps its dimension.
    run(
        &r,
        &mut doc,
        "grid.rename",
        json!({ "grid_id": g2.0, "name": "2a" }),
    )
    .unwrap();
    assert!((value(&r, &mut doc) - 7.5).abs() < 1e-9);

    // Deleting both lines takes the grid system too.
    let deleted = run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [g1.0, g2.0] }),
    )
    .unwrap();
    assert_eq!(deleted.as_array().unwrap().len(), 3);
    assert!(!doc.store().iter().any(grids::is_system));
}

#[test]
fn grids_save_as_one_ifc_grid_and_reopen_with_their_identity() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    let mut ids = Vec::new();
    for k in 0..3 {
        let x = f64::from(k) * 6.0;
        ids.push(grid(&r, &mut doc, [x, -1.0], [x, 13.0]));
    }
    for (k, name) in ["A", "B", "C"].iter().enumerate() {
        let y = k as f64 * 6.0;
        let id = grid(&r, &mut doc, [-1.0, y], [13.0, y]);
        run(
            &r,
            &mut doc,
            "grid.rename",
            json!({ "grid_id": id.0, "name": name }),
        )
        .unwrap();
        ids.push(id);
    }
    project.save(&doc).unwrap();

    let check = |path: &Path, u: &[&str], v: &[&str]| {
        let ifc = IfcModel::open(path).unwrap();
        let g = ifc.entities_of_type("IfcGrid").unwrap();
        assert_eq!(g.len(), 1);
        let e = entity(&ifc, g[0].id);
        let tags = |attr: &str| -> Vec<String> {
            e.refs(attr)
                .into_iter()
                .map(|a| entity(&ifc, a).string("AxisTag").unwrap().to_owned())
                .collect()
        };
        assert_eq!(tags("UAxes"), u);
        assert_eq!(tags("VAxes"), v);
        // Contained in the level.
        let contained = ifc
            .referenced_by(g[0].id, "IfcRelContainedInSpatialStructure")
            .unwrap();
        let storey = entity(
            &ifc,
            entity(&ifc, contained[0]).refs("RelatingStructure")[0],
        );
        assert_eq!(storey.class, "IfcBuildingStorey");
        // Curves in file units, 2D.
        let axis = entity(&ifc, e.refs("VAxes")[0]);
        let curve = entity(&ifc, axis.refs("AxisCurve")[0]);
        let start = entity(&ifc, curve.refs("Points")[0]);
        assert_eq!(
            start.get("Coordinates"),
            &AttrValue::List(vec![AttrValue::Real(0.0), AttrValue::Real(-1.0)])
        );
        ifc
    };
    check(&path, &["A", "B", "C"], &["1", "2", "3"]);

    // Renaming and deleting rewrite the axes; no stale axes are left in the file.
    run(
        &r,
        &mut doc,
        "grid.rename",
        json!({ "grid_id": ids[3].0, "name": "A1" }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [ids[5].0] }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let ifc = check(&path, &["A1", "B"], &["1", "2", "3"]);
    assert_eq!(ifc.entities_of_type("IfcGridAxis").unwrap().len(), 5);

    // Reopened: the same grids, IDs and GlobalIds.
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    let before = grids::list(doc.store());
    let after = grids::list(reopened.store());
    assert_eq!(after, before);
}
