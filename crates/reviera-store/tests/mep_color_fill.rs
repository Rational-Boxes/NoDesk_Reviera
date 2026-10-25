// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe colour fill legends (MEP-DUCT-010): a velocity scheme with bands at 4 and 8 m/s
//! colours each duct segment in the band of its velocity; a second duct legend in the view is
//! refused, a pipe legend accepted; deleting the legend removes the duct colouring from that
//! view and no other; with Calculations = None the segments take the No value colour; default
//! bands; parameters checked.

use std::collections::HashMap;

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
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

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn fills(doc: &Document, view: ElementId) -> HashMap<ElementId, [f32; 4]> {
    reviera_core::mep_color_fill::fills(doc.store(), doc.store().get(view).unwrap())
}

const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

#[test]
fn duct_and_pipe_legends() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("fill.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A supply system: three diffusers of 0.35 m³/s on 250 mm branches off a 400 mm main.
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
    ok(&r, &mut doc, "connector.set", json!({ "element_id": t["Supply Diffuser 600"], "connector_index": 0, "flow_config": "preset", "flow": 0.35 }));
    let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [0, 0], "level": l1.0 })));
    let xs = [6.0, 9.0, 12.0];
    let diffusers: Vec<u64> = xs.iter().map(|x| ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [x, 3.0], "level": l1.0, "offset": 2.7 })).as_u64().unwrap()).collect();
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": diffusers })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
    ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 0, 0.9], [3, 0, 0.9], [3, 0, 3.6], [14, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
    for x in xs {
        ok(&r, &mut doc, "duct.create", json!({ "points": [[x, 0, 3.6], [x, 3, 3.6], [x, 3, 2.85]], "level": l1.0, "shape": "round", "diameter": 0.25, "system_type_id": "supply_air" }));
    }
    ok(&r, &mut doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
    let views = ok(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }));
    let plan = id(&views[0]);
    let other = id(&ok(&r, &mut doc, "view.duplicate", json!({ "view_id": plan.0 })));

    // Velocity in bands at 4 and 8 m/s.
    let scheme = ok(&r, &mut doc, "colorscheme.create", json!({ "name": "Duct Velocity", "category": "cat.ducts", "parameter": "Velocity", "mode": "range", "ranges": [0, 4, 8] }));
    ok(&r, &mut doc, "colorscheme.set_rows", json!({ "scheme_id": scheme, "include_fittings": false, "rows": [
        { "at_least": 0, "color": "#0000ff" }, { "at_least": 4, "color": "#00ff00" }, { "at_least": 8, "color": "#ff0000" }
    ] }));
    ok(&r, &mut doc, "mep_color_legend.place", json!({ "view_id": plan.0, "kind": "duct", "scheme_id": "Duct Velocity", "point": [16, 4] }));
    ok(&r, &mut doc, "mep_color_legend.place", json!({ "view_id": other.0, "kind": "duct", "scheme_id": scheme, "point": [16, 4] }));
    let f = fills(&doc, plan);
    let segments: Vec<ElementId> = doc.store().iter().filter(|e| reviera_core::mep_runs::segment_recipe(e).is_some()).map(|e| e.id()).collect();
    let (mut bands, mut seen) = (0, std::collections::HashSet::new());
    for s in &segments {
        let e = doc.store().get(*s).unwrap();
        let Some(v) = reviera_core::mep_network::kept_value(e, "velocity").and_then(Value::as_f64) else {
            panic!("segment {s} {:?} has no velocity: {:?}", reviera_core::mep_runs::segment_recipe(e).map(|r| (r.start, r.end)), e.param("rxd:mep_analysis"));
        };
        let want = if v >= 8.0 { RED } else if v >= 4.0 { GREEN } else { BLUE };
        assert_eq!(f.get(s), Some(&want), "segment {s} at {v} m/s");
        if seen.insert(format!("{want:?}")) {
            bands += 1;
        }
    }
    assert!(bands >= 2, "the main and the branches fall in different bands");
    assert!(f.keys().all(|k| segments.contains(k)), "fittings left out");

    // One duct legend per view; a pipe legend is accepted.
    let e = run(&r, &mut doc, "mep_color_legend.place", json!({ "view_id": plan.0, "kind": "duct", "scheme_id": scheme, "point": [16, 8] })).unwrap_err();
    assert_eq!(e.code, "mep_color_legend.exists");
    let pipe = ok(&r, &mut doc, "colorscheme.create", json!({ "name": "Pipe Temperature", "category": "cat.pipes", "parameter": "Fluid Temperature" }));
    assert!(run(&r, &mut doc, "mep_color_legend.place", json!({ "view_id": plan.0, "kind": "pipe", "scheme_id": scheme, "point": [16, 8] })).is_err(), "a duct scheme for a pipe legend");
    ok(&r, &mut doc, "mep_color_legend.place", json!({ "view_id": plan.0, "kind": "pipe", "scheme_id": pipe, "point": [16, 8] }));
    assert!(run(&r, &mut doc, "colorscheme.create", json!({ "name": "Bad", "category": "cat.ducts", "parameter": "Slope" })).is_err(), "slope is for pipes");

    // Deleting the plan's duct legend removes its fill there only.
    let legend = reviera_core::mep_color_fill::legends(doc.store(), doc.store().get(plan).unwrap()).into_iter().find(|(_, l)| l.mep == reviera_core::mep_color_fill::Kind::Duct).unwrap().0;
    ok(&r, &mut doc, "element.delete", json!({ "element_ids": [legend.0] }));
    assert!(fills(&doc, plan).is_empty());
    assert_eq!(fills(&doc, other).len(), segments.len());
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(fills(&doc, plan).len(), segments.len());

    // Calculations = None: no calculated value, so the No value colour.
    ok(&r, &mut doc, "colorscheme.set_rows", json!({ "scheme_id": scheme, "no_value_color": "#808080", "rows": [
        { "at_least": 0, "color": "#0000ff" }, { "at_least": 4, "color": "#00ff00" }, { "at_least": 8, "color": "#ff0000" }
    ] }));
    ok(&r, &mut doc, "system_type.set", json!({ "type_id": "supply_air", "calculations": "none" }));
    let grey = reviera_core::color_schemes::rgba("#808080").unwrap();
    assert!(fills(&doc, plan).values().all(|c| *c == grey));

    // Default bands: five equal ones over the sizes found.
    let size = ok(&r, &mut doc, "colorscheme.create", json!({ "name": "Duct Size", "category": "cat.ducts", "parameter": "Size", "mode": "range" }));
    let rows = ok(&r, &mut doc, "colorscheme.legend", json!({ "scheme_id": size }));
    let lows: Vec<f64> = rows.as_array().unwrap().iter().map(|r| r["at_least"].as_f64().unwrap()).collect();
    assert_eq!(lows, vec![250.0, 280.0, 310.0, 340.0, 370.0]);
}
