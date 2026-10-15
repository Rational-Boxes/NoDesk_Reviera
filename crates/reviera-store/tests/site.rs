// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Toposolids, contours, cut and fill, simplify, property lines and site components
//! (ARC-TOPO-001 … 005, ARC-SITE-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, toposolids};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn setup() -> (
    tempfile::TempDir,
    LocalProject,
    Document,
    CommandRegistry,
    ElementId,
) {
    let tmp = tempfile::tempdir().unwrap();
    let (p, doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("site.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let l = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    (tmp, p, doc, r, l)
}

#[test]
fn contours_follow_the_site_settings() {
    let (_t, mut p, mut doc, r, l) = setup();
    // 10 % over 50 m: 0.5 m to 5.5 m.
    let t = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": [[0.0, 0.0, 0.5], [50.0, 0.0, 5.5], [50.0, 50.0, 5.5], [0.0, 50.0, 0.5]] }),
    )
    .unwrap());
    let c = toposolids::contours(doc.store(), doc.store().get(t).unwrap());
    let heights: Vec<f64> = c.iter().map(|x| x.0).collect();
    assert_eq!(heights, vec![1.0, 2.0, 3.0, 4.0, 5.0], "{heights:?}");
    // Evenly 10 m apart in x.
    let xs: Vec<f64> = c.iter().map(|x| x.1[0][0]).collect();
    for w in xs.windows(2) {
        assert!((w[1] - w[0] - 10.0).abs() < 1e-6, "{xs:?}");
    }
    // Interval 10 through 5: −20 … 20 gives −15, −5, 5, 15.
    run(
        &r,
        &mut doc,
        "site.set_settings",
        json!({ "contour_interval": 10.0, "passing_elevation": 5.0 }),
    )
    .unwrap();
    let t2 = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": [[100.0, 0.0, -20.0], [140.0, 0.0, 20.0], [140.0, 40.0, 20.0], [100.0, 40.0, -20.0]] }),
    )
    .unwrap());
    let mut h: Vec<f64> = toposolids::contours(doc.store(), doc.store().get(t2).unwrap())
        .iter()
        .map(|x| x.0)
        .collect();
    h.dedup();
    assert_eq!(h, vec![-15.0, -5.0, 5.0, 15.0]);
    p.save(&doc).unwrap();
}

#[test]
fn graded_cut_and_fill_and_subdivisions() {
    let (_t, mut p, mut doc, r, l) = setup();
    let a = 50f64.sqrt() * 1.0;
    // Flat 20 × 20 m; the middle point's neighbours make a diamond of 100 m².
    let t = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "boundary": [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]],
                "points": [[10.0, 10.0, 0.0], [10.0 - a, 10.0, 0.0], [10.0 + a, 10.0, 0.0], [10.0, 10.0 - a, 0.0], [10.0, 10.0 + a, 0.0],
                           [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 20.0, 0.0], [0.0, 20.0, 0.0]] }),
    )
    .unwrap());
    let g = id(&run(
        &r,
        &mut doc,
        "toposolid.create_graded_region",
        json!({ "toposolid_id": t.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "toposolid.set_points",
        json!({ "toposolid_id": g.0, "points": [[10.0, 10.0, -1.0]] }),
    )
    .unwrap();
    let cf = run(
        &r,
        &mut doc,
        "toposolid.cut_fill",
        json!({ "toposolid_id": g.0 }),
    )
    .unwrap();
    assert!(
        (cf["cut"].as_f64().unwrap() - 100.0 / 3.0).abs() < 1e-6,
        "{cf}"
    );
    assert!(cf["fill"].as_f64().unwrap().abs() < 1e-9);
    // A road excavated 0.3 m: the volume, and the toposolid cut by it.
    let road = id(&run(
        &r,
        &mut doc,
        "toposolid.subdivide",
        json!({ "toposolid_id": t.0, "region": [[0.0, 2.0], [20.0, 2.0], [20.0, 7.0], [0.0, 7.0]], "offset": -0.3 }),
    )
    .unwrap());
    let info = run(
        &r,
        &mut doc,
        "toposolid.info",
        json!({ "toposolid_id": road.0 }),
    )
    .unwrap();
    assert!(
        (info["excavation"].as_f64().unwrap() - 30.0).abs() < 1e-6,
        "{info}"
    );
    assert_eq!(reviera_core::csg::resolved(doc.store()).len(), 1);
    p.save(&doc).unwrap();
}

#[test]
fn simplify_keeps_its_word() {
    let (_t, _p, mut doc, r, l) = setup();
    // A 30 × 30 grid of points on a gentle bump.
    let mut pts = Vec::new();
    for i in 0..30 {
        for j in 0..30 {
            let (x, y) = (f64::from(i), f64::from(j));
            pts.push(json!([
                x,
                y,
                ((x - 15.0).powi(2) + (y - 15.0).powi(2)).sqrt() * 0.05
            ]));
        }
    }
    let t = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": pts }),
    )
    .unwrap());
    let before = toposolids::recipe(doc.store().get(t).unwrap())
        .unwrap()
        .boundary
        .clone();
    let s = run(
        &r,
        &mut doc,
        "toposolid.simplify",
        json!({ "toposolid_id": t.0, "target_points": 300 }),
    )
    .unwrap();
    assert!(s["points"].as_u64().unwrap() <= 300, "{s}");
    let after = toposolids::recipe(doc.store().get(t).unwrap()).unwrap();
    assert_eq!(after.boundary, before, "the boundary is kept");
    // The reported deviation is the real one: every original point is within it.
    let dev = s["max_deviation"].as_f64().unwrap();
    let e = doc.store().get(t).unwrap();
    for q in pts.iter().map(|v| {
        [
            v[0].as_f64().unwrap(),
            v[1].as_f64().unwrap(),
            v[2].as_f64().unwrap(),
        ]
    }) {
        let z = toposolids::height_at(doc.store(), e, [q[0], q[1]]).unwrap();
        assert!((z - q[2]).abs() <= dev + 1e-9);
    }
    // A planar terrain simplifies with zero error to its boundary plus one point.
    let flat: Vec<Value> = (0..10)
        .flat_map(|i| (0..10).map(move |j| json!([f64::from(i), f64::from(j) + 40.0, 0.0])))
        .collect();
    let f = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": flat }),
    )
    .unwrap());
    let s = run(
        &r,
        &mut doc,
        "toposolid.simplify",
        json!({ "toposolid_id": f.0, "max_vertical_error": 0.0 }),
    )
    .unwrap();
    assert!(s["max_deviation"].as_f64().unwrap() < 1e-9);
    let left = toposolids::recipe(doc.store().get(f).unwrap()).unwrap();
    let interior = left
        .points
        .iter()
        .filter(|q| {
            !left
                .boundary
                .iter()
                .any(|b| (b[0] - q.x).abs() < 1e-9 && (b[1] - q.y).abs() < 1e-9)
        })
        .count();
    assert!(interior <= 1, "{interior} interior points left");
    // A locked survey point stays when simplifying to the minimum.
    let q = after
        .points
        .iter()
        .find(|q| !after.boundary.iter().any(|b| b[0] == q.x && b[1] == q.y))
        .map(|q| [q.x, q.y])
        .unwrap();
    run(
        &r,
        &mut doc,
        "toposolid.set_points",
        json!({ "toposolid_id": t.0, "lock": [q] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "toposolid.simplify",
        json!({ "toposolid_id": t.0, "target_points": 1 }),
    )
    .unwrap();
    let left = toposolids::recipe(doc.store().get(t).unwrap()).unwrap();
    assert!(
        left.points
            .iter()
            .any(|p| p.locked && p.x == q[0] && p.y == q[1])
    );
    // Both at once is refused.
    assert_eq!(
        run(
            &r,
            &mut doc,
            "toposolid.simplify",
            json!({ "toposolid_id": f.0, "target_points": 5, "max_vertical_error": 0.1 })
        )
        .unwrap_err()
        .code,
        "toposolid.simplify_args"
    );
}

#[test]
fn property_lines_and_a_tree_on_the_ground() {
    let (tmp, mut p, mut doc, r, l) = setup();
    let pl = run(
        &r,
        &mut doc,
        "propertyline.create",
        json!({ "table": [
            { "distance": 30.0, "bearing": "N 0°E" },
            { "distance": 20.0, "bearing": "N 90°E" },
            { "distance": 30.0, "bearing": "S 0°E" },
            { "distance": 20.0, "bearing": "S 90°W" },
        ] }),
    )
    .unwrap();
    assert!((pl["area"].as_f64().unwrap() - 600.0).abs() < 1e-6, "{pl}");
    // A tree on a toposolid rises with the ground under it.
    let t = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [5.0, 5.0, 0.0]] }),
    )
    .unwrap());
    let obj = tmp.path().join("tree.obj");
    std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 3\nv 1 0 3\nv 1 1 3\nv 0 1 3\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n").unwrap();
    let ty = id(&run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": obj, "category": "generic_model" }),
    )
    .unwrap());
    let tree = id(&run(
        &r,
        &mut doc,
        "sitecomponent.place",
        json!({ "type_id": ty.0, "point": [5.0, 5.0], "host_id": t.0, "level": l.0 }),
    )
    .unwrap());
    let z = |doc: &Document| {
        reviera_core::bodies::all(doc.store())
            .into_iter()
            .find(|b| b.element == tree)
            .unwrap()
            .bounds()
            .unwrap()
            .0[2]
    };
    let z0 = z(&doc);
    run(
        &r,
        &mut doc,
        "toposolid.set_points",
        json!({ "toposolid_id": t.0, "points": [[5.0, 5.0, 1.0]] }),
    )
    .unwrap();
    assert!((z(&doc) - z0 - 1.0).abs() < 1e-6, "{} {}", z0, z(&doc));
    p.save(&doc).unwrap();
}

#[test]
fn a_hill_with_a_road_cut_saves() {
    let (_t, mut p, mut doc, r, l) = setup();
    let mut pts = Vec::new();
    for i in 0..13 {
        for j in 0..13 {
            let (x, y) = (f64::from(i) * 2.5, f64::from(j) * 2.5);
            pts.push(json!([x, y, 1.5 * (x / 8.0).sin() + (y / 6.0).cos()]));
        }
    }
    let t = id(&run(
        &r,
        &mut doc,
        "toposolid.create",
        json!({ "level_id": l.0, "points": pts }),
    )
    .unwrap());
    // The body is closed: every edge has exactly two faces.
    let (_, idx) = toposolids::regenerate(doc.store(), doc.store().get(t).unwrap()).unwrap();
    let mut edges = std::collections::HashMap::new();
    for c in idx.chunks(3) {
        for (a, b) in [(c[0], c[1]), (c[1], c[2]), (c[2], c[0])] {
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    assert!(edges.values().all(|n| *n == 2));
    run(
        &r,
        &mut doc,
        "toposolid.subdivide",
        json!({ "toposolid_id": t.0, "region": [[0.0, 12.0], [30.0, 12.0], [30.0, 16.0], [0.0, 16.0]], "offset": -0.3 }),
    )
    .unwrap();
    // The excavation takes the road's volume out of the ground, and only that.
    let vol = |bodies: &[reviera_core::bodies::Body]| {
        let b = bodies.iter().find(|b| b.element == t).unwrap();
        b.parts
            .iter()
            .map(|p| {
                let (pts, idx) = &p.mesh;
                idx.chunks(3)
                    .map(|c| {
                        let [a, b, c] =
                            [pts[c[0] as usize], pts[c[1] as usize], pts[c[2] as usize]];
                        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                            + a[2] * (b[0] * c[1] - b[1] * c[0]))
                            / 6.0
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
    };
    let mut bodies = reviera_core::bodies::all(doc.store());
    let before = vol(&bodies);
    reviera_geom::csg::apply(&Default::default(), doc.store(), &mut bodies);
    let after = vol(&bodies);
    assert!(
        (before - after - 30.0 * 4.0 * 0.3).abs() < 0.05,
        "{before} → {after}"
    );
    let start = std::time::Instant::now();
    p.save(&doc).unwrap();
    assert!(
        start.elapsed().as_secs() < 30,
        "saving took {:?}",
        start.elapsed()
    );
}
