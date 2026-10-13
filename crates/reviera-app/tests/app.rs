// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The application session: file commands, routing to the active document, live store and
//! recovery commands, permissions.

use std::path::Path;
use std::time::Duration;

use reviera_app::{App, AppConfig};
use reviera_core::{
    Caller, Command, CommandContext, CommandMeta, Effect, ElementId, NewElement, Origin,
    Permission, Result,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
struct ProxyParams {
    name: String,
}

struct AddProxy;

impl Command for AddProxy {
    type Params = ProxyParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "test.add_proxy",
        version: 1,
        summary: "",
        description: "",
        effects: &[Effect::Edit],
        transaction_name: "Add proxy",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ProxyParams) -> Result<ElementId> {
        cx.edit()?
            .create(NewElement::new("IfcBuildingElementProxy").name(p.name))
    }
}

fn app(dir: &Path) -> App {
    let live = reviera_livestore::Options {
        dir: dir.join("live"),
        group_commit: Duration::from_millis(20),
        encryption: reviera_livestore::Encryption::Passphrase("test passphrase".into()),
        ..Default::default()
    };
    let mut app = App::new(AppConfig {
        live: Some(live),
        untitled_dir: dir.join("untitled"),
    });
    app.registry_mut().unwrap().add(AddProxy).unwrap();
    app
}

fn run(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .result
}

fn fails(app: &mut App, name: &str, args: Value) -> String {
    app.execute(name, args, &Caller::user())
        .unwrap_err()
        .code
        .into_owned()
}

#[test]
fn new_save_as_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    assert_eq!(fails(&mut app, "model.version", json!({})), "document.none");

    let id = run(&mut app, "file.new", json!({}));
    assert_eq!(app.documents()[0].title, "Untitled");
    run(&mut app, "test.add_proxy", json!({"name": "P1"}));
    assert_eq!(fails(&mut app, "file.save", json!({})), "file.untitled");
    assert_eq!(
        fails(&mut app, "file.close", json!({"document_id": id})),
        "file.unsaved_changes"
    );

    let path = dir.path().join("house.ifc");
    let saved = run(&mut app, "file.save_as", json!({"path": path}));
    assert_eq!(saved["created"], 1);
    assert!(!app.documents()[0].dirty);
    assert_eq!(app.documents()[0].title, "house");
    run(&mut app, "file.close", json!({"document_id": id}));
    assert!(app.documents().is_empty());

    let id = run(&mut app, "file.open", json!({"path": path}));
    let doc = &app.document(id.as_u64().unwrap()).unwrap().doc;
    assert_eq!(
        doc.store()
            .iter()
            .filter(|e| e.name.as_deref() == Some("P1"))
            .count(),
        1
    );
    assert_eq!(
        fails(&mut app, "file.open", json!({"path": path})),
        "file.already_open"
    );
    run(&mut app, "test.add_proxy", json!({"name": "P2"}));
    run(
        &mut app,
        "file.close",
        json!({"document_id": id, "save": "yes"}),
    );

    let mut other = reviera_app::App::new(AppConfig {
        live: None,
        untitled_dir: dir.path().into(),
    });
    run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(other.document(1).unwrap().doc.store().len(), 6);
}

#[test]
fn a_second_session_gets_read_only_access() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.ifc");
    let mut first = app(dir.path());
    run(&mut first, "file.new", json!({}));
    run(&mut first, "file.save_as", json!({"path": path}));

    let mut second = app(dir.path());
    assert_eq!(
        fails(&mut second, "file.open", json!({"path": path})),
        "file.locked"
    );
    run(
        &mut second,
        "file.open",
        json!({"path": path, "read_only": true}),
    );
    run(&mut second, "test.add_proxy", json!({"name": "draft"}));
    assert_eq!(fails(&mut second, "file.save", json!({})), "file.read_only");
    let copy = dir.path().join("copy.ifc");
    run(&mut second, "file.save_as", json!({"path": copy}));
    assert!(copy.exists());
}

#[test]
fn live_store_commands_follow_edits_and_saves() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    run(&mut app, "file.new", json!({}));
    run(
        &mut app,
        "file.save_as",
        json!({"path": dir.path().join("p.ifc")}),
    );
    run(&mut app, "test.add_proxy", json!({"name": "A"}));
    let agent = Caller {
        origin: Origin::mcp("agent", "s1"),
        permission: Permission::ModelEdit,
    };
    app.execute("test.add_proxy", json!({"name": "B"}), &agent)
        .unwrap();

    let head = run(&mut app, "live.flush", json!({}));
    let status = run(&mut app, "live.status", json!({}));
    assert_eq!(status["head_seq"], head);
    assert_eq!(status["pending"], 0);
    assert!(status["saved_seq"].as_i64().unwrap() < head.as_i64().unwrap());

    let all = run(&mut app, "live.history", json!({}));
    let names: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].clone())
        .collect();
    assert_eq!(names, [json!("Add proxy"), json!("Add proxy")]);
    let agent_only = run(&mut app, "live.history", json!({"origin": "mcp:agent"}));
    assert_eq!(agent_only.as_array().unwrap().len(), 1);
    assert_eq!(agent_only[0]["origin"]["session_id"], "s1");

    let enc = run(&mut app, "live.encryption.status", json!({}));
    assert_eq!(
        (enc["enabled"].clone(), enc["key_source"].clone()),
        (json!(true), json!("passphrase"))
    );

    run(&mut app, "file.save", json!({}));
    let status = run(&mut app, "live.status", json!({}));
    assert_eq!(status["saved_seq"], status["head_seq"]);
    assert_eq!(run(&mut app, "recovery.list", json!({})), json!([]));
    assert_eq!(
        fails(&mut app, "recovery.discard", json!({"store_key": "nope"})),
        "recovery.not_found"
    );
}

#[test]
fn permissions_and_exit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    run(&mut app, "file.new", json!({}));
    let reader = Caller {
        origin: Origin::mcp("viewer", "s2"),
        permission: Permission::ReadOnly,
    };
    let e = app
        .execute(
            "file.save_as",
            json!({"path": dir.path().join("x.ifc")}),
            &reader,
        )
        .unwrap_err();
    assert_eq!(e.code, "permission.denied");
    assert_eq!(
        fails(&mut app, "file.new", json!({"kind": "family"})),
        "file.unsupported_kind"
    );
    assert_eq!(
        fails(
            &mut app,
            "file.save_as",
            json!({"path": "x.ifczip", "format": "ifczip"})
        ),
        "file.unsupported_format"
    );

    run(&mut app, "file.new", json!({"schema": "IFC4X3"}));
    assert_eq!(app.documents().len(), 2);
    assert_eq!(app.documents()[1].schema, "IFC4X3_ADD2");
    assert_eq!(
        fails(&mut app, "app.exit", json!({})),
        "app.unsaved_changes"
    );
    run(&mut app, "app.exit", json!({"force": true}));
    assert!(app.documents().is_empty());

    let names: Vec<_> = app.registry().iter().map(|s| s.name.clone()).collect();
    for n in [
        "file.open",
        "file.save",
        "recovery.restore",
        "live.status",
        "edit.undo",
        "test.add_proxy",
    ] {
        assert!(names.iter().any(|x| x == n), "{n}");
    }
}

#[test]
fn selection_is_session_state_of_the_active_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    run(&mut app, "file.new", json!({}));
    let a = run(&mut app, "test.add_proxy", json!({"name": "A"}));
    let b = run(&mut app, "test.add_proxy", json!({"name": "B"}));
    let version = run(&mut app, "model.version", json!({}));

    let info = run(&mut app, "selection.set", json!({"ids": [a]}));
    assert_eq!(info["ids"], json!([a]));
    assert_eq!(info["by_category"], json!({"cat.generic_models": 1}));
    run(&mut app, "selection.add", json!({"element_ids": [b]}));
    assert_eq!(
        run(&mut app, "selection.get", json!({}))["ids"],
        json!([a, b])
    );
    run(
        &mut app,
        "selection.set",
        json!({"ids": [a], "mode": "remove"}),
    );
    assert_eq!(run(&mut app, "selection.get", json!({}))["ids"], json!([b]));
    run(&mut app, "selection.clear", json!({}));
    assert_eq!(run(&mut app, "selection.get", json!({}))["ids"], json!([]));
    assert_eq!(
        fails(&mut app, "selection.set", json!({"ids": [999999]})),
        "element.not_found"
    );
    // Selecting is not a model change.
    assert_eq!(run(&mut app, "model.version", json!({})), version);
}

#[test]
fn new_walls_appear_in_the_scene_before_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    run(&mut app, "file.new", json!({}));
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
    assert!(app.render_scene(doc).unwrap().meshes.is_empty());

    let wall = run(
        &mut app,
        "wall.create",
        json!({"curve": {"line": {"start": [0, 0], "end": [4, 0]}}, "base_level": level.0, "top_constraint": {"unconnected": 3}}),
    );
    let wall = ElementId(wall.as_u64().unwrap());
    let scene = app.render_scene(doc).unwrap();
    assert_eq!(scene.meshes.len(), 1);
    assert_eq!(scene.meshes[0].element, Some(wall));
    let bounds = scene.bounds().unwrap();
    assert_eq!((bounds.0[0], bounds.1[0], bounds.1[2]), (0.0, 4.0, 3.0));

    // After saving, the wall is still drawn once (from its recipe, not also from the IFC).
    run(
        &mut app,
        "file.save_as",
        json!({"path": dir.path().join("w.ifc")}),
    );
    assert_eq!(app.render_scene(doc).unwrap().meshes.len(), 1);
    run(&mut app, "edit.undo", json!({}));
    assert!(app.render_scene(doc).unwrap().meshes.is_empty());
}

#[test]
fn views_are_model_objects_and_navigation_is_session_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    run(&mut app, "file.new", json!({}));
    let doc = app.active().unwrap();
    let view = ElementId(run(&mut app, "view.create_3d", json!({})).as_u64().unwrap());
    // The default view is reused, and names are unique per kind.
    assert_eq!(run(&mut app, "view.create_3d", json!({})), json!(view.0));
    let second = run(&mut app, "view.create_3d", json!({"kind": "named"}));
    assert_eq!(
        fails(
            &mut app,
            "view.rename",
            json!({"view_id": second, "new_name": "{3D}"})
        ),
        "view.name_taken"
    );
    let list = run(&mut app, "view.list", json!({}));
    let names: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["{3D 2}", "{3D}"]);
    let path = dir.path().join("views.ifc");
    run(&mut app, "file.save_as", json!({"path": path}));
    assert!(!app.documents()[0].dirty);

    // Navigation: no undo entry, not a model change.
    let mut cam = reviera_core::views::ViewCamera::fitting([0.0; 3], [10.0, 8.0, 3.0]);
    cam.perspective = false;
    app.set_view_camera(doc, view, cam);
    run(
        &mut app,
        "view.orient",
        json!({"view_id": view.0, "preset": "top"}),
    );
    let top = app.view_camera(doc, view).unwrap();
    assert!(!top.perspective);
    assert!((top.eye[0] - top.target[0]).abs() < 1e-9 && top.eye[2] > top.target[2]);
    assert!(!app.documents()[0].dirty);
    // Two view creations; navigation added nothing.
    assert_eq!(
        run(&mut app, "edit.history", json!({}))
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    // Saved with the next save, and back on reopen.
    run(&mut app, "test.add_proxy", json!({"name": "P"}));
    run(&mut app, "file.save", json!({}));
    run(&mut app, "file.close", json!({"document_id": doc}));
    let doc = run(&mut app, "file.open", json!({"path": path}))
        .as_u64()
        .unwrap();
    assert_eq!(app.view_camera(doc, view), Some(top));

    // A script's saved camera wins over older navigation.
    run(
        &mut app,
        "view.set_camera",
        json!({"view_id": view.0, "eye": [20, -20, 15], "target": [0, 0, 0], "projection": "perspective"}),
    );
    let c = app.view_camera(doc, view).unwrap();
    assert_eq!((c.eye, c.perspective), ([20.0, -20.0, 15.0], true));
    assert_eq!(
        run(&mut app, "view.get_camera", json!({"view_id": view.0}))["eye"],
        json!([20.0, -20.0, 15.0])
    );

    // view.open asks the UI to show the view.
    run(&mut app, "view.open", json!({"view_id": second}));
    assert_eq!(
        app.take_view_requests(),
        [(doc, ElementId(second.as_u64().unwrap()))]
    );
    run(&mut app, "view.delete", json!({"view_ids": [second]}));
    assert_eq!(
        run(&mut app, "view.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn models_load_on_another_thread_and_attach_on_this_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path());
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc");
    let stages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = stages.clone();
    let path = file.clone();
    let loaded = std::thread::spawn(move || {
        reviera_app::load(&path, &|s| seen.lock().unwrap().push(s)).unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(
        *stages.lock().unwrap(),
        [
            reviera_app::LoadStage::Reading,
            reviera_app::LoadStage::Geometry
        ]
    );
    let id = app.open_loaded(loaded, true).unwrap();
    assert_eq!(app.active(), Some(id));
    // The geometry made on the other thread is used as it is.
    let start = std::time::Instant::now();
    let scene = app.render_scene(id).unwrap();
    assert!(scene.meshes.len() > 50);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    // A file already open is reported, with its document.
    let again = reviera_app::load(&file, &|_| {}).unwrap();
    let e = app.open_loaded(again, true).unwrap_err();
    assert_eq!(e.code, "file.already_open");
    assert_eq!(app.document_at(&file), Some(id));
}
