// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Files and scripts from before the project's name was spelt correctly ("Reviera", before
//! 2026-11-04) still work: a model's `Reviera_*` property sets are read as `Riviera_*` and saved
//! under the new names, and scripts may still call `reviera.<domain>.<verb>`.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> riviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

#[test]
fn a_model_saved_under_the_old_name_keeps_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    let old = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/legacy/reviera_named_stairlifts.ifc");
    let text = std::fs::read_to_string(&old).unwrap();
    assert!(text.contains("'Reviera_Element'") && !text.contains("'Riviera_Element'"), "the fixture keeps the old names");
    // Copied, so that saving does not change the fixture.
    let path = dir.path().join("old.ifc");
    std::fs::copy(&old, &path).unwrap();
    ok(&mut app, "file.open", json!({ "path": path }));

    // Its Riviera data: the three lifts on their stairs, with their published values.
    let lifts = ok(&mut app, "stairlift.list", json!({}));
    assert_eq!(lifts.as_array().map(Vec::len), Some(3), "{lifts:#}");
    assert_eq!(ok(&mut app, "stairlift.check", json!({})), json!([]));
    {
        let d = app.document(app.active().unwrap()).unwrap();
        let store = d.doc.store();
        let kinds: Vec<&Value> = store.iter().filter_map(|e| e.param("Riviera_Stairlift.Kind")).collect();
        assert_eq!(kinds.len(), 3);
        assert!(store.iter().all(|e| e.params.keys().all(|k| !k.starts_with("Reviera_"))));
    }

    // Saved, it carries only the current names.
    ok(&mut app, "file.save", json!({}));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'Riviera_Element'") && text.contains("'Riviera_Stairlift'"));
    assert!(!text.contains("'Reviera_"), "an old-named set was kept");
    let mut again = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled2") });
    ok(&mut again, "file.open", json!({ "path": path }));
    assert_eq!(ok(&mut again, "stairlift.list", json!({})).as_array().map(Vec::len), Some(3));
}

#[test]
fn scripts_may_use_the_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let out = ok(&mut app, "script.run", json!({ "code": r#"
        local l = reviera.level.list()[1].id
        reviera.level.create{ elevation = 3.0 }
        return #riviera.level.list()
    "# }));
    assert_eq!(out["result"], 2);
}
