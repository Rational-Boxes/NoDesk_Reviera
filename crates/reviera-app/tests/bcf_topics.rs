// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BCF issues (COL-BCF-001): a topic made from a view of the views sample, commented on and given
//! a second viewpoint, kept with the model through a save and reopen, exported as BCF 3.0 and 2.1
//! and imported into another copy, where its viewpoint shows the same section box; imported again
//! it changes nothing, and edits made in the other copy come back merged by GUID.

use std::io::Read;
use std::path::Path;

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn open(work: &Path, tag: &str) -> (App, std::path::PathBuf) {
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.join(tag) });
    let copy = work.join(format!("{tag}.ifc"));
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples/views_inspection.ifc"), &copy).unwrap();
    ok(&mut app, "file.open", json!({ "path": copy }));
    (app, copy)
}

fn view(app: &mut App, name: &str) -> u64 {
    let all = ok(app, "view.list", json!({ "kind": "view.3d" }));
    all.as_array().unwrap().iter().find(|v| v["name"] == name).unwrap_or_else(|| panic!("no view {name}"))["id"].as_u64().unwrap()
}

fn names(zip_path: &Path) -> Vec<String> {
    let z = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
    let mut n: Vec<String> = z.file_names().map(str::to_owned).collect();
    n.sort();
    n
}

fn entry(zip_path: &Path, name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn issues_are_kept_exported_imported_and_merged() {
    let work = tempfile::tempdir().unwrap();
    let (mut a, path_a) = open(work.path(), "a");
    let kitchen = view(&mut a, "Section Box - Kitchen");
    let wall = a.document(a.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcWall").unwrap().id().0;
    let t = ok(&mut a, "bcf.create_topic", json!({
        "title": "Kitchen wall & window clash",
        "description": "Check the sill",
        "fields": { "priority": "High", "labels": ["Architecture"], "assigned_to": "arch@example.com" },
        "view_id": kitchen,
        "element_ids": [wall],
        "author": "coordinator@example.com",
    }));
    let topic = ok(&mut a, "bcf.get", json!({ "topic_id": t }));
    let guid = topic["guid"].as_str().unwrap().to_owned();
    assert_eq!((topic["status"].as_str(), topic["topic_type"].as_str()), (Some("Open"), Some("Issue")));
    assert_eq!(topic["viewpoints"].as_array().unwrap().len(), 1);
    assert_eq!(topic["viewpoints"][0]["viewpoint"]["clipping_planes"].as_array().unwrap().len(), 6, "the kitchen's box");
    let vp1 = topic["viewpoints"][0]["guid"].as_str().unwrap().to_owned();
    ok(&mut a, "bcf.add_comment", json!({ "topic_id": guid, "text": "Raise the sill to 1100", "viewpoint": vp1, "author": "arch@example.com" }));
    let slice = view(&mut a, "Vertical Slice");
    ok(&mut a, "bcf.add_viewpoint", json!({ "topic_id": guid, "view_id": slice }));
    // Listed and filtered.
    assert_eq!(ok(&mut a, "bcf.list", json!({ "priority": "high" })).as_array().unwrap().len(), 1);
    assert_eq!(ok(&mut a, "bcf.list", json!({ "status": "Closed" })).as_array().unwrap().len(), 0);
    assert_eq!(ok(&mut a, "bcf.list", json!({ "text": "sill" }))[0]["comments"], 1);

    // Kept with the model.
    ok(&mut a, "file.save", json!({}));
    let mut a2 = App::new(AppConfig { live: None, untitled_dir: work.path().join("a2") });
    ok(&mut a2, "file.open", json!({ "path": path_a }));
    let back = ok(&mut a2, "bcf.get", json!({ "topic_id": guid }));
    assert_eq!(back["comments"][0]["text"], "Raise the sill to 1100");
    assert_eq!(back["viewpoints"].as_array().unwrap().len(), 2);

    // Exported as 3.0 and 2.1.
    let bcf3 = work.path().join("issues.bcf");
    let r = ok(&mut a, "bcf.export", json!({ "path": bcf3 }));
    assert_eq!((r["topics"].as_u64(), r["comments"].as_u64(), r["viewpoints"].as_u64()), (Some(1), Some(1), Some(2)), "{r}");
    let n = names(&bcf3);
    assert!(n.contains(&"bcf.version".to_owned()) && n.contains(&"extensions.xml".to_owned()) && n.contains(&format!("{guid}/markup.bcf")), "{n:?}");
    assert!(n.contains(&format!("{guid}/{vp1}.bcfv")), "{n:?}");
    let markup = entry(&bcf3, &format!("{guid}/markup.bcf"));
    assert!(markup.contains("<Title>Kitchen wall &amp; window clash</Title>") && markup.contains("<Comments>"), "{markup}");
    let bcf2 = work.path().join("issues21.bcfzip");
    ok(&mut a, "bcf.export", json!({ "path": bcf2, "version": "2.1" }));
    assert!(entry(&bcf2, "bcf.version").contains("VersionId=\"2.1\""));
    assert!(!names(&bcf2).contains(&"extensions.xml".to_owned()));

    // Imported into another copy (from 2.1), its first viewpoint shown in the orthographic view.
    let (mut b, _) = open(work.path(), "b");
    let r = ok(&mut b, "bcf.import", json!({ "path": bcf2 }));
    assert_eq!((r["version"].as_str(), r["added"].as_u64(), r["viewpoints"].as_u64()), (Some("2.1"), Some(1), Some(2)), "{r}");
    assert_eq!(r["problems"], json!([]));
    let target = view(&mut b, "Orthographic");
    let shown = ok(&mut b, "bcf.apply_viewpoint", json!({ "topic_id": guid, "target_view_id": target }));
    assert_eq!((shown["section_box"].as_bool(), shown["selected"].as_u64(), shown["not_found"].clone()), (Some(true), Some(1), json!([])), "{shown}");
    let (was, now) = (ok(&mut a, "view.get", json!({ "view_id": kitchen })), ok(&mut b, "view.get", json!({ "view_id": target })));
    for k in 0..3 {
        assert!((was["sectionBox"]["min"][k].as_f64().unwrap() - now["sectionBox"]["min"][k].as_f64().unwrap()).abs() < 1e-3);
    }
    let in_b = ok(&mut b, "bcf.get", json!({ "topic_id": guid }));
    assert_eq!((in_b["title"].clone(), in_b["labels"].clone(), in_b["assigned_to"].clone()), (json!("Kitchen wall & window clash"), json!(["Architecture"]), json!("arch@example.com")));
    // The same file again changes nothing.
    let r = ok(&mut b, "bcf.import", json!({ "path": bcf3 }));
    assert_eq!((r["added"].as_u64(), r["unchanged"].as_u64()), (Some(0), Some(1)), "{r}");

    // Resolved and answered in the other copy, then merged back.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    ok(&mut b, "bcf.update_topic", json!({ "topic_id": guid, "fields": { "status": "Resolved" }, "author": "arch@example.com" }));
    ok(&mut b, "bcf.add_comment", json!({ "topic_id": guid, "text": "Sill raised", "author": "arch@example.com" }));
    let back_file = work.path().join("back.bcf");
    ok(&mut b, "bcf.export", json!({ "path": back_file }));
    let r = ok(&mut a, "bcf.import", json!({ "path": back_file }));
    assert_eq!(r["updated"], 1, "{r}");
    let merged = ok(&mut a, "bcf.get", json!({ "topic_id": guid }));
    assert_eq!(merged["status"], "Resolved");
    let texts: Vec<&str> = merged["comments"].as_array().unwrap().iter().map(|c| c["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["Raise the sill to 1100", "Sill raised"]);

    // Deleting is one step to undo.
    ok(&mut a, "bcf.delete_topic", json!({ "topic_ids": [guid] }));
    assert_eq!(ok(&mut a, "bcf.list", json!({})).as_array().unwrap().len(), 0);
    ok(&mut a, "edit.undo", json!({}));
    assert_eq!(ok(&mut a, "bcf.list", json!({})).as_array().unwrap().len(), 1);
    let lists = ok(&mut a, "bcf.lists", json!({}));
    assert!(lists["statuses"].as_array().unwrap().contains(&json!("Resolved")) && lists["users"].as_array().unwrap().contains(&json!("arch@example.com")), "{lists}");
}
