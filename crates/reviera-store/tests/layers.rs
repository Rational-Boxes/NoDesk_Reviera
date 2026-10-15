// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Regions, extendable layers and wrapping (ARC-CMPD-003/004, ARC-WRAP-001).

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| m.0[t[k] as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

/// A wall's parts: (class, z range, volume).
fn parts(doc: &Document, w: ElementId) -> Vec<(String, [f64; 2], f64)> {
    reviera_core::bodies::all(doc.store())
        .into_iter()
        .filter(|b| b.element == w)
        .flat_map(|b| b.parts)
        .map(|p| {
            let z = p.mesh.0.iter().fold([f64::MAX, f64::MIN], |a, q| {
                [a[0].min(q[2]), a[1].max(q[2])]
            });
            (p.class.clone(), z, volume(&p.mesh))
        })
        .collect()
}

struct Setup {
    _t: tempfile::TempDir,
    project: LocalProject,
    doc: Document,
    r: CommandRegistry,
    level: ElementId,
    ty: ElementId,
}

fn setup(layers: Value) -> Setup {
    let t = tempfile::tempdir().unwrap();
    let (project, mut doc) = LocalProject::new(Schema::Ifc4, &t.path().join("w.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A first wall makes the default type.
    let w = id(&run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 50.0], "end": [5.0, 50.0] } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [w.0] }),
    )
    .unwrap();
    let ty = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .unwrap()
        .id();
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": ty.0, "layers": layers }),
    )
    .unwrap();
    Setup {
        _t: t,
        project,
        doc,
        r,
        level,
        ty,
    }
}

fn wall(s: &mut Setup, y: f64, height: f64) -> ElementId {
    id(&run(
        &s.r,
        &mut s.doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, y], "end": [6.0, y] } }, "base_level": s.level.0, "top_constraint": { "unconnected": height } }),
    )
    .unwrap())
}

#[test]
fn a_brick_plinth_region_and_extensions() {
    let mut s = setup(json!([
        { "function": "finish1", "material": "Render", "thickness": 0.02 },
        { "function": "thermal_air", "material": "Insulation", "thickness": 0.1 },
        { "function": "structure", "material": "Concrete", "thickness": 0.2 },
    ]));
    run(
        &s.r,
        &mut s.doc,
        "compound.split_region",
        json!({ "type_id": s.ty.0, "layer": 0, "from": "base", "distance": 0.6, "material": "Brick" }),
    )
    .unwrap();
    // Brick from 0 to 600 mm on a 3 m and a 1 m wall; clipped at 0.5 m on a 0.5 m wall.
    for (y, h, top) in [(0.0, 3.0, 0.6), (4.0, 1.0, 0.6), (8.0, 0.5, 0.5)] {
        let w = wall(&mut s, y, h);
        let p = parts(&s.doc, w);
        let brick: Vec<_> = p.iter().filter(|x| x.0 == "IfcWall:layer/Brick").collect();
        assert_eq!(brick.len(), 1, "{p:?}");
        assert!(
            (brick[0].1[0]).abs() < 1e-9 && (brick[0].1[1] - top).abs() < 1e-9,
            "{:?}",
            brick[0]
        );
        let render = p.iter().any(|x| x.0 == "IfcWall:layer/Render");
        assert_eq!(render, h > 0.6, "{p:?}");
    }
    // Unlocking the bottoms of layers 1 and 3 but not 2 is refused, naming them.
    run(
        &s.r,
        &mut s.doc,
        "compound.set_extendable",
        json!({ "type_id": s.ty.0, "index": 0, "base": true }),
    )
    .unwrap();
    let e = run(
        &s.r,
        &mut s.doc,
        "compound.set_extendable",
        json!({ "type_id": s.ty.0, "index": 2, "base": true }),
    )
    .unwrap_err();
    assert_eq!(e.code, "compound.extendable_adjacent");
    assert!(e.message.contains("1, 3"), "{}", e.message);
    // Base Extension −300 mm: the render (with its brick) drops 300 mm, the rest stays.
    let w = wall(&mut s, 12.0, 3.0);
    run(
        &s.r,
        &mut s.doc,
        "wall.set_extension",
        json!({ "wall_ids": [w.0], "base": -0.3 }),
    )
    .unwrap();
    let p = parts(&s.doc, w);
    let lowest = |m: &str| {
        p.iter()
            .filter(|x| x.0.ends_with(m))
            .map(|x| x.1[0])
            .fold(f64::MAX, f64::min)
    };
    assert!((lowest("/Brick") + 0.3).abs() < 1e-9, "{p:?}");
    assert!(lowest("/Concrete").abs() < 1e-9);
    // No unlocked top edge: the top extension is refused.
    assert_eq!(
        run(
            &s.r,
            &mut s.doc,
            "wall.set_extension",
            json!({ "wall_ids": [w.0], "top": 0.5 })
        )
        .unwrap_err()
        .code,
        "wall.no_extendable"
    );
    s.project.save(&s.doc).unwrap();
}

#[test]
fn wrapping_at_inserts_and_ends_keeps_the_material() {
    let mut s = setup(json!([
        { "function": "finish1", "material": "Render", "thickness": 0.02, "wraps": true },
        { "function": "structure", "material": "Concrete", "thickness": 0.2 },
        { "function": "thermal_air", "material": "Insulation", "thickness": 0.05 },
        { "function": "finish2", "material": "Plasterboard", "thickness": 0.0125, "wraps": true },
    ]));
    let width = 0.02 + 0.2 + 0.05 + 0.0125;
    let w = wall(&mut s, 0.0, 3.0);
    let total = |s: &Setup| parts(&s.doc, w).iter().map(|x| x.2).sum::<f64>();
    let plain = total(&s);
    assert!((plain - width * 6.0 * 3.0).abs() < 1e-6);
    // Ends wrap on the exterior: the render turns round both free ends; nothing is lost.
    run(
        &s.r,
        &mut s.doc,
        "compound.set_wrapping",
        json!({ "type_id": s.ty.0, "at_ends": "exterior" }),
    )
    .unwrap();
    let p = parts(&s.doc, w);
    assert!(
        p.iter().filter(|x| x.0.ends_with("/Render")).count() >= 3,
        "band and two caps: {p:?}"
    );
    assert!((total(&s) - plain).abs() < 1e-6, "{} vs {plain}", total(&s));
    // A window, with interior wrapping at inserts: the plasterboard lines the jambs back to
    // the core; the wall loses only the opening's volume through its whole width.
    run(
        &s.r,
        &mut s.doc,
        "compound.set_wrapping",
        json!({ "type_id": s.ty.0, "at_ends": "none", "at_inserts": "interior" }),
    )
    .unwrap();
    let win = id(&run(
        &s.r,
        &mut s.doc,
        "window.place",
        json!({ "host_wall_id": w.0, "point_along": 3.0 }),
    )
    .unwrap());
    let placed =
        reviera_core::inserts::resolve(s.doc.store(), s.doc.store().get(win).unwrap()).unwrap();
    let c = placed.cut();
    let opening = (c.x[1] - c.x[0]) * (c.z[1] - c.z[0]) * width;
    assert!(
        (total(&s) - (plain - opening)).abs() < 1e-6,
        "{} vs {}",
        total(&s),
        plain - opening
    );
    let boards = parts(&s.doc, w)
        .iter()
        .filter(|x| x.0.ends_with("/Plasterboard"))
        .count();
    assert_eq!(boards, 5, "the band and four jamb linings");
    s.project.save(&s.doc).unwrap();
}
