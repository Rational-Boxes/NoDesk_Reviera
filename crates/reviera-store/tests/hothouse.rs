// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Hothouse climate and crop light (ARC-GLZ-008, ARC-GLZ-009): a crop's climate on an enclosure,
//! its daily light month by month, grow lights laid out to make up the darkest month, and all of
//! it saved to IFC and read back.

use reviera_core::{Caller, CommandRegistry, Document};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user()).map(|o| o.result)
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

#[test]
fn a_tomato_range_is_lit_for_its_darkest_month() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hot.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let crops = run(&r, &mut doc, "growing.crops", json!({})).unwrap();
    assert!(crops.as_array().unwrap().iter().any(|c| c["id"] == "tomato" && c["dli_target"] == 25.0));
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "venlo", "level_id": level.0, "span": 8.0, "spans": 2, "length": 24.0 })).unwrap();
    // No location and no weather: no sun to reckon with.
    assert!(run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh })).is_err());
    run(&r, &mut doc, "location.set", json!({ "lat": 52.0, "lon": 4.4 })).unwrap();
    run(&r, &mut doc, "enclosure.set_climate", json!({ "enclosure_id": gh, "crop": "tomato", "night_temperature": 17.0 })).unwrap();
    let e = doc.store().get(reviera_core::ElementId(gh.as_u64().unwrap())).unwrap();
    assert_eq!(e.param("Reviera_GrowingClimate.Crop").and_then(|v| v["value"].as_str()), Some("Tomato (high-wire)"));
    assert!((num(&e.param("Reviera_GrowingClimate.NightTemperature").unwrap()["value"]) - 290.15).abs() < 1e-9);
    assert!(run(&r, &mut doc, "enclosure.set_climate", json!({ "enclosure_id": gh, "photoperiod": 30 })).is_err());
    assert!(run(&r, &mut doc, "enclosure.set_climate", json!({ "enclosure_id": gh, "crop": "banana" })).is_err());

    // At 52° N under clear glass on aluminium bars: December is darkest, far short of 25 mol.
    let light = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh })).unwrap();
    assert_eq!(light["source"], "estimate");
    let t = num(&light["transmission"]["total"]);
    assert!((0.7..0.92).contains(&t), "{light}");
    assert_eq!(light["darkest_month"], 12, "{light}");
    let dec = &light["months"][11];
    assert!((3.0..10.0).contains(&num(&dec["natural"])) && dec["meets_target"] == false, "{dec}");
    let june = &light["months"][5];
    assert!(num(&june["natural"]) > 25.0 && june["meets_target"] == true, "{june}");
    let need = num(&light["required_ppfd"]);
    assert!((200.0..400.0).contains(&need), "{light}");
    // A cloudier estimate needs more.
    let dull = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh, "clearness": 0.35 })).unwrap();
    assert!(num(&dull["required_ppfd"]) > need);

    // Grow lights for that need: the mean at least the target, every month met.
    let lights = run(&r, &mut doc, "growing.lights", json!({ "enclosure_id": gh })).unwrap();
    let s = run(&r, &mut doc, "growing.lights_summary", json!({ "lights_id": lights })).unwrap();
    assert!((num(&s["target_ppfd"]) - need).abs() < 1e-6, "{s}");
    assert!(num(&s["average_ppfd"]) >= need - 1e-6 && num(&s["average_ppfd"]) < need * 1.3, "{s}");
    assert!((num(&s["efficacy"]) - 2.5).abs() < 1e-9 && s["warnings"].as_array().unwrap().is_empty());
    let lit = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh })).unwrap();
    assert!(lit["months"].as_array().unwrap().iter().all(|m| m["meets_target"] == true), "{lit}");
    // Under the gutters, and over every span.
    let store = doc.store();
    let fixtures: Vec<_> = store.iter().filter_map(reviera_core::hothouse::light_recipe).collect();
    assert_eq!(fixtures.len() as u64, s["fixtures"].as_u64().unwrap());
    let rec = reviera_core::enclosures::recipe(store.get(reviera_core::ElementId(gh.as_u64().unwrap())).unwrap()).unwrap();
    assert!(fixtures.iter().all(|f| f.z < rec.eave - 0.3 && f.at[1] > 0.0 && f.at[1] < 16.0 && f.at[0] > 0.0 && f.at[0] < 24.0));
    // An old fixture is flagged; one hung above the gutters refused.
    run(&r, &mut doc, "growing.set_lights", json!({ "lights_id": lights, "photon_flux": 1100, "power": 630 })).unwrap();
    let s2 = run(&r, &mut doc, "growing.lights_summary", json!({ "lights_id": lights })).unwrap();
    assert_eq!(s2["warnings"].as_array().unwrap().len(), 1, "{s2}");
    assert!(num(&s2["fixtures"]) > num(&s["fixtures"]), "dimmer fixtures, more of them");
    assert!(run(&r, &mut doc, "growing.set_lights", json!({ "lights_id": lights, "height": 9.0 })).is_err());
    // A longer range is lit again.
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": gh, "length": 36.0 })).unwrap();
    let s3 = run(&r, &mut doc, "growing.lights_summary", json!({ "lights_id": lights })).unwrap();
    assert!(num(&s3["fixtures"]) > num(&s2["fixtures"]) * 1.3, "{s3}");
    assert!(num(&s3["average_ppfd"]) >= need - 1e-6);

    // Saved: grow lights as light fixtures, the climate and the lights' values with them.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCLIGHTFIXTURE(") && text.contains("GrowLightLayout") && text.contains("Reviera_GrowLight") && text.contains("Reviera_GrowingClimate"));
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| reviera_core::enclosures::recipe(e).is_some()).unwrap();
    let c = reviera_core::hothouse::climate(back).expect("the climate comes back");
    assert_eq!((c.crop.as_deref(), c.night_temperature), (Some("tomato"), Some(17.0)));
    assert_eq!(doc2.store().iter().filter(|e| reviera_core::hothouse::light_recipe(e).is_some()).count() as u64, s3["fixtures"].as_u64().unwrap());

    // Cleared.
    let gh2 = reviera_core::ElementId(gh.as_u64().unwrap());
    run(&r, &mut doc, "enclosure.set_climate", json!({ "enclosure_id": gh, "clear": true })).unwrap();
    assert!(doc.store().get(gh2).unwrap().params.keys().all(|k| !k.starts_with("Reviera_GrowingClimate.")));
}

#[test]
fn the_weather_files_radiation_is_the_outdoor_light() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("w.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0 })).unwrap();
    // A weather file's months: 10 MJ/m² a day all year is 19.6 mol outdoors.
    let station = json!({ "name": "Test", "region": "", "country": "", "wmo": "", "latitude": 40.0, "longitude": 0.0, "time_zone": 0.0, "elevation": 0.0 });
    run(&r, &mut doc, "weather.apply", json!({ "file": "t.epw", "epw": { "station": station, "period": [2000, 2000], "design": {}, "monthly_global": vec![10.0; 12] }, "set_location": true })).unwrap();
    let light = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh })).unwrap();
    assert_eq!(light["source"], "weather");
    assert!((num(&light["months"][0]["outdoor"]) - 19.6).abs() < 1e-9, "{light}");
    // No climate: no target, nothing short.
    assert!(light["required_ppfd"].is_null() && light["months"].as_array().unwrap().iter().all(|m| m["meets_target"] == true));
    // Months typed over it.
    let given = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh, "outdoor_dli": vec![5.0; 12] })).unwrap();
    assert_eq!(given["source"], "given");
    assert!(run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh, "outdoor_dli": vec![5.0; 3] })).is_err());
}
