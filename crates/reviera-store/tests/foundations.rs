// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Foundations (STR-FND-001 … 003): isolated footings under columns and at grids, wall
//! foundations (bearing and retaining) following their walls, foundation slabs, and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, foundations};
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| ElementId(x.as_u64().unwrap()))
        .collect()
}

fn geometry(doc: &Document, f: ElementId) -> foundations::FoundationGeometry {
    foundations::regenerate(doc.store(), doc.store().get(f).unwrap()).unwrap()
}

fn centre(g: &foundations::FoundationGeometry) -> [f64; 2] {
    let n = g.ring.len() as f64;
    [
        g.ring.iter().map(|p| p[0]).sum::<f64>() / n,
        g.ring.iter().map(|p| p[1]).sum::<f64>() / n,
    ]
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array(
        (0..4)
            .map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } }))
            .collect(),
    )
}

#[test]
fn footings_wall_foundations_and_slabs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.ifc");
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

    // 20 columns, 4 by 5; footings at columns: 20, centred under them, tops at their bases.
    let mut cols = Vec::new();
    for i in 0..4 {
        for j in 0..5 {
            cols.push(id(run(
                &r,
                &mut doc,
                "column.place",
                json!({ "point": [i as f64 * 6.0, j as f64 * 6.0], "base_level": l1.0, "base_offset": -0.2, "top_constraint": { "level": l2.0 } }),
            )));
        }
    }
    let placed = run(
        &r,
        &mut doc,
        "footing.place_at_columns",
        json!({ "column_ids": cols.iter().map(|c| c.0).collect::<Vec<_>>() }),
    );
    let footings = ids(&placed);
    assert_eq!(footings.len(), 20);
    let g = geometry(&doc, footings[7]);
    let c = reviera_core::columns::column_recipe(doc.store().get(cols[7]).unwrap())
        .unwrap()
        .point;
    assert!(near(centre(&g)[0], c[0]) && near(centre(&g)[1], c[1]));
    // US default: 6'-0" × 6'-0" × 1'-6"; top at the column's base (−200 mm).
    let i = &run(
        &r,
        &mut doc,
        "foundation.info",
        json!({ "ids": [footings[7].0] }),
    )[0];
    assert!(near(i["top"].as_f64().unwrap(), -0.2));
    assert!(near(i["bottom"].as_f64().unwrap(), -0.2 - 0.4572));
    assert!(near(i["area"].as_f64().unwrap(), 1.8288 * 1.8288));
    assert_eq!(i["host"], cols[7].0);
    // Again: the columns have footings, so none are added.
    let again = run(
        &r,
        &mut doc,
        "footing.place_at_columns",
        json!({ "column_ids": [cols[0].0] }),
    );
    assert!(ids(&again).is_empty());

    // A footing follows its column; when the column goes, it stays where it was.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [cols[0].0], "vector": [0.5, 0.0, 0.0] }),
    );
    assert!(near(centre(&geometry(&doc, footings[0]))[0], 0.5));
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [cols[0].0], "confirm_dependents": true }),
    );
    let g = geometry(&doc, footings[0]);
    assert!(near(centre(&g)[0], 0.5) && near(g.top, -0.2));
    assert!(
        foundations::footing_recipe(doc.store().get(footings[0]).unwrap())
            .unwrap()
            .column
            .is_none()
    );

    // At grids: two by two lines make four footings, turned to the grids.
    let mut grids = Vec::new();
    for (k, x) in [40.0, 46.0].into_iter().enumerate() {
        grids.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [x, -2.0], "end": [x, 10.0] } }, "name": (["X1", "X2"][k]) }),
        )));
    }
    for (k, y) in [0.0, 6.0].into_iter().enumerate() {
        grids.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [38.0, y], "end": [50.0, y] } }, "name": (["Y1", "Y2"][k]) }),
        )));
    }
    let at = run(
        &r,
        &mut doc,
        "footing.place_at_grids",
        json!({ "grid_ids": grids.iter().map(|g| g.0).collect::<Vec<_>>(), "level": l1.0, "offset": -0.5 }),
    );
    assert_eq!(ids(&at).len(), 4);
    assert!(near(geometry(&doc, ids(&at)[0]).top, -0.5));

    // A wall along x and its bearing foundation: 24" wide, centred, under the wall's base.
    let wall = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 40.0], "end": [10.0, 40.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    ));
    let wf = ids(&run(
        &r,
        &mut doc,
        "wall_foundation.create",
        json!({ "wall_ids": [wall.0] }),
    ))[0];
    let i = &run(&r, &mut doc, "foundation.info", json!({ "ids": [wf.0] }))[0];
    assert!(near(i["area"].as_f64().unwrap(), 10.0 * 0.6096));
    assert!(near(i["top"].as_f64().unwrap(), 0.0) && near(i["bottom"].as_f64().unwrap(), -0.3048));
    let ys = |doc: &Document| {
        let g = geometry(doc, wf);
        let lo = g.ring.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let hi = g.ring.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        (lo, hi)
    };
    let (lo, hi) = ys(&doc);
    assert!(near(lo, 40.0 - 0.3048) && near(hi, 40.0 + 0.3048));
    // Retaining: toe 600 mm beyond the exterior face (+y), heel 1200 mm beyond the interior.
    let t = reviera_core::GlobalId::parse(
        doc.store()
            .get(wf)
            .unwrap()
            .param("@type")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .and_then(|g| doc.store().find_by_guid(&g))
    .unwrap();
    let ret = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": t.0, "new_name": "Retaining Footing" }),
    ));
    for (param, value) in [
        ("function", json!("retaining")),
        ("toe", json!(0.6)),
        ("heel", json!("1200 mm")),
        ("end_extension", json!(0.3)),
    ] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": ret.0, "param": param, "value": value }),
        );
    }
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [wf.0], "type_id": ret.0 }),
    );
    let w = reviera_core::walls::regenerate(doc.store(), doc.store().get(wall).unwrap())
        .unwrap()
        .width;
    let (lo, hi) = ys(&doc);
    assert!(near(hi, 40.0 + w / 2.0 + 0.6), "{hi}");
    assert!(near(lo, 40.0 - w / 2.0 - 1.2), "{lo}");
    let g = geometry(&doc, wf);
    let xs: Vec<f64> = g.ring.iter().map(|p| p[0]).collect();
    assert!(xs.iter().any(|x| near(*x, -0.3)) && xs.iter().any(|x| near(*x, 10.3)));
    // It follows its wall, and goes with it.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [wall.0], "vector": [0.0, 1.0, 0.0] }),
    );
    assert!(near(ys(&doc).1, 41.0 + w / 2.0 + 0.6));

    // A foundation slab: top at Level 1 − 200 mm, 12" thick below, BASESLAB.
    let slab = id(run(
        &r,
        &mut doc,
        "foundation_slab.create",
        json!({ "boundary": [rect([60.0, 0.0], [70.0, 8.0])], "level": l1.0, "offset": -0.2 }),
    ));
    let e = doc.store().get(slab).unwrap();
    assert_eq!(e.class, "IfcSlab");
    assert_eq!(e.param("@PredefinedType").unwrap(), "BASESLAB");
    let zs: Vec<f64> = reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == slab)
        .flat_map(|b| b.parts.into_iter().flat_map(|p| p.mesh.0))
        .map(|p| p[2])
        .collect();
    assert!(near(zs.iter().copied().fold(f64::MIN, f64::max), -0.2));
    assert!(near(
        zs.iter().copied().fold(f64::MAX, f64::min),
        -0.2 - 0.3048
    ));

    // IFC: 24 pad footings and a strip footing, with bodies; reopened, still foundations.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCFOOTING"), 25);
    assert!(text.contains(".PAD_FOOTING.") && text.contains(".STRIP_FOOTING."));
    assert!(text.contains(".BASESLAB."));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(foundations::all(doc2.store()).len(), 25);
    assert_eq!(
        reviera_core::categories::category_of(doc2.store().get(wf).unwrap()).map(|c| c.name),
        Some("Structural Foundations")
    );

    // Deleting the wall deletes its foundation.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [wall.0], "confirm_dependents": true }),
    );
    assert!(doc.store().get(wf).is_none());
}
