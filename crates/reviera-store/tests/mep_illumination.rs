// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Average estimated illumination (ANA-LIGHT-001), the spec's cases: a 6 × 5 m space with six
//! 3 000 lm fixtures, CU 0.6 and LLF 0.8 gives 288 lx; with the workplane at 0.76 m and the
//! fixtures at 3.0 m, RCR ≈ 4.11; moving one fixture next door changes both spaces at once; a
//! fixture type with no CU table is excluded with "no CU data". Also CU interpolation, the
//! space's LLF override, and the target illuminance written to and read from IFC.

use reviera_core::mep_illumination::CuTable;
use reviera_core::{Caller, CommandRegistry, Document};
use reviera_ifc::{IfcModel, Schema};
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

fn calc(r: &CommandRegistry, doc: &mut Document, space: &Value) -> Value {
    ok(r, doc, "lighting.calc_average", json!({ "space_ids": [space] }))[0].clone()
}

#[test]
fn cu_interpolation() {
    let t = CuTable {
        rcr: vec![0.0, 2.0],
        columns: vec![[0.8, 0.5, 0.2], [0.8, 0.3, 0.2], [0.5, 0.5, 0.2], [0.5, 0.3, 0.2]],
        values: vec![vec![1.0, 0.9, 0.8, 0.7], vec![0.6, 0.5, 0.4, 0.3]],
    };
    assert!((t.cu(1.0, [0.65, 0.4, 0.2]) - 0.65).abs() < 1e-12);
    // Clamped at the edges: RCR above the last row, reflectances beyond the columns.
    assert!((t.cu(5.0, [0.9, 0.7, 0.1]) - 0.6).abs() < 1e-12);
    assert!((t.cu(0.0, [0.5, 0.3, 0.2]) - 0.7).abs() < 1e-12);
}

#[test]
fn lumen_method() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lux.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // Two 6 × 5 m spaces side by side, bounded by separation lines (no wall thickness).
    let lines: Vec<Value> = [([0, 0], [12, 0]), ([12, 0], [12, 5]), ([12, 5], [0, 5]), ([0, 5], [0, 0]), ([6, 0], [6, 5])]
        .iter()
        .map(|(a, b)| json!({ "start": a, "end": b }))
        .collect();
    ok(&r, &mut doc, "roomseparator.create", json!({ "level_id": l1.0, "lines": lines }));
    let a = ok(&r, &mut doc, "space.place", json!({ "level_id": l1.0, "point": [3, 2.5] }));
    let b = ok(&r, &mut doc, "space.place", json!({ "level_id": l1.0, "point": [9, 2.5] }));

    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Ceiling Luminaire", "Wall Luminaire"] }));
    let lum = t["Ceiling Luminaire"].clone();
    ok(&r, &mut doc, "lighting_fixture_type.set_lighting", json!({
        "type_id": lum, "lumens": 3000.0,
        "cu_table": { "rcr": [0, 10], "columns": [[0.8, 0.5, 0.2]], "values": [[0.6], [0.6]] }
    }));
    let mut fixtures = Vec::new();
    for x in [1.0, 3.0, 5.0] {
        for y in [1.5, 3.5] {
            fixtures.push(ok(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": lum, "point": [x, y], "level": l1.0, "offset": 3.0, "work_plane": true })));
        }
    }
    let res = calc(&r, &mut doc, &a);
    assert_eq!(res["fixtures"], 6);
    assert!((res["illuminance"].as_f64().unwrap() - 288.0).abs() < 1e-6, "{res}");
    assert!((res["rcr"].as_f64().unwrap() - 5.0 * 2.24 * 11.0 / 30.0).abs() < 1e-6, "{res}");
    assert_eq!(calc(&r, &mut doc, &b)["illuminance"], 0.0, "no fixtures: 0 lx");

    // Moving one fixture into the next space changes both, with no command to recalculate.
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [fixtures[5]], "vector": [6.0, 0.0, 0.0] }));
    assert!((calc(&r, &mut doc, &a)["illuminance"].as_f64().unwrap() - 240.0).abs() < 1e-6);
    assert!((calc(&r, &mut doc, &b)["illuminance"].as_f64().unwrap() - 48.0).abs() < 1e-6);
    // space.info reports it too.
    let info = &ok(&r, &mut doc, "space.info", json!({ "ids": [a] }))[0];
    assert!((info["illuminance"].as_f64().unwrap() - 240.0).abs() < 1e-6);

    // A type with no CU table: excluded, with the reason.
    let wall_lum = t["Wall Luminaire"].clone();
    ok(&r, &mut doc, "lighting_fixture_type.set_lighting", json!({ "type_id": wall_lum, "lumens": 1000.0 }));
    let w = ok(&r, &mut doc, "lighting_fixture.place", json!({ "type_id": wall_lum, "point": [3.0, 2.5], "level": l1.0, "offset": 2.2, "work_plane": true }));
    let res = calc(&r, &mut doc, &a);
    assert_eq!(res["excluded"], json!([{ "fixture": w, "reason": "no CU data" }]));
    assert_eq!(res["fixtures"], 5);

    // The space's own light loss factor; a target, met or not.
    ok(&r, &mut doc, "space.set", json!({ "ids": [a], "light_loss_factor": 1.0, "target_illuminance": 320 }));
    let res = calc(&r, &mut doc, &a);
    assert!((res["illuminance"].as_f64().unwrap() - 300.0).abs() < 1e-6, "{res}");
    assert_eq!((res["target"].as_f64(), res["meets_target"].as_bool()), (Some(320.0), Some(false)));
    ok(&r, &mut doc, "space.set", json!({ "ids": [a], "target_illuminance": 300 }));
    assert_eq!(calc(&r, &mut doc, &a)["meets_target"], true);
    assert!(run(&r, &mut doc, "space.set", json!({ "ids": [a], "light_loss_factor": 1.5 })).is_err());
    assert!(run(&r, &mut doc, "lighting_fixture_type.set_lighting", json!({ "type_id": lum, "cu_table": { "rcr": [0], "columns": [[0.8, 0.5, 0.2]], "values": [[0.6]] } })).is_err());

    // The target is an ordinary IFC property: written and read back.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCPROPERTYSET" && r.attributes.contains("'Pset_SpaceLightingRequirements'")));
    assert!(rows.iter().any(|r| r.attributes.contains("'Illuminance'") && r.attributes.contains("IFCILLUMINANCEMEASURE(300.")));
    let (_, mut doc2, _) = LocalProject::open(&path).unwrap();
    let res = calc(&r, &mut doc2, &a);
    assert_eq!((res["target"].as_f64(), res["meets_target"].as_bool()), (Some(300.0), Some(true)));
    assert!((res["illuminance"].as_f64().unwrap() - 300.0).abs() < 1e-6, "the type's data survives: {res}");
}
