// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel modifiers and parametric cuts (STR-STL-008): a secondary beam coped at a girder loses
//! the girder's top flange zone and follows a deeper girder; Uncope restores it, deletes the empty
//! sub-connection and is undone in one step; a miter; shorten and extend; a skewed cope; corner
//! and contour cuts on a plate; cut by another element; IFC openings voiding their elements.

use reviera_core::bodies::Mesh;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
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

/// An element's body after booleans and cuts.
fn body(doc: &Document, e: ElementId) -> Vec<Mesh> {
    let mut bodies = reviera_core::bodies::all(doc.store());
    reviera_geom::csg::apply(&Default::default(), doc.store(), &mut bodies);
    bodies
        .into_iter()
        .find(|b| b.element == e)
        .map(|b| b.parts.into_iter().map(|p| p.mesh).collect())
        .unwrap_or_default()
}

fn volume(ms: &[Mesh]) -> f64 {
    ms.iter()
        .map(|(p, i)| {
            i.chunks(3)
                .map(|t| {
                    let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
                })
                .sum::<f64>()
        })
        .sum::<f64>()
        .abs()
}

fn points(ms: &[Mesh]) -> Vec<[f64; 3]> {
    ms.iter().flat_map(|m| m.0.iter().copied()).collect()
}

const IN: f64 = 0.0254;

#[test]
fn copes_miters_shortening_and_plate_cuts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cuts.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w16 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W16X26" }));
    let w12 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W12X26" }));
    let beam = |doc: &mut Document, ty: &Value, a: [f64; 2], b: [f64; 2]| {
        id(run(&r, doc, "beam.create", json!({ "type_id": ty, "curve": { "start": a, "end": b }, "reference_level": l2.0 })))
    };
    let girder = beam(&mut doc, &w16, [0.0, 0.0], [6.0, 0.0]);
    let joist = beam(&mut doc, &w12, [3.0, 0.0], [3.0, 5.0]);

    // The cope: the joist's end loses the girder's top flange zone (8.8 mm flange, 13 mm
    // clearance) within the flange width (5.5") plus the clearance; the joist is cut, not the
    // girder.
    // The joist stops clear of the girder's flanges; bring its end to 13 mm off the web (an
    // extension through Shorten), as a coped joist is.
    let start_y = |doc: &Document| points(&body(doc, joist)).into_iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let extend = start_y(&doc) - (0.25 * IN / 2.0 + 0.013);
    run(&r, &mut doc, "steel_modifier.shorten", json!({ "member_id": joist.0, "end": "start", "length": -extend }));
    assert!((start_y(&doc) - (0.25 * IN / 2.0 + 0.013)).abs() < 1e-6);
    let v0 = volume(&body(&doc, joist));
    let g0 = volume(&body(&doc, girder));
    let made = run(&r, &mut doc, "steel_cut.cope", json!({ "member_ids": [girder.0, joist.0], "clearance": 0.013 }));
    assert_eq!(made["cuts"].as_array().unwrap().len(), 1);
    let sub = id(made["sub_connection"].clone());
    let near = |doc: &Document, reach: f64| {
        points(&body(doc, joist))
            .into_iter()
            .filter(|p| p[1] < reach - 1e-4)
            .map(|p| p[2])
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let top = 4.0 - 0.345 * IN - 0.013;
    let reach = 5.5 * IN / 2.0 + 0.013;
    assert!(near(&doc, reach) <= top + 1e-6, "{} > {top}", near(&doc, reach));
    assert!(volume(&body(&doc, joist)) < v0 - 1e-5);
    assert!((volume(&body(&doc, girder)) - g0).abs() < 1e-9, "the girder is not cut");
    // A deeper girder (W18X35, a 0.425" flange, 6" wide): the cope follows.
    let w18 = run(&r, &mut doc, "beam.load_section", json!({ "catalog": "aisc_w", "designation": "W18X35" }));
    run(&r, &mut doc, "element.set_type", json!({ "element_ids": [girder.0], "type_id": w18 }));
    let top18 = 4.0 - 0.425 * IN - 0.013;
    assert!(near(&doc, 6.0 * IN / 2.0 + 0.013) <= top18 + 1e-6, "follows the deeper girder");
    assert!(near(&doc, 6.0 * IN / 2.0 + 0.013) < top - 1e-4);

    // Uncope restores the end and deletes the empty sub-connection, in one undo step.
    let report = run(&r, &mut doc, "steel_cut.uncope", json!({ "member_ids": [joist.0] }));
    assert_eq!(report["removed"].as_array().unwrap().len(), 1);
    assert_eq!(report["sub_connections_deleted"], json!([sub.0]));
    assert!(doc.store().get(sub).is_none());
    let uncut: Vec<Mesh> = reviera_core::bodies::all(doc.store())
        .into_iter()
        .find(|b| b.element == joist)
        .map(|b| b.parts.into_iter().map(|p| p.mesh).collect())
        .unwrap();
    assert!((volume(&body(&doc, joist)) - volume(&uncut)).abs() < 1e-9, "the full end is back");
    run(&r, &mut doc, "edit.undo", json!({}));
    let copes = |doc: &mut Document| {
        run(&r, doc, "steel_cut.list", json!({ "element_ids": [joist.0] }))
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["cut"]["kind"] == "cope")
            .count()
    };
    assert_eq!(copes(&mut doc), 1, "undone in one step");
    assert!(doc.store().get(sub).is_some());
    run(&r, &mut doc, "edit.redo", json!({}));
    let none = r.execute(&mut doc, "steel_cut.uncope", json!({ "member_ids": [joist.0] }), &Caller::user()).unwrap_err();
    assert_eq!(none.code, "uncope.nothing");

    // A miter at a corner: each beam keeps its side of the bisecting plane.
    let a = beam(&mut doc, &w12, [10.0, 0.0], [12.0, 0.0]);
    let b = beam(&mut doc, &w12, [12.0, 0.0], [12.0, 2.0]);
    let m = run(&r, &mut doc, "steel_cut.miter", json!({ "a": a.0, "b": b.0 }));
    assert_eq!(m["cuts"].as_array().unwrap().len(), 2);
    let n = [std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2];
    for p in points(&body(&doc, a)) {
        // a keeps its side of the plane through the joint (12, 0) halving the corner.
        assert!((p[0] - 12.0) * n[0] + p[1] * n[1] <= 1e-6, "{p:?}");
    }
    for p in points(&body(&doc, b)) {
        assert!((p[0] - 12.0) * n[0] + p[1] * n[1] >= -1e-6, "{p:?}");
    }

    // Shorten the joist's far end by 200 mm, then extend it 100 mm instead.
    let len = |doc: &mut Document| run(&r, doc, "beam.info", json!({ "beam_ids": [joist.0] }))[0]["length"].as_f64().unwrap();
    let l0 = len(&mut doc);
    let s = run(&r, &mut doc, "steel_modifier.shorten", json!({ "member_id": joist.0, "end": "end", "length": 0.2 }));
    assert!((len(&mut doc) - (l0 - 0.2)).abs() < 1e-9);
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [s["cuts"][0]] }));
    run(&r, &mut doc, "steel_modifier.shorten", json!({ "member_id": joist.0, "end": "end", "length": -0.1, "angle_h": 30.0 }));
    assert!((len(&mut doc) - (l0 + 0.1)).abs() < 1e-9);

    // A skewed cope on a free beam's end: 50 mm deep, 120 mm long.
    let c = beam(&mut doc, &w12, [20.0, 0.0], [23.0, 0.0]);
    let vc = volume(&body(&doc, c));
    run(&r, &mut doc, "steel_modifier.cope_skewed", json!({ "member_id": c.0, "end": "start", "depth": 0.05, "length": 0.12 }));
    let high = points(&body(&doc, c)).into_iter().filter(|p| p[0] < 20.12 - 1e-4).map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!(high <= 4.0 - 0.05 + 1e-6, "{high}");
    assert!(volume(&body(&doc, c)) < vc);

    // A plate: a straight corner cut takes a 50 mm triangle, a contour cut a 50 mm square hole.
    let plate = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "plane": { "origin": [30.0, 0.0, 0.0], "x_dir": [1.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0] },
                "contour": [[0.0, 0.0], [0.3, 0.0], [0.3, 0.2], [0.0, 0.2]], "thickness": 0.01 }),
    ));
    let area = |doc: &mut Document| run(&r, doc, "plate.info", json!({ "element_ids": [plate.0] }))[0]["area"].as_f64().unwrap();
    run(&r, &mut doc, "steel_modifier.corner_cut", json!({ "plate_id": plate.0, "corner": 0, "kind": "straight", "size": 0.05 }));
    assert!((area(&mut doc) - (0.06 - 0.00125)).abs() < 1e-9);
    run(&r, &mut doc, "steel_modifier.corner_cut", json!({ "plate_id": plate.0, "corner": 2, "kind": "convex", "size": 0.05 }));
    let rounded = 0.06 - 0.00125 - 0.0025 * (1.0 - std::f64::consts::FRAC_PI_4);
    assert!((area(&mut doc) - rounded).abs() < 2e-5, "{}", area(&mut doc));
    let vp = volume(&body(&doc, plate));
    run(
        &r,
        &mut doc,
        "steel_modifier.contour_cut",
        json!({ "element_id": plate.0, "face": { "element": plate.0, "face": "top" },
                "contour": [[0.125, 0.075], [0.175, 0.075], [0.175, 0.125], [0.125, 0.125]] }),
    );
    assert!((volume(&body(&doc, plate)) - (vp - 0.05 * 0.05 * 0.01)).abs() < 1e-7);

    // Cut by: a second plate crossing the first takes its own shape out of it.
    let crossing = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "plane": { "origin": [30.25, 0.0, -0.05], "x_dir": [0.0, 1.0, 0.0], "normal": [1.0, 0.0, 0.0] },
                "contour": [[0.02, 0.0], [0.06, 0.0], [0.06, 0.1], [0.02, 0.1]], "thickness": 0.01, "justification": "centre" }),
    ));
    let vp = volume(&body(&doc, plate));
    run(&r, &mut doc, "steel_cut.cut_by", json!({ "cut_id": plate.0, "cutter_id": crossing.0 }));
    assert!((volume(&body(&doc, plate)) - (vp - 0.01 * 0.04 * 0.01)).abs() < 1e-7);

    // IFC: an opening for each cut with a body, voiding its element.
    let cuts = run(&r, &mut doc, "steel_cut.list", json!({}));
    let with_body = cuts
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| !(c["cut"]["kind"] == "shorten" && c["cut"]["angle_h"] == 0.0))
        .count();
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCOPENINGELEMENT"), cuts.as_array().unwrap().len());
    assert_eq!(n("IFCRELVOIDSELEMENT"), with_body);
    assert!(text.contains("'COPE_SKEWED'") && text.contains("'MITER'") && text.contains("'SUB_CONNECTION'"));

    // Deleting the girder deletes its cope; deleting a mitered beam deletes both miters' partner
    // cut on it and the cut made from it.
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [girder.0] }));
    assert!(run(&r, &mut doc, "steel_cut.list", json!({ "element_ids": [joist.0] }))
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["cut"]["kind"] != "cope"));
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [a.0] }));
    assert!(run(&r, &mut doc, "steel_cut.list", json!({ "element_ids": [b.0] })).as_array().unwrap().is_empty());
}
