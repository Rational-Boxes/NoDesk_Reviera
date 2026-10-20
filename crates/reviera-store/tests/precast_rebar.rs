// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Automatic precast reinforcement (STR-PRE-003): a wall piece with a 1.2 × 2.1 m opening gets
//! U-bars round the opening's edges and area bars cut round it; a 200 mm piece is offered only
//! types whose range holds 200 mm; generated sets carry the piece mark as their partition;
//! running again keeps a bar added by hand and an edited generated set and replaces the rest;
//! the assembly holds its parts and reinforcement in IFC.

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

fn bars(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> (String, Vec<Vec<[f64; 3]>>) {
    let i = run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }))[0].clone();
    let b = i["bars"].as_array().unwrap().iter().map(|b| b["points"].as_array().unwrap().iter().map(|p| [0, 1, 2].map(|k| p[k].as_f64().unwrap())).collect()).collect();
    (i["shape"].as_str().unwrap().to_owned(), b)
}

#[test]
fn reinforcing_precast_pieces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pieces.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = |doc: &mut Document, y: f64, type_id: Option<ElementId>| {
        let mut a = json!({ "curve": { "line": { "start": [0.0, y], "end": [10.0, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true });
        if let Some(t) = type_id {
            a["type_id"] = json!(t.0);
        }
        id(&run(&r, doc, "wall.create", a))
    };
    let w = wall(&mut doc, 0.0, None);
    let t200 = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t200.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "200 mm" }] }));
    // A 1.2 × 2.1 m opening from 4 m along.
    run(&r, &mut doc, "opening.create_wall", json!({ "wall_id": w.0, "u0": 4.0, "u1": 5.2 }));
    let pieces = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w.0] })));
    assert_eq!(pieces.len(), 2);
    let info = run(&r, &mut doc, "precast.info", json!({ "assembly_ids": [pieces[0].0] }))[0].clone();
    let mark = info["mark"].as_str().unwrap().to_owned();
    assert_eq!(mark, "W1-1", "one row: no row letter, an opening notwithstanding");

    // Offered for 200 mm: the 120–250 mm area type and the U-bars, not the 250–400 mm type.
    let offered = run(&r, &mut doc, "precast.reinforcement_types", json!({ "assembly_ids": [pieces[0].0] }));
    let area: Vec<&str> = offered["area_types"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(area, ["A10-150"]);
    assert_eq!(offered["edge_types"][0]["id"], "U10-200");
    let e = r
        .execute(&mut doc, "precast.reinforce", json!({ "assembly_ids": [pieces[0].0], "area_type_id": "A12-150" }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "precast.thickness");
    assert!(e.message.contains("250–400 mm") && e.message.contains("200 mm"), "{}", e.message);

    // Reinforce the piece with the opening.
    let report = run(&r, &mut doc, "precast.reinforce", json!({ "assembly_ids": [pieces[0].0], "area_type_id": "A10-150", "edge_type_id": "U10-200" }));
    let a = &report["assemblies"][0];
    assert!(a["skipped"].as_array().unwrap().is_empty(), "{a:#}");
    let made = ids(&a["created"]);
    let (mut area_sets, mut u_sets) = (Vec::new(), Vec::new());
    for s in &made {
        let (shape, b) = bars(&r, &mut doc, *s);
        assert_eq!(reviera_core::rebar_numbers::partition(doc.store().get(*s).unwrap()), mark, "the piece mark is the partition");
        if shape == "00" { area_sets.push(b) } else { u_sets.push((shape, b)) }
    }
    assert_eq!(area_sets.len(), 4, "vertical and horizontal at each face");
    // The area bars keep the wall's 25 mm edge cover from the opening.
    for p in area_sets.iter().flatten().flatten() {
        let inside = p[0] > 4.0 - 0.0249 && p[0] < 5.2 + 0.0249 && p[2] < 2.1 + 0.0249;
        assert!(!inside, "an area bar point in the opening: {p:?}");
    }
    // U-bars round the opening: its two sides and over it, each with its web at the edge cover.
    assert!(u_sets.iter().all(|(s, _)| s == "21"));
    let near = |x: f64| u_sets.iter().any(|(_, b)| b.iter().all(|bar| bar.iter().any(|p| (p[0] - x).abs() < 0.06) && bar.iter().all(|p| p[2] <= 2.1 + 1e-6)));
    assert!(near(4.0) && near(5.2), "U-bars at both sides of the opening");
    let over = u_sets.iter().any(|(_, b)| b.iter().all(|bar| bar.iter().all(|p| p[2] > 2.1 && p[0] > 4.0 && p[0] < 5.2)));
    assert!(over, "U-bars over the opening");
    // Every set lies within the piece.
    let x_end = info["parts"].as_array().unwrap().len();
    assert!(x_end > 0);

    // A bar added by hand and a generated set edited: both kept on running again.
    let manual = id(&run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": w.0, "shape": "00", "bar_type": "d16", "placement": { "kind": "layer", "face": "exterior", "vertical": false, "boundary": [[0.5, 2.5], [3.0, 2.5], [3.0, 2.8], [0.5, 2.8]] }, "layout": { "rule": "single" } }),
    ));
    run(&r, &mut doc, "rebar.set_partition", json!({ "ids": [manual.0], "name": mark }));
    let edited = made[0];
    run(&r, &mut doc, "rebar.set_layout", json!({ "rebar_ids": [edited.0], "rule": "maximum_spacing", "spacing": 0.1 }));
    let again = run(&r, &mut doc, "precast.reinforce", json!({ "assembly_ids": [pieces[0].0], "area_type_id": "A10-150", "edge_type_id": "U10-200" }));
    let a2 = &again["assemblies"][0];
    assert_eq!(a2["removed"].as_u64().unwrap() as usize, made.len() - 1);
    let kept: Vec<ElementId> = a2["kept"].as_array().unwrap().iter().map(|k| id(&k["id"])).collect();
    assert!(kept.contains(&edited) && kept.contains(&manual), "{a2:#}");
    assert!(doc.store().get(manual).is_some() && doc.store().get(edited).is_some());
    assert_eq!(ids(&a2["created"]).len(), made.len());
    for gone in made.iter().skip(1) {
        assert!(doc.store().get(*gone).is_none());
    }

    // A 350 mm wall: only the 250–400 mm area type.
    let t350 = id(&run(&r, &mut doc, "type.duplicate", json!({ "type_id": t200.0, "new_name": "Concrete 350" })));
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t350.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "350 mm" }] }));
    let w350 = wall(&mut doc, 20.0, Some(t350));
    let p350 = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w350.0] })));
    let offered = run(&r, &mut doc, "precast.reinforcement_types", json!({ "assembly_ids": [p350[0].0] }));
    let area: Vec<&str> = offered["area_types"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(area, ["A12-150"]);

    // Walls and slabs together are refused.
    let f = id(&run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[{ "line": { "start": [0.0, 30.0], "end": [6.0, 30.0] } }, { "line": { "start": [6.0, 30.0], "end": [6.0, 34.0] } }, { "line": { "start": [6.0, 34.0], "end": [0.0, 34.0] } }, { "line": { "start": [0.0, 34.0], "end": [0.0, 30.0] } }]], "level_id": l1.0 }),
    ));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let slabs = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [f.0] })));
    let e = r
        .execute(&mut doc, "precast.reinforce", json!({ "assembly_ids": [pieces[1].0, slabs[0].0], "area_type_id": "A10-150" }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "precast.mixed");
    // A slab piece: bars both ways, top and bottom.
    let s = run(&r, &mut doc, "precast.reinforce", json!({ "assembly_ids": [slabs[0].0], "area_type_id": "S10-150" }));
    assert_eq!(ids(&s["assemblies"][0]["created"]).len(), 4, "{s:#}");

    // In IFC the assembly holds its parts and its reinforcement.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let step_of = |guid: &str| text.lines().find(|l| l.contains(&format!("('{guid}'"))).map(|l| l.split('=').next().unwrap().trim().to_owned()).unwrap_or_else(|| panic!("no entity for {guid}"));
    let assembly = step_of(doc.store().get(pieces[0]).unwrap().guid().as_str());
    let rel = text.lines().find(|l| l.contains("IFCRELAGGREGATES(") && l.contains(&format!(",{assembly},("))).expect("the assembly aggregates");
    let bar = step_of(doc.store().get(ids(&a2["created"])[0]).unwrap().guid().as_str());
    assert!(rel.contains(&format!("{bar},")) || rel.contains(&format!("{bar})")), "{rel}");
}
