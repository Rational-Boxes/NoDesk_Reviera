// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Issue coordination (COL-BCF-002, COL-BCF-004): `issues.*` over a model's own BCF topics, a
//! project on a BCF API server (an in-memory one answering as OpenConstructionERP's does), and a
//! model opened from FileEngine; both ways, open and resolved issues alike.

use riviera_app::coordination::Servers;
use riviera_app::fileengine::Connections;
use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.join("fileengine.json"), dir.join("copies"));
    *app.coordination_mut() = Servers::isolated(dir.join("coordination.json"), dir.join("issues"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> riviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

/// A model with one wall, saved at `path`; the wall's GlobalId.
fn hall(app: &mut App, path: &std::path::Path) -> String {
    run(app, "file.new", json!({})).unwrap();
    let level = run(app, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    run(app, "file.save_as", json!({ "path": path })).unwrap();
    app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcWall").unwrap().guid().to_string()
}

fn ids(v: &Value) -> Vec<String> {
    v["issues"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn a_models_own_issues_listed_open_and_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    hall(&mut app, &dir.path().join("hall.ifc"));
    assert_eq!(run(&mut app, "coordination.status", json!({})).unwrap()["source"], "model");
    let a = run(&mut app, "issues.new", json!({ "text": "Door swing clashes with the column" })).unwrap();
    let b = run(&mut app, "issues.new", json!({ "title": "Fire rating", "text": "Check the wall" })).unwrap();
    assert_eq!((a["title"].as_str(), a["kept"].as_str()), (Some("Door swing clashes with the column"), Some("model")));
    run(&mut app, "issues.set_status", json!({ "id": b["id"], "resolved": true })).unwrap();
    let all = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((all["total"].as_u64(), all["open"].as_u64(), all["resolved"].as_u64()), (Some(2), Some(1), Some(1)));
    assert_eq!(ids(&all), vec![a["id"].as_str().unwrap(), b["id"].as_str().unwrap()], "open first");
    assert_eq!(all["issues"][1]["resolved"], true);
    let open = run(&mut app, "issues.list", json!({ "status": "open" })).unwrap();
    assert_eq!(ids(&open), vec![a["id"].as_str().unwrap()]);
    let found = run(&mut app, "issues.list", json!({ "text": "fire" })).unwrap();
    assert_eq!(ids(&found), vec![b["id"].as_str().unwrap()]);
    let r = run(&mut app, "issues.update", json!({ "id": a["id"], "fields": { "priority": "High", "assigned_to": "ann@acme.test" } })).unwrap();
    assert_eq!((r["priority"].as_str(), r["assigned_to"].as_str()), (Some("High"), Some("ann@acme.test")));
    let r = run(&mut app, "issues.comment", json!({ "id": a["id"], "text": "Moved the door" })).unwrap();
    assert_eq!(r["comments"].as_array().unwrap().len(), 1);
    assert_eq!(run(&mut app, "issues.publish", json!({})).unwrap_err().code, "issues.no_server");
}

#[test]
fn a_model_bound_to_a_bcf_server() {
    use riviera_opencde::fake::Fake;
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("ann@acme.test", "pw");
    let mut app = new_app(dir.path());
    let wall = hall(&mut app, &dir.path().join("hall.ifc"));
    // Made before the model was bound: kept with the model.
    let before = run(&mut app, "issues.new", json!({ "title": "Clash: duct and beam", "text": "Found by the clash check" })).unwrap();
    run(&mut app, "issues.comment", json!({ "id": before["id"], "text": "Seen" })).unwrap();
    run(&mut app, "file.save", json!({})).unwrap();

    // The server, found from its root, signed in by e-mail and password.
    assert_eq!(run(&mut app, "coordination.add_server", json!({ "name": "ERP", "url": fake.url, "user": "ann@acme.test", "secret": "wrong" })).unwrap_err().code, "coordination.not_authorised");
    let s = run(&mut app, "coordination.add_server", json!({ "name": "ERP", "url": fake.url, "user": "ann@acme.test", "secret": "pw" })).unwrap();
    assert!(s["url"].as_str().unwrap().ends_with("/api/v1/bcf/"), "{s}");
    assert_eq!((s["version"].as_str(), s["secret"].as_str()), (Some("3.0"), Some("session")));
    assert!(s.get("secret_value").is_none());
    let server = s["id"].as_str().unwrap().to_owned();
    let projects = run(&mut app, "coordination.projects", json!({ "server_id": server })).unwrap();
    assert_eq!(projects[0]["name"], "Hall");
    run(&mut app, "coordination.bind", json!({ "server_id": server, "project_id": Fake::PROJECT })).unwrap();
    let st = run(&mut app, "coordination.status", json!({})).unwrap();
    assert_eq!((st["source"].as_str(), st["server"].as_str(), st["project"].as_str()), (Some("bcf_api"), Some("ERP"), Some("Hall")));
    assert!(!app.document(app.active().unwrap()).unwrap().is_dirty(), "binding leaves the model unchanged");

    // Issues raised in the server's web pages, one of them closed.
    fake.add_topic(json!({ "guid": "aaaaaaaa-0000-4000-8000-000000000001", "title": "Wall too thin", "topic_status": "Open", "topic_type": "Issue", "priority": "High",
        "creation_author": "bob@acme.test", "creation_date": "2026-11-01T09:00:00Z", "modified_date": "2026-11-01T09:00:00Z",
        "comments": [{ "guid": "c1", "comment": "Needs 200 mm", "author": "bob@acme.test", "date": "2026-11-01T09:00:00Z" }],
        "viewpoints": [{ "guid": "v1", "perspective_camera": { "camera_view_point": { "x": 3, "y": -10, "z": 2 }, "camera_direction": { "x": 0, "y": 1, "z": 0 }, "camera_up_vector": { "x": 0, "y": 0, "z": 1 }, "field_of_view": 60 },
                         "components": { "selection": [{ "ifc_guid": wall }] } }] }), Some(b"\x89PNG\r\n"));
    fake.add_topic(json!({ "guid": "aaaaaaaa-0000-4000-8000-000000000002", "title": "Old question", "topic_status": "Closed", "creation_author": "bob@acme.test",
        "creation_date": "2026-10-01T09:00:00Z", "comments": [], "viewpoints": [] }), None);
    let v = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((v["source"].as_str(), v["total"].as_u64(), v["open"].as_u64(), v["resolved"].as_u64(), v["local"].as_u64()), (Some("bcf_api"), Some(3), Some(2), Some(1), Some(1)), "{v}");
    let thin = v["issues"].as_array().unwrap().iter().find(|i| i["title"] == "Wall too thin").unwrap();
    assert_eq!((thin["kept"].as_str(), thin["objects"].clone(), thin["comments"][0]["text"].as_str()), (Some("server"), json!([wall]), Some("Needs 200 mm")));
    assert_eq!(v["issues"].as_array().unwrap().last().unwrap()["status"], "Closed", "resolved ones last, marked");
    assert_eq!(run(&mut app, "issues.snapshot", json!({ "id": thin["id"] })).unwrap(), "iVBORw0K");
    let shown = run(&mut app, "issues.show", json!({ "id": thin["id"] })).unwrap();
    assert_eq!(shown["selected"], 1, "{shown}");

    // Back to the server: a reply, edits, closing and reopening.
    let r = run(&mut app, "issues.comment", json!({ "id": thin["id"], "text": "Made it 200 mm" })).unwrap();
    assert_eq!(r["comments"][1]["author"], "ann@acme.test");
    let r = run(&mut app, "issues.update", json!({ "id": thin["id"], "fields": { "assigned_to": "ann@acme.test", "labels": ["structure"] } })).unwrap();
    assert_eq!((r["assigned_to"].as_str(), r["priority"].as_str(), r["labels"].clone()), (Some("ann@acme.test"), Some("High"), json!(["structure"])));
    let r = run(&mut app, "issues.set_status", json!({ "id": thin["id"], "resolved": true })).unwrap();
    assert_eq!((r["status"].as_str(), r["resolved"].as_bool()), (Some("Closed"), Some(true)));
    let r = run(&mut app, "issues.set_status", json!({ "id": thin["id"], "resolved": false })).unwrap();
    assert_eq!(r["status"], "Open");

    // Raised here from the 3D view: on the server, with a viewpoint.
    run(&mut app, "selection.set", json!({ "ids": [wall] })).ok();
    let n = run(&mut app, "issues.new", json!({ "title": "Fire rating", "text": "Borders the stair", "fields": { "priority": "Normal" } })).unwrap();
    assert_eq!(n["kept"], "server");
    let t = fake.topics().into_iter().find(|t| t["title"] == "Fire rating").unwrap();
    assert_eq!((t["description"].as_str(), t["priority"].as_str(), t["viewpoints"].as_array().unwrap().len()), (Some("Borders the stair"), Some("Normal"), 1));
    assert!(t["viewpoints"][0]["perspective_camera"].is_object() || t["viewpoints"][0]["orthogonal_camera"].is_object(), "{t}");

    // The model's own topic put on the server, GUID and comment kept; once only.
    let p = run(&mut app, "issues.publish", json!({})).unwrap();
    assert_eq!(p["published"].as_array().unwrap().len(), 1, "{p}");
    let up = fake.topics().into_iter().find(|t| t["guid"] == before["id"]).expect("same GUID on the server");
    assert!(up["comments"][0]["comment"].as_str().unwrap().ends_with(": Seen"), "{up}");
    assert_eq!(run(&mut app, "issues.publish", json!({})).unwrap()["already"], 1);
    let v = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((v["total"].as_u64(), v["local"].as_u64()), (Some(4), Some(0)));

    // Out as a BCF file, server issues and all.
    let path = dir.path().join("out.bcf");
    let x = run(&mut app, "issues.export_bcf", json!({ "path": path })).unwrap();
    assert_eq!((x["topics"].as_u64(), x["snapshots"].as_u64().unwrap() >= 1), (Some(4), true), "{x}");

    // Deleted on the server.
    run(&mut app, "issues.delete", json!({ "id": "aaaaaaaa-0000-4000-8000-000000000002" })).unwrap();
    assert_eq!(fake.topics().len(), 3);

    // Unbound: the model's own again.
    run(&mut app, "coordination.unbind", json!({})).unwrap();
    assert_eq!(run(&mut app, "issues.list", json!({})).unwrap()["source"], "model");
}

#[test]
fn a_fileengine_models_own_topics_published() {
    use riviera_fileengine::fake::Fake;
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = run(&mut app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": "fesk_test", "secret": "fesks_secret" })).unwrap();
    hall(&mut app, &dir.path().join("hall.ifc"));
    let eng = fake.uid_of("Engineering").unwrap();
    run(&mut app, "file.save_as", json!({ "path": format!("fileengine://{conn}/{eng}/hall.ifc") })).unwrap();
    assert_eq!(run(&mut app, "coordination.status", json!({})).unwrap()["source"], "fileengine");

    // A clash topic kept with the model, beside the server's (none yet).
    let local = run(&mut app, "issues.new", json!({ "title": "Clash: pipe and beam", "text": "From the clash check", "in_model": true, "fields": { "priority": "High" } })).unwrap();
    let v = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((v["source"].as_str(), v["local"].as_u64()), (Some("fileengine"), Some(1)));

    // Raised on the server, resolved there: still listed, marked.
    let s = run(&mut app, "issues.new", json!({ "title": "Stair headroom", "text": "Check it" })).unwrap();
    assert_eq!(s["kept"], "server");
    run(&mut app, "issues.set_status", json!({ "id": s["id"], "resolved": true })).unwrap();
    let v = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((v["total"].as_u64(), v["resolved"].as_u64()), (Some(2), Some(1)));
    assert_eq!(run(&mut app, "issues.update", json!({ "id": s["id"], "fields": { "title": "x" } })).unwrap_err().code, "issues.not_supported");

    let p = run(&mut app, "issues.publish", json!({})).unwrap();
    assert_eq!(p["published"].as_array().unwrap().len(), 1, "{p}");
    let t = fake.threads().into_iter().find(|t| t["title"] == "Clash: pipe and beam").unwrap();
    assert_eq!(t["anchor"]["kind"], "model-viewpoint");
    assert!(t["comments"][0]["body"].as_str().unwrap().contains("Priority: High"), "{t}");
    let v = run(&mut app, "issues.list", json!({})).unwrap();
    assert_eq!((v["total"].as_u64(), v["local"].as_u64()), (Some(2), Some(0)), "published once, listed once: {v}");
    assert_eq!(run(&mut app, "issues.publish", json!({})).unwrap()["already"], 1);
    let _ = local;
}
