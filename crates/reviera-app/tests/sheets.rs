// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sheets (DOC-SHT-001…003, DOC-SCH-011): numbering, title-block values, viewports of plans and
//! schedules, one sheet per view, and PDF pages of the sheet's size.

use reviera_app::sheets::ItemKind;
use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    serde_json::from_value(v).unwrap()
}

#[test]
fn sheets_hold_views_and_schedules_and_print_at_their_size() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let doc = app.active().unwrap();
    let store = app.document(doc).unwrap().doc.store().clone();
    let level = store
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let plans = run(
        &mut app,
        "view.create_plan",
        json!({ "level_ids": [level.0] }),
    )
    .unwrap();
    let plan: ElementId = serde_json::from_value(plans[0].clone()).unwrap();
    run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0.0, 0.0], "end": [8.0, 0.0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap();

    // Numbering: A101, then A102; numbers are unique.
    let a = id(run(&mut app, "sheet.create", json!({ "name": "Plans" })).unwrap());
    let b = id(run(&mut app, "sheet.create", json!({ "size": "A3" })).unwrap());
    let number = |app: &App, s: ElementId| {
        let store = app.document(doc).unwrap().doc.store();
        reviera_core::sheets::sheet_data(store.get(s).unwrap())
            .unwrap()
            .number
    };
    assert_eq!(number(&app, a), "A101");
    assert_eq!(number(&app, b), "A102");
    let err = run(
        &mut app,
        "sheet.set_parameter",
        json!({ "sheet_id": b.0, "number": "A101" }),
    )
    .unwrap_err();
    assert_eq!(err.code, "sheet.number_in_use");
    run(
        &mut app,
        "sheet.set_parameter",
        json!({ "sheet_id": a.0, "parameters": { "Drawn By": "JH", "Issue Date": "2026-10-13" } }),
    )
    .unwrap();
    let err = run(
        &mut app,
        "sheet.set_parameter",
        json!({ "sheet_id": a.0, "parameters": { "Colour": "red" } }),
    )
    .unwrap_err();
    assert_eq!(err.code, "sheet.unknown_parameter");

    // A plan goes on one sheet only.
    let vp: u32 = serde_json::from_value(
        run(
            &mut app,
            "sheet.place_view",
            json!({ "sheet_id": a.0, "view_id": plan.0, "position": [300.0, 300.0] }),
        )
        .unwrap(),
    )
    .unwrap();
    let err = run(
        &mut app,
        "sheet.place_view",
        json!({ "sheet_id": b.0, "view_id": plan.0 }),
    )
    .unwrap_err();
    assert_eq!(err.code, "view.already_on_sheet");

    let schedule = id(run(
        &mut app,
        "schedule.create",
        json!({ "category": "cat.walls" }),
    )
    .unwrap());
    run(
        &mut app,
        "sheet.place_schedule",
        json!({ "sheet_id": a.0, "schedule_id": schedule.0, "position": [40.0, 560.0] }),
    )
    .unwrap();

    // The viewport is drawn at 1:100 around its position: the 8 m wall is 80 mm long.
    let s = app.sheet_drawing(doc, a).unwrap();
    let paper = s.drawing.paper.clone().unwrap();
    assert_eq!(
        (paper.size, paper.width, paper.height),
        ("A1", 841.0, 594.0)
    );
    let view = s
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Viewport)
        .unwrap();
    assert_eq!(view.id, vp);
    let w = (view.max[0] - view.min[0]) * 1000.0;
    assert!(w > 79.9 && w < 120.0, "viewport width {w} mm");
    assert!(view.min[0] < 0.3 && view.max[0] > 0.3);
    let table = s
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Schedule)
        .unwrap();
    assert!((table.min[0] - 0.04).abs() < 1e-6 && (table.max[1] - 0.56).abs() < 1e-6);
    let texts: Vec<&str> = s.drawing.labels.iter().map(|l| l.text.as_str()).collect();
    for t in [
        "A101",
        "Plans",
        "JH",
        "2026-10-13",
        "1 : 100",
        "Wall Schedule",
    ] {
        assert!(texts.contains(&t), "{t} missing from {texts:?}");
    }

    // Moving the viewport moves its drawing; removing it frees the view for another sheet.
    run(
        &mut app,
        "viewport.move",
        json!({ "sheet_id": a.0, "viewport_id": vp, "position": [400.0, 300.0] }),
    )
    .unwrap();
    let moved = app.sheet_drawing(doc, a).unwrap();
    let mv = moved.items.iter().find(|i| i.id == vp).unwrap();
    assert!((mv.min[0] - view.min[0] - 0.1).abs() < 1e-4);
    run(
        &mut app,
        "viewport.remove",
        json!({ "sheet_id": a.0, "viewport_ids": [vp] }),
    )
    .unwrap();
    run(
        &mut app,
        "sheet.place_view",
        json!({ "sheet_id": b.0, "view_id": plan.0 }),
    )
    .unwrap();

    // Each sheet prints as one page of its paper size.
    let path = std::env::temp_dir().join(format!("reviera-sheets-{}.pdf", std::process::id()));
    let out = run(
        &mut app,
        "export.pdf",
        json!({ "range": { "views": [a.0, b.0] }, "path_or_folder": path }),
    )
    .unwrap();
    assert_eq!(out["sheets"], json!(["A1", "A3"]));
    let bytes = std::fs::read(&path).unwrap();
    let pdf = String::from_utf8_lossy(&bytes);
    assert!(
        pdf.contains("/MediaBox [0 0 2383.937 1683.7795]"),
        "A1 page"
    );
    std::fs::remove_file(&path).ok();

    // Sheets rename and delete like views; a deleted view leaves its sheet.
    run(
        &mut app,
        "view.rename",
        json!({ "view_id": b.0, "new_name": "Ground Floor" }),
    )
    .unwrap();
    run(&mut app, "view.delete", json!({ "view_ids": [plan.0] })).unwrap();
    let store = app.document(doc).unwrap().doc.store().clone();
    let sheet = store.get(b).unwrap();
    assert_eq!(sheet.name.as_deref(), Some("Ground Floor"));
    assert!(
        reviera_core::sheets::sheet_data(sheet)
            .unwrap()
            .viewports
            .is_empty()
    );
    run(&mut app, "view.delete", json!({ "view_ids": [b.0] })).unwrap();
    assert!(app.document(doc).unwrap().doc.store().get(b).is_none());
    run(&mut app, "sheet.delete", json!({ "sheet_ids": [a.0] })).unwrap();
    let store = app.document(doc).unwrap().doc.store();
    assert!(store.get(a).is_none() && store.get(schedule).is_some());
}
