// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Data connectors (ANA-ENV-010) against a local stand-in for PVGIS: `pvgis.tmy` fetches an EPW
//! for the project location and imports it; `pvgis.pv` reads PVcalc's yearly and monthly yield
//! and converts the azimuth to PVGIS's aspect; a PVGIS error message reaches the user; a
//! connector switched off refuses to run.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

/// A synthetic year: −8 °C winter nights to 31 °C summer afternoons, clear days.
fn epw() -> String {
    let mut s = String::from("LOCATION,Testville,TS,Testland,Synthetic,123456,52.0,0.0,0.0,40.0\n");
    for h in ["DESIGN CONDITIONS,0", "TYPICAL/EXTREME PERIODS,0", "GROUND TEMPERATURES,0", "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0", "COMMENTS 1,synthetic", "COMMENTS 2,", "DATA PERIODS,1,1,Data,Sunday, 1/ 1,12/31"] {
        s.push_str(h);
        s.push('\n');
    }
    let mut n = 0;
    for (m, d) in [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31].iter().enumerate() {
        for day in 1..=*d {
            for hour in 1..=24 {
                let doy = n as f64 / 24.0;
                let season = -(std::f64::consts::TAU * (doy + 10.0) / 365.0).cos();
                let daily = -(std::f64::consts::TAU * (hour as f64 - 3.0) / 24.0).cos();
                let t = 11.5 + 14.5 * season + 5.0 * daily;
                let up = (7..=18).contains(&hour);
                let (dni, dhi, ext) = if up { (650, 110, 900) } else { (0, 0, 0) };
                s.push_str(&format!("2001,{},{day},{hour},60,A7A7,{t:.1},{:.1},50,101325,{ext},1300,300,{},{dni},{dhi},0,0,0,0,180,3.0,5,5,15.0,77777,9,999999999,0,0.1,0,88,0.0,0.0,0.0\n", m + 1, t - 6.0, dhi + 300));
                n += 1;
            }
        }
    }
    s
}

/// Serves `tmy` and `PVcalc` like PVGIS; records each request line.
fn serve(seen: Arc<Mutex<Vec<String>>>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                    break;
                }
            }
            seen.lock().unwrap().push(line.clone());
            let (status, body) = if line.contains("lat=0.0000") {
                ("400 Bad Request", r#"{"message":"Location over the sea. Please, select another location","status":400}"#.to_owned())
            } else if line.contains("/tmy?") {
                ("200 OK", epw())
            } else if line.contains("/PVcalc?") {
                let months: Vec<String> = (1..=12).map(|m| format!(r#"{{"month":{m},"E_m":{}}}"#, 100 + m)).collect();
                ("200 OK", format!(r#"{{"outputs":{{"monthly":{{"fixed":[{}]}},"totals":{{"fixed":{{"E_y":1278.0,"H(i)_y":1450.5}}}}}}}}"#, months.join(",")))
            } else {
                ("404 Not Found", "{}".to_owned())
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    format!("http://{addr}/api/v5_3")
}

#[test]
fn pvgis_connector() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(seen.clone());
    let cfg = tempfile::tempdir().unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_PVGIS_URL", &url);
        std::env::set_var("XDG_CONFIG_HOME", cfg.path());
        std::env::remove_var("APPDATA");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    ok(&mut app, "location.set", json!({ "lat": 52.0, "lon": 13.0 }));
    let e = app.execute("pvgis.tmy", json!({}), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "connectors.untitled");
    ok(&mut app, "file.save_as", json!({ "path": dir.path().join("site.ifc") }));

    let w = ok(&mut app, "pvgis.tmy", json!({ "start_year": 2005, "end_year": 2020 }));
    assert!(w["file"].as_str().unwrap().contains("pvgis-52.000_13.000.epw"), "{w}");
    assert!(dir.path().join("site.resources/weather/pvgis-52.000_13.000.epw").exists());
    assert!(w["weather"]["derived"]["heating_dry_bulb"].as_f64().unwrap() < 0.0);
    let req = seen.lock().unwrap().last().unwrap().clone();
    assert!(req.starts_with("GET /api/v5_3/tmy?lat=52.0000&lon=13.0000&outputformat=epw&startyear=2005&endyear=2020 "), "{req}");

    let pv = ok(&mut app, "pvgis.pv", json!({ "peak_power": 1.0, "azimuth": 90.0 }));
    assert_eq!(pv["yearly"], 1278.0);
    assert_eq!(pv["monthly"].as_array().unwrap().len(), 12);
    assert_eq!(pv["specific"], 1278.0);
    assert_eq!(pv["irradiation"], 1450.5);
    assert!(pv["attribution"].as_str().unwrap().contains("CC BY 4.0"));
    let req = seen.lock().unwrap().last().unwrap().clone();
    assert!(req.contains("angle=35") && req.contains("aspect=-90") && req.contains("loss=14"), "east is −90 for PVGIS: {req}");

    // PVGIS's own message reaches the user.
    ok(&mut app, "location.set", json!({ "lat": 0.0, "lon": 0.0 }));
    let e = app.execute("pvgis.pv", json!({ "peak_power": 1.0 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "connectors.refused");
    assert!(e.message.contains("over the sea"), "{}", e.message);

    // Switched off, it refuses; the setting persists per user.
    let list = ok(&mut app, "connectors.list", json!({}));
    assert_eq!(list[0]["name"], "pvgis");
    assert_eq!(list[0]["enabled"], true);
    ok(&mut app, "connectors.set", json!({ "name": "pvgis", "enabled": false }));
    let e = app.execute("pvgis.pv", json!({ "peak_power": 1.0 }), &Caller::user()).unwrap_err();
    assert_eq!(e.code, "connectors.off");
    assert!(cfg.path().join("reviera/connectors.json").exists());
    assert_eq!(ok(&mut app, "connectors.list", json!({}))[0]["enabled"], false);
}
