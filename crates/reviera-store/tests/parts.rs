// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parts: creating, dividing, merging, excluding and their properties (ARC-PART-001 … 004).

use reviera_core::{Caller, CommandRegistry, Document, ElementId, parts};
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

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array().unwrap().iter().map(id).collect()
}

struct Setup {
    t: tempfile::TempDir,
    project: LocalProject,
    doc: Document,
    r: CommandRegistry,
    level: ElementId,
    ty: ElementId,
}

const LAYERS: &str = r#"[
    { "function": "finish1", "material": "Brick", "thickness": 0.1 },
    { "function": "thermal_air", "material": "Insulation", "thickness": 0.05 },
    { "function": "structure", "material": "Block", "thickness": 0.1 },
    { "function": "finish2", "material": "Plaster", "thickness": 0.015 }
]"#;

fn setup() -> Setup {
    let t = tempfile::tempdir().unwrap();
    let (project, mut doc) = LocalProject::new(Schema::Ifc4, &t.path().join("p.ifc")).unwrap();
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
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
    let layers: Value = serde_json::from_str(LAYERS).unwrap();
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": ty.0, "layers": layers }),
    )
    .unwrap();
    Setup {
        t,
        project,
        doc,
        r,
        level,
        ty,
    }
}

fn wall(s: &mut Setup, start: [f64; 2], end: [f64; 2]) -> ElementId {
    id(&run(
        &s.r,
        &mut s.doc,
        "wall.create",
        json!({ "curve": { "line": { "start": start, "end": end } }, "base_level": s.level.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap())
}

fn body_volume(doc: &Document, e: ElementId) -> f64 {
    let b = reviera_core::bodies::all(doc.store());
    parts::volume(
        &b.iter()
            .filter(|b| b.element == e)
            .flat_map(|b| b.parts.iter().map(|p| p.mesh.clone()))
            .collect::<Vec<_>>(),
    )
}

fn info(s: &mut Setup, p: ElementId) -> Value {
    run(&s.r, &mut s.doc, "parts.info", json!({ "part_id": p.0 })).unwrap()
}

#[test]
fn a_wall_with_a_window_makes_four_layer_parts() {
    let mut s = setup();
    let w = wall(&mut s, [0.0, 0.0], [6.0, 0.0]);
    run(
        &s.r,
        &mut s.doc,
        "window.place",
        json!({ "host_wall_id": w.0, "point_along": 3.0 }),
    )
    .unwrap();
    let whole = body_volume(&s.doc, w);
    let made = ids(&run(
        &s.r,
        &mut s.doc,
        "parts.create",
        json!({ "host_ids": [w.0] }),
    )
    .unwrap());
    assert_eq!(made.len(), 4);
    let mut sum = 0.0;
    for (p, t) in made.iter().zip([0.1, 0.05, 0.1, 0.015]) {
        let v = info(&mut s, *p)["volume"].as_f64().unwrap();
        // Each has the opening cut.
        assert!(v < t * 6.0 * 3.0 - 1e-3, "{v}");
        sum += v;
    }
    assert!((sum - whole).abs() / whole < 1e-4, "{sum} vs {whole}");
    // The opening stays open: the slices' caps have holes, so the layer faces inside the wall
    // add up to two faces per inner boundary without the opening's area.
    let b = reviera_core::bodies::all(s.doc.store());
    let area_at = |y: f64| -> f64 {
        made.iter()
            .flat_map(|p| b.iter().filter(move |x| x.element == *p))
            .flat_map(|x| x.parts.iter())
            .map(|p| {
                let (pts, idx) = &p.mesh;
                idx.chunks(3)
                    .filter(|c| c.iter().all(|i| (pts[*i as usize][1] - y).abs() < 1e-9))
                    .map(|c| {
                        let [a, b, c] =
                            [pts[c[0] as usize], pts[c[1] as usize], pts[c[2] as usize]];
                        ((b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2])).abs() / 2.0
                    })
                    .sum::<f64>()
            })
            .sum()
    };
    let g = reviera_core::walls::regenerate(s.doc.store(), s.doc.store().get(w).unwrap()).unwrap();
    let half = g.width / 2.0;
    let y = g.start[1] + half - 0.1; // the brick/insulation boundary
    let face = area_at(y);
    assert!(face < 2.0 * 6.0 * 3.0 - 0.5 && face > 30.0, "{face}");
    let i = info(&mut s, made[1]);
    assert_eq!(i["material"], "Insulation");
    assert_eq!(i["construction"], "Finish");
    assert_eq!(info(&mut s, made[2])["construction"], "Core");
    assert_eq!(
        s.doc
            .store()
            .get(made[1])
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "INSULATION"
    );
    // Again: nothing eligible.
    assert_eq!(
        run(
            &s.r,
            &mut s.doc,
            "parts.create",
            json!({ "host_ids": [w.0] })
        )
        .unwrap_err()
        .code,
        "parts.none"
    );

    // Block 100 → 140 mm: the same parts, the block grown.
    let block = info(&mut s, made[2])["volume"].as_f64().unwrap();
    let mut layers: Value = serde_json::from_str(LAYERS).unwrap();
    layers[2]["thickness"] = json!(0.14);
    run(
        &s.r,
        &mut s.doc,
        "compound.set_layers",
        json!({ "type_id": s.ty.0, "layers": layers }),
    )
    .unwrap();
    let now = info(&mut s, made[2])["volume"].as_f64().unwrap();
    assert!((now / block - 1.4).abs() < 1e-6, "{block} → {now}");

    // A type without the plaster: its part goes, with a warning.
    layers.as_array_mut().unwrap().pop();
    let out =
        s.r.execute(
            &mut s.doc,
            "compound.set_layers",
            json!({ "type_id": s.ty.0, "layers": layers }),
            &Caller::user(),
        )
        .unwrap();
    assert!(out.warnings.iter().any(|w| w.id == "parts.layer_gone"));
    assert!(s.doc.store().get(made[3]).is_none());
    assert!(s.doc.store().get(made[2]).is_some());

    // The IFC: parts aggregated by the wall.
    s.project.save(&s.doc).unwrap();
    let text = std::fs::read_to_string(s.t.path().join("p.ifc")).unwrap();
    assert_eq!(text.matches("IFCBUILDINGELEMENTPART(").count(), 3);
    assert!(text.contains("IFCRELAGGREGATES("));
}

#[test]
fn dividing_with_a_gap_and_by_levels() {
    let mut s = setup();
    let w = wall(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let made = ids(&run(
        &s.r,
        &mut s.doc,
        "parts.create",
        json!({ "host_ids": [w.0] }),
    )
    .unwrap());
    let brick = made[0];
    // Three lines across at 2.5, 5 and 7.5 m, swept along the wall's face normal, 10 mm gap.
    let lines: Vec<Value> = [2.5, 5.0, 7.5]
        .iter()
        .map(|x| json!({ "start": [x, 0.0, 0.0], "end": [x, 0.0, 3.0] }))
        .collect();
    let d = run(
        &s.r,
        &mut s.doc,
        "parts.divide",
        json!({ "part_ids": [brick.0], "lines": lines, "gap": 0.01 }),
    )
    .unwrap();
    let pieces = ids(&d["parts"]);
    assert_eq!(pieces.len(), 4);
    let b = reviera_core::bodies::all(s.doc.store());
    let mut lengths: Vec<f64> = pieces
        .iter()
        .map(|p| {
            let (lo, hi) = b
                .iter()
                .find(|x| x.element == *p)
                .unwrap()
                .bounds()
                .unwrap();
            hi[0] - lo[0]
        })
        .collect();
    lengths.sort_by(f64::total_cmp);
    for (l, want) in lengths.iter().zip([2.49, 2.49, 2.495, 2.495]) {
        assert!((l - want).abs() < 1e-9, "{lengths:?}");
    }
    // The brick part is kept but not drawn.
    assert!(b.iter().all(|x| x.element != brick));
    assert_eq!(info(&mut s, brick)["intermediate"], true);

    // Deleting one piece deletes all four and shows the brick part again; deleting that
    // removes every part of the wall.
    let gone = run(
        &s.r,
        &mut s.doc,
        "element.delete",
        json!({ "element_ids": [pieces[1].0], "confirm_dependents": true }),
    )
    .unwrap();
    assert_eq!(gone.as_array().unwrap().len(), 4);
    assert!(
        reviera_core::bodies::all(s.doc.store())
            .iter()
            .any(|x| x.element == brick)
    );
    let gone = run(
        &s.r,
        &mut s.doc,
        "element.delete",
        json!({ "element_ids": [brick.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert_eq!(gone.as_array().unwrap().len(), 4);
    assert!(!s.doc.store().iter().any(|e| parts::recipe(e).is_some()));

    // By levels: a level at 1.5 m with +50 mm cuts the block there, and follows the level.
    let made = ids(&run(
        &s.r,
        &mut s.doc,
        "parts.create",
        json!({ "host_ids": [w.0] }),
    )
    .unwrap());
    let l2 = id(&run(
        &s.r,
        &mut s.doc,
        "level.create",
        json!({ "elevation": 1.5, "name": "Mid" }),
    )
    .unwrap());
    let d = run(
        &s.r,
        &mut s.doc,
        "parts.divide",
        json!({ "part_ids": [made[2].0], "refs": [{ "ref_id": l2.0, "offset": 0.05 }] }),
    )
    .unwrap();
    let pieces = ids(&d["parts"]);
    let low = |s: &Setup| {
        reviera_core::bodies::all(s.doc.store())
            .iter()
            .filter(|x| pieces.contains(&x.element))
            .map(|x| x.bounds().unwrap().1[2])
            .fold(f64::MAX, f64::min)
    };
    assert!((low(&s) - 1.55).abs() < 1e-9);
    run(
        &s.r,
        &mut s.doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 1.8 }),
    )
    .unwrap();
    assert!((low(&s) - 1.85).abs() < 1e-9, "{}", low(&s));
    s.project.save(&s.doc).unwrap();
}

#[test]
fn merging_excluding_overrides_and_visibility() {
    let mut s = setup();
    let w = wall(&mut s, [0.0, 0.0], [6.0, 0.0]);
    let made = ids(&run(
        &s.r,
        &mut s.doc,
        "parts.create",
        json!({ "host_ids": [w.0] }),
    )
    .unwrap());
    let plaster = made[3];
    let lines: Vec<Value> = [2.0, 4.0]
        .iter()
        .map(|x| json!({ "start": [x, 0.0, 0.0], "end": [x, 0.0, 3.0] }))
        .collect();
    let pieces = ids(&run(
        &s.r,
        &mut s.doc,
        "parts.divide",
        json!({ "part_ids": [plaster.0], "lines": lines }),
    )
    .unwrap()["parts"]);
    let total: f64 = pieces
        .iter()
        .map(|p| info(&mut s, *p)["volume"].as_f64().unwrap())
        .sum();
    let m = id(&run(
        &s.r,
        &mut s.doc,
        "parts.merge",
        json!({ "part_ids": pieces.iter().map(|p| p.0).collect::<Vec<_>>() }),
    )
    .unwrap());
    let v = info(&mut s, m)["volume"].as_f64().unwrap();
    assert!((v - total).abs() < 1e-9);
    // Brick and block: refused.
    assert_eq!(
        run(
            &s.r,
            &mut s.doc,
            "parts.merge",
            json!({ "part_ids": [made[0].0, made[2].0] })
        )
        .unwrap_err()
        .code,
        "parts.merge_material"
    );
    // Excluded: not drawn or exported, restored with the same element.
    run(
        &s.r,
        &mut s.doc,
        "parts.exclude",
        json!({ "part_ids": [made[1].0] }),
    )
    .unwrap();
    assert!(
        reviera_core::bodies::all(s.doc.store())
            .iter()
            .all(|b| b.element != made[1])
    );
    run(
        &s.r,
        &mut s.doc,
        "parts.restore",
        json!({ "part_ids": [made[1].0] }),
    )
    .unwrap();
    assert!(
        reviera_core::bodies::all(s.doc.store())
            .iter()
            .any(|b| b.element == made[1])
    );
    // Material override on one block part only.
    run(
        &s.r,
        &mut s.doc,
        "parts.set_overrides",
        json!({ "part_ids": [made[2].0], "material": "Lime render" }),
    )
    .unwrap();
    assert_eq!(info(&mut s, made[2])["material"], "Lime render");
    assert_eq!(info(&mut s, made[2])["material_by_original"], false);
    // A part cannot move on its own; with its wall it goes along.
    assert_eq!(
        run(
            &s.r,
            &mut s.doc,
            "element.move",
            json!({ "element_ids": [made[0].0], "vector": [1.0, 0.0, 0.0] })
        )
        .unwrap_err()
        .code,
        "parts.cannot_move"
    );
    run(
        &s.r,
        &mut s.doc,
        "element.move",
        json!({ "element_ids": [w.0, made[0].0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    // Visibility: Show Parts hides the wall, Show Original the parts.
    let v = run(
        &s.r,
        &mut s.doc,
        "view.create_plan",
        json!({ "kind": "floor_plan", "level_ids": [s.level.0] }),
    )
    .unwrap();
    let view = ids(&v)[0];
    assert!(parts::hidden_in(s.doc.store(), Some(view)).contains(&w));
    run(
        &s.r,
        &mut s.doc,
        "view.set_parts_visibility",
        json!({ "view_id": view.0, "mode": "show_original" }),
    )
    .unwrap();
    let h = parts::hidden_in(s.doc.store(), Some(view));
    assert!(h.contains(&made[0]) && !h.contains(&w));
    run(
        &s.r,
        &mut s.doc,
        "view.set_parts_visibility",
        json!({ "view_id": view.0, "mode": "show_both" }),
    )
    .unwrap();
    assert!(parts::hidden_in(s.doc.store(), Some(view)).is_empty());
    // A stacked or curtain wall is not eligible; a floor gives one part.
    let cw = id(&run(&s.r, &mut s.doc, "curtain.create", json!({ "curve": { "start": [0.0, 10.0], "end": [5.0, 10.0] }, "base_level": s.level.0, "top_constraint": { "unconnected": 3.0 } })).unwrap());
    assert_eq!(
        run(
            &s.r,
            &mut s.doc,
            "parts.create",
            json!({ "host_ids": [cw.0] })
        )
        .unwrap_err()
        .code,
        "parts.none"
    );
    s.project.save(&s.doc).unwrap();
}
