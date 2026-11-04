// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! FileEngine everywhere (COL-FE-012 … 016) against a real server, when `RIVIERA_FILEENGINE_URL`,
//! `_KEY` and `_SECRET` are set (skipped otherwise). It writes only in a folder of its own under
//! `RIVIERA_FILEENGINE_FOLDER` (default `Engineering/Riviera test`), deleted at the end: an export
//! and an import by address, a Component Library made, added to and read from a second machine,
//! a library update, a parameter library edited, and a publish set run twice.

use riviera_app::fileengine::Connections;
use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.join("fileengine.json"), dir.join("copies"));
    app
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn signed_in(app: &mut App, url: &str, key: &str, secret: &str) -> String {
    let c = ok(app, "fileengine.connect", json!({ "url": url }));
    let conn = c["id"].as_str().unwrap().to_owned();
    ok(app, "fileengine.sign_in", json!({ "connection_id": conn, "key": key, "secret": secret }));
    conn
}

#[test]
fn fileengine_everywhere_on_a_real_server() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(key), Some(secret)) = (var("RIVIERA_FILEENGINE_URL"), var("RIVIERA_FILEENGINE_KEY"), var("RIVIERA_FILEENGINE_SECRET")) else {
        eprintln!("RIVIERA_FILEENGINE_URL, _KEY and _SECRET are not all set: skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut ann = new_app(&dir.path().join("ann"));
    let conn = signed_in(&mut ann, &url, &key, &secret);
    let folder = var("RIVIERA_FILEENGINE_FOLDER").unwrap_or_else(|| "Engineering/Riviera test".to_owned());
    let base = ok(&mut ann, "fileengine.browse", json!({ "connection_id": conn, "path": folder }));
    assert_eq!(base["writable"], true, "{folder} must be writable");
    let run_name = format!("everywhere {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let sub = ok(&mut ann, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": base["folder_uid"], "name": run_name })).as_str().unwrap().to_owned();
    let cleanup = || {
        let cl = riviera_fileengine::Client::new(&url, false).unwrap();
        cl.sign_in(&key, &secret).unwrap();
        // Files first, deepest folders first, then the run's folder.
        let mut folders = vec![sub.clone()];
        let mut k = 0;
        while k < folders.len() {
            for n in cl.list(&folders[k], false).unwrap_or_default() {
                if n.is_dir() {
                    folders.push(n.uid);
                } else {
                    let _ = cl.delete_file(&n.uid);
                }
            }
            k += 1;
        }
        for f in folders.iter().rev() {
            let _ = cl.delete_dir(f);
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ok(&mut ann, "file.new", json!({}));
        let level = ok(&mut ann, "level.list", json!({}))[0]["id"].clone();
        ok(&mut ann, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } }));
        ok(&mut ann, "bcf.create_topic", json!({ "title": "Live check" }));

        // COL-FE-012: an export to an address, again as a new version, and read back.
        let out = ok(&mut ann, "bcf.export", json!({ "path": format!("fileengine://{conn}/{sub}/issues.bcf") }));
        let uid = out["fileengine"][0]["uid"].as_str().unwrap().to_owned();
        let again = ok(&mut ann, "bcf.export", json!({ "path": format!("fileengine://{conn}/{sub}/issues.bcf") }));
        assert_eq!((again["fileengine"][0]["uid"].as_str(), again["fileengine"][0]["replaced"].as_bool()), (Some(uid.as_str()), Some(true)));
        ok(&mut ann, "bcf.import", json!({ "path": format!("fileengine://{conn}/{uid}") }));

        // COL-FE-013: a library made on the server, an item added, read on a second machine.
        let lib = ok(&mut ann, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": sub, "name": "Library" })).as_str().unwrap().to_owned();
        let address = format!("fileengine://{conn}/{lib}");
        ok(&mut ann, "library.create", json!({ "path": address, "name": "Live library" }));
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("block.obj"), "mtllib block.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nusemtl m\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 3 1 4\n").unwrap();
        std::fs::write(src.join("block.mtl"), "newmtl m\nKd 0.5 0.5 0.5\n").unwrap();
        ok(&mut ann, "library.import_files", json!({ "library": address, "files": [src.join("block.obj")] }));
        let mut bob = new_app(&dir.path().join("bob"));
        let bconn = signed_in(&mut bob, &url, &key, &secret);
        ok(&mut bob, "file.new", json!({}));
        let opened = ok(&mut bob, "library.open", json!({ "path": format!("fileengine://{bconn}/{lib}") }));
        assert_eq!((opened["name"].as_str(), opened["items"].as_u64()), (Some("Live library"), Some(1)), "{opened:#}");
        let mirror = opened["path"].as_str().unwrap().to_owned();
        let blevel = ok(&mut bob, "level.list", json!({}))[0]["id"].clone();
        ok(&mut bob, "component.place", json!({ "library": mirror, "item": "block.obj", "points": [[1.0, 1.0]], "level": blevel }));
        assert_eq!(ok(&mut bob, "library.updates", json!({ "library": mirror }))[0]["state"], "current");

        // COL-FE-016: Ann changes the item; Bob sees the update and reloads it.
        std::fs::write(src.join("block.obj"), "mtllib block.mtl\nv 0 0 0\nv 2 0 0\nv 0 2 0\nv 0 0 2\nusemtl m\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 3 1 4\n").unwrap();
        let ann_mirror = riviera_app::fe_library::mirror_dir(&ann, &conn, &lib).unwrap();
        riviera_app::fe_library::pull(&mut ann, &ann_mirror, &conn, &lib).unwrap();
        std::fs::copy(src.join("block.obj"), ann_mirror.join("block.obj")).unwrap();
        riviera_app::fe_library::push(&mut ann, &ann_mirror).unwrap();
        let up = ok(&mut bob, "library.updates", json!({ "library": mirror }));
        assert_eq!(up[0]["state"], "changed", "{up:#}");
        ok(&mut bob, "library.reload_item", json!({ "library": mirror, "type_id": up[0]["type_id"] }));

        // COL-FE-014: a parameter library made on the server and edited.
        let plib = ok(&mut ann, "paramlib.create", json!({ "path": format!("fileengine://{conn}/{sub}/live.rplib.json"), "name": "Live parameters" })).as_str().unwrap().to_owned();
        ok(&mut ann, "paramlib.definition.add", json!({ "library_id": plib, "def": { "name": "Live Check", "data_type": "text", "ifc": { "pset": "ACME_Common", "property": "LiveCheck" } } }));
        let opened = ok(&mut bob, "paramlib.open", json!({ "path_or_url": format!("fileengine://{bconn}/{sub}/live.rplib.json") }));
        assert_eq!(opened["definitions"], 1);

        // COL-FE-015: a publish set run twice.
        let set = ok(&mut ann, "publish.save_set", json!({ "name": "Live issue", "connection_id": conn, "folder_uid": sub, "outputs": [
            { "command": "bcf.export", "name": "published.bcf" },
            { "command": "export.cobie", "args": { "overwrite": true }, "name": "COBie.xlsx" }
        ] }));
        let first = ok(&mut ann, "publish.run", json!({ "set_id": set["id"] }));
        assert!(first.as_array().unwrap().iter().all(|p| p["error"].is_null()), "{first:#}");
        let second = ok(&mut ann, "publish.run", json!({ "set_id": set["id"] }));
        assert!(second.as_array().unwrap().iter().all(|p| p["uploads"][0]["replaced"] == true), "{second:#}");
    }));
    cleanup();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
