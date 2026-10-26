// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Weather files (ANA-EM-001): importing an EPW file fills the location and the design
//! temperatures; a file truncated at 8 000 records is refused naming the line where it ends; a
//! user override of the heating design temperature survives importing the same file again and
//! is marked; the file is copied beside the project; a far station is warned about.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A synthetic year: −10 °C at the coldest hour of January to 30 °C in July afternoons.
fn epw(records: usize) -> String {
    let mut s = String::from("LOCATION,Testville,TS,Testland,Synthetic,123456,52.0,13.0,1.0,40.0\n");
    for h in ["DESIGN CONDITIONS,0", "TYPICAL/EXTREME PERIODS,0", "GROUND TEMPERATURES,0", "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0", "COMMENTS 1,synthetic", "COMMENTS 2,", "DATA PERIODS,1,1,Data,Sunday, 1/ 1,12/31"] {
        s.push_str(h);
        s.push('\n');
    }
    let days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut n = 0;
    'all: for (m, d) in days.iter().enumerate() {
        for day in 1..=*d {
            for hour in 1..=24 {
                if n == records {
                    break 'all;
                }
                let doy = n as f64 / 24.0;
                let season = -(2.0 * std::f64::consts::PI * (doy + 10.0) / 365.0).cos();
                let daily = -(2.0 * std::f64::consts::PI * (hour as f64 - 3.0) / 24.0).cos();
                let t = 10.0 + 15.0 * season + 5.0 * daily;
                let ext = if (7..=18).contains(&hour) { 800.0 } else { 0.0 };
                s.push_str(&format!("2001,{},{day},{hour},60,A7A7,{t:.1},{:.1},50,101325,{ext},1300,300,{},300,100,0,0,0,0,180,3.0,5,5,15.0,77777,9,999999999,0,0.1,0,88,0.0,0.0,0.0\n", m + 1, t - 8.0, ext * 0.5));
                n += 1;
            }
        }
    }
    s
}

#[test]
fn weather_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let model = dir.path().join("w.ifc");
    ok(&mut app, "file.save_as", json!({ "path": model }));
    let src = dir.path().join("in").join("test.epw");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();

    // Truncated at 8 000 records: refused, naming the line it ends at.
    std::fs::write(&src, epw(8000)).unwrap();
    let e = run(&mut app, "weather.import", json!({ "path": src })).unwrap_err();
    assert_eq!(e.code, "weather.short_epw");
    assert!(e.message.contains("line 8008") && e.message.contains("8000 of 8760"), "{}", e.message);

    // The whole year: location and design temperatures filled.
    std::fs::write(&src, epw(8760)).unwrap();
    let rep = ok(&mut app, "weather.import", json!({ "path": src }));
    assert_eq!(rep["file"], "w.resources/weather/test.epw");
    assert!(dir.path().join("w.resources/weather/test.epw").exists(), "copied beside the project");
    let loc = ok(&mut app, "location.get", json!({}));
    assert_eq!((loc["latitude"].as_f64(), loc["longitude"].as_f64(), loc["elevation"].as_f64()), (Some(52.0), Some(13.0), Some(40.0)));
    let d = &rep["weather"]["derived"];
    let (heat, cool) = (d["heating_dry_bulb"].as_f64().unwrap(), d["cooling_dry_bulb"].as_f64().unwrap());
    assert!((-10.5..-8.0).contains(&heat), "{d}");
    assert!((28.0..30.5).contains(&cool), "{d}");
    assert!(d["cooling_wet_bulb"].as_f64().unwrap() < cool);
    assert!((9.0..10.5).contains(&d["daily_range"].as_f64().unwrap()), "{d}");
    assert_eq!(d["clearness"].as_f64(), Some(0.5));
    assert_eq!(rep["weather"]["station"]["wmo"], "123456");

    // An override of the heating temperature survives importing the file again.
    ok(&mut app, "weather.set_design", json!({ "overrides": { "heating_dry_bulb": -12.0 } }));
    ok(&mut app, "weather.import", json!({ "path": src }));
    let w = ok(&mut app, "weather.get", json!({}));
    assert_eq!(w["overrides"]["heating_dry_bulb"], -12.0, "marked as the user's");
    assert_eq!(w["derived"]["heating_dry_bulb"].as_f64(), Some(heat), "the file's value is kept apart");
    assert!(run(&mut app, "weather.set_design", json!({ "overrides": { "heating_dry_bulb": 30.0, "cooling_dry_bulb": 25.0 } })).is_err());
    let rev = w["revision"].as_u64().unwrap();
    assert!(rev >= 3, "every change bumps the revision for staleness");

    // A site far from the station: warned, accepted; the location is kept.
    ok(&mut app, "location.set", json!({ "lat": 48.0, "lon": 13.0, "elevation": 500.0 }));
    let rep = ok(&mut app, "weather.import", json!({ "path": src }));
    let warns: Vec<&str> = rep["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warns.iter().any(|w| w.contains("km")) && warns.iter().any(|w| w.contains("460 m below")), "{warns:?}");
    assert_eq!(ok(&mut app, "location.get", json!({}))["latitude"], 48.0);
}
