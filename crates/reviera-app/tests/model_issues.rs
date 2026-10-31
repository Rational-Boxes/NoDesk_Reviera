// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Issues on a FileEngine model (COL-FE-009): the discussion threads raised on it, read when it
//! opens without changing it, shown in a 3D view, replied to, resolved and raised from a view;
//! read offline from the copy beside the working copy. Against the in-memory FileEngine here;
//! against a real one in `a_real_models_issues` when `REVIERA_FILEENGINE_URL`, `_KEY`, `_SECRET`
//! and `REVIERA_FILEENGINE_ISSUES_MODEL` (the path of a model with issues) are set: read-only.

use reviera_app::fileengine::Connections;
use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use reviera_fileengine::fake::Fake;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.join("fileengine.json"), dir.join("copies"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

/// A thread as FileEngine's 3D viewer raises one: a viewpoint anchor with a snapshot and a
/// measurement, about one wall.
fn viewer_thread(file_uid: &str, wall: &str) -> Value {
    json!({
        "id": "7d06cf1c73924613a246a881cd042e70", "file_uid": file_uid, "version": "", "title": "", "status": "open",
        "resolved_by": null, "opened_by": "karry@acme.test", "created_at": "2026-09-05T18:15:29+00:00", "updated_at": "2026-09-05T18:15:29+00:00",
        "anchor_stale": false,
        "anchor": { "kind": "model-viewpoint", "schema": "fileengine.anchor.v1",
            "viewpoint": {
                "snapshot": { "snapshot_type": "png", "snapshot_data": "data:image/png;base64,iVBORw0KGgo=" },
                "components": { "selection": [], "visibility": { "default_visibility": true, "exceptions": [] } },
                "clipping_planes": [{ "location": { "x": 3, "y": 0, "z": 1.5 }, "direction": { "x": 0, "y": 0, "z": -1 } }],
                "perspective_camera": { "field_of_view": 60, "camera_view_point": { "x": 3, "y": -8, "z": 4 },
                    "camera_direction": { "x": 0, "y": 0.894, "z": -0.447 }, "camera_up_vector": { "x": 0, "y": 0.447, "z": 0.894 } } },
            "object_refs": [{ "id": wall }],
            "measurements": [{ "type": "distance", "value": 1.0097792 }],
            "model_source": { "kind": "model" } },
        "comments": [{ "id": "547fde09a14e4cb491d27866d6372fa4", "thread_id": "7d06cf1c73924613a246a881cd042e70", "author": "karry@acme.test",
            "body": "@ann@acme.test We need to also offer an option with a conventional straight stair.\nSecond line.", "created_at": "2026-09-05T18:15:29+00:00", "deleted": false, "redacted": false }]
    })
}

#[test]
fn a_models_issues_from_filengine() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = run(&mut app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": "fesk_test", "secret": "fesks_secret" })).unwrap();
    // A model on the server, with one wall.
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let wall = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcWall").unwrap().guid().to_string();
    let eng = fake.uid_of("Engineering").unwrap();
    let up = run(&mut app, "file.save_as", json!({ "path": format!("fileengine://{conn}/{eng}/hall.ifc") })).unwrap();
    let uid = up["uploaded"]["uid"].as_str().unwrap().to_owned();
    let id = app.active().unwrap();
    run(&mut app, "file.close", json!({ "document_id": id, "save": "no" })).unwrap();
    // Not on FileEngine: no issues there to read.
    run(&mut app, "file.new", json!({})).unwrap();
    assert_eq!(run(&mut app, "fileengine.issues", json!({})).unwrap_err().code, "fileengine.not_linked");
    let id = app.active().unwrap();
    run(&mut app, "file.close", json!({ "document_id": id, "save": "no" })).unwrap();

    // Someone raises an issue in FileEngine's viewer; opening the model reads it.
    fake.add_thread(viewer_thread(&uid, &wall));
    let opened = run(&mut app, "fileengine.open", json!({ "connection_id": conn, "uid": uid })).unwrap();
    assert_eq!(opened["issues"], 1, "{opened}");
    let doc = app.active().unwrap();
    assert!(!app.document(doc).unwrap().is_dirty(), "reading issues leaves the model unchanged");
    let v = run(&mut app, "fileengine.issues", json!({})).unwrap();
    assert_eq!((v["open"].as_u64(), v["offline"].as_bool()), (Some(1), Some(false)));
    let i = &v["issues"][0];
    assert_eq!(i["title"], "We need to also offer an option with a conventional straight stair.", "the first line, without the mention");
    assert_eq!(i["opened_by"], "karry@acme.test");
    assert_eq!(i["objects"], json!([wall]));
    assert_eq!(i["measurements"], json!(["1.010 m"]));
    assert_eq!(i["has_snapshot"], true);
    assert_eq!(i["viewpoint"]["clipping_planes"][0]["location"], json!([3.0, 0.0, 1.5]));
    // The viewer's planes point to the side kept; BCF's (and Reviera's) to the side cut away.
    assert_eq!(i["viewpoint"]["clipping_planes"][0]["direction"], json!([-0.0, -0.0, 1.0]));
    let thread = i["thread_id"].as_str().unwrap().to_owned();
    assert_eq!(run(&mut app, "fileengine.issue_snapshot", json!({ "thread_id": thread })).unwrap(), "iVBORw0KGgo=");

    // Shown in a 3D view: the camera and cut, and the wall it is about selected.
    let shown = run(&mut app, "fileengine.show_issue", json!({ "thread_id": thread })).unwrap();
    assert!(shown["not_found"].as_array().unwrap().is_empty(), "{shown}");
    assert_eq!(shown["selected"], 1, "{shown}");

    // Replied to, resolved on the version open here, and opened again.
    let r = run(&mut app, "fileengine.comment_issue", json!({ "thread_id": thread, "text": "A straight-run option is in the next version." })).unwrap();
    assert_eq!(r["comments"].as_array().unwrap().len(), 2);
    assert_eq!(r["comments"][1]["author"], "ann@acme.test");
    let r = run(&mut app, "fileengine.resolve_issue", json!({ "thread_id": thread, "resolved": true })).unwrap();
    assert_eq!((r["status"].as_str(), r["resolved_by"].as_str()), (Some("resolved"), Some("ann@acme.test")));
    assert_eq!(fake.threads()[0]["resolved_version"], opened["link"]["version"]);
    let r = run(&mut app, "fileengine.resolve_issue", json!({ "thread_id": thread, "resolved": false })).unwrap();
    assert_eq!(r["status"], "open");

    // Raised from the 3D view: the server gets a viewer-shaped anchor.
    run(&mut app, "selection.set", json!({ "ids": [wall] })).ok();
    let n = run(&mut app, "fileengine.new_issue", json!({ "text": "Check the wall's fire rating\nIt borders the stair.", "title": "Fire rating" })).unwrap();
    assert_eq!((n["title"].as_str(), n["status"].as_str()), (Some("Fire rating"), Some("open")));
    let t = fake.threads().into_iter().find(|t| t["title"] == "Fire rating").unwrap();
    assert_eq!(t["anchor"]["kind"], "model-viewpoint");
    // A camera even from a view never navigated: the one its snapshot is drawn with.
    let cam = &t["anchor"]["viewpoint"]["perspective_camera"];
    assert!(cam["camera_view_point"]["x"].is_number() && cam["camera_direction"]["z"].is_number(), "{}", t["anchor"]);
    assert_eq!(t["anchor"]["object_refs"], json!([{ "id": wall }]));
    assert_eq!(t["version"], opened["link"]["version"]);
    let kept = app.document(doc).unwrap().doc.store().iter().filter(|e| e.class.starts_with("rxd:coordination/")).count();
    assert_eq!(kept, 0, "the issues stay on the server, not in the model");

    // Offline: the copy beside the working copy.
    fake.revoke_tokens();
    run(&mut app, "fileengine.sign_out", json!({ "connection_id": conn, "forget": true })).ok();
    let v = run(&mut app, "fileengine.issues", json!({ "refresh": false })).unwrap();
    assert_eq!(v["issues"].as_array().unwrap().len(), 2);
}

/// Against a real server: a model with issues, read only.
#[test]
fn a_real_models_issues() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(key), Some(secret), Some(model)) = (var("REVIERA_FILEENGINE_URL"), var("REVIERA_FILEENGINE_KEY"), var("REVIERA_FILEENGINE_SECRET"), var("REVIERA_FILEENGINE_ISSUES_MODEL")) else {
        eprintln!("REVIERA_FILEENGINE_URL, _KEY, _SECRET and _ISSUES_MODEL are not all set: skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let conn = run(&mut app, "fileengine.connect", json!({ "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": key, "secret": secret })).unwrap();
    let (folder, name) = model.rsplit_once('/').unwrap();
    let f = run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "path": folder })).unwrap();
    let uid = f["entries"].as_array().unwrap().iter().find(|e| e["name"] == name).expect("the model")["uid"].as_str().unwrap().to_owned();
    let opened = run(&mut app, "fileengine.open", json!({ "connection_id": conn, "uid": uid, "read_only": true })).unwrap();
    let v = run(&mut app, "fileengine.issues", json!({})).unwrap();
    for i in v["issues"].as_array().unwrap() {
        eprintln!("{} [{}] by {}: {} comments, {} objects, measurements {}, viewpoint {}", i["title"], i["status"], i["opened_by"], i["comments"].as_array().unwrap().len(), i["objects"].as_array().unwrap().len(), i["measurements"], !i["viewpoint"].is_null());
    }
    assert!(opened["issues"].as_u64().unwrap() > 0 && !v["issues"].as_array().unwrap().is_empty());
    let first = v["issues"].as_array().unwrap().iter().find(|i| !i["viewpoint"].is_null()).unwrap();
    let shown = run(&mut app, "fileengine.show_issue", json!({ "thread_id": first["thread_id"] })).unwrap();
    eprintln!("shown: {shown}");
    assert!(shown["not_found"].as_array().unwrap().is_empty(), "its elements are in the model: {shown}");
}
