// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Circuit renumbering (MEP-ELEC-021), the spec's cases: circuits in slots 1, 2, 5 and 9 of a
//! single-phase panel become 1, 2, 3 and 4 and the last is named "LP1-4"; a circuit locked in
//! slot 7 stays there and no other takes it; a 3-pole circuit keeps its three slots; start 40
//! on a 42-slot panel with 5 circuits is refused naming the first that does not fit. Also: no
//! change adds no undo step, and the phase loads balance before and after.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user()).map(|o| o.result)
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn slots(doc: &Document, c: ElementId) -> Vec<u32> {
    reviera_core::mep_electrical::circuits(doc.store()).into_iter().find(|x| x.id == c).unwrap().slots
}

#[test]
fn renumbering() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("renum.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Distribution Board", "Receptacle", "Air Handling Unit"] }));
    let x = std::cell::Cell::new(0.0);
    let place = |doc: &mut Document, ty: &str| {
        x.set(x.get() + 1.0);
        id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t[ty], "point": [x.get(), 0.0], "level": l1.0 })))
    };
    let lp1 = place(&mut doc, "Distribution Board");
    let lp2 = place(&mut doc, "Distribution Board");
    let lp3 = place(&mut doc, "Distribution Board");
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": lp1.0, "name": "LP1", "distribution_system": "230_single" }));
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": lp2.0, "name": "LP2" }));
    ok(&r, &mut doc, "panel.set", json!({ "panel_id": lp3.0, "name": "LP3", "slots": 42 }));
    let circuit_at = |doc: &mut Document, ty: &str, panel: ElementId, slot: u32| {
        let dev = place(doc, ty);
        let c = id(&ok(&r, doc, "circuit.create", json!({ "kind": "power", "element_ids": [dev.0] }))["circuit"]);
        ok(&r, doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": panel.0, "slot": slot }));
        c
    };

    // 1, 2, 5, 9 on the single-phase LP1 close up to 1, 2, 3, 4.
    let one: Vec<ElementId> = [1, 2, 5, 9].iter().map(|s| circuit_at(&mut doc, "Receptacle", lp1, *s)).collect();
    let pre = ok(&r, &mut doc, "circuit.renumber_preview", json!({ "panel_id": lp1.0 }));
    assert_eq!(pre["moves"].as_array().unwrap().iter().filter(|m| m["changed"] == true).count(), 2);
    assert_eq!(slots(&doc, one[2]), vec![5], "a preview changes nothing");
    let out = ok(&r, &mut doc, "circuit.renumber", json!({ "panel_id": lp1.0 }));
    let now: Vec<Vec<u32>> = one.iter().map(|c| slots(&doc, *c)).collect();
    assert_eq!(now, vec![vec![1], vec![2], vec![3], vec![4]]);
    assert_eq!(doc.store().get(one[3]).unwrap().name.as_deref(), Some("LP1-4"));
    assert_eq!(out["moves"][3]["name_before"], "LP1-9");
    // No gaps from 1: nothing changes and no undo step is added.
    let before = doc.version();
    ok(&r, &mut doc, "circuit.renumber", json!({ "panel_id": lp1.0 }));
    assert_eq!(doc.version(), before);
    // Taken and out-of-range slots are refused when choosing one.
    let dev = place(&mut doc, "Receptacle");
    let c = id(&ok(&r, &mut doc, "circuit.create", json!({ "kind": "power", "element_ids": [dev.0] }))["circuit"]);
    assert_eq!(run(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": lp1.0, "slot": 3 })).unwrap_err().code, "panel.slot_taken");
    assert!(run(&r, &mut doc, "circuit.select_panel", json!({ "circuit_id": c.0, "panel_id": lp1.0, "slot": 99 })).is_err());

    // LP2 (three-phase): a circuit locked in 7 stays; the AHU's 3 poles stay together.
    let a = circuit_at(&mut doc, "Receptacle", lp2, 1);
    let b = circuit_at(&mut doc, "Receptacle", lp2, 4);
    let locked = circuit_at(&mut doc, "Receptacle", lp2, 7);
    let ahu = circuit_at(&mut doc, "Air Handling Unit", lp2, 10);
    let d = circuit_at(&mut doc, "Receptacle", lp2, 20);
    let e = circuit_at(&mut doc, "Receptacle", lp2, 25);
    ok(&r, &mut doc, "circuit.set_locked", json!({ "circuit_ids": [locked.0], "locked": true }));
    let out = ok(&r, &mut doc, "circuit.renumber", json!({ "panel_id": lp2.0 }));
    assert_eq!(slots(&doc, locked), vec![7]);
    let ahu_slots = slots(&doc, ahu);
    assert_eq!(ahu_slots.len(), 3);
    assert!(ahu_slots.windows(2).all(|w| w[1] == w[0] + 2), "{ahu_slots:?}");
    let all: Vec<u32> = [a, b, ahu, d, e].iter().flat_map(|c| slots(&doc, *c)).collect();
    assert!(!all.contains(&7), "no other circuit in the locked slot: {all:?}");
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "no slot used twice");
    assert_eq!((slots(&doc, a), slots(&doc, b)), (vec![1], vec![2]));
    // The connected load is the same, wherever it sits.
    let sum = |k: &str| out[k].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).sum::<f64>();
    assert!((sum("phase_before") - sum("phase_after")).abs() < 1e-6);

    // Start 40 on a 42-slot panel holding 5 circuits: refused, naming the fourth.
    let five: Vec<ElementId> = (1..=5).map(|s| circuit_at(&mut doc, "Receptacle", lp3, s)).collect();
    let err = run(&r, &mut doc, "circuit.renumber", json!({ "panel_id": lp3.0, "start": 40 })).unwrap_err();
    assert_eq!(err.code, "circuit.renumber_full");
    assert!(err.message.contains("LP3-4"), "{}", err.message);
    assert_eq!(err.details["circuit"], five[3].0);
    assert_eq!(slots(&doc, five[4]), vec![5], "nothing moved");
}
