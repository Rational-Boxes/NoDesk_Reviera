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
    wall_h(app, level, a, b, 3.0)
}

fn wall_h(app: &mut App, level: &Value, a: [f64; 2], b: [f64; 2], h: f64) -> u64 {
    ok(app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level, "top_constraint": { "unconnected": h } })).as_u64().unwrap()
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
    assert_eq!(in_use(&mut app), ["us_plumbing", "us_spa"], "the IBC by default");
    ok(&mut app, "code.set", json!({ "code": "irc" }));
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    assert_eq!(in_use(&mut app), ["us_plumbing", "ada_2010", "us_spa"]);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["us_nkba"] }));
    assert_eq!(in_use(&mut app), ["us_nkba"]);
    assert_eq!(run(&mut app, "bath.set_rule_packs", json!({ "packs": ["nope"] })).unwrap_err().code, "bath.no_pack");
    ok(&mut app, "bath.set_rule_packs", json!({}));
    assert_eq!(in_use(&mut app), ["us_plumbing", "ada_2010", "us_spa"], "automatic again");
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

#[test]
fn generic_components_load_place_and_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let types = ok(&mut app, "bath.load_components", json!({}));
    let n = types.as_object().unwrap().len();
    assert!(n >= 24, "{types:#}");
    assert_eq!(ok(&mut app, "bath.load_components", json!({})), types, "loading again reuses them");
    let listed = ok(&mut app, "mesh_object.types", json!({}));
    let size = |name: &str| -> Vec<f64> {
        let t = listed.as_array().unwrap().iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no {name}"));
        t["size"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
    };
    assert!((size("Bath, 1700 × 700")[0] - 1.7).abs() < 1e-9);
    assert!((size("WC, accessible height")[2] - 0.81).abs() < 1e-9);
    assert_eq!(run(&mut app, "bath.load_components", json!({ "names": ["Jacuzzi"] })).unwrap_err().code, "bath.no_component");

    // On the wall: a WC, its outlet 305 mm (12 in) off the wall face, and its clear spaces.
    let wc = ok(&mut app, "bath.place_component", json!({ "type_id": types["WC, close-coupled"], "point": [2.0, -0.3], "level": level })).as_u64().unwrap();
    let ports = ok(&mut app, "connector.list", json!({ "element_id": wc }));
    let waste = ports.as_array().unwrap().iter().find(|p| p["connector"]["name"] == "Waste").unwrap_or_else(|| panic!("{ports:#}"));
    let face = ok(&mut app, "clearance.list", json!({ "element_ids": [wc] })).as_array().unwrap().iter().find(|c| c["rule"] == "IPC 405.3.1 side").unwrap()["max"][1].as_f64().unwrap();
    assert!((face - waste["position"][1].as_f64().unwrap() - 0.305).abs() < 0.002, "{waste} {face}");
    // A mirror over a lavatory, and a hand dryer.
    let mirror = ok(&mut app, "bath.place_component", json!({ "type_id": types["Mirror, 600 × 800"], "point": [3.5, -0.2], "level": level })).as_u64().unwrap();
    ok(&mut app, "bath.place_component", json!({ "type_id": types["Lavatory, wall-hung"], "point": [3.5, -0.3], "level": level }));
    ok(&mut app, "bath.place_component", json!({ "type_id": types["Hand dryer"], "point": [4.5, -0.2], "level": level }));
    // Only bathroom components.
    let other = ok(&mut app, "mep_type.load_starters", json!({ "names": ["Thermostat"] }));
    assert_eq!(run(&mut app, "bath.place_component", json!({ "type_id": other["Thermostat"], "point": [1.0, -0.2], "level": level })).unwrap_err().code, "bath.not_a_component");

    let path = dir.path().join("components.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.lines().any(|l| l.starts_with('#') && l.contains("IFCFURNITURE(") && l.contains("'Mirror'") && l.contains(".USERDEFINED.")), "a mirror as IfcFurniture USERDEFINED 'Mirror'");
    assert!(text.lines().any(|l| l.contains("IFCFURNITURETYPE(") && l.contains("'Mirror'")), "its type's ElementType");
    assert!(text.contains("IFCELECTRICAPPLIANCE(") && text.contains(".HANDDRYER."));
    assert!(text.contains("IFCSANITARYTERMINAL(") && text.contains(".TOILETPAN."));
    let _ = mirror;
}

/// A room closed by four walls whose inner faces are `w` × `l` (x by y), its corner at the origin;
/// returns the level and the walls (south, east, north, west).
fn room(app: &mut App, w: f64, l: f64) -> (Value, [u64; 4]) {
    room_h(app, w, l, 3.0)
}

/// As `room`, its walls `h` high.
fn room_h(app: &mut App, w: f64, l: f64, height: f64) -> (Value, [u64; 4]) {
    ok(app, "file.new", json!({}));
    let level = ok(app, "level.list", json!({}))[0]["id"].clone();
    // The wall thickness, from a probe room's fit.
    let probe = [wall(app, &level, [20.0, 0.0], [23.0, 0.0]), wall(app, &level, [23.0, 0.0], [23.0, 4.0]), wall(app, &level, [23.0, 4.0], [20.0, 4.0]), wall(app, &level, [20.0, 4.0], [20.0, 0.0])];
    let fit = ok(app, "bath.layout", json!({ "template": "cloakroom", "point": [21.5, 2.0], "level": level, "fit_only": true }))["fit"].clone();
    let t = 4.0 - fit["length"].as_f64().unwrap();
    ok(app, "element.delete", json!({ "element_ids": probe }));
    let h = t / 2.0;
    let (x0, y0, x1, y1) = (-h, -h, w + h, l + h);
    let s = wall_h(app, &level, [x0, y0], [x1, y0], height);
    let e = wall_h(app, &level, [x1, y0], [x1, y1], height);
    let n = wall_h(app, &level, [x1, y1], [x0, y1], height);
    let wst = wall_h(app, &level, [x0, y1], [x0, y0], height);
    (level, [s, e, n, wst])
}

fn errors(app: &mut App) -> Vec<Value> {
    findings(app).into_iter().filter(|f| f["severity"] == "error").collect()
}

#[test]
fn a_full_bath_in_five_by_eight_feet() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, walls) = room(&mut app, 1.525, 2.44);
    let out = ok(&mut app, "bath.layout", json!({ "template": "full_bath", "point": [0.76, 1.2], "level": level }));
    assert_eq!(out["placed"].as_array().unwrap().len(), 3, "{out:#}");
    let fit = &out["fit"];
    assert!((fit["length"].as_f64().unwrap() - 2.44).abs() < 0.002 && (fit["depth"].as_f64().unwrap() - 1.525).abs() < 0.002, "{fit}");
    assert_eq!(errors(&mut app), Vec::<Value>::new(), "spaced to the rules");
    // The wet wall is a long side (west or east), and the WC is 15 in or more from the tub.
    let wc = out["placed"][1].as_u64().unwrap();
    let ports = ok(&mut app, "connector.list", json!({ "element_id": wc }));
    let waste = ports.as_array().unwrap().iter().find(|p| p["connector"]["name"] == "Waste").unwrap()["position"].clone();
    let along = waste[1].as_f64().unwrap();
    let tub_edge = 0.76;
    assert!(along - tub_edge >= 0.381 - 0.002 || (2.44 - along) - tub_edge >= 0.381 - 0.002, "{waste}");

    // Laid out again with replace: the same three, not six.
    let again = ok(&mut app, "bath.layout", json!({ "template": "full_bath", "point": [0.76, 1.2], "level": level, "replace": true }));
    assert!(run(&mut app, "element.get_ids", json!({ "element_ids": [wc] })).is_err(), "the earlier WC removed");
    assert_eq!(again["placed"].as_array().unwrap().len(), 3);
    let _ = walls;
}

#[test]
fn too_small_a_room_is_refused_with_the_shortfall() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, _) = room(&mut app, 1.525, 1.8);
    let e = run(&mut app, "bath.layout", json!({ "template": "full_bath", "point": [0.76, 0.9], "level": level })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit");
    assert!(e.message.contains("mm short"), "{}", e.message);
    let short = e.details["length_short"].as_f64().unwrap();
    assert!(short > 0.3 && short < 0.5, "{short}");
    assert!(ok(&mut app, "mesh_object.types", json!({})).as_array().unwrap().iter().all(|t| t["copies"] == 0), "nothing placed");
}

#[test]
fn an_accessible_toilet_with_its_grab_bars() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, walls) = room(&mut app, 1.8, 2.6);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["us_plumbing", "ada_2010"] }));
    let out = ok(&mut app, "bath.layout", json!({ "template": "accessible_single_user", "point": [0.9, 1.3], "level": level, "wet_wall_id": walls[3] }));
    assert_eq!(out["grab_bars"]["placed"].as_array().unwrap().len(), 2, "{out:#}");
    let f = findings(&mut app);
    assert!(!f.iter().any(|x| x["rule"] == "ADA 2010 604.2" || x["kind"] == "missing_bar" || x["kind"] == "bar"), "{f:#?}");
    assert!(!f.iter().any(|x| x["kind"] == "intrusion" && x["severity"] == "error"), "{f:#?}");
}

#[test]
fn a_product_in_place_of_a_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let types = ok(&mut app, "bath.load_components", json!({ "names": ["WC, close-coupled", "Mirror, 600 × 800"] }));
    let wc = ok(&mut app, "bath.place_component", json!({ "type_id": types["WC, close-coupled"], "point": [2.0, -0.3], "level": level })).as_u64().unwrap();
    let guid = ok(&mut app, "element.get_ids", json!({ "element_ids": [wc] }))[0]["guid"].clone();

    let obj = box_obj(dir.path());
    let product = ok(&mut app, "bath.make_product", json!({ "generic": types["WC, close-coupled"], "path": obj, "name": "Acme Model 1", "turn": 90.0, "units": "m", "manufacturer": "Acme", "model_label": "Model 1", "article_number": "A-100" })).as_u64().unwrap();
    let listed = ok(&mut app, "mesh_object.types", json!({}));
    let t = listed.as_array().unwrap().iter().find(|t| t["id"] == product).unwrap();
    assert_eq!(t["category"], "plumbing_fixture");
    let size: Vec<f64> = t["size"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((size[0] - 0.36).abs() < 1e-6 && (size[1] - 0.62).abs() < 1e-6, "turned: {size:?}");
    let ports = ok(&mut app, "connector.list", json!({ "element_id": product }));
    assert_eq!(ports.as_array().unwrap().len(), 2, "the generic's connectors: {ports:#}");

    // Swapped: the same element, a new size, its waste 40 mm nearer the wall's face side.
    let report = ok(&mut app, "bath.swap_product", json!({ "element_ids": [wc], "type_id": product }));
    let r = &report[0];
    assert_eq!(r["element"], wc);
    assert!((r["size_after"][1].as_f64().unwrap() - 0.62).abs() < 1e-6 && (r["size_before"][1].as_f64().unwrap() - 0.7).abs() < 1e-6, "{r:#}");
    let moved: Vec<&str> = r["connectors_moved"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(moved, ["Waste"], "the cold supply on the back stays put: {r:#}");
    assert!((r["connectors_moved"][0]["distance"].as_f64().unwrap() - 0.04).abs() < 0.002, "{r:#}");
    assert_eq!(ok(&mut app, "element.get_ids", json!({ "element_ids": [wc] }))[0]["guid"], guid);
    let path = dir.path().join("product.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("'Pset_ManufacturerTypeInformation'") && ifc.contains("'Acme'") && ifc.contains("'A-100'"), "manufacturer data saved");

    // Undone: the placeholder again.
    ok(&mut app, "edit.undo", json!({}));
    let back = ok(&mut app, "mesh_object.types", json!({}));
    assert_eq!(back.as_array().unwrap().iter().find(|t| t["id"] == types["WC, close-coupled"]).unwrap()["copies"], 1);
    // A mirror is not a WC.
    assert_eq!(run(&mut app, "bath.swap_product", json!({ "element_ids": [wc], "type_id": types["Mirror, 600 × 800"] })).unwrap_err().code, "type.wrong_family");
}

/// A manufacturer's WC as an OBJ, drawn with its front toward −x: 620 deep along x, 360 wide,
/// 800 high.
fn box_obj(dir: &std::path::Path) -> std::path::PathBuf {
    let obj = dir.join("acme_wc.obj");
    let (x, y, z) = (0.31, 0.18, 0.8);
    let mut text = String::new();
    for (a, b, c) in [(-x, -y, 0.0), (x, -y, 0.0), (x, y, 0.0), (-x, y, 0.0), (-x, -y, z), (x, -y, z), (x, y, z), (-x, y, z)] {
        text.push_str(&format!("v {a} {b} {c}\n"));
    }
    for f in ["1 3 2", "1 4 3", "5 6 7", "5 7 8", "1 2 6", "1 6 5", "2 3 7", "2 7 6", "3 4 8", "3 8 7", "4 1 5", "4 5 8"] {
        text.push_str(&format!("f {f}\n"));
    }
    std::fs::write(&obj, text).unwrap();
    obj
}

#[test]
fn a_products_documents_go_with_its_type() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let types = ok(&mut app, "bath.load_components", json!({ "names": ["WC, close-coupled"] }));
    let product = ok(&mut app, "bath.make_product", json!({ "generic": types["WC, close-coupled"], "path": box_obj(dir.path()), "name": "Acme Model 1", "turn": 90.0, "units": "m", "manufacturer": "Acme" })).as_u64().unwrap();
    ok(&mut app, "bath.place_component", json!({ "type_id": product, "point": [2.0, -0.3], "level": level }));
    // Generic placeholders are not listed; the product is.
    let listed = ok(&mut app, "docs.materials", json!({}));
    let p = listed.as_array().unwrap().iter().find(|m| m["type_id"] == product).unwrap_or_else(|| panic!("{listed:#}")).clone();
    assert_eq!(p["label"], "Product: Acme Model 1");
    assert!(!listed.as_array().unwrap().iter().any(|m| m["label"] == "Product: WC, close-coupled"));
    let key = p["material"].as_str().unwrap().to_owned();

    let docs = dir.path().join("Acme Model 1 documents");
    std::fs::create_dir(&docs).unwrap();
    std::fs::write(docs.join("data sheet.pdf"), b"%PDF- a data sheet").unwrap();
    std::fs::write(docs.join("epd.pdf"), b"%PDF- an EPD").unwrap();
    ok(&mut app, "docs.link", json!({ "material": key, "path": docs }));
    ok(&mut app, "docs.set_doc_type", json!({ "material": key, "file_uid": "epd.pdf", "doc_type": "EPD" }));
    ok(&mut app, "docs.enter_values", json!({ "material": key, "set": "epd", "values": { "epd_number": "EPD-ACME-1", "gwp_a1a3": "45" }, "units": { "gwp_a1a3": "kg CO2 eq" } }));
    assert_eq!(run(&mut app, "docs.enter_values", json!({ "material": key, "set": "epd", "carbon": true, "values": { "gwp_a1a3": "45", "declared_unit": "1 piece" } })).unwrap_err().code, "docs.invalid_input", "carbon factors are for materials");

    let path = dir.path().join("documented.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    ok(&mut app, "file.save", json!({}));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("IFCRELASSOCIATESDOCUMENT(").count(), 1, "once, after two saves");
    assert_eq!(text.lines().filter(|l| l.contains("IFCPROPERTYSET(") && l.contains("'Reviera_EPD'")).count(), 1);
    assert!(text.contains("'EPD-ACME-1'") && text.contains("Acme Model 1 documents"));
}

#[test]
fn fixture_counts_by_occupancy() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let level = model(&mut app);
    let need = |c: &Value, f: &str| -> Value { c["needs"].as_array().unwrap().iter().find(|n| n["fixture"] == f).cloned().unwrap_or_else(|| panic!("no {f}: {c:#}")) };
    // IPC Table 403.1, Group B, 360 occupants half and half.
    let c = ok(&mut app, "bath.fixture_count", json!({ "occupancy": "B", "occupant_load": 360 }));
    assert_eq!(c["row"], "IPC 403.1 B");
    let wc = need(&c, "water_closet");
    assert_eq!((wc["required"].as_u64(), wc["male"].as_u64(), wc["female"].as_u64()), (Some(10), Some(5), Some(5)), "{c:#}");
    assert_eq!(need(&c, "lavatory")["required"], 8);
    assert_eq!(need(&c, "drinking_fountain")["required"], 4);
    assert_eq!(need(&c, "service_sink")["required"], 1);
    assert_eq!(c["urinals_may_replace"], 2, "50 % of the males' 5 WCs, rounded down");
    let all = ok(&mut app, "bath.fixture_count", json!({ "occupancy": "B", "occupant_load": 360, "all_gender": true }));
    assert_eq!((need(&all, "water_closet")["required"].as_u64(), need(&all, "lavatory")["required"].as_u64()), (Some(9), Some(6)));
    // Three WCs modelled on the level: seven short.
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["WC, close-coupled"] }));
    for x in [1.0, 2.0, 3.0] {
        ok(&mut app, "bath.place_component", json!({ "type_id": t["WC, close-coupled"], "point": [x, -0.3], "level": level }));
    }
    let c = ok(&mut app, "bath.fixture_count", json!({ "occupancy": "B", "occupant_load": 360, "level_id": level }));
    assert_eq!((need(&c, "water_closet")["modelled"].as_u64(), need(&c, "water_closet")["short"].as_u64()), (Some(3), Some(7)));
    // HSE L24, by name.
    let uk = |app: &mut App, n: u32| ok(app, "bath.fixture_count", json!({ "occupancy": "workplace", "occupant_load": n, "pack": "uk_workplace" }));
    let c60 = uk(&mut app, 60);
    assert_eq!((need(&c60, "water_closet")["required"].as_u64(), need(&c60, "lavatory")["required"].as_u64()), (Some(4), Some(4)));
    let c130 = uk(&mut app, 130);
    assert_eq!(need(&c130, "water_closet")["required"], 7);
    assert_eq!(run(&mut app, "bath.fixture_count", json!({ "occupancy": "Z", "occupant_load": 10 })).unwrap_err().code, "bath.no_occupancy");
}

#[test]
fn a_toilet_core_along_a_wall() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // A 9 m room, 4 m deep; the core along its north wall (inner face at y = 4): 8.23 m of
    // compartments, urinals and lavatories.
    let (level, walls) = room(&mut app, 9.0, 4.0);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["us_plumbing", "ada_2010"] }));
    let core = ok(&mut app, "bath.toilet_core", json!({ "wall_id": walls[2], "start": [0.0, 3.9], "end": [9.0, 3.9], "level": level, "wcs": 4, "urinals": 2, "lavatories": 3 }));
    assert_eq!(core["doors"].as_array().unwrap().len(), 4, "{core:#}");
    assert_eq!(core["placed"].as_array().unwrap().len(), 4 + 2 + 3);
    assert_eq!(core["grab_bars"].as_array().unwrap().len(), 2, "the accessible WC's");
    // Five side panels, four fronts, one urinal screen.
    assert_eq!(core["partitions"].as_array().unwrap().len(), 5 + 4 + 1, "{core:#}");
    let f = findings(&mut app);
    let errors: Vec<&Value> = f.iter().filter(|x| x["severity"] == "error").collect();
    assert!(errors.is_empty(), "{errors:#?}");

    // Again with replace: the same counts, not doubled.
    let again = ok(&mut app, "bath.toilet_core", json!({ "wall_id": walls[2], "start": [0.0, 3.9], "end": [9.0, 3.9], "level": level, "wcs": 4, "urinals": 2, "lavatories": 3, "replace": true }));
    assert_eq!(again["placed"].as_array().unwrap().len(), 9);
    let first = core["doors"][0].as_u64().unwrap();
    assert!(run(&mut app, "element.get_ids", json!({ "element_ids": [first] })).is_err());

    // Too short a wall: refused with the shortfall.
    let e = run(&mut app, "bath.toilet_core", json!({ "wall_id": walls[0], "start": [0.0, 0.1], "end": [3.0, 0.1], "level": level, "wcs": 4, "urinals": 2, "lavatories": 3 })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit");
    assert!(e.details["short"].as_f64().unwrap() > 0.0);
}

#[test]
fn a_toilet_core_counted_for_its_occupancy() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, walls) = room(&mut app, 9.0, 4.0);
    // 360 in Group B: five WCs a sex; for the men's room, urinals may take two of them.
    let core = ok(&mut app, "bath.toilet_core", json!({ "wall_id": walls[2], "start": [0.0, 3.9], "end": [9.0, 3.9], "level": level, "occupancy": "B", "occupant_load": 360, "sex": "male", "urinals_for_wcs": true }));
    assert_eq!((core["wcs"].as_u64(), core["urinals"].as_u64(), core["lavatories"].as_u64()), (Some(3), Some(2), Some(4)), "{core:#}");
}

#[test]
fn a_locker_room() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, _) = room(&mut app, 6.0, 5.0);
    ok(&mut app, "bath.set_rule_packs", json!({ "packs": ["us_plumbing", "ada_2010"] }));
    let out = ok(&mut app, "bath.locker_room", json!({ "point": [3.0, 2.5], "level": level, "lockers": 40, "tiers": 2, "showers": 6 }));
    assert_eq!((out["lockers"].as_u64(), out["accessible_lockers"].as_u64()), (Some(40), Some(2)), "{out:#}");
    assert!((out["fit"]["aisle"].as_f64().unwrap() - 0.76).abs() < 1e-9, "the ADA's 30 in in front of a locker");
    let types = ok(&mut app, "mesh_object.types", json!({}));
    let copies = |name: &str| types.as_array().unwrap().iter().find(|t| t["name"] == name).map_or(0, |t| t["copies"].as_u64().unwrap());
    assert_eq!((copies("Locker, two tier"), copies("Shower head, gang"), copies("Bench, accessible")), (20, 6, 1));
    assert!(copies("Bench, locker room") >= 1);
    let f = findings(&mut app);
    let errors: Vec<&Value> = f.iter().filter(|x| x["severity"] == "error").collect();
    assert!(errors.is_empty(), "{errors:#?}");
    // Too many lockers: how many fit.
    let e = run(&mut app, "bath.locker_room", json!({ "point": [3.0, 2.5], "level": level, "lockers": 200, "tiers": 2, "showers": 6, "replace": true })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit");
    assert!(e.message.contains("lockers fit"), "{}", e.message);
}

#[test]
fn a_locker_room_too_narrow_for_approved_document_m() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let (level, _) = room(&mut app, 6.0, 4.0);
    ok(&mut app, "code.set", json!({ "code": "uk_adk", "occupancy": "other" }));
    let e = run(&mut app, "bath.locker_room", json!({ "point": [3.0, 2.0], "level": level, "lockers": 20 })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit");
    assert!((e.details["depth_needed"].as_f64().unwrap() - 4.219).abs() < 0.001, "{}", e.details);
}

#[test]
fn bathroom_electrical_zones() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // A 4 × 3 m bathroom with a bath along its south wall, under England's code (BS 7671 701).
    let (level, _) = room(&mut app, 4.0, 3.0);
    ok(&mut app, "code.set", json!({ "code": "uk_adk", "occupancy": "dwelling" }));
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Bath, 1700 × 700", "Shower head, gang"] }));
    let bath = ok(&mut app, "bath.place_component", json!({ "type_id": t["Bath, 1700 × 700"], "point": [1.0, 0.1], "level": level })).as_u64().unwrap();
    let list = ok(&mut app, "clearance.list", json!({ "element_ids": [bath] }));
    let zone = |rule: &str| -> Value { list.as_array().unwrap().iter().find(|c| c["kind"] == "electrical_zone" && c["rule"] == rule).cloned().unwrap_or_else(|| panic!("{list:#}")) };
    assert!((zone("uk_bs7671 zone 1")["max"][2].as_f64().unwrap() - 2.25).abs() < 0.01);
    let (z1, z2) = (zone("uk_bs7671 zone 1"), zone("uk_bs7671 zone 2"));
    assert!((z1["min"][0].as_f64().unwrap() - z2["min"][0].as_f64().unwrap() - 0.6).abs() < 0.01, "zone 2 reaches 0.6 m beyond zone 1");
    // A gang shower head on the north wall: a 1.2 m zone 1 round it.
    let head = ok(&mut app, "bath.place_component", json!({ "type_id": t["Shower head, gang"], "point": [3.0, 2.9], "level": level })).as_u64().unwrap();
    let hz = ok(&mut app, "clearance.list", json!({ "element_ids": [head] }));
    let r = hz.as_array().unwrap().iter().find(|c| c["rule"] == "uk_bs7671 zone 1").unwrap();
    assert!((r["max"][0].as_f64().unwrap() - r["min"][0].as_f64().unwrap() - 2.4).abs() < 0.02, "{r}");
    // Electrical zones are not clear spaces.
    let c = ok(&mut app, "clearance.check", json!({ "element_ids": [bath] }));
    assert!(c.as_array().unwrap().iter().all(|i| i["kind"] != "electrical_zone"));
    ok(&mut app, "element.delete", json!({ "element_ids": [head] }));

    let starters = ok(&mut app, "mep_type.load_starters", json!({ "names": ["Receptacle", "Wall Luminaire"] }));
    // A socket on the east wall 1.5 m from the bath's zone 1 (its end at x ≈ 1.85): reported.
    let near = ok(&mut app, "device.place", json!({ "type_id": starters["Receptacle"], "point": [3.35, 1.0], "level": level, "offset": 0.4, "work_plane": true })).as_u64().unwrap();
    let f = findings(&mut app);
    assert!(f.iter().any(|x| x["kind"] == "electrical" && x["other"] == near && x["message"].as_str().unwrap().contains("socket outlet")), "{f:#?}");
    ok(&mut app, "element.move", json!({ "element_ids": [near], "vector": [0.6, 1.4, 0.0] }));
    let f = findings(&mut app);
    assert!(!f.iter().any(|x| x["other"] == near), "3 m away: {f:#?}");
    // A luminaire over the bath in zone 1: no rating → a note; IP20 → an error; IPX4 → fine.
    let lum = ok(&mut app, "lighting_fixture.place", json!({ "type_id": starters["Wall Luminaire"], "point": [1.0, 0.4], "level": level, "offset": 2.0, "work_plane": true })).as_u64().unwrap();
    let state = |app: &mut App| findings(app).into_iter().find(|x| x["other"] == lum).map(|x| x["severity"].as_str().unwrap().to_owned());
    assert_eq!(state(&mut app).as_deref(), Some("note"));
    ok(&mut app, "pset.set", json!({ "element_id": lum, "pset_name": "Pset_ElectricalDeviceCommon", "property_name": "IP_Code", "value": "IP20" }));
    assert_eq!(state(&mut app).as_deref(), Some("error"));
    ok(&mut app, "pset.set", json!({ "element_id": lum, "pset_name": "Pset_ElectricalDeviceCommon", "property_name": "IP_Code", "value": "IPX4" }));
    assert_eq!(state(&mut app), None);
}

/// A floor over a `room`'s inside.
fn room_floor(app: &mut App, level: &Value, w: f64, l: f64) -> u64 {
    let c = [[0.0, 0.0], [w, 0.0], [w, l], [0.0, l]];
    let curves: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    ok(app, "floor.create", json!({ "loops": [curves], "level_id": level })).as_u64().unwrap()
}

#[test]
fn shower_falls_to_a_drain() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // A roll-in shower on a flat floor under the IPC: reported as having no fall.
    let (level, _) = room(&mut app, 3.0, 2.5);
    let floor = room_floor(&mut app, &level, 3.0, 2.5);
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Roll-in shower, 60 × 30 in", "Shower tray, 900 × 900"] }));
    let shower = ok(&mut app, "bath.place_component", json!({ "type_id": t["Roll-in shower, 60 × 30 in"], "point": [1.0, 0.1], "level": level })).as_u64().unwrap();
    let falls = |app: &mut App| findings(app).into_iter().filter(|f| f["kind"] == "falls" && f["element"] == shower).collect::<Vec<_>>();
    let f = falls(&mut app);
    assert!(f.len() == 1 && f[0]["message"].as_str().unwrap().contains("no fall"), "{f:#?}");
    // A centre drain at the pack's 2 %: every face of the area falls at least that.
    let out = ok(&mut app, "bath.shower_falls", json!({ "element_ids": [shower] }));
    assert_eq!((out["drains"].as_array().unwrap().len(), out["floors"].clone()), (1, json!([floor])));
    assert_eq!(falls(&mut app), Vec::<Value>::new());
    let shape = ok(&mut app, "shape.get", json!({ "element_id": floor }));
    let drop = shape["points"][0]["z"].as_f64().unwrap();
    assert!((drop + 0.02 * 0.7625).abs() < 0.002, "{shape}");
    // Laid again: the earlier drain and falls are replaced, not deepened.
    let again = ok(&mut app, "bath.shower_falls", json!({ "element_ids": [shower] }));
    let types = ok(&mut app, "mesh_object.types", json!({}));
    let drains = types.as_array().unwrap().iter().find(|t| t["name"] == "Floor Drain").map_or(0, |t| t["copies"].as_u64().unwrap());
    assert_eq!(drains, 1, "{types:#}");
    let shape = ok(&mut app, "shape.get", json!({ "element_id": floor }));
    assert_eq!((shape["points"].as_array().unwrap().len(), shape["lines"].as_array().unwrap().len()), (1, 1), "{shape} {again}");
    assert!((shape["points"][0]["z"].as_f64().unwrap() - drop).abs() < 1e-6);
    // A tray is not level access.
    let tray = ok(&mut app, "bath.place_component", json!({ "type_id": t["Shower tray, 900 × 900"], "point": [2.4, 0.1], "level": level })).as_u64().unwrap();
    let e = run(&mut app, "bath.shower_falls", json!({ "element_ids": [tray] })).unwrap_err();
    assert_eq!(e.code, "bath.not_level_access");

    // Approved Document M: 1 in 20 to a linear drain is steeper than its 1 in 40.
    let (level, _) = room(&mut app, 3.0, 2.5);
    room_floor(&mut app, &level, 3.0, 2.5);
    ok(&mut app, "code.set", json!({ "code": "uk_adk", "occupancy": "dwelling" }));
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Roll-in shower, 60 × 30 in"] }));
    let shower = ok(&mut app, "bath.place_component", json!({ "type_id": t["Roll-in shower, 60 × 30 in"], "point": [1.0, 0.1], "level": level })).as_u64().unwrap();
    ok(&mut app, "bath.shower_falls", json!({ "element_ids": [shower], "fall": 0.05, "drain": "linear" }));
    let f: Vec<Value> = findings(&mut app).into_iter().filter(|f| f["kind"] == "falls").collect();
    assert!(f.iter().any(|x| x["message"].as_str().unwrap().contains("steeper")), "{f:#?}");
}

#[test]
fn waterproofing_extents() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // A 900 × 900 tray in the south-west corner of a room under the IPC: its back and side walls
    // to 6 ft, and its floor.
    let (level, [south, _, _, west]) = room(&mut app, 3.0, 2.5);
    room_floor(&mut app, &level, 3.0, 2.5);
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Shower tray, 900 × 900", "Urinal, wall-hung", "Roll-in shower, 60 × 30 in"] }));
    let tray = ok(&mut app, "bath.place_component", json!({ "type_id": t["Shower tray, 900 × 900"], "point": [0.45, 0.1], "level": level })).as_u64().unwrap();
    let s = ok(&mut app, "bath.waterproofing", json!({ "element_ids": [tray] }));
    let ext = s["extents"].as_array().unwrap();
    let walls: Vec<&Value> = ext.iter().filter(|e| e["surface"] == "wall").collect();
    let hosts: Vec<u64> = walls.iter().map(|e| e["host"].as_u64().unwrap()).collect();
    assert!(walls.len() == 2 && hosts.contains(&south) && hosts.contains(&west), "{s:#}");
    assert!((s["wall_area"].as_f64().unwrap() - 2.0 * 0.9 * 1.829).abs() < 0.01, "{s:#}");
    assert!((s["floor_area"].as_f64().unwrap() - 0.81).abs() < 0.01, "{s:#}");
    // Shown with the virtual volumes.
    let list = ok(&mut app, "clearance.list", json!({ "element_ids": [tray] }));
    assert_eq!(list.as_array().unwrap().iter().filter(|c| c["kind"] == "waterproofing").count(), 3, "{list:#}");

    // A urinal on the north wall: 4 ft up and 2 ft past each side, 2 ft of floor in front.
    let urinal = ok(&mut app, "bath.place_component", json!({ "type_id": t["Urinal, wall-hung"], "point": [2.2, 2.4], "level": level })).as_u64().unwrap();
    let s = ok(&mut app, "bath.waterproofing", json!({ "element_ids": [urinal] }));
    let wall = s["extents"].as_array().unwrap().iter().find(|e| e["surface"] == "wall").unwrap().clone();
    assert!((wall["height"].as_f64().unwrap() - 1.219).abs() < 1e-3 && (wall["width"].as_f64().unwrap() - (0.36 + 1.22)).abs() < 0.01, "{s:#}");
    let floor = s["extents"].as_array().unwrap().iter().find(|e| e["surface"] == "floor").unwrap().clone();
    assert!((floor["height"].as_f64().unwrap() - (0.35 + 0.61)).abs() < 0.01, "{s:#}");

    // Approved Document M: a level-access shower makes a wet room, its whole floor tanked.
    let (level, _) = room(&mut app, 3.0, 2.5);
    room_floor(&mut app, &level, 3.0, 2.5);
    ok(&mut app, "code.set", json!({ "code": "uk_adk", "occupancy": "dwelling" }));
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Roll-in shower, 60 × 30 in"] }));
    let shower = ok(&mut app, "bath.place_component", json!({ "type_id": t["Roll-in shower, 60 × 30 in"], "point": [1.0, 0.1], "level": level })).as_u64().unwrap();
    let s = ok(&mut app, "bath.waterproofing", json!({ "element_ids": [shower] }));
    assert!((s["floor_area"].as_f64().unwrap() - 7.5).abs() < 0.05, "{s:#}");
}

#[test]
fn a_hot_tub_its_service_space_electrics_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // A 2 m hot tub free-standing in a 6 × 4 m room under the IRC: 0.9 m clear all round.
    let (level, _) = room(&mut app, 6.0, 4.0);
    ok(&mut app, "code.set", json!({ "code": "irc" }));
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Hot tub, 2.0 × 2.0 m", "Vanity basin, 900"] }));
    let tub = ok(&mut app, "bath.place_component", json!({ "type_id": t["Hot tub, 2.0 × 2.0 m"], "point": [2.5, 2.0], "level": level, "work_plane": true })).as_u64().unwrap();
    let (lo, hi) = space(&mut app, tub, "Spa service access");
    assert!((hi[0] - lo[0] - 3.8).abs() < 0.01 && (hi[1] - lo[1] - 3.8).abs() < 0.01, "{lo:?} {hi:?}");
    // A vanity on the south wall in front of it stands in that space.
    let vanity = ok(&mut app, "bath.place_component", json!({ "type_id": t["Vanity basin, 900"], "point": [2.5, 0.1], "level": level })).as_u64().unwrap();
    let f = findings(&mut app);
    assert!(f.iter().any(|x| x["element"] == tub && x["kind"] == "intrusion" && x["other"] == vanity), "{f:#?}");
    ok(&mut app, "element.delete", json!({ "element_ids": [vanity] }));
    // A receptacle 1.2 m from it: under 6 ft; moved to 2.0 m: fine.
    let s = ok(&mut app, "mep_type.load_starters", json!({ "names": ["Receptacle"] }));
    let r = ok(&mut app, "device.place", json!({ "type_id": s["Receptacle"], "point": [4.7, 2.0], "level": level, "offset": 0.4, "work_plane": true })).as_u64().unwrap();
    let near = |app: &mut App| findings(app).into_iter().any(|x| x["element"] == tub && x["kind"] == "electrical" && x["other"] == r);
    assert!(near(&mut app));
    ok(&mut app, "element.move", json!({ "element_ids": [r], "vector": [0.8, 0.0, 0.0] }));
    assert!(!near(&mut app));
    // On the ground floor it asks for a pad.
    let load = |app: &mut App, id: u64| findings(app).into_iter().find(|x| x["element"] == id && x["kind"] == "load");
    assert!(load(&mut app, tub).unwrap()["message"].as_str().unwrap().contains("pad"));
    // Upstairs: about 5.4 kPa filled against 1.92; at a filled weight of 300 kg, under it.
    let up = ok(&mut app, "level.create", json!({ "elevation": 3.0 }));
    let upper = ok(&mut app, "bath.place_component", json!({ "type_id": t["Hot tub, 2.0 × 2.0 m"], "point": [2.5, 2.0], "level": up, "work_plane": true })).as_u64().unwrap();
    let l = load(&mut app, upper).unwrap();
    assert!((l["measured"].as_f64().unwrap() - 5.4).abs() < 0.05, "{l:#}");
    ok(&mut app, "pset.set", json!({ "element_id": upper, "pset_name": "SpaProperties", "property_name": "FilledWeight", "value": 300.0 }));
    assert!(load(&mut app, upper).is_none());
}

#[test]
fn a_sauna_its_benches_heater_and_door() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // 2.4 × 2.0 m, 2.2 m high: 10.6 m³, an 8 kW heater; two tiers, the upper 1.15 m below the
    // ceiling.
    let (level, [south, ..]) = room_h(&mut app, 2.4, 2.0, 2.2);
    let s = ok(&mut app, "bath.sauna", json!({ "point": [1.2, 1.0], "level": level }));
    assert!((s["sizing"]["volume"].as_f64().unwrap() - 10.56).abs() < 0.05 && s["heater_kw"] == 8.0, "{s:#}");
    assert_eq!(s["benches"], 8, "{s:#}");
    assert!((s["bench_tops"][0].as_f64().unwrap() - 1.05).abs() < 1e-6 && (s["bench_tops"][1].as_f64().unwrap() - 0.6).abs() < 1e-6);
    let heater = s["heater"].as_u64().unwrap();
    let sauna = |app: &mut App| findings(app).into_iter().filter(|f| f["kind"] == "sauna" || (f["element"] == heater && f["kind"] == "intrusion")).collect::<Vec<_>>();
    assert_eq!(sauna(&mut app), Vec::<Value>::new());
    let list = ok(&mut app, "clearance.list", json!({ "element_ids": [heater] }));
    assert!(list.as_array().unwrap().iter().any(|c| c["rule"] == "Heater safety distances"), "{list:#}");
    // A 3 kW heater is too small for the room.
    ok(&mut app, "pset.set", json!({ "element_id": heater, "pset_name": "SaunaHeater", "property_name": "Power", "value": 3000.0 }));
    let f = sauna(&mut app);
    assert!(f.len() == 1 && f[0]["message"].as_str().unwrap().contains("suits 2–4.5"), "{f:#?}");
    ok(&mut app, "pset.set", json!({ "element_id": heater, "pset_name": "SaunaHeater", "property_name": "Power", "value": 8000.0 }));
    // A door in the south wall: inward is reported, outward is not.
    let door = ok(&mut app, "door.place", json!({ "host_wall_id": south, "point_along": 1.8 })).as_u64().unwrap();
    let into = |app: &mut App| sauna(app).iter().any(|f| f["other"] == door);
    let first = into(&mut app);
    ok(&mut app, "insert.flip_facing", json!({ "element_ids": [door] }));
    assert_ne!(first, into(&mut app), "flipping the door changes the finding");
    // Too narrow for two tiers and a walkway.
    let (level, _) = room_h(&mut app, 2.4, 1.5, 2.2);
    let e = run(&mut app, "bath.sauna", json!({ "point": [1.2, 0.75], "level": level })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit", "{e:?}");
}

#[test]
fn a_steam_room_its_ceiling_bench_head_and_falls() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    // 2.4 × 1.8 m with walls 2.4 m high, on a floor: the ceiling 2.13 m over the bench wall
    // falling 1:6 to 1.83 m across the room, four bench modules, a drain under it.
    let (level, _) = room_h(&mut app, 2.4, 1.8, 2.4);
    room_floor(&mut app, &level, 2.4, 1.8);
    let s = ok(&mut app, "bath.steam_room", json!({ "point": [1.2, 0.9], "level": level }));
    assert!((s["ceiling_high"].as_f64().unwrap() - 2.13).abs() < 1e-6 && (s["ceiling_low"].as_f64().unwrap() - 1.83).abs() < 0.001, "{s:#}");
    assert_eq!(s["benches"], 4);
    assert!(s["drain"].is_u64(), "{s:#}");
    assert!((s["design_volume"].as_f64().unwrap() - 2.4 * 1.8 * (2.13 + 1.83) / 2.0).abs() < 0.01, "tile, no height or outside-wall factors: {s:#}");
    let head = s["steam_head"].as_u64().unwrap();
    let steam = |app: &mut App| findings(app).into_iter().filter(|f| f["kind"] == "steam").collect::<Vec<_>>();
    assert_eq!(steam(&mut app), Vec::<Value>::new());
    // Natural stone, an outside wall and the ceiling at the walls' 2.4 m: × 1.65 × 1.1, and
    // 10 % a foot above 7 ft.
    let again = ok(&mut app, "bath.steam_room", json!({ "point": [1.2, 0.9], "level": level, "height": 2.4, "material": "stone", "exterior_wall": true, "replace": true, "floor_falls": false }));
    let v = 2.4 * 1.8 * (2.4 + (2.4 - 0.1667 * 1.8)) / 2.0;
    let expect = v * 1.65 * (1.0 + 0.1 * (2.4 - 2.13) / 0.3048) * 1.1;
    assert!((again["design_volume"].as_f64().unwrap() - expect).abs() < 0.01, "{again:#}");
    assert!(run(&mut app, "element.get_ids", json!({ "element_ids": [head] })).is_err(), "the earlier layout replaced");
    // No ceiling: reported.
    ok(&mut app, "element.delete", json!({ "element_ids": [again["ceiling"]] }));
    let f = steam(&mut app);
    assert!(f.len() == 1 && f[0]["message"].as_str().unwrap().contains("no ceiling"), "{f:#?}");
    // Too deep for a 1:6 ceiling under 2.13 m.
    let (level, _) = room_h(&mut app, 3.0, 2.4, 2.4);
    let e = run(&mut app, "bath.steam_room", json!({ "point": [1.5, 1.2], "level": level, "bench_wall_id": null })).unwrap_err();
    assert_eq!(e.code, "bath.no_fit", "{e:?}");
}

#[test]
fn accessible_saunas_and_spas() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let access = |app: &mut App| findings(app).into_iter().filter(|f| f["kind"] == "access").map(|f| f["message"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    // A 2.4 × 2.0 m sauna under the ADA: its lower bench 450 mm deep at 600 mm is not to §903,
    // and its benches leave no 60 in turning space.
    let (level, _) = room_h(&mut app, 2.4, 2.0, 2.2);
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    ok(&mut app, "bath.sauna", json!({ "point": [1.2, 1.0], "level": level }));
    let a = access(&mut app);
    assert!(a.len() == 2 && a.iter().any(|m| m.contains("§903")) && a.iter().any(|m| m.contains("turning space")), "{a:#?}");
    // 3.2 × 2.9 m with an accessible lower bench (610 deep, 460 high): both met.
    let (level, _) = room_h(&mut app, 3.2, 2.9, 2.2);
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    let s = ok(&mut app, "bath.sauna", json!({ "point": [1.6, 1.45], "level": level, "accessible": true }));
    assert!((s["bench_tops"][1].as_f64().unwrap() - 0.46).abs() < 1e-9, "{s:#}");
    assert_eq!(access(&mut app), Vec::<String>::new());

    // A hot tub in a club (not a dwelling): no means of entry, then a transfer wall beside it.
    let (level, _) = room(&mut app, 6.0, 5.0);
    ok(&mut app, "code.set", json!({ "code": "ibc", "occupancy": "other" }));
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    let t = ok(&mut app, "bath.load_components", json!({ "names": ["Hot tub, 2.0 × 2.0 m", "Spa transfer wall"] }));
    ok(&mut app, "bath.place_component", json!({ "type_id": t["Hot tub, 2.0 × 2.0 m"], "point": [3.0, 2.0], "level": level, "work_plane": true }));
    let a = access(&mut app);
    assert!(a.len() == 1 && a[0].contains("means of entry"), "{a:#?}");
    ok(&mut app, "bath.place_component", json!({ "type_id": t["Spa transfer wall"], "point": [3.0, 3.2], "level": level, "work_plane": true }));
    assert_eq!(access(&mut app), Vec::<String>::new());
}
