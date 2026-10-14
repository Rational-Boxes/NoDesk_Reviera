// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Lua scripts and plugins: the `reviera.*` API, forms answered by the call, one undo step per
//! run, rollback on failure, and the built-in Castle Turret plugin in every style.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn app() -> App {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    app
}

fn count(app: &App, f: impl Fn(&reviera_core::Element) -> bool) -> usize {
    let d = app.document(app.active().unwrap()).unwrap();
    d.doc.store().iter().filter(|e| f(e)).count()
}

fn walls(app: &App) -> usize {
    count(app, |e| reviera_core::walls::wall_recipe(e).is_some())
}

#[test]
fn scripts_call_commands_as_one_undo_step() {
    let mut app = app();
    let out = run(
        &mut app,
        "script.run",
        json!({ "name": "two walls", "code": r#"
            local l = reviera.level.list()[1].id
            for i = 0, 1 do
              reviera.wall.create{ curve = { line = { start = {0, i * 3}, ["end"] = {4, i * 3} } },
                                   base_level = l, top_constraint = { unconnected = 3 } }
            end
            print("done")
            return #reviera.level.list()
        "# }),
    )
    .unwrap();
    assert_eq!(out["result"], 1);
    assert_eq!(out["output"], json!(["done"]));
    assert_eq!(walls(&app), 2);
    // One undo removes both.
    run(&mut app, "edit.undo", json!({})).unwrap();
    assert_eq!(walls(&app), 0);
    // A failing script leaves nothing behind.
    let err = run(
        &mut app,
        "script.run",
        json!({ "code": r#"
            local l = reviera.level.list()[1].id
            reviera.wall.create{ curve = { line = { start = {0, 0}, ["end"] = {4, 0} } },
                                 base_level = l, top_constraint = { unconnected = 3 } }
            reviera.wall.create{ curve = { line = { start = {0, 0}, ["end"] = {0, 0} } },
                                 base_level = l, top_constraint = { unconnected = 3 } }
        "# }),
    )
    .unwrap_err();
    assert_eq!(err.code, "script.failed");
    assert_eq!(walls(&app), 0);
}

#[test]
fn the_turret_plugin_builds_every_style() {
    let list = run(&mut app(), "plugin.list", json!({})).unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "turret")
    );
    for (k, style) in [
        "Cone",
        "Pointed dome",
        "Onion dome",
        "Hemisphere",
        "Battlements",
    ]
    .iter()
    .enumerate()
    {
        let mut app = app();
        let out = run(
            &mut app,
            "plugin.run",
            json!({ "name": "turret", "answers": {
                "roof": style, "radius": "3000", "sides": 12, "x": k as f64 * 10.0,
            } }),
        )
        .unwrap_or_else(|e| panic!("{style}: {e:?}"));
        let r = &out["result"];
        assert_eq!(r["walls"].as_array().unwrap().len(), 12, "{style}");
        assert_eq!(r["slits"], 4, "{style}: {out}");
        assert_eq!(count(&app, |e| e.class == "IfcWindow"), 4, "{style}");
        if *style == "Battlements" {
            assert!(r["parapet"].as_u64().unwrap() >= 24, "{out}");
            assert_eq!(
                count(&app, |e| reviera_core::floors::slab_recipe(e).is_some()),
                1
            );
        } else {
            let roofs: Vec<_> = {
                let d = app.document(app.active().unwrap()).unwrap();
                reviera_core::roofs::all(d.doc.store())
            };
            assert_eq!(roofs.len(), 1, "{style}");
            // The roof sits on the walls (7 m) and rises above them.
            assert!((roofs[0].1.plate - 7.0).abs() < 1e-9, "{style}");
            assert!(roofs[0].1.max_height() > 9.0, "{style}");
        }
        assert!(
            out["output"][0].as_str().unwrap().starts_with("Turret:"),
            "{out}"
        );
        // One undo step takes it all away.
        run(&mut app, "edit.undo", json!({})).unwrap();
        assert_eq!(walls(&app), 0, "{style}");
    }
    // Answers out of range: the plugin fails with the field's problem, and nothing is left.
    let mut a = app();
    let err = run(
        &mut a,
        "plugin.run",
        json!({ "name": "turret", "answers": { "sides": 3 } }),
    )
    .unwrap_err();
    assert!(err.message.contains("Sides"), "{}", err.message);
    assert_eq!(walls(&a), 0);
}

#[test]
fn the_library_imports_and_removes_scripts() {
    let lib = tempfile::tempdir().unwrap();
    // The library is per process here.
    unsafe { std::env::set_var("REVIERA_LIBRARY", lib.path()) };
    let src = tempfile::tempdir().unwrap();
    let file = src.path().join("hello.lua");
    std::fs::write(
        &file,
        "-- title: Hello World\n-- description: Says hello\nprint('hello')\n",
    )
    .unwrap();
    let mut app = app();
    let got = run(&mut app, "script.import", json!({ "path": file })).unwrap();
    assert_eq!(got["title"], "Hello World");
    assert_eq!(got["in_library"], true);
    let out = run(&mut app, "plugin.run", json!({ "name": "hello" })).unwrap();
    assert_eq!(out["output"], json!(["hello"]));
    assert_eq!(
        run(&mut app, "script.import", json!({ "path": file }))
            .unwrap_err()
            .code,
        "script.exists"
    );
    run(&mut app, "script.remove", json!({ "name": "hello" })).unwrap();
    assert!(run(&mut app, "plugin.run", json!({ "name": "hello" })).is_err());
    // Built-in plugins stay.
    assert_eq!(
        run(&mut app, "script.remove", json!({ "name": "turret" }))
            .unwrap_err()
            .code,
        "script.not_in_library"
    );
}
