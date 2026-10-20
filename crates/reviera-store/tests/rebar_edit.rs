// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar editing, propagation and splitting (STR-RBR-006), splices and couplers (STR-RBR-015):
//! a stirrup set propagated from one concrete beam to 10 beams of different lengths spans each
//! between its covers; splitting a 20-bar set at bar 8 gives sets of 8 and 12 of the same type
//! and shape; removed, moved and reset bars; a segment set by hand; a spliced set gives two sets
//! overlapping by the lap and changing together; a coupler between Ø25 bar ends 300 mm apart is
//! refused (more than 250 mm); couplers export connecting their sets and go with them.

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

fn info(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> Value {
    run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }))[0].clone()
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array((0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect())
}

#[test]
fn edit_split_propagate_splice_and_couple() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();

    // A concrete 300 × 500 beam type.
    let b0 = id(run(&r, &mut doc, "beam.create", json!({ "curve": { "start": [0.0, 0.0], "end": [5.0, 0.0] }, "reference_level": l1.0 })));
    let steel = doc.store().get(b0).unwrap().param("@type").unwrap().as_str().unwrap().to_owned();
    let steel = doc.store().find_by_guid(&reviera_core::GlobalId::parse(&steel).unwrap()).unwrap();
    let ty = id(run(&r, &mut doc, "type.duplicate", json!({ "type_id": steel.0, "new_name": "Concrete 300 x 500" })));
    for (k, v) in [("section", json!("rectangular")), ("width", json!("300 mm")), ("depth", json!("500 mm")), ("material_kind", json!("concrete"))] {
        run(&r, &mut doc, "type.set_param", json!({ "type_id": ty.0, "param": k, "value": v }));
    }
    let beam = |r: &CommandRegistry, doc: &mut Document, y: f64, l: f64| {
        id(run(r, doc, "beam.create", json!({ "type_id": ty.0, "curve": { "start": [0.0, y], "end": [l, y] }, "reference_level": l1.0 })))
    };
    let first = beam(&r, &mut doc, 2.0, 4.0);
    let links = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": first.0, "shape": "51", "bar_type": "d10", "placement": { "kind": "section" }, "layout": { "rule": "maximum_spacing", "spacing": 0.2 } }),
    ));
    // Propagated to 10 beams of 3 to 7.5 m.
    let targets: Vec<(ElementId, f64)> = (0..10).map(|k| {
        let l = 3.0 + 0.5 * k as f64;
        (beam(&r, &mut doc, 4.0 + 2.0 * k as f64, l), l)
    }).collect();
    let out = run(&r, &mut doc, "rebar.propagate", json!({ "rebar_ids": [links.0], "target_host_ids": targets.iter().map(|t| t.0.0).collect::<Vec<_>>() }));
    assert_eq!(out["failed"].as_array().unwrap().len(), 0, "{out}");
    let made: Vec<ElementId> = out["created"].as_array().unwrap().iter().map(|v| id(v.clone())).collect();
    assert_eq!(made.len(), 10);
    let cover = 0.035;
    for (set, (host, l)) in made.iter().zip(&targets) {
        let i = info(&r, &mut doc, *set);
        assert_eq!(i["host"], host.0);
        // The array runs between the end covers less a bar.
        let span = l - 2.0 * cover - 0.01;
        assert!((i["array_length"].as_f64().unwrap() - span).abs() < 1e-6, "{l}: {}", i["array_length"]);
        assert_eq!(i["count"].as_u64().unwrap() as usize, (span / 0.2 - 1e-9).ceil() as usize + 1);
    }
    // A host of another kind is reported, not filled.
    let f = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([30.0, 0.0], [36.0, 4.0])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let out = run(&r, &mut doc, "rebar.propagate", json!({ "rebar_ids": [links.0], "target_host_ids": [f.0] }));
    assert_eq!(out["failed"].as_array().unwrap().len(), 1);

    // A 20-bar set split at bar 8: 8 and 12 bars, same type and shape.
    let twenty = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": f.0, "shape": "00", "bar_type": "d12", "placement": { "kind": "layer", "face": "bottom" }, "layout": { "rule": "fixed_number", "n": 20 } }),
    ));
    let rest = id(run(&r, &mut doc, "rebar.split_set", json!({ "rebar_id": twenty.0, "at_index": 8 })));
    let (a, b) = (info(&r, &mut doc, twenty), info(&r, &mut doc, rest));
    assert_eq!((a["count"].as_u64(), b["count"].as_u64()), (Some(8), Some(12)));
    assert_eq!((a["bar_type"].clone(), a["shape"].clone()), (b["bar_type"].clone(), b["shape"].clone()));
    let y = |i: &Value, k: usize| i["bars"][k]["points"][0][1].as_f64().unwrap();
    assert!(y(&b, 0) > y(&a, 7), "the rest follow on");

    // Remove a bar, move another, reset.
    run(&r, &mut doc, "rebar.remove_bar", json!({ "rebar_id": rest.0, "index": 10 }));
    assert_eq!(info(&r, &mut doc, rest)["count"], 11);
    let before = y(&info(&r, &mut doc, rest), 0);
    run(&r, &mut doc, "rebar.move_bar_in_set", json!({ "rebar_id": rest.0, "bar_index": 8, "delta": 0.05 }));
    assert!((y(&info(&r, &mut doc, rest), 0) - before - 0.05).abs() < 1e-9);
    run(&r, &mut doc, "rebar.reset_bars", json!({ "rebar_id": rest.0 }));
    let i = info(&r, &mut doc, rest);
    assert_eq!(i["count"], 12);
    assert!((y(&i, 0) - before).abs() < 1e-9);

    // A segment set by hand: the links' A, and refused past the room inside the cover.
    run(&r, &mut doc, "rebar.set_segment_length", json!({ "rebar_id": links.0, "segment": "A", "length": 0.2 }));
    let dims = info(&r, &mut doc, links)["dimensions"].clone();
    assert!((dims[0][1].as_f64().unwrap() - 0.2).abs() < 1e-9, "{dims}");
    let e = r.execute(&mut doc, "rebar.set_segment_length", json!({ "rebar_id": links.0, "segment": "A", "length": 0.5 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar.segment");

    // Splice the first part 2 m from the bars' start with a 600 mm lap.
    let parts = run(&r, &mut doc, "rebar.splice", json!({ "rebar_id": twenty.0, "length": 2.0, "lap": 0.6 }));
    let (p1, p2) = (id(parts[0].clone()), id(parts[1].clone()));
    let (i1, i2) = (info(&r, &mut doc, p1), info(&r, &mut doc, p2));
    let full = 6.0 - 0.05;
    assert!((i1["lengths"][0].as_f64().unwrap() - 2.3).abs() < 1e-9, "{}", i1["lengths"][0]);
    assert!((i2["lengths"][0].as_f64().unwrap() - (full - 1.7)).abs() < 1e-9, "{}", i2["lengths"][0]);
    // The two overlap by the lap: the first ends 600 mm past where the second starts.
    let end1 = i1["bars"][0]["points"][1][0].as_f64().unwrap();
    let start2 = i2["bars"][0]["points"][0][0].as_f64().unwrap();
    assert!((end1 - start2 - 0.6).abs() < 1e-9);
    // They change together.
    run(&r, &mut doc, "rebar.set", json!({ "rebar_ids": [p1.0], "bar_type": "d16" }));
    assert_eq!(info(&r, &mut doc, p2)["bar_type"], "Ø16");
    // Links are not spliced.
    let e = r.execute(&mut doc, "rebar.splice", json!({ "rebar_id": links.0, "length": 0.1 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar.splice");

    // Couplers: Ø25 bars end to end 300 mm apart are refused (more than 10 × 25 = 250 mm).
    let g = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([40.0, 0.0], [46.0, 4.0])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [g.0], "structural": true }));
    let one = |r: &CommandRegistry, doc: &mut Document, boundary: Value| {
        id(run(
            r,
            doc,
            "rebar.place",
            json!({ "host_id": g.0, "shape": "00", "bar_type": "d25", "placement": { "kind": "layer", "face": "bottom", "boundary": boundary }, "layout": { "rule": "single" } }),
        ))
    };
    let left = one(&r, &mut doc, json!([[40.0, 1.0], [42.0, 1.0], [42.0, 3.0], [40.0, 3.0]]));
    let right = one(&r, &mut doc, json!([[42.25, 1.0], [46.0, 1.0], [46.0, 3.0], [42.25, 3.0]]));
    let ends = |l: ElementId, rr: ElementId| json!({ "bar_end_refs": [{ "rebar_id": l.0, "bar": 0, "end": "end" }, { "rebar_id": rr.0, "bar": 0, "end": "start" }] });
    let e = r.execute(&mut doc, "rebar_coupler.place", ends(left, right), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar_coupler.gap", "{e:?}");
    assert!(e.message.contains("300 mm apart") && e.message.contains("250"), "{}", e.message);
    // 200 mm apart is accepted.
    let near = one(&r, &mut doc, json!([[42.15, 1.0], [46.0, 1.0], [46.0, 3.0], [42.15, 3.0]]));
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [right.0] }));
    let c = id(run(&r, &mut doc, "rebar_coupler.place", ends(left, near)));
    let ci = run(&r, &mut doc, "rebar_coupler.info", json!({ "coupler_ids": [c.0] }))[0].clone();
    assert!((ci["diameter"].as_f64().unwrap() - 0.04).abs() < 1e-9);
    assert_eq!(ci["sets"], json!([left.0, near.0]));
    assert_eq!(reviera_core::categories::category_of(doc.store().get(c).unwrap()).unwrap().name, "Structural Rebar Couplers");
    // On one bar end.
    let solo = id(run(&r, &mut doc, "rebar_coupler.place", json!({ "bar_end_refs": [{ "rebar_id": left.0, "bar": 0, "end": "start" }] })));

    // Export: the coupler connects its two sets.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCRELCONNECTSWITHREALIZINGELEMENTS("));
    assert!(text.contains("'COUPLER'"));
    // Couplers go with their bars.
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [left.0] }));
    assert!(doc.store().get(c).is_none() && doc.store().get(solo).is_none());
}
