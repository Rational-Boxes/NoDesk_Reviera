// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curtain walls (ARC-CW-001 … 006, ARC-WALL-013): grids, panels, mullions, embedding in a
//! wall, and IFC aggregation.

use reviera_core::curtain;
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

#[test]
fn a_curtain_wall_in_a_wall_with_its_parts_in_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 0], "end": [10, 0] } },
            "base_level": l1.0, "top_constraint": { "unconnected": 4.0 },
        }),
    )
    .unwrap());
    // 6 m × 3 m along the wall's line, from x = 2.
    let cw = id(run(
        &r,
        &mut doc,
        "curtain.create",
        json!({
            "curve": { "start": [2, 0], "end": [8, 0] },
            "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
        }),
    )
    .unwrap());
    // The type: three vertical lines and one horizontal (Fixed Number).
    let t = doc
        .store()
        .iter()
        .find(|e| curtain::curtain_type(e).is_some())
        .unwrap()
        .id();
    for (param, value) in [
        ("u_layout", json!("fixed_number")),
        ("u_number", json!(3)),
        ("v_layout", json!("fixed_number")),
        ("v_number", json!(1)),
    ] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": t.0, "param": param, "value": value }),
        )
        .unwrap();
    }
    let g = curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
    assert_eq!(g.u_lines, vec![1.5, 3.0, 4.5]);
    assert_eq!(g.v_lines, vec![1.5]);
    let panels = g.parts.iter().filter(|p| !p.mullion).count();
    assert_eq!(panels, 8);
    // Mullions: 2 border verticals and 3 interior ones in a piece per row (2 rows);
    // horizontals between them, at the base, the transom and the head (4 bays each).
    let mullions = g.parts.len() - panels;
    assert_eq!(mullions, 2 + 3 * 2 + 3 * 4);

    // The wall is cut where the curtain wall runs (6 m × 3 m × 0.2 m less).
    let volume = |doc: &Document| -> f64 {
        reviera_core::bodies::all(doc.store())
            .into_iter()
            .filter(|b| b.element == wall)
            .flat_map(|b| b.parts)
            .map(|p| {
                p.mesh
                    .1
                    .chunks(3)
                    .map(|t| {
                        let [a, b, c] = [
                            p.mesh.0[t[0] as usize],
                            p.mesh.0[t[1] as usize],
                            p.mesh.0[t[2] as usize],
                        ];
                        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                            + a[2] * (b[0] * c[1] - b[1] * c[0]))
                            / 6.0
                    })
                    .sum::<f64>()
            })
            .sum()
    };
    assert!((volume(&doc) - (10.0 * 4.0 - 6.0 * 3.0) * 0.2).abs() < 1e-6);
    // Embedded, the two are not an overlap.
    assert!(reviera_core::bodies::related(doc.store(), cw, wall));

    // Center justification of the vertical lines is an instance setting.
    run(
        &r,
        &mut doc,
        "curtain.set",
        json!({ "curtain_ids": [cw.0], "base_offset": 0.5 }),
    )
    .unwrap();
    let g = curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
    assert_eq!(g.z, [0.5, 3.5]);

    // IFC: one IfcCurtainWall aggregating 8 IfcPlate and its IfcMembers.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |rows: &[reviera_ifc::StepRow], class: &str| {
        rows.iter().filter(|r| r.class == class).count()
    };
    assert_eq!(count(&rows, "IFCCURTAINWALL"), 1);
    assert_eq!(count(&rows, "IFCPLATE"), 8);
    assert_eq!(count(&rows, "IFCMEMBER"), 20);
    // Saved again: the parts are kept, not added.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(count(&rows, "IFCPLATE"), 8);
    // Reopened, the parts are not elements; the curtain wall is.
    drop(project);
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        doc.store()
            .iter()
            .filter(|e| e.class == "IfcPlate" || e.class == "IfcMember")
            .count(),
        0
    );
    assert_eq!(curtain::all(doc.store()).len(), 1);
    // Moved, its parts are rewritten; deleted, they go.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [cw.0], "vector": [0.5, 0.0, 0.0] }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(count(&rows, "IFCPLATE"), 8);
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [cw.0], "confirm_dependents": true }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(count(&rows, "IFCPLATE"), 0);
    assert_eq!(count(&rows, "IFCMEMBER"), 0);
}

#[test]
fn manual_grid_lines_segments_and_doors() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cw.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let cw = id(run(
        &r,
        &mut doc,
        "curtain.create",
        json!({
            "curve": { "start": [0, 0], "end": [6, 0] },
            "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
        }),
    )
    .unwrap());
    let t = doc
        .store()
        .iter()
        .find(|e| curtain::curtain_type(e).is_some())
        .unwrap()
        .id();
    for (param, value) in [
        ("u_layout", json!("fixed_number")),
        ("u_number", json!(1)),
        ("v_layout", json!("none")),
    ] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": t.0, "param": param, "value": value }),
        )
        .unwrap();
    }
    let g =
        |doc: &Document| curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
    let panels = |doc: &Document| {
        g(doc)
            .parts
            .into_iter()
            .filter(|p| !p.mullion)
            .collect::<Vec<_>>()
    };
    assert_eq!(panels(&doc).len(), 2);
    // A manual horizontal line at 1.5 m: four panels.
    let key = run(
        &r,
        &mut doc,
        "curtain.add_grid",
        json!({ "curtain_id": cw.0, "direction": "v", "position": 1.5 }),
    )
    .unwrap();
    assert_eq!(g(&doc).v_lines, vec![1.5]);
    assert_eq!(panels(&doc).len(), 4);
    // Removing the vertical line's lower segment merges the two lower panels into one twice as
    // wide; restoring it splits them again.
    run(&r, &mut doc, "curtain.set_grid_segments", json!({ "curtain_id": cw.0, "direction": "u", "position": 3.0, "segments_on": [false, true] })).unwrap();
    let p = panels(&doc);
    assert_eq!(p.len(), 3);
    let wide = p.iter().find(|q| q.z[0] < 0.5).unwrap();
    assert!(wide.x[1] - wide.x[0] > 5.8, "{wide:?}");
    run(&r, &mut doc, "curtain.set_grid_segments", json!({ "curtain_id": cw.0, "direction": "u", "position": 3.0, "segments_on": [true, true] })).unwrap();
    assert_eq!(panels(&doc).len(), 4);
    // Moving and deleting the manual line.
    run(
        &r,
        &mut doc,
        "curtain.move_grid",
        json!({ "curtain_id": cw.0, "grid_key": key, "position": 2.1 }),
    )
    .unwrap();
    assert_eq!(g(&doc).v_lines, vec![2.1]);
    // A door in the lower left cell: an IfcDoor in the curtain wall, standing on its base.
    run(
        &r,
        &mut doc,
        "curtain.set_panel_type",
        json!({ "curtain_id": cw.0, "cells": [[0, 0]], "panel": "door" }),
    )
    .unwrap();
    let door = panels(&doc)
        .into_iter()
        .find(|q| q.panel == curtain::Panel::Door)
        .unwrap();
    assert!((door.z[0]).abs() < 1e-9 && door.x[0] < 0.1);
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("IFCDOOR(") && text.contains("'Curtain Wall Door"),
        "the door is written"
    );
    run(
        &r,
        &mut doc,
        "curtain.delete_grid",
        json!({ "curtain_id": cw.0, "grid_key": key }),
    )
    .unwrap();
    assert!(g(&doc).v_lines.is_empty());
}

/// A gable roof over x 0..10 m, `y0`..`y0 + 6`, its long sides sloping at 30° from a 3 m plate.
fn gable(r: &CommandRegistry, doc: &mut Document, level: ElementId, y0: f64) -> ElementId {
    let outer = [[0.0, y0], [10.0, y0], [10.0, y0 + 6.0], [0.0, y0 + 6.0]];
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": outer[i], "end": outer[(i + 1) % 4] }, "defines_slope": i % 2 == 0 }))
        .collect();
    id(run(r, doc, "roof.create_footprint", json!({ "curves": curves, "level_id": level.0, "base_offset": 3.0, "slope": "30°" })).unwrap())
}

#[test]
fn a_curtain_wall_attached_to_roofs_fills_the_gables() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("g.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // Two spans side by side, as a gutter-connected greenhouse: one end wall under both.
    let a = gable(&r, &mut doc, l1, 0.0);
    let b = gable(&r, &mut doc, l1, 6.0);
    let cw = id(run(&r, &mut doc, "curtain.create", json!({
        "curve": { "start": [0, 0], "end": [0, 12] }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
    })).unwrap());
    let roof_under = |doc: &Document, roof: ElementId, p: [f64; 2]| {
        reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap().underside(p).unwrap()
    };
    let ridge = roof_under(&doc, a, [0.001, 3.0]);
    let valley = roof_under(&doc, a, [0.001, 6.0]);
    assert!(ridge > valley + 1.5, "{ridge} {valley}");
    for roof in [a, b] {
        run(&r, &mut doc, "wall.attach", json!({ "wall_ids": [cw.0], "target_id": roof.0 })).unwrap();
    }
    let g = curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
    // Laid out up to the ridges, every part under the roofs' underside.
    assert!((g.z[1] - ridge).abs() < 1e-6, "{:?} vs {ridge}", g.z);
    let profile = |y: f64| roof_under(&doc, if y < 6.0 { a } else { b }, [0.001, y.clamp(0.001, 11.999)]);
    let clipped = g.parts.iter().filter(|p| p.outline.is_some()).count();
    assert!(clipped > 4, "the gables clip panels and mullions: {clipped}");
    for p in &g.parts {
        let points: Vec<[f64; 2]> = p.outline.clone().unwrap_or_else(|| vec![[p.x[0], p.z[1]], [p.x[1], p.z[1]]]);
        for q in points {
            assert!(q[1] <= profile(q[0]) + 1e-6, "part {p:?} pokes through the roof at {q:?}");
        }
        let (pos, idx) = g.mesh(p);
        assert!(!pos.is_empty() && idx.len() % 3 == 0 && idx.len() >= 36);
    }
    // Some glass reaches up near the ridge, and none stands over the valley's height there.
    assert!(g.parts.iter().any(|p| !p.mullion && p.z[1] > ridge - 0.3), "glass up into the gable");
    // Detached, it is a plain 3 m wall again.
    run(&r, &mut doc, "wall.detach", json!({ "wall_ids": [cw.0] })).unwrap();
    let g = curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
    assert!((g.z[1] - 3.0).abs() < 1e-9 && g.parts.iter().all(|p| p.outline.is_none()));
    // A wall the roofs do not cover is refused.
    let out = id(run(&r, &mut doc, "curtain.create", json!({
        "curve": { "start": [20, 0], "end": [20, 6] }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
    })).unwrap());
    let err = run(&r, &mut doc, "wall.attach", json!({ "wall_ids": [out.0], "target_id": a.0 })).unwrap_err();
    assert!(format!("{err:?}").contains("wall.attach_not_covered"), "{err:?}");
}
