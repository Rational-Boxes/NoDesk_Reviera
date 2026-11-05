// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Alignments (ARC-ALN-001): tangents and curves from points of intersection, chainage and offset,
//! and a vertical profile with parabolic curves.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};
use std::f64::consts::PI;

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn an_l_shaped_alignment_curves_round_its_corner_and_rises_on_its_profile() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let made = ok(&mut app, "alignment.create", json!({
        "level": level, "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], "radii": [30.0],
        "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 100.0, "elevation": 8.0, "curve": 100.0 }, { "station": 200.0, "elevation": 10.0 }]
    }));
    let length = 200.0 - 60.0 + 30.0 * PI / 2.0;
    assert!((f(&made["length"]) - length).abs() < 1e-9 && (f(&made["end_station"]) - length).abs() < 1e-9, "{made:#}");
    let id = made["alignment"].clone();

    // Half way round the curve: 30 m from its centre at (70, 30), heading north-east.
    let mid = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 70.0 + 30.0 * PI / 4.0 }));
    let p = [f(&mid["point"][0]), f(&mid["point"][1])];
    assert!(((p[0] - 70.0).hypot(p[1] - 30.0) - 30.0).abs() < 1e-9, "{mid:#}");
    assert!((f(&mid["direction"][0]) - f(&mid["direction"][1])).abs() < 1e-9 && f(&mid["direction"][0]) > 0.0);
    // 5 m to the left at chainage 50 is (50, 5), and it comes back as chainage 50, offset 5.
    let left = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 50.0, "offset": 5.0 }));
    assert!((f(&left["point"][0]) - 50.0).abs() < 1e-9 && (f(&left["point"][1]) - 5.0).abs() < 1e-9, "{left:#}");
    let back = ok(&mut app, "alignment.station_of", json!({ "alignment_id": id, "point": [50.0, 5.0] }));
    assert!((f(&back["station"]) - 50.0).abs() < 1e-9 && (f(&back["offset"]) - 5.0).abs() < 1e-9, "{back:#}");
    // On the curve, 2 m outside it (to the right of a left turn): offset −2.
    let out = ok(&mut app, "alignment.station_of", json!({ "alignment_id": id, "point": [70.0 + 32.0 * (PI / 4.0).cos(), 30.0 - 32.0 * (PI / 4.0).sin()] }));
    assert!((f(&out["offset"]) + 2.0).abs() < 1e-9 && (f(&out["station"]) - (70.0 + 30.0 * PI / 4.0)).abs() < 1e-6, "{out:#}");

    // The profile: 8 % then 2 %; on the straight grades, and at the PVI below it by A × L ÷ 8.
    let z = |app: &mut App, s: f64| f(&ok(app, "alignment.point_at", json!({ "alignment_id": id, "station": s }))["elevation"]);
    assert!((z(&mut app, 25.0) - 2.0).abs() < 1e-9 && (z(&mut app, 175.0) - 9.5).abs() < 1e-9);
    assert!((z(&mut app, 100.0) - (8.0 - 0.06 * 100.0 / 8.0)).abs() < 1e-9);
    let listed = ok(&mut app, "alignment.list", json!({}));
    assert_eq!(listed[0]["curves"].as_array().unwrap().len(), 1, "{listed:#}");
    assert!((f(&listed[0]["curves"][0]["start_station"]) - 70.0).abs() < 1e-9 && listed[0]["curves"][0]["left"] == true);

    // Moved 10 m east, its curve follows.
    ok(&mut app, "element.move", json!({ "element_ids": [id], "vector": [10.0, 0.0, 0.0] }));
    let mid = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 70.0 + 30.0 * PI / 4.0 }));
    assert!(((f(&mid["point"][0]) - 80.0).hypot(f(&mid["point"][1]) - 30.0) - 30.0).abs() < 1e-9, "{mid:#}");

    // Off the alignment, and a radius too large for its legs: refused.
    assert!(app.execute("alignment.point_at", json!({ "alignment_id": id, "station": 500.0 }), &Caller::user()).is_err());
    assert!(app.execute("alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], "radii": [30.0] }), &Caller::user()).is_err());
}
