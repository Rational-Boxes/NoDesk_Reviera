// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stairs that reach their top and open the floor over them (ARC-STAIR-007), vertical openings
//! (ARC-OPEN-002) and the model's building code (DM-CODE-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        { "line": { "start": [x0, y0], "end": [x1, y0] } },
        { "line": { "start": [x1, y0], "end": [x1, y1] } },
        { "line": { "start": [x1, y1], "end": [x0, y1] } },
        { "line": { "start": [x0, y1], "end": [x0, y0] } },
    ])
}

/// Level 1 at 0, Level 2 at 3 m with a 10 × 10 m floor (300 mm thick, top at the level), and a
/// stair from Level 1 whose single run is drawn too short.
fn model(r: &CommandRegistry, doc: &mut Document) -> (ElementId, ElementId, ElementId) {
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(r, doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let floor = id(run(
        r,
        doc,
        "floor.create",
        json!({ "loops": [square(0.0, 0.0, 10.0, 10.0)], "level_id": l2.0 }),
    )
    .unwrap());
    let stair = id(run(
        r,
        doc,
        "stair.create",
        json!({ "base_level": l1.0, "runs": [{ "start": [2.0, 5.0], "end": [4.0, 5.0] }] }),
    )
    .unwrap());
    (l2, floor, stair)
}

fn floor_area(doc: &Document, floor: ElementId) -> f64 {
    let store = doc.store();
    let e = store.get(floor).unwrap();
    let g = reviera_core::floors::regenerate(store, e).unwrap();
    reviera_core::slab_openings::cut_profiles(store, e, &g.profiles)
        .iter()
        .map(reviera_core::sketch::Profile::area)
        .sum()
}

/// The farthest the stair's railings reach along +x.
fn railing_reach(doc: &Document) -> f64 {
    reviera_core::railings::all(doc.store())
        .iter()
        .flat_map(|(_, g)| g.parts.iter().flat_map(|m| m.0.iter().map(|p| p[0])))
        .fold(f64::MIN, f64::max)
}

#[test]
fn a_short_run_extends_to_the_top_and_its_railings_follow() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("s.ifc")).unwrap();
    let r = registry();
    let (l2, _, stair) = model(&r, &mut doc);
    // 3 m at 180 mm is 17 risers: the run goes on to 16 treads of 280 mm (to x = 6.48).
    let g = reviera_core::stairs::regenerate(doc.store(), doc.store().get(stair).unwrap()).unwrap();
    assert_eq!((g.risers, g.placed), (17, 17));
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    let before = railing_reach(&doc);
    assert!(before > 6.3, "{before}");
    // Raising Level 2 adds risers; the run and its railings extend.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 3.6 }),
    )
    .unwrap();
    let g = reviera_core::stairs::regenerate(doc.store(), doc.store().get(stair).unwrap()).unwrap();
    assert_eq!((g.risers, g.placed), (20, 20));
    let after = railing_reach(&doc);
    assert!(
        (after - before - 3.0 * 0.28).abs() < 0.05,
        "{before} → {after}"
    );
}

#[test]
fn the_floor_over_a_stair_opens_for_the_codes_headroom() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let (_, floor, stair) = model(&r, &mut doc);

    // The model's code is the IBC: 2032 mm under a slab whose underside is at 2.7 m.
    let code = run(&r, &mut doc, "code.get", json!({})).unwrap();
    assert_eq!(code["settings"]["code"], "ibc");
    let ibc = run(
        &r,
        &mut doc,
        "stair.headroom_check",
        json!({ "stair_id": stair.0 }),
    )
    .unwrap();
    assert!((ibc["headroom"].as_f64().unwrap() - 2.032).abs() < 1e-9);
    let o = &ibc["obstructions"][0];
    assert_eq!(o["host"], floor.0);
    let ibc_area = o["area"].as_f64().unwrap();
    // Short where the next nosing is above 2.7 − 2.032 = 0.668 m: from the 3rd tread (the 4th
    // riser's nosing, 0.706 m) to the top, 1.2 m wide: 14 treads.
    assert!((ibc_area - 14.0 * 0.28 * 1.2).abs() < 0.02, "{ibc_area}");
    assert!(o["clearance"].as_f64().unwrap() < 2.032);

    // Another code for this stair only: Canada's 1950 mm in a dwelling opens a little less.
    let nbc = run(
        &r,
        &mut doc,
        "stair.headroom_check",
        json!({ "stair_id": stair.0, "code": "nbc_canada" }),
    )
    .unwrap();
    let nbc_area = nbc["obstructions"][0]["area"].as_f64().unwrap();
    assert!(nbc_area < ibc_area, "{nbc_area} vs {ibc_area}");
    assert!(nbc["clause"].as_str().unwrap().contains("9.8.2.2"));

    // Opening it: the floor loses the region, and the check finds nothing left to open.
    let made = run(
        &r,
        &mut doc,
        "stair.create_openings",
        json!({ "stair_id": stair.0 }),
    )
    .unwrap();
    let opening = id(made[0].clone());
    assert!((floor_area(&doc, floor) - (100.0 - ibc_area)).abs() < 0.02);
    let again = run(
        &r,
        &mut doc,
        "stair.headroom_check",
        json!({ "stair_id": stair.0 }),
    )
    .unwrap();
    assert!(again["obstructions"][0]["uncovered"].as_f64().unwrap() < 1e-4);
    // Running it again reuses the opening.
    let made = run(
        &r,
        &mut doc,
        "stair.create_openings",
        json!({ "stair_id": stair.0 }),
    )
    .unwrap();
    assert_eq!(id(made[0].clone()), opening);

    // The grips' command: a wider opening.
    run(
        &r,
        &mut doc,
        "opening.set_loops",
        json!({ "opening_id": opening.0, "loops": [[[2.5, 4.0], [7.0, 4.0], [7.0, 6.0], [2.5, 6.0]]] }),
    )
    .unwrap();
    assert!((floor_area(&doc, floor) - (100.0 - 4.5 * 2.0)).abs() < 1e-6);
    let e = run(
        &r,
        &mut doc,
        "opening.set_loops",
        json!({ "opening_id": opening.0, "loops": [[[20.0, 20.0], [21.0, 20.0], [21.0, 21.0]]] }),
    )
    .unwrap_err();
    assert_eq!(e.code, "opening.misses_host");

    // IFC: the opening voids the floor, whose own body stays whole.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCRELVOIDSELEMENT")
            .count(),
        1
    );

    // The model's own code setting.
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" })).unwrap();
    let uk = run(
        &r,
        &mut doc,
        "stair.headroom_check",
        json!({ "stair_id": stair.0 }),
    )
    .unwrap();
    assert!((uk["headroom"].as_f64().unwrap() - 2.0).abs() < 1e-9);

    // Deleting the floor deletes its opening.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [floor.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert!(doc.store().get(opening).is_none());
}

#[test]
fn a_vertical_opening_at_the_edge_notches_the_floor_and_moves_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("s.ifc")).unwrap();
    let r = registry();
    let (_, floor, _) = model(&r, &mut doc);
    let opening = id(run(
        &r,
        &mut doc,
        "opening.create_vertical",
        json!({ "host_id": floor.0, "loops": [[[-1.0, 2.0], [1.0, 2.0], [1.0, 3.0], [-1.0, 3.0]]] }),
    )
    .unwrap());
    assert!((floor_area(&doc, floor) - 99.0).abs() < 1e-6);
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [floor.0], "vector": [2.0, 0.0, 0.0] }),
    )
    .unwrap();
    let rec =
        reviera_core::slab_openings::opening_recipe(doc.store().get(opening).unwrap()).unwrap();
    assert_eq!(rec.loops[0][0], [1.0, 2.0]);
    assert!((floor_area(&doc, floor) - 99.0).abs() < 1e-6);
}
