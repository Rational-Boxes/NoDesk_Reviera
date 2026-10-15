// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Areas, area schemes and colour schemes (ARC-AREA-001, ARC-CLR-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

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

struct Model {
    _t: tempfile::TempDir,
    project: LocalProject,
    doc: Document,
    r: CommandRegistry,
    level: ElementId,
}

fn model() -> Model {
    let t = tempfile::tempdir().unwrap();
    let (project, doc) = LocalProject::new(Schema::Ifc4, &t.path().join("a.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    Model {
        _t: t,
        project,
        doc,
        r,
        level,
    }
}

fn wall(m: &mut Model, a: [f64; 2], b: [f64; 2]) -> ElementId {
    id(&run(
        &m.r,
        &mut m.doc,
        "wall.create",
        json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": m.level.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap())
}

fn rect(m: &mut Model, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<ElementId> {
    let c = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    (0..4).map(|k| wall(m, c[k], c[(k + 1) % 4])).collect()
}

fn area_of(m: &Model, a: ElementId) -> f64 {
    reviera_core::areas::all(m.doc.store())
        .into_iter()
        .find(|x| x.id == a)
        .unwrap()
        .area
}

#[test]
fn gross_building_on_the_exterior_faces() {
    let mut m = model();
    rect(&mut m, 0.0, 0.0, 10.0, 6.0);
    let plans = run(
        &m.r,
        &mut m.doc,
        "areaplan.create",
        json!({ "scheme_id": "Gross Building", "level_ids": [m.level.0], "auto_boundaries": true }),
    )
    .unwrap();
    let plan = id(&plans[0]);
    let a = id(&run(
        &m.r,
        &mut m.doc,
        "area.place",
        json!({ "view_id": plan.0, "point": [5.0, 3.0] }),
    )
    .unwrap());
    // The footprint: 10.2 × 6.2 m to the exterior faces.
    assert!(
        (area_of(&m, a) - 10.2 * 6.2).abs() < 1e-6,
        "{}",
        area_of(&m, a)
    );
    m.project.save(&m.doc).unwrap();
}

#[test]
fn rentable_offices_share_the_centreline() {
    let mut m = model();
    let mut walls = rect(&mut m, 0.0, 0.0, 10.0, 6.0);
    walls.push(wall(&mut m, [5.0, 0.0], [5.0, 6.0]));
    let plan = id(&run(
        &m.r,
        &mut m.doc,
        "areaplan.create",
        json!({ "scheme_id": "Rentable", "level_ids": [m.level.0] }),
    )
    .unwrap()[0]);
    run(
        &m.r,
        &mut m.doc,
        "areaboundary.create",
        json!({ "view_id": plan.0, "wall_ids": walls.iter().map(|w| w.0).collect::<Vec<_>>(), "apply_rules": true }),
    )
    .unwrap();
    let left = id(&run(
        &m.r,
        &mut m.doc,
        "area.place",
        json!({ "view_id": plan.0, "point": [2.5, 3.0], "area_type": "Office Area" }),
    )
    .unwrap());
    let right = id(&run(
        &m.r,
        &mut m.doc,
        "area.place",
        json!({ "view_id": plan.0, "point": [7.5, 3.0], "area_type": "Office Area" }),
    )
    .unwrap());
    // Office ↔ outside: the office-side faces; Office ↔ Office: the partition's centreline.
    assert!(
        (area_of(&m, left) - 4.9 * 5.8).abs() < 1e-6,
        "{}",
        area_of(&m, left)
    );
    assert!((area_of(&m, right) - 4.9 * 5.8).abs() < 1e-6);
    // The right one becomes Exterior Area: the boundary moves to the office side's face.
    run(
        &m.r,
        &mut m.doc,
        "area.set_type",
        json!({ "area_ids": [right.0], "area_type": "Exterior Area" }),
    )
    .unwrap();
    assert!(
        (area_of(&m, left) - 4.8 * 5.8).abs() < 1e-6,
        "{}",
        area_of(&m, left)
    );
    let list = run(
        &m.r,
        &mut m.doc,
        "area.list",
        json!({ "scheme_id": "Rentable" }),
    )
    .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
    m.project.save(&m.doc).unwrap();
}

#[test]
fn colour_schemes_by_value_and_range() {
    let mut m = model();
    // Five rooms in a row.
    rect(&mut m, 0.0, 0.0, 25.0, 5.0);
    for x in [5.0, 10.0, 15.0, 20.0] {
        wall(&mut m, [x, 0.0], [x, 5.0]);
    }
    run(
        &m.r,
        &mut m.doc,
        "param.project.add",
        json!({ "def": { "name": "Department", "data_type": "text" }, "categories": ["cat.rooms"], "is_instance": true }),
    )
    .unwrap();
    let mut rooms = Vec::new();
    for (k, dept) in ["Admin", "Sales", "Design", "Legal", "IT"]
        .iter()
        .enumerate()
    {
        let room = id(&run(
            &m.r,
            &mut m.doc,
            "room.place",
            json!({ "level_id": m.level.0, "point": [2.5 + 5.0 * k as f64, 2.5] }),
        )
        .unwrap());
        run(
            &m.r,
            &mut m.doc,
            "param.set",
            json!({ "element_ids": [room.0], "param_ref": "Department", "value": dept }),
        )
        .unwrap();
        rooms.push(room);
    }
    let s = run(
        &m.r,
        &mut m.doc,
        "colorscheme.create",
        json!({ "name": "Departments", "parameter": "Department" }),
    )
    .unwrap();
    let rows = run(
        &m.r,
        &mut m.doc,
        "colorscheme.legend",
        json!({ "scheme_id": s }),
    )
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let colors: std::collections::HashSet<&str> =
        rows.iter().map(|r| r["color"].as_str().unwrap()).collect();
    assert_eq!(colors.len(), 5, "distinct colours");
    // A new value adds a sixth row.
    run(
        &m.r,
        &mut m.doc,
        "param.set",
        json!({ "element_ids": [rooms[0].0], "param_ref": "Department", "value": "Research" }),
    )
    .unwrap();
    let rows = run(
        &m.r,
        &mut m.doc,
        "colorscheme.legend",
        json!({ "scheme_id": "Departments" }),
    )
    .unwrap();
    assert_eq!(
        rows.as_array().unwrap().len(),
        5,
        "Admin is gone, Research is new"
    );
    run(
        &m.r,
        &mut m.doc,
        "colorscheme.set_rows",
        json!({ "scheme_id": "Departments", "rows": [{ "value": "Admin", "color": "#ff0000" }] }),
    )
    .unwrap();
    let rows = run(
        &m.r,
        &mut m.doc,
        "colorscheme.legend",
        json!({ "scheme_id": "Departments" }),
    )
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 6, "the stored Admin row stays, not in use");
    assert_eq!(rows[0]["in_use"], json!(false));
    // A plan shows it: five rooms filled.
    let plan = m.doc.store().iter().find(|e| {
        reviera_core::views::view_data(e).is_some_and(|d| d.kind == reviera_core::plans::KIND_PLAN)
    });
    let plan = plan.map(|p| p.id()).unwrap_or_else(|| {
        id(&run(
            &m.r,
            &mut m.doc,
            "view.create_plan",
            json!({ "level_ids": [m.level.0] }),
        )
        .unwrap()[0])
    });
    run(
        &m.r,
        &mut m.doc,
        "view.set_color_scheme",
        json!({ "view_id": plan.0, "scheme_id": "Departments" }),
    )
    .unwrap();
    let fills = reviera_core::color_schemes::fills(m.doc.store(), m.doc.store().get(plan).unwrap());
    assert_eq!(fills.len(), 5);

    // By range on Area: a 20.0 m² room falls in the 20–50 row.
    let mut m = model();
    rect(&mut m, 0.0, 0.0, 4.2, 5.2);
    let room = id(&run(
        &m.r,
        &mut m.doc,
        "room.place",
        json!({ "level_id": m.level.0, "point": [2.0, 2.0] }),
    )
    .unwrap());
    let s = run(
        &m.r,
        &mut m.doc,
        "colorscheme.create",
        json!({ "name": "By Area", "parameter": "Area", "mode": "range", "ranges": [0.0, 10.0, 20.0, 50.0] }),
    )
    .unwrap();
    let scheme = reviera_core::color_schemes::find(m.doc.store(), s.as_str().unwrap()).unwrap();
    let rows = reviera_core::color_schemes::rows_now(m.doc.store(), &scheme);
    let c = reviera_core::color_schemes::color_of(m.doc.store(), &scheme, &rows, room).unwrap();
    let want = reviera_core::color_schemes::rgba(&rows[2].color).unwrap();
    assert_eq!(c, want, "the 20–50 row");
    assert!(rows[2].in_use && !rows[1].in_use);
}
