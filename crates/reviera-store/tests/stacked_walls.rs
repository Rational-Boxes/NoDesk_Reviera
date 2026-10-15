// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stacked walls (ARC-STKW-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, walls};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

#[test]
fn stacked_wall_rows_and_break_up() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A basic wall to get the default type, and a thicker base type.
    let plain = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 5.0], "end": [6.0, 5.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let upper = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .unwrap()
        .id();
    let base = id(&run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": upper.0, "new_name": "Plinth 300" }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": base.0, "param": "width", "value": 0.3 }),
    )
    .unwrap();
    // Two variable rows are refused.
    assert_eq!(
        run(
            &r,
            &mut doc,
            "stackedwall.create_type",
            json!({ "name": "Bad", "rows": [{ "wall_type_id": upper.0 }, { "wall_type_id": base.0 }] }),
        )
        .unwrap_err()
        .code,
        "stacked.variable"
    );
    // Upper: variable; base: 900 mm, flush on the exterior face.
    let st = id(&run(
        &r,
        &mut doc,
        "stackedwall.create_type",
        json!({ "name": "Stacked", "offset_reference": "finish_face_exterior", "rows": [
            { "wall_type_id": upper.0 },
            { "wall_type_id": base.0, "height": 0.9 },
        ] }),
    )
    .unwrap());
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "type_id": st.0, "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 2.7 } }),
    )
    .unwrap());
    let door = id(&run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 3.0 }),
    )
    .unwrap());
    let e = doc.store().get(wall).unwrap();
    let g = walls::regenerate(doc.store(), e).unwrap();
    let subs = walls::stacked::layout(doc.store(), e, &g).unwrap();
    assert_eq!(subs.len(), 2);
    assert!((subs[1].z[1] - subs[1].z[0] - 0.9).abs() < 1e-9, "{subs:?}");
    assert!((subs[0].z[1] - subs[0].z[0] - 1.8).abs() < 1e-9, "{subs:?}");
    // Exterior faces flush (exterior is +y for a wall drawn along +x).
    let ext = |s: &walls::stacked::SubWall| s.centre + s.width / 2.0;
    assert!((ext(&subs[0]) - ext(&subs[1])).abs() < 1e-9, "{subs:?}");
    // The body is two pieces.
    let parts = reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == wall)
        .map(|b| b.parts.len())
        .sum::<usize>();
    assert_eq!(parts, 2);
    // Too short for its fixed rows.
    assert_eq!(
        run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "type_id": st.0, "curve": { "line": { "start": [0.0, -4.0], "end": [6.0, -4.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 0.5 } }),
        )
        .unwrap_err()
        .code,
        "stacked.too_short"
    );
    // Saved: sub-walls aggregated with their derived GlobalIds; reopened without them as elements.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for s in &subs {
        assert!(
            text.contains(s.guid.as_str()),
            "sub-wall {} written",
            s.guid.as_str()
        );
    }
    assert!(text.contains(".ELEMENTEDWALL."));
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    assert!(reopened.store().find_by_guid(&subs[0].guid).is_none());
    // Break Up: two basic walls with the sub-walls' GlobalIds; the door goes to the upper one.
    let made: Vec<ElementId> = run(
        &r,
        &mut doc,
        "wall.break_up",
        json!({ "stacked_wall_ids": [wall.0] }),
    )
    .unwrap()
    .as_array()
    .unwrap()
    .iter()
    .map(id)
    .collect();
    assert_eq!(made.len(), 2);
    for (m, s) in made.iter().zip(&subs) {
        assert_eq!(doc.store().get(*m).unwrap().guid(), &s.guid);
        let g = walls::regenerate(doc.store(), doc.store().get(*m).unwrap()).unwrap();
        assert!((g.base_z - s.z[0]).abs() < 1e-9 && (g.height - (s.z[1] - s.z[0])).abs() < 1e-9);
        assert!((g.width - s.width).abs() < 1e-9);
    }
    assert!(doc.store().get(wall).is_none());
    let host = reviera_core::inserts::insert_recipe(doc.store().get(door).unwrap())
        .unwrap()
        .host;
    assert_eq!(host, subs[0].guid.as_str());
    let _ = plain;
    project.save(&doc).unwrap();
}
