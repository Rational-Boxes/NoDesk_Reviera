// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Datum extents (ARC-DATUM-001) and scope boxes (ARC-SCOPE-001): a grid's 2D extent in one plan
//! leaves another plan alone until propagated; reset, bubbles, elbows and maximize; a scope box
//! clips its grid, hides it from the plans it misses, follows a turn, and is saved as IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, datum_extents};
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn get(r: &CommandRegistry, doc: &mut Document, datum: ElementId, view: ElementId) -> Value {
    run(
        r,
        doc,
        "datum.get",
        json!({ "datum_id": datum.0, "view_id": view.0 }),
    )
    .unwrap()
}

#[test]
fn extents_per_view_and_scope_boxes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("datums.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(&run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let _ = l2;
    run(
        &r,
        &mut doc,
        "view.create_plan",
        json!({ "level_ids": [l1.0] }),
    )
    .unwrap();
    let plan = |doc: &Document, name: &str| {
        doc.store()
            .iter()
            .find(|e| reviera_core::views::is_view(e) && e.name.as_deref() == Some(name))
            .unwrap()
            .id()
    };
    let (a, b) = (plan(&doc, "Level 1"), plan(&doc, "Level 2"));
    let grid = id(&run(
        &r,
        &mut doc,
        "grid.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [0.0, 20.0] } } }),
    )
    .unwrap());

    // A 2D end in plan A: plan B and the 3D extent are unchanged.
    run(
        &r,
        &mut doc,
        "datum.set_extent",
        json!({ "datum_id": grid.0, "view_id": a.0, "end": "end", "point": [0.0, 12.0] }),
    )
    .unwrap();
    let ga = get(&r, &mut doc, grid, a);
    assert_eq!(ga["view"]["mode"], "2D");
    assert_eq!(ga["view"]["line"]["end"], json!([0.0, 12.0]));
    assert_eq!(ga["extent"]["line"]["end"], json!([0.0, 20.0]));
    assert_eq!(get(&r, &mut doc, grid, b)["view"]["mode"], "3D");
    // Propagated, B has it too.
    run(
        &r,
        &mut doc,
        "datum.propagate_extents",
        json!({ "datum_ids": [grid.0], "source_view_id": a.0, "target_view_ids": [b.0] }),
    )
    .unwrap();
    assert_eq!(
        get(&r, &mut doc, grid, b)["view"]["line"]["end"],
        json!([0.0, 12.0])
    );
    // Reset in A; bubbles and elbows per view.
    run(
        &r,
        &mut doc,
        "datum.reset_to_3d",
        json!({ "datum_ids": [grid.0], "view_id": a.0 }),
    )
    .unwrap();
    assert_eq!(get(&r, &mut doc, grid, a)["view"]["mode"], "3D");
    run(
        &r,
        &mut doc,
        "datum.set_bubble",
        json!({ "datum_id": grid.0, "view_id": a.0, "end": "start", "visible": true }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "datum.add_elbow",
        json!({ "datum_id": grid.0, "view_id": a.0, "end": "end" }),
    )
    .unwrap();
    let ga = get(&r, &mut doc, grid, a);
    assert_eq!(ga["view"]["bubbles"], json!([true, true]));
    assert_eq!(ga["view"]["elbows"], json!([null, 5.0]));
    assert_eq!(
        get(&r, &mut doc, grid, b)["view"]["elbows"],
        json!([null, null])
    );

    // Maximize: over a wall from (-10, 30) to (10, 30), with a 1 m margin.
    run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [-10.0, 30.0], "end": [10.0, 30.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "datum.maximize_3d",
        json!({ "datum_ids": [grid.0] }),
    )
    .unwrap();
    let line = &get(&r, &mut doc, grid, a)["extent"]["line"];
    assert!(line["end"][1].as_f64().unwrap() > 30.0, "{line}");

    // A scope box from Level 1, 2 m high (below Level 2's cut), round (-5, -5)…(5, 5).
    let sb = id(&run(
        &r,
        &mut doc,
        "scopebox.create",
        json!({ "name": "Core", "rectangle": { "corner_a": [-5.0, -5.0], "corner_b": [5.0, 5.0] }, "level_id": l1.0, "height": 2.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "datum.assign_scope_box",
        json!({ "datum_ids": [grid.0], "scope_box_id": sb.0 }),
    )
    .unwrap();
    let line = get(&r, &mut doc, grid, a)["extent"]["line"].clone();
    assert!(
        (line["start"][1].as_f64().unwrap() + 5.0).abs() < 1e-9
            && (line["end"][1].as_f64().unwrap() - 5.0).abs() < 1e-9,
        "clipped to the box: {line}"
    );
    let e = doc.store().get(grid).unwrap().clone();
    assert!(datum_extents::shows_in_plan(doc.store(), &e, 1.2));
    assert!(
        !datum_extents::shows_in_plan(doc.store(), &e, 4.2),
        "Level 2's cut misses the box"
    );
    // Turned 30°, the grid is clipped to the turned box: longer than its sides.
    run(
        &r,
        &mut doc,
        "scopebox.set_extents",
        json!({ "scope_box_id": sb.0, "rotation": 30f64.to_radians() }),
    )
    .unwrap();
    let line = get(&r, &mut doc, grid, a)["extent"]["line"].clone();
    let len = (line["end"][1].as_f64().unwrap() - line["start"][1].as_f64().unwrap()).abs();
    assert!(
        (len - 10.0 / 30f64.to_radians().cos()).abs() < 1e-6,
        "{len}"
    );
    let listed = run(&r, &mut doc, "scopebox.list", json!({})).unwrap();
    assert_eq!(listed[0]["datums"], json!([grid.0]));

    // Saved: the box is an IfcAnnotation with an IfcBoundingBox; reopened, the grid keeps its box.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(
        rows.iter()
            .any(|r| r.class == "IFCANNOTATION" && r.attributes.contains("'ScopeBox'"))
    );
    assert_eq!(
        rows.iter().filter(|r| r.class == "IFCBOUNDINGBOX").count(),
        1
    );
    drop(project);
    let (_p2, mut doc2, _) = LocalProject::open(&path).unwrap();
    let g2 = doc2
        .store()
        .iter()
        .find(|e| reviera_core::grids::is_grid(e))
        .unwrap()
        .id();
    let a2 = doc2
        .store()
        .iter()
        .find(|e| e.name.as_deref() == Some("Level 1") && reviera_core::views::is_view(e))
        .unwrap()
        .id();
    let got = get(&r, &mut doc2, g2, a2);
    assert!(got["extent"]["scope_box"].is_string(), "{got}");
    assert_eq!(got["view"]["elbows"], json!([null, 5.0]));
    // Unassigned, it keeps its extent.
    run(
        &r,
        &mut doc2,
        "datum.assign_scope_box",
        json!({ "datum_ids": [g2.0], "scope_box_id": null }),
    )
    .unwrap();
    let got = get(&r, &mut doc2, g2, a2);
    assert!(got["extent"]["scope_box"].is_null());
    assert_eq!(got["extent"]["z"], json!([0.0, 2.0]));
}
