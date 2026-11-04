// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! FileEngine wherever a file is read or written (COL-FE-012), against the in-memory FileEngine:
//! an export written to a FileEngine address is uploaded with its metadata, and again as a new
//! version; an import reads from an address; an OBJ comes with its MTL from its folder; a folder
//! of CSV files is uploaded file by file; a bad address or connection stops the command before it
//! runs; every command that takes a path marks it.

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

#[test]
fn exports_and_imports_take_fileengine_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = connected(&mut app, &fake);
    let eng = fake.uid_of("Engineering").unwrap();
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } }));
    ok(&mut app, "bcf.create_topic", json!({ "title": "Check the sill" }));

    // An export to a FileEngine address: uploaded, with what it came from.
    let out = ok(&mut app, "bcf.export", json!({ "path": format!("fileengine://{conn}/{eng}/issues.bcf") }));
    let up = &out["fileengine"][0];
    assert_eq!((up["name"].as_str(), up["replaced"].as_bool(), up["parameter"].as_str()), (Some("issues.bcf"), Some(false), Some("/path")), "{out:#}");
    let uid = fake.uid_of("Engineering/issues.bcf").expect("uploaded");
    assert!(fake.content(&uid).unwrap().starts_with(b"PK"), "a BCF archive");
    let meta = fake.metadata(&uid);
    assert_eq!(meta.get("riviera.output").map(String::as_str), Some("bcf.export"));
    assert!(meta.contains_key("riviera.published_at") && meta.contains_key("ifc.project_guid"), "{meta:?}");
    // Again: a new version of the same file.
    let again = ok(&mut app, "bcf.export", json!({ "path": format!("fileengine://{conn}/{eng}/issues.bcf") }));
    assert_eq!((again["fileengine"][0]["uid"].as_str(), again["fileengine"][0]["replaced"].as_bool()), (Some(uid.as_str()), Some(true)));

    // An import from an address (by uid, and by folder and name).
    let r = ok(&mut app, "bcf.import", json!({ "path": format!("fileengine://{conn}/{uid}") }));
    assert!(r.is_object(), "{r:#}");
    ok(&mut app, "bcf.import", json!({ "path": format!("fileengine://{conn}/{eng}/issues.bcf") }));

    // An OBJ with its MTL, read from its folder together.
    let local = dir.path().join("mesh");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("box.obj"), "mtllib box.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nusemtl red\nf 1 3 2\nf 1 4 3\nf 5 6 7\nf 5 7 8\nf 1 2 6\nf 1 6 5\nf 2 3 7\nf 2 7 6\nf 3 4 8\nf 3 8 7\nf 4 1 5\nf 4 5 8\n").unwrap();
    std::fs::write(local.join("box.mtl"), "newmtl red\nKd 0.8 0.1 0.1\n").unwrap();
    let meshes = ok(&mut app, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Meshes" })).as_str().unwrap().to_owned();
    for f in ["box.obj", "box.mtl"] {
        ok(&mut app, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": meshes, "path": local.join(f) }));
    }
    let local_report = ok(&mut app, "mesh_object.inspect", json!({ "path": local.join("box.obj") }));
    let remote_report = ok(&mut app, "mesh_object.inspect", json!({ "path": format!("fileengine://{conn}/{meshes}/box.obj") }));
    assert_eq!(remote_report, local_report, "the MTL came with the OBJ");
    ok(&mut app, "mesh_object.import", json!({ "path": format!("fileengine://{conn}/{meshes}/box.obj"), "name": "Box" }));

    // A folder of CSV files (COBie as CSV), uploaded one by one.
    let cobie = ok(&mut app, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "COBie" })).as_str().unwrap().to_owned();
    let out = ok(&mut app, "export.cobie", json!({ "path": format!("fileengine://{conn}/{cobie}"), "format": "csv" }));
    let names: Vec<&str> = out["fileengine"].as_array().unwrap().iter().filter_map(|u| u["name"].as_str()).collect();
    assert!(names.len() > 5 && names.iter().all(|n| n.ends_with(".csv")), "{names:?}");
    assert!(fake.uid_of(&format!("Engineering/COBie/{}", names[0])).is_some());

    // Before it runs: an address with no name where a file is written, an unknown connection.
    assert_eq!(run(&mut app, "bcf.export", json!({ "path": format!("fileengine://{conn}/{eng}") })).unwrap_err().code, "fileengine.bad_address");
    assert!(run(&mut app, "bcf.export", json!({ "path": format!("fileengine://nobody/{eng}/x.bcf") })).is_err());
    assert!(fake.uid_of("Engineering/x.bcf").is_none());
}

#[test]
fn every_command_that_takes_a_path_marks_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = new_app(dir.path());
    let mut marked = std::collections::BTreeMap::new();
    for spec in app.registry().iter() {
        let mut found = Vec::new();
        riviera_app::fe_paths::marked_parameters(&spec.params, &spec.params, "", &mut found, 0);
        if !found.is_empty() {
            marked.insert(spec.name.clone(), found.into_iter().map(|(p, _)| p).collect::<Vec<_>>());
        }
    }
    // The commands COL-FE-012 to COL-FE-016 cover.
    let mut expected = [
        "array.along_path", "array.set", "bath.make_product", "bath.report", "bcf.export", "bcf.export_viewpoint", "bcf.import", "bcf.import_viewpoint", "bimserver.download_revision", "clash.export_report", "classification.load", "component.create", "component.place", "component.place_fragment", "docs.report", "embodied_carbon.export", "embodied_carbon.import_factors", "export.cobie", "export.pdf", "gbxml.export", "gbxml.import_loads", "hbjson.export", "issues.export_bcf", "keynotes.load", "library.create", "library.delete_items", "library.get_meta", "library.import_files", "library.list", "library.open", "library.reload_item", "library.restore_items", "library.set_meta", "library.updates", "lighting_fixture_type.set_photometry", "merge.three_way", "mesh_object.import", "mesh_object.inspect", "mesh_object.reload", "paramlib.create", "paramlib.export_ifc_templates", "paramlib.import_legacy", "paramlib.open", "precast.cam_export", "precast_config.export", "precast_config.import", "pressure_loss_report.export", "profiler.export_report", "profiler.export_trace", "recovery.restore", "results.export_package", "schedule.export", "script.import", "script.run", "solar.export", "steel_nc.export", "structural_analysis.export_model", "structural_analysis.import_results", "structural_connection.check_report", "subscription.add", "subscription.set", "weather.import",
    ];
    expected.sort_unstable();
    let names: Vec<&str> = marked.keys().map(String::as_str).collect();
    assert_eq!(names, expected, "{marked:#?}");
}

/// Publish sets (COL-FE-015): kept with the model, published in one step, again as new versions;
/// a failing output reported while the others publish.
#[test]
fn publish_sets_publish_together() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = connected(&mut app, &fake);
    let eng = fake.uid_of("Engineering").unwrap();
    let issued = ok(&mut app, "fileengine.make_folder", json!({ "connection_id": conn, "parent_uid": eng, "name": "Issued" })).as_str().unwrap().to_owned();
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } }));
    ok(&mut app, "bcf.create_topic", json!({ "title": "Check the sill" }));
    let schedule = ok(&mut app, "schedule.create", json!({ "category": "cat.walls", "name": "Walls" }));

    let set = ok(&mut app, "publish.save_set", json!({
        "name": "Weekly issue", "connection_id": conn, "folder_uid": issued, "folder_name": "Issued",
        "outputs": [
            { "command": "bcf.export", "name": "issues.bcf" },
            { "command": "export.cobie", "args": { "overwrite": true }, "name": "COBie.xlsx" },
            { "command": "schedule.export", "args": { "schedule_id": schedule }, "name": "Walls.csv" }
        ]
    }));
    let id = set["id"].as_str().unwrap().to_owned();
    // Kept with the model through saving and reopening.
    let path = dir.path().join("model.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let doc = app.active().unwrap();
    ok(&mut app, "file.close", json!({ "document_id": doc, "save": "no" }));
    ok(&mut app, "file.open", json!({ "path": path }));
    assert_eq!(ok(&mut app, "publish.list_sets", json!({}))[0]["name"], "Weekly issue");

    let first = ok(&mut app, "publish.run", json!({ "set_id": id }));
    let names: Vec<(&str, bool)> = first.as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap(), p["error"].is_null() && p["uploads"].as_array().unwrap().len() == 1)).collect();
    assert_eq!(names, [("issues.bcf", true), ("COBie.xlsx", true), ("Walls.csv", true)], "{first:#}");
    for n in ["issues.bcf", "COBie.xlsx", "Walls.csv"] {
        assert!(fake.uid_of(&format!("Engineering/Issued/{n}")).is_some(), "{n}");
    }
    // Again: a new version of each.
    let again = ok(&mut app, "publish.run", json!({ "set_id": id }));
    assert!(again.as_array().unwrap().iter().all(|p| p["uploads"][0]["replaced"] == true), "{again:#}");

    // An output that fails is reported; the others still publish.
    let mixed = ok(&mut app, "publish.run", json!({ "connection_id": conn, "folder_uid": issued, "outputs": [
        { "command": "schedule.export", "args": { "schedule_id": 999_999 }, "name": "Missing.csv" },
        { "command": "bcf.export", "name": "issues2.bcf" }
    ] }));
    assert!(mixed[0]["error"].is_string() && mixed[1]["error"].is_null(), "{mixed:#}");
    assert!(fake.uid_of("Engineering/Issued/issues2.bcf").is_some());
    assert_eq!(ok(&mut app, "publish.remove_set", json!({ "set_id": id })), 1);
}
