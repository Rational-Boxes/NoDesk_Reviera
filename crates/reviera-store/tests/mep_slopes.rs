// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sloped piping (MEP-PIPE-002) and parallel pipes (MEP-PIPE-003): 2 % on a 20 m sanitary run
//! from its downstream end with a branch kept connected and Gradient written; a fixture that
//! cannot follow refusing it; parallel pipes of 3 × 1 at 150 mm on a sloped L-run; the refusals.

use reviera_core::mep_runs::segment_recipe;
use reviera_core::mep_slopes::GRADIENT;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(|x| ElementId(x.as_u64().unwrap())).collect()
}

#[test]
fn two_percent_on_a_sanitary_run() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("slope.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // 20 m: 10 along x, an elbow, 10 along y, level at 1 m; a branch tees in at x = 5.
    let main = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 0, 1], [10, 0, 1], [10, 10, 1]], "level": l1.0, "diameter": 0.1, "system_type_id": "sanitary" }));
    let branch = ok(&r, &mut doc, "pipe.create", json!({ "points": [[5, 4, 1], [5, 0, 1]], "level": l1.0, "diameter": 0.05, "system_type_id": "sanitary" }));
    let bseg = *ids(&branch["segments"]).iter().find(|s| segment_recipe(doc.store().get(**s).unwrap()).unwrap().size.diameter == 0.05).unwrap();
    let mut section: Vec<ElementId> = ids(&main["segments"]);
    section.extend(ids(&main["fittings"]));
    section.extend(ids(&branch["fittings"]));
    // The main's half the tee cut off.
    section.extend(ids(&branch["segments"]).into_iter().filter(|s| segment_recipe(doc.store().get(*s).unwrap()).unwrap().size.diameter == 0.1));
    let seg = |doc: &Document, id: ElementId| segment_recipe(doc.store().get(id).unwrap()).unwrap();
    // The upstream end at (0, 0) and the downstream at (10, 10).
    let up = *section.iter().find(|s| doc.store().get(**s).and_then(segment_recipe).is_some_and(|x| x.start[0].abs() < 1e-9 && x.start[1].abs() < 1e-9)).unwrap();
    let down = *section.iter().find(|s| doc.store().get(**s).and_then(segment_recipe).is_some_and(|x| (x.end[1] - 10.0).abs() < 1e-9)).unwrap();
    let z_up = seg(&doc, up).start[2];
    let z_down = seg(&doc, down).end[2];

    let rep = ok(&r, &mut doc, "mep_run.set_slope", json!({ "element_ids": section, "slope": 0.02, "control_point": { "element_id": down.0, "port": 1 } }));
    // The downstream end stays; the upstream end is 0.4 m higher.
    assert!((seg(&doc, down).end[2] - z_down).abs() < 1e-9);
    assert!((seg(&doc, up).start[2] - z_up - 0.4).abs() < 0.002, "{} {rep}", seg(&doc, up).start[2] - z_up);
    // Every main segment slopes 2 %, with its Gradient.
    for s in section.iter().filter(|s| doc.store().get(**s).and_then(segment_recipe).is_some()) {
        let x = seg(&doc, *s);
        assert!((x.slope.abs() - 0.02).abs() < 1e-9, "{x:?}");
        let g = doc.store().get(*s).unwrap().param(GRADIENT).and_then(|v| v["value"].as_f64()).unwrap();
        assert!((g - 0.02).abs() < 1e-9);
    }
    // The branch stayed joined: its end at the tee's port, its own slope recalculated.
    assert!(ids(&rep["branches"]).contains(&bseg), "{rep}");
    let b = seg(&doc, bseg);
    let tee = ids(&branch["fittings"])[0];
    let tee_port = reviera_core::mep::ports(doc.store(), doc.store().get(tee).unwrap()).into_iter().find(|p| (p.position[0] - b.end[0]).abs() < 0.01 && (p.position[1] - b.end[1]).abs() < 0.01).unwrap();
    assert!((tee_port.position[2] - b.end[2]).abs() < 1e-9);
    assert!(reviera_core::mep::network(doc.store(), up).contains(&bseg));
    // Not one run.
    let e = run(&r, &mut doc, "mep_run.set_slope", json!({ "element_ids": [up.0, down.0], "slope": 0.02 })).unwrap_err();
    assert_eq!(e.message, "Selection is not a single connected section");
    // The allowed list; another slope is applied with a warning.
    let list = ok(&r, &mut doc, "mep_settings.slopes", json!({}));
    assert!(list.as_array().unwrap().iter().any(|v| v.as_f64() == Some(0.02)));
    let out = run(&r, &mut doc, "mep_run.set_slope", json!({ "element_ids": section, "slope": 0.03, "control_point": { "element_id": down.0, "port": 1 } })).unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "slope.not_allowed"));

    // A lavatory on the run's upstream end cannot rise with it: refused, naming it.
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Lavatory"] }));
    let lav = ElementId(ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Lavatory"], "point": [30, -0.2], "level": l1.0 })).as_u64().unwrap());
    let waste = reviera_core::mep::ports(doc.store(), doc.store().get(lav).unwrap()).into_iter().find(|p| p.connector.name == "Waste").unwrap();
    let w = waste.position;
    let lz = reviera_core::levels::elevation(doc.store().get(l1).unwrap());
    let fix = ok(&r, &mut doc, "pipe.create", json!({ "points": [[w[0], w[1], w[2] - lz], [w[0], w[1] + 5.0, w[2] - lz], [w[0] + 5.0, w[1] + 5.0, w[2] - lz]], "level": l1.0, "diameter": 0.04 }));
    assert!(ids(&fix["connected_to"]).contains(&lav), "{fix}");
    let mut fs = ids(&fix["segments"]);
    fs.extend(ids(&fix["fittings"]));
    let last = *fs.iter().find(|s| doc.store().get(**s).and_then(segment_recipe).is_some_and(|x| (x.end[0] - w[0] - 5.0).abs() < 1e-9)).unwrap();
    let e = run(&r, &mut doc, "mep_run.set_slope", json!({ "element_ids": fs, "slope": 0.02, "control_point": { "element_id": last.0, "port": 1 } })).unwrap_err();
    assert_eq!(e.code, "slope.branch");
    assert!(e.message.contains(&lav.0.to_string()), "{}", e.message);
}

#[test]
fn parallel_pipes_on_a_sloped_l_run() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("par.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let run_ = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 0], [10, 0], [10, 10]], "level": l1.0, "diameter": 0.05, "slope": 0.01 }));
    let first = ids(&run_["segments"])[0];
    // 3 × 1 at 150 mm: two more L-runs, each with its elbow, concentric.
    let made = ids(&ok(&r, &mut doc, "pipe.create_parallel", json!({ "run_ids": [first.0], "h_count": 3, "h_offset": 0.15 })));
    let segs: Vec<_> = made.iter().filter_map(|m| doc.store().get(*m).and_then(segment_recipe).map(|s| (*m, s))).collect();
    let elbows = made.iter().filter(|m| doc.store().get(**m).and_then(reviera_core::mep_runs::fitting_recipe).is_some_and(|f| f.part == reviera_core::mep_runs::Part::Elbow)).count();
    assert_eq!((segs.len(), elbows), (4, 2), "{made:?}");
    // The copies' first legs lie 150 and 300 mm to the left (+y) of the original's.
    let mut ys: Vec<f64> = segs.iter().filter(|(_, s)| s.start[0].abs() < 1e-6).map(|(_, s)| s.start[1]).collect();
    ys.sort_by(f64::total_cmp);
    assert!(ys.len() == 2 && (ys[0] - 0.15).abs() < 1e-9 && (ys[1] - 0.3).abs() < 1e-9, "{ys:?}");
    // The same 1 % slope.
    for (_, s) in &segs {
        assert!((s.slope - 0.01).abs() < 1e-12, "{s:?}");
        let d = [s.end[0] - s.start[0], s.end[1] - s.start[1], s.end[2] - s.start[2]];
        assert!((d[2].abs() / d[0].hypot(d[1]) - 0.01).abs() < 1e-6, "{d:?}");
    }
    // An offset under the pipe's size is refused.
    let e = run(&r, &mut doc, "pipe.create_parallel", json!({ "run_ids": [first.0], "h_count": 2, "h_offset": 0.04 })).unwrap_err();
    assert!(e.message.starts_with("Offset smaller than pipe outside size"), "{}", e.message);
    // A run with a tee is refused, naming it.
    let br = ok(&r, &mut doc, "pipe.create", json!({ "points": [[5, 4, 2.35], [5, 0, 2.35]], "level": l1.0, "diameter": 0.05 }));
    let tee = ids(&br["fittings"])[0];
    let e = run(&r, &mut doc, "pipe.create_parallel", json!({ "run_ids": [first.0], "h_count": 2, "h_offset": 0.15 })).unwrap_err();
    assert!(e.message.contains("Tee") && e.message.contains(&tee.0.to_string()), "{}", e.message);
}
