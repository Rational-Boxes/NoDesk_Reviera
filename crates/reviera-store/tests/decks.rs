// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Decks (ARC-DECK-001) by command: boards, framing and the cut list; settings and outline edits
//! laying them out again; railings on the open edges; and the saved bodies.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, decks};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

#[test]
fn a_deck_by_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deck.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let c = [[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]];
    let outline: Vec<Value> = (0..4)
        .map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } }))
        .collect();
    let deck = ElementId(
        run(
            &r,
            &mut doc,
            "deck.create",
            json!({ "outline": [outline], "level_id": level.0, "height": "700 mm" }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let e = doc.store().get(deck).unwrap();
    assert_eq!(e.param("@ObjectType").unwrap(), "DECK");
    // Boards along the 6 m side: 28 courses across the 4 m.
    let boards = run(&r, &mut doc, "deck.boards", json!({ "deck_id": deck.0 })).unwrap();
    let courses: std::collections::BTreeSet<u64> = boards
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["course"].as_u64())
        .collect();
    assert_eq!(courses.len(), 28);
    let list = run(&r, &mut doc, "deck.cut_list", json!({ "deck_id": deck.0 })).unwrap();
    assert!((list["area"].as_f64().unwrap() - 24.0).abs() < 1e-9);
    assert!(list["joists"].as_u64().unwrap() >= 16);
    let total = list["board_length"].as_f64().unwrap();
    // 28 courses of 6 m, less nothing: every course runs the full length.
    assert!((total - 28.0 * 6.0).abs() < 1e-6, "{total}");

    // A picture frame and wider boards lay the deck out again.
    run(
        &r,
        &mut doc,
        "deck.set_boards",
        json!({ "deck_id": deck.0, "settings": { "pictureFrame": 1, "boardWidth": 0.19 } }),
    )
    .unwrap();
    let g = decks::regenerate(doc.store(), doc.store().get(deck).unwrap()).unwrap();
    assert_eq!(g.boards.iter().filter(|b| b.course.is_none()).count(), 4);
    // The outline grows 1 m east; boards and joists follow.
    run(
        &r,
        &mut doc,
        "sketch.move_curve",
        json!({ "element_id": deck.0, "key": 2, "vector": [1.0, 0.0] }),
    )
    .unwrap();
    let g2 = decks::regenerate(doc.store(), doc.store().get(deck).unwrap()).unwrap();
    assert!((g2.cut_list.area - 28.0).abs() < 1e-9);
    assert!(g2.cut_list.joists > g.cut_list.joists);
    // Railings guard its open edges at its top.
    let made = run(
        &r,
        &mut doc,
        "railing.create_on_edges",
        json!({ "host_id": deck.0 }),
    )
    .unwrap();
    assert_eq!(made.as_array().unwrap().len(), 1);

    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    // The deck's body is its parts, not a slab.
    let breps = ifc.entities_of_type("IfcFacetedBrep").unwrap().len();
    assert!(breps >= g2.parts.len(), "{breps}");
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    let again = decks::regenerate(reopened.store(), reopened.store().get(deck).unwrap()).unwrap();
    assert_eq!(again.boards.len(), g2.boards.len());

    // Deleting the deck deletes its railing.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [deck.0] }),
    )
    .unwrap();
    assert!(
        doc.store()
            .iter()
            .all(|e| reviera_core::railings::railing_recipe(e).is_none())
    );
}
