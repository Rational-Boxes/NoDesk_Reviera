// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Counters with height zones (ARC-INT-004) and the surfaces rule kind (ARC-RULE-002): a 42 in
//! transaction counter with a 36 in accessible portion that passes the ADA 2010 §904.4 checks;
//! a forward-approach portion open below, its knee clearance kept; a portion too high, too short,
//! or with a chair under it, reported.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

const IN: f64 = 0.0254;

fn kinds(found: &Value) -> Vec<(String, String)> {
    found.as_array().unwrap().iter().map(|f| (f["kind"].as_str().unwrap().to_owned(), f["rule"].as_str().unwrap().to_owned())).collect()
}

#[test]
fn counters_keep_their_accessible_portions() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    ok(&mut app, "rules.set_packs", json!({ "domain": "interiors", "packs": ["ada_2010_interiors", "practice_interiors"] }));
    // A wall along y = 0, 200 mm thick: its face at y = 0.1.
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] } }, "base_level": level, "top_constraint": { "unconnected": 2.7 } }));

    // A 3 m transaction counter at 42 in, its last 36 in lowered to 36 in for a parallel approach.
    let c = ok(&mut app, "counter.run", json!({ "start": [0.5, 0.1], "end": [3.5, 0.1], "level": level, "kind": "transaction", "accessible": { "approach": "parallel" } }));
    assert!((c["height"].as_f64().unwrap() - 42.0 * IN).abs() < 1e-9, "{c:#}");
    let p = &c["portion"];
    assert!((p["to"].as_f64().unwrap() - 3.0).abs() < 1e-9 && (p["to"].as_f64().unwrap() - p["from"].as_f64().unwrap() - 36.0 * IN).abs() < 1e-9, "{c:#}");
    assert_eq!(c["elements"].as_array().unwrap().len(), 4, "a body and a top each: {c:#}");
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let top = store.get(riviera_core::ElementId(p["top"].as_u64().unwrap())).unwrap();
    let it = riviera_core::rules::item(&store, top).unwrap();
    assert!((it.z[1] - 36.0 * IN).abs() < 1e-6 && (it.back - it.front - 25.0 * IN).abs() < 1e-6, "{it:?}");
    assert!((it.frame.apply([0.0, it.back, 0.0])[1] - 0.1).abs() < 1e-6, "its back on the wall face");
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]), "36 in long, 36 in high, its clear floor space free");
    assert_eq!(ok(&mut app, "overlap.find", json!({}))["overlaps"], json!([]));

    // A forward approach further along: 30 in, open below, its knee clearance free.
    let f = ok(&mut app, "counter.run", json!({ "start": [4.0, 0.1], "end": [7.0, 0.1], "level": level, "kind": "service", "accessible": { "approach": "forward", "at": "start" } }));
    assert_eq!(f["elements"].as_array().unwrap().len(), 3, "the portion has no body: {f:#}");
    assert_eq!(ok(&mut app, "rules.check", json!({})), json!([]));
    // A chair under it stands in the knee clearance and its clear floor space.
    let types = ok(&mut app, "furnishing.load", json!({ "names": ["Dining chair"] }));
    let at = 4.0 + 15.0 * IN;
    let chair = ok(&mut app, "mesh_object.place", json!({ "type_id": types["Dining chair"], "point": [at, 0.1 + 0.25], "level": level }));
    let found = kinds(&ok(&mut app, "rules.check", json!({})));
    assert!(found.contains(&("intrusion".into(), "ADA 2010 306.3".into())), "{found:?}");
    ok(&mut app, "element.delete", json!({ "element_ids": [chair] }));

    // Raised to 38 in, the parallel portion is too high; a 28 in forward portion is too short.
    ok(&mut app, "element.delete", json!({ "element_ids": c["elements"] }));
    ok(&mut app, "counter.run", json!({ "start": [0.5, 0.1], "end": [3.5, 0.1], "level": level, "kind": "transaction", "accessible": { "height": 38.0 * IN } }));
    ok(&mut app, "element.delete", json!({ "element_ids": f["elements"] }));
    ok(&mut app, "counter.run", json!({ "start": [4.0, 0.1], "end": [7.0, 0.1], "level": level, "accessible": { "approach": "forward", "at": "start", "length": 28.0 * IN } }));
    let found = ok(&mut app, "rules.check", json!({}));
    let mut k = kinds(&found);
    k.sort();
    // The short portion's 30 in knee clearance also takes in the body beside it.
    let want: Vec<(String, String)> = [("intrusion", "ADA 2010 306.3"), ("surface_height", "ADA 2010 904.4.1 surface"), ("surface_length", "ADA 2010 904.4.2 surface")].iter().map(|(a, b)| ((*a).into(), (*b).into())).collect();
    assert_eq!(k, want, "{found:#}");

    // A portion longer than the counter is refused.
    let e = app.execute("counter.run", json!({ "start": [0.5, 2.0], "end": [1.0, 2.0], "level": level, "accessible": {} }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "counter.portion_outside");
}
