// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The analytical model (STR-AM-001, 003, 004, 006): physical to analytical on a frame (members
//! for columns, girders, braces; nodes merged at the joins), nodes following levels, releases,
//! associations, analytical to physical, a member without a physical counterpart, and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, analytical};
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
fn analytical_model_from_a_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("am.ifc");
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
    // A one-bay frame: two columns, a girder, a brace; an architectural column is left out.
    let mut cols = Vec::new();
    for x in [0.0, 6.0] {
        cols.push(id(run(
            &r,
            &mut doc,
            "column.place",
            json!({ "point": [x, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
        )));
    }
    let arch = id(run(
        &r,
        &mut doc,
        "column.place_architectural",
        json!({ "point": [20.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let girder = id(run(
        &r,
        &mut doc,
        "beam.create",
        json!({ "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2.0 }),
    ));
    let brace = id(run(
        &r,
        &mut doc,
        "brace.create",
        json!({ "start_point": [0.0, 0.0], "start_level": l1.0, "end_point": [6.0, 0.0], "end_level": l2.0 }),
    ));

    // Physical to analytical: four members; the architectural column skipped with a reason.
    let report = run(
        &r,
        &mut doc,
        "analytical_automation.physical_to_analytical",
        json!({}),
    );
    assert_eq!(report["created"].as_array().unwrap().len(), 4, "{report:#}");
    let skipped = report["skipped"].as_array().unwrap();
    assert!(
        skipped
            .iter()
            .any(|s| s[0] == arch.0 && s[1].as_str().unwrap().contains("architectural"))
    );
    // Nodes merged at the joins: column feet (2), column heads (2) shared with the girder and brace.
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert_eq!(info["nodes"], 4, "{info:#}");
    let members = info["members"].as_array().unwrap();
    let of = |p: ElementId| {
        members
            .iter()
            .find(|m| m["physical"][0] == p.0)
            .unwrap_or_else(|| panic!("no member for {p}"))
            .clone()
    };
    assert_eq!(of(cols[0])["role"], "column");
    assert_eq!(of(girder)["role"], "beam");
    assert_eq!(of(girder)["predefined_type"], "RIGID_JOINED_MEMBER");
    let b = of(brace);
    assert_eq!(b["role"], "brace");
    assert_eq!(b["predefined_type"], "PIN_JOINED_MEMBER");
    // The girder's ends are the columns' heads.
    let g = of(girder);
    assert!(near(g["start"][2].as_f64().unwrap(), 4.0));
    assert!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| of(cols[0])["nodes"].as_array().unwrap().contains(n)
                || of(cols[1])["nodes"].as_array().unwrap().contains(n))
    );
    // A second run creates nothing: everything is associated.
    let again = run(
        &r,
        &mut doc,
        "analytical_automation.physical_to_analytical",
        json!({}),
    );
    assert!(again["created"].as_array().unwrap().is_empty());
    // Associating an element already associated is refused, naming the association.
    let gm = ElementId(g["id"].as_u64().unwrap());
    let err = r
        .execute(
            &mut doc,
            "analytical_association.add",
            json!({ "physical_ids": [girder.0], "analytical_ids": [gm.0] }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(err.code, "analytical.already_associated");

    // Nodes follow their level: Level 2 up 200 mm, the girder's member follows.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 4.2 }),
    );
    let m = analytical::members(doc.store())
        .into_iter()
        .find(|(i, _)| *i == gm)
        .unwrap()
        .1;
    assert!(near(m.start[2], 4.2) && near(m.end[2], 4.2));

    // Releases: pinned at both ends makes the girder pin-joined.
    run(
        &r,
        &mut doc,
        "analytical_member.set_releases",
        json!({ "member_ids": [gm.0], "end": "both", "preset": "pinned" }),
    );
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    let g = info["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == gm.0)
        .unwrap()
        .clone();
    assert_eq!(g["predefined_type"], "PIN_JOINED_MEMBER");
    assert_eq!(g["releases"][0][3], false);

    // Analytical to physical: the member's section changed to W14X48 changes the girder's type.
    let w14 = id(run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "aisc_w", "designation": "W14X48" }),
    ));
    run(
        &r,
        &mut doc,
        "analytical_member.set",
        json!({ "member_ids": [gm.0], "section_type_id": w14.0 }),
    );
    let rep = run(
        &r,
        &mut doc,
        "analytical_automation.analytical_to_physical",
        json!({}),
    );
    assert!(
        rep["updated"]
            .as_array()
            .unwrap()
            .contains(&json!(girder.0))
    );
    let i = &run(&r, &mut doc, "beam.info", json!({ "beam_ids": [girder.0] }))[0];
    assert_eq!(i["designation"], "W14X48");

    // A member with no physical counterpart, and merging nodes.
    let free = id(run(
        &r,
        &mut doc,
        "analytical_member.create",
        json!({ "start": [30.0, 0.0, 0.0], "end": [30.0, 0.0, 4.2], "role": "column" }),
    ));
    assert!(
        analytical::members(doc.store())
            .iter()
            .any(|(i, _)| *i == free)
    );
    let merged = run(&r, &mut doc, "analytical_node.merge", json!({}));
    assert_eq!(merged["merged"], 0);

    // IFC: members, nodes, their relations and the analysis model; reopened, still there.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCSTRUCTURALCURVEMEMBER"), 5);
    assert_eq!(count("IFCSTRUCTURALPOINTCONNECTION"), 6);
    assert_eq!(count("IFCRELCONNECTSSTRUCTURALMEMBER"), 10);
    assert_eq!(count("IFCSTRUCTURALANALYSISMODEL"), 1);
    assert_eq!(count("IFCRELASSIGNSTOGROUP"), 1);
    assert!(text.contains(".PIN_JOINED_MEMBER.") && text.contains("IFCEDGE"));
    // Saved again unchanged: still one set of relations.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCRELCONNECTSSTRUCTURALMEMBER")
            .count(),
        10
    );
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(analytical::members(doc2.store()).len(), 5);

    // Disconnect: associations go, both models stay; deleting a node deletes its members.
    let gone = run(&r, &mut doc, "analytical_automation.disconnect", json!({}));
    assert_eq!(gone.as_array().unwrap().len(), 4);
    assert_eq!(analytical::members(doc.store()).len(), 5);
    let n = analytical::members(doc.store())
        .into_iter()
        .find(|(i, _)| *i == free)
        .unwrap()
        .1
        .nodes[0];
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [n.0], "confirm_dependents": true }),
    );
    assert_eq!(analytical::members(doc.store()).len(), 4);
}
