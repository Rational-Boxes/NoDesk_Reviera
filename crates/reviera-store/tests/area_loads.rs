// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Area loads and editing loads, cases and combinations (STR-LOAD-001, STR-LC-001): a 5 kPa area
//! load on a 6 × 4 m slab panel carried to the beams across its span by tributary width (10 kN/m
//! to each edge beam; 5, 10 and 5 with a middle beam), the frame's reactions totalling the load,
//! a load in a member's local axes turned to project axes, IFC planar actions, renaming a case
//! through the formulas, and refused circular formulas and deletions in use.

use reviera_core::frame::Load;
use reviera_core::{Caller, CommandRegistry, Document};
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

fn refused(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> String {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_err()
        .code
        .to_string()
}

/// The line loads carried from area loads, by member GlobalId: z per metre.
fn carried(doc: &Document) -> Vec<(String, f64)> {
    let d = reviera_core::structural_model::distill(doc.store()).unwrap();
    let mut out: Vec<(String, f64)> = d
        .model
        .loads
        .iter()
        .filter_map(|l| match l {
            Load::Line {
                member,
                start,
                from_area: Some(_),
                ..
            } => Some((d.key_map[member].clone(), start[2])),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

#[test]
fn area_loads_and_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("area.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let w12 = run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "aisc_w", "designation": "W12X26" }),
    );
    let member = |r: &CommandRegistry, doc: &mut Document, a: [f64; 3], b: [f64; 3]| {
        run(
            r,
            doc,
            "analytical_member.create",
            json!({ "start": a, "end": b, "section_type_id": w12 }),
        )
    };
    // Beams along x at y = 0 and 4 (across the span), edge beams along y (with it).
    let south = member(&r, &mut doc, [0.0, 0.0, 0.0], [6.0, 0.0, 0.0]);
    let north = member(&r, &mut doc, [0.0, 4.0, 0.0], [6.0, 4.0, 0.0]);
    member(&r, &mut doc, [0.0, 0.0, 0.0], [0.0, 4.0, 0.0]);
    member(&r, &mut doc, [6.0, 0.0, 0.0], [6.0, 4.0, 0.0]);
    // The panel's first edge runs along y: it spans along y, onto the beams along x.
    let slab = run(
        &r,
        &mut doc,
        "analytical_panel.create_by_boundary",
        json!({ "boundary": [[0.0, 0.0, 0.0], [0.0, 4.0, 0.0], [6.0, 4.0, 0.0], [6.0, 0.0, 0.0]] }),
    );
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    for n in info["panels"][0]["nodes"].as_array().unwrap() {
        run(
            &r,
            &mut doc,
            "boundary_condition.create",
            json!({ "kind": "point", "host_id": n, "state": "pinned" }),
        );
    }
    let d = run(&r, &mut doc, "load_case.create", json!({ "name": "DL", "nature": "D" }));
    let load = run(
        &r,
        &mut doc,
        "load.create_area",
        json!({ "host_id": slab, "case_id": d, "value": [0.0, 0.0, -5000.0] }),
    );
    assert_eq!(
        refused(&r, &mut doc, "load.create_area", json!({ "host_id": south, "case_id": d, "value": [0.0, 0.0, -1.0] })),
        "load.no_host"
    );
    let guid = |doc: &Document, id: &Value| {
        doc.store()
            .get(reviera_core::ElementId(id.as_u64().unwrap()))
            .unwrap()
            .guid()
            .to_string()
    };
    let c = carried(&doc);
    assert_eq!(c.len(), 2, "{c:?}");
    assert!(c.iter().all(|(_, q)| (q + 10_000.0).abs() < 1e-6), "{c:?}");
    let mut hosts: Vec<String> = c.iter().map(|(g, _)| g.clone()).collect();
    hosts.sort();
    let mut want = vec![guid(&doc, &south), guid(&doc, &north)];
    want.sort();
    assert_eq!(hosts, want);

    // Solved: the reactions total the 120 kN on the slab.
    let distilled = reviera_core::structural_model::distill(doc.store()).unwrap();
    assert!(distilled.model.notes.is_empty(), "{:?}", distilled.model.notes);
    let solved = reviera_core::frame::solve(&distilled.model).unwrap();
    let total: f64 = solved.results[0].reactions.iter().map(|(_, f)| f[2]).sum();
    assert!((total - 120_000.0).abs() < 1.0, "{total}");

    // A middle beam takes half: 5, 10 and 5 kN/m.
    member(&r, &mut doc, [0.0, 2.0, 0.0], [6.0, 2.0, 0.0]);
    let c = carried(&doc);
    let q: Vec<f64> = c.iter().map(|(_, q)| (q / 1000.0 * 10.0).round() / 10.0).collect();
    assert_eq!(q, [-10.0, -5.0, -5.0], "{c:?}");

    // The load's value and case change; the case's nature shows the new one.
    let l2 = run(&r, &mut doc, "load_case.create", json!({ "name": "LL", "nature": "L" }));
    run(
        &r,
        &mut doc,
        "load.set",
        json!({ "load_id": load, "case_id": l2, "value": [0.0, 0.0, -2000.0] }),
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
    assert_eq!((count("DL"), count("LL")), (0, 1));
    let c = carried(&doc);
    assert!((c[0].1 + 4000.0).abs() < 1e-6, "{c:?}");

    // A panel with no members on it: noted, not analysed.
    let loose = run(
        &r,
        &mut doc,
        "analytical_panel.create_by_boundary",
        json!({ "boundary": [[20.0, 0.0, 0.0], [20.0, 2.0, 0.0], [22.0, 2.0, 0.0]] }),
    );
    run(
        &r,
        &mut doc,
        "load.create_area",
        json!({ "host_id": loose, "case_id": d, "value": [0.0, 0.0, -1000.0] }),
    );
    let distilled = reviera_core::structural_model::distill(doc.store()).unwrap();
    assert!(
        distilled.model.notes.iter().any(|n| n.contains("no members")),
        "{:?}",
        distilled.model.notes
    );

    // A load in a column's local axes (z toward x when vertical) acts along project x.
    let column = member(&r, &mut doc, [10.0, 0.0, 0.0], [10.0, 0.0, 3.0]);
    run(
        &r,
        &mut doc,
        "load.create_line",
        json!({ "host_id": column, "case_id": d, "start": [0.0, 0.0, 1000.0], "orientation": "host_local" }),
    );
    let distilled = reviera_core::structural_model::distill(doc.store()).unwrap();
    let local = distilled
        .model
        .loads
        .iter()
        .find_map(|l| match l {
            Load::Line {
                start,
                from_area: None,
                ..
            } if start[0].abs() > 1.0 => Some(*start),
            _ => None,
        })
        .unwrap();
    assert!((local[0] - 1000.0).abs() < 1e-6 && local[2].abs() < 1e-6, "{local:?}");

    // Cases and combinations edited: renaming a case renames it in the formulas.
    let u = run(
        &r,
        &mut doc,
        "load_combination.create",
        json!({ "name": "U1", "formula": "1.2*DL + 1.6*LL" }),
    );
    run(&r, &mut doc, "load_combination.create", json!({ "name": "U2", "formula": "1.0*U1" }));
    run(&r, &mut doc, "load_case.set", json!({ "case_id": l2, "name": "LIVE", "self_weight": false }));
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    let u1 = list["combinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "U1")
        .unwrap()
        .clone();
    assert_eq!(u1["formula"], "1.2*DL + 1.6*LIVE");
    // Circular through U2: refused. Usages and limit state kept.
    assert_eq!(
        refused(&r, &mut doc, "load_combination.set", json!({ "combination_id": u, "formula": "1.0*U2" })),
        "load.circular"
    );
    run(
        &r,
        &mut doc,
        "load_combination.set",
        json!({ "combination_id": u, "limit_state": "serviceability", "usages": ["Gravity", "Gravity", " "] }),
    );
    let list = run(&r, &mut doc, "load_case.list", json!({}));
    let u1 = list["combinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "U1")
        .unwrap()
        .clone();
    assert_eq!(u1["usages"], json!(["Gravity"]));
    assert_eq!(u1["limit_state"], "serviceability");
    // U2 uses U1: it cannot be deleted yet.
    assert_eq!(
        refused(&r, &mut doc, "load_combination.delete", json!({ "combination_id": u })),
        "load.in_use"
    );

    // IFC: planar actions with planar forces, connected to their panels; saved and reopened.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCSTRUCTURALPLANARACTION"), 2);
    assert_eq!(n("IFCSTRUCTURALLOADPLANARFORCE"), 2);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".LOCAL_COORDS."), "the column's local load");
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    let c2 = carried(&doc2);
    assert_eq!(c2.len(), 3, "{c2:?}");
}
