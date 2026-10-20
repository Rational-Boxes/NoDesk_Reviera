// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mounting parts (STR-PRE-005): a 9 t wall piece under "2 anchors up to 10 t" gets 2 lifting
//! anchors symmetric about its centre of gravity; a 20 t piece with bands only to 16 t gets none
//! and is reported; running again after an anchor was moved keeps it and reports it; a slab piece
//! gets anchors on its top face; anchors are held by the assembly in IFC with the piece's weight.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

fn head(doc: &Document, part: ElementId) -> [f64; 3] {
    reviera_core::precast_parts::recipe(doc.store().get(part).unwrap()).unwrap().point
}

#[test]
fn lifting_anchors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anchors.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = |doc: &mut Document, y: f64, l: f64, h: f64| {
        id(&run(&r, doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, y], "end": [l, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": h }, "structural": true })))
    };
    // A 6 m × 3 m × 200 mm wall: 9 t, one piece.
    let w = wall(&mut doc, 0.0, 6.0, 3.0);
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "200 mm" }] }));
    let bands = run(&r, &mut doc, "precast_config.get", json!({ "path": "wall.solid.connections.bands" }));
    assert_eq!(bands, json!([{ "max_weight": 10.0, "count": 2 }, { "max_weight": 16.0, "count": 4 }]));
    let a = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w.0] })));
    assert_eq!(a.len(), 1);
    let rep = run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [a[0].0] }));
    let p = &rep[0];
    assert!((p["weight"].as_f64().unwrap() - 9.0).abs() < 1e-6, "{p:#}");
    let made = ids(&p["created"]);
    assert_eq!(made.len(), 2);
    assert!(p["warnings"].as_array().unwrap().is_empty(), "{p:#}");
    let cg = p["centre"][0].as_f64().unwrap();
    assert!((cg - 3.0).abs() < 1e-6);
    let (h0, h1) = (head(&doc, made[0]), head(&doc, made[1]));
    // Symmetric about the centre of gravity, on the top edge, in the middle of the thickness.
    assert!(((h0[0] + h1[0]) / 2.0 - cg).abs() < 1e-6, "{h0:?} {h1:?}");
    assert!((h0[2] - 3.0).abs() < 1e-6 && h0[1].abs() < 1e-6);
    assert!(h0[0] >= 0.3 && h1[0] <= 5.7);

    // Running again after one anchor was moved: it is kept and reported, the other replaced.
    run(&r, &mut doc, "element.move", json!({ "element_ids": [made[0].0], "vector": [0.2, 0.0, 0.0] }));
    let again = run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [a[0].0] }));
    let g = &again[0];
    assert_eq!(g["removed"], 1);
    let kept: Vec<ElementId> = g["kept"].as_array().unwrap().iter().map(|k| id(&k["id"])).collect();
    assert_eq!(kept, [made[0]]);
    assert!(g["kept"][0]["why"].as_str().unwrap().contains("moved"));
    assert!(doc.store().get(made[1]).is_none());
    // A part placed by hand is kept too.
    let hand = id(&run(&r, &mut doc, "mounting_part.place", json!({ "assembly_id": a[0].0, "point": [3.0, 0.0, 3.0] })));
    let third = run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [a[0].0] }));
    let kept: Vec<ElementId> = third[0]["kept"].as_array().unwrap().iter().map(|k| id(&k["id"])).collect();
    assert!(kept.contains(&hand) && kept.contains(&made[0]));

    // An 8 m × 4 m × 250 mm wall in one piece: 20 t, past the 16 t band: none, reported.
    run(&r, &mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_weight", "value": 25.0 }));
    let t250 = id(&run(&r, &mut doc, "type.duplicate", json!({ "type_id": t.0, "new_name": "Concrete 250" })));
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t250.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "250 mm" }] }));
    let heavy = id(&run(&r, &mut doc, "wall.create", json!({ "type_id": t250.0, "curve": { "line": { "start": [0.0, 10.0], "end": [8.0, 10.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 4.0 }, "structural": true })));
    let ha = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [heavy.0] })));
    assert_eq!(ha.len(), 1);
    let hr = run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [ha[0].0] }));
    assert!((hr[0]["weight"].as_f64().unwrap() - 20.0).abs() < 1e-6);
    assert!(ids(&hr[0]["created"]).is_empty());
    assert!(hr[0]["warnings"][0].as_str().unwrap().contains("heavier than the last weight band"), "{hr:#}");

    // A 2.4 m × 5 m × 200 mm slab piece (6 t): two anchors on its top face along its length.
    let sq = json!([{ "line": { "start": [0.0, 20.0], "end": [5.0, 20.0] } }, { "line": { "start": [5.0, 20.0], "end": [5.0, 22.4] } }, { "line": { "start": [5.0, 22.4], "end": [0.0, 22.4] } }, { "line": { "start": [0.0, 22.4], "end": [0.0, 20.0] } }]);
    let f = id(&run(&r, &mut doc, "floor.create", json!({ "loops": [sq], "level_id": l1.0 })));
    run(&r, &mut doc, "precast_config.set", json!({ "path": "slab.solid.segmentation.max_width", "value": 6.0 }));
    run(&r, &mut doc, "precast_config.set", json!({ "path": "slab.solid.segmentation.max_weight", "value": 20.0 }));
    let sa = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [f.0] })));
    assert_eq!(sa.len(), 1);
    let sr = run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [sa[0].0] }));
    let sp = ids(&sr[0]["created"]);
    assert_eq!(sp.len(), 2, "{sr:#}");
    let (s0, s1) = (head(&doc, sp[0]), head(&doc, sp[1]));
    let top = reviera_core::floors::regenerate(doc.store(), doc.store().get(f).unwrap()).unwrap().top;
    assert!((s0[2] - top).abs() < 1e-6 && (s0[1] - 21.2).abs() < 1e-6 && ((s0[0] + s1[0]) / 2.0 - 2.5).abs() < 1e-6, "{s0:?} {s1:?}");

    // In IFC: anchors held by the assembly, the weight on it.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'LIFTING_ANCHOR'") && text.contains("'NetWeight'"));
    let guid = doc.store().get(a[0]).unwrap().guid().to_string();
    let step = text.lines().find(|l| l.contains(&format!("('{guid}'"))).unwrap().split('=').next().unwrap().trim().to_owned();
    let rel = text.lines().find(|l| l.contains("IFCRELAGGREGATES(") && l.contains(&format!(",{step},("))).unwrap();
    let anchor = doc.store().get(hand).unwrap().guid().to_string();
    let astep = text.lines().find(|l| l.contains(&format!("('{anchor}'"))).unwrap().split('=').next().unwrap().trim().to_owned();
    assert!(rel.contains(&format!("{astep},")) || rel.contains(&format!("{astep})")), "{rel}");
}
