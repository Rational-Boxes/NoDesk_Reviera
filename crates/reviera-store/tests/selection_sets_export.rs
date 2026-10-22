// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Saved selections (ARC-SEL-002): removing three walls from a set is one undo step; with
//! *Selection sets as IfcGroup* each set exports as an IfcGroup whose IfcRelAssignsToGroup lists
//! exactly its members, rewritten on each save and not read back as an element.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
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

#[test]
fn edit_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sets.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let walls: Vec<ElementId> = (0..6)
        .map(|k| ElementId(ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, k as f64 * 2.0], "end": [5.0, k as f64 * 2.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })).as_u64().unwrap()))
        .collect();
    let set = ok(&r, &mut doc, "selectionset.save", json!({ "name": "Corridor", "element_ids": walls }));
    let count = |r: &CommandRegistry, doc: &mut Document| ok(r, doc, "selectionset.list", json!({}))[0]["count"].as_u64().unwrap();
    // Three removed in one edit: one undo step puts them back.
    ok(&r, &mut doc, "selectionset.edit", json!({ "id": set, "remove": &walls[..3] }));
    assert_eq!(count(&r, &mut doc), 3);
    ok(&r, &mut doc, "edit.undo", json!({ "steps": 1 }));
    assert_eq!(count(&r, &mut doc), 6);
    ok(&r, &mut doc, "selectionset.edit", json!({ "id": set, "remove": &walls[..3] }));

    // Not exported until the option is on.
    project.save(&doc).unwrap();
    assert!(!IfcModel::open(&path).unwrap().step_rows().unwrap().iter().any(|r| r.class == "IFCGROUP" && r.attributes.contains("'SelectionSet'")));
    ok(&r, &mut doc, "selectionset.set_export", json!({ "as_ifc_group": true }));
    project.save(&doc).unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let groups: Vec<_> = rows.iter().filter(|r| r.class == "IFCGROUP" && r.attributes.contains("'SelectionSet'")).collect();
    assert_eq!(groups.len(), 1, "one per set, after two saves");
    assert!(groups[0].attributes.contains("'Corridor'"));
    let token = |text: &str, id: u32| text.split(|c: char| !(c == '#' || c.is_ascii_digit())).any(|t| t == format!("#{id}"));
    let rel = rows.iter().find(|r| r.class == "IFCRELASSIGNSTOGROUP" && token(&r.attributes, groups[0].id.0)).expect("its members");
    let member_steps: Vec<u32> = rows
        .iter()
        .filter(|row| row.global_id.as_ref().is_some_and(|g| walls[3..].iter().any(|w| doc.store().get(*w).unwrap().guid().to_string() == *g)))
        .map(|row| row.id.0)
        .collect();
    assert_eq!(member_steps.len(), 3);
    assert!(member_steps.iter().all(|s| token(&rel.attributes, *s)));
    let listed = rel.attributes.matches('#').count() - 1;
    assert_eq!(listed, 3, "exactly the members: {}", rel.attributes);
    // Reopened: the set is still one set, and no group element came back.
    let (_p, again, _) = LocalProject::open(&path).unwrap();
    assert_eq!(reviera_core::selection_sets::all(again.store()).len(), 1);
    assert!(!again.store().iter().any(|e| e.class == "IfcGroup" && e.name.as_deref() == Some("Corridor")));
    // Off again: the groups go on the next save.
    ok(&r, &mut doc, "selectionset.set_export", json!({ "as_ifc_group": false }));
    project.save(&doc).unwrap();
    assert!(!IfcModel::open(&path).unwrap().step_rows().unwrap().iter().any(|r| r.class == "IFCGROUP" && r.attributes.contains("'SelectionSet'")));
}
