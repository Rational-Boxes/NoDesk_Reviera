// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel connection types (STR-STL-001, STR-STL-002): a shear plate between a W-beam and a column
//! makes a plate, a bolt group and welds, and its plate grows with the bolt rows; an extended end
//! plate, a pinned base plate with anchor rods and a brace gusset; project and instance
//! parameters; input signatures refused; IFC plates, fasteners and welds aggregated by their
//! connections; deleting a connection deletes its fabrication elements.

use reviera_core::steel_fab::{self, Part};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

const IN: f64 = 0.0254;

/// The info of one connection.
fn info(r: &CommandRegistry, doc: &mut Document, c: ElementId) -> Value {
    run(r, doc, "structural_connection.info", json!({ "connection_ids": [c.0] }))[0].clone()
}

fn count(v: &Value, k: &str) -> usize {
    v[k].as_array().unwrap().len()
}

/// A plate's extent along a direction.
fn extent(part: &Part, dir: [f64; 3]) -> f64 {
    let Part::Plate { outline, .. } = part else {
        panic!("not a plate")
    };
    let d: Vec<f64> = outline.iter().map(|q| q[0] * dir[0] + q[1] * dir[1] + q[2] * dir[2]).collect();
    d.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - d.iter().cloned().fold(f64::INFINITY, f64::min)
}

fn plate_of(doc: &Document, c: &Value) -> Part {
    let p = ElementId(c["plates"][0].as_u64().unwrap());
    steel_fab::all(doc.store())
        .into_iter()
        .find(|(i, _)| *i == p)
        .unwrap()
        .1
}

#[test]
fn steel_connections_make_plates_bolts_and_welds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w12 = run(&r, &mut doc, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let w16 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W16X26" }));
    let hss = run(&r, &mut doc, "brace.load_section", json!({ "catalog": "aisc_hss_rect", "designation": "HSS6X6X3/8" }));
    let cols: Vec<ElementId> = [0.0, 6.0]
        .iter()
        .map(|x| {
            id(run(
                &r,
                &mut doc,
                "column.place",
                json!({ "type_id": w12, "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
            ))
        })
        .collect();
    let beam = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "type_id": w16, "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));
    let brace = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "type_id": hss, "start_point": [0.0, 0.0], "start_level": l1.0, "end_point": [6.0, 0.0], "end_level": l2.0 }),
    ));

    // The library.
    let lib = run(&r, &mut doc, "connection_type.list", json!({}));
    let ids: Vec<&str> = lib.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["shear_plate", "end_plate", "base_plate", "gusset"]);

    // A shear plate: a plate, three bolts, two welds; 3 rows at 3" with 1.5" edges: 9" high.
    let sp = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [cols[0].0, beam.0], "type_id": "shear_plate" }),
    ));
    let i = info(&r, &mut doc, sp);
    assert_eq!((count(&i, "plates"), count(&i, "bolts"), count(&i, "welds")), (1, 3, 2), "{i:#}");
    assert!((extent(&plate_of(&doc, &i), [0.0, 0.0, 1.0]) - 9.0 * IN).abs() < 1e-6);
    let before = i.clone();
    // Four rows: the plate grows to 12", the same element (and the three bolts kept).
    run(
        &r,
        &mut doc,
        "structural_connection.override",
        json!({ "connection_id": sp.0, "params": { "rows": 4 } }),
    );
    let i = info(&r, &mut doc, sp);
    assert_eq!(count(&i, "bolts"), 4);
    assert_eq!(i["plates"], before["plates"]);
    assert_eq!(i["bolts"].as_array().unwrap()[..3], before["bolts"].as_array().unwrap()[..3]);
    assert_eq!(i["override_by_instance"], true);
    assert!((extent(&plate_of(&doc, &i), [0.0, 0.0, 1.0]) - 12.0 * IN).abs() < 1e-6);
    // The plate starts at the column's face: unrotated, the column's flange width lies along x
    // (the beam's direction), so half of 9.995" from its centre.
    let col_depth = 9.995 * IN;
    let Part::Plate { outline, .. } = plate_of(&doc, &i) else { unreachable!() };
    let nearest = outline.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min);
    assert!((nearest - col_depth / 2.0).abs() < 1e-3, "{nearest}");

    // An extended end plate at the other end: one plate, eight bolts, four welds.
    let ep = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [cols[1].0, beam.0], "type_id": "end_plate" }),
    ));
    let i = info(&r, &mut doc, ep);
    assert_eq!((count(&i, "plates"), count(&i, "bolts"), count(&i, "welds")), (1, 8, 4), "{i:#}");

    // A pinned base plate under the first column: a plate, four anchor rods, four welds; the
    // plate 3" beyond the column all round.
    let bp = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [cols[0].0], "type_id": "base_plate" }),
    ));
    let i = info(&r, &mut doc, bp);
    assert_eq!((count(&i, "plates"), count(&i, "bolts"), count(&i, "welds")), (1, 4, 4), "{i:#}");
    let w = extent(&plate_of(&doc, &i), [1.0, 0.0, 0.0]);
    assert!((w - (9.995 * IN + 6.0 * IN)).abs() < 0.01, "{w}");

    // A gusset for the brace at the column.
    let gu = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [cols[0].0, brace.0], "type_id": "gusset" }),
    ));
    let i = info(&r, &mut doc, gu);
    assert_eq!((count(&i, "plates"), count(&i, "bolts"), count(&i, "welds")), (1, 3, 2), "{i:#}");

    // At the brace's top, under the beam framing into the same column: the gusset stays below
    // the beam's bottom flange, and so do its bolts.
    let top = id(run(
        &r,
        &mut doc,
        "structural_connection.create",
        json!({ "member_ids": [cols[1].0, brace.0], "type_id": "gusset" }),
    ));
    let i = info(&r, &mut doc, top);
    let Part::Plate { outline, .. } = plate_of(&doc, &i) else { unreachable!() };
    let beam_bottom = 4.0 - 15.7 * IN;
    let high = outline.iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!(high <= beam_bottom + 0.02, "{high} vs {beam_bottom}");
    let parts = steel_fab::all(doc.store());
    for b in i["bolts"].as_array().unwrap() {
        let (_, p) = parts.iter().find(|(id, _)| id.0 == b.as_u64().unwrap()).unwrap();
        let Part::Bolt { start, .. } = p else { unreachable!() };
        assert!(start[2] < beam_bottom, "a bolt in the beam: {start:?}");
    }
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [top.0] }));

    // Input signatures: a base plate takes a column, a shear plate a beam (not a brace) second.
    let bad = |doc: &mut Document, args: Value| {
        r.execute(doc, "structural_connection.create", args, &Caller::user())
            .unwrap_err()
            .code
            .to_string()
    };
    assert_eq!(bad(&mut doc, json!({ "member_ids": [beam.0], "type_id": "base_plate" })), "connection.inputs");
    assert_eq!(bad(&mut doc, json!({ "member_ids": [cols[0].0, brace.0], "type_id": "shear_plate" })), "connection.inputs");
    // One base plate per column.
    assert_eq!(bad(&mut doc, json!({ "member_ids": [cols[0].0], "type_id": "base_plate" })), "connection.exists");

    // The project's value for a type's parameter reaches its connections (not overridden ones).
    let n = run(
        &r,
        &mut doc,
        "connection_type.edit_parameters",
        json!({ "type_id": "end_plate", "params": { "bolt_diameter": 0.875 * IN } }),
    );
    assert_eq!(n, 1);
    let i = info(&r, &mut doc, ep);
    assert!((i["params"]["bolt_diameter"].as_f64().unwrap() - 0.875 * IN).abs() < 1e-9);

    // IFC: plates, bolts and anchor rods with their nominal sizes, welds; aggregated.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCPLATE"), 4);
    assert_eq!(n("IFCMECHANICALFASTENER"), 4 + 8 + 4 + 3);
    assert_eq!(n("IFCFASTENER"), 2 + 4 + 4 + 2);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".ANCHORBOLT."));
    assert!(text.contains("'rxd:connection'"));
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let i = info(&r, &mut doc2, sp);
    assert_eq!(count(&i, "bolts"), 4, "kept through save and reopen");

    // Deleting a connection deletes its plates, bolts and welds.
    run(&r, &mut doc2, "element.delete", json!({ "element_ids": [sp.0] }));
    let left = steel_fab::all(doc2.store()).len();
    assert_eq!(left, (1 + 8 + 4) + (1 + 4 + 4) + (1 + 3 + 2));
}
