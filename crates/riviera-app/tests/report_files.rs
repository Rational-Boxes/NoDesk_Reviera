// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Report files (DOC-OUT-016): any read-only check or report written as HTML, Markdown, CSV or
//! JSON with its header, locally or to a FileEngine folder; commands that change the model are
//! refused.

use riviera_app::fileengine::Connections;
use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use riviera_fileengine::fake::Fake;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

#[test]
fn checks_are_written_as_report_files_locally_and_to_fileengine() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.path().join("fileengine.json"), dir.path().join("copies"));
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // A bed with a chair in its ADA clear space: one error to report.
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    let t = ok(&mut app, "furnishing.load", json!({ "names": ["Bed, queen 60 × 80 in", "Task chair"] }));
    ok(&mut app, "mesh_object.place", json!({ "type_id": t["Bed, queen 60 × 80 in"], "point": [3.0, 3.0], "level": level }));
    ok(&mut app, "mesh_object.place", json!({ "type_id": t["Task chair"], "point": [1.85, 2.6], "level": level }));
    assert_eq!(ok(&mut app, "rules.check", json!({})).as_array().unwrap().len(), 1);

    // HTML by default: the header, then the findings, the error row marked.
    let html = dir.path().join("out/rules.html");
    let w = ok(&mut app, "report.write", json!({ "command": "rules.check", "path": html, "title": "Guest room check" }));
    assert_eq!((w["format"].as_str(), w["tables"].as_u64(), w["rows"].as_u64()), (Some("html"), Some(1), Some(1)), "{w:#}");
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(text.contains("<h1>Guest room check</h1>") && text.contains("<dt>Command</dt><dd>rules.check {}</dd>") && text.contains("UTC"), "{text}");
    assert!(text.contains("<tr class=\"error\">") && text.contains("ADA 2010 806.2.3 left"), "{text}");
    // CSV: the findings one row each, the severity first.
    let csv = dir.path().join("rules.csv");
    ok(&mut app, "report.write", json!({ "command": "rules.check", "path": csv }));
    let text = std::fs::read_to_string(&csv).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("severity,kind,element,rule,message") && lines[1].starts_with("error,intrusion,"), "{text}");
    // JSON: the result as returned, under a header.
    let js = dir.path().join("rules.json");
    ok(&mut app, "report.write", json!({ "command": "rules.check", "path": js }));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&js).unwrap()).unwrap();
    assert_eq!((v["report"]["command"].as_str().map(|c| c.starts_with("rules.check")), v["result"].as_array().map(Vec::len)), (Some(true), Some(1)));
    // Markdown: the packs as one table.
    let md = dir.path().join("packs.md");
    ok(&mut app, "report.write", json!({ "command": "rules.packs", "args": { "domain": "interiors" }, "path": md }));
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.starts_with("# List the rule packs of every domain\n") && text.contains("| ada_2010_interiors |") && text.contains("## Results"), "{text}");

    // Commands that change the model, or are not commands, are refused.
    for (c, a) in [("wall.create", json!({})), ("no.such", json!({})), ("report.write", json!({}))] {
        let e = app.execute("report.write", json!({ "command": c, "args": a, "path": dir.path().join("x.html") }), &Caller::user()).unwrap_err();
        assert!(e.code == "report.not_a_report" || e.code == "report.no_command", "{c}: {e:?}");
    }
    // The checks Save Report… offers.
    let list = ok(&mut app, "report.list", json!({}));
    assert!(list.as_array().unwrap().iter().any(|r| r["command"] == "rules.check") && list.as_array().unwrap().iter().any(|r| r["command"] == "bath.check"));

    // To a FileEngine folder: uploaded there, with the command that made it.
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let c = ok(&mut app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" }));
    let conn = c["id"].as_str().unwrap().to_owned();
    ok(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": "fesk_test", "secret": "fesks_secret" }));
    let eng = fake.uid_of("Engineering").unwrap();
    let out = ok(&mut app, "report.write", json!({ "command": "rules.check", "path": format!("fileengine://{conn}/{eng}/guest-room-check.html") }));
    assert_eq!(out["fileengine"][0]["name"].as_str(), Some("guest-room-check.html"), "{out:#}");
    let uid = fake.uid_of("Engineering/guest-room-check.html").expect("uploaded");
    assert!(String::from_utf8(fake.content(&uid).unwrap()).unwrap().contains("ADA 2010 806.2.3 left"));
    assert_eq!(fake.metadata(&uid).get("riviera.output").map(String::as_str), Some("report.write"));
}
