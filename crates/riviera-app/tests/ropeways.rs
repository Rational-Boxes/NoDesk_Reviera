// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Aerial ropeways (ARC-VT-027): supports along an alignment, sagging spans, and the clearance,
//! tower and capacity checks.

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
fn a_gondola_line_is_supported_and_checked_span_by_span() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let line = |app: &mut App, pts: Value, radii: Value| ok(app, "alignment.create", json!({ "level": level, "points": pts, "radii": radii }))["alignment"].clone();

    // 600 m across flat ground: terminals at the ends and three towers 150 m apart.
    let a = line(&mut app, json!([[0.0, 0.0], [600.0, 0.0]]), json!([]));
    let r = ok(&mut app, "ropeway.line", json!({ "alignment_id": a }));
    let kinds: Vec<&str> = r["supports"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["terminal", "tower", "tower", "tower", "terminal"], "{r:#}");
    assert_eq!(r["spans"].as_array().unwrap().len(), 4);
    assert_eq!((f(&r["capacity"]), f(&r["trip"])), (2400.0, 100.0));
    // The loaded rope turns down over each tower by its two spans' sags: on the flat between towers
    // each end leaves at 4 × 0.04, so it turns −2 atan 0.16.
    let turns = r["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 3);
    assert!((f(&turns[1]) + 2.0 * 0.16f64.atan()).abs() < 1e-9, "{turns:?}");
    // Between two towers the loaded cabin hangs 18.3 − 6 (sag) − 3.5 m above the ground at mid-span.
    let mid = &r["spans"][1];
    assert!((f(&mid["clearance"]) - (18.3 - 0.04 * 150.0 - 3.5)).abs() < 1e-6 && (f(&mid["required"]) - 2.5).abs() < 1e-9, "{mid:#}");
    // The check reports exactly the spans whose clearance falls short, and the overflight.
    let found = ok(&mut app, "ropeway.check", json!({ "demand": 3000.0 }));
    let short = r["spans"].as_array().unwrap().iter().filter(|s| f(&s["clearance"]) < f(&s["required"])).count();
    let rules: Vec<&str> = found.as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap()).collect();
    assert_eq!(rules.iter().filter(|r| **r == "RM2 clearance").count(), short, "{found:#}");
    assert!(rules.contains(&"overflight") && rules.contains(&"capacity"));

    // A corner is an angle station.
    let bent = line(&mut app, json!([[0.0, 100.0], [300.0, 100.0], [300.0, 400.0]]), json!([0.0]));
    let r2 = ok(&mut app, "ropeway.line", json!({ "alignment_id": bent }));
    let kinds: Vec<&str> = r2["supports"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["terminal", "tower", "angle", "tower", "terminal"], "{r2:#}");
    // Taller towers 300 m apart: the cabins clear the ground better between them, and fly higher.
    let tall = ok(&mut app, "ropeway.line", json!({ "alignment_id": a, "tower_height": 30.0, "tower_spacing": 300.0 }));
    let kinds = tall["supports"].as_array().unwrap().len();
    assert_eq!(kinds, 3, "{tall:#}");
    assert!(f(&tall["spans"][0]["highest"]) > f(&r["spans"][0]["highest"]));
    assert_eq!(ok(&mut app, "ropeway.list", json!({})).as_array().unwrap().len(), 3);

    // A curved alignment is refused: ropeways run in straight legs.
    let curved = line(&mut app, json!([[0.0, 500.0], [300.0, 500.0], [300.0, 800.0]]), json!([100.0]));
    assert!(app.execute("ropeway.line", json!({ "alignment_id": curved }), &Caller::user()).is_err());
}
