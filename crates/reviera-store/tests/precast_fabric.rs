// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom fabric sheets (STR-PRE-004): a slab piece with two openings gets one sheet a face whose
//! wires keep the clearance from each opening; a 9 m × 2.4 m wall piece with a 6 m largest sheet
//! gets two sheets a face lapped by the lap; identical pieces' sheets share a number; sheets are
//! IfcReinforcingMesh held by the assembly.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

/// A sheet's wires (plan or wall-plane points as they are, world metres).
fn wires(doc: &Document, sheet: ElementId) -> Vec<[[f64; 3]; 2]> {
    let e = doc.store().get(sheet).unwrap();
    let r = reviera_core::precast_fabric::recipe(e).unwrap();
    let w = reviera_core::precast_fabric::wires(doc.store(), &r).unwrap();
    w.major.iter().chain(&w.minor).map(|b| [b.points[0], b.points[b.points.len() - 1]]).collect()
}

/// The distance in plan from a segment to a rectangle (0 inside).
fn to_rect(s: [[f64; 3]; 2], r: [f64; 4]) -> f64 {
    (0..=200)
        .map(|k| {
            let t = k as f64 / 200.0;
            let p = [s[0][0] + (s[1][0] - s[0][0]) * t, s[0][1] + (s[1][1] - s[0][1]) * t];
            let dx = (r[0] - p[0]).max(0.0).max(p[0] - r[1]);
            let dy = (r[2] - p[1]).max(0.0).max(p[1] - r[3]);
            dx.hypot(dy)
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn custom_fabric_sheets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fabric.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let rule = run(&r, &mut doc, "precast_config.get", json!({ "path": "fabric_rules" }))[0].clone();
    assert_eq!((rule["id"].clone(), rule["max_length"].clone(), rule["lap"].clone()), (json!("C8-150"), json!(6.0), json!(0.3)));

    // A 2.4 m square slab, one piece, with two 400 mm openings.
    let sq = |x0: f64, y0: f64, s: f64| json!([{ "line": { "start": [x0, y0], "end": [x0 + s, y0] } }, { "line": { "start": [x0 + s, y0], "end": [x0 + s, y0 + s] } }, { "line": { "start": [x0 + s, y0 + s], "end": [x0, y0 + s] } }, { "line": { "start": [x0, y0 + s], "end": [x0, y0] } }]);
    let f = id(&run(&r, &mut doc, "floor.create", json!({ "loops": [sq(0.0, 0.0, 2.4)], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let holes = [[0.5, 0.9, 0.5, 0.9], [1.2, 1.6, 0.7, 1.1]];
    for h in holes {
        run(&r, &mut doc, "opening.create_vertical", json!({ "host_id": f.0, "loops": [[[h[0], h[2]], [h[1], h[2]], [h[1], h[3]], [h[0], h[3]]]] }));
    }
    let slab = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [f.0] })));
    assert_eq!(slab.len(), 1);
    let rep = run(&r, &mut doc, "precast.generate_custom_fabric", json!({ "assembly_ids": [slab[0].0] }));
    let sheets = ids(&rep[0]["sheets"]);
    assert_eq!(sheets.len(), 2, "one sheet at each face: {rep:#}");
    for s in &sheets {
        let ws = wires(&doc, *s);
        assert!(ws.len() > 20);
        // Every wire keeps the 50 mm clearance from both openings; some stop right at it.
        let mut at_clearance = 0;
        for w in &ws {
            for h in holes {
                let d = to_rect(*w, h);
                assert!(d > 0.05 - 1e-6, "a wire {d} m from an opening");
                if (d - 0.05).abs() < 1e-6 {
                    at_clearance += 1;
                }
            }
            // And the 50 mm edge distance from the slab's edges.
            for p in w {
                assert!(p[0] > 0.05 - 1e-6 && p[0] < 2.35 + 1e-6 && p[1] > 0.05 - 1e-6 && p[1] < 2.35 + 1e-6, "{p:?}");
            }
        }
        assert!(at_clearance > 4, "{at_clearance}");
    }
    // Short wires left out, and counted.
    assert!(rep[0]["dropped_wires"].as_u64().unwrap() > 0, "{rep:#}");

    // Two identical 9 m × 2.4 m walls, one piece each (limits raised), a 6 m largest sheet.
    run(&r, &mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_length", "value": 10.0 }));
    run(&r, &mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_weight", "value": 30.0 }));
    let wall = |doc: &mut Document, y: f64| {
        id(&run(&r, doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, y], "end": [9.0, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 2.4 }, "structural": true })))
    };
    let (w1, w2) = (wall(&mut doc, 10.0), wall(&mut doc, 20.0));
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "200 mm" }] }));
    let p1 = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w1.0] })));
    let p2 = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w2.0] })));
    assert_eq!((p1.len(), p2.len()), (1, 1));
    let r1 = run(&r, &mut doc, "precast.generate_custom_fabric", json!({ "assembly_ids": [p1[0].0, p2[0].0] }));
    let s1 = ids(&r1[0]["sheets"]);
    let s2 = ids(&r1[1]["sheets"]);
    assert_eq!(s1.len(), 4, "two sheets a face");
    // Lapped by 300 mm: the first ends 4.65 m along, the second starts at 4.35 m.
    let along = |s: ElementId| {
        let xs: Vec<f64> = wires(&doc, s).iter().flatten().map(|p| p[0]).collect();
        (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
    };
    let (a, b) = (along(s1[0]), along(s1[1]));
    assert!((a.1 - 4.65).abs() < 1e-6 && (b.0 - 4.35).abs() < 1e-6, "{a:?} {b:?}");
    assert!((a.0 - 0.05).abs() < 1e-6 && (b.1 - 8.95).abs() < 1e-6, "edge distance at the wall's ends");
    // The identical pieces' sheets share numbers.
    let n = |s: ElementId| reviera_core::rebar_numbers::number_of(doc.store(), s);
    for (x, y) in s1.iter().zip(&s2) {
        assert_eq!(n(*x), n(*y));
        assert!(!n(*x).is_empty());
    }

    // Running again replaces them.
    let again = run(&r, &mut doc, "precast.generate_custom_fabric", json!({ "assembly_ids": [p1[0].0] }));
    assert_eq!(again[0]["removed"], 4);
    assert!(s1.iter().all(|s| doc.store().get(*s).is_none()));

    // Written as meshes held by their assembly.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.matches("IFCREINFORCINGMESH(").count() >= 2 + 4 + 4);
    let guid = doc.store().get(slab[0]).unwrap().guid().to_string();
    let step = text.lines().find(|l| l.contains(&format!("('{guid}'"))).unwrap().split('=').next().unwrap().trim().to_owned();
    let rel = text.lines().find(|l| l.contains("IFCRELAGGREGATES(") && l.contains(&format!(",{step},("))).unwrap();
    let sheet = doc.store().get(sheets[0]).unwrap().guid().to_string();
    let sheet_step = text.lines().find(|l| l.contains(&format!("('{sheet}'"))).unwrap().split('=').next().unwrap().trim().to_owned();
    assert!(rel.contains(&format!("{sheet_step},")) || rel.contains(&format!("{sheet_step})")), "{rel}");
}
