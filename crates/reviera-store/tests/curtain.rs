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
    // Mullions: 3 interior verticals and 2 border ones; horizontals between them, at the base,
    // the transom and the head (4 bays each).
    let mullions = g.parts.len() - panels;
    assert_eq!(mullions, 5 + 3 * 4);

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
    assert_eq!(count(&rows, "IFCMEMBER"), 17);
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
