// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Pipe fittings and accessories (MEP-PIPE-004): a gate valve inserted in a 65 mm pipe splits it
//! and gets flanges from the union role; an elbow placed mid-pipe is refused and one on an end
//! connects; Cap Open Ends caps three ends at their sizes; a tee becomes a cross; a run with a
//! valve is refused for parallel pipes, naming it; the IFC.

use reviera_core::mep_runs::{Part, fitting_recipe, segment_recipe};
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

#[test]
fn valves_fittings_caps_and_crosses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fit.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A gate valve in a 65 mm pipe: the pipe is split, flanges on both sides.
    let pipe = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 0], [6, 0]], "level": l1.0, "diameter": 0.065, "system_type_id": "domestic_cold_water" }));
    let seg = ids(&pipe["segments"])[0];
    let v = ok(&r, &mut doc, "pipe_accessory.place", json!({ "kind": "gate_valve", "pipe_id": seg.0 }));
    let valve = id(&v["accessory"]);
    let e = doc.store().get(valve).unwrap();
    assert_eq!((e.class.as_str(), e.name.as_deref(), e.param("@PredefinedType").and_then(Value::as_str)), ("IfcValve", Some("Gate Valve"), Some("ISOLATING")));
    assert_eq!(fitting_recipe(e).unwrap().legs[0].size.diameter, 0.065);
    let flanges = ids(&v["flanges"]);
    assert_eq!(flanges.len(), 2);
    assert!(flanges.iter().all(|f| fitting_recipe(doc.store().get(*f).unwrap()).unwrap().part == Part::Flange));
    let second = id(&v["split"]);
    // One connected run: pipe, flange, valve, flange, pipe; the two halves meet nowhere else.
    let net = reviera_core::mep::network(doc.store(), seg);
    for x in [valve, second, flanges[0], flanges[1]] {
        assert!(net.contains(&x), "{x:?} {net:?}");
    }
    assert!(reviera_core::mep::open_ports(doc.store(), &[reviera_core::mep::Domain::Pipe]).iter().all(|p| ![valve, flanges[0], flanges[1]].contains(&p.element)));
    let (a, b) = (segment_recipe(doc.store().get(seg).unwrap()).unwrap(), segment_recipe(doc.store().get(second).unwrap()).unwrap());
    assert!(a.end[0] < 3.0 && b.start[0] > 3.0);
    // Threaded below 50 mm: no flanges.
    let small = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 3], [4, 3]], "level": l1.0, "diameter": 0.025 }));
    let c = ok(&r, &mut doc, "pipe_accessory.place", json!({ "kind": "check_valve", "pipe_id": ids(&small["segments"])[0].0 }));
    assert!(ids(&c["flanges"]).is_empty());
    assert_eq!(doc.store().get(id(&c["accessory"])).unwrap().param("@PredefinedType").and_then(Value::as_str), Some("CHECK"));

    // An elbow mid-pipe is refused; on the open end it connects.
    let e = run(&r, &mut doc, "pipe_fitting.place", json!({ "part": "elbow", "host_id": second.0, "point": [5.0, 0.0] })).unwrap_err();
    assert_eq!(e.message, "Elbows must be placed at an open end");
    let el = ok(&r, &mut doc, "pipe_fitting.place", json!({ "part": "elbow", "point": [6.0, 0.0] }));
    let elbow = id(&el["fitting"]);
    assert_eq!(fitting_recipe(doc.store().get(elbow).unwrap()).unwrap().part, Part::Elbow);
    assert!(reviera_core::mep::network(doc.store(), second).contains(&elbow));

    // A run with three open ends: Cap Open Ends adds three caps of their sizes.
    let main = ok(&r, &mut doc, "pipe.create", json!({ "points": [[0, 10], [6, 10]], "level": l1.0, "diameter": 0.065 }));
    let branch = ok(&r, &mut doc, "pipe.create", json!({ "points": [[3, 14], [3, 10]], "level": l1.0, "diameter": 0.05 }));
    let tee = ids(&branch["fittings"])[0];
    let mut runs: Vec<ElementId> = ids(&main["segments"]);
    runs.extend(ids(&branch["segments"]));
    let caps = ids(&ok(&r, &mut doc, "pipe.cap_open_ends", json!({ "element_ids": runs })));
    assert_eq!(caps.len(), 3);
    let mut sizes: Vec<f64> = caps.iter().map(|c| fitting_recipe(doc.store().get(*c).unwrap()).unwrap().legs[0].size.diameter).collect();
    sizes.sort_by(f64::total_cmp);
    assert_eq!(sizes, vec![0.05, 0.065, 0.065]);

    // Tee to cross: a fourth, open leg of the run's size.
    ok(&r, &mut doc, "mep_fitting.tee_to_cross", json!({ "fitting_id": tee.0 }));
    let f = fitting_recipe(doc.store().get(tee).unwrap()).unwrap();
    assert_eq!((f.part, f.legs.len(), f.legs[3].size.diameter), (Part::Cross, 4, 0.065));
    assert!(reviera_core::mep::open_ports(doc.store(), &[]).iter().any(|p| p.element == tee && p.index == 3));

    // Parallel pipes refuse the run with the valve, naming it.
    let e = run(&r, &mut doc, "pipe.create_parallel", json!({ "run_ids": [seg.0], "h_count": 2, "h_offset": 0.2 })).unwrap_err();
    assert!(e.message.contains("Gate Valve") && e.message.contains(&valve.0.to_string()), "{}", e.message);

    // IFC.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCVALVE" && r.attributes.contains(".ISOLATING.")));
    assert!(rows.iter().filter(|r| r.class == "IFCPIPEFITTING" && r.attributes.contains(".CONNECTOR.")).count() >= 2);
}
