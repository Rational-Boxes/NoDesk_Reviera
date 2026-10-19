// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel numbering and NC output (STR-STL-010, STR-STL-007): identical beams (one drawn the
//! other way round) share a part mark and one with a hole gets its own; a column and its base
//! plate are one assembly; a stiffener shop-welded to a beam takes the beam's assembly mark and a
//! site-welded one does not; tags and steel assemblies in IFC; DSTV NC1 pieces for a beam's holes
//! and a plate's outline.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_dstv::{Face, ProfileCode};
use reviera_ifc::Schema;
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

fn marks(doc: &Document, e: ElementId) -> (String, Option<String>) {
    reviera_core::steel_numbering::marks_of(doc.store().get(e).unwrap()).unwrap()
}

#[test]
fn identical_parts_and_assemblies_share_marks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("numbering.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w12 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W12X26" }));
    let beam = |doc: &mut Document, a: [f64; 2], b: [f64; 2]| {
        id(run(&r, doc, "beam.create", json!({ "type_id": w12, "curve": { "start": a, "end": b }, "reference_level": l2.0 })))
    };
    let b1 = beam(&mut doc, [0.0, 0.0], [5.0, 0.0]);
    let b2 = beam(&mut doc, [0.0, 3.0], [5.0, 3.0]);
    // Drawn the other way round.
    let b3 = beam(&mut doc, [5.0, 6.0], [0.0, 6.0]);
    let b4 = beam(&mut doc, [0.0, 9.0], [5.0, 9.0]);
    let b5 = beam(&mut doc, [0.0, 12.0], [5.0, 12.0]);

    let rep = run(&r, &mut doc, "steel_numbering.run", json!({}));
    assert_eq!(marks(&doc, b1).0, "b1");
    for b in [b2, b3, b4, b5] {
        assert_eq!(marks(&doc, b).0, "b1", "identical beams share the mark");
    }
    assert_eq!(rep["parts"][0]["count"], 5);
    assert_eq!(doc.store().get(b1).unwrap().param("@Tag"), Some(&json!("b1")));

    // Two 22 mm holes through b2's web: a new mark for b2 only.
    run(
        &r,
        &mut doc,
        "hole_pattern.create",
        json!({ "element_ids": [b2.0], "face": { "element": b2.0, "face": "web_1" },
                "layout": { "shape": "rect", "rows": 1, "columns": 2, "spacing_x": 0.1, "spacing_y": 0.0, "centre": [2.5, 0.0] },
                "diameter": 0.020, "hole_tolerance": 0.002 }),
    );
    run(&r, &mut doc, "steel_numbering.run", json!({}));
    assert_eq!(marks(&doc, b2).0, "b2");
    for b in [b1, b3, b4, b5] {
        assert_eq!(marks(&doc, b).0, "b1");
    }

    // A column with a base plate: one assembly C1 holding the column and the plate.
    let w12c = run(&r, &mut doc, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let col = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": w12c, "point": [20.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let bp = id(run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [col.0], "type_id": "base_plate" })));
    let bp_plate = id(run(&r, &mut doc, "structural_connection.info", json!({ "connection_ids": [bp.0] }))[0]["plates"][0].clone());

    // Identical stiffeners on b4 and b5, one shop-welded, one site-welded.
    let stiffener = |doc: &mut Document, b: ElementId| {
        id(run(
            &r,
            doc,
            "plate.create",
            json!({ "face": { "element": b.0, "face": "web_1" }, "contour": [[1.0, -0.1], [1.1, -0.1], [1.1, 0.1], [1.0, 0.1]], "thickness": 0.01 }),
        ))
    };
    let s4 = stiffener(&mut doc, b4);
    let s5 = stiffener(&mut doc, b5);
    for (b, s, location) in [(b4, s4, "shop"), (b5, s5, "site")] {
        let edges = run(&r, &mut doc, "weld.edges", json!({ "element_ids": [b.0, s.0] }));
        let edge = edges.as_array().unwrap().first().unwrap_or_else(|| panic!("no edge between {b:?} and {s:?}")).clone();
        run(&r, &mut doc, "weld.create", json!({ "element_ids": [b.0, s.0], "edge": edge, "location": location }));
    }
    let rep = run(&r, &mut doc, "steel_numbering.run", json!({}));
    // The beams are still b1 (the stiffeners are their own parts), the stiffeners share a mark.
    assert_eq!(marks(&doc, b4).0, "b1");
    assert_eq!(marks(&doc, s4).0, marks(&doc, s5).0);
    // Shop welded: the stiffener is in b4's assembly; site welded: on its own.
    assert_eq!(marks(&doc, s4).1, marks(&doc, b4).1);
    assert_ne!(marks(&doc, s5).1, marks(&doc, b5).1);
    assert!(marks(&doc, s5).1.unwrap().starts_with('P'));
    // b4 with its stiffener is a different assembly from the bare beams.
    assert_ne!(marks(&doc, b4).1, marks(&doc, b1).1);
    assert_eq!(marks(&doc, b1).1, marks(&doc, b3).1, "bare beams share an assembly mark");
    // The column and its base plate.
    assert_eq!(marks(&doc, col).0, "c1");
    assert_eq!(marks(&doc, col).1.as_deref(), Some("C1"));
    assert_eq!(marks(&doc, bp_plate).1.as_deref(), Some("C1"));
    let c1 = rep["assemblies"].as_array().unwrap().iter().find(|a| a["mark"] == "C1").unwrap();
    assert_eq!(c1["parts"].as_array().unwrap().len(), 2);
    // The assemblies are elements; running again does not double them.
    run(&r, &mut doc, "steel_numbering.run", json!({}));
    let assemblies = doc
        .store()
        .iter()
        .filter(|e| e.param("@ObjectType") == Some(&json!("STEEL_ASSEMBLY")))
        .count();
    let expected: u64 = rep["assemblies"].as_array().unwrap().iter().map(|a| a["count"].as_u64().unwrap()).sum();
    assert_eq!(assemblies as u64, expected);

    // IFC: tags and steel assemblies.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'STEEL_ASSEMBLY'"));
    assert!(text.contains("'b2'") && text.contains("'C1'"));
    assert!(text.contains("'rxd:steel_assembly'"));

    // NC: b2 with its two holes on the web, the stiffener's outline, the quantities.
    let pieces = reviera_core::steel_numbering::nc_pieces(doc.store(), &Default::default());
    let b2nc = pieces.iter().find(|p| p.piece == "b2").unwrap();
    assert_eq!(b2nc.code, ProfileCode::I);
    assert_eq!(b2nc.profile, "W12X26");
    assert_eq!(b2nc.holes.len(), 2);
    assert!(b2nc.holes.iter().all(|h| matches!(h.face, Face::V | Face::H) && (h.diameter - 22.0).abs() < 1e-6));
    let mut xs: Vec<f64> = b2nc.holes.iter().map(|h| h.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!((xs[1] - xs[0] - 100.0).abs() < 0.01, "{xs:?}");
    let b1nc = pieces.iter().find(|p| p.piece == "b1").unwrap();
    assert_eq!(b1nc.quantity, 4);
    let st = pieces.iter().find(|p| p.piece == marks(&doc, s4).0).unwrap();
    assert_eq!(st.code, ProfileCode::B);
    assert_eq!(st.quantity, 2);
    assert!((st.length - 200.0).abs() < 0.01 && (st.height - 100.0).abs() < 0.01, "{st:?}");
    assert!((st.web_thickness - 10.0).abs() < 1e-9);
    let back = reviera_dstv::parse(&reviera_dstv::write(st)).unwrap();
    assert_eq!(back.contours[0].points.len(), 5);
}
