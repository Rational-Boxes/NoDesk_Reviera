// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Growing-area schedules (ARC-GLZ-015): a schedule of greenhouses with their form, floor and
//! growing area, crop and set points, and one of growing layouts and their pieces.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e}")).result
}

fn column(t: &Value, heading: &str) -> Vec<String> {
    let k = t["headings"].as_array().unwrap().iter().position(|h| h == heading).unwrap_or_else(|| panic!("no column {heading}: {}", t["headings"]));
    t["rows"].as_array().unwrap().iter().map(|r| r["cells"][k]["text"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn greenhouses_and_their_growing_in_schedules() {
    let mut app = App::new(AppConfig { live: None, untitled_dir: std::env::temp_dir() });
    run(&mut app, "file.new", json!({}));
    let level = run(&mut app, "level.list", json!({}))[0]["id"].clone();
    // A 9.6 × 40 m gable house of tomatoes with benches, and a Venlo house with no layout yet.
    let gh = run(&mut app, "enclosure.create", json!({ "form": "gable", "level_id": level, "length": 40.0, "span": 9.6, "bay": 4.0 }));
    run(&mut app, "growing.layout", json!({ "host_id": gh, "system": "benches", "direction": "along" }));
    run(&mut app, "enclosure.set_climate", json!({ "enclosure_id": gh, "crop": "tomato" }));
    run(&mut app, "enclosure.create", json!({ "form": "venlo", "level_id": level, "origin": [0.0, 30.0], "length": 24.0, "spans": 2 }));

    let fields = run(&mut app, "schedule.fields", json!({ "category": "cat.glazed_enclosures" }));
    assert!(fields.as_array().unwrap().iter().any(|f| f["key"] == "glz.growing_area"), "{fields}");
    let s = run(&mut app, "schedule.create", json!({ "category": "cat.glazed_enclosures", "fields": ["glz.form", "glz.floor_area", "glz.growing_area", "glz.growing_share", "glz.pieces", "glz.crop", "glz.night_temperature", "glz.target_dli", "glz.heat_loss"] }));
    let t = run(&mut app, "schedule.get_table", json!({ "schedule_id": s }));
    assert_eq!(t["rows"].as_array().unwrap().len(), 2, "{t}");
    let forms = column(&t, "Form");
    let gable = forms.iter().position(|f| f == "Gable").expect("the gable house");
    assert!(forms.contains(&"Venlo".to_owned()));
    assert!(column(&t, "Floor Area")[gable].starts_with("384"), "{t}");
    let share: f64 = column(&t, "Growing Share")[gable].trim_end_matches(" %").parse().unwrap();
    assert!((60.0..75.0).contains(&share), "{t}");
    assert_eq!(column(&t, "Growing Pieces")[gable], "8");
    assert_eq!(column(&t, "Crop")[gable], "Tomato (high-wire)", "the preset's name");
    assert!(column(&t, "Target DLI")[gable].ends_with("mol/m²/d"));
    // No weather yet: no outside design temperature, so no heat loss; with one, there is.
    assert_eq!(column(&t, "Design Heat Loss")[gable], "");
    let venlo = 1 - gable;
    assert_eq!((column(&t, "Growing Area")[venlo].starts_with('0'), column(&t, "Crop")[venlo].as_str()), (true, ""));

    // The growing layouts, and their pieces.
    let g = run(&mut app, "schedule.create", json!({ "category": "cat.growing_systems" }));
    let t = run(&mut app, "schedule.get_table", json!({ "schedule_id": g }));
    let systems = column(&t, "System");
    assert_eq!(systems.iter().filter(|s| *s == "Benches").count(), 9, "the layout and its 8 benches: {t}");
    let pieces = column(&t, "Pieces");
    assert!(pieces.contains(&"8".to_owned()) && pieces.iter().filter(|p| *p == "1").count() == 8, "{t}");
}
