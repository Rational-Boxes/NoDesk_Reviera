// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Classification systems and keynotes (DM-CLS-001): Ss_25_10_30 on a wall type exports as an
//! IfcClassificationReference linked to the IfcClassification "Uniclass 2015"; the IFC tree reads
//! back as a system; keynotes load relative to the project and reload. (Titles here are made up:
//! Reviera ships no publisher's table.)

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

#[test]
fn classification_and_keynotes() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let doc = app.active().unwrap();
    let level = app.document(doc).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [5, 0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3 } }));
    let wall_type = app.document(doc).unwrap().doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();

    let csv = dir.path().join("uniclass.csv");
    std::fs::write(&csv, "code,parent_code,title\nSs,,Test systems\nSs_25,Ss,Test group\nSs_25_10,Ss_25,Test subgroup\nSs_25_10_30,Ss_25_10,Test object\n").unwrap();
    let sys = ok(&mut app, "classification.load", json!({ "path_or_url": csv, "name": "Uniclass 2015", "source": "Test", "edition": "v1" }));
    assert!(run(&mut app, "classification.assign", json!({ "element_ids": [wall_type.0], "system_id": "Uniclass 2015", "code": "Zz_99" })).is_err(), "not in the table");
    ok(&mut app, "classification.assign", json!({ "element_ids": [wall_type.0], "system_id": sys, "code": "Ss_25_10_30" }));
    let of = ok(&mut app, "classification.of", json!({ "element_id": wall_type.0 }));
    assert_eq!(of[0]["code"], "Ss_25_10_30");
    assert_eq!(of[0]["title"], "Test object");
    assert_eq!(ok(&mut app, "classification.search", json!({ "system_id": sys, "text": "subgroup" })).as_array().unwrap().len(), 1);

    // Saved: the reference, its chain of parents and the classification.
    let model = dir.path().join("project.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    ok(&mut app, "file.save", json!({}));
    let ifc = reviera_ifc::IfcModel::open(&model).unwrap();
    let rows = ifc.step_rows().unwrap();
    let refs: Vec<_> = rows.iter().filter(|r| r.class == "IFCCLASSIFICATIONREFERENCE").collect();
    assert_eq!(refs.len(), 4, "the code and its three ancestors, once after two saves");
    let leaf = refs.iter().find(|r| r.attributes.contains("'Ss_25_10_30'")).expect("the code");
    // Up the ReferencedSource chain to the classification.
    let mut at = leaf.id;
    let mut root = None;
    for _ in 0..5 {
        match ifc.attribute(at, "ReferencedSource").unwrap() {
            reviera_ifc::AttrValue::Ref(r) => {
                let row = rows.iter().find(|x| x.id.0 == r).unwrap();
                if row.class == "IFCCLASSIFICATION" {
                    root = Some(row);
                    break;
                }
                at = row.id;
            }
            _ => break,
        }
    }
    assert!(root.expect("reaches the classification").attributes.contains("'Uniclass 2015'"));
    let type_guid = app.document(doc).unwrap().doc.store().get(wall_type).unwrap().guid().to_string();
    let type_step = rows.iter().find(|r| r.global_id.as_deref() == Some(type_guid.as_str())).unwrap().id.0;
    let rel = rows.iter().find(|r| r.class == "IFCRELASSOCIATESCLASSIFICATION" && r.attributes.contains(&format!("#{})", leaf.id.0))).expect("associated");
    assert!(rel.attributes.contains(&format!("#{type_step}")), "{}", rel.attributes);

    // Reopened: the same elements, the assignment kept (from RXD), nothing read back as elements.
    let count = app.document(doc).unwrap().doc.store().iter().count();
    ok(&mut app, "file.close", json!({ "document_id": doc }));
    ok(&mut app, "file.open", json!({ "path": model }));
    let doc = app.active().unwrap();
    assert_eq!(app.document(doc).unwrap().doc.store().iter().count(), count);
    let wall_type = app.document(doc).unwrap().doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    assert_eq!(ok(&mut app, "classification.of", json!({ "element_id": wall_type.0 }))[0]["code"], "Ss_25_10_30");

    // The IFC tree reads back as a system.
    let again = ok(&mut app, "classification.load", json!({ "path_or_url": model, "name": "Uniclass 2015" }));
    assert_eq!(again, sys, "replaced by name, the same id");
    let items = ok(&mut app, "classification.search", json!({ "system_id": sys }));
    assert_eq!(items.as_array().unwrap().len(), 4, "the codes used and their ancestors");
    assert_eq!(items[0]["code"], "Ss");
    assert_eq!(ok(&mut app, "classification.of", json!({ "element_id": wall_type.0 }))[0]["code"], "Ss_25_10_30", "still assigned");

    // Keynotes beside the project: kept relative, reloaded after a change.
    let keys = dir.path().join("keynotes.txt");
    std::fs::write(&keys, "A\tGeneral\t\nA10\tConcrete wall\tA\n").unwrap();
    let rep = ok(&mut app, "keynotes.load", json!({ "path": keys }));
    assert_eq!(rep["path"], "keynotes.txt");
    std::fs::write(&keys, "A\tGeneral\t\nA10\tConcrete wall, fair-faced\tA\nA20\tBlock wall\tA\n").unwrap();
    assert_eq!(ok(&mut app, "keynotes.reload", json!({}))["entries"], 3);
    let t = ok(&mut app, "keynotes.list", json!({ "text": "fair" }));
    assert_eq!(t["entries"][0]["key"], "A10");

    // Deleting the system unclassifies.
    assert_eq!(ok(&mut app, "classification.delete", json!({ "system_id": sys })), 1);
    assert!(ok(&mut app, "classification.of", json!({ "element_id": wall_type.0 })).as_array().unwrap().is_empty());
}
