// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Temporary hide, isolate and translucency per view, the model tree and Selection Box
//! (DOC-GFX-008, UI-TREE-001, DOC-VIEW-008).

use std::path::Path;

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

#[test]
fn temporary_visibility_is_per_view_and_reaches_subtrees() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc");
    let doc = run(
        &mut app,
        "file.open",
        json!({ "path": file, "read_only": true }),
    )
    .unwrap()
    .as_u64()
    .unwrap();
    let tree = run(&mut app, "model.tree", json!({})).unwrap();
    assert!(tree["children"].is_array());
    let store = app.document(doc).unwrap().doc.store().clone();
    let storey = store
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = store
        .iter()
        .find(|e| e.class.starts_with("IfcWall"))
        .unwrap()
        .id();
    let doors = store.iter().filter(|e| e.class == "IfcDoor").count();
    assert!(doors > 0);

    // Hiding a storey with its children hides its walls and doors, in the active view only.
    let n = run(
        &mut app,
        "view.temp_hide",
        json!({ "element_ids": [storey.0], "by": "subtree" }),
    )
    .unwrap()["count"]
        .as_u64()
        .unwrap();
    assert!(n > 10, "{n}");
    let state = app.temp_state(doc, None);
    let v = state.visibility();
    assert_eq!(v.opacity(Some(storey)), None);
    let views = run(&mut app, "view.create_3d", json!({ "kind": "named" })).unwrap();
    let other: ElementId = serde_json::from_value(views).unwrap();
    assert!(
        app.temp_state(doc, Some(other)).is_empty(),
        "other views are untouched"
    );

    // Translucent un-hides; show clears; reset clears everything.
    run(
        &mut app,
        "view.temp_translucent",
        json!({ "element_ids": [wall.0] }),
    )
    .unwrap();
    let state = app.temp_state(doc, None);
    assert!(!state.hidden.contains(&wall) && state.translucent[&wall] == 0.25);
    run(
        &mut app,
        "view.temp_show",
        json!({ "element_ids": [wall.0] }),
    )
    .unwrap();
    assert!(!app.temp_state(doc, None).translucent.contains_key(&wall));
    let bad = json!({ "element_ids": [wall.0], "opacity": 1.5 });
    assert!(run(&mut app, "view.temp_translucent", bad).is_err());

    // Isolating by category ghosts the rest when asked (hidden elements stay hidden).
    run(&mut app, "view.temp_reset", json!({})).unwrap();
    run(
        &mut app,
        "view.temp_isolate",
        json!({ "element_ids": [wall.0], "by": "category", "ghost_others": true }),
    )
    .unwrap();
    let got = run(&mut app, "view.get_temp_state", json!({})).unwrap();
    assert!(got["isolated"]["elements"].as_array().unwrap().len() > 1);
    let v = app.temp_state(doc, None).visibility();
    let slab = store.iter().find(|e| e.class == "IfcSlab").unwrap().id();
    assert_eq!(v.opacity(Some(slab)), Some(0.25));
    run(&mut app, "view.temp_reset", json!({})).unwrap();
    assert!(app.temp_state(doc, None).is_empty());
}

#[test]
fn selection_box_fits_around_elements() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    let dir = tempfile::tempdir().unwrap();
    let _ = &dir;
    run(&mut app, "file.new", json!({})).unwrap();
    let doc = app.active().unwrap();
    let level = app
        .document(doc)
        .unwrap()
        .doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 0], "end": [6, 0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap();
    let view: ElementId = serde_json::from_value(
        run(
            &mut app,
            "view.fit_section_box",
            json!({ "element_ids": [wall] }),
        )
        .unwrap(),
    )
    .unwrap();
    let data =
        reviera_core::views::view_data(app.document(doc).unwrap().doc.store().get(view).unwrap())
            .unwrap();
    let b = data.section_box.unwrap();
    assert!(b.enabled);
    assert!(
        (b.min[0] + 0.3).abs() < 1e-6 && (b.max[0] - 6.3).abs() < 1e-6,
        "{b:?}"
    );
    assert!((b.max[2] - 3.3).abs() < 1e-6, "{b:?}");
}
