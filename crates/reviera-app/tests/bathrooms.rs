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
    ok(app, "file.new", json!({}));
    let level = ok(app, "level.list", json!({}))[0]["id"].clone();
    // The wall thickness, from a probe room's fit.
    let probe = [wall(app, &level, [20.0, 0.0], [23.0, 0.0]), wall(app, &level, [23.0, 0.0], [23.0, 4.0]), wall(app, &level, [23.0, 4.0], [20.0, 4.0]), wall(app, &level, [20.0, 4.0], [20.0, 0.0])];
    let fit = ok(app, "bath.layout", json!({ "template": "cloakroom", "point": [21.5, 2.0], "level": level, "fit_only": true }))["fit"].clone();
    let t = 4.0 - fit["length"].as_f64().unwrap();
    ok(app, "element.delete", json!({ "element_ids": probe }));
    let h = t / 2.0;
    let (x0, y0, x1, y1) = (-h, -h, w + h, l + h);
    let s = wall(app, &level, [x0, y0], [x1, y0]);
    let e = wall(app, &level, [x1, y0], [x1, y1]);
    let n = wall(app, &level, [x1, y1], [x0, y1]);
    let wst = wall(app, &level, [x0, y1], [x0, y0]);
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
