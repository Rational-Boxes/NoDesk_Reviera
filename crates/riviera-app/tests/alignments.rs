// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Alignments (ARC-ALN-001): tangents and curves from points of intersection, chainage and offset,
//! and a vertical profile with parabolic curves.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};
use std::f64::consts::PI;

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn an_l_shaped_alignment_curves_round_its_corner_and_rises_on_its_profile() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let made = ok(&mut app, "alignment.create", json!({
        "level": level, "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], "radii": [30.0],
        "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 100.0, "elevation": 8.0, "curve": 100.0 }, { "station": 200.0, "elevation": 10.0 }]
    }));
    let length = 200.0 - 60.0 + 30.0 * PI / 2.0;
    assert!((f(&made["length"]) - length).abs() < 1e-9 && (f(&made["end_station"]) - length).abs() < 1e-9, "{made:#}");
    let id = made["alignment"].clone();

    // Half way round the curve: 30 m from its centre at (70, 30), heading north-east.
    let mid = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 70.0 + 30.0 * PI / 4.0 }));
    let p = [f(&mid["point"][0]), f(&mid["point"][1])];
    assert!(((p[0] - 70.0).hypot(p[1] - 30.0) - 30.0).abs() < 1e-9, "{mid:#}");
    assert!((f(&mid["direction"][0]) - f(&mid["direction"][1])).abs() < 1e-9 && f(&mid["direction"][0]) > 0.0);
    // 5 m to the left at chainage 50 is (50, 5), and it comes back as chainage 50, offset 5.
    let left = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 50.0, "offset": 5.0 }));
    assert!((f(&left["point"][0]) - 50.0).abs() < 1e-9 && (f(&left["point"][1]) - 5.0).abs() < 1e-9, "{left:#}");
    let back = ok(&mut app, "alignment.station_of", json!({ "alignment_id": id, "point": [50.0, 5.0] }));
    assert!((f(&back["station"]) - 50.0).abs() < 1e-9 && (f(&back["offset"]) - 5.0).abs() < 1e-9, "{back:#}");
    // On the curve, 2 m outside it (to the right of a left turn): offset −2.
    let out = ok(&mut app, "alignment.station_of", json!({ "alignment_id": id, "point": [70.0 + 32.0 * (PI / 4.0).cos(), 30.0 - 32.0 * (PI / 4.0).sin()] }));
    assert!((f(&out["offset"]) + 2.0).abs() < 1e-9 && (f(&out["station"]) - (70.0 + 30.0 * PI / 4.0)).abs() < 1e-6, "{out:#}");

    // The profile: 8 % then 2 %; on the straight grades, and at the PVI below it by A × L ÷ 8.
    let z = |app: &mut App, s: f64| f(&ok(app, "alignment.point_at", json!({ "alignment_id": id, "station": s }))["elevation"]);
    assert!((z(&mut app, 25.0) - 2.0).abs() < 1e-9 && (z(&mut app, 175.0) - 9.5).abs() < 1e-9);
    assert!((z(&mut app, 100.0) - (8.0 - 0.06 * 100.0 / 8.0)).abs() < 1e-9);
    let listed = ok(&mut app, "alignment.list", json!({}));
    assert_eq!(listed[0]["curves"].as_array().unwrap().len(), 1, "{listed:#}");
    assert!((f(&listed[0]["curves"][0]["start_station"]) - 70.0).abs() < 1e-9 && listed[0]["curves"][0]["left"] == true);

    // Moved 10 m east, its curve follows.
    ok(&mut app, "element.move", json!({ "element_ids": [id], "vector": [10.0, 0.0, 0.0] }));
    let mid = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": 70.0 + 30.0 * PI / 4.0 }));
    assert!(((f(&mid["point"][0]) - 80.0).hypot(f(&mid["point"][1]) - 30.0) - 30.0).abs() < 1e-9, "{mid:#}");

    // Off the alignment, and a radius too large for its legs: refused.
    assert!(app.execute("alignment.point_at", json!({ "alignment_id": id, "station": 500.0 }), &Caller::user()).is_err());
    assert!(app.execute("alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], "radii": [30.0] }), &Caller::user()).is_err());
}

fn drive(app: &mut App) -> Value {
    let level = ok(app, "level.list", json!({}))[0]["id"].clone();
    ok(app, "alignment.create", json!({
        "level": level, "name": "Drive", "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], "radii": [30.0], "start_station": 1000.0,
        "profile": [{ "station": 1000.0, "elevation": 0.0 }, { "station": 1100.0, "elevation": 8.0, "curve": 100.0 }, { "station": 1200.0, "elevation": 10.0 }]
    }))
}

#[test]
fn an_ifc4x3_project_writes_an_ifc_alignment_with_its_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({ "schema": "IFC4X3" }));
    drive(&mut app);
    let model = dir.path().join("road.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    ok(&mut app, "file.save", json!({}));
    let ifc = riviera_ifc::IfcModel::open(&model).unwrap();
    let rows = ifc.step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    // One alignment; a line, an arc and a line; a grade, a parabolic arc and a grade; once after
    // two saves.
    assert_eq!((count("IFCALIGNMENT"), count("IFCANNOTATION")), (1, 0));
    assert_eq!((count("IFCALIGNMENTHORIZONTAL"), count("IFCALIGNMENTVERTICAL")), (1, 1));
    assert_eq!((count("IFCALIGNMENTHORIZONTALSEGMENT"), count("IFCALIGNMENTVERTICALSEGMENT"), count("IFCALIGNMENTSEGMENT")), (3, 3, 6));
    let arc = rows.iter().find(|r| r.class == "IFCALIGNMENTHORIZONTALSEGMENT" && r.attributes.contains(".CIRCULARARC.")).expect("the curve");
    assert!(arc.attributes.contains("30.") && !arc.attributes.contains("-30."), "turning left, radius 30: {}", arc.attributes);
    assert_eq!(rows.iter().filter(|r| r.class == "IFCALIGNMENTVERTICALSEGMENT" && r.attributes.contains(".PARABOLICARC.")).count(), 1);
    // Its geometry: a composite curve of a line, an arc and a line; its start chainage on a STATION
    // referent; placed, and aggregated in the project.
    let geometry = |rows: &[riviera_ifc::StepRow]| {
        let c = |class: &str| rows.iter().filter(|r| r.class == class).count();
        (c("IFCCOMPOSITECURVE"), c("IFCCURVESEGMENT"), c("IFCCIRCLE"), c("IFCREFERENT"))
    };
    assert_eq!(geometry(&rows), (1, 3, 1, 1));
    let curve = rows.iter().find(|r| r.class == "IFCSHAPEREPRESENTATION" && r.attributes.contains("'Axis'") && r.attributes.contains("'Curve2D'"));
    assert!(curve.is_some(), "an Axis Curve2D representation");
    assert!(rows.iter().any(|r| r.class == "IFCREFERENT" && r.attributes.contains(".STATION.")));
    assert!(rows.iter().any(|r| r.class == "IFCPROPERTYSET" && r.attributes.contains("'Pset_Stationing'")));
    assert!(rows.iter().any(|r| r.class == "IFCPROPERTYSINGLEVALUE" && r.attributes.contains("'Station'") && r.attributes.contains("IFCLENGTHMEASURE(1000.")));
    let alignment = rows.iter().find(|r| r.class == "IFCALIGNMENT").unwrap();
    assert!(!alignment.attributes.contains(",$,$,$)"), "placed and represented: {}", alignment.attributes);
    assert!(rows.iter().any(|r| r.class == "IFCRELAGGREGATES" && r.attributes.contains(&format!("#{}", alignment.id.0))));

    // Opened again: the same alignment, its layouts not read as elements.
    let mut again = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled2") });
    ok(&mut again, "file.open", json!({ "path": model }));
    let list = ok(&mut again, "alignment.list", json!({}));
    assert_eq!((list.as_array().unwrap().len(), list[0]["name"].as_str(), list[0]["curves"].as_array().unwrap().len()), (1, Some("Drive"), 1), "{list:#}");
    let z = ok(&mut again, "alignment.point_at", json!({ "alignment_id": list[0]["alignment"], "station": 1025.0 }));
    assert!((f(&z["elevation"]) - 2.0).abs() < 1e-9);
    ok(&mut again, "file.save", json!({}));
    let rows = riviera_ifc::IfcModel::open(&model).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCALIGNMENTSEGMENT").count(), 6);
    assert_eq!(geometry(&rows), (1, 3, 1, 1), "replaced, not doubled");
    assert_eq!(rows.iter().filter(|r| r.class == "IFCLINE").count(), 2);
}

#[test]
fn an_ifc4_project_keeps_an_alignment_as_an_annotation() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    drive(&mut app);
    let model = dir.path().join("road.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    let rows = riviera_ifc::IfcModel::open(&model).unwrap().step_rows().unwrap();
    assert!(rows.iter().any(|r| r.class == "IFCANNOTATION" && r.attributes.contains("'Alignment'")));
    assert!(!rows.iter().any(|r| r.class.starts_with("IFCALIGNMENT")));
}

#[test]
fn spirals_ease_into_and_out_of_a_curve() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({ "schema": "IFC4X3" }));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // A 45° corner, a 100 m curve with 40 m spirals each side.
    let made = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [200.0, 0.0], [400.0, 200.0]], "radii": [100.0], "spirals": [40.0] }));
    let id = made["alignment"].clone();
    let end = f(&made["end_station"]);
    // Walk it every half metre: no jumps, curvature never above 1/R, and it ends on the last point.
    let at = |app: &mut App, s: f64| {
        let p = ok(app, "alignment.point_at", json!({ "alignment_id": id, "station": s }));
        ([f(&p["point"][0]), f(&p["point"][1])], f(&p["direction"][1]).atan2(f(&p["direction"][0])))
    };
    let n = (end / 0.5).floor() as usize;
    let mut prev = at(&mut app, 0.0);
    let mut steepest = 0.0f64;
    for k in 1..=n {
        let cur = at(&mut app, 0.5 * k as f64);
        let step = (cur.0[0] - prev.0[0]).hypot(cur.0[1] - prev.0[1]);
        assert!((step - 0.5).abs() < 1e-3, "a jump of {step} m at {}", 0.5 * k as f64);
        steepest = steepest.max((cur.1 - prev.1).abs() / 0.5);
        prev = cur;
    }
    assert!(steepest <= 1.0 / 100.0 + 1e-6, "curvature {steepest}");
    let last = at(&mut app, end).0;
    assert!((last[0] - 400.0).hypot(last[1] - 200.0) < 1e-6, "ends at {last:?}");
    let listed = ok(&mut app, "alignment.list", json!({}));
    assert_eq!((f(&listed[0]["curves"][0]["spiral"]), f(&listed[0]["curves"][0]["radius"])), (40.0, 100.0), "{listed:#}");
    // Halfway along the entry spiral the curvature is about half the curve's.
    let cs = f(&listed[0]["curves"][0]["start_station"]);
    let (a, b) = (at(&mut app, cs + 19.5).1, at(&mut app, cs + 20.5).1);
    assert!(((b - a) - 1.0 / 200.0).abs() < 1e-4, "{}", b - a);
    // And a point beside it reads back its chainage.
    let side = ok(&mut app, "alignment.point_at", json!({ "alignment_id": id, "station": cs + 10.0, "offset": 3.0 }));
    let back = ok(&mut app, "alignment.station_of", json!({ "alignment_id": id, "point": side["point"] }));
    assert!((f(&back["station"]) - (cs + 10.0)).abs() < 1e-4 && (f(&back["offset"]) - 3.0).abs() < 1e-4, "{back:#}");

    // In IFC 4.3: two CLOTHOID segments from a straight (radius 0) to the curve and back; no
    // composite curve yet.
    let model = dir.path().join("spirals.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    let rows = riviera_ifc::IfcModel::open(&model).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCALIGNMENTHORIZONTALSEGMENT" && r.attributes.contains(".CLOTHOID.")).count(), 2);
    assert_eq!(rows.iter().filter(|r| r.class == "IFCCOMPOSITECURVE").count(), 0);

    // Spirals that turn more than the corner: refused.
    assert!(app.execute("alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [200.0, 0.0], [400.0, 200.0]], "radii": [100.0], "spirals": [200.0] }), &Caller::user()).is_err());
}
