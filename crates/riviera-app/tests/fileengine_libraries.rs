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

    // Ann changes the chair on the server: Bob sees *Update available*, and reloads it.
    assert_eq!(ok(&mut bob, "library.updates", json!({ "library": mirror }))[0]["state"], "current");
    let chair = fake.uid_of("Engineering/Library/chair.obj").unwrap();
    let mut bigger = String::from_utf8(fake.content(&chair).unwrap()).unwrap();
    bigger = bigger.replace("v 0 0 1", "v 0 0 2");
    fake.write_as(&chair, bigger.as_bytes(), "ann@acme.test");
    let up = ok(&mut bob, "library.updates", json!({ "library": mirror }));
    assert_eq!((up[0]["item"].as_str(), up[0]["state"].as_str(), up[0]["uses"].as_u64()), (Some("chair.obj"), Some("changed"), Some(1)), "{up:#}");
    ok(&mut bob, "library.reload_item", json!({ "library": mirror, "type_id": up[0]["type_id"] }));
    assert_eq!(ok(&mut bob, "library.updates", json!({ "library": mirror }))[0]["state"], "current");

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

/// Parameter libraries, script folders and keynote files on FileEngine (COL-FE-014).
#[test]
fn parameter_libraries_scripts_and_keynotes_on_fileengine() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let eng = fake.uid_of("Engineering").unwrap();

    // Made on FileEngine and edited: each change goes back as a new version.
    let mut ann = new_app(&dir.path().join("ann"));
    let conn = connected(&mut ann, &fake);
    let made = ok(&mut ann, "paramlib.create", json!({ "path": format!("fileengine://{conn}/{eng}/acme.rplib.json"), "name": "Acme parameters" }));
    let lib = made.as_str().unwrap().to_owned();
    let uid = fake.uid_of("Engineering/acme.rplib.json").expect("uploaded");
    ok(&mut ann, "paramlib.definition.add", json!({ "library_id": lib, "def": { "name": "Acoustic Class", "data_type": "text", "ifc": { "pset": "ACME_Common", "property": "AcousticClass" } } }));
    assert!(String::from_utf8(fake.content(&uid).unwrap()).unwrap().contains("Acoustic Class"));

    // Opened on another machine from FileEngine, by folder and name.
    let mut bob = new_app(&dir.path().join("bob"));
    let bconn = connected(&mut bob, &fake);
    let opened = ok(&mut bob, "paramlib.open", json!({ "path_or_url": format!("fileengine://{bconn}/{eng}/acme.rplib.json") }));
    assert_eq!((opened["name"].as_str(), opened["definitions"].as_u64()), (Some("Acme parameters"), Some(1)));
    // Bob adds one; Ann, who opened the older version, adds another: merged by GUID, both kept,
    // and Ann's open library becomes the merge.
    ok(&mut bob, "paramlib.definition.add", json!({ "library_id": lib, "def": { "name": "Fire Rating", "data_type": "text", "ifc": { "pset": "ACME_Common", "property": "FireRating" } } }));
    ok(&mut ann, "paramlib.definition.add", json!({ "library_id": lib, "def": { "name": "Finish", "data_type": "text", "ifc": { "pset": "ACME_Common", "property": "Finish" } } }));
    let on_server = String::from_utf8(fake.content(&uid).unwrap()).unwrap();
    assert!(on_server.contains("Fire Rating") && on_server.contains("Finish") && on_server.contains("Acoustic Class"), "{on_server}");
    let listed = ok(&mut ann, "paramlib.list", json!({}));
    assert!(listed.to_string().contains("\"definitions\":3"), "{listed}");

    // A plugin folder imported from FileEngine, and a keynote file.
    let src = dir.path().join("plugin");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("plugin.lua"), "return { title = \"Hello\", run = function() return 1 end }\n").unwrap();
    let plugins = ok(&mut ann, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Hello" })).as_str().unwrap().to_owned();
    ok(&mut ann, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": plugins, "path": src.join("plugin.lua") }));
    // Into a script library of the test's own, never the user's.
    let scripts = dir.path().join("scripts");
    // SAFETY: no other test in this binary reads the variable.
    unsafe { std::env::set_var("RIVIERA_LIBRARY", &scripts) };
    let imported = run(&mut ann, "script.import", json!({ "path": format!("fileengine://{conn}/{plugins}") }));
    assert!(imported.is_ok(), "{imported:?}");
    assert!(scripts.join("Hello").join("plugin.lua").is_file());
    let notes = dir.path().join("keynotes.txt");
    std::fs::write(&notes, "A\tGeneral\t\nA1\tSee specification\tA\n").unwrap();
    ok(&mut ann, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": eng, "path": notes }));
    ok(&mut ann, "file.new", json!({}));
    let k = ok(&mut ann, "keynotes.load", json!({ "path": format!("fileengine://{conn}/{eng}/keynotes.txt") }));
    assert_eq!(k["entries"], 2, "{k:#}");
    // Changed on the server: reloading reads the new version.
    let kuid = fake.uid_of("Engineering/keynotes.txt").unwrap();
    fake.write_as(&kuid, b"A\tGeneral\t\nA1\tSee specification\tA\nA2\tSee drawings\tA\n", "bob@acme.test");
    assert_eq!(ok(&mut ann, "keynotes.reload", json!({}))["entries"], 3);
}

/// Large library items are fetched when placed, not when the library is read (COL-FE-017).
#[test]
fn large_items_wait_until_placed() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let eng = fake.uid_of("Engineering").unwrap();
    let mut ann = new_app(&dir.path().join("ann"));
    let conn = connected(&mut ann, &fake);
    ok(&mut ann, "file.new", json!({}));
    let lib = ok(&mut ann, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Big" })).as_str().unwrap().to_owned();
    let address = format!("fileengine://{conn}/{lib}");
    ok(&mut ann, "library.create", json!({ "path": address, "name": "Big things" }));
    // An OBJ over 2 MB: a strip of many small triangles.
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut obj = String::from("mtllib hall.mtl\nusemtl m\n");
    let n = 40_000;
    for i in 0..n {
        let x = f64::from(i) * 0.001;
        obj.push_str(&format!("v {x:.6} 0.000000 0.000000\nv {x:.6} 1.000000 0.000000\n"));
    }
    for i in 0..n - 1 {
        let a = 2 * i + 1;
        obj.push_str(&format!("f {a} {} {}\nf {} {} {}\n", a + 2, a + 1, a + 1, a + 2, a + 3));
    }
    assert!(obj.len() > 2 * 1024 * 1024);
    std::fs::write(src.join("hall.obj"), &obj).unwrap();
    std::fs::write(src.join("hall.mtl"), "newmtl m\nKd 0.3 0.3 0.6\n").unwrap();
    ok(&mut ann, "library.import_files", json!({ "library": address, "files": [src.join("hall.obj")] }));

    // Bob reads the library: the big item is a placeholder, the small MTL is there.
    let mut bob = new_app(&dir.path().join("bob"));
    let bconn = connected(&mut bob, &fake);
    ok(&mut bob, "file.new", json!({}));
    let opened = ok(&mut bob, "library.open", json!({ "path": format!("fileengine://{bconn}/{lib}") }));
    let mirror = std::path::PathBuf::from(opened["path"].as_str().unwrap());
    assert_eq!(std::fs::metadata(mirror.join("hall.obj")).unwrap().len(), 0, "not downloaded yet");
    assert!(std::fs::metadata(mirror.join("hall.mtl")).unwrap().len() > 0);
    // Placing it fetches it.
    let level = ok(&mut bob, "level.list", json!({}))[0]["id"].clone();
    ok(&mut bob, "component.place", json!({ "library": mirror, "item": "hall.obj", "points": [[0.0, 0.0]], "level": level }));
    assert_eq!(std::fs::metadata(mirror.join("hall.obj")).unwrap().len(), obj.len() as u64);
    assert_eq!(ok(&mut bob, "library.updates", json!({ "library": mirror }))[0]["state"], "current");
    // A change Bob makes to the library sends nothing for an item still waiting, and keeps it.
    ok(&mut bob, "library.set_meta", json!({ "library": mirror, "paths": ["hall.obj"], "details": { "description": "The hall" } }));
    assert_eq!(fake.content(&fake.uid_of("Engineering/Big/hall.obj").unwrap()).unwrap().len(), obj.len());
}
