// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Automatic penetrations (MEP-PEN-001): a 100 mm pipe through a wall gets a 150 mm round
//! opening that follows it, a riser opens the floor, a duct through a beam is flagged only, the
//! rules, and the IFC openings (made, moved and removed with their runs).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn span(outline: &Value, k: usize) -> f64 {
    let v: Vec<f64> = outline.as_array().unwrap().iter().map(|p| p[k].as_f64().unwrap()).collect();
    v.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - v.iter().cloned().fold(f64::INFINITY, f64::min)
}

#[test]
fn openings_follow_their_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pen.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    // A 100 mm pipe through it, square, at 1.5 m.
    let pipe = ok(&r, &mut doc, "pipe.create", json!({ "points": [[2.0, -2.0, 1.5], [2.0, 2.0, 1.5]], "level": l1.0, "diameter": 0.1 }));
    let seg = id(&pipe["segments"][0]);
    let x = ok(&r, &mut doc, "mep.penetrations", json!({}));
    let w: Vec<&Value> = x.as_array().unwrap().iter().filter(|c| c["host"] == wall.0).collect();
    assert_eq!(w.len(), 1, "{x}");
    assert_eq!((w[0]["mode"].as_str(), w[0]["host_kind"].as_str()), (Some("create"), Some("wall")));
    // 100 mm + 2 × 25 mm clearance, round.
    assert!((span(&w[0]["outline"], 0) - 0.15).abs() < 1e-9 && (span(&w[0]["outline"], 1) - 0.15).abs() < 1e-9, "{}", w[0]);
    assert_eq!(w[0]["outline"].as_array().unwrap().len(), 24);
    let cut = |doc: &Document| reviera_core::inserts::cuts_by_host(doc.store()).remove(&wall).unwrap_or_default();
    let c0 = cut(&doc);
    assert_eq!(c0.len(), 1);
    assert!(((c0[0].x[0] + c0[0].x[1]) / 2.0 - 2.0).abs() < 1e-9);

    // Saved, the wall is voided by a Penetration opening.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let opening = rows.iter().find(|r| r.class == "IFCOPENINGELEMENT" && r.attributes.contains("'Penetration'")).expect("an opening");
    assert!(rows.iter().any(|r| r.class == "IFCRELVOIDSELEMENT" && r.attributes.contains(&format!("#{})", opening.id.0))));

    // The pipe moves 500 mm along the wall: so does the opening (the same IFC entity).
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [seg.0], "vector": [0.5, 0.0, 0.0] }));
    let c1 = cut(&doc);
    assert!(((c1[0].x[0] + c1[0].x[1]) / 2.0 - 2.5).abs() < 1e-9, "{c1:?}");
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let pens: Vec<_> = rows.iter().filter(|r| r.class == "IFCOPENINGELEMENT" && r.attributes.contains("'Penetration'")).collect();
    assert_eq!(pens.len(), 1);
    assert_eq!(pens[0].id, opening.id);

    // A riser through a floor opens the floor.
    let c = [[3.0, 2.0], [6.0, 2.0], [6.0, 5.0], [3.0, 5.0]];
    let edges: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    let floor = id(&ok(&r, &mut doc, "floor.create", json!({ "loops": [edges], "level_id": l1.0 })));
    ok(&r, &mut doc, "pipe.create", json!({ "points": [[4.0, 3.0, 1.0], [4.0, 3.0, -1.0]], "level": l1.0, "diameter": 0.05 }));
    let fe = doc.store().get(floor).unwrap().clone();
    let g = reviera_core::floors::regenerate(doc.store(), &fe).unwrap();
    let profiles = reviera_core::slab_openings::cut_profiles(doc.store(), &fe, &g.profiles);
    assert_eq!(profiles.iter().map(|p| p.holes.len()).sum::<usize>(), 1, "{profiles:?}");

    // A duct through a beam is flagged, not opened.
    let beam = id(&ok(&r, &mut doc, "beam.create", json!({ "curve": { "start": [0.0, 10.0], "end": [5.0, 10.0] }, "reference_level": l1.0 })));
    let bz = reviera_core::beams::all(doc.store()).into_iter().find(|(b, _)| *b == beam).unwrap().1;
    let (m, _) = bz.mesh();
    let mid_z = (m.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min) + m.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max)) / 2.0;
    ok(&r, &mut doc, "duct.create", json!({ "points": [[2.0, 8.0, mid_z], [2.0, 12.0, mid_z]], "level": l1.0, "shape": "round", "diameter": 0.1 }));
    let x = ok(&r, &mut doc, "mep.penetrations", json!({}));
    let b: Vec<&Value> = x.as_array().unwrap().iter().filter(|c| c["host"] == beam.0).collect();
    assert_eq!(b.len(), 1, "{x}");
    assert_eq!((b[0]["mode"].as_str(), b[0]["outline"].as_array().map(Vec::len)), (Some("flag"), Some(0)));
    assert_eq!(run(&r, &mut doc, "mep.set_penetration_rule", json!({ "host_category": "framing", "mode": "create" })).unwrap_err().code, "mep.penetration_framing");

    // Rules: walls never opened for domestic cold water; a 50 mm clearance for everything else.
    ok(&r, &mut doc, "mep.set_penetration_rule", json!({ "host_category": "wall", "mode": "create", "clearance": 0.05 }));
    let x = ok(&r, &mut doc, "mep.penetrations", json!({ "run_ids": [seg.0] }));
    assert!((span(&x[0]["outline"], 0) - 0.2).abs() < 1e-9);
    ok(&r, &mut doc, "mep.set_penetration_rule", json!({ "host_category": "wall", "mode": "never" }));
    assert!(cut(&doc).is_empty());

    // The pipe deleted (and the rule back), its opening goes from the IFC too.
    ok(&r, &mut doc, "mep.set_penetration_rule", json!({ "host_category": "wall", "mode": "create" }));
    ok(&r, &mut doc, "element.delete", json!({ "element_ids": [seg.0] }));
    assert!(cut(&doc).is_empty());
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let pens = rows.iter().filter(|r| r.class == "IFCOPENINGELEMENT" && r.attributes.contains("'Penetration'")).count();
    // Only the floor's remains.
    assert_eq!(pens, 1);
}
