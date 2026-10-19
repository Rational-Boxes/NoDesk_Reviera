// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Trusses (STR-TRUSS-001): Pratt, Howe and Warren layouts, stretching (the panels stay and the
//! webs re-proportion), the bearing chord, removing a truss (keeping its members), and IFC.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, beams};
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn ids(v: &Value) -> Vec<ElementId> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| ElementId(x.as_u64().unwrap()))
        .collect()
}

fn member(doc: &Document, m: ElementId) -> beams::BeamGeometry {
    beams::all(doc.store())
        .into_iter()
        .find(|(i, _)| *i == m)
        .unwrap_or_else(|| panic!("member {m} does not regenerate"))
        .1
}

#[test]
fn trusses_stretch_and_keep_their_panels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" }));
    let l2 = id(run(
        &r,
        &mut doc,
        "level.create",
        json!({ "elevation": 4.0 }),
    ));

    // A Pratt truss over 12 m, 6 panels, 1.5 m deep, bearing on its bottom chord at Level 2.
    let t = id(run(
        &r,
        &mut doc,
        "truss.create",
        json!({ "start": [0.0, 0.0], "end": [12.0, 0.0], "level": l2.0, "panels": 6, "height": 1.5 }),
    ));
    assert_eq!(doc.store().get(t).unwrap().class, "IfcElementAssembly");
    let info = |doc: &mut Document| run(&r, doc, "truss.info", json!({ "truss_id": t.0 }));
    let i = info(&mut doc);
    let (chords, webs) = (ids(&i["chords"]), ids(&i["webs"]));
    // 2 chords; 7 verticals and 6 diagonals.
    assert_eq!((chords.len(), webs.len()), (2, 13));
    let top = member(&doc, chords[0]);
    assert!(near(top.a[2], 5.5) && near(top.b[0], 12.0));
    let bottom = member(&doc, chords[1]);
    assert!(near(bottom.a[2], 4.0));
    // Chords are CHORD members, webs STRUT; the verticals are vertical.
    let e = doc.store().get(chords[0]).unwrap();
    assert_eq!(e.class, "IfcMember");
    assert_eq!(e.param("@PredefinedType").unwrap(), "CHORD");
    assert_eq!(
        doc.store()
            .get(webs[0])
            .unwrap()
            .param("@PredefinedType")
            .unwrap(),
        "STRUT"
    );
    let v = member(&doc, webs[1]);
    assert!(near(v.a[0], v.b[0]) && near(v.a[0], 2.0) && near(v.b[2] - v.a[2], 1.5));
    // Pratt: the first diagonal runs from the top at the support down toward mid-span.
    let d = member(&doc, webs[7]);
    assert!(near(d.a[0], 0.0) && near(d.a[2], 5.5) && near(d.b[0], 2.0) && near(d.b[2], 4.0));

    // Stretched to 15 m: still 6 panels, the webs re-proportioned (2.5 m panels).
    run(
        &r,
        &mut doc,
        "truss.set",
        json!({ "truss_id": t.0, "end": [15.0, 0.0] }),
    );
    let i = info(&mut doc);
    assert_eq!(ids(&i["webs"]).len(), 13);
    assert!(near(member(&doc, webs[1]).a[0], 2.5));
    assert!(near(member(&doc, chords[0]).length(), 15.0));

    // Howe: the first diagonal rises toward mid-span; Warren: no interior verticals.
    run(
        &r,
        &mut doc,
        "truss.set",
        json!({ "truss_id": t.0, "layout": "howe" }),
    );
    let w = ids(&info(&mut doc)["webs"]);
    let d = member(&doc, w[7]);
    assert!(near(d.a[2], 4.0) && near(d.b[2], 5.5));
    run(
        &r,
        &mut doc,
        "truss.set",
        json!({ "truss_id": t.0, "layout": "warren", "panels": 8 }),
    );
    let w = ids(&info(&mut doc)["webs"]);
    assert_eq!(w.len(), 2 + 8);
    // Bearing on the top chord: the bottom chord hangs below the line.
    run(
        &r,
        &mut doc,
        "truss.set",
        json!({ "truss_id": t.0, "bearing": "top" }),
    );
    let c = ids(&info(&mut doc)["chords"]);
    assert!(near(member(&doc, c[0]).a[2], 4.0) && near(member(&doc, c[1]).a[2], 2.5));

    // IFC: one TRUSS assembly aggregating its chords and webs; reopened, still a truss.
    let n = 2 + w.len();
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCELEMENTASSEMBLY")
            .count(),
        1
    );
    assert!(text.contains(".TRUSS.") && text.contains(".CHORD.") && text.contains(".STRUT."));
    drop(project);
    let (_p, mut doc2, _) = LocalProject::open(&path).unwrap();
    let i2 = run(&r, &mut doc2, "truss.info", json!({ "truss_id": t.0 }));
    assert_eq!(ids(&i2["chords"]).len() + ids(&i2["webs"]).len(), n);

    // Remove Truss: the members stay where they are, as plain framing.
    let before = member(&doc, c[0]).a;
    let kept = run(&r, &mut doc, "truss.remove", json!({ "truss_id": t.0 }));
    assert_eq!(ids(&kept).len(), n);
    assert!(doc.store().get(t).is_none());
    assert_eq!(member(&doc, c[0]).a, before);
    assert_eq!(beams::all(doc.store()).len(), n);
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        { "line": { "start": [x0, y0], "end": [x1, y0] } },
        { "line": { "start": [x1, y0], "end": [x1, y1] } },
        { "line": { "start": [x1, y1], "end": [x0, y1] } },
        { "line": { "start": [x0, y1], "end": [x0, y0] } },
    ])
}

/// Profiles and types (STR-TRUSS-001): a Fink gable (a W of four webs, the top chord split at the
/// apex), a gable Howe with heel posts, a scissor (its bottom chord split too), a bowstring (its
/// top chord split at each panel point), Edit Profile with a refused crossing, and a truss
/// attached under a roof following its underside.
#[test]
fn truss_profiles_types_and_roof_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let info = |doc: &mut Document, t: ElementId| {
        run(&r, doc, "truss.info", json!({ "truss_id": t.0 }))
    };
    let top_z = |doc: &Document, chords: &[ElementId]| -> f64 {
        chords
            .iter()
            .map(|c| {
                let m = member(doc, *c);
                m.a[2].max(m.b[2])
            })
            .fold(f64::NEG_INFINITY, f64::max)
    };

    // A Fink gable over 8 m: a quarter of the span high (2 m) by default, no heels.
    let fink = id(run(
        &r,
        &mut doc,
        "truss.create",
        json!({ "start": [0.0, 0.0], "end": [8.0, 0.0], "level": l1.0, "layout": "fink", "profile": "gable" }),
    ));
    let i = info(&mut doc, fink);
    let (chords, webs) = (ids(&i["chords"]), ids(&i["webs"]));
    assert_eq!((chords.len(), webs.len()), (3, 4), "{i:#}");
    assert!(near(top_z(&doc, &chords), 2.0));
    assert_eq!(i["profile"], "gable");
    // The webs run from the bottom chord's third points.
    let feet: Vec<f64> = webs.iter().map(|w| member(&doc, *w).a[0]).collect();
    assert!(feet.iter().all(|x| near(*x, 8.0 / 3.0) || near(*x, 16.0 / 3.0)), "{feet:?}");

    // A gable Howe with 300 mm heels: two heel posts, five verticals, six diagonals.
    let howe = id(run(
        &r,
        &mut doc,
        "truss.create",
        json!({ "start": [0.0, 10.0], "end": [12.0, 10.0], "level": l1.0, "layout": "howe", "profile": "gable", "panels": 6, "heel": 0.3 }),
    ));
    let i = info(&mut doc, howe);
    assert_eq!((ids(&i["chords"]).len(), ids(&i["webs"]).len()), (3, 13), "{i:#}");
    let webs = ids(&i["webs"]);
    let first = member(&doc, webs[0]);
    assert!(near(first.b[2] - first.a[2], 0.3), "the heel post is the heel's height");

    // Scissor: the bottom chord rises to half the height at mid-span.
    run(&r, &mut doc, "truss.set", json!({ "truss_id": howe.0, "profile": "scissor" }));
    let i = info(&mut doc, howe);
    let chords = ids(&i["chords"]);
    assert_eq!(chords.len(), 4);
    let bottom_peak = chords
        .iter()
        .skip(2)
        .map(|c| member(&doc, *c).b[2].max(member(&doc, *c).a[2]))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(near(bottom_peak, 1.5), "{bottom_peak}");

    // Bowstring: the top chord in six pieces, one per panel.
    run(&r, &mut doc, "truss.set", json!({ "truss_id": howe.0, "profile": "bowstring" }));
    let i = info(&mut doc, howe);
    assert_eq!(ids(&i["chords"]).len(), 7);

    // Edit Profile: a custom top chord; one that dips below the bottom is refused.
    run(
        &r,
        &mut doc,
        "truss.edit_profile",
        json!({ "truss_id": howe.0, "top": [[0.0, 0.5], [0.3, 1.5], [1.0, 0.5]] }),
    );
    let i = info(&mut doc, howe);
    assert_eq!(i["profile"], "custom");
    assert_eq!(ids(&i["chords"]).len(), 3);
    assert!(near(top_z(&doc, &ids(&i["chords"])), 1.5));
    let bad = r
        .execute(
            &mut doc,
            "truss.edit_profile",
            json!({ "truss_id": howe.0, "top": [[0.0, 0.5], [1.0, 0.5]], "bottom": [[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]] }),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(bad.code, "truss.profile");

    // Attached under a hipped roof: the top chord follows the underside, less half its depth.
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(20.0, 0.0, 30.0, 8.0), "level_id": l1.0, "base_offset": 3.0 }),
    ));
    let under = id(run(
        &r,
        &mut doc,
        "truss.create",
        json!({ "start": [25.0, 0.5], "end": [25.0, 7.5], "level": l1.0, "offset": 3.0, "layout": "howe", "panels": 6 }),
    ));
    run(&r, &mut doc, "truss.attach", json!({ "truss_id": under.0, "target_id": roof.0 }));
    let i = info(&mut doc, under);
    assert_eq!(i["attach_top"], roof.0);
    let chords = ids(&i["chords"]);
    assert_eq!(chords.len(), 7, "the top chord in six pieces, the bottom in one");
    let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    let ridge = g.underside([25.0, 4.0]).unwrap();
    let half = {
        let c = member(&doc, chords[0]);
        c.ty.depth / 2.0
    };
    assert!(near(top_z(&doc, &chords), ridge - half), "{} vs {}", top_z(&doc, &chords), ridge - half);
    // Detached: back to its flat profile.
    run(&r, &mut doc, "truss.detach", json!({ "truss_id": under.0 }));
    let i = info(&mut doc, under);
    assert!(i["attach_top"].is_null());
    assert_eq!(ids(&i["chords"]).len(), 2);

    // IFC: each truss one assembly aggregating its members.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.class == "IFCELEMENTASSEMBLY")
            .count(),
        3
    );
}
