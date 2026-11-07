// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Cabinet runs and countertops (ARC-KIT-001/002): a run along a wall filled with standard US
//! widths and a filler, facing away from the wall; wall units 18 in over the counter; a
//! countertop on the base units at 36 in, overhanging 1 in; all Casework, none overlapping.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

const IN: f64 = 0.0254;

#[test]
fn a_run_of_cabinets_is_filled_faced_and_topped() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    ok(&mut app, "code.set", json!({ "code": "irc" }));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // A wall along y = 0, 200 mm thick: its face at y = 0.1.
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [4.0, 0.0] } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } }));
    // 3.6 m of base units: three 36 in, a 33 in, and a 19 mm filler.
    let base = ok(&mut app, "cabinet.run", json!({ "start": [0.2, 0.1], "end": [3.8, 0.1], "level": level, "kind": "base" }));
    assert_eq!(base["region"], "us");
    let widths: Vec<u32> = base["units"].as_array().unwrap().iter().filter(|u| u["filler"] == false).map(|u| (u["width"].as_f64().unwrap() / IN).round() as u32).collect();
    assert_eq!(widths, [36, 36, 36, 33], "{base:#}");
    assert!((base["filler"].as_f64().unwrap() - (3.6 - 141.0 * IN)).abs() < 1e-9);
    // The units face away from the wall: centred 12 in in front of its face.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let first = store.get(riviera_core::ElementId(base["units"][0]["element"].as_u64().unwrap())).unwrap();
    let g = riviera_core::mesh_objects::regenerate(&store, first).unwrap();
    assert!((g.placement.origin[1] - (0.1 + 12.0 * IN)).abs() < 1e-9 && (g.placement.origin[0] - (0.2 + 18.0 * IN)).abs() < 1e-9, "{:?}", g.placement.origin);
    let it = riviera_core::rules::item(&store, first).unwrap();
    assert!((it.z[1] - 34.5 * IN).abs() < 1e-6, "34½ in high: {:?}", it.z);
    assert_eq!(riviera_core::categories::category_of(first).map(|c| c.key), Some("cat.casework"));
    // Wall units over it: 18 in above a 36 in counter.
    let wall = ok(&mut app, "cabinet.run", json!({ "start": [0.2, 0.1], "end": [3.8, 0.1], "level": level, "kind": "wall" }));
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let w0 = riviera_core::rules::item(&store, store.get(riviera_core::ElementId(wall["units"][0]["element"].as_u64().unwrap())).unwrap()).unwrap();
    assert!((w0.z[0] - 54.0 * IN).abs() < 1e-6 && (w0.back - w0.front - 12.0 * IN).abs() < 1e-6);

    // A countertop on the base run: the run's length, 25 in deep, its top at 36 in.
    let tops = ok(&mut app, "countertop.from_runs", json!({ "runs": [base["run"]] }));
    assert_eq!(tops.as_array().unwrap().len(), 1, "{tops:#}");
    assert!((tops[0]["length"].as_f64().unwrap() - 3.6).abs() < 1e-6 && (tops[0]["depth"].as_f64().unwrap() - 25.0 * IN).abs() < 1e-6, "{tops:#}");
    assert!((tops[0]["height"].as_f64().unwrap() - 36.0 * IN).abs() < 1e-6);
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let top = store.get(riviera_core::ElementId(tops[0]["element"].as_u64().unwrap())).unwrap();
    assert_eq!(top.class, "IfcSystemFurnitureElement");
    let t = riviera_core::rules::item(&store, top).unwrap();
    // Its back on the wall face, its front 1 in past the units'.
    let back_y = t.frame.apply([0.0, t.back, 0.0])[1];
    let front_y = t.frame.apply([0.0, t.front, 0.0])[1];
    assert!((back_y - 0.1).abs() < 1e-6 && (front_y - (0.1 + 25.0 * IN)).abs() < 1e-6, "{back_y} {front_y}");
    // Nothing overlaps: not the units, the filler, the top, the wall units or the wall.
    let overlaps = ok(&mut app, "overlap.find", json!({}));
    assert_eq!(overlaps["overlaps"], json!([]), "{overlaps:#}");
    // The runs are listed.
    let runs = ok(&mut app, "cabinet.runs", json!({}));
    assert!(runs.as_array().unwrap().len() >= 2, "{runs:#}");
}
