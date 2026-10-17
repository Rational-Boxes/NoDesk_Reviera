// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Beams (STR-BEAM-001, STR-BEAM-002): placing them on grids between columns, automatic usage
//! (girders, joists, purlins), joins and cutbacks at supports, justification, extensions,
//! disjoining, the modify tools, and IFC.

use reviera_core::beams;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn ids(v: Value) -> Vec<ElementId> {
    v.as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|x| ElementId(x.as_u64().unwrap_or_else(|| panic!("{v}"))))
        .collect()
}

fn geometry(doc: &Document, b: ElementId) -> beams::BeamGeometry {
    beams::all(doc.store())
        .into_iter()
        .find(|(id, _)| *id == b)
        .unwrap()
        .1
}

fn info(r: &CommandRegistry, doc: &mut Document, b: ElementId) -> Value {
    run(r, doc, "beam.info", json!({ "beam_ids": [b.0] })).unwrap()[0].clone()
}

fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn beams_on_grids_usage_joins_and_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" })).unwrap();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })).unwrap());

    // Lettered lines A–C along x, numbered lines 1–4 across them: 12 concrete columns.
    let mut lettered = Vec::new();
    for (k, y) in [0.0, 6.0, 12.0].into_iter().enumerate() {
        lettered.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [-2.0, y], "end": [20.0, y] } }, "name": (["A", "B", "C"][k]) }),
        )
        .unwrap()));
    }
    let mut numbered = Vec::new();
    for (k, x) in [0.0, 6.0, 12.0, 18.0].into_iter().enumerate() {
        numbered.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [x, -2.0], "end": [x, 14.0] } }, "name": (["1", "2", "3", "4"][k]) }),
        )
        .unwrap()));
    }
    let all_grids: Vec<u64> = lettered.iter().chain(&numbered).map(|g| g.0).collect();
    let cols = ids(run(
        &r,
        &mut doc,
        "column.place_at_grids",
        json!({ "grid_ids": all_grids, "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap());
    assert_eq!(cols.len(), 12);

    // On grids: 3 lines, each crossing 4 columns, make 9 beams, one per bay, in one undo step.
    let girders = ids(run(
        &r,
        &mut doc,
        "beam.create_on_grids",
        json!({ "grid_ids": lettered.iter().map(|g| g.0).collect::<Vec<_>>(), "level": l2.0 }),
    )
    .unwrap());
    assert_eq!(girders.len(), 9);
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert!(beams::all(doc.store()).is_empty());
    run(&r, &mut doc, "edit.redo", json!({})).unwrap();
    assert_eq!(beams::all(doc.store()).len(), 9);

    // Each is a W12X26 girder in A992 on two columns, its top at Level 2, stopping 12.5 mm short of
    // each column's face: 6 m − 2 × 200 mm − 2 × 12.5 mm.
    let g0 = girders[0];
    let i = info(&r, &mut doc, g0);
    assert_eq!(i["usage"], "girder");
    assert_eq!(i["usage_locked"], false);
    assert_eq!(i["designation"], "W12X26");
    assert_eq!(i["material"], "Steel ASTM A992");
    assert_eq!(i["supports"][0][0], "column");
    assert_eq!(i["supports"][1][0], "column");
    assert!((i["length"].as_f64().unwrap() - 5.575).abs() < 1e-9, "{i}");
    let g = geometry(&doc, g0);
    let top = g.mesh().0.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let bottom = g.mesh().0.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    assert!((top - 4.0).abs() < 1e-9);
    assert!((top - bottom - g.ty.depth).abs() < 1e-9);
    // A closed, outward body: its volume is the section's area times its length.
    let v = volume(&g.mesh());
    assert!((v - g.ty.area() * 5.575).abs() < 1e-9, "{v}");
    let e = doc.store().get(g0).unwrap();
    assert_eq!(e.class, "IfcBeam");
    assert_eq!(e.param("@ObjectType").unwrap(), "GIRDER");
    assert_eq!(
        e.param("Pset_BeamCommon.LoadBearing").unwrap()["value"],
        true
    );

    // A beam between two girders, across bay 1–2 (along x = 3, from A to B): a joist, stopping at
    // the girders' flanges.
    let joist = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [3.0, 0.0], "end": [3.0, 6.0] }, "reference_level": l2.0 }),
    )
    .unwrap());
    let i = info(&r, &mut doc, joist);
    assert_eq!(i["usage"], "joist");
    assert_eq!(i["supports"][0][0], "beam");
    let flange = geometry(&doc, g0).ty.width;
    let expected = 6.0 - flange - 2.0 * beams::STEEL_CUTBACK;
    assert!(
        (i["length"].as_f64().unwrap() - expected).abs() < 1e-9,
        "{i}"
    );
    assert_eq!(
        doc.store()
            .get(joist)
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "JOIST"
    );

    // A beam framing into the joist: a purlin, made as an IfcMember.
    let purlin = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [3.0, 3.0], "end": [6.0, 3.0] }, "reference_level": l2.0 }),
    )
    .unwrap());
    let i = info(&r, &mut doc, purlin);
    assert_eq!(i["usage"], "purlin");
    let e = doc.store().get(purlin).unwrap();
    assert_eq!(e.class, "IfcMember");
    assert_eq!(e.param("@PredefinedType").unwrap(), "PURLIN");

    // Usage locked by hand, then automatic again.
    run(
        &r,
        &mut doc,
        "beam.set_usage",
        json!({ "beam_ids": [joist.0], "usage": "other" }),
    )
    .unwrap();
    assert_eq!(info(&r, &mut doc, joist)["usage_locked"], true);
    assert_eq!(
        doc.store()
            .get(joist)
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "BEAM"
    );
    run(
        &r,
        &mut doc,
        "beam.set_usage",
        json!({ "beam_ids": [joist.0], "usage": "automatic" }),
    )
    .unwrap();
    assert_eq!(info(&r, &mut doc, joist)["usage"], "joist");

    // z justification top with a −50 mm offset: the top of steel 50 mm below the location line.
    run(
        &r,
        &mut doc,
        "framing.set_justification",
        json!({ "ids": [g0.0], "z_just": "top", "z_offset": -0.05 }),
    )
    .unwrap();
    let top = geometry(&doc, g0)
        .mesh()
        .0
        .iter()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    assert!((top - 3.95).abs() < 1e-9);
    // Left justification: the beam lies to the right of its line (here, below y = 0).
    run(
        &r,
        &mut doc,
        "framing.set_justification",
        json!({ "ids": [g0.0], "y_just": "left" }),
    )
    .unwrap();
    let ys: Vec<f64> = geometry(&doc, g0).mesh().0.iter().map(|p| p[1]).collect();
    assert!(ys.iter().all(|y| *y <= 1e-9));

    // Disjoining an end removes that end's join and cutback only; an extension lengthens it.
    let before = info(&r, &mut doc, g0)["length"].as_f64().unwrap();
    run(
        &r,
        &mut doc,
        "framing.disjoin",
        json!({ "id": g0.0, "end": "start" }),
    )
    .unwrap();
    let after = info(&r, &mut doc, g0)["length"].as_f64().unwrap();
    assert!((after - before - 0.2 - beams::STEEL_CUTBACK).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "framing.set_extension",
        json!({ "id": g0.0, "end": "end", "length": 0.1 }),
    )
    .unwrap();
    assert!((info(&r, &mut doc, g0)["length"].as_f64().unwrap() - after - 0.1).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "framing.join",
        json!({ "id": g0.0, "end": "start" }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "framing.set_cutback",
        json!({ "id": g0.0, "end": "start", "length": 0.0 }),
    )
    .unwrap();
    assert!(
        (info(&r, &mut doc, g0)["length"].as_f64().unwrap() - before - 0.1 - beams::STEEL_CUTBACK)
            .abs()
            < 1e-9
    );

    // A sloped beam: one end 1 m up; horizontal orientation keeps the section upright.
    let sloped = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [30.0, 0.0], "end": [34.0, 0.0] }, "reference_level": l2.0, "end_offset": 1.0, "orientation": "horizontal" }),
    )
    .unwrap());
    let g = geometry(&doc, sloped);
    assert_eq!(info(&r, &mut doc, sloped)["usage"], "other");
    assert!((g.length() - 17f64.sqrt()).abs() < 1e-9);
    let v = volume(&g.mesh());
    // Upright and cut plumb: the section's area times the plan length.
    assert!((v - g.ty.area() * 4.0).abs() < 1e-9, "{v}");

    // The modify tools: copied 2 m up with Level 2 they follow it.
    // One list of copies per vector.
    let copies = ids(run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [sloped.0], "vectors": [[0.0, 3.0, 0.0]] }),
    )
    .unwrap()[0]
        .clone());
    assert_eq!(copies.len(), 1);
    assert!((geometry(&doc, copies[0]).a[1] - 3.0).abs() < 1e-9);
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 4.5 }),
    )
    .unwrap();
    assert!((geometry(&doc, joist).a[2] - 4.5).abs() < 1e-9);

    // A standard section as a beam type, then a beam of it.
    let ipe = id(run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "eu_ipe", "designation": "IPE 300" }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [sloped.0], "type_id": ipe.0 }),
    )
    .unwrap();
    assert_eq!(
        geometry(&doc, sloped).ty.designation.as_deref(),
        Some("IPE 300")
    );

    // IFC: beams and a member with bodies, types, and their usages; reopened, still beams.
    let n = beams::all(doc.store()).len();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCBEAM"), n - 1);
    assert_eq!(count("IFCMEMBER"), 1);
    assert!(count("IFCBEAMTYPE") >= 2);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".PURLIN.") && text.contains("'GIRDER'"));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(beams::all(doc2.store()).len(), n);
    assert_eq!(
        reviera_core::categories::category_of(doc2.store().get(g0).unwrap()).map(|c| c.name),
        Some("Structural Framing")
    );

    // Deleting Level 2 deletes the beams on it.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [l2.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert!(beams::all(doc.store()).is_empty());
}
