// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter libraries (DM-SPAR-001): a 500-definition library opens quickly and passes the
//! schema; a legacy file of 50 parameters imports with identical GUIDs; a changed data type is
//! refused on reopening; a project using a shared parameter schedules it with the library file
//! gone; conflicts keep or update; IFC templates carry the GUIDs.

use std::time::Instant;

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn guid(k: usize) -> String {
    format!("3b0d2a4e-5f61-4c7a-9e2b-{k:012x}")
}

#[test]
fn parameter_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });

    // 500 definitions: open fast, schema-valid.
    let defs: Vec<Value> = (0..500).map(|k| json!({ "guid": guid(1000 + k), "name": format!("Param {k}"), "data_type": if k % 2 == 0 { "text" } else { "length" }, "group": "g-a", "labels": ["bulk"] })).collect();
    let big = json!({ "schema": "reviera.paramlib", "schema_version": "1.0.0", "library": { "id": "6f1c9d2e-8a4b-4c1e-9f0a-2b7d3e5c1a90", "name": "Big", "version": "1.0.0" }, "groups": [{ "id": "g-a", "name": "A" }], "definitions": defs });
    let big_path = dir.path().join("big.rplib.json");
    std::fs::write(&big_path, serde_json::to_string(&big).unwrap()).unwrap();
    let t = Instant::now();
    let s = ok(&mut app, "paramlib.open", json!({ "path_or_url": big_path }));
    let took = t.elapsed().as_millis();
    let limit = if cfg!(debug_assertions) { 2000 } else { 200 };
    assert!(took < limit, "{took} ms");
    assert_eq!(s["definitions"], 500);
    assert_eq!(ok(&mut app, "paramlib.search", json!({ "text": "param 49", "data_type": "length" })).as_array().unwrap().len(), 6, "49, 491, 493, …");

    // A changed data type is refused when the library is opened again.
    ok(&mut app, "paramlib.close", json!({ "library_id": "6f1c9d2e-8a4b-4c1e-9f0a-2b7d3e5c1a90" }));
    let changed = serde_json::to_string(&big).unwrap().replacen("\"data_type\":\"text\"", "\"data_type\":\"number\"", 1);
    std::fs::write(&big_path, changed).unwrap();
    let e = run(&mut app, "paramlib.open", json!({ "path_or_url": big_path })).unwrap_err();
    assert_eq!(&*e.code, "paramlib.data_type_changed", "{}", e.message);

    // A legacy file of 50 parameters: identical GUIDs.
    let mut legacy = String::from("# This is a shared parameter file.\n*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tIdentity\n*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\n");
    for k in 0..50 {
        legacy.push_str(&format!("PARAM\t{}\tShared {k}\t{}\t\t1\t1\tdescription {k}\t1\t0\n", guid(k).to_uppercase(), if k == 0 { "TEXT" } else { "LENGTH" }));
    }
    let legacy_path = dir.path().join("shared.txt");
    std::fs::write(&legacy_path, legacy).unwrap();
    let lib_path = dir.path().join("practice.rplib.json");
    let lib = ok(&mut app, "paramlib.create", json!({ "path": lib_path, "name": "Practice" })).as_str().unwrap().to_owned();
    let rep = ok(&mut app, "paramlib.import_legacy", json!({ "path": legacy_path, "target_library_id": lib }));
    assert_eq!(rep["added"], 50);
    let all = ok(&mut app, "paramlib.search", json!({ "text": "shared" }));
    let guids: Vec<String> = all.as_array().unwrap().iter().map(|f| f["definition"]["guid"].as_str().unwrap().to_owned()).collect();
    assert!((0..50).all(|k| guids.contains(&guid(k))));
    // Written with sorted keys and 2-space indentation.
    let text = std::fs::read_to_string(&lib_path).unwrap();
    assert!(text.starts_with("{\n  \"definitions\": ["), "{}", &text[..40]);
    // Its data type cannot be edited; a definition can be deprecated in favour of another.
    assert!(run(&mut app, "paramlib.definition.edit", json!({ "library_id": lib, "guid": guid(1), "changes": { "data_type": "text" } })).is_err());
    ok(&mut app, "paramlib.definition.deprecate", json!({ "library_id": lib, "guid": guid(2), "superseded_by": guid(3) }));
    let added = ok(&mut app, "paramlib.definition.add", json!({ "library_id": lib, "def": { "name": "Acoustic Class", "data_type": "text", "enum": ["A", "B"], "ifc": { "pset": "ACME_Common", "property": "AcousticClass" } } }));
    assert!(run(&mut app, "paramlib.definition.add", json!({ "library_id": lib, "def": { "name": "Bad", "data_type": "text", "ifc": { "pset": "Reviera_X", "property": "Y" } } })).is_err());

    // In a project: embedded, scheduled, and still there with the library gone.
    ok(&mut app, "file.new", json!({}));
    let doc = app.active().unwrap();
    let level = app.document(doc).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall: ElementId = serde_json::from_value(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [5, 0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3 } }))).unwrap();
    let g0 = ok(&mut app, "paramlib.add_to_project", json!({ "library_id": lib, "guid": guid(0), "categories": ["cat.walls"], "is_instance": true }));
    assert_eq!(g0, json!(guid(0)));
    ok(&mut app, "param.set", json!({ "element_ids": [wall.0], "param_ref": guid(0), "value": "F60" }));
    let project = reviera_core::parameters::project_parameters(app.document(doc).unwrap().doc.store());
    assert_eq!(project[0].library.as_ref().unwrap().name, "Practice", "the source is embedded");
    // Conflicts: the project's name differs from the library's.
    ok(&mut app, "param.project.modify", json!({ "param_id": guid(0), "changes": { "name": "Fire Class" } }));
    assert!(run(&mut app, "paramlib.add_to_project", json!({ "library_id": lib, "guid": guid(0), "categories": ["cat.walls"], "is_instance": true })).is_err());
    ok(&mut app, "paramlib.add_to_project", json!({ "library_id": lib, "guid": guid(0), "categories": ["cat.walls"], "is_instance": true, "on_conflict": "keep" }));
    assert_eq!(reviera_core::parameters::project_parameters(app.document(doc).unwrap().doc.store())[0].name, "Fire Class");
    ok(&mut app, "paramlib.add_to_project", json!({ "library_id": lib, "guid": guid(0), "categories": ["cat.walls"], "is_instance": true, "on_conflict": "update" }));
    assert_eq!(reviera_core::parameters::project_parameters(app.document(doc).unwrap().doc.store())[0].name, "Shared 0");

    // IFC templates: one per current definition, GlobalIds from the GUIDs.
    let ifc_path = dir.path().join("templates.ifc");
    let n = ok(&mut app, "paramlib.export_ifc_templates", json!({ "library_id": lib, "path": ifc_path }));
    assert_eq!(n, json!(50), "49 imported still current plus Acoustic Class");
    let rows = reviera_ifc::IfcModel::open(&ifc_path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCPROJECTLIBRARY"));
    let g = reviera_core::GlobalId::from_uuid(uuid::Uuid::parse_str(added.as_str().unwrap()).unwrap()).to_string();
    assert!(rows.iter().any(|r| r.class == "IFCSIMPLEPROPERTYTEMPLATE" && r.global_id.as_deref() == Some(g.as_str())));

    // Saved, the library and its cache deleted, reopened: the column still shows the value.
    let model = dir.path().join("project.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    ok(&mut app, "file.close", json!({ "document_id": doc }));
    std::fs::remove_file(&lib_path).unwrap();
    let _ = std::fs::remove_dir_all(dir.path().join("paramlib-cache"));
    ok(&mut app, "file.open", json!({ "path": model }));
    let s = ok(&mut app, "schedule.create", json!({ "category": "cat.walls", "fields": [format!("param:{}", guid(0))] }));
    let t = ok(&mut app, "schedule.get_table", json!({ "schedule_id": s }));
    assert_eq!(t["rows"][0]["cells"][0]["text"], "F60", "{t}");
}
