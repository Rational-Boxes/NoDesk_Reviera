// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Precast shop drawings (STR-PRE-006): 10 pieces of which 4 are identical, grouped, give 7
//! sheets; each has dimensioned views, a centre-of-gravity symbol and a bill of materials; without
//! grouping, one sheet per piece; running again replaces the sheets.

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

fn texts(doc: &Document, sheet: ElementId) -> Vec<String> {
    let d = reviera_core::sheets::sheet_data(doc.store().get(sheet).unwrap()).unwrap();
    d.shop_drawings.iter().flat_map(|i| reviera_core::precast_shop::sheet_graphics(doc.store(), i).unwrap().0.texts.into_iter().map(|t| t.2)).collect()
}

#[test]
fn shop_drawings_group_identical_pieces() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("shop.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = |doc: &mut Document, y: f64, l: f64| {
        id(&run(&r, doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, y], "end": [l, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true })))
    };
    let mut walls = Vec::new();
    // Four identical 5 m walls and six others, every one a single piece.
    for k in 0..4 {
        walls.push(wall(&mut doc, k as f64 * 5.0, 5.0));
    }
    for (k, l) in [3.0, 3.5, 4.0, 4.5, 6.0, 6.5].iter().enumerate() {
        walls.push(wall(&mut doc, 30.0 + k as f64 * 5.0, *l));
    }
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "200 mm" }] }));
    let pieces = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": walls.iter().map(|w| w.0).collect::<Vec<_>>() })));
    assert_eq!(pieces.len(), 10);
    // The identical ones reinforced and anchored alike.
    let same: Vec<u64> = pieces[..4].iter().map(|p| p.0).collect();
    run(&r, &mut doc, "precast.reinforce", json!({ "assembly_ids": same, "area_type_id": "A10-150", "edge_type_id": "U10-200" }));
    run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": pieces.iter().map(|p| p.0).collect::<Vec<_>>() }));

    let all: Vec<u64> = pieces.iter().map(|p| p.0).collect();
    let sheets = ids(&run(&r, &mut doc, "precast.shop_drawings", json!({ "assembly_ids": all, "group_identical": true })));
    assert_eq!(sheets.len(), 7, "4 identical pieces share one sheet");
    // The shared sheet names its four marks.
    let shared = sheets
        .iter()
        .find(|s| reviera_core::sheets::sheet_data(doc.store().get(**s).unwrap()).unwrap().shop_drawings[0].assemblies.len() == 4)
        .copied()
        .expect("a sheet for the four");
    let tx = texts(&doc, shared);
    assert!(tx.iter().any(|t| t.contains("x 4:")), "{tx:?}");
    // Dimensioned: the 5 m length and 3 m height; the centre of gravity; the bill of materials.
    assert!(tx.iter().any(|t| t == "5000") && tx.iter().any(|t| t == "3000"), "{tx:?}");
    assert!(tx.iter().any(|t| t.starts_with("CoG 2500 / 1500")), "{tx:?}");
    assert!(tx.iter().any(|t| t == "Bill of materials") && tx.iter().any(|t| t == "Concrete") && tx.iter().any(|t| t == "Rebar") && tx.iter().any(|t| t == "Mounting part"), "{tx:?}");
    assert!(tx.iter().any(|t| t.contains("Front view 1 : 25")), "5 m in 240 mm: 1:25: {tx:?}");
    for s in &sheets {
        let tx = texts(&doc, *s);
        assert!(tx.iter().any(|t| t.starts_with("CoG")) && tx.iter().any(|t| t == "Bill of materials"));
    }
    assert_eq!(reviera_core::sheets::sheet_data(doc.store().get(shared).unwrap()).unwrap().size, "A3");

    // One sheet per piece without grouping; the earlier sheets are replaced.
    let each = ids(&run(&r, &mut doc, "precast.shop_drawings", json!({ "assembly_ids": pieces.iter().map(|p| p.0).collect::<Vec<_>>(), "group_identical": false })));
    assert_eq!(each.len(), 10);
    assert!(sheets.iter().all(|s| doc.store().get(*s).is_none()));
}
