// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roof drainage (MEP-PIPE-010): a gutter round a hip roof's four eaves, hung on the fascias
//! and mitred; an outlet with a downspout to the ground; the fall toward it; the gutter
//! following the overhang; sizing; and the IFC.

use reviera_core::mep_gutters::{geometry, gutter_recipe};
use reviera_core::mep_runs::segment_recipe;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

const RECT: [[f64; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];

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

#[test]
fn a_gutter_round_a_hip_roof() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gutter.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": RECT[(i + 1) % 4], "end": RECT[i] } }, "base_level": level.0, "top_constraint": { "unconnected": 3 } }))));
    }
    let faces: Vec<Value> = (0..4)
        .map(|i| {
            let (a, b) = (RECT[i], RECT[(i + 1) % 4]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l = d[0].hypot(d[1]);
            let out = [d[1] / l * 0.1, -d[0] / l * 0.1];
            json!({ "line": { "start": [a[0] + out[0], a[1] + out[1]], "end": [b[0] + out[0], b[1] + out[1]] }, "lock": { "element": walls[i].0, "sub": "face:ext" } })
        })
        .collect();
    let roof = id(&ok(&r, &mut doc, "roof.create_footprint", json!({ "curves": faces, "level_id": level.0, "base_offset": 3.0, "slope": std::f64::consts::FRAC_PI_4 })));
    ok(&r, &mut doc, "roof.set_eaves", json!({ "roof_id": roof.0, "overhang": 0.5, "fascia": true }));

    // Along all four eaves: one run, closed round the roof, set out from the fascias.
    let gutter = id(&ok(&r, &mut doc, "gutter.create", json!({ "roof_id": roof.0 })));
    let geo = |doc: &Document| geometry(doc.store(), &gutter_recipe(doc.store().get(gutter).unwrap()).unwrap()).unwrap();
    let g = geo(&doc);
    assert_eq!(g.line.len(), 5, "{:?}", g.line);
    let (first, last) = (g.line[0], g.line[4]);
    assert!((first[0] - last[0]).abs() < 1e-9 && (first[1] - last[1]).abs() < 1e-9, "closed round the roof");
    // The eave line is 500 mm out from the walls' exterior faces (at 0.1 out); the fascia's
    // 25 mm and half the 125 mm gutter beyond it.
    let out = 0.1 + 0.5 + 0.025 + 0.0625;
    let xs: Vec<f64> = g.line.iter().map(|p| p[0]).collect();
    assert!(xs.iter().any(|x| (x + out).abs() < 1e-6) && xs.iter().any(|x| (x - 10.0 - out).abs() < 1e-6), "{xs:?}");
    // Level until it has an outlet, just under the eave (2.5 m).
    assert!(g.line.iter().all(|p| (p[2] - 2.48).abs() < 1e-9), "{:?}", g.line);
    assert!(!g.solids[0].points.is_empty());

    // An outlet near the south-west corner with a downspout to the level.
    let o = ok(&r, &mut doc, "gutter.add_outlet", json!({ "gutter_id": gutter.0, "point": [0.5, -1.0], "downspout_to": { "level": level.0 } }));
    let at = o["at"].as_f64().unwrap();
    let down: Vec<ElementId> = o["downspout"].as_array().unwrap().iter().map(id).collect();
    assert!(!down.is_empty());
    let g = geo(&doc);
    // The gutter falls toward the outlet: lowest there, by 1:500 of the longest way to it.
    let low = g.line.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let high = g.line.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!((high - 2.48).abs() < 1e-9 && high - low > 0.01, "{low} {high}");
    // The downspout: vertical runs, a leg back toward the wall by the eave's offset, down to 0.
    let segs: Vec<_> = down.iter().filter_map(|e| segment_recipe(doc.store().get(*e).unwrap())).collect();
    let bottom = segs.iter().map(|s| s.start[2].min(s.end[2])).fold(f64::INFINITY, f64::min);
    assert!(bottom.abs() < 1e-9, "{bottom}");
    let vertical = segs.iter().filter(|s| (s.start[0] - s.end[0]).abs() < 1e-6 && (s.start[1] - s.end[1]).abs() < 1e-6).count();
    let sloped = segs.len() - vertical;
    assert!(vertical >= 2 && sloped >= 1, "{segs:?}");
    // It ends against the wall: its foot just outside the exterior face.
    let foot = segs.iter().find(|s| s.start[2].min(s.end[2]).abs() < 1e-9).unwrap();
    let fx = foot.start[0].min(foot.end[0]);
    let fy = foot.start[1].min(foot.end[1]);
    assert!(fx < -0.1 || fy < -0.1, "outside the wall: {fx} {fy}");
    assert!(fx > -0.3 || fy > -0.3, "near the wall: {fx} {fy}");
    // One storm system with gutter, outlet and downspout, connected.
    let sys = ok(&r, &mut doc, "system.list", json!({}));
    let s = sys.as_array().unwrap().iter().find(|s| s["classification"] == "storm_drainage").expect("a storm system");
    let members: Vec<u64> = s["elements"].as_array().unwrap().iter().map(|e| e.as_u64().unwrap()).collect();
    assert!(members.contains(&gutter.0) && members.contains(&o["fitting"].as_u64().unwrap()) && down.iter().all(|d| members.contains(&d.0)), "{s}");
    assert!(reviera_core::mep::network(doc.store(), gutter).contains(&down[0]));

    // A larger overhang moves the gutter with the eave.
    ok(&r, &mut doc, "roof.set_eaves", json!({ "roof_id": roof.0, "overhang": 0.7 }));
    let g2 = geo(&doc);
    assert!(g2.line.iter().any(|p| (p[0] + out + 0.2).abs() < 1e-6), "{:?}", g2.line);
    let _ = at;

    // Sizing at 75 mm/h: the whole roof's plan area to the one outlet.
    let rep = ok(&r, &mut doc, "gutter.size", json!({ "gutter_id": gutter.0 }));
    let area = rep["area"].as_f64().unwrap();
    assert!((area - 11.6 * 7.6).abs() < 0.5, "{rep}");
    let flow = rep["outlets"][0]["flow"].as_f64().unwrap();
    assert!((flow - area * 75.0 / 3600.0).abs() < 1e-6);
    // About 1.8 l/s: more than a 68 mm downspout or a 125 mm gutter at 1:500 takes.
    assert_eq!(rep["outlets"][0]["ok"], false);
    assert!(!rep["warnings"].as_array().unwrap().is_empty());
    ok(&r, &mut doc, "gutter.set", json!({ "gutter_id": gutter.0, "width": 0.2, "depth": 0.12, "fall": 0.005 }));
    let rep = ok(&r, &mut doc, "gutter.size", json!({ "gutter_id": gutter.0, "rainfall": 50 }));
    assert!(rep["outlets"][0]["gutter_capacity"].as_f64().unwrap() > rep["outlets"][0]["flow"].as_f64().unwrap(), "{rep}");

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let seg = rows.iter().find(|r| r.class == "IFCPIPESEGMENT" && r.attributes.contains(".GUTTER.")).expect("the gutter");
    assert!(rows.iter().any(|r| r.class == "IFCRELCONNECTSELEMENTS" && r.attributes.contains(&format!("#{})", seg.id.0))));
    assert!(rows.iter().any(|r| r.class == "IFCPIPEFITTING" && r.attributes.contains(".CONNECTOR.")));
    assert!(rows.iter().any(|r| r.class == "IFCDISTRIBUTIONSYSTEM" && r.attributes.contains(".STORMWATER.")));
}
