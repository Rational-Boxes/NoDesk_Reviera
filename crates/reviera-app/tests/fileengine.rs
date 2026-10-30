// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! FileEngine models (COL-FE-001…005): connections and signing in, browsing, saving a model to a
//! folder, Ctrl+S as a new version with the model's metadata, someone else saving first, opening
//! by `fileengine://`, versions and restore, read-only models. Against the in-memory FileEngine
//! here; against a real one in `a_real_server_round_trip` when `REVIERA_FILEENGINE_URL`, `_KEY`
//! and `_SECRET` are set (it writes only in `REVIERA_FILEENGINE_FOLDER`, default
//! `Engineering/Reviera test`).

use reviera_app::fileengine::{Connections, link_of};
use reviera_app::{App, AppConfig};
use reviera_core::{Caller, Origin};
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

fn add_wall(app: &mut App, y: f64) {
    let level = run(app, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(app, "wall.create", json!({ "curve": { "line": { "start": [0, y], "end": [6, y] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
}

fn close(app: &mut App) {
    let id = app.active().unwrap();
    run(app, "file.close", json!({ "document_id": id, "save": "no" })).unwrap();
}

fn walls(app: &App) -> usize {
    app.document(app.active().unwrap()).unwrap().doc.store().iter().filter(|e| e.class == "IfcWall").count()
}

/// A connection to `fake`, signed in as ann.
fn connected(app: &mut App, fake: &Fake) -> String {
    let c = run(app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" })).unwrap();
    let id = c["id"].as_str().unwrap().to_owned();
    run(app, "fileengine.sign_in", json!({ "connection_id": id, "key": "fesk_test", "secret": "fesks_secret" })).unwrap();
    id
}

#[test]
fn connections_and_signing_in() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    assert_eq!(run(&mut app, "fileengine.connect", json!({ "url": "http://files.example.org" })).unwrap_err().code, "fileengine.bad_url");
    let c = run(&mut app, "fileengine.connect", json!({ "url": fake.url })).unwrap();
    assert_eq!((c["name"].as_str(), c["secret"].as_str(), c["connected"].as_bool()), (Some("localhost"), Some("none"), Some(false)));
    assert!(c["url"].as_str().unwrap().ends_with("/api/"));
    assert_eq!(run(&mut app, "fileengine.connect", json!({ "url": fake.url })).unwrap_err().code, "fileengine.duplicate");
    let id = c["id"].as_str().unwrap();

    assert_eq!(run(&mut app, "fileengine.whoami", json!({ "connection_id": id })).unwrap_err().code, "fileengine.not_signed_in");
    assert_eq!(run(&mut app, "fileengine.sign_in", json!({ "connection_id": id, "key": "fesk_test", "secret": "nope" })).unwrap_err().code, "fileengine.not_authorised");
    // An agent never handles a credential, and cannot remove one.
    let mcp = Caller { origin: Origin::mcp("probe", "s1"), ..Caller::user() };
    assert_eq!(app.execute("fileengine.sign_in", json!({ "connection_id": id, "key": "fesk_test:fesks_secret" }), &mcp).unwrap_err().code, "permission.denied");
    // key:secret in one.
    let who = run(&mut app, "fileengine.sign_in", json!({ "connection_id": id, "key": "fesk_test:fesks_secret" })).unwrap();
    assert_eq!((who["user"].as_str(), who["tenant"].as_str(), who["secret"].as_str()), (Some("ann@acme.test"), Some("acme"), Some("session")));
    let listed = run(&mut app, "fileengine.list_connections", json!({})).unwrap();
    assert_eq!((listed[0]["user"].as_str(), listed[0]["connected"].as_bool()), (Some("ann@acme.test"), Some(true)));
    assert!(!listed.to_string().contains("fesks_secret"), "the credential is never listed: {listed}");
    let settings = std::fs::read_to_string(dir.path().join("fileengine.json")).unwrap();
    assert!(!settings.contains("fesk_test") && !settings.contains("fesks_secret"), "nor written to the settings: {settings}");

    // Signed out: the stored credential signs in again on demand (an agent may use it).
    run(&mut app, "fileengine.sign_out", json!({ "connection_id": id })).unwrap();
    assert_eq!(app.execute("fileengine.whoami", json!({ "connection_id": id }), &mcp).unwrap().result["user"], "ann@acme.test");
    run(&mut app, "fileengine.sign_out", json!({ "connection_id": id, "forget": true })).unwrap();
    assert_eq!(run(&mut app, "fileengine.whoami", json!({ "connection_id": id })).unwrap_err().code, "fileengine.not_signed_in");
    assert_eq!(app.execute("fileengine.remove", json!({ "connection_id": id }), &mcp).unwrap_err().code, "permission.denied");
    run(&mut app, "fileengine.remove", json!({ "connection_id": id })).unwrap();
    assert!(run(&mut app, "fileengine.list_connections", json!({})).unwrap().as_array().unwrap().is_empty());
}

#[test]
fn save_to_a_folder_then_save_new_versions() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = connected(&mut app, &fake);

    let top = run(&mut app, "fileengine.browse", json!({ "connection_id": conn })).unwrap();
    assert_eq!(top["entries"][0]["name"], "Engineering");
    let eng = top["entries"][0]["uid"].as_str().unwrap().to_owned();
    let folder = run(&mut app, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Reviera test" })).unwrap();
    let folder = folder.as_str().unwrap().to_owned();
    assert_eq!(run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "path": "Engineering/Reviera test" })).unwrap()["folder_uid"], folder);

    run(&mut app, "file.new", json!({})).unwrap();
    add_wall(&mut app, 0.0);
    assert_eq!(run(&mut app, "fileengine.status", json!({})).unwrap()["linked"], false);
    let saved = run(&mut app, "file.save_as", json!({ "path": format!("fileengine://{conn}/{folder}/tower.ifc") })).unwrap();
    let up = &saved["uploaded"];
    let uid = up["uid"].as_str().unwrap().to_owned();
    let v1 = up["version"].as_str().unwrap().to_owned();
    assert!(!v1.is_empty());
    assert!(String::from_utf8(fake.content(&uid).unwrap()).unwrap().starts_with("ISO-10303-21;"));
    let meta = fake.metadata(&uid);
    assert_eq!(meta.get("ifc.schema").map(String::as_str), Some("IFC4"));
    assert_eq!(meta.get("ifc.project_guid").map(String::len), Some(22));
    assert!(meta.get("reviera.levels").unwrap().starts_with('['), "{meta:?}");
    // The document is the FileEngine model now.
    let status = run(&mut app, "fileengine.status", json!({})).unwrap();
    assert_eq!((status["linked"].as_bool(), status["link"]["uid"].as_str(), status["link"]["version"].as_str()), (Some(true), Some(uid.as_str()), Some(v1.as_str())));
    let path = std::path::PathBuf::from(saved["path"].as_str().unwrap());
    assert!(path.starts_with(dir.path().join("copies")) && path.ends_with("tower.ifc"), "{path:?}");
    // The name is taken now.
    run(&mut app, "file.new", json!({})).unwrap();
    assert_eq!(run(&mut app, "file.save_as", json!({ "path": format!("fileengine://{conn}/{folder}/tower.ifc") })).unwrap_err().code, "fileengine.exists");
    close(&mut app);

    // Ctrl+S uploads a new version; the comment is kept with it.
    add_wall(&mut app, 5.0);
    let s2 = run(&mut app, "file.save", json!({})).unwrap();
    let v2 = s2["uploaded"]["version"].as_str().unwrap().to_owned();
    assert!(v2 > v1);
    add_wall(&mut app, 10.0);
    let s3 = run(&mut app, "fileengine.save", json!({ "comment": "Third wall" })).unwrap();
    let v3 = s3["version"].as_str().unwrap().to_owned();
    let versions = run(&mut app, "fileengine.versions", json!({})).unwrap();
    let rows = versions.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[0]["version"].as_str(), rows[0]["current"].as_bool(), rows[0]["newest"].as_bool()), (Some(v3.as_str()), Some(true), Some(true)));
    assert_eq!((rows[0]["comment"].as_str(), rows[0]["saved_by"].as_str()), (Some("Third wall"), Some("ann@acme.test")));
    assert!(rows[1]["comment"].is_null());

    // Restore the first version: a new version with its content, and the model reopened at it.
    let r = run(&mut app, "fileengine.restore_version", json!({ "version": v1 })).unwrap();
    assert_eq!(r["restored"], v1.as_str());
    assert_eq!(walls(&app), 1);
    assert_eq!(run(&mut app, "fileengine.versions", json!({})).unwrap().as_array().unwrap().len(), 4);
    assert_eq!(run(&mut app, "fileengine.status", json!({})).unwrap()["link"]["version"], r["version"]);

    // A past version opens read-only and cannot be saved over the file.
    let doc = app.active().unwrap();
    let old = run(&mut app, "file.open", json!({ "path": format!("fileengine://{conn}/{uid}?version={v3}") })).unwrap();
    assert_ne!(old, json!(doc));
    assert_eq!(walls(&app), 3);
    assert_eq!(run(&mut app, "fileengine.status", json!({})).unwrap()["link"]["historic"], true);
    assert_eq!(run(&mut app, "fileengine.save", json!({})).unwrap_err().code, "fileengine.read_only");
}

#[test]
fn someone_else_saved_first() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = connected(&mut app, &fake);
    let eng = fake.uid_of("Engineering").unwrap();
    run(&mut app, "file.new", json!({})).unwrap();
    add_wall(&mut app, 0.0);
    let saved = run(&mut app, "fileengine.save_as", json!({ "connection_id": conn, "folder_uid": eng, "name": "hall.ifc" })).unwrap();
    let uid = saved["uid"].as_str().unwrap().to_owned();
    // Bob's version: an empty model.
    let mut other = new_app(&dir.path().join("bob"));
    run(&mut other, "file.new", json!({})).unwrap();
    let bob_path = dir.path().join("bob.ifc");
    run(&mut other, "file.save_as", json!({ "path": bob_path })).unwrap();
    let bob = std::fs::read(&bob_path).unwrap();
    let bob = bob.as_slice();
    fake.write_as(&uid, bob, "bob@acme.test");

    add_wall(&mut app, 5.0);
    let e = run(&mut app, "file.save", json!({})).unwrap_err();
    assert_eq!(e.code, "fileengine.conflict");
    assert_eq!(e.details["saved_by"], "bob@acme.test");
    assert!(e.message.starts_with("Saved on this computer"), "{e:?}");
    assert_eq!(fake.content(&uid).unwrap(), bob, "bob's version is untouched");
    let path = app.document(app.active().unwrap()).unwrap().project.path().to_owned();
    assert!(link_of(&path).unwrap().unsent);
    // Opening it again keeps the unsent work, never overwritten by the download.
    let again = run(&mut app, "fileengine.open", json!({ "connection_id": conn, "uid": uid })).unwrap();
    assert_eq!((again["existing"].as_bool(), again["newer_on_server"].as_bool()), (Some(true), Some(true)));
    assert_eq!(walls(&app), 2);

    // Saved as a copy beside it: the document is the copy now; the original stays bob's.
    let copy = run(&mut app, "fileengine.save", json!({ "on_conflict": "copy" })).unwrap();
    assert_eq!((copy["name"].as_str(), copy["copy_of"].as_str()), (Some("hall (ann's copy).ifc"), Some(uid.as_str())));
    assert_eq!(fake.content(&uid).unwrap(), bob);
    assert_eq!(run(&mut app, "fileengine.status", json!({})).unwrap()["link"]["name"], "hall (ann's copy).ifc");
    let names: Vec<String> = run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "folder_uid": eng })).unwrap()["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["hall (ann's copy).ifc", "hall.ifc"]);

    // Or over it: bob's stays in the history.
    let orig = run(&mut app, "file.open", json!({ "path": format!("fileengine://{conn}/{uid}") })).unwrap();
    assert!(orig.is_number());
    assert_eq!(walls(&app), 0, "bob's (empty) model");
    add_wall(&mut app, 0.0);
    fake.write_as(&uid, bob, "bob@acme.test");
    assert_eq!(run(&mut app, "file.save", json!({})).unwrap_err().code, "fileengine.conflict");
    run(&mut app, "fileengine.save", json!({ "on_conflict": "overwrite" })).unwrap();
    assert_ne!(fake.content(&uid).unwrap(), bob);
    let versions = run(&mut app, "fileengine.versions", json!({})).unwrap();
    assert_eq!(versions[1]["saved_by"], "bob@acme.test");
}

#[test]
fn a_model_the_user_may_not_write_opens_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = connected(&mut app, &fake);
    let eng = fake.uid_of("Engineering").unwrap();
    run(&mut app, "file.new", json!({})).unwrap();
    let up = run(&mut app, "fileengine.save_as", json!({ "connection_id": conn, "folder_uid": eng, "name": "site.ifc" })).unwrap();
    let uid = up["uid"].as_str().unwrap().to_owned();
    close(&mut app);
    fake.make_read_only(&eng);
    assert_eq!(run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "folder_uid": eng })).unwrap()["writable"], false);
    run(&mut app, "fileengine.open", json!({ "connection_id": conn, "uid": uid })).unwrap();
    let st = run(&mut app, "fileengine.status", json!({})).unwrap();
    assert_eq!(st["link"]["read_only"], true);
    assert_eq!(run(&mut app, "file.save", json!({})).unwrap_err().code, "file.read_only");
    assert_eq!(run(&mut app, "fileengine.save", json!({})).unwrap_err().code, "fileengine.read_only");
    // A folder does not open, nor a bad address.
    assert_eq!(run(&mut app, "fileengine.open", json!({ "connection_id": conn, "uid": eng })).unwrap_err().code, "fileengine.not_a_model");
    assert_eq!(run(&mut app, "file.open", json!({ "path": "fileengine://only-one-part" })).unwrap_err().code, "fileengine.bad_address");
}

#[test]
fn a_real_server_round_trip() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(key), Some(secret)) = (var("REVIERA_FILEENGINE_URL"), var("REVIERA_FILEENGINE_KEY"), var("REVIERA_FILEENGINE_SECRET")) else {
        eprintln!("REVIERA_FILEENGINE_URL, _KEY and _SECRET are not all set: skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let c = run(&mut app, "fileengine.connect", json!({ "url": url })).unwrap();
    let conn = c["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": key, "secret": secret })).unwrap();
    let folder = var("REVIERA_FILEENGINE_FOLDER").unwrap_or_else(|| "Engineering/Reviera test".to_owned());
    let base = run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "path": folder })).unwrap();
    assert_eq!(base["writable"], true, "{folder} must be writable");
    let run_name = format!("app {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let sub = run(&mut app, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": base["folder_uid"], "name": run_name })).unwrap();
    let sub = sub.as_str().unwrap().to_owned();

    run(&mut app, "file.new", json!({})).unwrap();
    add_wall(&mut app, 0.0);
    let saved = run(&mut app, "file.save_as", json!({ "path": format!("fileengine://{conn}/{sub}/probe.ifc") })).unwrap();
    let uid = saved["uploaded"]["uid"].as_str().unwrap().to_owned();
    std::thread::sleep(std::time::Duration::from_millis(20));
    add_wall(&mut app, 5.0);
    run(&mut app, "fileengine.save", json!({ "comment": "second wall" })).unwrap();
    let versions = run(&mut app, "fileengine.versions", json!({})).unwrap();
    assert_eq!(versions.as_array().unwrap().len(), 2);
    assert_eq!(versions[0]["comment"], "second wall");
    // Reopened from the server: the same two walls.
    close(&mut app);
    std::fs::remove_dir_all(dir.path().join("copies")).unwrap();
    run(&mut app, "file.open", json!({ "path": format!("fileengine://{conn}/{uid}") })).unwrap();
    assert_eq!(walls(&app), 2);
    close(&mut app);

    // Clean up with the client (deleting is not a Reviera command).
    let cl = reviera_fileengine::Client::new(&url, false).unwrap();
    cl.sign_in(&key, &secret).unwrap();
    cl.delete_file(&uid).unwrap();
    cl.delete_dir(&sub).unwrap();
}
