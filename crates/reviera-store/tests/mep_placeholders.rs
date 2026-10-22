// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Placeholders and conversion (MEP-DUCT-007): an L-shaped pair converts to two segments and an
//! elbow at the drawn elevation; a converted placeholder keeps its GlobalId and system; a
//! five-leg junction is left open and listed while every other point converts (an end on a
//! placeholder's side splits it, and converts to a tee); an exported placeholder has an `Axis`
//! representation and no `Body`.

use reviera_core::mep_placeholders::ConvertReport;
use reviera_core::mep_runs::{Part, fitting_recipe, segment_recipe};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema, StepId};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(|x| ElementId(x.as_u64().unwrap())).collect()
}

/// The representation identifiers of an exported element.
fn representations(m: &IfcModel, guid: &str) -> Vec<String> {
    let rows = m.step_rows().unwrap();
    let row = rows.iter().find(|r| r.global_id.as_deref() == Some(guid)).expect("exported");
    let e = m.attributes_many(&[row.id]).unwrap().remove(0);
    let Some(shape) = e.refs("Representation").first().copied() else { return Vec::new() };
    let reps: Vec<StepId> = m.attributes_many(&[shape]).unwrap().remove(0).refs("Representations");
    m.attributes_many(&reps).unwrap().iter().map(|r| r.string("RepresentationIdentifier").unwrap_or_default().to_string()).collect()
}

#[test]
fn placeholders_and_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("placeholders.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // An L-shaped pair at 2.9 m: no fittings, joined at the corner.
    let rep = ok(&r, &mut doc, "duct_placeholder.create", json!({ "points": [[0, 10], [5, 10], [5, 14]], "level": l1.0, "middle_elevation": 2.9, "shape": "rectangular", "width": 0.4, "height": 0.3 }));
    let pair = ids(&rep["segments"]);
    assert_eq!(pair.len(), 2);
    let partners = reviera_core::mep::partners(doc.store());
    assert!(partners.get(&(pair[0], 1)).is_some_and(|v| v.contains(&(pair[1], 0))), "joined at the corner");
    assert!(reviera_core::mep_runs::all(doc.store()).iter().filter(|a| pair.contains(&a.0)).all(|a| a.3.is_empty()), "no body");

    // A placeholder exported: an Axis, no Body.
    project.save(&doc).unwrap();
    let guid = doc.store().get(pair[0]).unwrap().guid().to_string();
    let m = IfcModel::open(&path).unwrap();
    assert_eq!(representations(&m, &guid), vec!["Axis".to_string()]);

    let rep: ConvertReport = serde_json::from_value(ok(&r, &mut doc, "placeholder.convert", json!({ "ids": pair }))).unwrap();
    assert_eq!(rep.segments, pair);
    assert_eq!(rep.fittings.len(), 1);
    assert!(rep.open.is_empty(), "{:?}", rep.open);
    let elbow = fitting_recipe(doc.store().get(rep.fittings[0]).unwrap()).unwrap();
    assert_eq!(elbow.part, Part::Elbow);
    assert!((elbow.centre[0] - 5.0).abs() < 1e-9 && (elbow.centre[1] - 10.0).abs() < 1e-9);
    for id in &pair {
        let s = segment_recipe(doc.store().get(*id).unwrap()).unwrap();
        assert!(!s.placeholder);
        assert!((s.start[2] - 2.9).abs() < 1e-9 && (s.end[2] - 2.9).abs() < 1e-9, "at the original elevation: {s:?}");
    }
    assert_eq!(doc.store().get(pair[0]).unwrap().guid().to_string(), guid, "its GlobalId is kept");
    let partners = reviera_core::mep::partners(doc.store());
    for (id, k) in [(pair[0], 1), (pair[1], 0)] {
        assert!(partners.get(&(id, k)).is_some_and(|v| v[0].0 == rep.fittings[0]), "joined to the elbow");
    }
    // Exported again: a Body now, no Axis left behind.
    project.save(&doc).unwrap();
    assert_eq!(representations(&IfcModel::open(&path).unwrap(), &guid), vec!["Body".to_string()]);

    // A system's placeholders keep their membership: up from a diffuser and across.
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Supply Diffuser 600"] }));
    let diff = ElementId(ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [0, 30], "level": l1.0, "offset": 2.7 })).as_u64().unwrap());
    let sys = ElementId(ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": [diff.0] })).as_u64().unwrap());
    let port = reviera_core::mep::port(doc.store(), diff, 0).unwrap();
    let p = port.position;
    let rep = ok(&r, &mut doc, "duct_placeholder.create", json!({ "points": [p, [p[0], p[1], 3.4], [p[0] + 4.0, p[1], 3.4]], "level": l1.0, "shape": "round", "diameter": 0.25 }));
    let up = ids(&rep["segments"]);
    assert_eq!(rep["system"], json!(sys.0), "joined the diffuser's system: {rep}");
    let member = |doc: &Document, id: ElementId| {
        let s = reviera_core::mep_systems::systems(doc.store()).into_iter().find(|s| s.id == sys).unwrap();
        s.elements.contains(&id)
    };
    assert!(up.iter().all(|id| member(&doc, *id)));
    let guids: Vec<String> = up.iter().map(|id| doc.store().get(*id).unwrap().guid().to_string()).collect();
    let rep: ConvertReport = serde_json::from_value(ok(&r, &mut doc, "placeholder.convert", json!({ "ids": up }))).unwrap();
    assert!(rep.open.is_empty(), "{:?}", rep.open);
    for (id, g) in up.iter().zip(&guids) {
        assert_eq!(&doc.store().get(*id).unwrap().guid().to_string(), g);
        assert!(member(&doc, *id), "still in the system");
    }
    assert!(member(&doc, rep.fittings[0]), "the elbow joined it");
    let partners = reviera_core::mep::partners(doc.store());
    assert!(partners.get(&(up[0], 0)).is_some_and(|v| v[0].0 == diff), "still on the diffuser");

    // Five legs at (20, 0), an L beyond one of them, and a branch ending on a placeholder's side.
    let draw = |doc: &mut Document, pts: Value| ids(&ok(&r, doc, "duct_placeholder.create", json!({ "points": pts, "level": l1.0, "shape": "round", "diameter": 0.3 }))["segments"]);
    let mut all = draw(&mut doc, json!([[20, 0], [25, 0], [25, 4]]));
    for end in [[15.0, 0.0], [20.0, 5.0], [20.0, -5.0], [23.0, 3.0]] {
        all.extend(draw(&mut doc, json!([[20, 0], end])));
    }
    let main = draw(&mut doc, json!([[10, 20], [20, 20]]));
    let branch = draw(&mut doc, json!([[15, 24], [15, 20]]));
    assert_eq!(branch.len(), 2, "the branch and the main's second half: {branch:?}");
    let host = segment_recipe(doc.store().get(main[0]).unwrap()).unwrap();
    assert!((host.end[0] - 15.0).abs() < 1e-9, "split at the branch");
    all.extend(main.iter().chain(&branch));
    let rep: ConvertReport = serde_json::from_value(ok(&r, &mut doc, "placeholder.convert", json!({ "ids": all }))).unwrap();
    assert_eq!(rep.open.len(), 5, "{:?}", rep.open);
    assert!(rep.open.iter().all(|o| (o.at[0] - 20.0).abs() < 1e-9 && o.at[1].abs() < 1e-9 && o.reason.contains("5 legs")));
    let parts: Vec<Part> = rep.fittings.iter().map(|f| fitting_recipe(doc.store().get(*f).unwrap()).unwrap().part).collect();
    assert_eq!(parts, vec![Part::Elbow, Part::Tee], "the L's corner and the branch");
    let partners = reviera_core::mep::partners(doc.store());
    let five: Vec<ElementId> = all[..6].to_vec();
    for o in &rep.open {
        assert!(!partners.contains_key(&(o.element, o.port)), "left open");
    }
    assert!(five.iter().all(|id| !segment_recipe(doc.store().get(*id).unwrap()).unwrap().placeholder));
}

#[test]
fn placeholders_are_left_out_of_sizing_and_justify() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("p2.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A sloped pipe placeholder keeps its slope through conversion.
    let rep = ok(&r, &mut doc, "pipe_placeholder.create", json!({ "points": [[0, 0], [10, 0], [10, 5]], "level": l1.0, "diameter": 0.1, "slope": 0.01 }));
    let pipes = ids(&rep["segments"]);
    let e = r.execute(&mut doc, "mep_run.justify", json!({ "element_ids": pipes, "alignment": "BC" }), &Caller::user()).unwrap_err();
    assert_eq!(&*e.code, "mep_run.justify_placeholder");
    let e = r.execute(&mut doc, "mep_insulation.add", json!({ "element_ids": pipes, "thickness": 0.025 }), &Caller::user());
    assert!(e.is_err() || e.unwrap().warnings.iter().any(|w| w.message.contains("placeholder")));
    let before = segment_recipe(doc.store().get(pipes[1]).unwrap()).unwrap();
    assert!((before.start[2] - (2.4 - 0.1)).abs() < 1e-9, "falls 1 % over 10 m: {before:?}");
    ok(&r, &mut doc, "placeholder.convert", json!({ "ids": pipes }));
    let after = segment_recipe(doc.store().get(pipes[1]).unwrap()).unwrap();
    assert!((after.slope - 0.01).abs() < 1e-12);
    let fall = (after.end[2] - after.start[2]) / (after.end[1] - after.start[1]);
    assert!((fall + 0.01).abs() < 1e-9, "still falls 1 %: {after:?}");
}
