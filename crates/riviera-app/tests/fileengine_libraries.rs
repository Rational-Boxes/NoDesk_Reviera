// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Component Libraries hosted on FileEngine (COL-FE-013), against the in-memory FileEngine: a
//! library made in a FileEngine folder, an item imported into it, a second machine's mirror
//! listing and placing it, a change merged with someone else's, and an item deleted.

use riviera_app::fileengine::Connections;
use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use riviera_fileengine::fake::Fake;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.join("fileengine.json"), dir.join("copies"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> riviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn connected(app: &mut App, fake: &Fake) -> String {
    let c = ok(app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" }));
    let id = c["id"].as_str().unwrap().to_owned();
    ok(app, "fileengine.sign_in", json!({ "connection_id": id, "key": "fesk_test", "secret": "fesks_secret" }));
    id
}

fn obj(dir: &std::path::Path, name: &str, colour: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(format!("{name}.obj"));
    std::fs::write(&p, format!("mtllib {name}.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nusemtl m\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 3 1 4\n")).unwrap();
    std::fs::write(dir.join(format!("{name}.mtl")), format!("newmtl m\nKd {colour}\n")).unwrap();
    p
}

#[test]
fn a_library_shared_on_fileengine() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let eng = fake.uid_of("Engineering").unwrap();

    // Ann's machine: a library made in a FileEngine folder, an item imported into it.
    let mut ann = new_app(&dir.path().join("ann"));
    let conn = connected(&mut ann, &fake);
    ok(&mut ann, "file.new", json!({}));
    let lib = ok(&mut ann, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Library" })).as_str().unwrap().to_owned();
    let address = format!("fileengine://{conn}/{lib}");
    let made = ok(&mut ann, "library.create", json!({ "path": address, "name": "Acme components" }));
    assert!(fake.uid_of("Engineering/Library/riviera-library.json").is_some(), "{made:#}");
    ok(&mut ann, "library.import_files", json!({ "library": address, "files": [obj(&dir.path().join("src"), "chair", "0.8 0.2 0.2")] }));
    assert!(fake.uid_of("Engineering/Library/chair.obj").is_some() && fake.uid_of("Engineering/Library/chair.mtl").is_some());
    let manifest: Value = serde_json::from_slice(&fake.content(&fake.uid_of("Engineering/Library/riviera-library.json").unwrap()).unwrap()).unwrap();
    assert_eq!(manifest["name"], "Acme components");

    // Bob's machine (its own cache, the same server): opens it, lists and places the item.
    let mut bob = new_app(&dir.path().join("bob"));
    let bconn = connected(&mut bob, &fake);
    ok(&mut bob, "file.new", json!({}));
    let opened = ok(&mut bob, "library.open", json!({ "path": format!("fileengine://{bconn}/{lib}") }));
    assert_eq!((opened["name"].as_str(), opened["items"].as_u64()), (Some("Acme components"), Some(1)), "{opened:#}");
    let mirror = opened["path"].as_str().unwrap().to_owned();
    assert!(std::path::Path::new(&mirror).join("chair.obj").is_file());
    let listed = ok(&mut bob, "library.list", json!({ "library": mirror }));
    assert!(listed.to_string().contains("chair.obj"), "{listed:#}");
    let level = ok(&mut bob, "level.list", json!({}))[0]["id"].clone();
    ok(&mut bob, "component.place", json!({ "library": mirror, "item": "chair.obj", "points": [[1.0, 1.0]], "level": level }));

    // Bob adds an item through his mirror's path, while Ann's mirror still has the old manifest:
    // Ann's next change brings hers up to date first, so both items stay.
    ok(&mut bob, "library.import_files", json!({ "library": mirror, "files": [obj(&dir.path().join("src2"), "table", "0.2 0.8 0.2")] }));
    ok(&mut ann, "library.import_files", json!({ "library": address, "files": [obj(&dir.path().join("src3"), "lamp", "0.2 0.2 0.8")] }));
    let manifest: Value = serde_json::from_slice(&fake.content(&fake.uid_of("Engineering/Library/riviera-library.json").unwrap()).unwrap()).unwrap();
    let items = manifest["items"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    for want in ["chair.obj", "table.obj", "lamp.obj"] {
        assert!(items.iter().any(|k| k == want), "{want} in {items:?}");
    }

    // A change that meets someone else's on the server: merged entry by entry.
    let root = riviera_app::fe_library::mirror_dir(&ann, &conn, &lib).unwrap();
    riviera_app::fe_library::pull(&mut ann, &root, &conn, &lib).unwrap();
    let mut mine: Value = serde_json::from_str(&std::fs::read_to_string(root.join("riviera-library.json")).unwrap()).unwrap();
    mine["description"] = json!("Shared office furniture");
    std::fs::write(root.join("riviera-library.json"), serde_json::to_string_pretty(&mine).unwrap()).unwrap();
    let uid = fake.uid_of("Engineering/Library/riviera-library.json").unwrap();
    let mut theirs: Value = serde_json::from_slice(&fake.content(&uid).unwrap()).unwrap();
    theirs["items"]["chair.obj"]["name"] = json!("Office chair");
    fake.write_as(&uid, serde_json::to_string_pretty(&theirs).unwrap().as_bytes(), "bob@acme.test");
    let pushed = riviera_app::fe_library::push(&mut ann, &root).unwrap();
    assert!(pushed.merged && pushed.conflicts.is_empty(), "{pushed:?}");
    let now: Value = serde_json::from_slice(&fake.content(&uid).unwrap()).unwrap();
    assert_eq!((now["description"].as_str(), now["items"]["chair.obj"]["name"].as_str()), (Some("Shared office furniture"), Some("Office chair")));

    // Deleting an item moves it to `.deleted` on the server too.
    ok(&mut ann, "library.delete_items", json!({ "library": address, "items": ["lamp.obj"] }));
    assert!(fake.uid_of("Engineering/Library/lamp.obj").is_none());
    assert!(fake.uid_of("Engineering/Library/.deleted/lamp.obj").is_some());
}
