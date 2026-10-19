// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel detailing by hand (STR-STL-005, STR-STL-006, STR-STL-007): a plate on a column flange
//! sits outside it and moves with it, and stays put when the column goes; an open contour is
//! refused; a 2 × 3 M20 bolt pattern through a 15 mm and a 10 mm plate has a 25 mm grip and six
//! holes in each; shear studs; a fillet weld on an edge where a stiffener meets a plate, refused
//! off it, flagged for review when the plate changes and accepted; IFC plates, fasteners, holes
//! and weld properties; a weld goes with its element.

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

fn fail(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> String {
    r.execute(doc, name, args, &Caller::user()).unwrap_err().code.to_string()
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn v3(v: &Value) -> [f64; 3] {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn rect(w: f64, h: f64) -> Value {
    json!([[-w / 2.0, -h / 2.0], [w / 2.0, -h / 2.0], [w / 2.0, h / 2.0], [-w / 2.0, h / 2.0]])
}

#[test]
fn plates_fastener_patterns_and_welds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("detailing.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));
    let w12 = run(&r, &mut doc, "column.load_section", json!({ "catalog": "aisc_w", "designation": "W12X53" }));
    let col = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": w12, "point": [0.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));

    // A 15 mm plate, bottom-justified on the column's first flange face: outside the flange,
    // its bottom on the face.
    let faces = run(&r, &mut doc, "face.list", json!({ "element_ids": [col.0] }));
    let flange = faces.as_array().unwrap().iter().find(|f| f["face"] == "flange_1").unwrap().clone();
    let (fo, fn_) = (v3(&flange["origin"]), v3(&flange["normal"]));
    let plate = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "face": { "element": col.0, "face": "flange_1" }, "contour": [[1.0, -0.1], [1.4, -0.1], [1.4, 0.1], [1.0, 0.1]],
                "thickness": 0.015, "justification": "bottom", "role": "stiffener" }),
    ));
    let info = run(&r, &mut doc, "plate.info", json!({ "element_ids": [plate.0] }))[0].clone();
    assert_eq!(info["hosted"], true);
    assert!((info["thickness"].as_f64().unwrap() - 0.015).abs() < 1e-12);
    for q in info["outline"].as_array().unwrap() {
        assert!(dot(sub(v3(q), fo), fn_).abs() < 1e-9, "the plate's bottom is on the face");
    }
    // The flange face is half the column's depth from its axis.
    assert!((dot(sub(fo, [0.0, 0.0, fo[2]]), fn_) - 12.06 * 0.0254 / 2.0).abs() < 0.003, "{fo:?}");

    // Bolts through the plate and the flange grip only those two (15 mm and the W12X53's
    // 0.575" flange), not the column's far flange.
    let through = id(run(
        &r,
        &mut doc,
        "bolt_pattern.create",
        json!({ "element_ids": [plate.0, col.0], "face": { "element": col.0, "face": "flange_1" },
                "layout": { "shape": "rect", "rows": 2, "columns": 2, "spacing_x": 0.08, "spacing_y": 0.08, "centre": [1.2, 0.0] } }),
    ));
    let ti = run(&r, &mut doc, "fastener_pattern.info", json!({ "element_ids": [through.0] }))[0].clone();
    assert!((ti["grip"].as_f64().unwrap() - (0.015 + 0.575 * 0.0254)).abs() < 1e-6, "{ti:#}");
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [through.0] }));

    // Moving the column moves the plate.
    let before = v3(&info["outline"][0]);
    run(&r, &mut doc, "element.move", json!({ "element_ids": [col.0], "vector": [1.0, 0.5, 0.0] }));
    let moved = v3(&run(&r, &mut doc, "plate.info", json!({ "element_ids": [plate.0] }))[0]["outline"][0]);
    assert!((moved[0] - before[0] - 1.0).abs() < 1e-9 && (moved[1] - before[1] - 0.5).abs() < 1e-9);

    // A contour with a 2 mm gap is refused, naming the open segment.
    let gap = r
        .execute(
            &mut doc,
            "plate.create",
            json!({ "level": l1.0, "contour": { "segments": [
                { "start": [0.0, 0.0], "end": [0.3, 0.0] },
                { "start": [0.3, 0.0], "end": [0.3, 0.2] },
                { "start": [0.3, 0.2], "end": [0.0, 0.2] },
                { "start": [0.0, 0.2], "end": [0.0, 0.002] }
            ] } }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(gap.code, "plate.open_contour");
    assert_eq!(gap.details["segment"], 3);
    assert_eq!(fail(&r, &mut doc, "plate.create", json!({ "level": l1.0, "contour": [[0, 0], [1, 1], [1, 0], [0, 1]] })), "plate.self_intersecting");
    assert_eq!(fail(&r, &mut doc, "plate.create", json!({ "level": l1.0, "contour": rect(0.2, 0.2), "thickness": 0.3 })), "plate.thickness");

    // A 15 mm plate on Level 1 at x = 5 m, and a 10 mm plate on its top face.
    let p15 = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "plane": { "origin": [5.0, 0.0, 0.0], "x_dir": [1.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0] },
                "contour": rect(0.4, 0.3), "thickness": 0.015 }),
    ));
    let p10 = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "face": { "element": p15.0, "face": "top" }, "contour": rect(0.3, 0.2), "thickness": 0.010 }),
    ));
    // 2 × 3 M20 bolts from the top: a 25 mm grip, six holes in each plate.
    let bolts = id(run(
        &r,
        &mut doc,
        "bolt_pattern.create",
        json!({ "element_ids": [p15.0, p10.0], "face": { "element": p10.0, "face": "top" },
                "layout": { "shape": "rect", "rows": 2, "columns": 3, "spacing_x": 0.07, "spacing_y": 0.07 },
                "diameter": 0.020, "standard": "generic_metric", "hole_tolerance": 0.002 }),
    ));
    let bi = run(&r, &mut doc, "fastener_pattern.info", json!({ "element_ids": [bolts.0] }))[0].clone();
    assert!((bi["grip"].as_f64().unwrap() - 0.025).abs() < 1e-9, "{bi:#}");
    assert_eq!(bi["count"], 6);
    assert_eq!(bi["fasteners"].as_array().unwrap().len(), 6);
    assert_eq!(bi["holes"][p15.0.to_string()], 6);
    assert_eq!(bi["holes"][p10.0.to_string()], 6);
    assert!((bi["hole_diameter"].as_f64().unwrap() - 0.022).abs() < 1e-12);
    // Grip, two washers, a nut and thread: 25 + 20 × 1.4 = 53, up to 55 mm.
    assert!((bi["length"].as_f64().unwrap() - 0.055).abs() < 1e-9, "{bi:#}");
    // Three columns of bolts become four: found again.
    run(
        &r,
        &mut doc,
        "fastener_pattern.set",
        json!({ "pattern_ids": [bolts.0], "layout": { "shape": "rect", "rows": 2, "columns": 4, "spacing_x": 0.06, "spacing_y": 0.07 } }),
    );
    let bi = run(&r, &mut doc, "fastener_pattern.info", json!({ "element_ids": [bolts.0] }))[0].clone();
    assert_eq!(bi["fasteners"].as_array().unwrap().len(), 8);
    assert_eq!(bi["holes"][p15.0.to_string()], 8);
    // The holes are cut in the plates' bodies: eight 22 mm holes (12-sided) through 15 mm.
    let mut bodies = reviera_core::bodies::all(doc.store());
    reviera_geom::csg::apply(&Default::default(), doc.store(), &mut bodies);
    let vol: f64 = bodies
        .iter()
        .filter(|b| b.element == p15)
        .flat_map(|b| b.parts.iter())
        .map(|p| {
            p.mesh.1.chunks(3).map(|t| {
                let (a, b, c) = (p.mesh.0[t[0] as usize], p.mesh.0[t[1] as usize], p.mesh.0[t[2] as usize]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            }).sum::<f64>()
        })
        .sum::<f64>()
        .abs();
    let r = 0.011;
    assert!((vol - (0.4 * 0.3 * 0.015 - 8.0 * 3.0 * r * r * 0.015)).abs() < 1e-8, "{vol}");
    let r = registry();
    // A pattern that misses its elements is refused.
    assert_eq!(
        fail(&r, &mut doc, "bolt_pattern.create", json!({ "element_ids": [p15.0, p10.0], "level": l1.0,
            "layout": { "shape": "circle", "count": 4, "radius": 0.05, "centre": [20.0, 20.0] } })),
        "pattern.misses"
    );

    // Four shear studs on the 15 mm plate's top face, 100 mm long.
    let studs = id(run(
        &r,
        &mut doc,
        "shear_stud_pattern.create",
        json!({ "element_ids": [p15.0], "face": { "element": p15.0, "face": "top" },
                "layout": { "shape": "rect", "rows": 1, "columns": 4, "spacing_x": 0.08, "spacing_y": 0.0, "centre": [0.0, 0.12] } }),
    ));
    let si = run(&r, &mut doc, "fastener_pattern.info", json!({ "element_ids": [studs.0] }))[0].clone();
    assert_eq!(si["fasteners"].as_array().unwrap().len(), 4);
    assert!((si["length"].as_f64().unwrap() - 0.1).abs() < 1e-12);

    // A 10 mm stiffener standing on the 15 mm plate (across x = 5.1 m), and the edges where they
    // touch.
    let stiff = id(run(
        &r,
        &mut doc,
        "plate.create",
        json!({ "plane": { "origin": [5.15, 0.0, 0.015], "x_dir": [0.0, 1.0, 0.0], "normal": [1.0, 0.0, 0.0] },
                "contour": [[-0.1, 0.0], [0.1, 0.0], [0.1, 0.1], [-0.1, 0.1]], "thickness": 0.010, "justification": "centre" }),
    ));
    let edges = run(&r, &mut doc, "weld.edges", json!({ "element_ids": [p15.0, stiff.0] }));
    let edges = edges.as_array().unwrap();
    let edge = edges
        .iter()
        .find(|e| (v3(&e[0])[0] - 5.155).abs() < 1e-6 && (v3(&e[1])[0] - 5.155).abs() < 1e-6)
        .unwrap_or_else(|| panic!("no edge on the stiffener's +x side: {edges:?}"))
        .clone();
    // Off the touching edges: refused.
    assert_eq!(
        fail(&r, &mut doc, "weld.create", json!({ "element_ids": [p15.0, stiff.0], "edge": [[5.2, -0.1, 0.015], [5.2, 0.1, 0.03]] })),
        "weld.not_touching"
    );
    let weld = id(run(
        &r,
        &mut doc,
        "weld.create",
        json!({ "element_ids": [p15.0, stiff.0], "edge": edge, "weld_type": "fillet", "a": 0.006 }),
    ));
    let wi = run(&r, &mut doc, "weld.info", json!({ "element_ids": [weld.0] }))[0].clone();
    assert_eq!(wi["needs_review"], false);
    assert!((wi["length"].as_f64().unwrap() - 0.2).abs() < 1e-9);
    assert_eq!(fail(&r, &mut doc, "weld.create", json!({ "element_ids": [p15.0, stiff.0], "edge": edge, "weld_type": "glue" })), "weld.value");

    // The 15 mm plate grows: the weld stays where it is, flagged for review; accepting clears it.
    run(&r, &mut doc, "plate.set", json!({ "plate_ids": [p15.0], "contour": rect(0.5, 0.3) }));
    let wi = run(&r, &mut doc, "weld.info", json!({ "element_ids": [weld.0] }))[0].clone();
    assert_eq!(wi["needs_review"], true);
    assert_eq!(wi["edge"], edge);
    assert_eq!(run(&r, &mut doc, "weld.review", json!({})), json!([weld.0]));
    run(&r, &mut doc, "weld.set", json!({ "weld_ids": [weld.0], "accept": true }));
    assert_eq!(run(&r, &mut doc, "weld.review", json!({})), json!([]));

    // IFC: the plates, the bolts with their nominal diameter, the studs, 16 holes and the weld.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let n = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(n("IFCPLATE"), 4);
    assert_eq!(n("IFCMECHANICALFASTENER"), 8 + 4);
    assert_eq!(n("IFCOPENINGELEMENT"), 16);
    assert_eq!(n("IFCFASTENER"), 1);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(".STUDSHEARCONNECTOR."));
    assert!(text.contains("'rxd:hole'"));
    assert!(text.contains("'Pset_FastenerWeld'"));
    // The file's lengths are metres, so a = 6 mm is 0.006.
    assert!(text.contains("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)"), "lengths in metres");
    let a = text
        .lines()
        .find(|l| l.contains("IFCPROPERTYSINGLEVALUE('a'"))
        .and_then(|l| l.split("IFCPOSITIVELENGTHMEASURE(").nth(1))
        .and_then(|v| v.split(')').next())
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap();
    assert!((a - 0.006).abs() < 1e-12, "a = 6 mm");
    assert!(text.contains("FASTENER_GROUP"));
    // Saved again: the holes are rewritten, not doubled.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCOPENINGELEMENT").count(), 16);
    drop(project);
    let (_p, mut doc, _) = LocalProject::open(&path).unwrap();
    let bi = run(&r, &mut doc, "fastener_pattern.info", json!({ "element_ids": [bolts.0] }))[0].clone();
    assert_eq!(bi["fasteners"].as_array().unwrap().len(), 8, "kept through save and reopen");

    // Deleting the stiffener deletes its weld; deleting the column leaves its plate where it was.
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [stiff.0] }));
    assert!(doc.store().get(weld).is_none());
    let before = run(&r, &mut doc, "plate.info", json!({ "element_ids": [plate.0] }))[0]["outline"].clone();
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [col.0] }));
    let after = run(&r, &mut doc, "plate.info", json!({ "element_ids": [plate.0] }))[0].clone();
    assert_eq!(after["hosted"], false);
    assert_eq!(after["outline"], before);
}
