// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Shaped windows and doors (spec Ch 3 §3.4.3a, ARC-WIN-002): round and round-top outlines,
//! their openings in the wall, fitting, and their IFC openings.

use std::path::Path;

use reviera_core::inserts::{self, Shape};
use reviera_core::walls::{self, openings};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn new_project(dir: &Path) -> (LocalProject, Document) {
    LocalProject::new(Schema::Ifc4, &dir.join("shaped.ifc")).unwrap()
}

/// A 3 m high, 200 mm wall from (0, 0) to (`length`, 0) on Level 1.
fn wall(r: &CommandRegistry, doc: &mut Document, length: f64) -> ElementId {
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    id(run(
        r,
        doc,
        "wall.create",
        json!({
            "curve": { "line": { "start": [0, 0], "end": [length, 0] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap())
}

/// A new type of `kind` ("window" or "door") from the project's default, with `params` set.
fn shaped_type(
    r: &CommandRegistry,
    doc: &mut Document,
    kind: &str,
    name: &str,
    params: &[(&str, Value)],
) -> ElementId {
    // Placing one makes the default type; it is removed again.
    let w = wall(r, doc, 20.0);
    let first = id(run(
        r,
        doc,
        &format!("{kind}.place"),
        json!({ "host_wall_id": w.0, "point_along": 10.0 }),
    )
    .unwrap());
    let guid = doc
        .store()
        .get(first)
        .unwrap()
        .param("@type")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let default = doc
        .store()
        .find_by_guid(&reviera_core::GlobalId::parse(&guid).unwrap())
        .unwrap();
    run(r, doc, "element.delete", json!({ "element_ids": [w.0] })).unwrap();
    let t = id(run(
        r,
        doc,
        "type.duplicate",
        json!({ "type_id": default.0, "new_name": name }),
    )
    .unwrap());
    for (param, value) in params {
        run(
            r,
            doc,
            "type.set_param",
            json!({ "type_id": t.0, "param": param, "value": value }),
        )
        .unwrap_or_else(|e| panic!("{param}: {e}"));
    }
    t
}

fn placed(doc: &Document, id: ElementId) -> inserts::Placed {
    inserts::resolve(doc.store(), doc.store().get(id).unwrap()).unwrap()
}

/// Signed volume of a closed mesh.
fn volume(p: &[[f64; 3]], i: &[u32]) -> f64 {
    i.chunks(3)
        .map(|t| {
            let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

/// Where the triangles of a mesh cross the plane z = `z`.
fn crossings(p: &[[f64; 3]], i: &[u32], z: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for t in i.chunks(3) {
        let tri = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            if (a[2] - z) * (b[2] - z) < 0.0 {
                let s = (z - a[2]) / (b[2] - a[2]);
                out.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, z]);
            }
        }
    }
    out
}

#[test]
fn a_round_window_cuts_a_circle_and_shows_its_width_at_the_cut_plane() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, mut doc) = new_project(dir.path());
    let r = registry();
    let round = shaped_type(
        &r,
        &mut doc,
        "window",
        "Oculus 900",
        &[("shape", json!("round")), ("width", json!(0.9))],
    );
    let t = inserts::insert_type(doc.store().get(round).unwrap()).unwrap();
    assert_eq!(t.shape, Shape::Round);
    assert!(
        (t.height - 0.9).abs() < 1e-12,
        "a round window's height is its width"
    );

    // Centred 1.5 m up in a 5 m wall.
    let w = wall(&r, &mut doc, 5.0);
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "type_id": round.0, "host_wall_id": w.0, "point_along": 2.5, "sill_height": 1.05 }),
    )
    .unwrap());
    let p = placed(&doc, win);
    assert!(!p.cut().outline.is_empty());

    // The wall loses π · 0.45² · 0.2.
    let body = walls::solve(doc.store()).bodies[&w];
    let whole = body.mesh();
    let cut = openings::cut_mesh(&body, &inserts::cuts_by_host(doc.store())[&w]);
    let lost = volume(&whole.0, &whole.1) - volume(&cut.0, &cut.1);
    let exact = std::f64::consts::PI * 0.45 * 0.45 * 0.2;
    assert!((lost - exact).abs() / exact < 0.005, "{lost} vs {exact}");

    // A plan cut at 1.2 m shows an opening 2·√(0.45² − 0.3²) ≈ 0.67 m wide.
    let half = cut_half_width(&cut.0, &cut.1, 2.5, 1.2);
    let expected = (0.45f64 * 0.45 - 0.3 * 0.3).sqrt();
    assert!(
        (half - expected).abs() / expected < 0.005,
        "{} vs {}",
        2.0 * half,
        2.0 * expected
    );

    // The window's own frame and glass are closed meshes in the opening.
    for (mesh, _) in p.meshes() {
        assert!(volume(&mesh.0, &mesh.1) > 0.0);
    }

    // IFC: the opening is a circle extruded through the wall.
    let path = dir.path().join("shaped.ifc");
    project.save(&doc).unwrap();
    assert_eq!(rows(&path, "IFCCIRCLEPROFILEDEF"), 1);
    // IfcOpenShell cuts the wall by it, and draws the window's frame and glass.
    let meshes = IfcModel::open(&path).unwrap().tessellate(1).unwrap();
    let mesh_volume = |class: &str| -> f64 {
        meshes
            .iter()
            .filter(|m| m.class == class)
            .map(|m| {
                let p: Vec<[f64; 3]> = m.verts.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                volume(&p, &m.faces)
            })
            .sum()
    };
    let wall_volume = mesh_volume("IfcWall");
    let expected = 5.0 * 3.0 * 0.2 - exact;
    assert!(
        (wall_volume - expected).abs() / expected < 0.005,
        "{wall_volume} vs {expected}"
    );
    assert!(mesh_volume("IfcWindow") > 0.0);
    // Saving again rewrites nothing.
    let count = IfcModel::open(&path).unwrap().entity_count();
    project.save(&doc).unwrap();
    assert_eq!(IfcModel::open(&path).unwrap().entity_count(), count);
    // Back to a rectangle: boxes again, and nothing of the circle left behind.
    run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": round.0, "param": "shape", "value": "rectangle" }),
    )
    .unwrap();
    project.save(&doc).unwrap();
    assert_eq!(rows(&path, "IFCCIRCLEPROFILEDEF"), 0);
}

/// The number of `class` rows (upper case) in a saved file.
fn rows(path: &Path, class: &str) -> usize {
    IfcModel::open(path)
        .unwrap()
        .step_rows()
        .unwrap()
        .iter()
        .filter(|r| r.class == class)
        .count()
}

/// Half the opening's width at height `z` around `centre`: the nearest point of the cut mesh's
/// section there.
fn cut_half_width(p: &[[f64; 3]], i: &[u32], centre: f64, z: f64) -> f64 {
    crossings(p, i, z)
        .iter()
        .map(|q| (q[0] - centre).abs())
        .fold(f64::MAX, f64::min)
}

#[test]
fn a_semicircular_round_top_springs_below_its_arch_and_fits_by_its_proportions() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = new_project(dir.path());
    let r = registry();
    let arched = shaped_type(
        &r,
        &mut doc,
        "window",
        "Arched 1200 x 2000",
        &[
            ("shape", json!("Round Top")),
            ("width", json!(1.2)),
            ("height", json!(2.0)),
            ("default_sill", json!(0.5)),
        ],
    );
    let t = inserts::insert_type(doc.store().get(arched).unwrap()).unwrap();
    assert!((inserts::shape::spring_height(&t, 1.2, 2.0) - 1.4).abs() < 1e-12);
    let w = wall(&r, &mut doc, 5.0);
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "type_id": arched.0, "host_wall_id": w.0, "point_along": 2.5 }),
    )
    .unwrap());
    let p = placed(&doc, win);
    // The wall loses the rectangle below the spring and the half circle above it.
    let body = walls::solve(doc.store()).bodies[&w];
    let whole = body.mesh();
    let cut = openings::cut_mesh(&body, &inserts::cuts_by_host(doc.store())[&w]);
    let lost = volume(&whole.0, &whole.1) - volume(&cut.0, &cut.1);
    let exact = (1.2 * 1.4 + std::f64::consts::PI * 0.36 / 2.0) * 0.2;
    assert!((lost - exact).abs() / exact < 0.005, "{lost} vs {exact}");
    assert!(p.meshes().iter().all(|(m, _)| volume(&m.0, &m.1) > 0.0));

    // Too tall for a 1.8 m high wall above its sill... it gives up spring height first.
    let low = wall(&r, &mut doc, 5.0);
    run(
        &r,
        &mut doc,
        "wall.set_constraints",
        json!({ "wall_id": low.0, "top_constraint": { "unconnected": 2.0 } }),
    )
    .unwrap();
    let fitted = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "type_id": arched.0, "host_wall_id": low.0, "point_along": 2.5 }),
    )
    .unwrap());
    let f = placed(&doc, fitted);
    assert!(
        (f.ty.height - 1.5).abs() < 1e-9 && (f.ty.width - 1.2).abs() < 1e-9,
        "{:?}",
        (f.ty.width, f.ty.height)
    );
}

#[test]
fn a_round_window_fitted_into_a_narrow_wall_keeps_its_circle() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = new_project(dir.path());
    let r = registry();
    let round = shaped_type(
        &r,
        &mut doc,
        "window",
        "Oculus 900",
        &[("shape", json!("round")), ("width", json!(0.9))],
    );
    let w = wall(&r, &mut doc, 0.6);
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "type_id": round.0, "host_wall_id": w.0, "point_along": 0.3 }),
    )
    .unwrap());
    let p = placed(&doc, win);
    assert!(
        (p.ty.width - 0.6).abs() < 1e-9 && (p.ty.height - 0.6).abs() < 1e-9,
        "{:?}",
        (p.ty.width, p.ty.height)
    );
}

#[test]
fn doors_take_arched_heads_but_not_round_outlines() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = new_project(dir.path());
    let r = registry();
    let gothic = shaped_type(
        &r,
        &mut doc,
        "door",
        "Gothic Door",
        &[
            ("shape", json!("round_top")),
            ("head_profile", json!("gothic")),
            ("height", json!(2.6)),
        ],
    );
    let err = run(
        &r,
        &mut doc,
        "type.set_param",
        json!({ "type_id": gothic.0, "param": "shape", "value": "round" }),
    )
    .unwrap_err();
    assert!(err.to_string().contains("doors"), "{err}");
    let w = wall(&r, &mut doc, 5.0);
    let door = id(run(
        &r,
        &mut doc,
        "door.place",
        json!({ "type_id": gothic.0, "host_wall_id": w.0, "point_along": 2.5 }),
    )
    .unwrap());
    let p = placed(&doc, door);
    let meshes = p.meshes();
    assert_eq!(meshes.len(), 2, "a frame and a panel");
    for (m, _) in &meshes {
        assert!(volume(&m.0, &m.1) > 0.0);
    }
    // The opening reaches the floor: the wall's bottom has a gap.
    let c = p.cut();
    assert!(
        c.z_at(2.5)
            .is_some_and(|z| z[0].abs() < 1e-9 && (z[1] - 2.6).abs() < 1e-9)
    );
}
