// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Egress routes across levels (ANA-ROUTE-004): from a point upstairs to an exit downstairs by the
//! stair, round a wall; an escalator taken only where the egress mode counts it.

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
fn routes_go_down_the_stairs_and_by_escalator_where_counted() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let l1 = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let l2 = ok(&mut app, "level.create", json!({ "elevation": 3.6 }));
    // A straight stair at the west end, rising east from (2, 4).
    ok(&mut app, "stair.create", json!({ "base_level": l1, "width": 1.2, "railings": false, "runs": [{ "start": [2.0, 4.0], "end": [7.0, 4.0] }] }));
    // A wall across Level 1 east of the stair, with a gap at its north end.
    ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": [12.0, -20.0], "end": [12.0, 8.0] } }, "base_level": l1, "top_constraint": { "unconnected": 3.0 } }));

    // From the east end of Level 2 to an exit at the east end of Level 1: across Level 2 to the
    // stair's head, down it, then round the wall's north end.
    let r = ok(&mut app, "egress.route", json!({ "start": { "point": [20.0, 4.0], "level": l2 }, "end": { "point": [20.0, 2.0], "level": l1 } }));
    assert_eq!(r["links"].as_array().unwrap().len(), 1, "{r:#}");
    assert_eq!(r["links"][0]["kind"], "stair");
    let legs = r["legs"].as_array().unwrap();
    assert_eq!((legs.len(), legs[0]["level"].clone(), legs[1]["level"].clone()), (2, l2.clone(), l1.clone()));
    // The second leg goes round the wall (north of y = 8).
    assert!(legs[1]["points"].as_array().unwrap().iter().any(|p| f(&p[1]) > 8.0), "{r:#}");
    // Time: legs at the walking speed (1.3 m/s), the stair at 0.6 m/s.
    let walk: f64 = legs.iter().map(|g| f(&g["length"])).sum();
    let stair = f(&r["links"][0]["length"]);
    assert!((f(&r["time"]) - (walk / 1.3 + stair / 0.6)).abs() < 1e-6, "{r:#}");

    // An escalator in the middle: under the IBC still the stair; under NFPA 130 the escalator.
    ok(&mut app, "escalator.create", json!({ "point": [22.0, 10.0], "rotation": std::f64::consts::PI, "base_level": l1, "top_level": l2 }));
    let ibc = ok(&mut app, "egress.route", json!({ "start": { "point": [20.0, 4.0], "level": l2 }, "end": { "point": [20.0, 2.0], "level": l1 } }));
    assert_eq!(ibc["links"][0]["kind"], "stair");
    let nfpa = ok(&mut app, "egress.route", json!({ "start": { "point": [20.0, 4.0], "level": l2 }, "end": { "point": [20.0, 2.0], "level": l1 }, "mode": "nfpa130" }));
    assert_eq!(nfpa["links"][0]["kind"], "escalator", "{nfpa:#}");
    assert!(f(&nfpa["time"]) < f(&ibc["time"]));

    // No way between levels without a link.
    let l3 = ok(&mut app, "level.create", json!({ "elevation": 7.2 }));
    assert!(app.execute("egress.route", json!({ "start": { "point": [1.0, 1.0], "level": l3 }, "end": { "point": [20.0, 2.0], "level": l1 } }), &Caller::user()).is_err());
}

#[test]
fn a_drawn_route_puts_a_path_of_travel_in_each_levels_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let l1 = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let l2 = ok(&mut app, "level.create", json!({ "elevation": 3.6 }));
    ok(&mut app, "stair.create", json!({ "base_level": l1, "width": 1.2, "railings": false, "runs": [{ "start": [2.0, 4.0], "end": [7.0, 4.0] }] }));
    ok(&mut app, "view.create_plan", json!({ "level_ids": [l1, l2] }));
    let args = json!({ "start": { "point": [20.0, 4.0], "level": l2 }, "end": { "point": [20.0, 2.0], "level": l1 } });
    let drawn = ok(&mut app, "egress.draw_route", args.clone());
    let paths = drawn["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 2, "{drawn:#}");
    // Each path follows its leg, end to end, at the leg's length.
    let listed = ok(&mut app, "path_of_travel.list", json!({}));
    for (leg, id) in drawn["legs"].as_array().unwrap().iter().zip(paths) {
        let p = listed.as_array().unwrap().iter().find(|p| p["id"] == *id).unwrap_or_else(|| panic!("{listed:#}"));
        assert!((f(&p["length"]) - f(&leg["length"])).abs() < 1e-6, "{p:#} {leg:#}");
    }
    // The route itself is as egress.route finds it.
    assert_eq!(drawn["time"], ok(&mut app, "egress.route", args)["time"]);
}

#[test]
fn a_platform_clears_through_its_stairs_and_half_from_its_escalators() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let platform = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let concourse = ok(&mut app, "level.create", json!({ "elevation": 3.6 }));
    ok(&mut app, "stair.create", json!({ "base_level": platform, "width": 1.5, "railings": false, "runs": [{ "start": [2.0, 4.0], "end": [7.0, 4.0] }] }));
    let args = |load: f64, mode: &str| json!({ "level": platform, "load": load, "remote": [40.0, 4.0], "safety": { "point": [20.0, 4.0], "level": concourse }, "mode": mode });

    // Stairs alone: 1500 mm at 0.0555 a minute a millimetre.
    let c = ok(&mut app, "egress.platform_clearing", args(250.0, "ibc"));
    assert!((f(&c["capacity"]) - 1500.0 * 0.0555).abs() < 1e-6, "{c:#}");
    assert!((f(&c["platform_time"]) - 250.0 / 83.25).abs() < 1e-6);
    assert_eq!(c["meets_platform"], true);
    // Walked at 37.7 m/min with the 3.6 m climb at 15 m/min; the wait is the platform time less
    // the walk to the stair.
    let walked: f64 = c["route"]["legs"].as_array().unwrap().iter().map(|g| f(&g["length"])).sum();
    assert!((f(&c["walking_time"]) - (walked / 37.7 + 3.6 / 15.0)).abs() < 1e-6, "{c:#}");
    let wait = f(&c["platform_time"]) - f(&c["route"]["legs"][0]["length"]) / 37.7;
    assert!((f(&c["wait"]) - wait).abs() < 1e-6 && (f(&c["safety_time"]) - (f(&c["walking_time"]) + wait)).abs() < 1e-6);

    // A pair of escalators: one out; what is left counts, up to the stairs' share.
    ok(&mut app, "escalator.create", json!({ "point": [22.0, 10.0], "base_level": platform, "top_level": concourse }));
    ok(&mut app, "escalator.create", json!({ "point": [22.0, 13.0], "base_level": platform, "top_level": concourse }));
    let c = ok(&mut app, "egress.platform_clearing", args(400.0, "nfpa130"));
    assert_eq!(c["exits"].as_array().unwrap().len(), 3, "{c:#}");
    assert!((f(&c["counted"]) - f(&c["escalators"]).min(83.25)).abs() < 1e-6 && f(&c["counted"]) > 0.0);
    assert!((f(&c["platform_time"]) - 400.0 / (83.25 + f(&c["counted"]))).abs() < 1e-6);
    // Too many for the stairs alone within 4 minutes.
    assert_eq!(ok(&mut app, "egress.platform_clearing", args(400.0, "ibc"))["meets_platform"], false);

    // The load worked out (NFPA 130 5.5.5.1): two trains entering at once and those waiting,
    // against a 40 × 10 m platform at 0.65 m² a person (615): the trains (650) govern.
    let lp = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let sq = [[0.0, 0.0], [40.0, 0.0], [40.0, 10.0], [0.0, 10.0]];
    let loop_: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": sq[i], "end": sq[(i + 1) % 4] } })).collect();
    ok(&mut app, "floor.create", json!({ "loops": [loop_], "level_id": lp }));
    let occ = |o: Value| json!({ "level": platform, "occupancy": o, "remote": [40.0, 4.0], "safety": { "point": [20.0, 4.0], "level": concourse } });
    let c = ok(&mut app, "egress.platform_clearing", occ(json!({ "trains": [300.0, 250.0], "entraining": 100.0, "area_per_person": 0.65 })));
    assert_eq!((f(&c["load"]), c["load_basis"]["from"].as_str()), (650.0, Some("trains")), "{c:#}");
    assert!((f(&c["load_basis"]["area"]) - 400.0).abs() < 1e-6 && (f(&c["load_basis"]["by_area"]) - 400.0 / 0.65).abs() < 1e-6);
    // A larger area given: the area governs.
    let c = ok(&mut app, "egress.platform_clearing", occ(json!({ "trains": [300.0, 250.0], "entraining": 100.0, "area_per_person": 0.65, "area": 520.0 })));
    assert_eq!((f(&c["load"]), c["load_basis"]["from"].as_str()), (800.0, Some("area")));
    // A load and an occupancy together are refused.
    let both = json!({ "level": platform, "load": 100.0, "occupancy": { "trains": [100.0] }, "remote": [40.0, 4.0], "safety": { "point": [20.0, 4.0], "level": concourse } });
    assert!(app.execute("egress.platform_clearing", both, &Caller::user()).is_err());
}

#[test]
fn occupant_evacuation_lifts_are_taken_only_when_marked_and_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let ground = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    // Eleven floors 3.5 m apart; a stair straight up at the west end, stacked flight on flight.
    let mut levels = vec![ground.clone()];
    for k in 1..=10 {
        levels.push(ok(&mut app, "level.create", json!({ "elevation": 3.5 * f64::from(k) })));
    }
    for k in 0..10 {
        ok(&mut app, "stair.create", json!({ "base_level": levels[k], "width": 1.2, "railings": false, "runs": [{ "start": [2.0, 4.0 + 3.0 * (k % 2) as f64], "end": [7.5, 4.0 + 3.0 * (k % 2) as f64] }] }));
    }
    // An elevator 60 m east, serving every floor.
    let lift = ok(&mut app, "elevator.create", json!({ "point": [60.0, 20.0], "base_level": levels[0], "top_level": levels[10], "walls": false, "equipment": false }))["elevator"].clone();
    let from = json!({ "point": [60.0, 15.0], "level": levels[10] });
    let to = json!({ "point": [60.0, 15.0], "level": ground });
    let route = |app: &mut App, lifts: bool| ok(app, "egress.route", json!({ "start": from, "end": to, "lifts": lifts }));

    // Not yet marked: the stairs, even when lifts are asked for.
    let unmarked = route(&mut app, true);
    assert!(unmarked["links"].as_array().unwrap().iter().all(|l| l["kind"] == "stair"), "{unmarked:#}");
    // Marked for occupant evacuation: taken when asked, in one ride with its wait.
    assert_eq!(ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": lift, "role": "occupant_evacuation" })), "occupant_evacuation");
    let by_lift = route(&mut app, true);
    let links = by_lift["links"].as_array().unwrap();
    assert_eq!((links.len(), links[0]["kind"].as_str()), (1, Some("lift")), "{by_lift:#}");
    let speed = ok(&mut app, "elevator.list", json!({})).as_array().unwrap().iter().find(|e| e["elevator"] == lift).map(|e| f(&e["speed"])).unwrap();
    assert!((f(&links[0]["time"]) - (60.0 + 10.0 + 35.0 / speed + speed)).abs() < 1e-6, "{by_lift:#}");
    assert!(f(&by_lift["time"]) < f(&unmarked["time"]));
    // Not asked: the stairs. A long wait: the stairs again.
    assert!(route(&mut app, false)["links"].as_array().unwrap().iter().all(|l| l["kind"] == "stair"));
    let slow = ok(&mut app, "egress.route", json!({ "start": from, "end": to, "lifts": true, "lift_wait": 1000.0 }));
    assert!(slow["links"].as_array().unwrap().iter().all(|l| l["kind"] == "stair"), "{slow:#}");
    // A fire service elevator is for firefighters: not taken, and IFC marks it.
    ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": lift, "role": "fire_service" }));
    assert!(route(&mut app, true)["links"].as_array().unwrap().iter().all(|l| l["kind"] == "stair"));
}

#[test]
fn evacuation_lifts_count_in_a_platforms_exit_capacity_with_two_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let platform = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let concourse = ok(&mut app, "level.create", json!({ "elevation": 6.0 }));
    ok(&mut app, "stair.create", json!({ "base_level": platform, "width": 1.5, "railings": false, "runs": [{ "start": [2.0, 4.0], "end": [12.0, 4.0] }] }));
    let lifts: Vec<Value> = (0..3)
        .map(|k| ok(&mut app, "elevator.create", json!({ "point": [30.0 + 4.0 * f64::from(k), 20.0], "base_level": platform, "top_level": concourse, "walls": false, "equipment": false }))["elevator"].clone())
        .collect();
    for l in &lifts {
        ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": l, "role": "occupant_evacuation" }));
    }
    let args = |lifts: bool| json!({ "level": platform, "load": 400.0, "remote": [40.0, 4.0], "safety": { "point": [20.0, 4.0], "level": concourse }, "lifts": lifts });
    let el = ok(&mut app, "elevator.list", json!({}));
    let one = {
        let (persons, speed) = (f(&el[0]["persons"]), f(&el[0]["speed"]));
        persons * 60.0 / (2.0 * (6.0 / speed + speed) + 10.0 + 2.0 * persons * 1.2)
    };

    // Not asked: no lifts.
    assert_eq!(f(&ok(&mut app, "egress.platform_clearing", args(false))["lifts"]), 0.0);
    // Three evacuation lifts and no fire service lift: one out, one kept for firefighters.
    let c = ok(&mut app, "egress.platform_clearing", args(true));
    assert!((f(&c["lifts"]) - one).abs() < 1e-6, "{c:#}");
    assert!((f(&c["capacity"]) - (83.25 + f(&c["lifts_counted"]))).abs() < 1e-6);
    // One of them for fire service: the other two evacuate, one out.
    ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": lifts[2], "role": "fire_service" }));
    let c = ok(&mut app, "egress.platform_clearing", args(true));
    assert!((f(&c["lifts"]) - one).abs() < 1e-6 && c["exits"].as_array().unwrap().iter().filter(|x| x["kind"] == "lift").count() == 2, "{c:#}");
    // Seven more evacuation lifts: more than the stair; only half the whole counts.
    for k in 3..10 {
        let l = ok(&mut app, "elevator.create", json!({ "point": [30.0 + 4.0 * f64::from(k), 20.0], "base_level": platform, "top_level": concourse, "walls": false, "equipment": false }))["elevator"].clone();
        ok(&mut app, "elevator.set_egress_role", json!({ "elevator_id": l, "role": "occupant_evacuation" }));
    }
    let c = ok(&mut app, "egress.platform_clearing", args(true));
    assert!(f(&c["lifts"]) > 83.25 && (f(&c["lifts_counted"]) - 83.25).abs() < 1e-6, "{c:#}");
}

#[test]
fn the_standing_area_stops_at_the_train_and_leaves_out_stairs_and_their_run_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let platform = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let concourse = ok(&mut app, "level.create", json!({ "elevation": 3.6 }));
    // A 100 × 10 m platform; a 1.5 m stair rising east from (20, 5).
    let sq = [[0.0, 0.0], [100.0, 0.0], [100.0, 10.0], [0.0, 10.0]];
    let loop_: Vec<Value> = (0..4).map(|i| json!({ "line": { "start": sq[i], "end": sq[(i + 1) % 4] } })).collect();
    ok(&mut app, "floor.create", json!({ "loops": [loop_], "level_id": platform }));
    ok(&mut app, "stair.create", json!({ "base_level": platform, "width": 1.5, "railings": false, "runs": [{ "start": [20.0, 5.0], "end": [26.0, 5.0] }] }));
    let mut occ = |o: Value| ok(&mut app, "egress.platform_clearing", json!({ "level": platform, "occupancy": o, "remote": [60.0, 2.0], "safety": { "point": [60.0, 2.0], "level": concourse } }));
    // A train 80 m long from x = 10: 800 m² less the stair (its run, 1.5 m wide) and 6 m × 1.5 m
    // before its foot.
    let c = occ(json!({ "trains": [100.0], "area_per_person": 0.5, "train": [[10.0, 5.0], [90.0, 5.0]] }));
    let area = f(&c["load_basis"]["area"]);
    let run = (area - 800.0 + 9.0) / -1.5;
    assert!(run > 5.0 && run < 7.0, "the stair's run is {run} m: {c:#}");
    assert!((f(&c["load"]) - area / 0.5).abs() < 1e-6 && c["load_basis"]["from"] == "area");
    // A train past the platform's end: only the platform counts.
    let c = occ(json!({ "trains": [100.0], "area_per_person": 0.5, "train": [[-50.0, 5.0], [150.0, 5.0]] }));
    assert!((f(&c["load_basis"]["area"]) - (1000.0 - 9.0 - 1.5 * run)).abs() < 0.05, "{c:#}");
}
