// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Traffic (ARC-VT-018, ARC-VT-019): an office tower's elevator group, its up-peak against the
//! office targets with the population estimated from its floors or given; a second group serving
//! the same floors raising the target for migration; escalators' capacity against a demand with
//! one unit out.

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") })
}

fn run(app: &mut App, name: &str, args: Value) -> riviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    run(app, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn elevator_groups_and_escalators_against_their_targets() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    ok(&mut app, "file.new", json!({}));
    let mut levels = vec![ok(&mut app, "level.list", json!({}))[0]["id"].clone()];
    for k in 1..=10 {
        levels.push(ok(&mut app, "level.create", json!({ "elevation": 3.5 * f64::from(k) })));
    }
    // 30 × 20 m floors: 600 m² each less the hoistways, about 6000 m² above the lobby, about 600
    // people at 10 m² a person.
    let c = [[0.0, 0.0], [30.0, 0.0], [30.0, 20.0], [0.0, 20.0]];
    let curves: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect();
    for l in &levels {
        ok(&mut app, "floor.create", json!({ "loops": [curves], "level_id": l }));
    }
    // Four cars serving all eleven levels.
    for k in 0..4 {
        ok(&mut app, "elevator.create", json!({ "point": [5.0 + 3.2 * f64::from(k), 8.0], "base_level": levels[0], "top_level": levels[10], "equipment": false }));
    }
    let g = ok(&mut app, "elevator.traffic", json!({}));
    let g = &g[0];
    assert_eq!((g["cars"].as_u64(), g["floors"].as_u64(), g["estimated"].as_bool()), (Some(4), Some(10), Some(true)), "{g:#}");
    let pop = f(&g["population"]);
    assert!(pop > 550.0 && pop <= 600.0, "{g:#}");
    // The figures hang together: interval = RTT / 4, HC5 = 300 · P · 4 / RTT, handling = HC5 / population.
    assert!((f(&g["interval"]) - f(&g["rtt"]) / 4.0).abs() < 1e-9);
    assert!((f(&g["hc5"]) - 300.0 * f(&g["passengers"]) * 4.0 / f(&g["rtt"])).abs() < 1e-9);
    assert!((f(&g["handling"]) - f(&g["hc5"]) / pop).abs() < 1e-9);
    // The passengers are 80 % of the lesser of persons by mass and by area (0.21 m² each).
    let persons = g["persons_by_mass"].as_u64().unwrap().min(g["persons_by_area"].as_u64().unwrap()) as f64;
    assert!((f(&g["passengers"]) - 0.8 * persons).abs() < 1e-9);
    // At 1.0 m/s over ten floors four cars are slow: the 30 s office interval is not met.
    assert_eq!(g["meets"], false, "{g:#}");
    assert_eq!(f(&g["target_interval"]), 30.0);

    // A given population, and the hotel targets: by area 0.3 m² a passenger.
    let h = ok(&mut app, "elevator.traffic", json!({ "building_use": "hotel", "population": 300 }));
    assert_eq!((h[0]["estimated"].as_bool(), f(&h[0]["population"])), (Some(false), 300.0));
    assert!(h[0]["persons_by_area"].as_u64() < g["persons_by_area"].as_u64());
    // Homes need a population: no estimate from floor area.
    assert!(run(&mut app, "elevator.traffic", json!({ "building_use": "residential" })).is_err());

    // A second group (a shuttle to Level 5) shares floors: 120 % of the target for migration.
    ok(&mut app, "elevator.create", json!({ "point": [25.0, 8.0], "base_level": levels[0], "top_level": levels[5], "equipment": false }));
    let both = ok(&mut app, "elevator.traffic", json!({}));
    assert_eq!(both.as_array().unwrap().len(), 2);
    assert!(both.as_array().unwrap().iter().all(|x| (f(&x["target_handling"]) - 0.144).abs() < 1e-9), "{both:#}");

    // An escalator pair from Level 1 to Level 2, each about 6000 persons an hour.
    let up = ok(&mut app, "escalator.create", json!({ "point": [3.0, 15.0], "base_level": levels[0], "top_level": levels[1] }));
    let down = ok(&mut app, "escalator.create", json!({ "point": [3.0, 17.0], "base_level": levels[0], "top_level": levels[1] }));
    let t = ok(&mut app, "escalator.traffic", json!({ "up": [up["escalator"]], "down": [down["escalator"]], "demand_up": 3000, "demand_down": 2000, "one_out": false }));
    assert_eq!((t["up"]["meets"].as_bool(), f(&t["up"]["utilisation"])), (Some(true), 0.5));
    // With one unit out each way, nothing is left: the demand falls to the stairs.
    let t = ok(&mut app, "escalator.traffic", json!({ "up": [up["escalator"]], "down": [down["escalator"]], "demand_up": 3000, "demand_down": 2000 }));
    assert_eq!((t["up"]["meets"].as_bool(), t["notes"].as_array().map(Vec::len)), (Some(false), Some(2)));
}
