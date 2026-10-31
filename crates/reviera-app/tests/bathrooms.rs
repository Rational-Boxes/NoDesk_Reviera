// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Bathroom rule packs and fixture clearance zones (ARC-BATH-001, ARC-BATH-002): packs chosen
//! from the code and accessibility rule set or by name, a project pack replacing a shipped one
//! and saved with the model, IPC spacing between a WC and a lavatory, the ADA WC centreline, its
//! clearance kept free of door swings but not of grab bars, zones following their fixture, and
//! the hand set where no side wall is modelled.

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

/// A new model with a rear wall along y = 0 (the room is at y < 0); returns the level.
fn model(app: &mut App) -> Value {
    ok(app, "file.new", json!({}));
    let level = ok(app, "level.list", json!({}))[0]["id"].clone();
    wall(app, &level, [0.0, 0.0], [6.0, 0.0]);
    level
}

fn wall(app: &mut App, level: &Value, a: [f64; 2], b: [f64; 2]) -> u64 {
    ok(app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).as_u64().unwrap()
}

/// A starter fixture on the rear wall, its centreline at `x`.
fn fixture(app: &mut App, level: &Value, name: &str, x: f64) -> u64 {
    let t = ok(app, "mep_type.load_starters", json!({ "names": [name] }));
    ok(app, "plumbing_fixture.place", json!({ "type_id": t[name], "point": [x, -0.2], "level": level })).as_u64().unwrap()
}

/// A fixture's clear spaces by rule: (min, max) boxes.
fn spaces(app: &mut App, id: u64) -> Vec<(String, [f64; 3], [f64; 3])> {
    let list = ok(app, "clearance.list", json!({ "element_ids": [id] }));
    list.as_array()
        .unwrap()
        .iter()
        .filter(|c| c["kind"] == "fixture_space")
        .map(|c| {
            let v = |k: &str| -> [f64; 3] { serde_json::from_value(c[k].clone()).unwrap() };
            (c["rule"].as_str().unwrap().to_owned(), v("min"), v("max"))
        })
        .collect()
}

fn space(app: &mut App, id: u64, rule: &str) -> ([f64; 3], [f64; 3]) {
    let all = spaces(app, id);
    all.iter().find(|(r, _, _)| r == rule).map(|(_, a, b)| (*a, *b)).unwrap_or_else(|| panic!("no {rule} space in {all:?}"))
}

fn findings(app: &mut App) -> Vec<Value> {
    ok(app, "bath.check", json!({})).as_array().unwrap().clone()
}

#[test]
fn packs_are_chosen_from_the_code_or_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    model(&mut app);
    let in_use = |app: &mut App| -> Vec<String> {
        ok(app, "bath.rule_packs", json!({})).as_array().unwrap().iter().filter(|p| p["in_use"] == true).map(|p| p["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(in_use(&mut app), ["us_plumbing"], "the IBC by default");
    ok(&mut app, "code.set", json!({ "code": "irc" }));
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    assert_eq!(in_use(&mut app), ["us_plumbing", "ada_2010"]);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["us_nkba"] }));
    assert_eq!(in_use(&mut app), ["us_nkba"]);
    assert_eq!(run(&mut app, "bath.set_rule_packs", json!({ "packs": ["nope"] })).unwrap_err().code, "bath.no_pack");
    ok(&mut app, "bath.set_rule_packs", json!({}));
    assert_eq!(in_use(&mut app), ["us_plumbing", "ada_2010"], "automatic again");
}

#[test]
fn a_project_pack_replaces_a_shipped_one_and_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let wc = fixture(&mut app, &level, "Water Closet", 2.0);
    let (a, b) = space(&mut app, wc, "IPC 405.3.1 front");
    assert!((b[1] - a[1] - 0.533).abs() < 1e-6, "{a:?} {b:?}");
    // The UPC's 24 in, as a project's own version of the pack.
    let mut pack = ok(&mut app, "bath.rule_packs", json!({})).as_array().unwrap().iter().find(|p| p["id"] == "us_plumbing").unwrap().clone();
    for k in ["in_use", "shipped"] {
        pack.as_object_mut().unwrap().remove(k);
    }
    pack["zones"][1]["out"][1][1] = json!(0.61);
    assert_eq!(ok(&mut app, "bath.add_rule_pack", json!({ "pack": pack })), "us_plumbing");
    let (a, b) = space(&mut app, wc, "IPC 405.3.1 front");
    assert!((b[1] - a[1] - 0.61).abs() < 1e-6, "{a:?} {b:?}");
    let listed = ok(&mut app, "bath.rule_packs", json!({}));
    assert_eq!(listed.as_array().unwrap().iter().find(|p| p["id"] == "us_plumbing").unwrap()["shipped"], false);
    // A pack that breaks the schema is refused.
    pack["layer"] = json!("mandatory");
    assert_eq!(run(&mut app, "bath.add_rule_pack", json!({ "pack": pack })).unwrap_err().code, "bath.pack_schema");

    let path = dir.path().join("bath.ifc");
    let guid = ok(&mut app, "element.get_ids", json!({ "element_ids": [wc] }))[0]["guid"].clone();
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let mut app = new_app(dir.path());
    ok(&mut app, "file.open", json!({ "path": path }));
    let wc = ok(&mut app, "element.find_by_guid", json!({ "guid": guid })).as_u64().unwrap();
    let (a, b) = space(&mut app, wc, "IPC 405.3.1 front");
    assert!((b[1] - a[1] - 0.61).abs() < 1e-6, "kept with the model: {a:?} {b:?}");
    ok(&mut app, "bath.remove_rule_pack", json!({ "id": "us_plumbing" }));
    let (a, b) = space(&mut app, wc, "IPC 405.3.1 front");
    assert!((b[1] - a[1] - 0.533).abs() < 1e-6, "the shipped pack back");
}

#[test]
fn a_lavatory_too_close_to_a_wc() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let wc = fixture(&mut app, &level, "Water Closet", 2.0);
    let lav = fixture(&mut app, &level, "Lavatory", 2.3);
    let f = findings(&mut app);
    assert!(
        f.iter().any(|x| x["rule"] == "IPC 405.3.1 side" && x["element"] == wc && x["other"] == lav && x["kind"] == "intrusion" && x["severity"] == "error" && x["pack"] == "us_plumbing"),
        "{f:#?}"
    );
    // 800 mm apart: clear.
    ok(&mut app, "element.move", json!({ "element_ids": [lav], "vector": [0.5, 0.0, 0.0] }));
    let f = findings(&mut app);
    assert!(f.is_empty(), "{f:#?}");
    // The zones followed the lavatory.
    let (a, _) = space(&mut app, lav, "IPC 405.3.1 side");
    assert!((a[0] - (2.8 - 0.381)).abs() < 0.01, "{a:?}");
}

#[test]
fn an_ada_wc_its_side_wall_door_and_grab_bar() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["ada_2010"] }));
    // A side wall along x = 0, and the WC 600 mm off its face.
    let side = wall(&mut app, &level, [0.0, 0.0], [0.0, -3.0]);
    let wc = fixture(&mut app, &level, "Water Closet", 0.7);
    let f = findings(&mut app);
    let d = f.iter().find(|x| x["rule"] == "ADA 2010 604.2").unwrap_or_else(|| panic!("{f:#?}"));
    assert_eq!((d["kind"].as_str(), d["element"].as_u64()), (Some("distance"), Some(wc)));
    let m = d["measured"].as_f64().unwrap();
    assert!(m > 0.455 && m < 0.7, "{m}");
    // Moved to 450 mm off the face: in range.
    ok(&mut app, "element.move", json!({ "element_ids": [wc], "vector": [0.45 - m, 0.0, 0.0] }));
    let f = findings(&mut app);
    assert!(!f.iter().any(|x| x["rule"] == "ADA 2010 604.2"), "{f:#?}");
    // Its clearance runs 1525 mm from the side wall's face.
    let (a, b) = space(&mut app, wc, "ADA 2010 604.3.1");
    assert!((b[0] - a[0] - 1.525).abs() < 0.01 && (b[1] - a[1] - 1.42).abs() < 0.01, "{a:?} {b:?}");

    // A grab bar along the side wall may be in it.
    ok(&mut app, "railing.create", json!({ "path": [[0.2, -0.3], [0.2, -1.2]], "base_level": level }));
    let f = findings(&mut app);
    assert!(!f.iter().any(|x| x["kind"] == "intrusion"), "{f:#?}");

    // A door in a wall in front, swinging in: not allowed.
    let front = wall(&mut app, &level, [0.0, -2.2], [3.0, -2.2]);
    let door = ok(&mut app, "door.place", json!({ "host_wall_id": front, "point_along": 1.0 })).as_u64().unwrap();
    let swing = ok(&mut app, "clearance.list", json!({ "element_ids": [door] }));
    let swing = swing.as_array().unwrap().iter().find(|c| c["kind"] == "door_swing").unwrap().clone();
    if swing["max"][1].as_f64().unwrap() < -2.2 {
        ok(&mut app, "insert.flip_facing", json!({ "element_ids": [door] }));
    }
    let f = findings(&mut app);
    assert!(f.iter().any(|x| x["kind"] == "door_swing" && x["other"] == door && x["rule"] == "ADA 2010 604.3.1"), "{f:#?}");
    let c = ok(&mut app, "clearance.check", json!({ "element_ids": [wc] }));
    assert!(c.as_array().unwrap().iter().any(|i| i["other"] == door && i["other_is_swing"] == true), "{c:#}");
    let _ = side;
}

#[test]
fn the_hand_where_no_side_wall_is_modelled() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["ada_2010"] }));
    let wc = fixture(&mut app, &level, "Water Closet", 4.0);
    let f = findings(&mut app);
    assert!(f.iter().any(|x| x["kind"] == "no_side_wall" && x["rule"] == "ADA 2010 604.2"), "{f:#?}");
    // Taken on the left (−x), 455 mm away: the clearance runs from there to the right.
    let (a, b) = space(&mut app, wc, "ADA 2010 604.3.1");
    assert!((a[0] - (4.0 - 0.455)).abs() < 0.01 && (b[0] - (4.0 - 0.455 + 1.525)).abs() < 0.01, "{a:?} {b:?}");
    ok(&mut app, "bath.set_hand", json!({ "element_ids": [wc], "hand": "right" }));
    let (a, b) = space(&mut app, wc, "ADA 2010 604.3.1");
    assert!((b[0] - (4.0 + 0.455)).abs() < 0.01 && (a[0] - (4.0 + 0.455 - 1.525)).abs() < 0.01, "{a:?} {b:?}");
    let wall_id = wall(&mut app, &level, [0.0, -5.0], [6.0, -5.0]);
    assert_eq!(run(&mut app, "bath.set_hand", json!({ "element_ids": [wall_id], "hand": "left" })).unwrap_err().code, "bath.not_a_fixture");
}

#[test]
fn ada_grab_bars_placed_checked_and_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["ada_2010"] }));
    wall(&mut app, &level, [0.0, 0.0], [0.0, -3.0]);
    let wc = fixture(&mut app, &level, "Water Closet", 0.55);
    let kinds = |app: &mut App| -> Vec<(String, String)> {
        findings(app).iter().map(|f| (f["kind"].as_str().unwrap().to_owned(), f["rule"].as_str().unwrap().to_owned())).collect()
    };
    let k = kinds(&mut app);
    assert!(k.contains(&("missing_bar".into(), "ADA 2010 604.5.1".into())) && k.contains(&("missing_bar".into(), "ADA 2010 604.5.2".into())), "{k:?}");

    let placed = ok(&mut app, "bath.place_grab_bars", json!({ "element_ids": [wc] }));
    assert_eq!(placed["placed"].as_array().unwrap().len(), 2, "{placed:#}");
    assert!(placed["skipped"].as_array().unwrap().is_empty(), "{placed:#}");
    let k = kinds(&mut app);
    assert!(!k.iter().any(|(kind, _)| kind == "missing_bar" || kind == "bar"), "{k:?}");
    // They may stand in the WC's clearance.
    assert!(!k.iter().any(|(kind, _)| kind == "intrusion"), "{k:?}");

    // Placed again: replaced, not doubled.
    let again = ok(&mut app, "bath.place_grab_bars", json!({}));
    let first = placed["placed"][0].as_u64().unwrap();
    assert!(run(&mut app, "element.get_ids", json!({ "element_ids": [first] })).is_err(), "the old bar is gone");
    let (side, rear) = (again["placed"][0].as_u64().unwrap(), again["placed"][1].as_u64().unwrap());

    // The side bar lowered to 30 in, the rear bar made 28 mm thick.
    ok(&mut app, "grab_bar.set", json!({ "element_ids": [side], "top": 0.762 }));
    ok(&mut app, "grab_bar.set", json!({ "element_ids": [rear], "diameter": 0.028 }));
    let f = findings(&mut app);
    let bar = |rule: &str| f.iter().find(|x| x["kind"] == "bar" && x["rule"] == rule).unwrap_or_else(|| panic!("{f:#?}"))["message"].as_str().unwrap().to_owned();
    assert!(bar("ADA 2010 604.5.1").contains("top 762 mm up"), "{}", bar("ADA 2010 604.5.1"));
    assert!(bar("ADA 2010 604.5.2").contains("28 mm thick"), "{}", bar("ADA 2010 604.5.2"));

    // Without a side wall, the side bar is skipped with the reason.
    let lone = fixture(&mut app, &level, "Water Closet", 4.5);
    let p = ok(&mut app, "bath.place_grab_bars", json!({ "element_ids": [lone] }));
    assert_eq!(p["placed"].as_array().unwrap().len(), 1, "{p:#}");
    assert!(p["skipped"][0]["reason"].as_str().unwrap().contains("no side wall"), "{p:#}");

    // Drawn by hand: an L on the side wall.
    let l = ok(&mut app, "grab_bar.create", json!({ "level": level, "points": [[0.1, -1.5, 0.9], [0.1, -0.9, 0.9], [0.1, -0.9, 1.5]] })).as_u64().unwrap();
    let path = dir.path().join("bars.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = text.lines().filter(|l| l.contains("IFCRAILING(") && l.contains(".HANDRAIL.")).count();
    assert_eq!(rows, 4, "three placed and one drawn");
    assert!(text.contains("IFCSWEPTDISKSOLID("));
    let _ = l;
}
