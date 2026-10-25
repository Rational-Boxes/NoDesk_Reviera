// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Project location and coordinates (DM-LOC-001): the survey point at E 500000, N 5700000 with
//! EPSG:25832 writes an IfcMapConversion and IfcProjectedCRS; rotating true north by 30°
//! changes TrueNorth and the shared coordinates but no element; a reported point agrees with
//! the survey point within 0.1 mm; the site's latitude and longitude are written as compound
//! angles and read back; bad values are refused; one undo reverts the dialog's change.

use reviera_core::{Caller, CommandRegistry, Document};
use reviera_ifc::{IfcModel, Schema};
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

#[test]
fn location_and_coordinates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loc.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [5, 0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }));

    ok(&r, &mut doc, "location.set", json!({ "lat": 52.52, "lon": 13.405, "elevation": 34.0, "time_zone": 1, "dst": true, "place": "Berlin" }));
    ok(&r, &mut doc, "coordinates.set_survey_point", json!({ "point": [0, 0, 0], "shared_coords": [500000.0, 5700000.0, 34.0] }));
    ok(&r, &mut doc, "coordinates.set_crs", json!({ "epsg_code": "EPSG:25832" }));
    assert!(run(&r, &mut doc, "coordinates.set_crs", json!({ "epsg_code": "UTM 32" })).is_err());
    assert!(run(&r, &mut doc, "location.set", json!({ "lat": 95.0, "lon": 0.0 })).is_err());

    // Reported at (10, 20, 3): 10 m east, 20 m north of the survey point.
    let rep = ok(&r, &mut doc, "coordinates.report", json!({ "point": [10.0, 20.0, 3.0] }));
    let s: Vec<f64> = serde_json::from_value(rep["shared"].clone()).unwrap();
    assert!((s[0] - 500_010.0).abs() < 1e-4 && (s[1] - 5_700_020.0).abs() < 1e-4 && (s[2] - 37.0).abs() < 1e-4, "{s:?}");

    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let find = |class: &str| rows.iter().find(|r| r.class == class).map(|r| r.attributes.clone());
    let mc = find("IFCMAPCONVERSION").expect("a map conversion");
    assert!(mc.contains("500000.") && mc.contains("5700000."), "{mc}");
    assert!(find("IFCPROJECTEDCRS").unwrap().contains("'EPSG:25832'"));
    let site = find("IFCSITE").unwrap();
    assert!(site.contains("(52,31,12,0)") && site.contains("(13,24,18,0)"), "{site}");

    // True north by 30°: TrueNorth turns, the wall does not, shared coordinates turn.
    let before = doc.store().get(reviera_core::ElementId(wall.as_u64().unwrap())).unwrap().params.clone();
    let tn = ok(&r, &mut doc, "position.rotate_true_north", json!({ "angle": 30.0 }));
    assert_eq!(tn, 30.0);
    assert_eq!(doc.store().get(reviera_core::ElementId(wall.as_u64().unwrap())).unwrap().params, before);
    let rep = ok(&r, &mut doc, "coordinates.report", json!({ "point": [0.0, 10.0, 0.0] }));
    let s: Vec<f64> = serde_json::from_value(rep["shared"].clone()).unwrap();
    // Project north is 30° clockwise of true north, so a point up the plan is east of north.
    assert!((s[0] - 500_000.0 - 10.0 * 30f64.to_radians().sin()).abs() < 1e-4, "{s:?}");
    assert!((s[1] - 5_700_000.0 - 10.0 * 30f64.to_radians().cos()).abs() < 1e-4, "{s:?}");
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.class == "IFCMAPCONVERSION").count(), 1, "replaced, not added");
    assert_eq!(rows.iter().filter(|r| r.class == "IFCPROJECTEDCRS").count(), 1);
    let ctx = rows.iter().find(|r| r.class == "IFCGEOMETRICREPRESENTATIONCONTEXT").unwrap();
    assert!(!ctx.attributes.ends_with(",$)"), "TrueNorth is set: {}", ctx.attributes);

    // Read back.
    let (_, doc2, _) = LocalProject::open(&path).unwrap();
    let l = reviera_core::location::get(doc2.store());
    assert_eq!((l.latitude, l.crs.as_deref(), l.true_north), (Some(52.52), Some("EPSG:25832"), 30.0));

    // The dialog's change in one step, undone in one.
    let mut all = l.clone();
    all.place = Some("Elsewhere".into());
    all.project_base_point = [1.0, 2.0, 0.0];
    ok(&r, &mut doc, "location.set_all", json!({ "location": all }));
    assert_eq!(reviera_core::location::get(doc.store()).project_base_point, [1.0, 2.0, 0.0]);
    let rep = ok(&r, &mut doc, "coordinates.report", json!({ "point": [1.0, 2.0, 0.0] }));
    assert_eq!(rep["project"], json!([0.0, 0.0, 0.0]));
    ok(&r, &mut doc, "edit.undo", json!({}));
    assert_eq!(reviera_core::location::get(doc.store()).project_base_point, [0.0, 0.0, 0.0]);
}

/// A file from another tool with a site latitude and longitude and no Reviera location: the
/// location comes from the site.
#[test]
fn location_from_an_imported_site() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("site.ifc");
    let (mut project, doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    project.save(&doc).unwrap();
    let mut ifc = IfcModel::open(&path).unwrap();
    let site = ifc.entities_of_type("IfcSite").unwrap()[0].id;
    let c = |v: &[i64]| reviera_ifc::AttrValue::List(v.iter().map(|x| reviera_ifc::AttrValue::Int(*x)).collect());
    ifc.set_attribute(site, "RefLatitude", &c(&[-33, -52, -10, 0])).unwrap();
    ifc.set_attribute(site, "RefLongitude", &c(&[151, 12, 30, 0])).unwrap();
    let other = dir.path().join("other.ifc");
    ifc.write(&other).unwrap();
    let (_, doc2, _) = LocalProject::open(&other).unwrap();
    let l = reviera_core::location::get(doc2.store());
    assert!((l.latitude.unwrap() + (33.0 + 52.0 / 60.0 + 10.0 / 3600.0)).abs() < 1e-9, "{l:?}");
    assert!((l.longitude.unwrap() - (151.0 + 12.0 / 60.0 + 30.0 / 3600.0)).abs() < 1e-9);
}
