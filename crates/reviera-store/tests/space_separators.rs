// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Space separation lines (MEP-SPACE-001): a space separator across a walled box splits the
//! spaces in it but not the room; Place Spaces Automatically then finds two regions; deleting
//! the separator gives the space the whole box again; a room separator still bounds both.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn space_area(doc: &Document, id: &Value) -> f64 {
    reviera_core::mep_spaces::info(doc.store(), ElementId(id.as_u64().unwrap())).unwrap().area
}

fn room_area(doc: &Document) -> f64 {
    reviera_core::rooms::all(doc.store())[0].geometry.area()
}

#[test]
fn space_separators() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("sep.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    for (a, b) in [([0.0, 0.0], [8.0, 0.0]), ([8.0, 0.0], [8.0, 4.0]), ([8.0, 4.0], [0.0, 4.0]), ([0.0, 4.0], [0.0, 0.0])] {
        ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }));
    }
    let whole_space = {
        let s = ok(&r, &mut doc, "space.place", json!({ "level_id": l1.0, "point": [2, 2] }));
        let a = space_area(&doc, &s);
        ok(&r, &mut doc, "edit.undo", json!({}));
        a
    };
    let sep = ok(&r, &mut doc, "space_separator.create", json!({ "level_id": l1.0, "lines": [{ "start": [4, 0], "end": [4, 4] }] }));
    let space = ok(&r, &mut doc, "space.place", json!({ "level_id": l1.0, "point": [2, 2] }));
    ok(&r, &mut doc, "room.place", json!({ "level_id": l1.0, "point": [2, 2] }));
    let half = space_area(&doc, &space);
    assert!((half - whole_space / 2.0).abs() < 1e-6, "{half} of {whole_space}");
    assert!((room_area(&doc) - whole_space).abs() < 1e-6, "the room ignores the space separator");
    assert_eq!(doc.store().get(ElementId(sep[0].as_u64().unwrap())).unwrap().name.as_deref(), Some("Space Separation"));
    // The other half gets the second space.
    let auto = ok(&r, &mut doc, "space.place_auto", json!({ "level_id": l1.0 }));
    assert_eq!(auto.as_array().unwrap().len(), 1);
    // Without the separator the space fills the box.
    ok(&r, &mut doc, "element.delete", json!({ "element_ids": sep }));
    assert!((space_area(&doc, &space) - whole_space).abs() < 1e-6);
    // A room separator bounds rooms and spaces both.
    ok(&r, &mut doc, "roomseparator.create", json!({ "level_id": l1.0, "lines": [{ "start": [4, 0], "end": [4, 4] }] }));
    assert!((space_area(&doc, &space) - whole_space / 2.0).abs() < 1e-6);
    assert!((room_area(&doc) - whole_space / 2.0).abs() < 1e-6);
}
