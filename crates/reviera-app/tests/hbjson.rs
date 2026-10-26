// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! HBJSON export end to end (ANA-ENV-009): four rooms, a window and a door exported as a
//! honeybee-schema Model. Room identifiers map back to GlobalIds, every face's vertices run
//! counter-clockwise seen from outside its room, the shared walls are Surface boundaries naming
//! each other, and the constructions keep the U-values.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn pts(face: &Value) -> Vec<[f64; 3]> {
    face["geometry"]["boundary"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()
}

fn normal(p: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn mean(p: &[[f64; 3]]) -> [f64; 3] {
    let k = p.len() as f64;
    [p.iter().map(|q| q[0]).sum::<f64>() / k, p.iter().map(|q| q[1]).sum::<f64>() / k, p.iter().map(|q| q[2]).sum::<f64>() / k]
}

#[test]
fn hbjson_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let mut walls = Vec::new();
    for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
        walls.push(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })));
    }
    let spaces = ok(&mut app, "space.place_auto", json!({ "level_id": level.0 }));
    ok(&mut app, "window.place", json!({ "host_wall_id": walls[0], "point_along": 2.0 }));
    ok(&mut app, "door.place", json!({ "host_wall_id": walls[1], "point_along": 6.0 }));

    let path = dir.path().join("model.hbjson");
    let r = ok(&mut app, "hbjson.export", json!({ "path": path }));
    assert_eq!(r["rooms"], 4, "{r}");
    assert_eq!(r["apertures"], 1, "{r}");
    assert_eq!(r["doors"], 1, "{r}");
    assert_eq!(r["adiabatic"], 0, "{r}");
    let m: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(m["type"], "Model");
    assert_eq!(m["units"], "Meters");
    assert_eq!(m["version"], reviera_core::hbjson::SCHEMA_VERSION);

    // Each room is a space, by GlobalId.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let rooms = m["rooms"].as_array().unwrap();
    for v in spaces.as_array().unwrap() {
        let sid = reviera_core::gbxml::space_id(&store.get(reviera_core::ElementId(v.as_u64().unwrap())).unwrap().guid().to_string());
        assert!(rooms.iter().any(|r| r["identifier"] == sid.as_str()), "{sid}");
    }
    let ident = |s: &str| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let mut faces = std::collections::BTreeMap::new();
    for room in rooms {
        assert!(ident(room["identifier"].as_str().unwrap()));
        assert_eq!(room["story"], "Level 1");
        let fs = room["faces"].as_array().unwrap();
        // A closed box: four walls or more, a floor and a ceiling.
        assert_eq!(fs.iter().filter(|f| f["face_type"] == "Floor").count(), 1);
        assert_eq!(fs.iter().filter(|f| f["face_type"] == "RoofCeiling").count(), 1);
        let all: Vec<[f64; 3]> = fs.iter().flat_map(pts).collect();
        let c = mean(&all);
        for f in fs {
            assert!(ident(f["identifier"].as_str().unwrap()));
            // Counter-clockwise from outside: the normal points away from the room's middle.
            let (p, n) = (pts(f), normal(&pts(f)));
            let m = mean(&p);
            assert!((m[0] - c[0]) * n[0] + (m[1] - c[1]) * n[1] + (m[2] - c[2]) * n[2] > 0.0, "{}", f["identifier"]);
            for sub in f["apertures"].as_array().into_iter().chain(f["doors"].as_array()).flatten() {
                let s = normal(&pts(sub));
                assert!(s[0] * n[0] + s[1] * n[1] + s[2] * n[2] > 0.0, "{}", sub["identifier"]);
            }
            faces.insert(f["identifier"].as_str().unwrap().to_owned(), (room["identifier"].clone(), f.clone()));
        }
    }
    // The crossing partitions: eight interior faces, each naming its twin, which names it back.
    let surface: Vec<_> = faces.values().filter(|(_, f)| f["boundary_condition"]["type"] == "Surface").collect();
    assert_eq!(surface.len(), 8);
    for (room, f) in &surface {
        let objs = f["boundary_condition"]["boundary_condition_objects"].as_array().unwrap();
        let (twin_room, twin) = &faces[objs[0].as_str().unwrap()];
        assert_eq!(&objs[1], twin_room);
        assert_ne!(twin_room, room);
        assert_eq!(twin["boundary_condition"]["boundary_condition_objects"], json!([f["identifier"], room]));
        assert_eq!(twin["properties"]["energy"]["construction"], f["properties"]["energy"]["construction"]);
    }
    assert_eq!(faces.values().filter(|(_, f)| f["boundary_condition"]["type"] == "Outdoors" && f["face_type"] == "Wall").count(), 8);
    assert_eq!(faces.values().filter(|(_, f)| f["boundary_condition"]["type"] == "Ground").count(), 4);

    // Constructions: every reference resolves, and the default exterior wall keeps U 0.28.
    let e = &m["properties"]["energy"];
    let cons: Vec<&str> = e["constructions"].as_array().unwrap().iter().map(|c| c["identifier"].as_str().unwrap()).collect();
    let mats: std::collections::BTreeMap<&str, &Value> = e["materials"].as_array().unwrap().iter().map(|c| (c["identifier"].as_str().unwrap(), c)).collect();
    for (_, f) in faces.values() {
        if let Some(c) = f["properties"]["energy"]["construction"].as_str() {
            assert!(cons.contains(&c), "{c}");
        }
    }
    let wall = e["constructions"].as_array().unwrap().iter().find(|c| c["identifier"] == "ExteriorWall_U0.280").unwrap();
    let r = mats[wall["materials"][0].as_str().unwrap()]["r_value"].as_f64().unwrap();
    assert!((1.0 / (r + 0.17) - 0.28).abs() < 1e-3);
    let glaz = mats.values().find(|m| m["type"] == "EnergyWindowMaterialSimpleGlazSys").unwrap();
    assert!(glaz["u_factor"].as_f64().unwrap() > 0.0 && glaz["shgc"].as_f64().unwrap() > 0.0 && glaz["vt"].as_f64().unwrap() > 0.0);
}
