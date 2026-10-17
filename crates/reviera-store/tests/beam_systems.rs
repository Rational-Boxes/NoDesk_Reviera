// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Beam systems (STR-BSYS-001): a bay of girders filled automatically, the layout rules, children
//! following a moved support, removing a system (keeping its beams) and deleting one, and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, beams};
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

fn beam(doc: &Document, b: ElementId) -> beams::BeamGeometry {
    beams::all(doc.store())
        .into_iter()
        .find(|(i, _)| *i == b)
        .unwrap()
        .1
}

#[test]
fn beam_systems_fill_bays_and_follow_supports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bs.ifc");
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
    // A 6 m × 9 m bay: four columns and four girders between them at Level 2.
    let corners = [[0.0, 0.0], [6.0, 0.0], [6.0, 9.0], [0.0, 9.0]];
    for c in corners {
        run(
            &r,
            &mut doc,
            "column.place",
            json!({ "point": c, "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        );
    }
    let mut girders = Vec::new();
    for k in 0..4 {
        girders.push(id(run(
            &r,
            &mut doc,
            "beam.create",
            json!({ "curve": { "start": corners[k], "end": corners[(k + 1) % 4] }, "reference_level": l2.0 }),
        )));
    }

    // In the bay, maximum spacing 1.2 m: beams parallel to the nearest support (the 9 m sides),
    // the fewest whose spacing is at most 1.2 m: 4 at 1.2 m.
    let sys = id(run(
        &r,
        &mut doc,
        "beam_system.create_auto",
        json!({ "point": [2.5, 4.5], "level": l2.0, "layout_rule": "maximum_spacing", "value": 1.2 }),
    ));
    let info = |doc: &mut Document| run(&r, doc, "beam_system.info", json!({ "system_id": sys.0 }));
    let i = info(&mut doc);
    assert_eq!(i["beams"].as_array().unwrap().len(), 4, "{i:#}");
    assert!(near(i["spacing"].as_f64().unwrap(), 1.2));
    assert!(near(i["direction"][0].as_f64().unwrap().abs(), 0.0));
    assert_eq!(doc.store().get(sys).unwrap().class, "IfcElementAssembly");
    // Its beams are joists framing into the girders.
    let first = ElementId(i["beams"][0].as_u64().unwrap());
    let bi = &run(&r, &mut doc, "beam.info", json!({ "beam_ids": [first.0] }))[0];
    assert_eq!(bi["usage"], "joist");
    assert_eq!(bi["supports"][0][0], "beam");
    let flange = beam(&doc, girders[0]).ty.width;
    assert!(near(
        bi["length"].as_f64().unwrap(),
        9.0 - flange - 2.0 * beams::STEEL_CUTBACK
    ));
    // With 1.4 m: 6 / 1.4 → 5 spaces, 4 beams at 1.2 m; with 1.5 m: 3 beams at 1.5 m.
    run(
        &r,
        &mut doc,
        "beam_system.set_layout",
        json!({ "system_id": sys.0, "value": 1.5 }),
    );
    let i = info(&mut doc);
    assert_eq!(i["beams"].as_array().unwrap().len(), 3);
    assert!(near(i["spacing"].as_f64().unwrap(), 1.5));

    // Fixed distance 2 m from the beginning: beams at 2 and 4 m; centred 2.5 m: 1.75 and 4.25 m.
    run(
        &r,
        &mut doc,
        "beam_system.set_layout",
        json!({ "system_id": sys.0, "rule": "fixed_distance", "value": 2.0 }),
    );
    let i = info(&mut doc);
    let xs = |doc: &Document, i: &Value| {
        let mut xs: Vec<f64> = i["beams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| beam(doc, ElementId(b.as_u64().unwrap())).a[0])
            .collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    let got = xs(&doc, &i);
    assert!(
        got.len() == 2 && near(got[0], 2.0) && near(got[1], 4.0),
        "{got:?}"
    );
    run(
        &r,
        &mut doc,
        "beam_system.set_layout",
        json!({ "system_id": sys.0, "value": 2.5, "justification": "centre" }),
    );
    let i = info(&mut doc);
    let got = xs(&doc, &i);
    assert!(near(got[0], 1.75) && near(got[1], 4.25), "{got:?}");

    // Fixed number 4, then the girder at x = 6 moves 300 mm: the locked edge follows and the
    // beams re-space in the same regeneration (6.3 / 5).
    run(
        &r,
        &mut doc,
        "beam_system.set_layout",
        json!({ "system_id": sys.0, "rule": "fixed_number", "value": 4 }),
    );
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [girders[1].0], "vector": [0.3, 0.0, 0.0] }),
    );
    let i = info(&mut doc);
    assert!(near(i["spacing"].as_f64().unwrap(), 6.3 / 5.0), "{i:#}");
    let got = xs(&doc, &i);
    assert!(near(got[3], 4.0 * 6.3 / 5.0), "{got:?}");

    // IFC: the system aggregates its beams; reopened, still a system with its beams.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCELEMENTASSEMBLY")
            .count(),
        1
    );
    assert!(text.contains(".BEAM_GRID.") && text.contains("'rxd:beam_system'"));
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let i2 = run(
        &r,
        &mut doc2,
        "beam_system.info",
        json!({ "system_id": sys.0 }),
    );
    assert_eq!(i2["beams"].as_array().unwrap().len(), 4);

    // Remove Beam System: the beams stay, where they were, as independent beams.
    let kept = run(
        &r,
        &mut doc,
        "beam_system.remove",
        json!({ "system_id": sys.0 }),
    );
    assert_eq!(kept.as_array().unwrap().len(), 4);
    assert!(doc.store().get(sys).is_none());
    let k0 = ElementId(kept[0].as_u64().unwrap());
    assert!(
        beams::beam_recipe(doc.store().get(k0).unwrap())
            .unwrap()
            .system
            .is_none()
    );
    assert_eq!(xs(&doc, &json!({ "beams": kept })), got);

    // A sketched system, deleted with element.delete: its beams go with it.
    let n = beams::all(doc.store()).len();
    let s2 = id(run(
        &r,
        &mut doc,
        "beam_system.create_sketch",
        json!({ "boundary": [[20.0, 0.0], [26.0, 0.0], [26.0, 6.0], [20.0, 6.0]], "direction_edge": 1, "layout_rule": "fixed_distance", "value": 1.0, "level": l2.0 }),
    ));
    assert_eq!(beams::all(doc.store()).len(), n + 5);
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [s2.0], "confirm_dependents": true }),
    );
    assert_eq!(beams::all(doc.store()).len(), n);
}
