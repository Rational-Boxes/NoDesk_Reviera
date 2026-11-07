// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Shared rule packs and use zones (ARC-RULE-001, ARC-RULE-002): interiors packs chosen from the
//! accessibility rule set or by name, every domain's packs listed and managed together, a
//! project pack replacing a shipped one and saved with the model, a bed's ADA spaces kept clear
//! of a nightstand, a practice note in front of a dresser, accessible marks, and zones that
//! follow their element.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A box `w` wide (x), `d` deep (y) and `h` high, centred on its origin.
fn box_obj(dir: &std::path::Path, name: &str, w: f64, d: f64, h: f64) -> String {
    let mut obj = String::new();
    for z in [0.0, h] {
        for (x, y) in [(-w / 2.0, -d / 2.0), (w / 2.0, -d / 2.0), (w / 2.0, d / 2.0), (-w / 2.0, d / 2.0)] {
            obj.push_str(&format!("v {x} {y} {z}\n"));
        }
    }
    obj.push_str("f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n");
    let path = dir.join(format!("{name}.obj"));
    std::fs::write(&path, obj).unwrap();
    path.to_string_lossy().into_owned()
}

/// Imports a box as a furniture type with a predefined type (`BED`) or kind (`Dresser`), and
/// places one copy.
fn furniture(app: &mut App, dir: &std::path::Path, level: &Value, name: &str, size: [f64; 3], at: [f64; 2], kind: &str) -> u64 {
    let path = box_obj(dir, name, size[0], size[1], size[2]);
    let ty = ok(app, "mesh_object.import", json!({ "path": path, "name": name, "category": "furniture" }));
    let ty = ty.get("type_id").cloned().unwrap_or(ty);
    let set = if kind.chars().all(|c| c.is_ascii_uppercase()) { json!({ "type_id": ty, "predefined_type": kind }) } else { json!({ "type_id": ty, "element_type": kind }) };
    ok(app, "mesh_object.set_type", set);
    let copy = ok(app, "mesh_object.place", json!({ "type_id": ty, "point": at, "level": level }));
    copy.as_u64().or_else(|| copy[0].as_u64()).unwrap()
}

/// An element's use zones by rule: (min, max) boxes.
fn zones(app: &mut App, id: u64) -> Vec<(String, [f64; 3], [f64; 3])> {
    let list = ok(app, "clearance.list", json!({ "element_ids": [id] }));
    let mut out: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["kind"] == "use_zone")
        .map(|c| {
            let v = |k: &str| -> [f64; 3] { serde_json::from_value(c[k].clone()).unwrap() };
            (c["rule"].as_str().unwrap().to_owned(), v("min"), v("max"))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..2).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

const IN: f64 = 0.0254;

#[test]
fn interiors_packs_and_use_zones() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();

    // Every domain's packs in one list; the ADA rule set brings in the ADA interiors pack.
    let packs = ok(&mut app, "rules.packs", json!({}));
    let has = |packs: &Value, domain: &str| packs.as_array().unwrap().iter().any(|p| p["domain"] == domain);
    assert!(has(&packs, "bath") && has(&packs, "vt") && has(&packs, "interiors"), "{packs:#}");
    let in_use = |app: &mut App, id: &str| ok(app, "rules.packs", json!({ "domain": "interiors" })).as_array().unwrap().iter().any(|p| p["id"] == id && p["in_use"] == true);
    assert!(!in_use(&mut app, "ada_2010_interiors"));
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    assert!(in_use(&mut app, "ada_2010_interiors") && !in_use(&mut app, "practice_interiors"));
    let ada = ok(&mut app, "rules.packs", json!({ "domain": "interiors" })).as_array().unwrap().iter().find(|p| p["id"] == "ada_2010_interiors").unwrap().clone();
    // Two bed spaces (§806.2.3), headroom (§307.4), projection (§307.2), and the counter
    // portions' two clear floor spaces, knee clearance and two surfaces (§904.4, §306), and the
    // accessible closet's reach (§308.3.2).
    assert_eq!((ada["rules"].as_u64(), ada["layer"].as_str(), ada["shipped"].as_bool()), (Some(10), Some("accessible"), Some(true)), "{ada:#}");

    // A queen bed (60 × 80 in) with its foot toward −y: a 30 × 48 in space along each side,
    // from its foot.
    let (bw, bd) = (60.0 * IN, 80.0 * IN);
    let bed = furniture(&mut app, dir.path(), &level, "Queen bed", [bw, bd, 0.6], [3.0, 3.0], "BED");
    let z = zones(&mut app, bed);
    assert_eq!(z.iter().map(|z| z.0.as_str()).collect::<Vec<_>>(), ["ADA 2010 806.2.3 left", "ADA 2010 806.2.3 right"], "{z:?}");
    let (x0, y0) = (3.0 - bw / 2.0, 3.0 - bd / 2.0);
    assert!(close(z[0].1, [x0 - 30.0 * IN, y0, 0.0]) && close(z[0].2, [x0, y0 + 48.0 * IN, 0.0]), "{z:?}");
    assert!(close(z[1].1, [3.0 + bw / 2.0, y0, 0.0]), "{z:?}");

    // A nightstand pushed into the left space is an error against 806.2.3; moved to the bed's
    // head, clear of the space, it is not.
    let stand = furniture(&mut app, dir.path(), &level, "Nightstand", [0.5, 0.4, 0.6], [x0 - 0.3, y0 + 0.6], "Nightstand");
    let found = ok(&mut app, "rules.check", json!({}));
    let errors: Vec<&Value> = found.as_array().unwrap().iter().filter(|x| x["severity"] == "error").collect();
    assert_eq!(errors.len(), 1, "{found:#}");
    assert_eq!((errors[0]["rule"].as_str(), errors[0]["other"].as_u64(), errors[0]["pack"].as_str(), errors[0]["source"].as_str()), (Some("ADA 2010 806.2.3 left"), Some(stand), Some("ada_2010_interiors"), Some("S1354")));
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [stand], "point": [x0 - 0.3, y0 + bd - 0.2] }));
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]));
    // clearance.check sees the zones too.
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [stand], "point": [x0 - 0.3, y0 + 0.6] }));
    assert!(ok(&mut app, "clearance.check", json!({})).as_array().unwrap().iter().any(|c| c.to_string().contains("806.2.3")));

    // Marked not accessible, the bed gets no ADA spaces.
    ok(&mut app, "rules.set_accessible", json!({ "element_ids": [bed], "accessible": false }));
    assert!(zones(&mut app, bed).is_empty());
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]));
    ok(&mut app, "rules.set_accessible", json!({ "element_ids": [bed], "accessible": true }));

    // The zones turn with the bed: turned a quarter, the spaces lie along y.
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [stand], "point": [8.0, 8.0] }));
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [bed], "rotation": "90°" }));
    let z = zones(&mut app, bed);
    let span = |b: &(String, [f64; 3], [f64; 3])| [b.2[0] - b.1[0], b.2[1] - b.1[1]];
    assert!((span(&z[0])[0] - 48.0 * IN).abs() < 1e-6 && (span(&z[0])[1] - 30.0 * IN).abs() < 1e-6, "{z:?}");
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [bed], "rotation": 0.0 }));

    // Practice by name only: a wall 600 mm in front of a dresser is a note, not an error.
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors", "packs": ["practice_interiors"] }));
    assert!(!in_use(&mut app, "ada_2010_interiors"));
    let dresser = furniture(&mut app, dir.path(), &level, "Dresser", [1.5, 0.5, 0.8], [10.0, 3.0], "Dresser");
    let wall_y = 3.0 - 0.25 - 0.6;
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [9.0, wall_y - 0.1], "end": [11.0, wall_y - 0.1] } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } }));
    let found = ok(&mut app, "rules.check", json!({ "element_ids": [dresser] }));
    assert_eq!(found.as_array().unwrap().len(), 1, "{found:#}");
    assert_eq!((found[0]["severity"].as_str(), found[0]["rule"].as_str(), found[0]["layer"].as_str()), (Some("note"), Some("Dresser front"), Some("recommended")));
    assert!(zones(&mut app, bed).iter().all(|z| z.0 == "Bed foot"));

    // A project pack widening the bed spaces to 36 in replaces the shipped pack, and is saved
    // with the model.
    let mut wide: Value = serde_json::from_str(include_str!("../../../schemas/rules/packs/ada_2010_interiors.json")).unwrap();
    wide["zones"][0]["across"][0][1] = json!(-36.0 * IN);
    wide["edition"] = json!("Office standard: 36 in beside beds");
    assert_eq!(ok(&mut app, "rules.add_pack", json!({ "pack": wide })), json!({ "domain": "interiors", "id": "ada_2010_interiors" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors" }));
    let z = zones(&mut app, bed);
    assert!((z[0].2[0] - z[0].1[0] - 36.0 * IN).abs() < 1e-6, "{z:?}");
    let listed = ok(&mut app, "rules.packs", json!({ "domain": "interiors" }));
    assert!(listed.as_array().unwrap().iter().any(|p| p["id"] == "ada_2010_interiors" && p["shipped"] == false));
    let model = dir.path().join("bedroom.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    let mut again = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled2") });
    ok(&mut again, "file.open", json!({ "path": model }));
    let beds: Vec<u64> = ok(&mut again, "clearance.list", json!({})).as_array().unwrap().iter().filter(|c| c["kind"] == "use_zone" && c["rule"] == "ADA 2010 806.2.3 left").map(|c| c["element"].as_u64().unwrap()).collect();
    assert_eq!(beds.len(), 1);
    let z = zones(&mut again, beds[0]);
    assert!((z[0].2[0] - z[0].1[0] - 36.0 * IN).abs() < 1e-6, "kept with the model: {z:?}");
    ok(&mut app, "rules.remove_pack", json!({ "domain": "interiors", "id": "ada_2010_interiors" }));
    let z = zones(&mut app, bed);
    assert!((z[0].2[0] - z[0].1[0] - 30.0 * IN).abs() < 1e-6, "the shipped pack is back: {z:?}");

    // A bathroom pack goes to the bathroom domain's own store, where bath.* sees it.
    let mut nkba: Value = serde_json::from_str(include_str!("../../../schemas/bath/packs/us_nkba.json")).unwrap();
    nkba["id"] = json!("office_bath");
    assert_eq!(ok(&mut app, "rules.add_pack", json!({ "pack": nkba })), json!({ "domain": "bath", "id": "office_bath" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "bath", "packs": ["office_bath"] }));
    let bath = ok(&mut app, "bath.rule_packs", json!({}));
    assert!(bath.as_array().unwrap().iter().any(|p| p["id"] == "office_bath" && p["in_use"] == true), "{bath:#}");
    ok(&mut app, "rules.remove_pack", json!({ "domain": "bath", "id": "office_bath" }));
    assert!(!ok(&mut app, "bath.rule_packs", json!({})).as_array().unwrap().iter().any(|p| p["id"] == "office_bath"));

    // A pack of no known format, or breaking the schema, is refused.
    assert!(app.execute("rules.add_pack", json!({ "pack": { "format": "other-1", "id": "x" } }), &Caller::user()).is_err());
    assert!(app.execute("rules.add_pack", json!({ "pack": { "format": "riviera.rules-1", "id": "x", "title": "X" } }), &Caller::user()).is_err());
    assert!(app.execute("rules.set_packs", json!({ "domain": "interiors", "packs": ["nothing"] }), &Caller::user()).is_err());
    assert_eq!(ok(&mut app, "rules.schema", json!({}))["$id"], "https://riviera.org/schemas/rules/riviera.rules-1.schema.json");
}
