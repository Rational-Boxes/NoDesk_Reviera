// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Area, path and fabric reinforcement (STR-RBR-010, 011, 012): a 6 × 4 m slab region with top
//! and bottom × major and minor layers at 200 mm makes four layers cut around a 1 × 1 m opening,
//! and *Remove Area System* in real-rebar mode leaves four rebar sets; a 5 m path at 150 mm with
//! alternating bars offset 300 mm alternates them; a fabric area over a slab with an opening
//! clips its sheets by the boundary and the opening with the set laps, and staggering moves every
//! second row's laps half a sheet; all of it is written to IFC and read back.

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

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn ring(a: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    vec![a, [b[0], a[1]], b, [a[0], b[1]]]
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = ring(a, b);
    Value::Array((0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect())
}

fn sys(r: &CommandRegistry, doc: &mut Document, s: ElementId) -> Value {
    run(r, doc, "reinforcement_system.info", json!({ "system_ids": [s.0] }))[0].clone()
}

fn pts(bar: &Value) -> Vec<[f64; 3]> {
    bar["points"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()
}

/// Whether a segment from `a` to `b` passes through the box (x0, y0)–(x1, y1) in plan.
fn crosses(a: [f64; 3], b: [f64; 3], lo: [f64; 2], hi: [f64; 2]) -> bool {
    (0..=100).any(|k| {
        let t = k as f64 / 100.0;
        let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        x > lo[0] + 1e-6 && x < hi[0] - 1e-6 && y > lo[1] + 1e-6 && y < hi[1] - 1e-6
    })
}

#[test]
fn area_path_and_fabric_reinforcement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("systems.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A 10 × 8 m structural slab with a 1 × 1 m opening at (3, 2)–(4, 3).
    let f = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([0.0, 0.0], [10.0, 8.0]), rect([3.0, 2.0], [4.0, 3.0])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let slab = reviera_core::floors::regenerate(doc.store(), doc.store().get(f).unwrap()).unwrap();
    assert_eq!(slab.profiles[0].holes.len(), 1, "the slab has its opening");

    // Area and path rebar as real rebar, decided before the first system.
    run(&r, &mut doc, "reinforcement_settings.set", json!({ "key": "area_path_rebar", "value": true }));

    // Area reinforcement over a 6 × 4 m region (1, 1)–(7, 5), its major edge along x.
    let layer = |face: &str, dir: &str, bar: &str| json!({ "face": face, "direction": dir, "bar_type": bar, "spacing": 0.2 });
    let area = id(run(
        &r,
        &mut doc,
        "area_reinforcement.create",
        json!({
            "host_id": f.0,
            "boundary": ring([1.0, 1.0], [7.0, 5.0]),
            "major_edge": 0,
            "layers": [layer("top", "major", "d12"), layer("top", "minor", "d10"), layer("bottom", "major", "d12"), layer("bottom", "minor", "d10")],
        }),
    ));
    assert_eq!(doc.store().get(area).unwrap().param("@PredefinedType").unwrap(), "REINFORCEMENT_UNIT");
    let i = sys(&r, &mut doc, area);
    let groups = i["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 4, "four layers");
    let (top, bottom) = (slab.top, slab.top - slab.thickness);
    for g in groups {
        let name = g["name"].as_str().unwrap();
        let d = g["diameter"].as_f64().unwrap();
        let mut split = 0;
        let mut by_line: std::collections::BTreeMap<i64, usize> = Default::default();
        for b in g["bars"].as_array().unwrap() {
            let p = pts(b);
            let (a, e) = (p[0], *p.last().unwrap());
            // Inside the region less the cover, never through the opening.
            for q in [a, e] {
                assert!(q[0] >= 1.0 + 0.025 - 1e-9 && q[0] <= 7.0 - 0.025 + 1e-9 && q[1] >= 1.0 + 0.025 - 1e-9 && q[1] <= 5.0 - 0.025 + 1e-9, "{name}: {q:?}");
            }
            assert!(!crosses(a, e, [3.0, 2.0], [4.0, 3.0]), "{name} passes through the opening");
            let key = if name.ends_with("major") { (a[1] * 1e6).round() as i64 } else { (a[0] * 1e6).round() as i64 };
            *by_line.entry(key).or_default() += 1;
            // Stacked: major at the cover, minor inside it.
            let z = a[2];
            let want = match name {
                "top major" => top - 0.025 - d / 2.0,
                "top minor" => top - 0.025 - 0.012 - d / 2.0,
                "bottom major" => bottom + 0.025 + d / 2.0,
                _ => bottom + 0.025 + 0.012 + d / 2.0,
            };
            assert!((z - want).abs() < 1e-9, "{name}: z {z} not {want}");
            if name.ends_with("major") {
                assert!((a[1] - e[1]).abs() < 1e-9, "major bars run along x");
            }
        }
        split += by_line.values().filter(|n| **n == 2).count();
        assert!(split >= 4, "{name}: bars on the opening's lines are cut in two ({split})");
        // Maximum spacing 200 across 4 m (major) or 6 m (minor) less the covers and a bar.
        let across = if name.ends_with("major") { 4.0 } else { 6.0 } - 0.05 - d;
        assert_eq!(by_line.len(), (across / 0.2 - 1e-9).ceil() as usize + 1, "{name}");
    }
    // The setting is locked now.
    let e = r
        .execute(&mut doc, "reinforcement_settings.set", json!({ "key": "area_path_rebar", "value": false }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "reinforcement_settings.locked");
    // A layer turned off; hooks into the slab.
    run(&r, &mut doc, "area_reinforcement.set", json!({ "system_ids": [area.0], "layers": [layer("top", "major", "d12"), { "face": "bottom", "direction": "major", "bar_type": "d12", "spacing": 0.2, "hook": "standard_90" }, { "face": "top", "direction": "minor", "bar_type": "d10", "spacing": 0.2, "on": false }] }));
    let i = sys(&r, &mut doc, area);
    assert_eq!(i["groups"].as_array().unwrap().len(), 2);
    let hooked = pts(&i["groups"][1]["bars"][0]);
    assert!(hooked.len() > 2 && hooked.iter().any(|p| p[2] > bottom + 0.1), "the bottom bars' hooks turn up into the slab");
    run(&r, &mut doc, "area_reinforcement.set", json!({ "system_ids": [area.0], "layers": [layer("top", "major", "d12"), layer("top", "minor", "d10"), layer("bottom", "major", "d12"), layer("bottom", "minor", "d10")] }));
    let counts: Vec<u64> = sys(&r, &mut doc, area)["groups"].as_array().unwrap().iter().map(|g| g["count"].as_u64().unwrap()).collect();

    // Path reinforcement: a 5 m path along the slab's edge y = 8, from x = 1 to 6, its bars to
    // the right (into the slab), primary 1 m long and alternating ones 300 mm in.
    let p = id(run(
        &r,
        &mut doc,
        "path_reinforcement.create",
        json!({
            "host_id": f.0, "path": [[1.0, 8.0], [6.0, 8.0]], "flip": true, "face": "top", "spacing": 0.15,
            "primary": { "bar_type": "d12", "length": 1.0 },
            "alternating": { "bar_type": "d10", "length": 1.0, "offset": 0.3 },
        }),
    ));
    let i = sys(&r, &mut doc, p);
    let n = ((5.0f64 - 0.05 - 0.012) / 0.15 - 1e-9).ceil() as u64 + 1;
    let (prim, alt) = (&i["groups"][0], &i["groups"][1]);
    assert_eq!(prim["count"].as_u64().unwrap() + alt["count"].as_u64().unwrap(), n);
    assert_eq!(prim["count"].as_u64().unwrap(), n.div_ceil(2));
    for b in prim["bars"].as_array().unwrap() {
        let q = pts(b);
        assert!((q[0][1] - (8.0 - 0.025)).abs() < 1e-9 && (q[1][1] - 7.0).abs() < 1e-9, "primary from the edge's cover 1 m in: {q:?}");
    }
    for b in alt["bars"].as_array().unwrap() {
        let q = pts(b);
        assert!((q[0][1] - 7.7).abs() < 1e-9 && (q[1][1] - 6.7).abs() < 1e-9, "alternating 300 mm in: {q:?}");
    }
    // In the right pattern: primary and alternating bars take turns along the path.
    let x0p = pts(&prim["bars"][0])[0][0];
    let x0a = pts(&alt["bars"][0])[0][0];
    let x1p = pts(&prim["bars"][1])[0][0];
    assert!(x0p < x0a && x0a < x1p && ((x1p - x0p) - 2.0 * (x0a - x0p)).abs() < 1e-9);
    assert!(i["volume"].as_f64().unwrap() > 0.0);

    // Remove Area System: four rebar sets that can be scheduled.
    let sets: Vec<u64> = run(&r, &mut doc, "area_reinforcement.remove_system", json!({ "system_id": area.0 })).as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(sets.len(), 4);
    assert!(doc.store().get(area).is_none());
    let infos = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": sets }));
    let after: Vec<u64> = infos.as_array().unwrap().iter().map(|i| i["count"].as_u64().unwrap()).collect();
    assert_eq!(after, counts, "the same bars, now sets");

    // A fabric area of A252 over (0, 0)–(10, 6) of the slab: sheets 4.8 × 2.4 lapped 300 mm.
    let fa = id(run(
        &r,
        &mut doc,
        "fabric_area.create",
        json!({ "host_id": f.0, "boundary": ring([0.0, 0.0], [10.0, 6.0]), "sheet_type_id": "A252", "major_lap": 0.3, "minor_lap": 0.3, "face": "bottom", "offset": 0.03 }),
    ));
    let i = sys(&r, &mut doc, fa);
    let sheets = i["sheets"].as_array().unwrap();
    // Rows every 2.1 m over 6 m: 3; columns every 4.5 m over 10 m: 3.
    assert_eq!(sheets.len(), 9, "{}", sheets.len());
    let corner = |s: &Value| [s["corners"][0][0].as_f64().unwrap(), s["corners"][0][1].as_f64().unwrap()];
    let row = |k: u64| sheets.iter().filter(|s| s["row"] == k).map(corner).collect::<Vec<_>>();
    assert!((row(0)[1][0] - row(0)[0][0] - 4.5).abs() < 1e-9, "end laps of 300 mm");
    assert!((row(1)[0][1] - row(0)[0][1] - 2.1).abs() < 1e-9, "side laps of 300 mm");
    let mut cut_by_opening = false;
    for s in sheets {
        for w in s["major"].as_array().unwrap().iter().chain(s["minor"].as_array().unwrap()) {
            let q = pts(w);
            for p in &q {
                assert!(p[0] >= 0.025 - 1e-9 && p[0] <= 10.0 - 0.025 + 1e-9 && p[1] >= 0.025 - 1e-9 && p[1] <= 6.0 - 0.025 + 1e-9, "inside the boundary less the cover: {p:?}");
            }
            assert!(!crosses(q[0], q[1], [3.0, 2.0], [4.0, 3.0]), "no wire through the opening");
            if (q[1][0] - 3.0 + 0.025).abs() < 1e-9 || (q[0][0] - 4.0 - 0.025).abs() < 1e-9 {
                cut_by_opening = true;
            }
        }
    }
    assert!(cut_by_opening, "wires stop at the opening less the cover");
    assert!(sheets.iter().any(|s| s["cut"] == true));
    // Wires sit on the bottom cover plus the offset, the minor ones on top of the major ones.
    let w = pts(&sheets[0]["major"][0]);
    assert!((w[0][2] - (bottom + 0.025 + 0.03 + 0.004)).abs() < 1e-9);
    // Staggered: every second row's laps move half a sheet.
    run(&r, &mut doc, "fabric_area.set", json!({ "system_ids": [fa.0], "lap_position": "staggered" }));
    let st = sys(&r, &mut doc, fa);
    let staggered: Vec<[f64; 2]> = st["sheets"].as_array().unwrap().iter().filter(|s| s["row"] == 1).map(corner).collect();
    let aligned = row(1);
    assert!((aligned[0][0] - staggered[0][0] - 2.4).abs() < 1e-9, "{aligned:?} {staggered:?}");
    let even: Vec<[f64; 2]> = st["sheets"].as_array().unwrap().iter().filter(|s| s["row"] == 2).map(corner).collect();
    assert_eq!(even, row(2), "even rows stay");

    // A single sheet, turned 90° and cut by the cover at the slab's corner.
    let one = id(run(&r, &mut doc, "fabric_sheet.place", json!({ "host_id": f.0, "sheet_type_id": "B503", "point": [9.0, 6.0], "rotation": 90.0, "cut_by_cover": true, "face": "top" })));
    let i = sys(&r, &mut doc, one);
    let s = &i["sheets"][0];
    assert_eq!(s["cut"], true);
    for w in s["major"].as_array().unwrap() {
        let q = pts(w);
        assert!((q[0][0] - q[1][0]).abs() < 1e-9, "major wires run along y when turned");
        assert!(q[1][1] <= 8.0 - 0.025 + 1e-9);
    }
    assert!(i["mass"].as_f64().unwrap() > 0.0);
    let types = run(&r, &mut doc, "fabric_sheet_type.list", json!(null));
    let a252 = types.as_array().unwrap().iter().find(|t| t["name"] == "A252").unwrap();
    assert!((a252["unit_mass"].as_f64().unwrap() - 3.95).abs() < 0.01, "{a252}");

    // The export: assemblies with their bars and sheets; it reads back without doubling.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".REINFORCEMENT_UNIT."));
    assert!(text.contains("IFCREINFORCINGMESH(") && text.contains("IFCREINFORCINGMESHTYPE("));
    assert!(text.contains("'rxd:rebar_part'"));
    let meshes = text.matches("IFCREINFORCINGMESH(").count();
    assert_eq!(meshes, st["sheets"].as_array().unwrap().len() + 1);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(doc2.store().iter().filter(|e| e.class == "IfcReinforcingMesh").count(), 1, "only the single sheet is an element");
    let back = run(&r, &mut doc2, "reinforcement_system.info", json!({ "system_ids": [fa.0, p.0] }));
    assert_eq!(back[0]["sheets"].as_array().unwrap().len(), st["sheets"].as_array().unwrap().len());
    assert_eq!(back[1]["bar_count"], sys(&r, &mut doc, p)["bar_count"]);
    project.save(&doc).unwrap();
    let again = std::fs::read_to_string(&path).unwrap();
    assert_eq!(again.matches("IFCREINFORCINGMESH(").count(), meshes, "parts rewritten, not added");
    assert_eq!(again.matches("IFCREINFORCINGMESHTYPE(").count(), text.matches("IFCREINFORCINGMESHTYPE(").count());

    // The systems go with their floor.
    let going = reviera_core::builtin::delete_closure(doc.store(), &[f]);
    let cats: std::collections::BTreeSet<&str> = going
        .iter()
        .filter_map(|g| doc.store().get(*g).and_then(reviera_core::categories::category_of))
        .map(|c| c.name)
        .collect();
    for c in ["Structural Path Reinforcement", "Structural Fabric Areas", "Structural Fabric Reinforcement", "Structural Rebar"] {
        assert!(cats.contains(c), "{c} in {cats:?}");
    }
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [f.0] }));
    for s in [p, fa, one] {
        assert!(doc.store().get(s).is_none());
    }
}

#[test]
fn area_reinforcement_in_a_wall() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("wall.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let w = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [5.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    // The whole wall, major bars along it (the outline's first edge), at both faces.
    let a = id(run(
        &r,
        &mut doc,
        "area_reinforcement.create",
        json!({ "host_id": w.0, "boundary": "whole_host", "layers": [
            { "face": "exterior", "direction": "major", "bar_type": "d10", "spacing": 0.3 },
            { "face": "exterior", "direction": "minor", "bar_type": "d10", "spacing": 0.3 },
            { "face": "interior", "direction": "major", "bar_type": "d10", "spacing": 0.3 },
        ] }),
    ));
    let i = sys(&r, &mut doc, a);
    let major = &i["groups"][0];
    let q = pts(&major["bars"][0]);
    assert!((q[0][2] - q[1][2]).abs() < 1e-9, "major bars run horizontally");
    let minor = pts(&i["groups"][1]["bars"][0]);
    assert!((minor[0][0] - minor[1][0]).abs() < 1e-9 && (minor[1][2] - minor[0][2]).abs() > 2.5, "minor bars run up the wall");
    // Faces that are not the wall's are refused.
    let e = r
        .execute(&mut doc, "area_reinforcement.create", json!({ "host_id": w.0, "layers": [{ "face": "top", "direction": "major", "bar_type": "d10", "spacing": 0.3 }] }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.face");
    // Without real rebar, Remove Area System is refused.
    let e = r.execute(&mut doc, "area_reinforcement.remove_system", json!({ "system_id": a.0 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "area_reinforcement.not_real");
}
