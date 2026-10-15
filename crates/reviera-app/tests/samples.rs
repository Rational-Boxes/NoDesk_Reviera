// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The sample corpus (`test-data/samples`): each sample is rebuilt from its manifest steps (Lua
//! scripts, plugin runs, settings) and must have no overlaps (beyond those it leaves on
//! purpose, `allow_overlaps`) and no intrusions into its clearances, then save and reopen. With `REVIERA_REGEN_SAMPLES=1` the corpus's `.ifc` files
//! are rewritten from the rebuild.

use std::path::{Path, PathBuf};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples")
}

#[test]
fn the_sample_corpus_builds_cleanly() {
    let dir = samples();
    let manifest: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("samples.json")).unwrap()).unwrap();
    assert!(!manifest.is_empty());
    // `1` rewrites every sample; a comma-separated list of names, only those.
    let regen_list = std::env::var("REVIERA_REGEN_SAMPLES").ok();
    let regen_this = |name: &str| {
        regen_list
            .as_deref()
            .is_some_and(|l| l == "1" || l.split(',').any(|n| n.trim() == name))
    };
    let out = tempfile::tempdir().unwrap();
    for sample in &manifest {
        let name = sample["name"].as_str().unwrap();
        let mut app = App::new(AppConfig {
            live: None,
            untitled_dir: out.path().to_owned(),
        });
        run(&mut app, "file.new", json!({})).unwrap();
        for step in sample["steps"].as_array().unwrap() {
            let mut args = step["args"].clone();
            // Scripts are named relative to the corpus.
            if let Some(f) = args["file"].as_str() {
                args["file"] = json!(dir.join(f));
            }
            let command = step["command"].as_str().unwrap();
            run(&mut app, command, args).unwrap_or_else(|e| panic!("{name}: {command}: {e:?}"));
        }
        // Overlaps the sample leaves on purpose (`allow_overlaps`), else none.
        let allowed = sample["allow_overlaps"].as_u64().unwrap_or(0) as usize;
        let overlaps = run(&mut app, "overlap.find", json!({})).unwrap();
        assert_eq!(
            overlaps["overlaps"].as_array().unwrap().len(),
            allowed,
            "{name}: {overlaps:#}"
        );
        let intrusions = run(&mut app, "clearance.check", json!({})).unwrap();
        assert_eq!(intrusions, json!([]), "{name}: {intrusions:#}");
        let path = if regen_this(name) {
            dir.join(format!("{name}.ifc"))
        } else {
            out.path().join(format!("{name}.ifc"))
        };
        let _ = std::fs::remove_file(&path);
        run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
        // Boolean-shaped bodies are written as their result, each relation recorded.
        let relations = {
            let d = app.document(app.active().unwrap()).unwrap();
            reviera_core::csg::resolved(d.doc.store()).len()
        };
        let ifc = reviera_ifc::IfcModel::open(&path).unwrap();
        assert_eq!(
            ifc.entities_of_type("IfcRelInterferesElements")
                .unwrap()
                .len(),
            relations,
            "{name}"
        );
        // It reopens with its Reviera elements intact.
        let mut again = App::new(AppConfig {
            live: None,
            untitled_dir: out.path().to_owned(),
        });
        run(&mut again, "file.open", json!({ "path": path })).unwrap();
        let reopened = run(&mut again, "overlap.find", json!({})).unwrap();
        assert_eq!(
            reopened["overlaps"].as_array().unwrap().len(),
            allowed,
            "{name} reopened: {reopened:#}"
        );
        assert_eq!(
            reopened["bodies"], overlaps["bodies"],
            "{name}: every body comes back"
        );
    }
}
