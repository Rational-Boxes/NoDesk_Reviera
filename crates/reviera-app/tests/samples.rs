// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The sample corpus (`test-data/samples`): each sample is rebuilt from its manifest steps (Lua
//! scripts, plugin runs, settings) and must have no overlaps (beyond those it leaves on
//! purpose, `allow_overlaps`) and no intrusions into its clearances, then save (importing its
//! `weather` file into `<name>.resources`) and reopen. With `REVIERA_REGEN_SAMPLES=1` the corpus's `.ifc` files
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
    // `REVIERA_SAMPLES=a,b` builds only those.
    let only = std::env::var("REVIERA_SAMPLES").ok();
    let out = tempfile::tempdir().unwrap();
    for sample in &manifest {
        let name = sample["name"].as_str().unwrap();
        if only
            .as_deref()
            .is_some_and(|l| !l.split(',').any(|n| n.trim() == name))
        {
            continue;
        }
        // Each sample has its walkthrough (`<name>.md`): the manual steps to build it in the
        // window, shown in Help.
        let walkthrough = std::fs::read_to_string(dir.join(format!("{name}.md")))
            .unwrap_or_else(|e| panic!("{name}: its walkthrough {name}.md: {e}"));
        assert!(walkthrough.starts_with("# ") && walkthrough.contains(".lua"), "{name}.md: a titled walkthrough naming its script");
        let mut app = App::new(AppConfig {
            live: None,
            untitled_dir: out.path().to_owned(),
        });
        run(&mut app, "file.new", json!({})).unwrap();
        // What a script writes beside the sample (a component library, say) is removed after a
        // check run; a regeneration replaces the resources folder with only what is kept.
        let resources = dir.join(format!("{name}.resources"));
        let kept = files_under(&resources);
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
        let _ = std::fs::remove_dir_all(path.with_extension("resources"));
        run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
        // A sample's weather file (`weather`) is imported once it is saved, so it is kept beside
        // it in `<name>.resources/weather` with a relative path.
        if let Some(w) = sample["weather"].as_str() {
            run(&mut app, "weather.import", json!({ "path": dir.join(w) })).unwrap_or_else(|e| panic!("{name}: weather.import: {e:?}"));
            run(&mut app, "file.save", json!({})).unwrap();
            let kept = reviera_core::location::get(app.document(app.active().unwrap()).unwrap().doc.store()).weather.file.unwrap();
            assert_eq!(kept, format!("{name}.resources/weather/{w}"), "{name}: the weather file is kept beside the sample");
        }
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
        if !regen_this(name) {
            for f in files_under(&resources).into_iter().filter(|f| !kept.contains(f)) {
                std::fs::remove_file(f).unwrap();
            }
            remove_empty_dirs(&resources);
        }
    }
}

/// Every file under `dir`, at any depth.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Removes `dir`'s empty folders, and `dir` itself when it is left empty.
fn remove_empty_dirs(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            remove_empty_dirs(&e.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// The structural problems sample (STR-AM-007) reports exactly the faults it was built with, from
/// its saved file.
#[test]
fn the_structural_checks_sample_reports_its_faults() {
    let file = samples().join("structural_checks.ifc");
    let work = tempfile::tempdir().unwrap();
    let copy = work.path().join("structural_checks.ifc");
    std::fs::copy(&file, &copy).unwrap();
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: work.path().to_owned(),
    });
    run(&mut app, "file.open", json!({ "path": copy })).unwrap();
    let report = run(&mut app, "analytical_model.check", json!({})).unwrap();
    let mut found: Vec<(String, String)> = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["code"].as_str().unwrap().to_owned(),
                i["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    found.sort();
    let mut want: Vec<(String, String)> = [
        ("C1", "disconnected_nodes"),
        ("C1", "end_on_span"),
        ("C1", "end_on_span"),
        ("C1", "free_end"),
        ("C2", "isolated_part"),
        ("C2", "isolated_part"),
        ("C2", "isolated_part"),
        ("C3", "position_mismatch"),
        ("C4", "outside_physical"),
        ("C5", "no_analytical"),
        ("C5", "no_physical"),
        ("C5", "no_physical"),
        ("C6", "beam_slab"),
        ("C6", "beam_slab"),
        ("C6", "beam_slab"),
        ("C6", "beam_slab"),
        ("C7", "pinned_both_ends"),
        ("S1", "unsupported_member"),
        ("S1", "unsupported_panel"),
        ("S2", "circular_support"),
    ]
    .iter()
    .map(|(c, k)| (c.to_string(), k.to_string()))
    .collect();
    want.sort();
    assert_eq!(found, want, "{report:#}");
    // The 40 mm gap is measured.
    let gap = report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "disconnected_nodes")
        .unwrap();
    assert!((gap["measured"].as_f64().unwrap() - 0.04).abs() < 1e-6);
}
