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
