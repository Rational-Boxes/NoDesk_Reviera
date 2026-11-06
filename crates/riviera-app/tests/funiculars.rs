// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Funiculars (ARC-VT-024): two counterbalanced cars along an alignment, the passing loop, the
//! trip and capacity, and the checks.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn a_funicular_climbs_its_alignment_and_passes_half_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // 300 m east rising 90 m: 30 % the whole way.
    let line = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [300.0, 0.0]], "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 300.0, "elevation": 90.0 }] }))["alignment"].clone();
    let made = ok(&mut app, "funicular.create", json!({ "alignment_id": line }));
    let length = 300.0f64.hypot(90.0);
    assert!((f(&made["length"]) - length).abs() < 1e-6 && (f(&made["rise"]) - 90.0).abs() < 1e-9 && (f(&made["max_grade"]) - 30.0).abs() < 1e-9, "{made:#}");
    // At 5 m/s with 10 s to start and stop: L ÷ 5 + 10 s; 60 a car every trip and a minute's stop.
    let trip = length / 5.0 + 10.0;
    assert!((f(&made["trip"]) - trip).abs() < 1e-6 && (f(&made["capacity"]) - 60.0 * 3600.0 / (trip + 60.0)).abs() < 1e-6);
    // The Abt loop half way, a car and two 20 m switches long.
    assert_eq!(made["passing_loop"], json!([150.0, 52.0]));
    assert_eq!(made["layout"], "abt_loop");

    // Checks: a note with no walkway; a demand beyond its capacity.
    let found = ok(&mut app, "funicular.check", json!({ "demand": 2000.0 }));
    let rules: Vec<&str> = found.as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap()).collect();
    assert!(rules.contains(&"capacity") && rules.contains(&"evacuation"), "{found:#}");

    // Four rails: no loop. With a walkway and within its capacity: nothing to report for it.
    let four = ok(&mut app, "funicular.create", json!({ "alignment_id": line, "layout": "four_rail", "gauge": 1.435, "walkway": true }));
    assert!(four["passing_loop"].is_null() && (f(&four["car_width"]) - 2.635).abs() < 1e-9);
    let found = ok(&mut app, "funicular.check", json!({ "demand": 500.0 }));
    assert!(found.as_array().unwrap().iter().all(|x| x["funicular"] != four["funicular"]), "{found:#}");
    assert_eq!(ok(&mut app, "funicular.list", json!({})).as_array().unwrap().len(), 2);

    // A flat alignment, or one too short for two cars and the loop: refused.
    let flat = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 20.0], [300.0, 20.0]] }))["alignment"].clone();
    assert!(app.execute("funicular.create", json!({ "alignment_id": flat }), &Caller::user()).is_err());
    let short = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 40.0], [60.0, 40.0]], "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 60.0, "elevation": 20.0 }] }))["alignment"].clone();
    assert!(app.execute("funicular.create", json!({ "alignment_id": short }), &Caller::user()).is_err());
}

#[test]
fn in_ifc_4_3_the_funicular_is_rails_and_vehicles_and_goes_with_its_parts() {
    let dir = tempfile::tempdir().unwrap();
    for schema in ["IFC4X3", "IFC4"] {
        let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join(format!("untitled-{schema}")) });
        ok(&mut app, "file.new", json!({ "schema": schema }));
        let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
        let line = ok(&mut app, "alignment.create", json!({ "level": level, "points": [[0.0, 0.0], [300.0, 0.0]], "profile": [{ "station": 0.0, "elevation": 0.0 }, { "station": 300.0, "elevation": 90.0 }] }))["alignment"].clone();
        let fun = ok(&mut app, "funicular.create", json!({ "alignment_id": line }))["funicular"].clone();
        let model = dir.path().join(format!("funicular-{schema}.ifc"));
        ok(&mut app, "file.save_as", json!({ "path": model }));
        let rows = riviera_ifc::IfcModel::open(&model).unwrap().step_rows().unwrap();
        let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
        if schema == "IFC4X3" {
            assert_eq!((count("IFCRAIL"), count("IFCVEHICLE"), count("IFCRAILTYPE"), count("IFCVEHICLETYPE")), (1, 2, 1, 2));
            // A USERDEFINED type names its kind (IFC rule CorrectPredefinedType).
            assert!(rows.iter().filter(|r| r.class == "IFCVEHICLETYPE").all(|r| r.attributes.contains("'FunicularCar'")), "ElementType set");
            // The funicular is a railway facility in the site, referencing the device, its rails,
            // cars and drive, and its alignment.
            let railway = || -> (usize, String, usize, bool) {
                let ifc = riviera_ifc::IfcModel::open(&model).unwrap();
                let rows = ifc.step_rows().unwrap();
                let railways: Vec<_> = rows.iter().filter(|r| r.class == "IFCRAILWAY").collect();
                let rw = railways[0];
                assert!(rw.attributes.contains("'Funicular'") && rw.attributes.contains(".USERDEFINED."), "{}", rw.attributes);
                let refs = rows.iter().find(|r| r.class == "IFCRELREFERENCEDINSPATIALSTRUCTURE" && r.attributes.ends_with(&format!("#{})", rw.id.0)) || r.attributes.contains(&format!(",#{})", rw.id.0))).expect("referenced elements");
                let members = match ifc.attribute(refs.id, "RelatedElements").unwrap() {
                    riviera_ifc::AttrValue::List(items) => items.len(),
                    other => panic!("{other:?}"),
                };
                let alignment = rows.iter().find(|r| r.class == "IFCALIGNMENT").unwrap();
                let site = rows.iter().find(|r| r.class == "IFCSITE").unwrap();
                let aggregated = rows.iter().any(|r| r.class == "IFCRELAGGREGATES" && r.attributes.contains(&format!("#{},(#{})", site.id.0, rw.id.0)));
                assert!(refs.attributes.contains(&format!("#{}", alignment.id.0)), "the alignment is referenced: {}", refs.attributes);
                let guid = rw.attributes.split('\'').nth(1).unwrap().to_owned();
                (railways.len(), guid, members, aggregated)
            };
            let (n, guid, members, aggregated) = railway();
            // The device, its two cars and drive (the rails are the device), and the alignment.
            assert_eq!((n, members, aggregated), (1, 5, true));
            // Saved again, and opened and saved again: still one, with the same GlobalId.
            ok(&mut app, "file.save", json!({}));
            let mut again = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled-again") });
            ok(&mut again, "file.open", json!({ "path": model }));
            ok(&mut again, "file.save", json!({}));
            assert_eq!(railway(), (1, guid, 5, true));
            assert_eq!(ok(&mut again, "funicular.list", json!({})).as_array().unwrap().len(), 1, "not read back as an element");
        } else {
            assert_eq!((count("IFCRAIL"), count("IFCVEHICLE"), count("IFCRAILWAY")), (0, 0, 0), "IFC4 has no such classes");
        }
        // Deleting the funicular takes its cars and drive with it.
        // (the rails, two cars and the drive house).
        let gone = ok(&mut app, "element.delete", json!({ "element_ids": [fun] }));
        assert!(gone.as_array().unwrap().len() >= 4, "{gone:#}");
        assert_eq!(ok(&mut app, "funicular.list", json!({})), json!([]));
    }
}
