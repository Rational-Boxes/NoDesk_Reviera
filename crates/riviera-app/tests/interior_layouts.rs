// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Room layouts (ARC-INT-001): a template furnished into a rectangular room, its pieces and their
//! use zones (from the rule packs in use) kept in the room and clear of each other; a room too
//! small reported with its shortfall and nothing placed.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// Four 200 mm walls round a `w` × `d` inside from (x, 0); returns the south wall.
fn room(app: &mut App, level: &Value, x: f64, w: f64, d: f64) -> u64 {
    let t = 0.1;
    let mut walls = Vec::new();
    for (a, b) in [([x - t, -t], [x + w + t, -t]), ([x + w + t, -t], [x + w + t, d + t]), ([x + w + t, d + t], [x - t, d + t]), ([x - t, d + t], [x - t, -t])] {
        walls.push(ok(app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } })).as_u64().unwrap());
    }
    walls[0]
}

#[test]
fn rooms_are_furnished_from_templates_with_their_clear_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors", "packs": ["ada_2010_interiors", "practice_interiors"] }));
    let names: Vec<String> = ok(&mut app, "interior.layouts", json!({})).as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
    assert!(names.len() >= 10 && names.contains(&"bedroom_queen".to_owned()), "{names:?}");

    // A 4.0 × 5.0 m bedroom, the bed's head against the south wall: it fits with its ADA spaces.
    let south = room(&mut app, &level, 0.0, 4.0, 5.0);
    let r = ok(&mut app, "interior.layout", json!({ "template": "bedroom_queen", "point": [2.0, 2.5], "level": level, "wall_id": south }));
    assert_eq!(r["placed"].as_array().unwrap().len(), 4, "{r:#}");
    assert_eq!(r["fit"]["length_short"], 0.0);
    assert_eq!(r["packs"], json!(["ada_2010_interiors", "practice_interiors"]));
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]), "nothing in any use zone");
    // The bed's head is at the south wall: its foot faces north, so its ADA spaces lie north of
    // its middle.
    let zones = ok(&mut app, "clearance.list", json!({}));
    let ada: Vec<&Value> = zones.as_array().unwrap().iter().filter(|z| z["rule"] == "ADA 2010 806.2.3 left").collect();
    assert_eq!(ada.len(), 1);
    let (lo, hi) = (ada[0]["min"][1].as_f64().unwrap(), ada[0]["max"][1].as_f64().unwrap());
    let foot = 0.02 + 80.0 * 0.0254 + 0.1;
    assert!((hi - foot).abs() < 1e-6 && (lo - (foot - 48.0 * 0.0254)).abs() < 1e-6, "{lo}..{hi}");
    // Again with replace: the same four pieces, not eight.
    let again = ok(&mut app, "interior.layout", json!({ "template": "bedroom_queen", "point": [2.0, 2.5], "level": level, "wall_id": south, "replace": true }));
    assert_eq!(again["placed"].as_array().unwrap().len(), 4);
    let beds = |app: &mut App| ok(app, "clearance.list", json!({})).as_array().unwrap().iter().filter(|z| z["rule"] == "ADA 2010 806.2.3 left").count();
    assert_eq!(beds(&mut app), 1);

    // A 3.0 × 3.0 m room is too narrow for a queen bed and its two 30 in spaces: nothing placed.
    room(&mut app, &level, 10.0, 3.0, 3.0);
    let small = ok(&mut app, "interior.layout", json!({ "template": "bedroom_queen", "point": [11.5, 1.5], "level": level }));
    assert!(small["placed"].as_array().unwrap().is_empty(), "{small:#}");
    let need = small["fit"]["length_needed"].as_f64().unwrap();
    assert!((need - (60.0 * 0.0254 + 0.1 + 2.0 * 30.0 * 0.0254)).abs() < 0.01 && small["fit"]["length_short"].as_f64().unwrap() > 0.1, "{small:#}");
    assert_eq!(beds(&mut app), 1, "nothing left behind");
    // Fit only: reported, not placed.
    let fit = ok(&mut app, "interior.layout", json!({ "template": "home_office", "point": [11.5, 1.5], "level": level, "fit_only": true }));
    assert!(fit["placed"].as_array().unwrap().is_empty() && fit["fit"]["length_short"] == 0.0, "{fit:#}");

    // Every template fits a 6 × 6 m room and leaves its use zones clear.
    for (k, t) in names.iter().enumerate() {
        let x = 20.0 + 8.0 * k as f64;
        room(&mut app, &level, x, 6.0, 6.0);
        let r = ok(&mut app, "interior.layout", json!({ "template": t, "point": [x + 3.0, 3.0], "level": level }));
        let placed = r["placed"].as_array().unwrap();
        assert!(!placed.is_empty(), "{t}: {r:#}");
        let found = ok(&mut app, "rules.check", json!({ "element_ids": placed }));
        assert_eq!(found, json!([]), "{t}");
    }
}
