// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Escalators and moving walks (ARC-VT-010 to ARC-VT-012): sized from the packs, made with the
//! floor openings their pits and headroom need, clear of everything; a neighbour too close found
//! by the side clearance; a 35° escalator allowed under EN 115-1 for a low rise and flagged under
//! A17.1; horizontal and inclined walks; written as IFC; deleted with their openings.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") })
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// Two levels 5 m apart, each with a 40 × 20 m floor 300 mm thick.
fn mall(app: &mut App) -> Vec<Value> {
    ok(app, "file.new", json!({}));
    let l1 = ok(app, "level.list", json!({}))[0]["id"].clone();
    let l2 = ok(app, "level.create", json!({ "elevation": 5.0 }));
    let c = [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
    let curves: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    for l in [&l1, &l2] {
        ok(app, "floor.create", json!({ "loops": [curves], "level_id": l }));
    }
    vec![l1, l2]
}

fn clean(app: &mut App) {
    let overlaps = ok(app, "overlap.find", json!({}));
    assert_eq!(overlaps["overlaps"], json!([]), "{overlaps:#}");
    let found = ok(app, "clearance.check", json!({}));
    assert_eq!(found, json!([]), "{found:#}");
}

#[test]
fn escalators_pairs_and_walks() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let l = mall(&mut app);

    // Under the IBC (US packs): 30°, 1000 mm steps, 2 flat steps; 1.732 × 5 + 4.70 m long.
    let a = ok(&mut app, "escalator.create", json!({ "point": [5.0, 5.0], "base_level": l[0], "top_level": l[1] }));
    assert!((a["lower_end"].as_f64().unwrap() + a["run"].as_f64().unwrap() + a["upper_end"].as_f64().unwrap() - (5.0 * 3f64.sqrt() + 4.70)).abs() < 1e-9);
    assert_eq!(a["openings"].as_array().unwrap().len(), 2);
    // Its pair alongside, 200 mm between the handrails' outer edges: clear of each one's 100 mm
    // side clearance.
    let pitch = a["handrail_outer"].as_f64().unwrap() + 0.2;
    let b = ok(&mut app, "escalator.create", json!({ "point": [5.0, 5.0 + pitch], "base_level": l[0], "top_level": l[1] }));
    clean(&mut app);
    assert_eq!(ok(&mut app, "escalator.check", json!({})), json!([]));
    ok(&mut app, "escalator.delete", json!({ "escalator": b["escalator"] }));

    // 50 mm apart is too close: within the 100 mm side clearance A17.1 keeps from a handrail.
    let near = ok(&mut app, "escalator.create", json!({ "point": [5.0, 5.0 + a["handrail_outer"].as_f64().unwrap() + 0.05], "base_level": l[0], "top_level": l[1] }));
    let found = ok(&mut app, "clearance.check", json!({}));
    assert!(found.as_array().unwrap().iter().any(|i| i["kind"] == "escalator_headroom" && i["other"] == near["escalator"]), "{found:#}");
    assert_eq!(ok(&mut app, "escalator.delete", json!({ "escalator": near["openings"][0] })), 3);

    // A 35° escalator: allowed by EN 115-1 at 5 m rise and 0.5 m/s, not by A17.1 (a note there,
    // the rule being advisory).
    let steep = ok(&mut app, "escalator.create", json!({ "point": [5.0, 13.0], "base_level": l[0], "top_level": l[1], "inclination": 35 }));
    let f = ok(&mut app, "escalator.check", json!({}));
    assert_eq!(f.as_array().unwrap().iter().map(|x| (x["kind"].as_str().unwrap(), x["severity"].as_str().unwrap())).collect::<Vec<_>>(), [("inclination", "note")], "{f:#}");
    ok(&mut app, "vt.set_rule_packs", json!({ "packs": ["en115", "practice_en"] }));
    assert_eq!(ok(&mut app, "escalator.check", json!({})), json!([]));
    // At 0.65 m/s it would need 30° and 3 flat steps.
    ok(&mut app, "escalator.delete", json!({ "escalator": steep["escalator"] }));
    let fast = ok(&mut app, "escalator.create", json!({ "point": [5.0, 13.0], "base_level": l[0], "top_level": l[1], "inclination": 35, "speed": 0.65, "flat_steps": 2 }));
    let f = ok(&mut app, "escalator.check", json!({ "escalators": [fast["escalator"]] }));
    let kinds: Vec<&str> = f.as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["inclination", "flat_steps"], "{f:#}");
    ok(&mut app, "escalator.delete", json!({ "escalator": fast["escalator"] }));
    // Under EN 115-1 the headroom is 2300 mm: the first escalator's upper opening, made for
    // A17.1's 2130 mm, is now short, and the slab is in its headroom.
    let found = ok(&mut app, "clearance.check", json!({}));
    assert!(found.as_array().unwrap().iter().all(|i| i["kind"] == "escalator_headroom" && i["element"] == a["escalator"]) && !found.as_array().unwrap().is_empty(), "{found:#}");
    ok(&mut app, "vt.set_rule_packs", json!({ "packs": null }));

    // A horizontal walk on the upper floor, and an inclined walk at 12° between the levels.
    let walk = ok(&mut app, "escalator.create", json!({ "kind": "moving_walk", "point": [15.0, 3.0], "base_level": l[1], "length": 20.0, "step_width": 1.4 }));
    assert_eq!((walk["inclination"].as_f64(), walk["openings"].as_array().map(Vec::len)), (Some(0.0), Some(0)));
    let ramp = ok(&mut app, "escalator.create", json!({ "kind": "moving_walk", "point": [5.0, 16.0], "base_level": l[0], "top_level": l[1] }));
    assert_eq!(ramp["inclination"], 12.0);
    clean(&mut app);

    // Written as transport elements of their kinds.
    let path = dir.path().join("mall.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".ESCALATOR.") && text.contains(".MOVINGWALKWAY.") && text.contains("'Reviera_Escalator'"));
    assert_eq!(ok(&mut app, "escalator.list", json!({})).as_array().unwrap().len(), 3);
}

