// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom connections (STR-STL-003): an end plate and an extra plate on the column made into a
//! type, placed on two other joins (one turned a quarter) where it makes both again in the same
//! place relative to the join; four members refused; a mismatched join refused; an exposed plate
//! thickness changed for every instance; the plate removed from every instance after
//! confirmation.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

/// The plates and end plate connections of an instance.
fn parts(doc: &Document, instance: ElementId) -> (Vec<ElementId>, Vec<ElementId>) {
    let g = doc.store().get(instance).unwrap().guid().to_string();
    let mut plates = Vec::new();
    let mut nested = Vec::new();
    for e in doc.store().iter() {
        let Some(t) = reviera_core::custom_connections::part_tag(e) else { continue };
        if t.instance != g {
            continue;
        }
        if t.sub.starts_with("plate:") {
            plates.push(e.id());
        } else if t.sub.starts_with("nested:") {
            nested.push(e.id());
        }
    }
    (plates, nested)
}

fn outline(r: &CommandRegistry, doc: &mut Document, plate: ElementId) -> Vec<[f64; 3]> {
    let i = run(r, doc, "plate.info", json!({ "element_ids": [plate.0] }));
    i[0]["outline"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()
}

#[test]
fn a_custom_connection_is_made_placed_and_edited() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("custom.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w12 = run(&r, &mut doc, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let w18 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W18X35" }));
    let column = |doc: &mut Document, x: f64, y: f64| {
        id(run(&r, doc, "column.place", json!({ "type_id": w12, "point": [x, y], "base_level": l1.0, "top_constraint": { "level": l2.0 } })))
    };
    let beam = |doc: &mut Document, a: [f64; 2], b: [f64; 2]| {
        id(run(&r, doc, "beam.create", json!({ "type_id": w18, "curve": { "start": a, "end": b }, "reference_level": l2.0 })))
    };
    let (c1, c2) = (column(&mut doc, 0.0, 0.0), column(&mut doc, 10.0, 0.0));
    // Turned a quarter, so its beam (along y) meets its web as on the others.
    let c3 = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": w12, "point": [20.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 }, "rotation": std::f64::consts::FRAC_PI_2 }),
    ));
    // Not turned: its beam along y meets its flange.
    let c4 = column(&mut doc, 30.0, 0.0);
    let b1 = beam(&mut doc, [0.0, 0.0], [6.0, 0.0]);
    let b2 = beam(&mut doc, [10.0, 0.0], [16.0, 0.0]);
    // Turned a quarter: the beam leaves along y.
    let b3 = beam(&mut doc, [20.0, 0.0], [20.0, 6.0]);
    let b6 = beam(&mut doc, [30.0, 0.0], [30.0, 6.0]);

    // Join 1: an extended end plate and a plate on the column's far flange.
    let ep = id(run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [c1.0, b1.0], "type_id": "end_plate" })));
    run(&r, &mut doc, "structural_connection.override", json!({ "connection_id": ep.0, "params": { "plate_thickness": 0.025 } }));
    let faces = run(&r, &mut doc, "face.list", json!({ "element_ids": [c1.0] }));
    // The flange facing away from the beam (-x).
    let far = faces
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["face"].as_str().unwrap().starts_with("flange"))
        .min_by(|a, b| a["normal"][0].as_f64().unwrap().total_cmp(&b["normal"][0].as_f64().unwrap()))
        .unwrap()["face"]
        .clone();
    let plate = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "face": { "element": c1.0, "face": far }, "contour": [[3.5, -0.1], [3.9, -0.1], [3.9, 0.1], [3.5, 0.1]], "thickness": 0.01 }),
    ));
    let source = outline(&r, &mut doc, plate);
    let made = run(
        &r,
        &mut doc,
        "structural_connection.customize",
        json!({ "connection_ids": [ep.0], "element_ids": [plate.0], "name": "End plate and doubler" }),
    );
    let ty = made["type_id"].as_str().unwrap().to_owned();
    let i1 = id(made["instance"].clone());
    // What was chosen is the first instance: kept, not copied.
    let (p1, n1) = parts(&doc, i1);
    assert_eq!((p1, n1), (vec![plate], vec![ep]));
    // The name is taken.
    let dup = r.execute(
        &mut doc,
        "structural_connection.customize",
        json!({ "connection_ids": [], "element_ids": [], "name": "End plate and doubler" }),
        &Caller::user(),
    );
    assert_eq!(dup.unwrap_err().code, "custom.name");

    // Placed on join 2: an end plate and the plate, 10 m along x.
    let i2 = id(run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [c2.0, b2.0], "type_id": ty })));
    let (p2, n2) = parts(&doc, i2);
    assert_eq!((p2.len(), n2.len()), (1, 1));
    let info = run(&r, &mut doc, "structural_connection.info", json!({ "connection_ids": [n2[0].0] }))[0].clone();
    assert_eq!(info["connection_type"], "end_plate");
    assert!((info["params"]["plate_thickness"].as_f64().unwrap() - 0.025).abs() < 1e-12, "nested parameters kept");
    for (a, b) in source.iter().zip(outline(&r, &mut doc, p2[0])) {
        assert!((b[0] - a[0] - 10.0).abs() < 1e-6 && (b[1] - a[1]).abs() < 1e-6 && (b[2] - a[2]).abs() < 1e-6, "{a:?} {b:?}");
    }
    assert_eq!(run(&r, &mut doc, "plate.info", json!({ "element_ids": [p2[0].0] }))[0]["hosted"], true);
    // On join 3 the plate turns with the beam: away from the beam is now -y.
    let i3 = id(run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [c3.0, b3.0], "type_id": ty })));
    let (p3, _) = parts(&doc, i3);
    for (a, b) in source.iter().zip(outline(&r, &mut doc, p3[0])) {
        // (x, y) about the column turns to (-y, x): a point at x = -d is at y = -d.
        assert!((b[0] - 20.0 - (-a[1])).abs() < 1e-6 && (b[1] - a[0]).abs() < 1e-6 && (b[2] - a[2]).abs() < 1e-6, "{a:?} {b:?}");
    }
    // A beam meeting the column's flange instead of its web is another join.
    let flange = r
        .execute(&mut doc, "structural_connection.create", json!({ "member_ids": [c4.0, b6.0], "type_id": ty }), &Caller::user())
        .unwrap_err();
    assert_eq!(flange.code, "custom.inputs");
    assert!(flange.message.contains("web"), "{flange:?}");
    // A join of other members is refused.
    let wrong = r.execute(
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [b1.0, c1.0], "type_id": ty }),
        &Caller::user(),
    );
    assert_eq!(wrong.unwrap_err().code, "custom.inputs");

    // Four members: refused.
    let b4 = beam(&mut doc, [0.0, 0.0], [0.0, 6.0]);
    let b5 = beam(&mut doc, [0.0, 0.0], [-6.0, 0.0]);
    let g = id(run(&r, &mut doc, "structural_connection.create", json!({ "member_ids": [c1.0, b1.0, b4.0, b5.0] })));
    let four = r
        .execute(&mut doc, "structural_connection.customize", json!({ "connection_ids": [g.0], "name": "Four" }), &Caller::user())
        .unwrap_err();
    assert_eq!(four.code, "custom.members");
    assert!(four.message.contains("three"), "{four:?}");

    // Expose the plate's thickness and make it 12 mm: every instance follows.
    let list = run(&r, &mut doc, "custom_connection.list", json!({}));
    assert_eq!(list[0]["instances"].as_array().unwrap().len(), 3);
    assert!(list[0]["sub_elements"].as_array().unwrap().contains(&json!("Plate 1")));
    let v = run(&r, &mut doc, "custom_connection.expose_parameter", json!({ "type_id": ty, "path": "Plate 1.thickness" }));
    assert!((v.as_f64().unwrap() - 0.01).abs() < 1e-12);
    let bad = r.execute(&mut doc, "custom_connection.expose_parameter", json!({ "type_id": ty, "path": "Plate 1.colour" }), &Caller::user());
    assert_eq!(bad.unwrap_err().code, "custom.path");
    run(&r, &mut doc, "custom_connection.set_parameter", json!({ "type_id": ty, "path": "Plate 1.thickness", "value": 0.012 }));
    for i in [i1, i2, i3] {
        let (ps, ns) = parts(&doc, i);
        assert_eq!((ps.len(), ns.len()), (1, 1));
        let t = run(&r, &mut doc, "plate.info", json!({ "element_ids": [ps[0].0] }))[0]["thickness"].as_f64().unwrap();
        assert!((t - 0.012).abs() < 1e-12);
    }

    // Removing the plate: confirmation with the count, then gone from all three.
    let (p1, _) = parts(&doc, i1);
    let ask = r
        .execute(&mut doc, "custom_connection.remove_elements", json!({ "type_id": ty, "element_ids": [p1[0].0] }), &Caller::user())
        .unwrap_err();
    assert_eq!(ask.code, "custom.confirm");
    assert_eq!(ask.details["instances"], 3);
    assert!(ask.message.contains('3'));
    let n = run(&r, &mut doc, "custom_connection.remove_elements", json!({ "type_id": ty, "element_ids": [p1[0].0], "confirm": true }));
    assert_eq!(n, 3);
    for i in [i1, i2, i3] {
        let (ps, ns) = parts(&doc, i);
        assert!(ps.is_empty());
        assert_eq!(ns.len(), 1, "the end plate stays");
    }
    // Deleting an instance deletes its parts.
    let (_, ns) = parts(&doc, i2);
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [i2.0] }));
    assert!(doc.store().get(ns[0]).is_none());
}
