// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Section and elevation drawings (DOC-VIEW-003/004): cut walls, projection beyond, levels and
//! grids crossing the view plane.

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
fn a_section_cuts_walls_and_shows_levels_and_grids() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
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
    // Two parallel walls running north, 6 m apart, 3 m high.
    for x in [0.0, 6.0] {
        run(
            &mut app,
            "wall.create",
            json!({
                "curve": { "line": { "start": [x, 0.0], "end": [x, 8.0] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap();
    }
    run(
        &mut app,
        "grid.create",
        json!({ "curve": { "line": { "start": [3.0, -2.0], "end": [3.0, 10.0] } } }),
    )
    .unwrap();
    // A section across both walls, drawn west to east at y = 4: it looks south.
    let view = id(run(
        &mut app,
        "view.create_section",
        json!({ "start": [-2.0, 4.0], "end": [8.0, 4.0], "far_offset": 10.0 }),
    )
    .unwrap());
    let d = app.section_drawing(doc, view).unwrap();
    // Two cut walls: 0.2 × 3 m each.
    let poche_area: f32 = d
        .graphics
        .poche
        .iter()
        .map(|m| {
            m.indices
                .chunks(3)
                .map(|t| {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
                    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
                })
                .sum::<f32>()
        })
        .sum();
    assert!((poche_area - 1.2).abs() < 1e-3, "{poche_area}");
    // Looking south, the east wall (x = 6) is on the viewer's left: 2 m from the line's east
    // end; the west wall 8 m in.
    let xs: Vec<f32> = d
        .graphics
        .poche
        .iter()
        .flat_map(|m| m.positions.iter().map(|p| p[0]))
        .collect();
    assert!(
        xs.iter().any(|x| (x - 1.9).abs() < 1e-4) && xs.iter().any(|x| (x - 8.1).abs() < 1e-4),
        "{xs:?}"
    );
    // Level 1 is drawn and labelled; the grid crosses the plane at 5 m with its bubble on top.
    assert!(d.labels.iter().any(|l| l.text == "Level 1"));
    assert!(
        d.labels
            .iter()
            .any(|l| l.text == "1" && (l.origin[0] - 5.0).abs() < 0.5)
    );
    // Flipped, it looks north and the west wall is on the left.
    run(
        &mut app,
        "section.edit",
        json!({ "view_id": view.0, "flip": true }),
    )
    .unwrap();
    let d = app.section_drawing(doc, view).unwrap();
    let xs: Vec<f32> = d
        .graphics
        .poche
        .iter()
        .flat_map(|m| m.positions.iter().map(|p| p[0]))
        .collect();
    assert!(xs.iter().any(|x| (x - 1.9).abs() < 1e-4), "{xs:?}");
    // An elevation looking north from outside cuts nothing but sees both walls' ends.
    let e = id(run(
        &mut app,
        "view.create_elevation",
        json!({ "point": [3.0, -5.0], "look": [0.0, 1.0] }),
    )
    .unwrap());
    let d = app.section_drawing(doc, e).unwrap();
    assert!(d.graphics.poche.is_empty());
    // The grid, which stops short of the elevation's plane, still shows: it is in the view.
    assert!(d.labels.iter().any(|l| l.text == "1"));
    assert!(d.graphics.lines.iter().any(|l| !l.segments.is_empty()));
}
