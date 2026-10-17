// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Loads, load cases and combinations, and boundary conditions (STR-LOAD-001, STR-LC-001,
//! STR-BC-001) on an analytical frame: natures from the code template, cases, combinations
//! with nested and circular formulas, ASCE 7 and EN 1990 generation, point and line loads,
//! supports at the column feet, and IFC.

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

#[test]
fn loads_cases_combinations_and_supports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ld.ifc");
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
    for x in [0.0, 6.0] {
        run(
            &r,
            &mut doc,
            "column.place",
            json!({ "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        );
    }
    let girder = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));
    run(
        &r,
        &mut doc,
        "analytical_automation.physical_to_analytical",
        json!({}),
    );
    // Free ends until the column feet are supported.
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert_eq!(info["free_nodes"].as_array().unwrap().len(), 2);
    let feet = run(
        &r,
        &mut doc,
        "boundary_condition.at_column_bases",
        json!({}),
    );
    assert_eq!(feet.as_array().unwrap().len(), 2);
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert!(info["free_nodes"].as_array().unwrap().is_empty());
    let gm = info["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["physical"][0] == girder.0)
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    // A user condition with a vertical spring.
    let top = info["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["physical"][0] == girder.0)
        .unwrap()["nodes"][1]
        .as_u64()
        .unwrap();
    run(
        &r,
        &mut doc,
        "boundary_condition.create",
        json!({ "kind": "point", "host_id": top, "state": "user", "dof": ["released", "released", { "spring": 5.0e6 }, "released", "released", "released"] }),
    );

    // US natures from the ASCE 7 template; cases.
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    assert_eq!(list["template"], "asce7");
    assert!(
        list["natures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["symbol"] == "Lr")
    );
    let d = id(run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "DL", "nature": "D", "self_weight": true }),
    ));
    let l = id(run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "LL", "nature": "L" }),
    ));
    let s = id(run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "SNOW", "nature": "S" }),
    ));
    let w = id(run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "WIND", "nature": "Wind" }),
    ));
    let refused = |doc: &mut Document, name: &str, args: Value| {
        r.execute(doc, name, args, &Caller::user()).unwrap_err()
    };
    assert_eq!(
        refused(
            &mut doc,
            "load_case.create",
            json!({ "name": "DL", "nature": "D" })
        )
        .code,
        "load.name_taken"
    );
    assert_eq!(
        refused(
            &mut doc,
            "load_case.create",
            json!({ "name": "A+B", "nature": "D" })
        )
        .code,
        "load.bad_name"
    );

    // Combinations: 1.2 DL + 1.6 LL has two factors; nesting expands; a circular one is refused.
    let c1 = id(run(
        &r,
        &mut doc,
        "load_combination.create",
        json!({ "name": "U2", "formula": "1.2*DL + 1.6*LL", "limit_state": "ultimate" }),
    ));
    run(
        &r,
        &mut doc,
        "load_combination.create",
        json!({ "name": "U2S", "formula": "1.0*U2 + 0.5*SNOW" }),
    );
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    let u2s = list["combinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "U2S")
        .unwrap()
        .clone();
    assert_eq!(u2s["factors"].as_array().unwrap().len(), 3);
    assert!(
        u2s["factors"]
            .as_array()
            .unwrap()
            .contains(&json!([l.0, 1.6]))
    );
    assert_eq!(
        refused(
            &mut doc,
            "load_combination.create",
            json!({ "name": "X", "formula": "1.0*X" })
        )
        .code,
        "load.circular"
    );
    assert_eq!(
        refused(
            &mut doc,
            "load_combination.create",
            json!({ "name": "Y", "formula": "1.0*NOPE" })
        )
        .code,
        "load.unknown_name"
    );

    // ASCE 7 generation: 1.4D, the snow and wind combinations; none with roof live or rain.
    let generated = run(&r, &mut doc, "load_combination.generate", json!({}));
    let formulas: Vec<String> = generated["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["formula"].as_str().unwrap().to_owned())
        .collect();
    assert!(formulas.contains(&"1.4*DL".to_owned()), "{formulas:?}");
    assert!(
        formulas.contains(&"1.2*DL + 1.6*LL + 0.5*SNOW".to_owned()),
        "{formulas:?}"
    );
    assert!(
        formulas.contains(&"0.9*DL + 1*WIND".to_owned()),
        "{formulas:?}"
    );
    assert!(formulas.iter().all(|f| !f.contains("Lr")));

    // Loads: a uniform line load on the girder, a point load at mid-span, a trapezoidal one.
    run(
        &r,
        &mut doc,
        "load.create_line",
        json!({ "host_id": gm, "case_id": d.0, "start": [0.0, 0.0, -5000.0] }),
    );
    run(
        &r,
        &mut doc,
        "load.create_point",
        json!({ "host_id": gm, "case_id": l.0, "forces": [0.0, 0.0, -10000.0, 0.0, 0.0, 0.0] }),
    );
    run(
        &r,
        &mut doc,
        "load.create_line",
        json!({ "host_id": gm, "case_id": s.0, "start": [0.0, 0.0, -2000.0], "end": [0.0, 0.0, -4000.0] }),
    );
    run(
        &r,
        &mut doc,
        "load.create_point",
        json!({ "host_id": top, "case_id": w.0, "forces": [3000.0, 0.0, 0.0, 0.0, 0.0, 0.0] }),
    );
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    let count = |name: &str| {
        list["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()["loads"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(
        (count("DL"), count("LL"), count("SNOW"), count("WIND")),
        (1, 1, 1, 1)
    );

    // IFC: actions, applied loads, cases, combinations by factor, supports, the model loaded by.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCSTRUCTURALPOINTACTION"), 2);
    assert_eq!(n("IFCSTRUCTURALCURVEACTION"), 2);
    assert_eq!(n("IFCSTRUCTURALLOADCASE"), 4);
    assert_eq!(n("IFCSTRUCTURALLOADCONFIGURATION"), 1);
    assert_eq!(n("IFCRELCONNECTSSTRUCTURALACTIVITY"), 4);
    // U2: 2 factors; U2S: 3.
    assert_eq!(n("IFCRELASSIGNSTOGROUPBYFACTOR"), 5);
    assert!(
        text.contains("IFCLINEARSTIFFNESSMEASURE(5000000.)"),
        "spring"
    );
    assert!(text.contains(".LINEAR."));
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let list2 = run(&r, &mut doc2, "load_case.list", json!({}));
    assert_eq!(list2["cases"].as_array().unwrap().len(), 4);
    assert_eq!(list2["supports"].as_array().unwrap().len(), 3);

    // Deleting the live case removes its load and takes it out of U2 (and so of U2S).
    let del = run(&r, &mut doc, "load_case.delete", json!({ "case_id": l.0 }));
    assert_eq!(del["loads"], 1);
    assert_eq!(del["combinations"], 1);
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    let u2 = list["combinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == c1.0)
        .unwrap()
        .clone();
    assert_eq!(u2["formula"], "1.2*DL");

    // A European model: EN 1990 6.10 with each variable action leading.
    run(
        &r,
        &mut doc,
        "structural_code.set",
        json!({ "template": "en1990" }),
    );
    run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "G1", "nature": "G" }),
    );
    run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "Q1", "nature": "Q" }),
    );
    run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "S1", "nature": "S" }),
    );
    let generated = run(
        &r,
        &mut doc,
        "load_combination.generate",
        json!({ "template": "en1990", "add": true }),
    );
    let formulas: Vec<String> = generated["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["formula"].as_str().unwrap().to_owned())
        .collect();
    // Q leading: 1.35 G + 1.5 Q + 1.5 × 0.5 S + 1.5 × 0.6 W; S leading: 1.35 G + 1.5 S + 1.5 × 0.7 Q
    // + 0.9 W (the wind case's nature, Wind, is in both templates).
    assert!(
        formulas.contains(&"1.35*G1 + 1.5*Q1 + 0.75*S1 + 0.9*WIND".to_owned()),
        "{formulas:?}"
    );
    assert!(
        formulas.contains(&"1.35*G1 + 1.5*S1 + 1.05*Q1 + 0.9*WIND".to_owned()),
        "{formulas:?}"
    );
    assert!(
        formulas.contains(&"1*G1 + 0.3*Q1".to_owned()),
        "{formulas:?}"
    );
    // The old US cases' natures are no longer in the template: flagged.
    assert!(
        generated["flagged"]
            .as_array()
            .unwrap()
            .contains(&json!("DL"))
    );
    assert!(!generated["created"].as_array().unwrap().is_empty());
}
