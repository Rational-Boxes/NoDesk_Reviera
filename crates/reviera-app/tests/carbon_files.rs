// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embodied carbon files (ANA-CARB-001): a factor library read from the Reviera CSV format
//! (importing it again replaces its factors and keeps the matches), a bad line refused by
//! number, and a run exported as CSV and JSON.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

#[test]
fn factor_import_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [10, 0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }));
    let material = ok(&mut app, "embodied_carbon.materials", json!({}))[0]["material"].as_str().unwrap().to_owned();

    let csv = dir.path().join("factors.csv");
    std::fs::write(&csv, "# made-up test values\nid,name,class,unit,a1_a3,c1_c4,d\nx1,Generic structure,structure,m3,300,10,-20\n").unwrap();
    let rep = ok(&mut app, "embodied_carbon.import_factors", json!({ "path": csv }));
    assert_eq!(rep["factors"], 1);
    let lib = rep["library_id"].as_str().unwrap().to_owned();
    ok(&mut app, "embodied_carbon.match", json!({ "material": material, "factor_ref": format!("{lib}/x1") }));
    // Again, with another value: the match stays.
    std::fs::write(&csv, "id,name,class,unit,a1_a3\nx1,Generic structure,structure,m3,250\n").unwrap();
    ok(&mut app, "embodied_carbon.import_factors", json!({ "path": csv }));
    assert_eq!(ok(&mut app, "embodied_carbon.materials", json!({}))[0]["status"], "matched");
    std::fs::write(&csv, "id,name,unit,a1_a3\nx1,X,m3,abc\n").unwrap();
    let e = run(&mut app, "embodied_carbon.import_factors", json!({ "path": csv })).unwrap_err();
    assert!(e.message.starts_with("line 2"), "{}", e.message);

    let r = ok(&mut app, "embodied_carbon.run", json!({}));
    assert!(r["totals"]["a1_a3"].as_f64().unwrap() > 0.0);
    let out = dir.path().join("carbon.csv");
    ok(&mut app, "embodied_carbon.export", json!({ "path": out }));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().any(|l| l.starts_with("total,all,")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("element,")));
    let js = dir.path().join("carbon.json");
    ok(&mut app, "embodied_carbon.export", json!({ "path": js, "format": "json" }));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&js).unwrap()).unwrap();
    assert_eq!(v["id"], r["id"]);
}
