// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural connections (STR-CONN-001) and piles and pile caps (STR-FND-004): a generic
//! connection at a beam-column join that follows its members and goes with them; single piles;
//! pile caps with their piles following the cap; and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, connections, foundations};
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn connections_and_piles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cp.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "din_18065" }));
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
    let c0 = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [0.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let c1 = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [6.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let g = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));

    // A generic connection at the girder's join with the first column, at the column's head.
    let conn = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [c0.0, g.0] }),
    ));
    let e = doc.store().get(conn).unwrap();
    assert_eq!(e.class, "IfcElementAssembly");
    assert_eq!(e.param("@ObjectType").unwrap(), "STRUCTURAL_CONNECTION");
    let info = |doc: &mut Document| {
        run(
            &r,
            doc,
            "structural_connection.info",
            json!({ "connection_ids": [conn.0] }),
        )[0]
        .clone()
    };
    let i = info(&mut doc);
    assert_eq!(i["connection_type"], "generic");
    assert_eq!(i["members"][0], c0.0);
    let p = i["point"].as_array().unwrap();
    assert!(near(p[0].as_f64().unwrap(), 0.0) && near(p[2].as_f64().unwrap(), 4.0));
    // Members that do not meet are refused.
    let err = r
        .execute(
            &mut doc,
            "structural_connection.create",
            json!({ "member_ids": [c1.0, c0.0] }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(err.code, "connection.no_join");
    // Its information.
    run(
        &r,
        &mut doc,
        "structural_connection.set",
        json!({ "connection_ids": [conn.0], "approval_status": "Approved", "code_check": "passed", "fabricator": "Stahlbau Nord" }),
    );
    let i = info(&mut doc);
    assert_eq!(i["approval_status"], "Approved");
    assert_eq!(i["code_check"], "passed");
    assert_eq!(i["fabricator"], "Stahlbau Nord");
    // It follows its members: the column moves, the join moves.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c0.0], "vector": [0.0, 0.0, 0.0] }),
    );
    assert_eq!(connections::all(doc.store()).len(), 1);

    // A single pile: a 450 mm bored pile 12 m long, its top 1 m below Level 1.
    let pile = id(run(
        &r,
        &mut doc,
        "pile.place",
        json!({ "point": [20.0, 0.0], "level": l1.0, "offset": -1.0 }),
    ));
    let e = doc.store().get(pile).unwrap();
    assert_eq!(e.class, "IfcPile");
    assert_eq!(e.param("@PredefinedType").unwrap(), "BORED");
    let pg = foundations::regenerate(doc.store(), e).unwrap();
    assert!(near(pg.top, -1.0) && near(pg.bottom, -13.0));

    // A pile cap with four piles under it, at three diameters; the piles follow the cap.
    let cap = id(run(
        &r,
        &mut doc,
        "pile_cap.place",
        json!({ "point": [30.0, 0.0], "level": l1.0, "offset": -0.5 }),
    ));
    assert_eq!(
        doc.store()
            .get(cap)
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "PILE_CAP"
    );
    let piles = |doc: &Document| {
        let guid = doc.store().get(cap).unwrap().guid().to_string();
        doc.store()
            .iter()
            .filter(|p| {
                foundations::pile_recipe(p)
                    .and_then(|r| r.cap)
                    .is_some_and(|c| c.cap == guid)
            })
            .map(|p| foundations::regenerate(doc.store(), p).unwrap())
            .collect::<Vec<_>>()
    };
    let under = piles(&doc);
    assert_eq!(under.len(), 4);
    let cg = foundations::regenerate(doc.store(), doc.store().get(cap).unwrap()).unwrap();
    // Each pile starts at the cap's underside, 0.675 m off its centre on each axis.
    for p in &under {
        assert!(near(p.top, cg.bottom));
        let c = [
            p.ring.iter().map(|q| q[0]).sum::<f64>() / 24.0,
            p.ring.iter().map(|q| q[1]).sum::<f64>() / 24.0,
        ];
        assert!(
            near((c[0] - 30.0).abs(), 0.675) && near(c[1].abs(), 0.675),
            "{c:?}"
        );
    }
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [cap.0], "vector": [2.0, 0.0, 0.0] }),
    );
    let moved = piles(&doc);
    let xs: f64 = moved
        .iter()
        .flat_map(|p| p.ring.iter().map(|q| q[0]))
        .sum::<f64>()
        / 96.0;
    assert!(near(xs, 32.0));

    // IFC: the cap aggregates its piles; piles and the connection are kept.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCPILE"), 5);
    assert!(text.contains(".PILE_CAP.") && text.contains("'rxd:pile_cap'"));
    assert!(text.contains("'STRUCTURAL_CONNECTION'"));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(connections::all(doc2.store()).len(), 1);
    assert_eq!(
        doc2.store()
            .iter()
            .filter(|e| foundations::pile_recipe(e).is_some())
            .count(),
        5
    );

    // A connection goes with its members; piles go with their cap.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [g.0], "confirm_dependents": true }),
    );
    assert!(connections::all(doc.store()).is_empty());
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [cap.0], "confirm_dependents": true }),
    );
    assert_eq!(
        doc.store()
            .iter()
            .filter(|e| foundations::pile_recipe(e).is_some())
            .count(),
        1
    );
}
