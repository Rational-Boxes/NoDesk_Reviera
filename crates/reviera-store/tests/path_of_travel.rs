// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Path of travel (ANA-ROUTE-001/002): with a wall between start and end and one door, the path
//! passes through the door and its length is within 1 % of the true shortest path keeping the
//! clearance; time = length ÷ walking speed; a waypoint lengthens it; moving the door and
//! updating re-routes; a component in the way is an obstacle unless its category is ignored.

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
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

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn info(r: &CommandRegistry, doc: &mut Document, p: ElementId) -> Value {
    ok(r, doc, "path_of_travel.list", json!({})).as_array().unwrap().iter().find(|x| x["id"] == p.0).unwrap().clone()
}

#[test]
fn path_of_travel() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("route.ifc")).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0, -5], "end": [0, 5] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    let door = id(&ok(&r, &mut doc, "door.place", json!({ "host_wall_id": wall.0, "point_along": 7.0 })));
    let plan = id(&ok(&r, &mut doc, "view.create_plan", json!({ "level_ids": [l1.0] }))[0]);

    let p = id(&ok(&r, &mut doc, "path_of_travel.create", json!({ "view_id": plan.0, "start": [-3, 0], "end": [3, 0] })));
    let i = info(&r, &mut doc, p);
    let route: Vec<[f64; 2]> = serde_json::from_value(i["route"].clone()).unwrap();
    // Through the door: the route crosses x = 0 within the door's width of y = 2.
    let st = doc.store().clone();
    let d = reviera_core::inserts::placed(&st).into_iter().find(|d| d.id == door).unwrap();
    let w = d.ty.width;
    let half = reviera_core::walls::regenerate(&st, st.get(wall).unwrap()).unwrap().width / 2.0;
    let crossing = route.windows(2).find_map(|s| {
        let (a, b) = (s[0], s[1]);
        ((a[0] < 0.0) != (b[0] < 0.0)).then(|| a[1] + (b[1] - a[1]) * (0.0 - a[0]) / (b[0] - a[0]))
    });
    let y = crossing.expect("the route crosses the wall");
    assert!((y - 2.0).abs() < w / 2.0, "through the door at y = {y}");
    // The true shortest path keeping 0.3 m: round the 0.3 m arcs at the lower wall piece's top
    // corners (±half, 2 − w/2), across between them.
    let c = 0.3;
    let corner = [-half, 2.0 - w / 2.0];
    let (dx, dy) = (corner[0] + 3.0, corner[1]);
    let dd = dx.hypot(dy);
    let ang = dy.atan2(dx) + (c / dd).asin();
    let truth = 2.0 * ((dd * dd - c * c).sqrt() + c * ang) + 2.0 * half;
    let len = i["length"].as_f64().unwrap();
    assert!(len >= truth - 1e-6 && len <= truth * 1.01, "{len} against {truth}");
    assert!((i["time"].as_f64().unwrap() - len / 1.3).abs() < 1e-9, "time = length ÷ 1.3 m/s");

    // A waypoint makes it longer; removing it gives the first route back.
    let longer = ok(&r, &mut doc, "path_of_travel.add_waypoint", json!({ "id": p.0, "point": [-2, -3] })).as_f64().unwrap();
    assert!(longer > len + 1.0);
    let back = ok(&r, &mut doc, "path_of_travel.add_waypoint", json!({ "id": p.0, "point": [0, 0], "index": 0, "remove": true })).as_f64().unwrap();
    assert!((back - len).abs() < 1e-9);

    // The door moves down: updating re-routes, shorter.
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [door.0], "vector": [0.0, -2.0, 0.0] }));
    let changed = ok(&r, &mut doc, "path_of_travel.update", json!({ "view_id": plan.0 }));
    assert_eq!(changed, json!([p.0]));
    assert!(info(&r, &mut doc, p)["length"].as_f64().unwrap() < len);

    // A box on the straight line between two new points: an obstacle, unless ignored.
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit"] }));
    ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [-6.0, 10.0], "level": l1.0 }));
    let q = id(&ok(&r, &mut doc, "path_of_travel.create", json!({ "view_id": plan.0, "start": [-12, 10], "end": [0, 10] })));
    let around = info(&r, &mut doc, q)["length"].as_f64().unwrap();
    assert!(around > 12.0 + 1e-3, "{around}");
    let cat = reviera_core::categories::category_of(doc.store().iter().find(|e| reviera_core::mesh_objects::mesh_object_recipe(e).is_some()).unwrap()).unwrap().key;
    ok(&r, &mut doc, "route_settings.set", json!({ "ignored_categories": [cat] }));
    ok(&r, &mut doc, "path_of_travel.update", json!({ "ids": [q.0] }));
    assert!((info(&r, &mut doc, q)["length"].as_f64().unwrap() - 12.0).abs() < 1e-9);

    // Starting inside the wall is refused; settings are checked.
    assert_eq!(run(&r, &mut doc, "path_of_travel.create", json!({ "view_id": plan.0, "start": [0, -4], "end": [3, 0] })).unwrap_err().code, "path_of_travel.no_route");
    assert!(run(&r, &mut doc, "route_settings.set", json!({ "zone_top": 0.1 })).is_err());
}
