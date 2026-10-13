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

/// The axis of `grid` tagged `tag`, and the list attribute that holds it.
fn axis_by_tag(ifc: &IfcModel, grid: StepId, tag: &str) -> (StepId, &'static str) {
    let g = entity(ifc, grid);
    for attr in ["UAxes", "VAxes", "WAxes"] {
        for a in g.refs(attr) {
            if entity(ifc, a).string("AxisTag") == Some(tag) {
                return (a, attr);
            }
        }
    }
    panic!("no axis {tag}");
}

#[test]
fn grids_edited_by_another_tool_follow_the_ifc_axes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc, path) = new_project(dir.path());
    let r = registry();
    for k in 0..3 {
        let x = f64::from(k) * 6.0;
        grid(&r, &mut doc, [x, -1.0], [x, 13.0]);
    }
    let a = grid(&r, &mut doc, [-1.0, 0.0], [13.0, 0.0]);
    run(
        &r,
        &mut doc,
        "grid.rename",
        json!({ "grid_id": a.0, "name": "A" }),
    )
    .unwrap();
    grid(&r, &mut doc, [-1.0, 6.0], [13.0, 6.0]);
    project.save(&doc).unwrap();
    let before = grids::list(doc.store());
    let id_of = |name: &str| before.iter().find(|g| g.name == name).unwrap().id;

    // Another tool: moves 2, renames B to B1, removes 3 and adds C.
    let mut ifc = IfcModel::open(&path).unwrap();
    let grid_step = ifc.entities_of_type("IfcGrid").unwrap()[0].id;
    let (two, _) = axis_by_tag(&ifc, grid_step, "2");
    let curve = entity(&ifc, two).refs("AxisCurve")[0];
    for (p, y) in entity(&ifc, curve)
        .refs("Points")
        .into_iter()
        .zip([-1.0, 13.0])
    {
        let coords = AttrValue::List(vec![AttrValue::Real(7.0), AttrValue::Real(y)]);
        ifc.set_attribute(p, "Coordinates", &coords).unwrap();
    }
    let (b, _) = axis_by_tag(&ifc, grid_step, "B");
    ifc.set_attribute(b, "AxisTag", &AttrValue::String("B1".into()))
        .unwrap();
    let (three, list) = axis_by_tag(&ifc, grid_step, "3");
    let kept: Vec<AttrValue> = entity(&ifc, grid_step)
        .refs(list)
        .into_iter()
        .filter(|a| *a != three)
        .map(|a| AttrValue::Ref(a.0))
        .collect();
    ifc.set_attribute(grid_step, list, &AttrValue::List(kept))
        .unwrap();
    ifc.remove(three).unwrap();
    let mut points = Vec::new();
    for xy in [[-1.0, 12.0], [13.0, 12.0]] {
        let p = ifc.create("IfcCartesianPoint").unwrap();
        let coords = AttrValue::List(xy.iter().map(|v| AttrValue::Real(*v)).collect());
        ifc.set_attribute(p, "Coordinates", &coords).unwrap();
        points.push(AttrValue::Ref(p.0));
    }
    let polyline = ifc.create("IfcPolyline").unwrap();
    ifc.set_attribute(polyline, "Points", &AttrValue::List(points))
        .unwrap();
    let c = ifc.create("IfcGridAxis").unwrap();
    ifc.set_attribute(c, "AxisTag", &AttrValue::String("C".into()))
        .unwrap();
    ifc.set_attribute(c, "AxisCurve", &AttrValue::Ref(polyline.0))
        .unwrap();
    ifc.set_attribute(c, "SameSense", &AttrValue::Bool(true))
        .unwrap();
    let mut u: Vec<AttrValue> = entity(&ifc, grid_step)
        .refs("UAxes")
        .into_iter()
        .map(|a| AttrValue::Ref(a.0))
        .collect();
    u.push(AttrValue::Ref(c.0));
    ifc.set_attribute(grid_step, "UAxes", &AttrValue::List(u))
        .unwrap();
    ifc.write(&path).unwrap();

    let (mut project, doc, report) = LocalProject::open(&path).unwrap();
    assert_eq!(report.reconciled.len(), 4, "{:?}", report.reconciled);
    assert!(project.is_dirty(&doc));
    let after = grids::list(doc.store());
    let find = |name: &str| after.iter().find(|g| g.name == name);
    // Moved, keeping its identity.
    let g2 = find("2").unwrap();
    assert_eq!((g2.id, g2.curve.start), (id_of("2"), [7.0, -1.0]));
    // Renamed, keeping its identity.
    assert_eq!(find("B1").unwrap().id, id_of("B"));
    assert!(find("B").is_none() && find("3").is_none());
    let c = find("C").expect("the added axis is a grid").clone();

    // Saved and opened again: nothing more to reconcile, and the new grid keeps its identity.
    project.save(&doc).unwrap();
    let (_, again, report) = LocalProject::open(&path).unwrap();
    assert!(report.reconciled.is_empty(), "{:?}", report.reconciled);
    assert_eq!(grids::list(again.store()), after);
    assert_eq!(
        grids::list(again.store()).iter().find(|g| g.name == "C"),
        Some(&c)
    );

    // Removing the IfcGrid removes Reviera's grids.
    let mut ifc = IfcModel::open(&path).unwrap();
    let grid_step = ifc.entities_of_type("IfcGrid").unwrap()[0].id;
    ifc.remove(grid_step).unwrap();
    ifc.write(&path).unwrap();
    let (_, doc, report) = LocalProject::open(&path).unwrap();
    assert!(grids::list(doc.store()).is_empty());
    assert_eq!(report.reconciled.len(), 5);
}
