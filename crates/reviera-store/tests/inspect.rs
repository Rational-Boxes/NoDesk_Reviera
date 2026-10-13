// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The model tree and the scopes of temporary visibility (UI-TREE-001, DOC-GFX-008), and the
//! section box and slice planes of 3D views (DOC-VIEW-008, DOC-VIEW-014).

use std::path::Path;

use reviera_core::inspect::{self, NodeKind, Scope, TreeNode};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn find<'a>(n: &'a TreeNode, f: &dyn Fn(&TreeNode) -> bool) -> Option<&'a TreeNode> {
    if f(n) {
        return Some(n);
    }
    n.children.iter().find_map(|c| find(c, f))
}

#[test]
fn the_fzk_house_tree_has_storeys_and_doors_under_their_walls() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc");
    let (project, doc, _) = LocalProject::open(&path).unwrap();
    let store = doc.store();
    let imported = project.spatial_parents().unwrap();
    let tree = inspect::tree(store, &imported);
    assert_eq!(tree.kind, NodeKind::Project);
    let mut storeys = Vec::new();
    fn collect<'a>(n: &'a TreeNode, out: &mut Vec<&'a TreeNode>) {
        if n.kind == NodeKind::Storey {
            out.push(n);
        }
        n.children.iter().for_each(|c| collect(c, out));
    }
    collect(&tree, &mut storeys);
    assert!(storeys.len() >= 2, "{storeys:?}");
    // Every door hangs under a wall (through its opening), not under a storey.
    let parents = inspect::parents(store, &imported);
    let doors: Vec<_> = store.iter().filter(|e| e.class == "IfcDoor").collect();
    assert!(!doors.is_empty());
    for d in &doors {
        let p = store.get(parents[&d.id()]).unwrap();
        assert!(
            p.class.starts_with("IfcWall"),
            "{} under {}",
            d.id(),
            p.class
        );
    }
    // The ground floor's subtree has walls, doors and slabs, and nothing from the other storey.
    let ground = storeys
        .iter()
        .min_by(|a, b| {
            let z = |n: &TreeNode| {
                store
                    .get(n.id.unwrap())
                    .and_then(|e| e.param("@Elevation"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            };
            z(a).total_cmp(&z(b))
        })
        .unwrap();
    let below = inspect::expand(store, &parents, &[ground.id.unwrap()], Scope::Subtree);
    let class_count = |c: &str| {
        below
            .iter()
            .filter(|id| store.get(**id).unwrap().class.starts_with(c))
            .count()
    };
    assert!(class_count("IfcWall") > 0 && class_count("IfcDoor") > 0);
    let tree_ids = ground.elements();
    assert_eq!(
        below,
        tree_ids.into_iter().collect(),
        "the tree and the subtree scope agree"
    );
    // Category nodes are labelled with their count.
    assert!(
        find(ground, &|n| n.kind == NodeKind::Category
            && n.label.starts_with("Walls ("))
        .is_some()
    );
}

#[test]
fn reviera_elements_are_placed_by_their_data_and_scopes_expand() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("t.ifc")).unwrap();
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = |doc: &mut Document, y: f64| {
        id(run(
            &r,
            doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": [0, y], "end": [6, y] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap())
    };
    let (a, b) = (wall(&mut doc, 0.0), wall(&mut doc, 5.0));
    let door = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "host_wall_id": a.0, "point_along": 2.0 }),
    )
    .unwrap());
    let store = doc.store();
    let parents = inspect::parents(store, &Default::default());
    assert_eq!(parents[&a], level);
    assert_eq!(parents[&door], a);
    let tree = inspect::tree(store, &Default::default());
    let storey = find(&tree, &|n| n.id == Some(level)).expect("the level is in the tree");
    let walls = find(storey, &|n| {
        n.kind == NodeKind::Category && n.label == "Walls (2)"
    })
    .expect("two walls on the level");
    let wall_a = walls.children.iter().find(|n| n.id == Some(a)).unwrap();
    assert_eq!(wall_a.children[0].id, Some(door));

    let sub = inspect::expand(store, &parents, &[a], Scope::Subtree);
    assert_eq!(sub, [a, door].into_iter().collect());
    let cat = inspect::expand(store, &parents, &[a], Scope::Category);
    assert_eq!(cat, [a, b].into_iter().collect());
    let ty = inspect::expand(store, &parents, &[b], Scope::Type);
    assert_eq!(
        ty,
        [a, b].into_iter().collect(),
        "both walls have the default type"
    );
    let all = inspect::expand(store, &parents, &[level], Scope::Subtree);
    assert!(all.contains(&a) && all.contains(&b) && all.contains(&door));
}

#[test]
fn section_boxes_and_slice_planes_are_undoable_view_data() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("t.ifc")).unwrap();
    let r = registry();
    let view = id(run(&r, &mut doc, "view.create_3d", json!({})).unwrap());
    let data =
        |doc: &Document| reviera_core::views::view_data(doc.store().get(view).unwrap()).unwrap();
    // A new box needs its corners; a flat box is refused.
    assert!(
        run(
            &r,
            &mut doc,
            "view.set_section_box",
            json!({ "view_id": view.0, "enabled": true })
        )
        .is_err()
    );
    let flat =
        json!({ "view_id": view.0, "enabled": true, "min": [0, 0, 0], "max": [5, 5, 0.001] });
    assert!(run(&r, &mut doc, "view.set_section_box", flat).is_err());
    run(
        &r,
        &mut doc,
        "view.set_section_box",
        json!({ "view_id": view.0, "enabled": true, "min": [0, 0, 0], "max": [5, 4, 3] }),
    )
    .unwrap();
    // Turning it off keeps the extents.
    run(
        &r,
        &mut doc,
        "view.set_section_box",
        json!({ "view_id": view.0, "enabled": false }),
    )
    .unwrap();
    let b = data(&doc).section_box.unwrap();
    assert!(!b.enabled && b.max == [5.0, 4.0, 3.0]);
    run(
        &r,
        &mut doc,
        "view.set_slice_planes",
        json!({ "view_id": view.0, "planes": [{ "origin": [0, 0, 1.2], "normal": [0, 0, -2] }] }),
    )
    .unwrap();
    let s = data(&doc).slice_planes;
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].normal, [0.0, 0.0, -1.0], "normals are made unit");
    let nine = json!({ "view_id": view.0, "planes": vec![json!({ "origin": [0, 0, 0], "normal": [1, 0, 0] }); 9] });
    assert!(run(&r, &mut doc, "view.set_slice_planes", nine).is_err());
    let zero =
        json!({ "view_id": view.0, "planes": [{ "origin": [0, 0, 0], "normal": [0, 0, 0] }] });
    assert!(run(&r, &mut doc, "view.set_slice_planes", zero).is_err());
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert!(data(&doc).slice_planes.is_empty());
}
