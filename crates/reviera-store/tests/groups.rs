// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Groups and saved selections (ARC-GRP-001 … 003, ARC-SEL-002).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, groups, walls};
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

fn members(doc: &Document, instance: ElementId) -> Vec<ElementId> {
    let g = groups::recipe(doc.store().get(instance).unwrap()).unwrap();
    g.members
        .iter()
        .filter_map(|m| {
            doc.store()
                .find_by_guid(&reviera_core::GlobalId::parse(&m.guid).unwrap())
        })
        .collect()
}

#[test]
fn groups_place_edit_exclude_and_save() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("g.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [4.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let door = id(&run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": wall.0, "point_along": 2.0 }),
    )
    .unwrap());
    // Grouping the wall takes its door along.
    let made = run(
        &r,
        &mut doc,
        "group.create",
        json!({ "element_ids": [wall.0], "name": "Bay" }),
    )
    .unwrap();
    let first = id(&made["instance_id"]);
    assert_eq!(members(&doc, first).len(), 2);
    assert!(members(&doc, first).contains(&door));
    // Members are read-only outside the editor.
    let e = run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [wall.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "element.group_member");
    // A second instance 10 m east, turned 90°: its own wall with its own door on it.
    let second = id(&run(
        &r,
        &mut doc,
        "group.place",
        json!({ "group_type_id": "Bay", "point": [12.0, 0.0], "rotation": "90°" }),
    )
    .unwrap());
    let m2 = members(&doc, second);
    assert_eq!(m2.len(), 2);
    let w2 = *m2
        .iter()
        .find(|m| walls::wall_recipe(doc.store().get(**m).unwrap()).is_some())
        .unwrap();
    let d2 = *m2.iter().find(|m| **m != w2).unwrap();
    let g2 = walls::regenerate(doc.store(), doc.store().get(w2).unwrap()).unwrap();
    // The first instance's origin is the wall's middle (2, 0): the copy runs north through (12, 0).
    assert!(
        (g2.start[0] - 12.0).abs() < 1e-6 && (g2.end[0] - 12.0).abs() < 1e-6,
        "{g2:?}"
    );
    assert!((g2.length() - 4.0).abs() < 1e-6);
    let host = reviera_core::inserts::insert_recipe(doc.store().get(d2).unwrap())
        .unwrap()
        .host;
    assert_eq!(host, doc.store().get(w2).unwrap().guid().to_string());
    // Edit the first: lengthen nothing, add a wall; Finish remakes the second with it too.
    run(
        &r,
        &mut doc,
        "group.edit",
        json!({ "instance_id": first.0 }),
    )
    .unwrap();
    let extra = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 2.0], "end": [4.0, 2.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "group.add_members",
        json!({ "element_ids": [extra.0] }),
    )
    .unwrap();
    run(&r, &mut doc, "group.finish", json!({})).unwrap();
    assert_eq!(members(&doc, first).len(), 3);
    assert_eq!(members(&doc, second).len(), 3);
    // Cancel leaves every instance as it was.
    run(
        &r,
        &mut doc,
        "group.edit",
        json!({ "instance_id": second.0 }),
    )
    .unwrap();
    let victim = members(&doc, second)[0];
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [victim.0] }),
    )
    .unwrap();
    run(&r, &mut doc, "group.cancel", json!({})).unwrap();
    assert_eq!(members(&doc, second).len(), 3);
    assert_eq!(members(&doc, first).len(), 3);
    // Exclude the second's door: gone from it only; restore brings it back.
    let d2 = *members(&doc, second)
        .iter()
        .find(|m| reviera_core::inserts::insert_recipe(doc.store().get(**m).unwrap()).is_some())
        .unwrap();
    run(
        &r,
        &mut doc,
        "group.exclude_member",
        json!({ "instance_id": second.0, "element_id": d2.0 }),
    )
    .unwrap();
    assert_eq!(members(&doc, second).len(), 2);
    assert_eq!(members(&doc, first).len(), 3);
    let doors = |doc: &Document| doc.store().iter().filter(|e| e.class == "IfcDoor").count();
    assert_eq!(doors(&doc), 1);
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("IFCRELASSIGNSTOGROUP(").count(), 2);
    run(
        &r,
        &mut doc,
        "group.restore_member",
        json!({ "instance_id": second.0 }),
    )
    .unwrap();
    assert_eq!(doors(&doc), 2);
    // Ungroup: the members stay, free to edit.
    let freed = run(
        &r,
        &mut doc,
        "group.ungroup",
        json!({ "instance_ids": [second.0] }),
    )
    .unwrap();
    assert_eq!(freed.as_array().unwrap().len(), 3);
    let any = id(&freed[0]);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [any.0], "vector": [0.0, 1.0, 0.0] }),
    )
    .unwrap();
    project.save(&doc).unwrap();
}

#[test]
fn saved_selections_drop_deleted_members() {
    let tmp = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("s.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let mut ids = Vec::new();
    for k in 0..12 {
        let y = k as f64 * 2.0;
        ids.push(id(&run(
            &r,
            &mut doc,
            "wall.create",
            json!({ "curve": { "line": { "start": [0.0, y], "end": [3.0, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
        )
        .unwrap()));
    }
    run(&r, &mut doc, "selectionset.save", json!({ "name": "North Walls", "element_ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() })).unwrap();
    // A taken name is refused unless replacing.
    assert_eq!(
        run(
            &r,
            &mut doc,
            "selectionset.save",
            json!({ "name": "North Walls", "element_ids": [ids[0].0] })
        )
        .unwrap_err()
        .code,
        "selectionset.name_taken"
    );
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [ids[3].0] }),
    )
    .unwrap();
    let loaded = run(
        &r,
        &mut doc,
        "selectionset.load",
        json!({ "id": "North Walls" }),
    )
    .unwrap();
    assert_eq!(loaded.as_array().unwrap().len(), 11);
    run(
        &r,
        &mut doc,
        "selectionset.edit",
        json!({ "id": "North Walls", "remove": [ids[0].0, ids[1].0, ids[2].0] }),
    )
    .unwrap();
    let list = run(&r, &mut doc, "selectionset.list", json!({})).unwrap();
    assert_eq!(list[0]["count"], json!(8));
    run(
        &r,
        &mut doc,
        "selectionset.rename",
        json!({ "id": "North Walls", "name": "Some Walls" }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "selectionset.delete",
        json!({ "id": "Some Walls" }),
    )
    .unwrap();
    assert!(
        run(&r, &mut doc, "selectionset.list", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
