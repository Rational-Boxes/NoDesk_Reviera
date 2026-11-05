// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Inclined elevators (ARC-VT-023): a car on a straight track up a slope, its track an alignment,
//! its landings, and the checks of its incline, drive and accessible car.

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
fn an_inclined_elevator_climbs_its_track_and_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let low = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let high = ok(&mut app, "level.create", json!({ "elevation": 12.0 }));
    let at = |p: [f64; 2], l: &Value| json!({ "point": p, "level": l });
    let make = |app: &mut App, extra: Value| {
        let mut args = json!({ "bottom": at([0.0, 0.0], &low), "top": at([30.0, 0.0], &high) });
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        app.execute("inclined_elevator.create", args, &Caller::user()).map(|o| o.result)
    };

    // The accessible car, commercial, on a double-drum drive: 30 m across and 12 m up.
    let made = make(&mut app, json!({ "size": "ada", "drive": "wdd" })).unwrap();
    let length = 30.0f64.hypot(12.0);
    assert!((f(&made["length"]) - length).abs() < 1e-9 && (f(&made["angle"]) - 12.0f64.atan2(30.0).to_degrees()).abs() < 1e-9, "{made:#}");
    assert!((f(&made["trip"]) - length / 0.25).abs() < 1e-9 && (f(&made["track_width"]) - 1.219).abs() < 1e-9);
    assert!((f(&made["capacity"]) - 2371.0 * 0.453_592_37).abs() < 1e-6);
    // Its track: an alignment from 0 up to 12 m.
    let tracks = ok(&mut app, "alignment.list", json!({}));
    assert_eq!(tracks[0]["elevations"], json!([0.0, 12.0]), "{tracks:#}");
    // A level landing at each gate, and nothing in them; nothing to report.
    let zones = ok(&mut app, "clearance.list", json!({}));
    assert_eq!(zones.as_array().unwrap().iter().filter(|z| z["kind"] == "inclined_landing").count(), 2);
    assert_eq!(ok(&mut app, "clearance.check", json!({})), json!([]));
    assert_eq!(ok(&mut app, "inclined_elevator.check", json!({})), json!([]));

    // On the default winding drum (≤ 2000 lb) the accessible car is too heavy.
    make(&mut app, json!({ "size": "ada" })).unwrap();
    let found = ok(&mut app, "inclined_elevator.check", json!({}));
    assert!(found.as_array().unwrap().iter().any(|x| x["rule"] == "drive" && x["message"].as_str().unwrap().contains("lifts at most")), "{found:#}");
    // A large car on an accessible route: not the accessible car.
    make(&mut app, json!({ "size": "large", "drive": "wdd", "accessible": true })).unwrap();
    let found = ok(&mut app, "inclined_elevator.check", json!({}));
    assert!(found.as_array().unwrap().iter().any(|x| x["rule"].as_str().unwrap().starts_with("ADA")), "{found:#}");
    assert_eq!(ok(&mut app, "inclined_elevator.list", json!({})).as_array().unwrap().len(), 3);

    // 12 m up over 3 m: 76°, a vertical elevator; landings one above the other: refused.
    make(&mut app, json!({ "top": at([3.0, 0.0], &high), "size": "small" })).unwrap();
    let found = ok(&mut app, "inclined_elevator.check", json!({}));
    assert!(found.as_array().unwrap().iter().any(|x| x["rule"] == "A17.1 5.1"), "{found:#}");
    assert!(make(&mut app, json!({ "top": at([0.0, 0.0], &high) })).is_err());
    assert!(make(&mut app, json!({ "top": at([30.0, 0.0], &low) })).is_err());
}
