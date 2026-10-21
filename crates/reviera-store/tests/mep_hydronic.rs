// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Hydronic network analysis and hydraulic separation (MEP-PIPE-008): a primary chiller loop and
//! a secondary fan coil loop joined by a common pipe between close tees. With two pumps in one
//! loop the network is invalid; with the common pipe a separation, the secondary pump's head
//! excludes the chiller's pressure drop and the primary pump carries the secondary's flow.

use reviera_core::mep_hydronic::HydronicReport;
use reviera_core::mep_runs::segment_recipe;
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn analyze(r: &CommandRegistry, doc: &mut Document) -> HydronicReport {
    serde_json::from_value(ok(r, doc, "hydronic.analyze", json!({}))).unwrap()
}

#[test]
fn primary_and_secondary_loops() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hydronic.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Chiller", "Circulation Pump", "Fan Coil Unit"] }));
    let place = |doc: &mut Document, name: &str, x: f64, y: f64| id(&ok(&r, doc, "mesh_object.place", json!({ "type_id": t[name], "point": [x, y], "level": l1.0 })));
    let chiller = place(&mut doc, "Chiller", 0.0, 0.0);
    let p1 = place(&mut doc, "Circulation Pump", 3.0, 0.0);
    let p2 = place(&mut doc, "Circulation Pump", 7.5, 7.0);
    let fcu = place(&mut doc, "Fan Coil Unit", 10.0, 7.0);
    let z = 0.15;
    let pipe = |doc: &mut Document, pts: &[[f64; 2]]| ok(&r, doc, "pipe.create", json!({ "points": pts.iter().map(|p| [p[0], p[1], z]).collect::<Vec<_>>(), "level": l1.0, "diameter": 0.05 }));
    // Primary: chiller → pump → round the loop → back into the chiller.
    pipe(&mut doc, &[[1.0, 0.0], [2.8, 0.0]]);
    pipe(&mut doc, &[[3.2, 0.0], [10.0, 0.0], [10.0, 4.0], [-2.0, 4.0], [-2.0, 0.0], [-1.0, 0.0]]);
    // Secondary: off the top main by two close tees, through its pump and the fan coil.
    pipe(&mut doc, &[[7.3, 7.0], [6.0, 7.0], [6.0, 4.0]]);
    pipe(&mut doc, &[[7.7, 7.0], [9.5, 7.0]]);
    pipe(&mut doc, &[[10.5, 7.0], [11.0, 7.0], [11.0, 9.0], [5.0, 9.0], [5.0, 4.0]]);
    // The common pipe between the tees.
    let common = doc
        .store()
        .iter()
        .find(|e| {
            segment_recipe(e).is_some_and(|s| {
                let mut xs = [s.start[0], s.end[0]];
                xs.sort_by(f64::total_cmp);
                (s.start[1] - 4.0).abs() < 1e-6 && (s.end[1] - 4.0).abs() < 1e-6 && (xs[0] - 5.0).abs() < 0.2 && (xs[1] - 6.0).abs() < 0.2
            })
        })
        .map(|e| e.id())
        .expect("the common pipe");

    // Off by default.
    assert_eq!(run(&r, &mut doc, "hydronic.analyze", json!({})).unwrap_err().code, "hydronic.off");
    ok(&r, &mut doc, "hydronic.set_enabled", json!({ "enabled": true }));

    // One loop with two pumps: invalid.
    let rep = analyze(&r, &mut doc);
    assert!(rep.loops.is_empty(), "{rep:?}");
    assert!(rep.problems.iter().any(|p| p.contains("one pump per loop")), "{:?}", rep.problems);

    // The common pipe a separation: two loops.
    ok(&r, &mut doc, "hydraulic_separation.add", json!({ "element_id": common.0 }));
    let rep = analyze(&r, &mut doc);
    assert!(rep.problems.is_empty(), "{:?}", rep.problems);
    assert_eq!(rep.loops.len(), 2, "{rep:?}");
    let primary = rep.loops.iter().find(|l| l.pump == p1).unwrap();
    let secondary = rep.loops.iter().find(|l| l.pump == p2).unwrap();
    assert_eq!((primary.role.as_str(), primary.source), ("primary", Some(chiller)));
    assert_eq!((secondary.role.as_str(), secondary.source), ("secondary", None));
    assert_eq!(secondary.critical, Some(fcu));
    // Flows: the fan coil's 0.2 L/s, which the primary pump carries too.
    assert!((secondary.flow - 0.0002).abs() < 1e-12, "{}", secondary.flow);
    assert!((primary.flow - 0.0002).abs() < 1e-12, "{}", primary.flow);
    assert_eq!(primary.critical, Some(common));
    // Heads: the secondary's is the fan coil's 20 kPa and its pipes, without the chiller's
    // 50 kPa; the primary's is the chiller's and its pipes.
    assert!(secondary.head > 20_000.0 && secondary.head < 25_000.0, "{}", secondary.head);
    assert!(primary.head > 50_000.0 && primary.head < 55_000.0, "{}", primary.head);
    assert!(primary.separations.contains(&common) && secondary.separations.contains(&common));

    // Removed: invalid again.
    ok(&r, &mut doc, "hydraulic_separation.remove", json!({ "element_id": common.0 }));
    assert!(analyze(&r, &mut doc).loops.is_empty());
    // Not a pipe: refused.
    assert_eq!(run(&r, &mut doc, "hydraulic_separation.add", json!({ "element_id": l1.0 })).unwrap_err().code, "hydraulic_separation.not_pipe");
}
