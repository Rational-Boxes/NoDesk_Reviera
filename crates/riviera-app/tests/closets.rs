// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Closets (ARC-INT-005): single, double-hang and accessible fittings along a closet's back wall;
//! the accessible rod and shelf within ADA 2010 §308.3.2's 46 in, and reported when raised.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

const IN: f64 = 0.0254;

#[test]
fn closets_hang_their_rods_within_reach() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors", "packs": ["ada_2010_interiors", "practice_interiors"] }));
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } }));
    let store = |app: &App| app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let item = |app: &App, id: &Value| {
        let s = store(app);
        riviera_core::rules::item(&s, s.get(riviera_core::ElementId(id.as_u64().unwrap())).unwrap()).unwrap()
    };

    // A single rod at 66 in, centred 12 in from the back wall; a 12 in shelf over it at 68 in.
    let single = ok(&mut app, "closet.create", json!({ "start": [0.2, 0.1], "end": [1.4, 0.1], "level": level }));
    let rod = item(&app, &single["rods"][0]);
    assert!((rod.z[1] - 66.0 * IN).abs() < 1e-6 && (rod.x[1] - rod.x[0] - 1.2).abs() < 1e-6, "{rod:?}");
    assert!((rod.frame.origin[1] - (0.1 + 12.0 * IN)).abs() < 1e-6, "{:?}", rod.frame.origin);
    let shelf = item(&app, &single["shelves"][0]);
    assert!((shelf.z[1] - 68.0 * IN).abs() < 1e-6 && (shelf.frame.apply([0.0, shelf.back, 0.0])[1] - 0.1).abs() < 1e-6, "{shelf:?}");
    let s = store(&app);
    assert_eq!(riviera_core::categories::category_of(s.get(rod.element).unwrap()).map(|c| c.key), Some("cat.casework"));

    // Double hang: 40 and 80 in, a shelf at 84 in.
    let double = ok(&mut app, "closet.create", json!({ "start": [1.6, 0.1], "end": [2.8, 0.1], "level": level, "layout": "double" }));
    let tops: Vec<f64> = double["rods"].as_array().unwrap().iter().map(|r| item(&app, r).z[1] / IN).collect();
    assert!((tops[0] - 40.0).abs() < 1e-4 && (tops[1] - 80.0).abs() < 1e-4, "{tops:?}");

    // Accessible: a rod at 42 in and a shelf at 46 in, which §308.3.2 allows; neither the
    // practice closets nor these give a finding.
    let acc = ok(&mut app, "closet.create", json!({ "start": [3.0, 0.1], "end": [4.2, 0.1], "level": level, "layout": "accessible" }));
    assert!((item(&app, &acc["shelves"][0]).z[1] - 46.0 * IN).abs() < 1e-6);
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]));
    assert_eq!(ok(&mut app, "overlap.find", json!({}))["overlaps"], json!([]));

    // Raised to 48 in, the accessible shelf is out of reach over the closet's depth.
    let shelf_id = acc["shelves"][0].clone();
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [shelf_id], "offset": 48.0 * IN - 0.75 * IN }));
    let found = ok(&mut app, "rules.check", json!({}));
    assert_eq!(found.as_array().unwrap().len(), 1, "{found:#}");
    assert_eq!((found[0]["kind"].as_str(), found[0]["rule"].as_str()), (Some("surface_height"), Some("ADA 2010 308.3.2")));

    // A rod with no room for hangers under the shelf is refused.
    let e = app.execute("closet.create", json!({ "start": [4.4, 0.1], "end": [5.6, 0.1], "level": level, "rod_heights": [67.0 * IN] }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "closet.no_hanger_room");
}
