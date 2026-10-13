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
