// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reinforcement, first milestone (STR-RBR-001, 002, 003, 004, 008, 009): links expanded into a
//! 300 × 600 column with 40 mm cover come to the column less twice the cover; a cover change moves
//! them; minimum clear spacing gives fewer bars when the bar grows from 12 to 20 mm; the custom
//! formula `3x100; ?x200; 5x100` gives 15 bars over 2000 mm; rounding shows `exact [rounded]`;
//! hooks in shape definitions locks once rebar is placed; a cover type in use cannot be deleted
//! without a replacement; the export writes BS 8666 shape codes; a floor made non-structural
//! orphans its bars, which are kept; deleting the column deletes its three sets.

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

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array((0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect())
}

fn info(r: &CommandRegistry, doc: &mut Document, set: ElementId) -> Value {
    run(r, doc, "rebar.info", json!({ "rebar_ids": [set.0] }))[0].clone()
}

fn dim(i: &Value, name: &str) -> f64 {
    i["dimensions"].as_array().unwrap().iter().find(|d| d[0] == name).unwrap()[1].as_f64().unwrap()
}

#[test]
fn layouts_and_formulas() {
    use reviera_core::rebar::{Layout, Rule, formula_spaces, positions};
    // 3 × 100 + 6 × 200 + 5 × 100 = 2000: 14 spaces, 15 bars.
    let l = Layout { rule: Rule::Custom, formula: Some("3x100; ?x200; 5x100".into()), ..Layout::default() };
    let p = positions(&l, 2.0, 0.012).unwrap();
    assert_eq!(p.len(), 15);
    assert!((p[14] - 2.0).abs() < 1e-9 && (p[3] - 0.3).abs() < 1e-9 && (p[4] - 0.5).abs() < 1e-9);
    // Distances at spacings.
    assert_eq!(formula_spaces("1400@100; 900@300", 2.3).unwrap().len(), 17);
    assert_eq!(formula_spaces("?x100; ?x200", 2.0).unwrap_err().code, "rebar.formula");
    assert_eq!(formula_spaces("30x100", 2.0).unwrap_err().code, "rebar.formula");
    // Maximum spacing: 2000 / 300 rounds up to 7 spaces, 8 bars, evenly.
    let l = Layout { rule: Rule::MaximumSpacing, spacing: Some(0.3), ..Layout::default() };
    let p = positions(&l, 2.0, 0.012).unwrap();
    assert_eq!(p.len(), 8);
    assert!((p[1] - 2.0 / 7.0).abs() < 1e-9);
    // Number with spacing that does not fit is refused.
    let l = Layout { rule: Rule::NumberWithSpacing, n: Some(12), spacing: Some(0.2), ..Layout::default() };
    assert!(positions(&l, 2.0, 0.012).is_err());
    // Rounding to 10 mm: 2437 → 2440, up and down too.
    use reviera_core::rebar::{RoundMethod, Rounding};
    let mut r = Rounding { on: true, method: RoundMethod::Nearest, bar_length: 0.01, segment_length: 0.005 };
    assert!((r.round(2.437, 0.01) - 2.44).abs() < 1e-9);
    r.method = RoundMethod::Down;
    assert!((r.round(2.437, 0.01) - 2.43).abs() < 1e-9);
    assert!((r.round(2.43, 0.01) - 2.43).abs() < 1e-9, "a whole increment stays");
}

#[test]
fn rebar_in_columns_floors_and_walls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rebar.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    // Metric bars.
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })));

    // A 300 x 600 concrete column.
    let c0 = id(run(&r, &mut doc, "column.place", json!({ "point": [20.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } })));
    let concrete = doc.store().get(c0).unwrap().param("@type").unwrap().as_str().unwrap().to_owned();
    let concrete = doc.store().find_by_guid(&reviera_core::GlobalId::parse(&concrete).unwrap()).unwrap();
    let ty = id(run(&r, &mut doc, "type.duplicate", json!({ "type_id": concrete.0, "new_name": "Concrete 300 x 600" })));
    run(&r, &mut doc, "type.set_param", json!({ "type_id": ty.0, "param": "width", "value": "300 mm" }));
    run(&r, &mut doc, "type.set_param", json!({ "type_id": ty.0, "param": "depth", "value": "600 mm" }));
    let col = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": ty.0, "point": [0.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    ));
    let can = run(&r, &mut doc, "rebar.can_host", json!({ "element_id": col.0 }));
    assert_eq!((can["can_host"].clone(), can["kind"].clone()), (json!(true), json!("column")), "{can:#}");

    // A 40 mm cover type on every face of the column.
    let c40 = run(&r, &mut doc, "rebar_cover_type.create", json!({ "name": "Columns 40", "offset": 0.04 }));
    run(&r, &mut doc, "rebar_cover.assign", json!({ "host_ids": [col.0], "cover_type_id": c40 }));

    // Links expanded to the host: outside dimensions the column less twice the cover.
    let links = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": col.0, "shape": "51", "bar_type": "d10", "placement": { "kind": "section" }, "layout": { "rule": "maximum_spacing", "spacing": 0.2 } }),
    ));
    let i = info(&r, &mut doc, links);
    assert!((dim(&i, "A") - 0.22).abs() < 1e-9 && (dim(&i, "B") - 0.52).abs() < 1e-9, "{:?}", i["dimensions"]);
    assert_eq!(i["role"], "ligature");
    let span: f64 = 4.0 - 0.08 - 0.01;
    assert_eq!(i["count"].as_u64().unwrap() as usize, (span / 0.2).ceil() as usize + 1);
    assert_eq!(doc.store().get(links).unwrap().param("@PredefinedType").unwrap(), "LIGATURE");
    // Every bar point keeps inside the cover box.
    for b in i["bars"].as_array().unwrap() {
        for p in b["points"].as_array().unwrap() {
            let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
            assert!(x.abs() <= 0.15 - 0.04 + 1e-9 || y.abs() <= 0.30 - 0.04 + 1e-9);
            assert!(x.abs() <= 0.15 - 0.04 + 1e-9 && y.abs() <= 0.30 - 0.04 + 1e-9, "({x}, {y})");
        }
    }
    // A bigger cover moves them in.
    run(&r, &mut doc, "rebar_cover_type.set", json!({ "cover_type_id": c40, "offset": 0.05 }));
    let i = info(&r, &mut doc, links);
    assert!((dim(&i, "A") - 0.20).abs() < 1e-9, "{:?}", i["dimensions"]);
    // A cover the bars cannot fit in is refused, naming the dimension.
    let thin = run(&r, &mut doc, "rebar_cover_type.create", json!({ "name": "Silly", "offset": 0.198 }));
    run(&r, &mut doc, "rebar_cover.assign", json!({ "host_ids": [c0.0], "cover_type_id": thin }));
    let e = r
        .execute(&mut doc, "rebar.place", json!({ "host_id": c0.0, "shape": "51", "bar_type": "d10", "placement": { "kind": "section" }, "layout": { "rule": "single" } }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "rebar.no_fit");
    assert!(e.message.contains("width"), "{e:?}");
    // Hooks in shape definitions is locked now.
    let e = r
        .execute(&mut doc, "reinforcement_settings.set", json!({ "key": "hooks_in_shape", "value": false }), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "reinforcement_settings.locked");

    // Corner bars at the column's two faces, and U bars.
    let left = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": col.0, "shape": "00", "bar_type": "d20", "placement": { "kind": "face", "face": "left" }, "layout": { "rule": "fixed_number", "n": 3 } }),
    ));
    let right = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": col.0, "shape": "21", "bar_type": "d16", "placement": { "kind": "face", "face": "right" }, "layout": { "rule": "fixed_number", "n": 3 } }),
    ));
    let i = info(&r, &mut doc, left);
    assert_eq!(i["count"], 3);
    assert!((dim(&i, "A") - (4.0 - 0.1)).abs() < 1e-9);
    assert_eq!(info(&r, &mut doc, right)["dimensions"].as_array().unwrap().len(), 3);

    // Rounding: exact [rounded].
    run(&r, &mut doc, "reinforcement_settings.set", json!({ "key": "rounding.on", "value": true }));
    run(&r, &mut doc, "reinforcement_settings.set", json!({ "key": "rounding.bar_length", "value": 0.01 }));
    let i = info(&r, &mut doc, right);
    let (l, rl) = (i["lengths"][0].as_f64().unwrap(), i["rounded_lengths"][0].as_f64().unwrap());
    assert!((rl - (l / 0.01).round() * 0.01).abs() < 1e-9);
    assert_eq!(i["length_text"], format!("{:.0} [{:.0}]", l * 1000.0, rl * 1000.0));
    let bad = r.execute(&mut doc, "reinforcement_settings.set", json!({ "key": "rounding.bar_length", "value": 0.2 }), &Caller::user());
    assert!(bad.is_err());

    // A structural floor: minimum clear spacing gives fewer 20 mm bars than 12 mm ones.
    let f = id(run(&r, &mut doc, "floor.create", json!({ "loops": [rect([0.0, 10.0], [8.0, 16.0])], "level_id": l1.0 })));
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": true }));
    let can = run(&r, &mut doc, "rebar.can_host", json!({ "element_id": f.0 }));
    assert_eq!(can["can_host"], true, "{can:#}");
    let mesh = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": f.0, "shape": "00", "bar_type": "d12", "placement": { "kind": "layer", "face": "bottom", "angle": 0.0 }, "layout": { "rule": "minimum_clear_spacing", "clear": 0.1 } }),
    ));
    let i12 = info(&r, &mut doc, mesh);
    // Across 6 m less the floor's 25 mm edge covers and a bar.
    let spread: f64 = 6.0 - 0.05 - 0.012;
    assert_eq!(i12["count"].as_u64().unwrap() as usize, (spread / 0.112).floor() as usize + 1);
    assert!((i12["lengths"][0].as_f64().unwrap() - (8.0 - 0.05)).abs() < 1e-9);
    run(&r, &mut doc, "rebar.set", json!({ "rebar_ids": [mesh.0], "bar_type": "d20" }));
    let i20 = info(&r, &mut doc, mesh);
    assert!(i20["count"].as_u64().unwrap() < i12["count"].as_u64().unwrap());
    let spread: f64 = 6.0 - 0.05 - 0.02;
    assert_eq!(i20["count"].as_u64().unwrap() as usize, (spread / 0.12).floor() as usize + 1);
    // The bars sit 25 mm above the floor's underside, to their surface.
    let z = i20["bars"][0]["points"][0][2].as_f64().unwrap();
    let floor = reviera_core::floors::regenerate(doc.store(), doc.store().get(f).unwrap()).unwrap();
    assert!((z - (floor.top - floor.thickness + 0.025 + 0.01)).abs() < 1e-9);
    // The custom formula across the floor.
    run(&r, &mut doc, "rebar.set_layout", json!({ "rebar_ids": [mesh.0], "rule": "custom", "formula": "3x100; ?x200; 5x100" }));
    let n = info(&r, &mut doc, mesh)["count"].as_u64().unwrap();
    assert_eq!(n as usize, 3 + ((spread - 0.8) / 0.2).floor() as usize + 5 + 1);

    // A structural wall's vertical bars, clipped to the wall less its cover.
    let w = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 20.0], "end": [5.0, 20.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    let can = run(&r, &mut doc, "rebar.can_host", json!({ "element_id": w.0 }));
    assert_eq!(can["can_host"], true, "{can:#}");
    let wb = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": w.0, "shape": "00", "bar_type": "d12", "placement": { "kind": "layer", "face": "exterior", "vertical": true }, "layout": { "rule": "maximum_spacing", "spacing": 0.2 } }),
    ));
    let i = info(&r, &mut doc, wb);
    assert!((i["lengths"][0].as_f64().unwrap() - (3.0 - 0.05)).abs() < 1e-9, "{:?}", i["lengths"]);

    // A cover type in use is only deleted with a replacement.
    let e = r.execute(&mut doc, "rebar_cover_type.delete", json!({ "cover_type_id": c40 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar_cover_type.used");
    let moved = run(&r, &mut doc, "rebar_cover_type.delete", json!({ "cover_type_id": c40, "replacement": "beams" }));
    assert!(moved.as_u64().unwrap() >= 3);
    let i = info(&r, &mut doc, links);
    assert!((dim(&i, "A") - (0.3 - 0.07)).abs() < 1e-9, "35 mm now: {:?}", i["dimensions"]);

    // The export: bars typed with their shape codes, aggregated by their hosts; it reads back.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCREINFORCINGBARTYPE"));
    assert!(text.contains("'51'") && text.contains("'21'") && text.contains("'00'"));
    assert!(text.contains(".LIGATURE.") && text.contains("'rxd:rebar'"));
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let back = run(&r, &mut doc2, "rebar.info", json!({ "rebar_ids": [links.0] }))[0].clone();
    assert_eq!(back["count"], info(&r, &mut doc, links)["count"]);
    assert_eq!(back["orphaned"], false);
    project.save(&doc).unwrap();
    let again = std::fs::read_to_string(&path).unwrap();
    assert_eq!(again.matches("IFCREINFORCINGBARTYPE(").count(), text.matches("IFCREINFORCINGBARTYPE(").count(), "types rewritten, not added");

    // A floor no longer structural: its bars are kept, orphaned.
    run(&r, &mut doc, "floor.set_structural", json!({ "floor_ids": [f.0], "structural": false }));
    let i = info(&r, &mut doc, mesh);
    assert_eq!(i["orphaned"], true);
    assert!(i["orphan_reason"].as_str().unwrap().contains("structural"));

    // Deleting the column deletes its three sets, which the confirmation counts.
    let going = reviera_core::builtin::delete_closure(doc.store(), &[col]);
    let rebar: Vec<_> = going
        .iter()
        .filter(|g| {
            doc.store().get(**g).and_then(reviera_core::categories::category_of).is_some_and(|c| c.name == "Structural Rebar")
        })
        .collect();
    assert_eq!(rebar.len(), 3);
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [col.0] }));
    for s in [links, left, right] {
        assert!(doc.store().get(s).is_none());
    }
    assert!(doc.store().get(mesh).is_some());
}
