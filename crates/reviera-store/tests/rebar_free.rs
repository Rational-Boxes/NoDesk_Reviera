// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Free-form rebar (STR-RBR-013): a surface distribution on a concrete dome keeps every bar at
//! the cover within 1 mm of the shell's underside at sampled points; an aligned distribution up a
//! tapered wall gives links whose width follows the taper; along a rectangular beam the links
//! match the section's own (shape 51); a non-concrete roof is refused; free-form sets are always
//! varying.

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

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The distance from a point to a triangle.
fn to_triangle(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let n = cross(sub(b, a), sub(c, a));
    let nl = dot(n, n).sqrt();
    let n = [n[0] / nl, n[1] / nl, n[2] / nl];
    let h = dot(sub(p, a), n);
    let q = [p[0] - n[0] * h, p[1] - n[1] * h, p[2] - n[2] * h];
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| dot(cross(sub(*v, *u), sub(q, *u)), n) >= -1e-12);
    if inside {
        return h.abs();
    }
    let seg = |u: V3, v: V3| {
        let d = sub(v, u);
        let t = (dot(sub(p, u), d) / dot(d, d)).clamp(0.0, 1.0);
        let x = sub(p, [u[0] + d[0] * t, u[1] + d[1] * t, u[2] + d[2] * t]);
        dot(x, x).sqrt()
    };
    seg(a, b).min(seg(b, c)).min(seg(c, a))
}

fn bars(i: &Value) -> Vec<Vec<V3>> {
    i["bars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["points"].as_array().unwrap().iter().map(|p| [0, 1, 2].map(|k| p[k].as_f64().unwrap())).collect())
        .collect()
}

#[test]
fn surface_and_aligned_distributions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("free.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A hemisphere of 5 m radius, 300 mm thick: refused until its type is concrete.
    let dome = id(run(&r, &mut doc, "roof.create_dome", json!({ "level_id": l1.0, "base_offset": 3.0, "center": [0.0, 0.0], "plan": { "circle": { "radius": 5.0 } } })));
    let place = |doc: &mut Document, args: Value| r.execute(doc, "rebar.place_free_form", args, &Caller::user());
    let surface = json!({ "host_id": dome.0, "kind": "surface", "bar_type": "d12", "face": "bottom", "direction": "x", "layout": { "rule": "maximum_spacing", "spacing": 0.5 } });
    let e = place(&mut doc, surface.clone()).unwrap_err();
    assert_eq!(e.code, "rebar.not_a_host", "{e:?}");
    let ty = doc.store().iter().find(|e| e.class == "IfcRoofType").unwrap().id();
    run(&r, &mut doc, "type.rename", json!({ "type_id": ty.0, "name": "Concrete Shell 300" }));
    let shell = id(place(&mut doc, surface).unwrap().result);

    // Every bar sampled at its points: 25 mm cover plus half a bar from the underside.
    let i = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [shell.0] }))[0].clone();
    assert!(i["count"].as_u64().unwrap() >= 15, "{}", i["count"]);
    let shell_count = i["count"].clone();
    assert_eq!(i["varying"], true);
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(dome).unwrap()).unwrap();
    let tris: Vec<[V3; 3]> = g.faces.iter().flat_map(|f| (1..f.bottom.len() - 1).map(move |k| [f.bottom[0], f.bottom[k], f.bottom[k + 1]])).collect();
    let want = 0.025 + 0.006;
    let mut worst = 0.0f64;
    let mut sampled = 0;
    for b in bars(&i) {
        // Each bar in its plane (x constant) and following the shell.
        assert!(b.iter().all(|p| (p[0] - b[0][0]).abs() < 1e-9));
        for p in b.iter().step_by(3) {
            let d = tris.iter().map(|t| to_triangle(*p, t[0], t[1], t[2])).fold(f64::INFINITY, f64::min);
            worst = worst.max((d - want).abs());
            sampled += 1;
        }
    }
    assert!(sampled > 100, "{sampled}");
    assert!(worst < 0.001, "the cover is off by {:.2} mm somewhere", worst * 1000.0);
    // The middle bar runs over the crown: up near the dome's height.
    let crown = bars(&i).iter().flatten().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!(crown > 3.0 + 5.0 - 0.05, "{crown}");

    // Free-form sets are always varying.
    let e = r.execute(&mut doc, "rebar.convert_to_standard", json!({ "rebar_ids": [shell.0] }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar.not_varying");

    // A 3 m wall, 240 mm at its base, each face leaning in 1°: links up it, square to the path.
    let w = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [20.0, 0.0], "end": [21.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": [
            { "function": "finish1", "material": "Render", "thickness": "20 mm" },
            { "function": "structure", "material": "Concrete", "thickness": "200 mm", "variable": true },
            { "function": "finish2", "material": "Plaster", "thickness": "20 mm" },
        ] }),
    );
    let lean = 1f64.to_radians();
    run(&r, &mut doc, "wall.set_cross_section", json!({ "wall_ids": [w.0], "mode": "tapered", "exterior_angle": lean, "interior_angle": lean }));
    let links = id(run(
        &r,
        &mut doc,
        "rebar.place_free_form",
        json!({ "host_id": w.0, "kind": "aligned", "path_ref": "up", "bar_type": "d8", "layout": { "rule": "fixed_number", "n": 5 } }),
    ));
    let i = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [links.0] }))[0].clone();
    assert_eq!((i["shape"].clone(), i["role"].clone()), (json!("51"), json!("ligature")));
    let ls = bars(&i);
    assert_eq!(ls.len(), 5);
    // Each link's width across the wall (y), at its height.
    let across = |b: &Vec<V3>| {
        let ys: Vec<f64> = b.iter().map(|p| p[1]).collect();
        ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min)
    };
    let (w0, z0) = (across(&ls[0]), ls[0][0][2]);
    for b in &ls[1..] {
        let want = w0 - 2.0 * (b[0][2] - z0) * lean.tan();
        assert!((across(b) - want).abs() < 1e-6, "{} vs {want}", across(b));
        // Level: square to the upright path.
        let (lo, hi) = b.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p[2]), h.max(p[2])));
        assert!(hi - lo < 0.008 + 1e-9, "a bar apart at the hooks at most: {}", hi - lo);
    }
    assert!(across(&ls[4]) < w0 - 0.05, "the top link is narrower");
    // Their lengths differ: several numbers.
    assert!(i["length_groups"].as_array().unwrap().len() == 5);

    // Along a rectangular 300 × 500 beam: the same links as the section placement's.
    let b0 = id(run(&r, &mut doc, "beam.create", json!({ "curve": { "start": [0.0, 20.0], "end": [5.0, 20.0] }, "reference_level": l1.0 })));
    let steel = doc.store().get(b0).unwrap().param("@type").unwrap().as_str().unwrap().to_owned();
    let steel = doc.store().find_by_guid(&reviera_core::GlobalId::parse(&steel).unwrap()).unwrap();
    let bty = id(run(&r, &mut doc, "type.duplicate", json!({ "type_id": steel.0, "new_name": "Concrete 300 x 500" })));
    for (k, v) in [("section", json!("rectangular")), ("width", json!("300 mm")), ("depth", json!("500 mm")), ("material_kind", json!("concrete"))] {
        run(&r, &mut doc, "type.set_param", json!({ "type_id": bty.0, "param": k, "value": v }));
    }
    let beam = id(run(&r, &mut doc, "beam.create", json!({ "type_id": bty.0, "curve": { "start": [0.0, 30.0], "end": [6.0, 30.0] }, "reference_level": l1.0 })));
    let section = id(run(&r, &mut doc, "rebar.place", json!({ "host_id": beam.0, "shape": "51", "bar_type": "d10", "placement": { "kind": "section" }, "layout": { "rule": "fixed_number", "n": 3 } })));
    let aligned = id(run(&r, &mut doc, "rebar.place_free_form", json!({ "host_id": beam.0, "kind": "aligned", "path_ref": "axis", "bar_type": "d10", "layout": { "rule": "fixed_number", "n": 3 } })));
    let dims = |doc: &mut Document, s: ElementId| {
        let i = run(&r, doc, "rebar.info", json!({ "rebar_ids": [s.0] }))[0].clone();
        let mut ab: Vec<f64> = i["dimensions"].as_array().unwrap()[..2].iter().map(|d| (d[1].as_f64().unwrap() * 1e6).round() / 1e6).collect();
        ab.sort_by(f64::total_cmp);
        (i["shape"].clone(), ab, i["count"].clone())
    };
    assert_eq!(dims(&mut doc, aligned), dims(&mut doc, section));

    // A surface distribution needs a roof.
    let e = r
        .execute(&mut doc, "rebar.place_free_form", json!({ "host_id": beam.0, "kind": "surface", "bar_type": "d10" }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.free_form");

    // Saved and read back.
    project.save(&doc).unwrap();
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| e.guid() == doc.store().get(shell).unwrap().guid()).unwrap().id();
    let i2 = run(&r, &mut doc2, "rebar.info", json!({ "rebar_ids": [back.0] }))[0].clone();
    assert_eq!(i2["count"], shell_count);
}
