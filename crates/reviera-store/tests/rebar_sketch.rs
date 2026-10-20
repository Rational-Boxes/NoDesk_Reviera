// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketched rebar and constraints (STR-RBR-007): a longitudinal bar constrained to a beam's top
//! cover moves 10 mm when the cover goes from 40 to 50 mm; its ends kept at the end covers; a bar
//! referencing it follows; a face 10° from parallel is refused; sketches match shapes 00, 11, 21,
//! 51 and 99; constraints removed leave the bar where it is; they survive saving.

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

/// The first bar's corners.
fn bar(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> Vec<[f64; 3]> {
    let i = run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }));
    i[0]["bars"][0]["points"].as_array().unwrap().iter().map(|p| [0, 1, 2].map(|k| p[k].as_f64().unwrap())).collect()
}

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({ "line": { "start": a, "end": b } })
}

#[test]
fn sketched_bars_follow_their_constraints() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sketch.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A concrete 300 × 500 beam, 6 m along x.
    let b0 = id(run(&r, &mut doc, "beam.create", json!({ "curve": { "start": [0.0, 10.0], "end": [5.0, 10.0] }, "reference_level": l1.0 })));
    let steel = doc.store().get(b0).unwrap().param("@type").unwrap().as_str().unwrap().to_owned();
    let steel = doc.store().find_by_guid(&reviera_core::GlobalId::parse(&steel).unwrap()).unwrap();
    let ty = id(run(&r, &mut doc, "type.duplicate", json!({ "type_id": steel.0, "new_name": "Concrete 300 x 500" })));
    for (k, v) in [("section", json!("rectangular")), ("width", json!("300 mm")), ("depth", json!("500 mm")), ("material_kind", json!("concrete"))] {
        run(&r, &mut doc, "type.set_param", json!({ "type_id": ty.0, "param": k, "value": v }));
    }
    let beam = id(run(&r, &mut doc, "beam.create", json!({ "type_id": ty.0, "curve": { "start": [0.0, 0.0], "end": [6.0, 0.0] }, "reference_level": l1.0 })));
    let c40 = run(&r, &mut doc, "rebar_cover_type.create", json!({ "name": "Top 40", "offset": 0.04 }));
    run(&r, &mut doc, "rebar_cover.assign", json!({ "host_ids": [beam.0], "face": "top", "cover_type_id": c40 }));
    let info = run(&r, &mut doc, "rebar.can_host", json!({ "element_id": beam.0 }));
    assert_eq!(info["can_host"], true, "{info:#}");

    // A Ø16 bar sketched along the beam's long section (x along it, y up from its centre).
    let top = id(run(&r, &mut doc, "rebar.sketch", json!({ "host_id": beam.0, "curves": [line([0.5, 0.1], [5.5, 0.1])], "bar_type": "d16" })));
    let s = bar(&r, &mut doc, top);
    let z_sketch = s[0][2];
    let faces = run(&r, &mut doc, "rebar.constraints", json!({ "rebar_id": top.0 }));
    let top_z = faces["faces"].as_array().unwrap().iter().find(|f| f["name"] == "top").unwrap()["point"][2].as_f64().unwrap();
    assert!((z_sketch - (top_z - 0.25 + 0.1)).abs() < 1e-9, "the sketch is 100 mm above the centre");
    assert_eq!(run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [top.0] }))[0]["shape"], "00");

    // To the top cover: its centre 40 + 8 mm under the top.
    run(&r, &mut doc, "rebar.constrain", json!({ "rebar_id": top.0, "handle_id": "segment:0", "target_kind": "cover", "target_ref": "top" }));
    let s = bar(&r, &mut doc, top);
    assert!(s.iter().all(|p| (p[2] - (top_z - 0.048)).abs() < 1e-9), "{s:?}");
    // Its ends at the end covers (35 mm, the beams' default).
    run(&r, &mut doc, "rebar.constrain", json!({ "rebar_id": top.0, "handle_id": "start", "target_kind": "cover", "target_ref": "start" }));
    run(&r, &mut doc, "rebar.constrain", json!({ "rebar_id": top.0, "handle_id": "end", "target_kind": "surface", "target_ref": "end", "offset": 0.1 }));
    let s = bar(&r, &mut doc, top);
    assert!((s[0][0] - 0.043).abs() < 1e-9 && (s[1][0] - 5.9).abs() < 1e-9, "{s:?}");

    // A second bar 60 mm below it, referencing it.
    let under = id(run(&r, &mut doc, "rebar.sketch", json!({ "host_id": beam.0, "curves": [line([1.0, 0.0], [5.0, 0.0])], "bar_type": "d12" })));
    run(&r, &mut doc, "rebar.constrain", json!({ "rebar_id": under.0, "handle_id": "segment:0", "target_kind": "bar", "target_ref": top.0, "offset": 0.06 }));
    assert!((bar(&r, &mut doc, under)[0][2] - (top_z - 0.048 - 0.06)).abs() < 1e-9);

    // The cover from 40 to 50 mm: both move 10 mm down; the ends stay.
    run(&r, &mut doc, "rebar_cover_type.set", json!({ "cover_type_id": c40, "offset": 0.05 }));
    let s = bar(&r, &mut doc, top);
    assert!(s.iter().all(|p| (p[2] - (top_z - 0.058)).abs() < 1e-9), "{s:?}");
    assert!((s[0][0] - 0.043).abs() < 1e-9);
    assert!((bar(&r, &mut doc, under)[0][2] - (top_z - 0.058 - 0.06)).abs() < 1e-9);

    // A bar sketched 10° off the beam: the top cover is refused, saying how far off it is.
    let t10 = 10f64.to_radians().tan();
    let skew = id(run(&r, &mut doc, "rebar.sketch", json!({ "host_id": beam.0, "curves": [line([1.0, -0.1], [3.0, -0.1 + 2.0 * t10])], "bar_type": "d12" })));
    let e = r
        .execute(&mut doc, "rebar.constrain", json!({ "rebar_id": skew.0, "handle_id": "segment:0", "target_kind": "cover", "target_ref": "top" }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.not_parallel");
    assert!(e.message.contains("10.0°") && e.message.contains("within 6°"), "{}", e.message);
    // 5° off is taken: the bar comes level at the cover.
    let t5 = 5f64.to_radians().tan();
    let five = id(run(&r, &mut doc, "rebar.sketch", json!({ "host_id": beam.0, "curves": [line([1.0, -0.1], [3.0, -0.1 + 2.0 * t5])], "bar_type": "d12" })));
    run(&r, &mut doc, "rebar.constrain", json!({ "rebar_id": five.0, "handle_id": "segment:0", "target_kind": "cover", "target_ref": "bottom" }));
    let f = bar(&r, &mut doc, five);
    assert!((f[0][2] - f[1][2]).abs() < 1e-9);

    // Constraints removed: the bar stays where they put it, and no longer follows.
    let before = bar(&r, &mut doc, under);
    run(&r, &mut doc, "rebar.reset_constraints", json!({ "rebar_id": under.0 }));
    assert_eq!(bar(&r, &mut doc, under), before);
    run(&r, &mut doc, "rebar_cover_type.set", json!({ "cover_type_id": c40, "offset": 0.04 }));
    assert_eq!(bar(&r, &mut doc, under), before);
    assert!((bar(&r, &mut doc, top)[0][2] - (top_z - 0.048)).abs() < 1e-9);

    // Shapes: an L, a U, a closed link, and an arc (special).
    let shape = |doc: &mut Document, curves: Value, style: &str| {
        let s = id(run(&r, doc, "rebar.sketch", json!({ "host_id": beam.0, "curves": curves, "bar_type": "d10", "style": style })));
        run(&r, doc, "rebar.info", json!({ "rebar_ids": [s.0] }))[0].clone()
    };
    assert_eq!(shape(&mut doc, json!([line([0.5, 0.15], [2.0, 0.15]), line([2.0, 0.15], [2.0, -0.15])]), "standard")["shape"], "11");
    let u = shape(&mut doc, json!([line([0.5, -0.15], [0.5, 0.15]), line([0.5, 0.15], [2.0, 0.15]), line([2.0, 0.15], [2.0, -0.15])]), "standard");
    assert_eq!(u["shape"], "21");
    let a = u["dimensions"][1][1].as_f64().unwrap();
    assert!((a - (1.5 + 0.01)).abs() < 1e-9, "B: the run plus half a bar at each bend: {a}");
    let link = shape(
        &mut doc,
        json!([line([1.0, -0.2], [1.0, 0.2]), line([1.0, 0.2], [1.2, 0.2]), line([1.2, 0.2], [1.2, -0.2]), line([1.2, -0.2], [1.0, -0.2])]),
        "stirrup_tie",
    );
    assert_eq!((link["shape"].clone(), link["role"].clone()), (json!("51"), json!("ligature")));
    let arc = shape(&mut doc, json!([{ "arc": { "start": [1.0, 0.0], "end": [3.0, 0.0], "through": [2.0, 0.15] } }]), "standard");
    assert_eq!(arc["shape"], "99");
    let pts = arc["bars"][0]["points"].as_array().unwrap();
    let peak = pts.iter().map(|p| p[2].as_f64().unwrap()).fold(f64::MIN, f64::max);
    assert!((peak - (top_z - 0.25 + 0.15)).abs() < 1e-3, "{peak}");

    // A set spread along the normal (across the beam) to the far side's cover.
    let set = id(run(
        &r,
        &mut doc,
        "rebar.sketch",
        json!({ "host_id": beam.0, "curves": [line([0.5, -0.2], [5.5, -0.2])], "bar_type": "d16", "layout": { "rule": "fixed_number", "n": 3 } }),
    ));
    let i = run(&r, &mut doc, "rebar.info", json!({ "rebar_ids": [set.0] }));
    assert_eq!(i[0]["count"], 3);
    // From the axis to the far face's cover (150 − 35 − 8 mm).
    assert!((i[0]["array_length"].as_f64().unwrap() - 0.107).abs() < 1e-9, "{}", i[0]["array_length"]);

    // Saved and read back, the constraints still hold.
    project.save(&doc).unwrap();
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| e.guid() == doc.store().get(top).unwrap().guid()).unwrap().id();
    let c = run(&r, &mut doc2, "rebar.constraints", json!({ "rebar_id": back.0 }));
    let constrained: Vec<&str> = c["handles"].as_array().unwrap().iter().filter(|h| !h["constraint"].is_null()).map(|h| h["id"].as_str().unwrap()).collect();
    assert_eq!(constrained, ["start", "segment:0", "end"]);
}
