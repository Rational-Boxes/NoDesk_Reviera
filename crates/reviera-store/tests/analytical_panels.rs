// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical panels and openings (STR-AM-002): panels by boundary and by extrusion (a curved one
//! on a cylinder), openings, splitting, cladding, panels for structural walls and floors made by
//! automation and sharing nodes with the frame, the checks over panels, the neutral model, and IFC.

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

fn refused(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> String {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_err()
        .code
        .to_string()
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array(
        (0..4)
            .map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } }))
            .collect(),
    )
}

fn panel(info: &Value, id: u64) -> Value {
    info["panels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("no panel {id} in {info:#}"))
        .clone()
}

fn issues<'a>(report: &'a Value, code: &str, kind: &str) -> Vec<&'a Value> {
    report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == code && i["kind"] == kind)
        .collect()
}

#[test]
fn panels_openings_split_cladding_and_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("panels.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 }));

    // A 6 × 4 m floor panel at 4 m.
    let slab = id(run(
        &r,
        &mut doc,
        "analytical_panel.create_by_boundary",
        json!({ "boundary": [[0.0, 0.0, 4.0], [6.0, 0.0, 4.0], [6.0, 4.0, 4.0], [0.0, 4.0, 4.0]], "thickness": 0.25, "material": "Concrete C30/37" }),
    ));
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    let p = panel(&info, slab.0);
    assert_eq!(p["predefined_type"], "SHELL");
    assert!((p["area"].as_f64().unwrap() - 24.0).abs() < 1e-9);
    assert_eq!(info["nodes"], 4);

    // Refused: not planar, crossing itself, fewer than three corners.
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "analytical_panel.create_by_boundary",
            json!({ "boundary": [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.1], [0.0, 1.0, 0.0]] })
        ),
        "analytical.not_planar"
    );
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "analytical_panel.create_by_boundary",
            json!({ "boundary": [[0.0, 0.0, 0.0], [2.0, 2.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 0.0]] })
        ),
        "analytical.self_intersecting"
    );

    // An opening inside; one outside is refused.
    run(
        &r,
        &mut doc,
        "analytical_opening.create",
        json!({ "panel_id": slab.0, "boundary": [[1.0, 1.0, 4.0], [2.0, 1.0, 4.0], [2.0, 2.0, 4.0], [1.0, 2.0, 4.0]] }),
    );
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "analytical_opening.create",
            json!({ "panel_id": slab.0, "boundary": [[5.0, 1.0, 4.0], [7.0, 1.0, 4.0], [7.0, 2.0, 4.0]] })
        ),
        "analytical.opening_outside"
    );
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert!((panel(&info, slab.0)["area"].as_f64().unwrap() - 23.0).abs() < 1e-9);

    // Split along x = 3: two panels sharing two new nodes; the opening stays on its side.
    let halves = run(
        &r,
        &mut doc,
        "analytical_panel.split",
        json!({ "panel_id": slab.0, "line": [[3.0, -1.0, 4.0], [3.0, 5.0, 4.0]] }),
    );
    let halves: Vec<u64> = halves
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert_eq!(info["nodes"], 6);
    let (a, b) = (panel(&info, halves[0]), panel(&info, halves[1]));
    let areas = [a["area"].as_f64().unwrap(), b["area"].as_f64().unwrap()];
    assert!((areas[0] + areas[1] - 23.0).abs() < 1e-9, "{areas:?}");
    assert_eq!(
        a["openings"].as_u64().unwrap() + b["openings"].as_u64().unwrap(),
        1
    );
    let shared = a["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| b["nodes"].as_array().unwrap().contains(n))
        .count();
    assert_eq!(shared, 2);

    // A curved wall panel by extrusion, on a cylinder of radius 5 m.
    let curved = id(run(
        &r,
        &mut doc,
        "analytical_panel.create_by_extrusion",
        json!({ "profile": { "arc": { "start": [15.0, 0.0], "through": [10.0, 5.0], "end": [5.0, 0.0] } }, "elevation": 0.0, "depth": 4.0, "thickness": 0.3 }),
    ));
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    let c = panel(&info, curved.0);
    assert_eq!(c["curved"], true);
    assert_eq!(c["role"], "wall");
    // Half a circle of radius 5 m, 4 m high: about π × 5 × 4.
    let want = std::f64::consts::PI * 5.0 * 4.0;
    assert!(
        (c["area"].as_f64().unwrap() - want).abs() / want < 0.01,
        "{}",
        c["area"]
    );

    // Cladding.
    run(
        &r,
        &mut doc,
        "analytical_panel.set_cladding",
        json!({ "panel_id": halves[1], "cladding": true }),
    );
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    assert_eq!(panel(&info, halves[1])["predefined_type"], "USERDEFINED");

    // IFC: surface members with faces on a plane and a cylinder, the opening an inner bound,
    // connections to the corner nodes; round trip.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCSTRUCTURALSURFACEMEMBER"), 3);
    assert_eq!(n("IFCCYLINDRICALSURFACE"), 1);
    assert_eq!(n("IFCPLANE"), 2);
    assert_eq!(n("IFCFACEBOUND"), 1);
    assert_eq!(n("IFCFACEOUTERBOUND"), 3);
    assert!(text.contains("'CLADDING'"), "cladding object type");
    assert!(text.contains(".USERDEFINED."));
    // Saving again rewrites the faces rather than piling them up.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter().filter(|r| r.class == "IFCFACESURFACE").count(),
        3
    );
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let info = run(&r, &mut doc2, "analytical_model.info", json!({}));
    assert_eq!(info["panels"].as_array().unwrap().len(), 3);
    assert_eq!(panel(&info, curved.0)["curved"], true);
}

#[test]
fn panels_from_walls_and_floors_and_their_checks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
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
        json!({ "elevation": 3.0 }),
    ));
    // Two bearing walls carrying a structural floor between them, and a column at a corner.
    for y in [0.0, 6.0] {
        run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "curve": { "line": { "start": [0.0, y], "end": [8.0, y] } }, "base_level": l1.0, "top_constraint": { "level": l2.0 }, "structural": true }),
        );
    }
    let f = id(run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [rect([0.0, 0.0], [8.0, 6.0])], "level_id": l2.0 }),
    ));
    run(
        &r,
        &mut doc,
        "floor.set_structural",
        json!({ "floor_ids": [f.0], "structural": true }),
    );
    let report = run(
        &r,
        &mut doc,
        "analytical_automation.physical_to_analytical",
        json!({}),
    );
    assert_eq!(report["created"].as_array().unwrap().len(), 3, "{report:#}");
    let info = run(&r, &mut doc, "analytical_model.info", json!({}));
    let panels = info["panels"].as_array().unwrap();
    assert_eq!(panels.len(), 3);
    let floor = panels.iter().find(|p| p["role"] == "floor").unwrap();
    assert_eq!(floor["physical"], json!([f.0]));
    assert!((floor["area"].as_f64().unwrap() - 48.0).abs() < 1e-6);
    // The walls' tops meet the floor's corners: 4 + 4 nodes for the walls, the floor's shared.
    assert_eq!(info["nodes"], 8, "{info:#}");

    // No supports: S3, and the walls and floor are one isolated part (C2).
    let checks = run(&r, &mut doc, "analytical_model.check", json!({}));
    assert_eq!(issues(&checks, "S3", "no_supports").len(), 1);
    let c2 = issues(&checks, "C2", "isolated_part");
    assert_eq!(c2.len(), 1, "{checks:#}");
    assert_eq!(c2[0]["element_ids"].as_array().unwrap().len(), 3);
    assert!(
        issues(&checks, "C3", "position_mismatch").is_empty(),
        "{checks:#}"
    );
    assert!(issues(&checks, "C6", "wall_slab").is_empty(), "{checks:#}");

    // Supports under the walls clear both.
    let wall_bottoms: Vec<u64> = panels
        .iter()
        .filter(|p| p["role"] == "wall")
        .flat_map(|p| {
            let nodes = p["nodes"].as_array().unwrap().clone();
            let corners = p["corners"].as_array().unwrap().clone();
            nodes
                .into_iter()
                .zip(corners)
                .filter(|(_, c)| c[2].as_f64().unwrap() < 0.01)
                .map(|(n, _)| n.as_u64().unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(wall_bottoms.len(), 4);
    for n in wall_bottoms {
        run(
            &r,
            &mut doc,
            "boundary_condition.create",
            json!({ "kind": "point", "host_id": n, "state": "pinned" }),
        );
    }
    let checks = run(&r, &mut doc, "analytical_model.check", json!({}));
    assert!(
        checks["counts_by_severity"].get("error").is_none(),
        "{checks:#}"
    );
    assert!(issues(&checks, "S1", "unsupported_panel").is_empty());

    // A loose panel with nothing under it: S1.
    let loose = id(run(
        &r,
        &mut doc,
        "analytical_panel.create_by_boundary",
        json!({ "boundary": [[20.0, 0.0, 3.0], [22.0, 0.0, 3.0], [22.0, 2.0, 3.0]] }),
    ));
    let checks = run(&r, &mut doc, "analytical_model.check_supports", json!({}));
    let s1 = issues(&checks, "S1", "unsupported_panel");
    assert_eq!(s1.len(), 1);
    assert_eq!(s1[0]["element_ids"], json!([loose.0]));

    // A floor panel 400 mm below the floor's top, on nodes of its own: C3 against the floor's top,
    // and C6 for both walls, whose panels no longer meet it.
    let floor_id = floor["id"].as_u64().unwrap();
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [floor_id] }),
    );
    let lowered = id(run(
        &r,
        &mut doc,
        "analytical_panel.create_by_boundary",
        json!({ "boundary": [[0.0, 0.0, 2.6], [8.0, 0.0, 2.6], [8.0, 6.0, 2.6], [0.0, 6.0, 2.6]], "thickness": 0.2 }),
    ));
    run(
        &r,
        &mut doc,
        "analytical_association.add",
        json!({ "physical_ids": [f.0], "analytical_ids": [lowered.0] }),
    );
    let checks = run(
        &r,
        &mut doc,
        "analytical_model.check_consistency",
        json!({}),
    );
    let c3 = issues(&checks, "C3", "position_mismatch");
    assert_eq!(c3.len(), 1, "{checks:#}");
    assert!((c3[0]["measured"].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert_eq!(issues(&checks, "C6", "wall_slab").len(), 2, "{checks:#}");
}
