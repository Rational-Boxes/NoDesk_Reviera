// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Path and morphed distributions (STR-RBR-014): longitudinal bars morphed through a beam's
//! links sit inside each link, touching it, and move in when the links' diameter changes; a link
//! carried along a 3D-curved path keeps each bar's plane square to the path's tangent; a main
//! bar not square to the path is refused; both go with their source set.

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
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn bars(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> Vec<Vec<V3>> {
    let i = run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }));
    i[0]["bars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["points"].as_array().unwrap().iter().map(|p| [0, 1, 2].map(|k| p[k].as_f64().unwrap())).collect())
        .collect()
}

/// The distance from a point to a polyline.
fn to_polyline(p: V3, l: &[V3]) -> f64 {
    l.windows(2)
        .map(|w| {
            let d = sub(w[1], w[0]);
            let t = if dot(d, d) > 0.0 { (dot(sub(p, w[0]), d) / dot(d, d)).clamp(0.0, 1.0) } else { 0.0 };
            len(sub(p, [w[0][0] + d[0] * t, w[0][1] + d[1] * t, w[0][2] + d[2] * t]))
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn path_and_morphed_distributions() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("paths.ifc")).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let b0 = id(run(&r, &mut doc, "beam.create", json!({ "curve": { "start": [0.0, 20.0], "end": [5.0, 20.0] }, "reference_level": l1.0 })));
    let steel = doc.store().get(b0).unwrap().param("@type").unwrap().as_str().unwrap().to_owned();
    let steel = doc.store().find_by_guid(&reviera_core::GlobalId::parse(&steel).unwrap()).unwrap();
    let ty = id(run(&r, &mut doc, "type.duplicate", json!({ "type_id": steel.0, "new_name": "Concrete 300 x 500" })));
    for (k, v) in [("section", json!("rectangular")), ("width", json!("300 mm")), ("depth", json!("500 mm")), ("material_kind", json!("concrete"))] {
        run(&r, &mut doc, "type.set_param", json!({ "type_id": ty.0, "param": k, "value": v }));
    }
    let beam = id(run(&r, &mut doc, "beam.create", json!({ "type_id": ty.0, "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l1.0 })));

    // Ø10 links, and eight Ø16 bars morphed through them.
    let links = id(run(&r, &mut doc, "rebar.place", json!({ "host_id": beam.0, "shape": "51", "bar_type": "d10", "placement": { "kind": "section" }, "layout": { "rule": "fixed_number", "n": 10 } })));
    let long = id(run(&r, &mut doc, "rebar.distribute_morphed", json!({ "driver_stirrup_set_id": links.0, "bar_type": "d16", "count": 8 })));
    let check = |doc: &mut Document, d_link: f64| {
        let ls = bars(&r, doc, links);
        let bs = bars(&r, doc, long);
        assert_eq!(bs.len(), 8);
        for b in &bs {
            assert_eq!(b.len(), ls.len(), "a point at each link");
            for (k, p) in b.iter().enumerate() {
                // Touching the link from inside: the centres a half bar and a half link apart.
                let loop_: Vec<V3> = ls[k][1..ls[k].len() - 1].to_vec();
                let d = to_polyline(*p, &loop_);
                let touch = d_link / 2.0 + 0.008;
                assert!((d - touch).abs() < 1e-6, "{d} vs {touch}");
                // And inside it: within the link's extent across and up.
                let (lo, hi) = loop_.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1]), l[2].min(q[2])], [h[0].max(q[0]), h[1].max(q[1]), h[2].max(q[2])]));
                assert!(p[1] > lo[1] && p[1] < hi[1] && p[2] > lo[2] && p[2] < hi[2], "{p:?} outside {lo:?} … {hi:?}");
            }
        }
        // Straight along the beam: the same across and up at every link.
        assert!(bs.iter().all(|b| b.iter().all(|p| (p[1] - b[0][1]).abs() < 1e-9 && (p[2] - b[0][2]).abs() < 1e-9)));
        bs
    };
    let before = check(&mut doc, 0.010);
    assert_eq!(run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [long.0] }))[0]["shape"], "00");
    // The links to Ø16: the bars move in with the smaller outline and thicker link.
    run(&r, &mut doc, "rebar.set", json!({ "rebar_ids": [links.0], "bar_type": "d16" }));
    let after = check(&mut doc, 0.016);
    let top = |bs: &Vec<Vec<V3>>| bs.iter().map(|b| b[0][2]).fold(f64::MIN, f64::max);
    assert!((top(&before) - top(&after) - (0.003 + 0.003)).abs() < 1e-6, "{} → {}", top(&before), top(&after));

    // A closed link sketched square to the beam at 0.5 m, carried along a path curving in plan and
    // rising: x = 0.5 + 3 sin t, y = 3 (1 − cos t), z = z0 + 0.3 t².
    let z0 = -0.25;
    let link = id(run(
        &r,
        &mut doc,
        "rebar.sketch",
        json!({ "host_id": beam.0, "plane": { "origin": [0.5, 0.0, z0], "x": [0.0, 1.0, 0.0], "normal": [1.0, 0.0, 0.0] }, "style": "stirrup_tie", "bar_type": "d10",
                "curves": [
                    { "line": { "start": [-0.1, -0.2], "end": [0.1, -0.2] } }, { "line": { "start": [0.1, -0.2], "end": [0.1, 0.2] } },
                    { "line": { "start": [0.1, 0.2], "end": [-0.1, 0.2] } }, { "line": { "start": [-0.1, 0.2], "end": [-0.1, -0.2] } },
                ] }),
    ));
    let path: Vec<V3> = (0..=60)
        .map(|k| {
            let t = k as f64 / 60.0 * std::f64::consts::FRAC_PI_2;
            [0.5 + 3.0 * t.sin(), 3.0 * (1.0 - t.cos()), z0 + 0.3 * t * t]
        })
        .collect();
    let along = id(run(&r, &mut doc, "rebar.distribute_along_path", json!({ "main_rebar_id": link.0, "path": path, "layout": { "rule": "fixed_number", "n": 7 } })));
    let bs = bars(&r, &mut doc, along);
    assert_eq!(bs.len(), 7);
    let first_len: f64 = bs[0].windows(2).map(|w| len(sub(w[1], w[0]))).sum();
    for b in &bs {
        // The bar's plane, from three of its points far apart.
        let n = cross(sub(b[b.len() / 3], b[0]), sub(b[2 * b.len() / 3], b[0]));
        let n = [n[0] / len(n), n[1] / len(n), n[2] / len(n)];
        assert!(b.iter().all(|p| dot(sub(*p, b[0]), n).abs() < 1e-6), "planar");
        // The path's tangent where the bar lies: the segment nearest its middle.
        let mid = b.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]).map(|v| v / b.len() as f64);
        let k = (0..path.len() - 1).min_by(|&i, &j| to_polyline(mid, &path[i..=i + 1]).total_cmp(&to_polyline(mid, &path[j..=j + 1]))).unwrap();
        let t = sub(path[k + 1], path[k]);
        let t = [t[0] / len(t), t[1] / len(t), t[2] / len(t)];
        assert!(dot(n, t).abs() > 1.0 - 1e-9, "square to the tangent: {}", dot(n, t));
        // The path passes through the bar's plane, near its middle.
        let w = sub(path[k + 1], path[k]);
        let u = (dot(sub(mid, path[k]), w) / dot(w, w)).clamp(0.0, 1.0);
        let foot = [path[k][0] + w[0] * u, path[k][1] + w[1] * u, path[k][2] + w[2] * u];
        assert!(dot(sub(foot, b[0]), n).abs() < 1e-6 && len(sub(foot, mid)) < 0.05, "on the path");
        let l: f64 = b.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
        assert!((l - first_len).abs() < 1e-9, "the same bar");
    }
    // A path square to the main bar's plane is refused.
    let e = r
        .execute(&mut doc, "rebar.distribute_along_path", json!({ "main_rebar_id": link.0, "path": [[0.5, 0.0, z0], [0.5, 3.0, z0]] }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.path");
    assert!(e.message.contains("not square"), "{}", e.message);

    // Both go with their source sets.
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [links.0, link.0] }));
    assert!(doc.store().get(long).is_none() && doc.store().get(along).is_none());
}
