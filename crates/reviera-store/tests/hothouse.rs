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

/// The axis-aligned bounds of a body's parts.
fn bounds(b: &reviera_core::bodies::Body) -> ([f64; 3], [f64; 3]) {
    b.parts.iter().flat_map(|p| p.mesh.0.iter()).fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])]))
}

#[test]
fn screens_hang_under_the_gutters_and_shade_and_insulate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screens.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "venlo", "level_id": level.0, "span": 8.0, "spans": 2, "length": 24.0 })).unwrap();
    let lib = run(&r, &mut doc, "enclosure.screen_library", json!({})).unwrap();
    assert!(lib.as_array().unwrap().len() >= 6);
    assert!(run(&r, &mut doc, "enclosure.set_screens", json!({ "enclosure_id": gh, "screens": [{ "kind": "energy", "material": "Silk" }] })).is_err());
    run(&r, &mut doc, "enclosure.set_screens", json!({ "enclosure_id": gh, "screens": [{ "kind": "energy" }, { "kind": "shade" }] })).unwrap();
    run(&r, &mut doc, "location.set", json!({ "lat": 52.0, "lon": 4.4 })).unwrap();
    run(&r, &mut doc, "enclosure.set_climate", json!({ "enclosure_id": gh, "crop": "lettuce" })).unwrap();
    run(&r, &mut doc, "growing.lights", json!({ "enclosure_id": gh })).unwrap();

    // A sheet per screen and span (a Venlo of 2 trusses × 2 roofs: 4 spans), under the eaves.
    let store = doc.store();
    let rec = reviera_core::enclosures::recipe(store.get(reviera_core::ElementId(gh.as_u64().unwrap())).unwrap()).unwrap();
    let screens: Vec<_> = store.iter().filter_map(reviera_core::hothouse::screen_recipe).collect();
    assert_eq!(screens.len(), 2 * rec.spans as usize);
    assert!(screens.iter().all(|s| s.z <= rec.eave - 0.25 + 1e-9 && s.z >= rec.eave - 0.3 - 1e-9));
    // Clear of the posts, the beams and the grow lights.
    let bodies = reviera_core::bodies::all(store);
    let screen_ids: Vec<_> = store.iter().filter(|e| reviera_core::hothouse::screen_recipe(e).is_some()).map(|e| e.id()).collect();
    for s in bodies.iter().filter(|b| screen_ids.contains(&b.element)) {
        let (lo, hi) = bounds(s);
        for o in bodies.iter().filter(|b| store.get(b.element).is_some_and(|e| matches!(e.class.as_str(), "IfcColumn" | "IfcBeam" | "IfcLightFixture"))) {
            let (ol, oh) = bounds(o);
            assert!((0..3).any(|k| hi[k] <= ol[k] + 1e-6 || oh[k] <= lo[k] + 1e-6), "screen {} meets {} {}", s.element, store.get(o.element).unwrap().class, o.element);
        }
    }

    // July: the shade screen would be closed, its light half the natural light.
    let light = run(&r, &mut doc, "enclosure.crop_light", json!({ "enclosure_id": gh })).unwrap();
    let july = &light["months"][6];
    assert!(july["shade_closed"] == true && (july["shaded"].as_f64().unwrap() - 0.5 * july["natural"].as_f64().unwrap()).abs() < 1e-9, "{july}");
    assert_eq!(light["months"][11]["shade_closed"], false);

    // The heat loss, and 62 % less with the energy screen closed.
    let h = run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": gh, "inside": 18.0, "outside": -10.0 })).unwrap();
    assert_eq!(h["construction_factor"], 1.05);
    assert!((h["with_screen"].as_f64().unwrap() - 0.38 * h["total"].as_f64().unwrap()).abs() < 1e-6, "{h}");

    // Saved: the screens are shading devices with their values; removed with [].
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCSHADINGDEVICE(") && text.contains("Reviera_Screen") && text.contains("EnergyScreen"));
    run(&r, &mut doc, "enclosure.set_screens", json!({ "enclosure_id": gh, "screens": [] })).unwrap();
    assert_eq!(doc.store().iter().filter_map(reviera_core::hothouse::screen_recipe).count(), 0);
}

#[test]
fn the_quick_heat_loss_follows_the_table_method() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("loss.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 9.6 × 40 m gable glasshouse at 15 °C inside, −10 °C outside, no wind factor.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": 40.0, "span": 9.6, "bay": 4.0 })).unwrap();
    assert_eq!(run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": gh })).unwrap_err().code, "growing.no_design");
    let h = run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": gh, "inside": 15.0, "outside": -10.0 })).unwrap();
    let f = |k: &str| h[k].as_f64().unwrap();
    // Glass on aluminium bars 1.125 m apart (about 48 in): 1.05.
    assert_eq!((f("construction_factor"), f("wind_factor"), f("air_changes"), f("delta_t")), (1.05, 1.0, 1.0, 25.0));
    let ua: f64 = h["parts"].as_array().unwrap().iter().map(|p| p["ua"].as_f64().unwrap()).sum();
    assert!((f("transmission") - ua * 1.05 * 25.0).abs() < 1e-6 && (f("infiltration") - 0.33 * f("volume") * 25.0).abs() < 1e-6, "{h}");
    assert!((f("total") - f("transmission") - f("infiltration")).abs() < 1e-6);
    // Single glass at 5.8 W/m²K over about twice the floor's area (a small gable house): some
    // 200–450 W per m² of floor at a 25 K difference.
    assert!((200.0..450.0).contains(&f("per_area")), "{h}");
    assert!(h["with_screen"].is_null());
    // Wind raises it; the factor stops at 1.16.
    let windy = run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": gh, "inside": 15.0, "outside": -10.0, "wind": 13.4 })).unwrap();
    assert!((windy["wind_factor"].as_f64().unwrap() - 1.12).abs() < 0.005, "{windy}");
    assert_eq!(reviera_core::hothouse::wind_factor(30.0), 1.16);
    // A film tunnel: film on metal hoops, 0.8 air changes; no screens in tunnels yet.
    let tunnel = run(&r, &mut doc, "enclosure.create", json!({ "form": "hoop", "level_id": level.0, "origin": [0, 30] })).unwrap();
    let t = run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": tunnel, "inside": 10.0, "outside": -5.0 })).unwrap();
    assert_eq!((t["construction_factor"].as_f64(), t["air_changes"].as_f64()), (Some(1.02), Some(0.8)));
    assert!(run(&r, &mut doc, "enclosure.set_screens", json!({ "enclosure_id": tunnel, "screens": [{ "kind": "energy" }] })).is_err());
}

#[test]
fn ventilation_and_co2_are_sized_by_the_published_rules() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("vent.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": 40.0, "span": 9.6, "bay": 4.0 })).unwrap();
    // 8 cfm/ft² of 4,133 ft²: 15.6 m³/s, two 10 m³/s fans, 1.25 × their face of inlets.
    let v = run(&r, &mut doc, "enclosure.ventilation", json!({ "enclosure_id": gh, "elevation": 0 })).unwrap();
    let f = |k: &str| v[k].as_f64().unwrap();
    assert!((f("airflow") - 15.6).abs() < 0.05, "{v}");
    assert_eq!(v["fans"], 2);
    assert!((f("inlet_area") - 1.25 * 2.0 * std::f64::consts::PI * 1.22 * 1.22 / 4.0).abs() < 1e-9);
    assert!(v["pad_area"].is_null());
    assert!((f("haf_airflow") - 15.6 / 4.0).abs() < 0.02 && (f("roof_vent_percent") > 0.0), "{v}");
    // With 100 mm pads: 15 % more air and the pads for it; a hot climate and altitude ask more.
    let p = run(&r, &mut doc, "enclosure.ventilation", json!({ "enclosure_id": gh, "elevation": 0, "pads": "100" })).unwrap();
    assert!((p["airflow"].as_f64().unwrap() - 17.94).abs() < 0.05 && (p["pad_area"].as_f64().unwrap() - 14.13).abs() < 0.05, "{p}");
    assert!((p["pad_rule_area"].as_f64().unwrap() - 19.2).abs() < 1e-9);
    let hot = run(&r, &mut doc, "enclosure.ventilation", json!({ "enclosure_id": gh, "climate": "hot", "elevation": 1500 })).unwrap();
    assert!(hot["airflow"].as_f64().unwrap() > 15.6 * 1.25 * 1.15, "{hot}");
    // A long house draws from both ends.
    let long = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "origin": [0, 30], "length": 60.0 })).unwrap();
    let lv = run(&r, &mut doc, "enclosure.ventilation", json!({ "enclosure_id": long })).unwrap();
    assert!(lv["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("both ends")), "{lv}");

    // CO₂ at 1,000 ppm: 0.55 kg/h per 100 m² at 1,300 ppm, scaled by 600/900; burners 30–36 kW per 1,000 m².
    let c = run(&r, &mut doc, "enclosure.co2", json!({ "enclosure_id": gh, "setpoint": 1000 })).unwrap();
    let area = 384.0;
    assert_eq!(c["glazing"], "glass");
    assert!((c["supply"].as_f64().unwrap() - 0.55 * area / 100.0 * 600.0 / 900.0).abs() < 1e-9, "{c}");
    assert!((c["burner_kw"][0].as_f64().unwrap() - 30.0 * area / 1000.0).abs() < 1e-9);
    assert!(run(&r, &mut doc, "enclosure.co2", json!({ "enclosure_id": gh, "setpoint": 6000 })).is_err());
    let tunnel = run(&r, &mut doc, "enclosure.create", json!({ "form": "hoop", "level_id": level.0, "origin": [0, 60] })).unwrap();
    assert_eq!(run(&r, &mut doc, "enclosure.co2", json!({ "enclosure_id": tunnel })).unwrap()["glazing"], "plastic");
}

#[test]
fn heating_is_sized_from_the_heat_loss() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("heat.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": 40.0, "span": 9.6, "bay": 4.0 })).unwrap();
    run(&r, &mut doc, "growing.layout", json!({ "host_id": gh, "system": "benches", "direction": "along" })).unwrap();
    let h = run(&r, &mut doc, "enclosure.heating", json!({ "enclosure_id": gh, "inside": 15.0, "outside": -10.0 })).unwrap();
    let f = |k: &str| h[k].as_f64().unwrap();
    let loss = run(&r, &mut doc, "enclosure.heat_loss", json!({ "enclosure_id": gh, "inside": 15.0, "outside": -10.0 })).unwrap()["total"].as_f64().unwrap();
    assert!((f("load_kw") - loss / 1000.0).abs() < 1e-9 && (120.0..140.0).contains(&f("load_kw")), "{h}");
    // At least one heater per 18 m of the 40 m length.
    assert!(h["unit_heaters"].as_u64().unwrap() >= 3 && f("heater_kw") * h["unit_heaters"].as_f64().unwrap() >= f("load_kw") - 1e-9, "{h}");
    assert!((f("perimeter_fin_m") - loss / 1150.0).abs() < 1e-6 && (f("perimeter_m") - 99.2).abs() < 1e-9);
    // Root-zone heat over the benches: under a quarter of the load; the benches' own runs.
    assert!(f("growing_area") > 0.0 && f("root_zone_percent") < 25.0 && h["bench_runs_m"].as_f64().unwrap() > 0.0, "{h}");
    assert!(h["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("wide")));
    assert!(run(&r, &mut doc, "enclosure.heating", json!({ "enclosure_id": gh, "outside": -10.0, "heater_kw": 500 })).is_err());
}
