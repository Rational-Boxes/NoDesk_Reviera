// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mechanical Settings (MEP-SET-001): a duct size taken off the size lists leaves the list but
//! ducts of that size stay valid; the zero slope cannot be removed; the Angles setting limits
//! run corners; friction equations by domain; pipe segments are exported as
//! IfcPipeSegmentTypes and not read back as elements; fluids and paths.

use reviera_core::mep_mech_settings::SizeRow;
use reviera_core::{Caller, CommandRegistry, Document};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<reviera_core::command::Outcome> {
    r.execute(doc, name, args, &Caller::user())
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

#[test]
fn mechanical_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mech.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    let all = ok(&r, &mut doc, "mechanical_settings.get", json!({}));
    assert_eq!(all["duct"]["air_density"], json!(1.204));
    assert_eq!(all["pipe"]["slopes"][0], json!(0.0), "zero first");
    assert!(run(&r, &mut doc, "mechanical_settings.get", json!({ "path": "duct.nope" })).is_err());

    // A 350 mm side off the size lists: no longer offered, ducts of it still fine.
    let duct = ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 0], [5, 0]], "level": l1.0, "shape": "rectangular", "width": 0.35, "height": 0.3 }));
    let mut rows: Vec<SizeRow> = serde_json::from_value(ok(&r, &mut doc, "duct_size_table.get", json!({ "shape": "rectangular" }))).unwrap();
    rows.iter_mut().filter(|x| (x.size - 0.35).abs() < 1e-9).for_each(|x| x.in_lists = false);
    ok(&r, &mut doc, "duct_size_table.set", json!({ "shape": "rectangular", "sizes": rows }));
    let lists = reviera_core::mep_mech_settings::list_sizes(doc.store(), reviera_core::mep::Shape::Rectangular);
    assert!(!lists.iter().any(|s| (s - 0.35).abs() < 1e-9) && lists.iter().any(|s| (s - 0.4).abs() < 1e-9));
    assert!(reviera_core::mep_mech_settings::sizing_sizes(doc.store(), reviera_core::mep::Shape::Rectangular).iter().any(|s| (s - 0.35).abs() < 1e-9), "still used in sizing");
    let seg = reviera_core::ElementId(duct["segments"][0].as_u64().unwrap());
    assert!((reviera_core::mep_runs::segment_recipe(doc.store().get(seg).unwrap()).unwrap().size.width - 0.35).abs() < 1e-9);
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [seg.0], "width": 0.35, "height": 0.25 }));
    ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 3], [5, 3]], "level": l1.0, "shape": "rectangular", "width": 0.35, "height": 0.3 }));
    assert!(run(&r, &mut doc, "duct_size_table.set", json!({ "shape": "round", "sizes": [{ "size": 0.2 }, { "size": 0.2 }] })).is_err(), "a size twice");

    // The zero slope stays; a steep one is warned about.
    let e = run(&r, &mut doc, "mechanical_settings.set", json!({ "path": "pipe.slopes", "value": [0.01, 0.02] })).unwrap_err();
    assert_eq!(e.message, "The zero slope cannot be removed");
    let o = run(&r, &mut doc, "mechanical_settings.set", json!({ "path": "pipe.slopes", "value": [0.0, 0.01, 2.0] })).unwrap();
    assert!(o.warnings.iter().any(|w| w.message.contains("steeper than 45°")));
    assert_eq!(ok(&r, &mut doc, "mechanical_settings.get", json!({ "path": "pipe.slopes" })), json!([0.0, 0.01, 2.0]));

    // Angles: multiples of 45°; a 60° corner is refused, a 90° one drawn.
    ok(&r, &mut doc, "mechanical_settings.set", json!({ "path": "duct.angles", "value": { "mode": "increment", "step": 45.0 } }));
    let e = run(&r, &mut doc, "duct.create", json!({ "points": [[0, 10], [5, 10], [7.5, 14.33]], "level": l1.0, "shape": "round", "diameter": 0.3 })).unwrap_err();
    assert_eq!(&*e.code, "mep_run.angle");
    ok(&r, &mut doc, "duct.create", json!({ "points": [[0, 10], [5, 10], [5, 14]], "level": l1.0, "shape": "round", "diameter": 0.3 }));
    assert!(run(&r, &mut doc, "mechanical_settings.set", json!({ "path": "pipe.angles", "value": { "mode": "increment", "step": 0 } })).is_err());

    // Friction equations by domain; bad air refused.
    ok(&r, &mut doc, "mechanical_settings.set", json!({ "path": "duct.calculation.friction_equation", "value": "altshul_tsal" }));
    assert_eq!(ok(&r, &mut doc, "mechanical_settings.get", json!({ "path": "pipe.calculation.pressure_drop" })), json!("colebrook"));
    assert_eq!(reviera_core::mep_mech_settings::equation(doc.store(), true), reviera_core::mep_sizing::Equation::AltshulTsal);
    ok(&r, &mut doc, "mechanical_settings.set", json!({ "path": "pipe.calculation.pressure_drop", "value": "haaland" }));
    assert_eq!(reviera_core::mep_mech_settings::equation(doc.store(), false), reviera_core::mep_sizing::Equation::Haaland);
    assert!(run(&r, &mut doc, "mechanical_settings.set", json!({ "path": "duct.air_density", "value": -1 })).is_err());
    ok(&r, &mut doc, "mechanical_settings.set", json!({ "path": "duct.air_density", "value": 1.19 }));
    assert_eq!(reviera_core::mep_mech_settings::air(doc.store()).0, 1.19);

    // A fluid and a pipe segment.
    let g = ok(&r, &mut doc, "fluid.create", json!({ "name": "Propylene Glycol 30%", "table": [{ "temperature": 0, "density": 1040, "viscosity": 0.0045 }, { "temperature": 40, "density": 1020, "viscosity": 0.0018 }] }));
    let (rho, _) = reviera_core::mep_mech_settings::fluid(doc.store(), "Propylene Glycol 30%", 20.0).unwrap();
    assert!((rho - 1030.0).abs() < 1e-9 && g.is_string());
    assert!(run(&r, &mut doc, "fluid.create", json!({ "name": "Bad", "table": [{ "temperature": 10, "density": 1000, "viscosity": 0.001 }, { "temperature": 10, "density": 990, "viscosity": 0.001 }] })).is_err(), "a temperature twice");
    let id = ok(
        &r,
        &mut doc,
        "pipe_segment.create",
        json!({ "material_id": "Copper", "schedule": "Type L", "roughness": 1.5e-6, "sizes": [{ "nominal": 0.015, "inner": 0.0138, "outer": 0.0159 }, { "nominal": 0.02, "inner": 0.0199, "outer": 0.0222, "in_sizing": false }] }),
    );
    assert_eq!(id, json!("copper___type_l"));
    let segs = ok(&r, &mut doc, "mechanical_settings.get", json!({ "path": "pipe.segments" }));
    assert_eq!(segs.as_array().unwrap().len(), 1, "the made segment replaces Generic: {segs}");
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let types: Vec<_> = rows.iter().filter(|r| r.class == "IFCPIPESEGMENTTYPE" && r.attributes.contains("Copper - Type L DN")).collect();
    assert_eq!(types.len(), 2);
    assert!(rows.iter().any(|r| r.class == "IFCPROPERTYSET" && r.attributes.contains("Pset_PipeSegmentTypeCommon")));
    // Saved again: still two; reopened: not elements.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCPIPESEGMENTTYPE" && r.attributes.contains("Copper - Type L DN")).count(), 2);
    let (_p, again, _) = LocalProject::open(&path).unwrap();
    assert!(!again.store().iter().any(|e| e.name.as_deref().is_some_and(|n| n.starts_with("Copper - Type L DN"))), "not read back as elements");
    assert_eq!(reviera_core::mep_mech_settings::get_all(again.store()).pipe.segments.len(), 1, "kept in the settings");
}
