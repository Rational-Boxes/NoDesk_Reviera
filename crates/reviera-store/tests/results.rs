// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Results Explorer and Results Manager (STR-RES-001): a simply supported 6 m beam under
//! 10 kN/m, solved in process and kept with the model; its extremes (45 kN·m at mid-span, 30 kN
//! at each support, the mid-span deflection 5wL⁴/384EI from the moment diagram), the moment
//! diagram on the tension side, reaction arrows, presets (built in, panel ones refused, the
//! project's own), a view's display kept and cleared, stale results, and deleting the set.

use reviera_core::{Caller, CommandRegistry, Document};
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

fn refused(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> String {
    r.execute(doc, name, args, &Caller::user())
        .unwrap_err()
        .code
        .to_string()
}

#[test]
fn results_explorer_and_manager() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("results.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let w12 = run(
        &r,
        &mut doc,
        "beam.load_section",
        json!({ "catalog": "aisc_w", "designation": "W12X26" }),
    );
    let m = run(
        &r,
        &mut doc,
        "analytical_member.create",
        json!({ "start": [0.0, 0.0, 0.0], "end": [6.0, 0.0, 0.0], "section_type_id": w12 }),
    );
    for end in ["start", "end"] {
        run(
            &r,
            &mut doc,
            "boundary_condition.create",
            json!({ "kind": "point", "host_id": m, "location": end, "state": "pinned" }),
        );
    }
    let d = run(
        &r,
        &mut doc,
        "load_case.create",
        json!({ "name": "D", "nature": "D" }),
    );
    let load = run(
        &r,
        &mut doc,
        "load.create_line",
        json!({ "host_id": m, "case_id": d, "start": [0.0, 0.0, -10000.0] }),
    );
    run(
        &r,
        &mut doc,
        "load_combination.create",
        json!({ "name": "U1", "formula": "1.4*D" }),
    );
    let view = run(&r, &mut doc, "view.create_3d", json!({}));

    // Nothing to show yet.
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "results.show",
            json!({ "view_id": view, "result_type": "moment_y" })
        ),
        "results.none"
    );

    // Solve in process and keep the results, as structural_analysis.collect does.
    let distilled = reviera_core::structural_model::distill(doc.store()).unwrap();
    let solved = reviera_core::frame::solve(&distilled.model).unwrap();
    let (e, iy) = (distilled.model.members[0].e, distilled.model.members[0].iy);
    run(
        &r,
        &mut doc,
        "structural_analysis.record",
        json!({ "job_id": "job-1", "provider": "test", "results": solved, "key_map": distilled.key_map }),
    );
    let packages = run(&r, &mut doc, "results.packages", json!({}));
    assert_eq!(packages[0]["id"], "job-1");
    assert_eq!(packages[0]["load_groups"], json!(["D", "U1"]));
    assert_eq!(packages[0]["stale"], false);

    // Extremes: 45 kN·m at mid-span, 30 kN up at each support, and the mid-span deflection.
    let x = run(
        &r,
        &mut doc,
        "results.extremes",
        json!({ "result_type": "moment_y", "load_group": "D" }),
    );
    let biggest = [
        x["min"]["value"].as_f64().unwrap(),
        x["max"]["value"].as_f64().unwrap(),
    ]
    .into_iter()
    .max_by(|a, b| a.abs().total_cmp(&b.abs()))
    .unwrap();
    assert!((biggest.abs() - 45.0).abs() < 0.3, "{x:#}");
    assert_eq!(x["unit"], "kN·m");
    let at = x["top"][0]["at"].as_f64().unwrap();
    assert!((at - 3.0).abs() < 1e-6, "{at}");
    let x = run(
        &r,
        &mut doc,
        "results.extremes",
        json!({ "result_type": "reactions", "load_group": "D" }),
    );
    assert!(
        (x["max"]["value"].as_f64().unwrap() - 30.0).abs() < 0.1,
        "{x:#}"
    );
    let x = run(
        &r,
        &mut doc,
        "results.extremes",
        json!({ "result_type": "deformed", "load_group": "D" }),
    );
    let want = 5.0 * 10000.0 * 6f64.powi(4) / (384.0 * e * iy) * 1000.0;
    let got = x["max"]["value"].as_f64().unwrap();
    assert!(
        (got - want).abs() / want < 0.02,
        "deflection {got} mm, want {want} mm"
    );
    assert!((x["max"]["at"].as_f64().unwrap() - 3.0).abs() < 1e-6);
    // The combination is 1.4 times as much.
    let u = run(
        &r,
        &mut doc,
        "results.extremes",
        json!({ "result_type": "deformed", "load_group": "U1" }),
    );
    assert!((u["max"]["value"].as_f64().unwrap() / got - 1.4).abs() < 1e-6);

    // The moment diagram hangs below the beam (the tension side); the deformed shape sags.
    let o = run(
        &r,
        &mut doc,
        "results.overlay",
        json!({ "display": { "result_type": "moment_y", "load_group": "D" } }),
    );
    let tips: Vec<f64> = o["positive"]
        .as_array()
        .unwrap()
        .iter()
        .chain(o["negative"].as_array().unwrap())
        .map(|s| s[1][2].as_f64().unwrap())
        .collect();
    let lowest = tips.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(
        (lowest + 1.5).abs() < 1e-6,
        "the largest moment drawn 1.5 m below: {lowest}"
    );
    assert!(tips.iter().all(|z| *z <= 1e-9));
    assert!(!o["labels"].as_array().unwrap().is_empty());
    // Values carry the solver's sign (a sagging My is negative under its axes).
    assert!(
        o["labels"][0]["text"]
            .as_str()
            .unwrap()
            .trim_start_matches('-')
            .starts_with("45")
    );
    let o = run(
        &r,
        &mut doc,
        "results.overlay",
        json!({ "display": { "result_type": "deformed", "load_group": "D", "scale": 2.0 } }),
    );
    let low = o["deformed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s[1][2].as_f64().unwrap())
        .fold(f64::INFINITY, f64::min);
    assert!(
        (low + 2.0).abs() < 1e-6,
        "sags 2 m at twice the scale: {low}"
    );
    let o = run(
        &r,
        &mut doc,
        "results.overlay",
        json!({ "display": { "result_type": "reactions", "load_group": "D" } }),
    );
    // Two supports, an arrow and two barbs each, pointing up at the node.
    assert_eq!(o["positive"].as_array().unwrap().len(), 6, "{o:#}");

    // Presets: at least 13; panel ones refused with the reason.
    let presets = run(&r, &mut doc, "results.presets", json!({}));
    assert!(presets.as_array().unwrap().len() >= 13);
    let err = r
        .execute(
            &mut doc,
            "results.apply_preset",
            json!({ "view_id": view, "preset_id": "floor_shear" }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(err.code, "results.preset_unavailable");
    assert!(err.message.contains("panel results"));
    let shown = run(
        &r,
        &mut doc,
        "results.apply_preset",
        json!({ "view_id": view, "preset_id": "support_reactions", "load_group": "U1" }),
    );
    assert_eq!(shown["result_type"], "reactions");
    let o = run(&r, &mut doc, "results.overlay", json!({ "view_id": view }));
    assert_eq!(o["load_group"], "U1");
    assert!((o["max"].as_f64().unwrap() - 42.0).abs() < 0.1, "{o:#}");

    // A view's display: shown, kept through save and reopen, and saved as a preset.
    run(
        &r,
        &mut doc,
        "results.show",
        json!({ "view_id": view, "result_type": "moment_y", "deformation": true, "scale": 0.5 }),
    );
    let id = run(
        &r,
        &mut doc,
        "results.save_preset",
        json!({ "name": "Beam moments half", "view_id": view }),
    );
    let presets = run(&r, &mut doc, "results.presets", json!({}));
    let mine = presets
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap();
    assert_eq!(mine["user"], true);
    assert_eq!(mine["display"]["scale"], 0.5);
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "results.save_preset",
            json!({ "name": "beam moments HALF", "view_id": view })
        ),
        "results.name_taken"
    );
    assert_eq!(
        refused(
            &r,
            &mut doc,
            "results.delete_preset",
            json!({ "preset_id": "beam_shear" })
        ),
        "results.no_preset"
    );
    project.save(&doc).unwrap();
    drop(project);
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    let o = run(&r, &mut doc, "results.overlay", json!({ "view_id": view }));
    assert_eq!(
        o["load_group"], "U1",
        "the view's display survives save and reopen"
    );
    assert!(!o["deformed"].as_array().unwrap().is_empty());
    run(
        &r,
        &mut doc,
        "results.delete_preset",
        json!({ "preset_id": id }),
    );

    // Editing a load makes the results stale.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [load] }),
    );
    let o = run(&r, &mut doc, "results.overlay", json!({ "view_id": view }));
    assert_eq!(o["stale"], true);

    // Deleting the set clears the view.
    run(&r, &mut doc, "results.delete_package", json!({}));
    assert!(
        run(&r, &mut doc, "results.packages", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        refused(&r, &mut doc, "results.overlay", json!({ "view_id": view })),
        "results.not_shown"
    );
    project.save(&doc).unwrap();
}
