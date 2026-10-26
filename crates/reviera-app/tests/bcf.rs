// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BCF viewpoints (COL-BCF-003): a 3D view of the views sample exported as a .bcfv with its
//! section box, isolate and selection, and shown again in another model's view: the box within a
//! millimetre, the camera within a tenth of a degree, the same three elements shown and the same
//! one selected; a slice plane round-trips too, and GlobalIds not in the model are reported.

use std::path::Path;

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn open(work: &Path, tag: &str) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.join(tag) });
    let copy = work.join(format!("{tag}.ifc"));
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples/views_inspection.ifc"), &copy).unwrap();
    ok(&mut app, "file.open", json!({ "path": copy }));
    app
}

fn view(app: &mut App, name: &str) -> u64 {
    let all = ok(app, "view.list", json!({ "kind": "view.3d" }));
    all.as_array().unwrap().iter().find(|v| v["name"] == name).unwrap_or_else(|| panic!("no view {name}"))["id"].as_u64().unwrap()
}

fn walls(app: &App) -> Vec<(u64, String)> {
    let store = app.document(app.active().unwrap()).unwrap().doc.store();
    let mut w: Vec<(u64, String)> = store.iter().filter(|e| e.class == "IfcWall").map(|e| (e.id().0, e.guid().to_string())).collect();
    w.sort();
    w
}

fn angle(a: &Value, b: &Value) -> f64 {
    let v = |c: &Value| -> [f64; 3] {
        let (e, t) = (&c["eye"], &c["target"]);
        [0, 1, 2].map(|k| t[k].as_f64().unwrap() - e[k].as_f64().unwrap())
    };
    let (x, y) = (v(a), v(b));
    let d = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    (d(x, y) / (d(x, x) * d(y, y)).sqrt()).clamp(-1.0, 1.0).acos().to_degrees()
}

#[test]
fn viewpoints_round_trip_through_bcf_files() {
    let work = tempfile::tempdir().unwrap();
    let mut a = open(work.path(), "a");
    let kitchen = view(&mut a, "Section Box - Kitchen");
    let w = walls(&a);
    // Three walls isolated, one selected.
    let three: Vec<u64> = w.iter().take(3).map(|x| x.0).collect();
    ok(&mut a, "view.temp_isolate", json!({ "view_id": kitchen, "element_ids": three }));
    ok(&mut a, "selection.set", json!({ "ids": [three[1]] }));
    let file = work.path().join("kitchen.bcfv");
    let out = ok(&mut a, "bcf.export_viewpoint", json!({ "view_id": kitchen, "path": file }));
    assert_eq!((out["clipping_planes"].as_u64(), out["selected"].as_u64(), out["default_visibility"].as_bool(), out["exceptions"].as_u64()), (Some(6), Some(1), Some(false), Some(3)), "{out}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("<PerspectiveCamera>") && text.contains("DefaultVisibility=\"false\""), "{text}");

    // Shown in another copy's orthographic view: the kitchen's box, camera, isolate and selection.
    let mut b = open(work.path(), "b");
    let target = view(&mut b, "Orthographic");
    let r = ok(&mut b, "bcf.import_viewpoint", json!({ "path": file, "view_id": target }));
    assert_eq!((r["section_box"].as_bool(), r["slice_planes"].as_u64(), r["isolated"].as_bool(), r["exceptions"].as_u64(), r["selected"].as_u64()), (Some(true), Some(0), Some(true), Some(3), Some(1)), "{r}");
    assert_eq!(r["not_found"], json!([]));
    let (was, now) = (ok(&mut a, "view.get", json!({ "view_id": kitchen })), ok(&mut b, "view.get", json!({ "view_id": target })));
    for k in 0..3 {
        for end in ["min", "max"] {
            let (p, q) = (was["sectionBox"][end][k].as_f64().unwrap(), now["sectionBox"][end][k].as_f64().unwrap());
            assert!((p - q).abs() < 1e-3, "{end}[{k}]: {p} vs {q}");
        }
    }
    assert!(angle(&was["camera"], &now["camera"]) < 0.1, "{} {}", was["camera"], now["camera"]);
    assert_eq!(now["camera"]["perspective"], true, "the kitchen view's projection");
    let temp = ok(&mut b, "view.get_temp_state", json!({ "view_id": target }));
    let mut isolated: Vec<u64> = temp["isolated"]["elements"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    isolated.sort();
    let wb = walls(&b);
    // The same walls, by GlobalId (their ids match in a copy, but the match is by GlobalId).
    let want: Vec<u64> = w.iter().take(3).map(|(_, g)| wb.iter().find(|(_, h)| h == g).unwrap().0).collect();
    assert_eq!(isolated, want);
    let sel = ok(&mut b, "selection.get", json!({}));
    assert_eq!(sel["ids"].as_array().map(|v| v.len()), Some(1), "{sel}");

    // A slice plane round-trips; a hidden element is a visible default with an exception.
    let slice = view(&mut a, "Vertical Slice");
    ok(&mut a, "view.temp_hide", json!({ "view_id": slice, "element_ids": [w[0].0] }));
    let file = work.path().join("slice.bcfv");
    ok(&mut a, "bcf.export_viewpoint", json!({ "view_id": slice, "path": file }));
    let r = ok(&mut b, "bcf.import_viewpoint", json!({ "path": file, "view_id": target }));
    assert_eq!((r["section_box"].as_bool(), r["slice_planes"].as_u64(), r["isolated"].as_bool(), r["exceptions"].as_u64()), (Some(false), Some(1), Some(false), Some(1)), "{r}");
    let now = ok(&mut b, "view.get", json!({ "view_id": target }));
    let was = ok(&mut a, "view.get", json!({ "view_id": slice }));
    assert_eq!(now["slicePlanes"], was["slicePlanes"]);
    // Undo takes the camera and cut back in one step (the kitchen's box), redo brings the slice.
    ok(&mut b, "edit.undo", json!({}));
    let back = ok(&mut b, "view.get", json!({ "view_id": target }));
    assert!(back["sectionBox"]["enabled"] == true && back.get("slicePlanes").is_none(), "{back}");
    ok(&mut b, "edit.redo", json!({}));

    // Elements not in the model are reported, not fatal.
    let text = std::fs::read_to_string(&file).unwrap().replace(&w[0].1, "0000000000000000000000");
    std::fs::write(&file, text).unwrap();
    let r = ok(&mut b, "bcf.import_viewpoint", json!({ "path": file, "view_id": target }));
    assert_eq!(r["not_found"], json!(["0000000000000000000000"]));
}
