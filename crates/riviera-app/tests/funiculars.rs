// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Funiculars (ARC-VT-024): two counterbalanced cars along an alignment, the passing loop, the
//! trip and capacity, and the checks.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn a_funicular_climbs_its_alignment_and_passes_half_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // 300 m east rising 90 m: 30 % the whole way.
    let line = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [300.0, 0.0]], "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 300.0, "elevation": 90.0 }] }))["alignment"].clone();
    let made = ok(&mut app, "funicular.create", json!({ "alignment_id": line }));
    let length = 300.0f64.hypot(90.0);
    assert!((f(&made["length"]) - length).abs() < 1e-6 && (f(&made["rise"]) - 90.0).abs() < 1e-9 && (f(&made["max_grade"]) - 30.0).abs() < 1e-9, "{made:#}");
    // At 5 m/s with 10 s to start and stop: L ÷ 5 + 10 s; 60 a car every trip and a minute's stop.
    let trip = length / 5.0 + 10.0;
    assert!((f(&made["trip"]) - trip).abs() < 1e-6 && (f(&made["capacity"]) - 60.0 * 3600.0 / (trip + 60.0)).abs() < 1e-6);
    // The Abt loop half way, a car and two 20 m switches long.
    assert_eq!(made["passing_loop"], json!([150.0, 52.0]));
    assert_eq!(made["layout"], "abt_loop");

    // Checks: a note with no walkway; a demand beyond its capacity.
    let found = ok(&mut app, "funicular.check", json!({ "demand": 2000.0 }));
    let rules: Vec<&str> = found.as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap()).collect();
    assert!(rules.contains(&"capacity") && rules.contains(&"evacuation"), "{found:#}");

    // Four rails: no loop. With a walkway and within its capacity: nothing to report for it.
    let four = ok(&mut app, "funicular.create", json!({ "alignment_id": line, "layout": "four_rail", "gauge": 1.435, "walkway": true }));
    assert!(four["passing_loop"].is_null() && (f(&four["car_width"]) - 2.635).abs() < 1e-9);
    let found = ok(&mut app, "funicular.check", json!({ "demand": 500.0 }));
    assert!(found.as_array().unwrap().iter().all(|x| x["funicular"] != four["funicular"]), "{found:#}");
    assert_eq!(ok(&mut app, "funicular.list", json!({})).as_array().unwrap().len(), 2);

    // A flat alignment, or one too short for two cars and the loop: refused.
    let flat = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 20.0], [300.0, 20.0]] }))["alignment"].clone();
    assert!(app.execute("funicular.create", json!({ "alignment_id": flat }), &Caller::user()).is_err());
    let short = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 40.0], [60.0, 40.0]], "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 60.0, "elevation": 20.0 }] }))["alignment"].clone();
    assert!(app.execute("funicular.create", json!({ "alignment_id": short }), &Caller::user()).is_err());
}
