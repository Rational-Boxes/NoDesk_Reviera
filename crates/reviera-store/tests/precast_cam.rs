// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! CAM export (STR-PRE-007): the neutral JSON of a wall piece with one opening lists its outline,
//! the opening, every bar with its shape and the lifting anchors in the piece's own system; the
//! rule `{Name:12:4:0:left}_{ProdNo}` names the files; `{Nme}_{ProdNo}` is refused with the
//! position before anything is written; the export leaves the undo stack as it was.

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

#[test]
fn neutral_export_and_filename_rules() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("cam.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 5 m × 3 m × 200 mm wall with a 1.2 × 2.1 m opening from 2 m along: one piece.
    let w = id(&run(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [10.0, 0.0], "end": [15.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true })));
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": "200 mm" }] }));
    run(&r, &mut doc, "opening.create_wall", json!({ "wall_id": w.0, "u0": 2.0, "u1": 3.2 }));
    let a = id(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w.0] }))[0]);
    run(&r, &mut doc, "precast.reinforce", json!({ "assembly_ids": [a.0], "area_type_id": "A10-150", "edge_type_id": "U10-200" }));
    run(&r, &mut doc, "precast.place_mounting_parts", json!({ "assembly_ids": [a.0] }));

    let writers = run(&r, &mut doc, "cam_writer.list", json!({}));
    let ids: Vec<&str> = writers.as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["neutral_json", "neutral_csv"]);

    let undo_before = doc.undo_history().count();
    let out = dir.path().join("cam");
    let rep = run(&r, &mut doc, "precast.cam_export", json!({ "assembly_ids": [a.0], "writer_id": "neutral_json", "folder": out.to_str().unwrap(), "filename_rule": "{Name:12:4:0:left}_{ProdNo}" }));
    assert_eq!(doc.undo_history().count(), undo_before, "the export changes nothing");
    let file = rep["files"][0]["path"].as_str().unwrap();
    assert!(file.ends_with("W1-1_1.json"), "{file}");
    let piece: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    // The outline and the opening, in millimetres from the front face's lower-left corner.
    assert_eq!(piece["outline"], json!([[0.0, 0.0], [5000.0, 0.0], [5000.0, 3000.0], [0.0, 3000.0]]));
    let o = &piece["openings"][0];
    let near = |v: &Value, want: [f64; 2]| (v[0].as_f64().unwrap() - want[0]).abs() < 1e-6 && (v[1].as_f64().unwrap() - want[1]).abs() < 1e-6;
    assert!(near(&o[0], [2000.0, 0.0]) && near(&o[2], [3200.0, 2100.0]), "{o}");
    // Every bar set with its shape and bars inside the piece.
    let bars = piece["bars"].as_array().unwrap();
    assert!(bars.len() >= 4);
    for s in bars {
        assert!(["00", "21"].contains(&s["shape"].as_str().unwrap()), "{}", s["shape"]);
        for b in s["bars"].as_array().unwrap() {
            for p in b.as_array().unwrap() {
                let (x, y, z) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap());
                assert!((-1e-6..=5000.0 + 1e-6).contains(&x) && (-1e-6..=3000.0 + 1e-6).contains(&y) && (-1e-6..=200.0 + 1e-6).contains(&z), "{p}");
            }
        }
    }
    // The two lifting anchors at the top edge, in the middle of the thickness.
    let embeds = piece["embeds"].as_array().unwrap();
    assert_eq!(embeds.len(), 2);
    for e in embeds {
        assert_eq!(e["kind"], "lifting_anchor");
        assert!((e["position"][1].as_f64().unwrap() - 3000.0).abs() < 1e-6 && (e["position"][2].as_f64().unwrap() - 100.0).abs() < 1e-6, "{e}");
    }
    assert_eq!(piece["mark"], "W1-1");

    // Zero padding to the minimum, left-aligned; the production number after.
    let rep = run(&r, &mut doc, "precast.cam_export", json!({ "writer_id": "neutral_csv", "folder": out.to_str().unwrap(), "filename_rule": "{Name:12:6:0:left}_{ProdNo}" }));
    assert!(rep["files"][0]["path"].as_str().unwrap().ends_with("W1-100_1.csv"), "{rep}");
    let csv = std::fs::read_to_string(rep["files"][0]["path"].as_str().unwrap()).unwrap();
    assert!(csv.starts_with("piece,W1-1,W1-1,1,solid_wall,5000,3000,200,"), "{csv}");
    assert!(csv.contains("\nembed,lifting_anchor,"));

    // An external writer named by a manifest in the data folder: here `cat`, which hands the
    // neutral document back.
    let data = dir.path().join("precast-data");
    std::fs::create_dir_all(data.join("cam_writers")).unwrap();
    std::fs::write(
        data.join("cam_writers").join("echo.json"),
        r#"{ "id": "echo", "name": "Echo", "version": "1.0", "license": "MIT", "extension": "txt", "command": ["cat"] }"#,
    )
    .unwrap();
    run(&r, &mut doc, "precast_config.set", json!({ "path": "data_folder", "value": data.to_str().unwrap() }));
    let writers = run(&r, &mut doc, "cam_writer.list", json!({}));
    assert_eq!(writers[2]["id"], "echo");
    let rep = run(&r, &mut doc, "precast.cam_export", json!({ "writer_id": "echo", "folder": out.to_str().unwrap() }));
    let echoed: Value = serde_json::from_str(&std::fs::read_to_string(rep["files"][0]["path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(echoed["mark"], "W1-1");
    assert!(rep["files"][0]["path"].as_str().unwrap().ends_with("W1-1.txt"), "the configuration's {{Mark}} rule");

    // A misspelt variable: refused with its position, nothing written.
    let empty = dir.path().join("none");
    let e = r
        .execute(&mut doc, "precast.cam_export", json!({ "writer_id": "neutral_json", "folder": empty.to_str().unwrap(), "filename_rule": "{Nme}_{ProdNo}" }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "cam.filename_rule");
    assert_eq!(e.details["position"], 1);
    assert!(e.message.contains("Nme") && e.message.contains("character 2"), "{}", e.message);
    assert!(!empty.exists());
}
