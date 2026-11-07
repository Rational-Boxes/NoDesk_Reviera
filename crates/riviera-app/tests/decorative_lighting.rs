// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Decorative lighting (ARC-INT-003) and the mounting rules (ARC-RULE-002): a chandelier sized
//! from its room and table and hung 32 in over it; one in open floor under a low ceiling
//! reported against ADA §307.4; pendants spaced over a table at 33 in; sconces along a wall
//! within §307.2's 4 in.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

const IN: f64 = 0.0254;

#[test]
fn chandeliers_pendants_and_sconces_hang_where_the_rules_ask() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors", "packs": ["ada_2010_interiors", "practice_interiors"] }));
    // A 4.0 × 5.0 m dining room, 2.7 m high.
    let t = 0.1;
    let mut walls = Vec::new();
    for (a, b) in [([-t, -t], [4.0 + t, -t]), ([4.0 + t, -t], [4.0 + t, 5.0 + t]), ([4.0 + t, 5.0 + t], [-t, 5.0 + t]), ([-t, 5.0 + t], [-t, -t])] {
        walls.push(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } })));
    }
    ok(&mut app, "room.place", json!({ "level_id": level, "point": [2.0, 2.5] }));
    let types = ok(&mut app, "furnishing.load", json!({ "names": ["Dining table, 4 seats 48 × 30 in", "Dining table, 6 seats 72 × 36 in"] }));
    let table = ok(&mut app, "mesh_object.place", json!({ "type_id": types["Dining table, 4 seats 48 × 30 in"], "point": [2.0, 1.5], "level": level }));

    // Over the table: the room (13.1 + 16.4 ft → 29.5 in) and the table (⅔ of 48 in → 32 in)
    // allow 29.5 in, so the 24 in chandelier; 32 in above the 29 in top.
    let c = ok(&mut app, "lighting.chandelier", json!({ "over_id": table, "level": level }));
    assert_eq!(c["item"], "Chandelier 24 in", "{c:#}");
    assert!((f(&c["diameter_max"]) / IN - (4.0 + 5.0) / 0.3048).abs() < 0.1, "{c:#}");
    assert!((f(&c["bottom"]) - (29.0 * IN + 32.0 * IN)).abs() < 1e-6, "{c:#}");
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]), "over the table: no headroom rule, and 32 in is within 30–34 in");

    // In open floor under a 2.4 m ceiling: it cannot keep 80 in, and §307.4 says so.
    let low = ok(&mut app, "lighting.chandelier", json!({ "point": [3.0, 4.2], "level": level, "ceiling": 2.4 }));
    assert!(low["notes"][0].as_str().unwrap().contains("too low to keep 80 in"), "{low:#}");
    let found = ok(&mut app, "rules.check", json!({}));
    assert_eq!(found.as_array().unwrap().len(), 1, "{found:#}");
    assert_eq!((found[0]["kind"].as_str(), found[0]["rule"].as_str(), found[0]["severity"].as_str()), (Some("headroom"), Some("ADA 2010 307.4"), Some("error")));
    assert!((f(&found[0]["min"]) - 80.0 * IN).abs() < 1e-4 && f(&found[0]["measured"]) < 80.0 * IN);
    ok(&mut app, "element.delete", json!({ "element_ids": low["placed"] }));

    // Pendants over the 72 in table: two, 27 in apart between them, 33 in over its top.
    let six = ok(&mut app, "mesh_object.place", json!({ "type_id": types["Dining table, 6 seats 72 × 36 in"], "point": [2.0, 3.6], "level": level }));
    let pd = ok(&mut app, "lighting.pendants", json!({ "over_id": six, "level": level }));
    let placed = pd["placed"].as_array().unwrap().clone();
    assert_eq!(placed.len(), 2, "{pd:#}");
    assert!((f(&pd["bottom"]) - (29.0 * IN + 33.0 * IN)).abs() < 1e-6);
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]));
    // Lowered to 24 in over the table: a note against the 30–36 in practice.
    ok(&mut app, "mesh_object.set", json!({ "element_ids": [placed[0]], "offset": 29.0 * IN + 24.0 * IN }));
    let found = ok(&mut app, "rules.check", json!({}));
    assert_eq!((found.as_array().unwrap().len(), found[0]["kind"].as_str(), found[0]["severity"].as_str()), (1, Some("height_over"), Some("note")), "{found:#}");
    assert!((f(&found[0]["measured"]) - 24.0 * IN).abs() < 1e-4 && found[0]["other"] == six, "{found:#}");

    // Sconces along the north wall's room side: two on its 4.2 m, centred at 63 in, 4 in deep,
    // inside the room (the face a room is on).
    let sc = ok(&mut app, "lighting.sconces", json!({ "wall_id": walls[2], "level": level }));
    assert_eq!(sc["placed"].as_array().unwrap().len(), 2, "{sc:#}");
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let at: Vec<[f64; 3]> = sc["placed"].as_array().unwrap().iter().map(|id| {
        let e = store.get(riviera_core::ElementId(id.as_u64().unwrap())).unwrap();
        riviera_core::mesh_objects::regenerate(&store, e).unwrap().placement.origin
    }).collect();
    assert!(at.iter().all(|p| p[1] < 5.0 && p[1] > 4.8), "on the room side of the wall at y = 5.1: {at:?}");
    assert!(at.iter().all(|p| (p[2] + 6.0 * IN - 63.0 * IN).abs() < 1e-6), "centred at 63 in: {at:?}");
    let found = ok(&mut app, "rules.check", json!({}));
    assert!(found.as_array().unwrap().iter().all(|x| x["kind"] != "projection"), "4 in is ADA's limit: {found:#}");
}
