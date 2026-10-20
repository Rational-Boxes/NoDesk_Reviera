// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Varying rebar sets (STR-RBR-005): bars through a tapered wall follow the taper and the
//! schedule shows the length range; a standard set's bars all take the first one's length;
//! converting back to standard is refused naming the number of lengths; each length takes its
//! own number and the tag shows the range; five lengths export as one `IfcElementAssembly`
//! (`REINFORCEMENT_UNIT`) of five `IfcReinforcingBar` groups, written once on every save and
//! read back as one set; a curved wall is refused.

use std::collections::HashMap;

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

fn lengths(i: &Value) -> Vec<f64> {
    i["lengths"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
}

#[test]
fn varying_sets_in_a_tapered_wall() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varying.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "uk_adk" }));
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let plan = id(run(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }))[0].clone());

    // A 3 m structural wall, 240 mm at its base, each face leaning in 1°.
    let w = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [10.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    let t = doc.store().iter().find(|e| e.class == "IfcWallType").unwrap().id();
    run(
        &r,
        &mut doc,
        "compound.set_layers",
        json!({ "type_id": t.0, "layers": [
            { "function": "finish1", "material": "Render", "thickness": "20 mm" },
            { "function": "structure", "material": "Concrete", "thickness": "200 mm", "variable": true },
            { "function": "finish2", "material": "Plaster", "thickness": "20 mm" },
        ] }),
    );
    let lean = 1f64.to_radians();
    run(&r, &mut doc, "wall.set_cross_section", json!({ "wall_ids": [w.0], "mode": "tapered", "exterior_angle": lean, "interior_angle": lean }));

    // Five varying bars through it: each one's length follows the taper.
    let place = |doc: &mut Document, varying: bool, along: f64| {
        id(run(
            &r,
            doc,
            "rebar.place",
            json!({ "host_id": w.0, "shape": "00", "bar_type": "d10", "placement": { "kind": "through", "along": along }, "layout": { "rule": "fixed_number", "n": 5 }, "varying": varying }),
        ))
    };
    let v = place(&mut doc, true, 5.0);
    let i = info(&r, &mut doc, v);
    assert_eq!(i["varying"], true);
    let ls = lengths(&i);
    assert_eq!(ls.len(), 5);
    let z: Vec<f64> = i["bars"].as_array().unwrap().iter().map(|b| b["points"][0][2].as_f64().unwrap()).collect();
    for k in 1..5 {
        // Thinner by 2 · tan 1° per metre up.
        let want = ls[0] - 2.0 * (z[k] - z[0]) * lean.tan();
        assert!((ls[k] - want).abs() < 1e-9, "bar {k}: {} vs {want}", ls[k]);
    }
    // The bottom bar: the base's 240 mm less the covers, measured square to the faces.
    let covers = ls[0] + 2.0 * z[0] * lean.tan();
    assert!(covers < 0.24 && covers > 0.24 - 0.1, "{covers}");
    assert_eq!(i["length_groups"].as_array().unwrap().len(), 5);
    let text = i["length_text"].as_str().unwrap();
    assert_eq!(text, format!("{:.0}–{:.0}", ls[4] * 1000.0, ls[0] * 1000.0));

    // The schedule shows the range; minimum and maximum have their own columns.
    let sched = id(run(&r, &mut doc, "rebar_schedule.create", json!({})));
    run(&r, &mut doc, "schedule.set_fields", json!({ "schedule_id": sched.0, "fields": ["rebar.number", "rebar.bar_length", "rebar.min_length", "rebar.max_length", "rebar.quantity"] }));
    let st = doc.store();
    let table = reviera_core::schedules::table(st, st.get(sched).unwrap(), &HashMap::new()).unwrap();
    let row: Vec<String> = table.rows[0].cells.iter().map(|c| c.text.clone()).collect();
    assert_eq!(row[1], text, "{row:?}");
    assert_eq!(row[2], format!("{:.0}", ls[4] * 1000.0));
    assert_eq!(row[3], format!("{:.0}", ls[0] * 1000.0));
    assert!(row[0].contains('–'), "five numbers: {row:?}");

    // Each length its own number; the tag shows the range.
    let numbers: Vec<u64> = i["length_groups"].as_array().unwrap().iter().map(|g| g["number"].as_u64().unwrap()).collect();
    let mut distinct = numbers.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 5);
    run(&r, &mut doc, "rebar_tag.place", json!({ "rebar_id": v.0, "view_id": plan.0 }));
    let tags = run(&r, &mut doc, "tag.list", json!({ "view_id": plan.0 }));
    assert_eq!(tags[0]["lines"][0], format!("{}–{}", distinct[0], distinct[4]));

    // Back to standard: refused, naming the five lengths.
    let e = r.execute(&mut doc, "rebar.convert_to_standard", json!({ "rebar_ids": [v.0] }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar.lengths_differ");
    assert!(e.message.contains("5 different lengths"), "{}", e.message);

    // A standard set: every bar the first one's length, centred in its section.
    let s = place(&mut doc, false, 7.0);
    let si = info(&r, &mut doc, s);
    let sl = lengths(&si);
    assert!(sl.iter().all(|l| (l - ls[0]).abs() < 1e-9), "{sl:?}");
    assert!(si["length_groups"].as_array().unwrap().is_empty());
    // Converted to varying it follows the taper, and stays varying.
    run(&r, &mut doc, "rebar.convert_to_varying", json!({ "rebar_ids": [s.0] }));
    assert!((lengths(&info(&r, &mut doc, s))[4] - ls[4]).abs() < 1e-9);
    // Made vertical, its bars share one length again and it converts back.
    run(&r, &mut doc, "wall.set_cross_section", json!({ "wall_ids": [w.0], "mode": "vertical" }));
    assert_eq!(info(&r, &mut doc, s)["varying"], true, "one length, still varying");
    run(&r, &mut doc, "rebar.convert_to_standard", json!({ "rebar_ids": [s.0] }));
    assert_eq!(info(&r, &mut doc, s)["varying"], false);
    run(&r, &mut doc, "element.delete", json!({ "element_ids": [s.0] }));
    run(&r, &mut doc, "wall.set_cross_section", json!({ "wall_ids": [w.0], "mode": "tapered", "exterior_angle": lean, "interior_angle": lean }));

    // Layers are not varied.
    let layer = id(run(
        &r,
        &mut doc,
        "rebar.place",
        json!({ "host_id": w.0, "shape": "00", "bar_type": "d10", "placement": { "kind": "layer", "face": "exterior", "vertical": true }, "layout": { "rule": "maximum_spacing", "spacing": 0.3 } }),
    ));
    let e = r.execute(&mut doc, "rebar.convert_to_varying", json!({ "rebar_ids": [layer.0] }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "rebar.not_varying");

    // A curved wall: refused, saying why.
    let arc = id(run(
        &r,
        &mut doc,
        "wall.create",
        json!({ "curve": { "line": { "start": [0.0, 10.0], "end": [10.0, 10.0] }, "through": [[5.0, 12.0]] }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 }, "structural": true }),
    ));
    let e = r
        .execute(
            &mut doc,
            "rebar.place",
            json!({ "host_id": arc.0, "shape": "00", "bar_type": "d10", "placement": { "kind": "through", "along": 2.0 }, "varying": true }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(e.code, "rebar.curved", "{e:?}");
    assert!(e.message.contains("curved"), "{}", e.message);

    // One reinforcement unit of five bar groups, written once however often it is saved.
    let unit_count = |text: &str| {
        let lines: Vec<&str> = text.lines().filter(|l| l.contains("IFCRELAGGREGATES") && l.contains("'rxd:rebar_unit'")).collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let members = lines[0].rsplit_once('(').unwrap().1.matches('#').count();
        let units = text.lines().filter(|l| l.contains("IFCELEMENTASSEMBLY") && l.contains(".REINFORCEMENT_UNIT.")).count();
        (units, members)
    };
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(unit_count(&text), (1, 5));
    let bars_before = text.matches("IFCREINFORCINGBAR(").count();
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(unit_count(&text), (1, 5));
    assert_eq!(text.matches("IFCREINFORCINGBAR(").count(), bars_before);

    // Read back: one varying set, the groups not loaded as elements.
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let sets: Vec<ElementId> = doc2.store().iter().filter(|e| reviera_core::rebar::rebar_recipe(e).is_some()).map(|e| e.id()).collect();
    assert_eq!(sets.len(), 2, "the varying set and the layer");
    assert_eq!(doc2.store().iter().filter(|e| e.class == "IfcElementAssembly" && e.param("@PredefinedType").and_then(Value::as_str) == Some("REINFORCEMENT_UNIT")).count(), 0);
    let back = sets.iter().map(|s| info(&r, &mut doc2, *s)).find(|i| i["varying"] == true).unwrap();
    assert_eq!(lengths(&back).len(), 5);
}
