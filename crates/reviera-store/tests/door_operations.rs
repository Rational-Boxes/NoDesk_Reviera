// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Door operations (ARC-DOOR-002): double swing, revolving and bi-parting sliding doors,
//! storefront glass leaves and automatic operators: their bodies, plan symbols, clearances,
//! the wall a sliding door takes, and IFC.

use reviera_core::clearances::{self, ClearanceKind, RuleSet};
use reviera_core::inserts::{self, Operation};
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

fn wall(r: &CommandRegistry, doc: &mut Document, y: f64, length: f64) -> ElementId {
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    id(run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, y], "end": [length, y] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3.5 },
        }),
    )
    .unwrap())
}

/// A door type duplicated from the default with `params`.
fn door_type(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    params: &[(&str, Value)],
) -> ElementId {
    let default = doc
        .store()
        .iter()
        .find(|e| e.name.as_deref() == Some(inserts::DEFAULT_DOOR_TYPE))
        .unwrap()
        .id();
    let t = id(run(
        r,
        doc,
        "type.duplicate",
        json!({ "type_id": default.0, "new_name": name }),
    )
    .unwrap());
    for (param, value) in params {
        run(
            r,
            doc,
            "type.set_param",
            json!({ "type_id": t.0, "param": param, "value": value }),
        )
        .unwrap_or_else(|e| panic!("{param}: {e}"));
    }
    t
}

fn placed(doc: &Document, id: ElementId) -> inserts::Placed {
    inserts::resolve(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

fn volume((p, i): &(Vec<[f64; 3]>, Vec<u32>)) -> f64 {
    i.chunks(3)
        .map(|t| {
            let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn double_revolving_sliding_and_storefront_doors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doors.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    // The default door type comes with the first door.
    let w1 = wall(&r, &mut doc, 0.0, 12.0);
    run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": w1.0, "point_along": 1.0 }),
    )
    .unwrap();

    // A pair of glass storefront doors, automatic.
    let pair = door_type(
        &r,
        &mut doc,
        "Storefront Pair 1800",
        &[
            ("width", json!(1.8)),
            ("operation", json!("double_swing")),
            ("panel", json!("Glass (Storefront)")),
            ("automatic", json!(true)),
        ],
    );
    let d = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "type_id": pair.0, "host_wall_id": w1.0, "point_along": 4.0 }),
    )
    .unwrap());
    let p = placed(&doc, d);
    assert_eq!(p.ty.operation, Operation::DoubleSwing);
    assert_eq!(p.leaves().len(), 2);
    // Frame (3), two storefront leaves (5 each: stiles, rails, glass), the operator.
    assert_eq!(p.boxes().len(), 3 + 2 * 5 + 1);
    assert_eq!(p.swing().len(), 2 * 17, "two leaves and their arcs");
    let c = clearances::of_door(&p, RuleSet::Ada2010);
    assert_eq!(
        c.iter()
            .filter(|c| c.kind == ClearanceKind::DoorSwing)
            .count(),
        2
    );
    // Automatic doors have no maneuvering clearances of their own.
    assert!(!c.iter().any(|c| c.kind == ClearanceKind::PullSide));
    let t = doc.store().get(pair).unwrap();
    assert_eq!(
        t.param("Pset_DoorCommon.HasDrive").map(|v| &v["value"]),
        Some(&json!(true))
    );

    // A revolving door 2 m across.
    let rev = door_type(
        &r,
        &mut doc,
        "Revolving 2000",
        &[
            ("width", json!(2.0)),
            ("height", json!(2.4)),
            ("operation", json!("revolving")),
        ],
    );
    let d2 = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "type_id": rev.0, "host_wall_id": w1.0, "point_along": 8.0 }),
    )
    .unwrap());
    let p2 = placed(&doc, d2);
    assert!(p2.boxes().is_empty());
    assert!(!p2.solids().is_empty());
    for (m, _) in p2.meshes() {
        assert!(volume(&m) > 0.0);
    }
    let drum = clearances::of_door(&p2, RuleSet::Ada2010);
    assert_eq!(drum.len(), 1, "the drum only");
    // A revolving door cannot be arched.
    let e = run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": rev.0, "param": "shape", "value": "round_top" }),
    )
    .unwrap_err();
    assert!(e.message.contains("rectangle"), "{e}");

    // An automatic bi-parting sliding door: it takes the wall its leaves slide over.
    let slide = door_type(
        &r,
        &mut doc,
        "Automatic Slider 1800",
        &[
            ("width", json!(1.8)),
            ("operation", json!("sliding")),
            ("panel", json!("glass")),
            ("automatic", json!("true")),
        ],
    );
    let w2 = wall(&r, &mut doc, 6.0, 8.0);
    let d3 = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "type_id": slide.0, "host_wall_id": w2.0, "point_along": 4.0 }),
    )
    .unwrap());
    let p3 = placed(&doc, d3);
    let travel = 0.9 - p3.ty.frame_width + 0.05;
    assert!((p3.slide_travel() - travel).abs() < 1e-9);
    let x = p3.extent();
    assert!(
        (x[0] - (4.0 - 0.9 - travel)).abs() < 1e-9 && (x[1] - (4.0 + 0.9 + travel)).abs() < 1e-9
    );
    let spaces: Vec<_> = clearances::of_door(&p3, RuleSet::None)
        .into_iter()
        .filter(|c| c.kind == ClearanceKind::SlideSpace)
        .collect();
    assert_eq!(spaces.len(), 2);
    // A window in the wall its leaves slide over is refused …
    let e = run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": w2.0, "point_along": 6.0, "fit": false }),
    )
    .unwrap_err();
    assert_eq!(e.code, "insert.overlaps");
    // … and placed near a wall's end, the door slides along until its leaves fit.
    let w3 = wall(&r, &mut doc, 12.0, 6.0);
    let d4 = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "type_id": slide.0, "host_wall_id": w3.0, "point_along": 0.5 }),
    )
    .unwrap());
    let p4 = placed(&doc, d4);
    assert!(p4.extent()[0] >= -1e-9, "{:?}", p4.extent());
    assert!(
        (p4.recipe.along - (0.9 + travel)).abs() < 1e-6,
        "{}",
        p4.recipe.along
    );

    // IFC: each door's operation.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let text: String = rows
        .iter()
        .map(|r| r.attributes.clone())
        .collect::<Vec<_>>()
        .join("\n");
    for op in [
        ".DOUBLE_DOOR_SINGLE_SWING.",
        ".REVOLVING.",
        ".DOUBLE_DOOR_SLIDING.",
        "'HasDrive'",
    ] {
        assert!(text.contains(op), "{op}");
    }
}
