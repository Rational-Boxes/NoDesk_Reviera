// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Site data connectors (ANA-ENV-010) against local stand-ins: `geocode.search` sends an
//! encoded query with an identifying User-Agent, reads Nominatim's places, answers a repeated
//! search from its cache and spaces requests a second apart; `elevation.fetch` writes Open-Meteo's
//! elevation to the location; `grid_intensity.fetch` averages the Carbon Intensity API's 30-day
//! windows; a refusal's message reaches the user; a connector switched off refuses to run.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

/// Answers like the three services; records each request line with its User-Agent and time.
fn serve(seen: Arc<Mutex<Vec<(String, String, Instant)>>>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut window = 0;
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut agent = String::new();
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                    break;
                }
                if h.to_ascii_lowercase().starts_with("user-agent:") {
                    agent = h[11..].trim().to_owned();
                }
            }
            seen.lock().unwrap().push((line.clone(), agent, Instant::now()));
            let (status, body) = if line.contains("/geo/search?q=Nowhere") {
                ("200 OK", "[]".to_owned())
            } else if line.contains("/geo/search?") {
                ("200 OK", r#"[{"place_id":1,"licence":"Data © OpenStreetMap contributors, ODbL 1.0.","lat":"47.3769","lon":"8.5417","addresstype":"city","name":"Zürich","display_name":"Zürich, Schweiz"},{"lat":"47.4","lon":"8.6","type":"village","display_name":"Zürich Nord"}]"#.to_owned())
            } else if line.contains("/meteo/elevation?latitude=0.00000") {
                ("400 Bad Request", r#"{"error":true,"reason":"Latitude must be in range of -90 to 90°."}"#.to_owned())
            } else if line.contains("/meteo/elevation?") {
                ("200 OK", r#"{"elevation":[408.0]}"#.to_owned())
            } else if line.contains("/ci/intensity/stats/") {
                window += 1;
                ("200 OK", format!(r#"{{"data":[{{"from":"a","to":"b","intensity":{{"max":{},"average":{},"min":{},"index":"moderate"}}}}]}}"#, 200 + window, 100 + window, 20 + window))
            } else {
                ("404 Not Found", "{}".to_owned())
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    format!("http://{addr}")
}

#[test]
fn site_connectors() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let base = serve(seen.clone());
    let config = tempfile::tempdir().unwrap();
    // SAFETY: this test binary has one test.
    unsafe {
        std::env::set_var("REVIERA_GEOCODER_URL", format!("{base}/geo"));
        std::env::set_var("REVIERA_OPEN_METEO_URL", format!("{base}/meteo/"));
        std::env::set_var("REVIERA_CARBON_INTENSITY_URL", format!("{base}/ci"));
        std::env::set_var("XDG_CONFIG_HOME", config.path());
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let names: Vec<String> = ok(&mut app, "connectors.list", json!({})).as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["pvgis", "nominatim", "open_meteo", "uk_carbon_intensity"]);

    // Geocoding: the query encoded, the User-Agent identifying, the places read.
    let r = ok(&mut app, "geocode.search", json!({ "query": "  Bahnhofstrasse 1,  Zürich " }));
    assert_eq!(r["cached"], false);
    assert_eq!(r["places"].as_array().unwrap().len(), 2);
    assert_eq!(r["places"][0]["name"], "Zürich, Schweiz");
    assert_eq!(r["places"][0]["lat"], 47.3769);
    assert_eq!(r["places"][1]["kind"], "village");
    assert!(r["attribution"].as_str().unwrap().contains("OpenStreetMap"));
    {
        let s = seen.lock().unwrap();
        let (line, agent, _) = s.last().unwrap();
        assert!(line.contains("/geo/search?q=Bahnhofstrasse%201%2C%20Z%C3%BCrich&format=jsonv2&limit=5 "), "{line}");
        assert!(agent.starts_with("BIM-Reviera/") && agent.contains("github.com"), "{agent}");
    }
    // The same search again: from the cache, no request.
    let n = seen.lock().unwrap().len();
    assert_eq!(ok(&mut app, "geocode.search", json!({ "query": "bahnhofstrasse 1, zürich" }))["cached"], true);
    assert_eq!(seen.lock().unwrap().len(), n);
    // A new search waits a second after the last request.
    assert_eq!(ok(&mut app, "geocode.search", json!({ "query": "Nowhere", "limit": 3 }))["places"], json!([]));
    {
        let s = seen.lock().unwrap();
        assert!(s[s.len() - 1].2.duration_since(s[s.len() - 2].2).as_millis() >= 1000);
    }
    assert_eq!(app.execute("geocode.search", json!({ "query": " " }), &Caller::user()).unwrap_err().code, "geocode.empty");

    // Elevation needs a location, then writes it.
    assert_eq!(app.execute("elevation.fetch", json!({}), &Caller::user()).unwrap_err().code, "connectors.no_location");
    ok(&mut app, "location.set", json!({ "lat": 47.3769, "lon": 8.5417 }));
    let e = ok(&mut app, "elevation.fetch", json!({}));
    assert_eq!(e["elevation"], 408.0);
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    assert_eq!(reviera_core::location::get(&store).elevation, 408.0);
    assert_eq!(reviera_core::location::get(&store).latitude, Some(47.3769));
    assert!(seen.lock().unwrap().last().unwrap().0.contains("/meteo/elevation?latitude=47.37690&longitude=8.54170 "));
    ok(&mut app, "location.set", json!({ "lat": 0.0, "lon": 0.0 }));
    let err = app.execute("elevation.fetch", json!({ "set": false }), &Caller::user()).unwrap_err();
    assert!(err.message.contains("Latitude must be in range"), "{}", err.message);

    // Grid intensity: one 30-day window, or twelve for a year, in kg/kWh.
    let g = ok(&mut app, "grid_intensity.fetch", json!({}));
    assert!((g["average"].as_f64().unwrap() - 0.101).abs() < 1e-9, "{g}");
    assert_eq!(g["region"], "Great Britain");
    let line = seen.lock().unwrap().last().unwrap().0.clone();
    assert!(line.contains("/ci/intensity/stats/") && line.contains("T00:00Z/"), "{line}");
    let y = ok(&mut app, "grid_intensity.fetch", json!({ "period": "year" }));
    // Windows 2–13: averages 102–113, mean 107.5 g/kWh; extremes over all twelve.
    assert!((y["average"].as_f64().unwrap() - 0.1075).abs() < 1e-9, "{y}");
    assert!((y["max"].as_f64().unwrap() - 0.213).abs() < 1e-9 && (y["min"].as_f64().unwrap() - 0.022).abs() < 1e-9);

    // Switched off: refused without a request.
    ok(&mut app, "connectors.set", json!({ "name": "nominatim", "enabled": false }));
    let n = seen.lock().unwrap().len();
    assert_eq!(app.execute("geocode.search", json!({ "query": "Bern" }), &Caller::user()).unwrap_err().code, "connectors.off");
    assert_eq!(seen.lock().unwrap().len(), n);
}
