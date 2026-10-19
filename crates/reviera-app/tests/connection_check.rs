// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Connection code checking (STR-STL-009) with the sample Lua provider: a W18X35 shear plate with
//! three 3/4" A325 bolts under a 246 kN end reaction fails bolt shear (82 kN a bolt against
//! φRn = 0.75 · 372 MPa · 285 mm² ≈ 79.5 kN) and the report shows the ratio; Presize grows the
//! bolts (and their count, as bearing on the web then governs) until it passes, as one undo step;
//! a beam size change makes the status not calculated; a connection without forces is refused.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

const IN: f64 = 0.0254;

#[test]
fn the_sample_provider_checks_and_presizes_a_shear_plate() {
    let work = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.path().to_owned() });
    run(&mut app, "file.new", json!({}));
    run(&mut app, "code.set", json!({ "code": "ibc" }));
    let l1 = run(&mut app, "level.list", json!({}))[0]["id"].clone();
    let l2 = run(&mut app, "level.create", json!({ "elevation": 4.0 }));
    let w12 = run(&mut app, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let w18 = run(&mut app, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W18X35" }));
    let col = run(
        &mut app,
        "column.place",
        json!({ "type_id": w12, "point": [0.0, 0.0], "base_level": l1, "top_constraint": { "level": l2 } }),
    );
    let beam = run(
        &mut app,
        "beam.create",
        json!({ "type_id": w18, "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l2 }),
    );
    let sp = run(&mut app, "structural_connection.create", json!({ "member_ids": [col, beam], "type_id": "shear_plate" }));

    // The sample provider is listed.
    let providers = run(&mut app, "connection_check.providers", json!({}));
    let sample = providers.as_array().unwrap().iter().find(|p| p["id"] == "connection_check_sample").unwrap();
    assert_eq!(sample["codes"], json!(["aisc360", "en1993"]));

    // Without design forces: refused, naming the connection.
    let e = app
        .execute("structural_connection.check", json!({ "connection_ids": [sp] }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "connection_check.no_forces");
    assert!(e.message.contains(&sp.to_string()), "{e:?}");

    run(
        &mut app,
        "structural_connection.set_design_forces",
        json!({ "connection_ids": [sp], "forces": [{ "group": "ASCE7 2.3.2", "v": 246_000.0 }] }),
    );
    let checked = run(&mut app, "structural_connection.check", json!({ "connection_ids": [sp], "code": "aisc360" }));
    let rec = &checked[0]["record"];
    assert_eq!(rec["ok"], false, "{rec:#}");
    let shear = rec["items"].as_array().unwrap().iter().find(|i| i["name"] == "Bolt shear").unwrap();
    let phi_rn = 0.75 * 54.0 * 6.894_757e6 * std::f64::consts::PI * (0.75 * IN).powi(2) / 4.0;
    assert!((shear["resistance"].as_f64().unwrap() - phi_rn).abs() < 1.0, "{shear:#}");
    assert!((shear["ratio"].as_f64().unwrap() - 82_000.0 / phi_rn).abs() < 1e-6);
    let status = run(&mut app, "structural_connection.check_status", json!({ "connection_ids": [sp] }));
    assert_eq!(status[0]["status"], "failed");

    // The report shows the ratio.
    let report = work.path().join("checks.html");
    run(&mut app, "structural_connection.check_report", json!({ "connection_ids": [sp], "path": report }));
    let html = std::fs::read_to_string(&report).unwrap();
    assert!(html.contains("Bolt shear") && html.contains("Failed"));
    assert!(html.contains(&format!("{:.2}", 82_000.0 / phi_rn)));

    // Presize: bigger bolts, ok, one undo step.
    let undo_before = app.execute("edit.history", json!({}), &Caller::user()).unwrap().result;
    let sized = run(&mut app, "structural_connection.presize", json!({ "connection_ids": [sp], "code": "aisc360" }));
    let s = &sized[0];
    assert_eq!(s["record"]["ok"], true, "{s:#}");
    assert!(s["record"]["utilisation"].as_f64().unwrap() <= 1.0);
    assert!(s["sizes"]["bolt_diameter"].as_f64().unwrap() > 0.75 * IN + 1e-9, "{s:#}");
    assert_eq!(run(&mut app, "structural_connection.check_status", json!({ "connection_ids": [sp] }))[0]["status"], "ok");
    let info = run(&mut app, "structural_connection.info", json!({ "connection_ids": [sp] }))[0].clone();
    assert!(info["params"]["bolt_diameter"].as_f64().unwrap() > 0.75 * IN + 1e-9);
    let history = app.execute("edit.history", json!({}), &Caller::user()).unwrap().result;
    let n = |h: &Value| h.as_array().map_or_else(|| h["undo"].as_array().map_or(0, Vec::len), Vec::len);
    assert_eq!(n(&history), n(&undo_before) + 1, "one undo step: {history:#}");
    run(&mut app, "edit.undo", json!({}));
    let info = run(&mut app, "structural_connection.info", json!({ "connection_ids": [sp] }))[0].clone();
    assert!((info["params"]["bolt_diameter"].as_f64().unwrap() - 0.75 * IN).abs() < 1e-9, "undone");
    run(&mut app, "edit.redo", json!({}));

    // A deeper beam: the check is stale.
    let w21 = run(&mut app, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W21X44" }));
    run(&mut app, "element.set_type", json!({ "element_ids": [beam], "type_id": w21 }));
    let st = run(&mut app, "structural_connection.check_status", json!({ "connection_ids": [sp] }));
    assert_eq!(st[0]["status"], "not_calculated", "{st:#}");

    // EN 1993-1-8 works through the same provider.
    let en = run(&mut app, "structural_connection.check", json!({ "connection_ids": [sp], "code": "en1993" }));
    assert_eq!(en[0]["record"]["code"], "en1993");
    assert!(!en[0]["record"]["items"].as_array().unwrap().is_empty());
}
