// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Stairlifts and inclined platform lifts (ARC-VT-013 to ARC-VT-015): a seated lift on a
//! dwelling stair, clear of everything; a platform lift whose head box meets the floor above;
//! the width a stair keeps beside the folded unit by stair use and occupants, and with the unit
//! parked clear; England's platform size; a turning stair takes the curved family; exit stairs;
//! written as IFC; deleted.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") })
}

fn run(app: &mut App, name: &str, args: Value) -> riviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn kinds(f: &Value) -> Vec<(String, String)> {
    f.as_array().unwrap().iter().map(|x| (x["kind"].as_str().unwrap().to_owned(), x["severity"].as_str().unwrap().to_owned())).collect()
}

/// Two levels 3 m apart, each with a 20 × 10 m floor, and a straight 1100 mm stair between them
/// with its opening in the upper floor.
fn house(app: &mut App) -> (Vec<Value>, Value) {
    ok(app, "file.new", json!({}));
    let l1 = ok(app, "level.list", json!({}))[0]["id"].clone();
    let l2 = ok(app, "level.create", json!({ "elevation": 3.0 }));
    let c = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]];
    let curves: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    for l in [&l1, &l2] {
        ok(app, "floor.create", json!({ "loops": [curves], "level_id": l }));
    }
    let stair = ok(app, "stair.create", json!({ "base_level": l1, "width": 1.1, "railings": false, "runs": [{ "start": [4.0, 3.0], "end": [8.5, 3.0] }] }));
    ok(app, "stair.create_openings", json!({ "stair_id": stair }));
    (vec![l1, l2], stair)
}

fn clean(app: &mut App) {
    let overlaps = ok(app, "overlap.find", json!({}));
    assert_eq!(overlaps["overlaps"], json!([]), "{overlaps:#}");
    let found = ok(app, "clearance.check", json!({}));
    assert_eq!(found, json!([]), "{found:#}");
}

#[test]
fn stairlifts_on_straight_and_turning_stairs() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (l, stair) = house(&mut app);
    clean(&mut app);

    // A seated lift on a dwelling's stair (US packs): 1100 − 320 mm folded leaves 780 mm, more
    // than the IBC's 20 in.
    let seat = ok(&mut app, "stairlift.create", json!({ "stair": stair, "side": "left", "stair_use": "dwelling" }));
    assert_eq!((seat["kind"].as_str(), seat["folded"].as_f64()), (Some("seated"), Some(0.32)));
    clean(&mut app);
    assert_eq!(ok(&mut app, "stairlift.check", json!({})), json!([]));
    ok(&mut app, "stairlift.delete", json!({ "stairlift": seat["stairlift"] }));

    // A platform lift on a stair serving 30 people: 1100 − 260 leaves 840 mm, short of 36 in.
    let plat = ok(&mut app, "stairlift.create", json!({ "stair": stair, "side": "right", "kind": "platform", "occupants": 30 }));
    assert_eq!((plat["family"].as_str(), plat["platform"].clone()), (Some("straight"), json!([0.8, 1.25])));
    let f = ok(&mut app, "stairlift.check", json!({}));
    assert_eq!(kinds(&f), [("clear_width".to_owned(), "error".to_owned())], "{f:#}");
    // Its head box, 1524 mm over a deck that rises 0.92 m above the nosings at this angle, meets
    // the upper floor, whose opening was made for the stair's own headroom.
    let found = ok(&mut app, "clearance.check", json!({}));
    assert!(found.as_array().unwrap().iter().any(|i| i["kind"] == "stairlift_run" && i["element"] == plat["stairlift"]), "{found:#}");
    ok(&mut app, "stairlift.delete", json!({ "stairlift": plat["stairlift"] }));

    // Parked clear of the stair, only its 81 mm rail is left beside the stair.
    let clear = ok(&mut app, "stairlift.create", json!({ "stair": stair, "side": "right", "kind": "platform", "occupants": 30, "parks_clear": true }));
    assert_eq!(ok(&mut app, "stairlift.check", json!({})), json!([]));

    // England: an 800 × 1000 platform is short of ADM's 800 × 1250 (the 840 mm left beside it
    // meets the 800 mm escape width).
    ok(&mut app, "vt.set_rule_packs", json!({ "packs": ["england_adm", "practice_en"] }));
    assert_eq!(ok(&mut app, "stairlift.check", json!({})), json!([]));
    ok(&mut app, "stairlift.delete", json!({ "stairlift": clear["stairlift"] }));
    let small = ok(&mut app, "stairlift.create", json!({ "stair": stair, "side": "right", "kind": "platform", "platform": [0.8, 1.0] }));
    let f = ok(&mut app, "stairlift.check", json!({}));
    assert_eq!(kinds(&f), [("platform_size".to_owned(), "error".to_owned())], "{f:#}");
    ok(&mut app, "stairlift.delete", json!({ "stairlift": small["stairlift"] }));
    ok(&mut app, "vt.set_rule_packs", json!({ "packs": null }));

    // A stair that turns at a landing takes the curved family; 1100 mm is short of the 1225 mm
    // its 800 × 1220 platform needs on the inside of a turn (a note: the maker's figure).
    let turning = ok(&mut app, "stair.create", json!({ "base_level": l[0], "width": 1.1, "railings": false, "runs": [{ "start": [12.0, 1.0], "end": [14.24, 1.0] }, { "start": [15.34, 2.1], "end": [15.34, 4.0] }] }));
    let curved = ok(&mut app, "stairlift.create", json!({ "stair": turning, "side": "left", "kind": "platform", "stair_use": "dwelling" }));
    assert_eq!((curved["family"].as_str(), curved["curved"].as_bool()), (Some("curved"), Some(true)));
    let f = ok(&mut app, "stairlift.check", json!({ "stairlifts": [curved["stairlift"]] }));
    assert_eq!(kinds(&f), [("stair_width".to_owned(), "note".to_owned())], "{f:#}");

    // An exit stair: NFPA 101's reading keeps units out of exit enclosures (a note: advisory).
    let exit = ok(&mut app, "stairlift.create", json!({ "stair": stair, "side": "left", "stair_use": "exit" }));
    let f = ok(&mut app, "stairlift.check", json!({ "stairlifts": [exit["stairlift"]] }));
    assert!(kinds(&f).contains(&("exit_enclosure".to_owned(), "note".to_owned())), "{f:#}");

    // Written as user-defined transport elements.
    let path = dir.path().join("stairlifts.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'STAIRLIFT'") && text.contains("'INCLINEDPLATFORMLIFT'") && text.contains("'Riviera_Stairlift'"));
    assert_eq!(ok(&mut app, "stairlift.list", json!({})).as_array().unwrap().len(), 2);
    assert!(run(&mut app, "stairlift.create", json!({ "stair": l[0] })).is_err());
}
