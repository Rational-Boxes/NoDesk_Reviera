// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Layered wall types (ARC-CMPD-001): layers, validation, the walls' width and IFC material
//! layer sets.

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

#[test]
fn a_layered_wall_type_sets_its_walls_width_and_writes_a_layer_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("l.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let wall = ElementId(
        run(
            &r,
            &mut doc,
            "wall.create",
            json!({
                "curve": { "line": { "start": [0, 0], "end": [5, 0] } },
                "base_level": l1.0, "top_constraint": { "unconnected": 3.0 },
            }),
        )
        .unwrap()
        .as_u64()
        .unwrap(),
    );
    let t = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .unwrap()
        .id();
    // Without layers of its own: one Structure layer of its width.
    let g = run(&r, &mut doc, "compound.get", json!({ "type_id": t.0 })).unwrap();
    assert_eq!(g["layered"], false);
    assert_eq!(g["layers"].as_array().unwrap().len(), 1);

    let layers = json!([
        { "function": "finish1", "material": "Render", "thickness": "20 mm" },
        { "function": "thermal_air", "material": "Mineral Wool", "thickness": "100 mm" },
        { "function": "membrane", "thickness": 0 },
        { "function": "structure", "material": "Concrete Block", "thickness": "190 mm" },
        { "function": "finish2", "material": "Plasterboard", "thickness": "12.5 mm" },
    ]);
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": layers }),
    )
    .unwrap();
    let g = run(&r, &mut doc, "compound.get", json!({ "type_id": t.0 })).unwrap();
    assert_eq!(g["core"], json!([3, 4]));
    assert!((g["thickness"].as_f64().unwrap() - 0.3225).abs() < 1e-9);
    let wg = reviera_core::walls::regenerate(doc.store(), doc.store().get(wall).unwrap()).unwrap();
    assert!((wg.width - 0.3225).abs() < 1e-9);
    // The width is the layers' now.
    let e = run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": t.0, "param": "width", "value": 0.4 }),
    )
    .unwrap_err();
    assert_eq!(e.code, "type.layered");
    // A finish at priority 1 outside the insulation is refused, naming both.
    let mut bad = layers.clone();
    bad[0]["priority"] = json!(1);
    let e = run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": bad }),
    )
    .unwrap_err();
    assert_eq!(e.code, "compound.invalid");
    assert!(e.message.contains("layer 1") && e.message.contains("layer 2"));

    // IFC: one layer set of five layers on the type, one usage on the wall.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCMATERIALLAYERSET"), 1);
    assert_eq!(count("IFCMATERIALLAYER"), 5);
    assert_eq!(count("IFCMATERIALLAYERSETUSAGE"), 1);
    assert_eq!(count("IFCRELASSOCIATESMATERIAL"), 2);
    // Priorities reversed for IFC: Structure 1 → 100, Finish 2 (5) → 60, the membrane 0.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'Structure','Core','LoadBearing',100"));
    assert!(text.contains("'Finish 2',$,'Finish',60"));
    // Saved again unchanged, the material rows are left as they are.
    let material_rows = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| l.contains("IFCMATERIAL") || l.contains("IFCRELASSOCIATESMATERIAL"))
            .map(str::to_owned)
            .collect()
    };
    project.save(&doc).unwrap();
    assert_eq!(
        material_rows(&std::fs::read_to_string(&path).unwrap()),
        material_rows(&text)
    );
    // Thinner insulation: still one set (replaced).
    let mut thinner = layers.clone();
    thinner[1]["thickness"] = json!("80 mm");
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": thinner }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCMATERIALLAYERSET"), 1);
    assert_eq!(count("IFCMATERIALLAYERSETUSAGE"), 1);
    assert_eq!(count("IFCRELASSOCIATESMATERIAL"), 2);
    // Reopened, the layers come back from the type's recipe.
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let g = run(&r, &mut doc2, "compound.get", json!({ "type_id": t.0 })).unwrap();
    assert_eq!(g["layers"][3]["material"], "Concrete Block");
    assert!((g["thickness"].as_f64().unwrap() - 0.3025).abs() < 1e-9);
}
