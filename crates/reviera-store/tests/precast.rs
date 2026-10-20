// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Precast configuration and splitting (STR-PRE-001, STR-PRE-002): a 30 m × 3 m wall is split
//! into the fewest pieces within 8 m and 12 t, with the configured gaps; no joint lies nearer an
//! opening than the minimum distance; invalid values are refused naming the rule; a configuration
//! exported from one project and imported into another splits an identical wall identically;
//! assemblies made with an older configuration are reported and re-split; a hollow-core floor
//! takes standard widths; each piece exports as an IfcElementAssembly holding its parts.

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

/// A project with a concrete wall type `width` thick, and its level.
fn project(path: &std::path::Path, width: &str) -> (LocalProject, Document, ElementId) {
    let (p, mut doc) = LocalProject::new(Schema::Ifc4, path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let w = id(&run(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 50.0], "end": [1.0, 50.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(&r, &mut doc, "compound.set_layers", json!({ "type_id": t.0, "layers": [{ "function": "structure", "material": "Concrete", "thickness": width }] }));
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [w.0] }));
    (p, doc, l1)
}

fn wall(r: &CommandRegistry, doc: &mut Document, l1: ElementId, y: f64, length: f64) -> ElementId {
    id(&run(
        r,
        doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, y], "end": [length, y] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    ))
}

/// Each assembly's extent along x and weight, in order along the wall.
fn pieces(r: &CommandRegistry, doc: &mut Document, assemblies: &[ElementId]) -> Vec<(f64, f64, f64, String)> {
    let info = run(r, doc, "precast.info", json!({ "assembly_ids": assemblies.iter().map(|a| a.0).collect::<Vec<_>>() }));
    let bodies = reviera_core::bodies::all(doc.store());
    let mut out: Vec<(f64, f64, f64, String)> = info
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let xs: Vec<f64> = a["parts"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|p| {
                    let e = doc.store().get(ElementId(p.as_u64().unwrap())).unwrap();
                    reviera_core::parts::geometry(doc.store(), e, &bodies).into_iter().flat_map(|m| m.0.into_iter().map(|q| q[0]))
                })
                .collect();
            (
                xs.iter().copied().fold(f64::MAX, f64::min),
                xs.iter().copied().fold(f64::MIN, f64::max),
                a["weight"].as_f64().unwrap(),
                a["mark"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

#[test]
fn configuration_and_split() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("precast.ifc");
    let (mut project, mut doc, l1) = project(&path, "250 mm");
    let r = registry();

    // Rules: refused naming the rule.
    let e = r.execute(&mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_length", "value": -1 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "precast_config.invalid");
    assert!(e.message.contains("wall.solid.segmentation.max_length") && e.message.contains("more than 0"), "{}", e.message);
    let e = r.execute(&mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.min_length", "value": 9.0 }), &Caller::user()).unwrap_err();
    assert!(e.message.contains("minimum length must not be more than the maximum"), "{}", e.message);
    let e = r.execute(&mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.joint_gap_vertical", "value": 0.2 }), &Caller::user()).unwrap_err();
    assert!(e.message.contains("0–100 mm"), "{}", e.message);
    let e = r.execute(&mut doc, "precast_config.set", json!({ "path": "wall.solid.nothing", "value": 1 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "precast_config.path");
    assert_eq!(run(&r, &mut doc, "precast_config.get", json!({ "path": "wall.solid.segmentation.max_length" })), json!(8.0));
    assert_eq!(run(&r, &mut doc, "precast_config.get", json!({ "path": "density" })), json!(2.5));

    // A 30 m × 3 m wall, 250 mm thick: 1.875 t a metre, so 12 t is 6.4 m and governs: five pieces.
    let w = wall(&r, &mut doc, l1, 0.0, 30.0);
    let a = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w.0] })));
    assert_eq!(a.len(), 5);
    let p = pieces(&r, &mut doc, &a);
    let gap = 0.02;
    for (k, (x0, x1, weight, mark)) in p.iter().enumerate() {
        assert_eq!(*mark, format!("W1-{}", k + 1));
        assert!(*weight <= 12.0 + 1e-9, "{weight}");
        assert!(x1 - x0 <= 8.0);
        // Even: 6 m less half a gap at each joint.
        let want = 6.0 - gap / 2.0 * if k == 0 || k == 4 { 1.0 } else { 2.0 };
        assert!((x1 - x0 - want).abs() < 1e-6, "piece {k}: {} vs {want}", x1 - x0);
        if k > 0 {
            assert!((x0 - p[k - 1].1 - gap).abs() < 1e-6, "the joint is the gap wide");
        }
    }
    // Four pieces would weigh 7.5 × 3 × 0.25 × 2.5 = 14 t: five is the fewest.
    // Already split: refused, naming the assemblies.
    let e = r.execute(&mut doc, "precast.split", json!({ "element_ids": [w.0] }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "precast.already_split");

    // A thinner limit: 9 t. The assemblies used the older rules; re-split makes them again.
    let v = run(&r, &mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_weight", "value": 9.0 }));
    let aff = run(&r, &mut doc, "precast_config.affected", json!({}));
    assert_eq!(aff["version"], v);
    assert_eq!(ids(&aff["older"]).len(), 5);
    let a = ids(&run(&r, &mut doc, "precast.resplit", json!({ "assembly_ids": [a[0].0] })));
    // 9 t is 4.8 m: seven pieces.
    assert_eq!(a.len(), 7);
    assert!(pieces(&r, &mut doc, &a).iter().all(|p| p.2 <= 9.0 + 1e-9));
    assert!(ids(&run(&r, &mut doc, "precast_config.affected", json!({}))["older"]).is_empty());

    // A 20 m wall with a door: no joint within 200 mm of it.
    let w2 = wall(&r, &mut doc, l1, 10.0, 20.0);
    run(&r, &mut doc, "door.place", json!({ "host_wall_id": w2.0, "point_along": 6.9 }));
    let cut = reviera_core::inserts::cuts_by_host(doc.store()).remove(&w2).unwrap()[0].clone();
    run(&r, &mut doc, "precast_config.set", json!({ "path": "wall.solid.segmentation.max_weight", "value": 20.0 }));
    let a2 = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [w2.0] })));
    assert_eq!(a2.len(), 3, "20 m in pieces of at most 8 m");
    let p2 = pieces(&r, &mut doc, &a2);
    for k in 1..p2.len() {
        let joint = (p2[k - 1].1 + p2[k].0) / 2.0;
        let clear = if joint < cut.x[0] { cut.x[0] - joint } else { joint - cut.x[1] };
        assert!(clear >= 0.2 - 1e-6, "a joint {clear} m from the door ({:?})", cut.x);
    }

    // A 10 m × 6 m hollow-core floor: 1.2 m planks and the rest.
    let f = id(&run(
        &r,
        &mut doc,
        "floor.create",
        json!({ "loops": [[{ "line": { "start": [0.0, 30.0], "end": [10.0, 30.0] } }, { "line": { "start": [10.0, 30.0], "end": [10.0, 36.0] } }, { "line": { "start": [10.0, 36.0], "end": [0.0, 36.0] } }, { "line": { "start": [0.0, 36.0], "end": [0.0, 30.0] } }]], "level_id": l1.0 }),
    ));
    let hc = ids(&run(&r, &mut doc, "precast.split", json!({ "element_ids": [f.0], "slab_kind": "hollow_core" })));
    assert_eq!(hc.len(), 9);
    let ps = pieces(&r, &mut doc, &hc);
    assert!((ps[0].1 - ps[0].0 - (1.2 - 0.01)).abs() < 1e-6, "{:?}", ps[0]);
    assert!((ps[8].1 - ps[8].0 - (0.4 - 0.01)).abs() < 1e-6, "{:?}", ps[8]);
    assert!(ps[0].3.starts_with("S1-"));

    // Exported as assemblies holding their parts.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let assembly_lines: Vec<&str> = text.lines().filter(|l| l.contains("IFCELEMENTASSEMBLY(") && l.contains("'PRECAST_ASSEMBLY'")).collect();
    assert_eq!(assembly_lines.len(), 7 + 3 + 9);
    let step = |l: &str| l.split('=').next().unwrap().trim().to_owned();
    let first = step(assembly_lines[0]);
    let holds = text.lines().filter(|l| l.contains("IFCRELAGGREGATES(") && l.contains(&format!(",{first},("))).count();
    assert_eq!(holds, 1, "{first} aggregates its parts");
    assert!(text.contains("'PieceMark'"));

    // The configuration into another project splits an identical wall identically.
    let file = dir.path().join("rules.json");
    run(&r, &mut doc, "precast_config.export", json!({ "file": file.to_str().unwrap() }));
    let (_p2, mut other, ol1) = self::project(&dir.path().join("other.ifc"), "250 mm");
    let imported = run(&r, &mut other, "precast_config.import", json!({ "file": file.to_str().unwrap() }));
    assert!(imported["missing"].as_array().unwrap().is_empty());
    let ow = wall(&r, &mut other, ol1, 10.0, 20.0);
    run(&r, &mut other, "door.place", json!({ "host_wall_id": ow.0, "point_along": 6.9 }));
    let oa = ids(&run(&r, &mut other, "precast.split", json!({ "element_ids": [ow.0] })));
    let op = pieces(&r, &mut other, &oa);
    let lengths = |p: &[(f64, f64, f64, String)]| p.iter().map(|q| ((q.1 - q.0) * 1e6).round() as i64).collect::<Vec<_>>();
    assert_eq!(lengths(&op), lengths(&p2));
}
