// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Automated people movers (ARC-VT-025/026): a guideway along an alignment with its lanes'
//! envelopes, stations with their platforms and trains, and the checks.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn rules(c: &Value) -> Vec<String> {
    c["findings"].as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn an_apm_guideway_with_stations_is_checked_for_curves_grades_and_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // Level, 400 m east then north-east round a 120 m curve.
    let line = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [400.0, 0.0], [700.0, 300.0]], "radii": [120.0] }))["alignment"].clone();
    let g = ok(&mut app, "apm.guideway", json!({ "alignment_id": line }));
    assert!((f(&g["deck_width"]) - (4.75 + 3.66 + 0.61)).abs() < 1e-9 && g["lanes"] == 2, "{g:#}");
    let gid = g["guideway"].clone();
    // A two-car station with side platforms at 1+00: the train and 6 m, 30.4 m long.
    let st = ok(&mut app, "apm.station", json!({ "guideway_id": gid, "station": 100.0, "name": "Terminal" }));
    assert!((f(&st["platform_length"]) - 30.4).abs() < 1e-9 && st["screen_doors"] == true, "{st:#}");
    // The lanes' envelopes are clear; the station's train stands in them by right.
    let zones = ok(&mut app, "clearance.list", json!({}));
    assert_eq!(zones.as_array().unwrap().iter().filter(|z| z["kind"] == "apm_envelope").count(), 2);
    assert_eq!(ok(&mut app, "clearance.check", json!({})), json!([]));

    // 120 s headway, 2 cars of 60: 3600 an hour each way; nothing to report but the demand.
    let c = ok(&mut app, "apm.check", json!({ "demand": 3000.0 }));
    assert_eq!((f(&c["capacity"]), rules(&c).len()), (3600.0, 0), "{c:#}");
    let c = ok(&mut app, "apm.check", json!({ "demand": 8000.0, "headway": 60.0 }));
    assert_eq!(rules(&c), ["headway", "capacity"], "{c:#}");

    // A station on the curve (it starts at about 3+50).
    ok(&mut app, "apm.station", json!({ "guideway_id": gid, "station": 400.0 }));
    assert!(rules(&ok(&mut app, "apm.check", json!({}))).contains(&"station on a curve".to_owned()));
    // A centre platform needs the lanes far enough apart.
    assert!(app.execute("apm.station", json!({ "guideway_id": gid, "station": 200.0, "platform": "centre" }), &Caller::user()).is_err());
    let wide = ok(&mut app, "apm.guideway", json!({ "alignment_id": line, "track_separation": 8.0 }))["guideway"].clone();
    ok(&mut app, "apm.station", json!({ "guideway_id": wide, "station": 200.0, "platform": "centre" }));
    let listed = ok(&mut app, "apm.list", json!({}));
    assert_eq!(listed[0]["stations"].as_array().unwrap().len(), 2, "{listed:#}");

    // Steep and tight: 8 % between stations, 60 m and 40 m curves.
    let steep = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 100.0], [200.0, 100.0], [260.0, 160.0], [260.0, 300.0]], "radii": [60.0, 40.0],
        "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 100.0, "elevation": 8.0 }] }))["alignment"].clone();
    let g2 = ok(&mut app, "apm.guideway", json!({ "alignment_id": steep, "lanes": 1 }))["guideway"].clone();
    let c = ok(&mut app, "apm.check", json!({}));
    let mine: Vec<&Value> = c["findings"].as_array().unwrap().iter().filter(|x| x["element"] == g2).collect();
    let r: Vec<(&str, &str)> = mine.iter().map(|x| (x["rule"].as_str().unwrap(), x["severity"].as_str().unwrap())).collect();
    assert!(r.contains(&("curve radius", "warning")) && r.contains(&("curve radius", "error")) && r.contains(&("grade", "error")), "{c:#}");
}
