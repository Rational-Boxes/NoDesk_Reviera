// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Default surface styles on save: walls, floors and windows get their classes' colours, a
//! window's glass is translucent and its frame is not, a second save adds nothing, and IFC4X3
//! files get them too.

use std::collections::BTreeMap;

use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{AttrValue, IfcModel, Schema, StepId};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn model(schema: Schema, path: &std::path::Path) -> (LocalProject, Document) {
    let (project, mut doc) = LocalProject::new(schema, path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let wall = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": [0.0, 0.0], "end": [6.0, 0.0] } }, "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } })));
    ok(&r, &mut doc, "window.place", json!({ "host_wall_id": wall.0, "point_along": 3.0 }));
    ok(&r, &mut doc, "floor.create", json!({ "loops": [rect([0.0, -4.0], [6.0, 0.0])], "level_id": l1.0 }));
    (project, doc)
}

fn rect(a: [f64; 2], b: [f64; 2]) -> Value {
    let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
    Value::Array((0..4).map(|i| json!({ "line": { "start": c[i], "end": c[(i + 1) % 4] } })).collect())
}

/// Each styled item's surface style name and transparency, by the class of the element whose
/// body it styles.
fn styles_by_class(ifc: &IfcModel) -> BTreeMap<String, Vec<(String, f64)>> {
    let mut out: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    for class in ["IfcWall", "IfcWindow", "IfcSlab"] {
        for e in ifc.entities_of_type(class).unwrap() {
            let el = ifc.attributes_many(&[e.id]).unwrap().remove(0);
            for shape in el.refs("Representation") {
                let reps = ifc.attributes_many(&[shape]).unwrap().remove(0).refs("Representations");
                for rep in ifc.attributes_many(&reps).unwrap() {
                    if rep.string("RepresentationIdentifier") != Some("Body") {
                        continue;
                    }
                    for item in rep.refs("Items") {
                        for styled in ifc.referenced_by(item, "IfcStyledItem").unwrap() {
                            out.entry(class.to_owned()).or_default().push(style_of(ifc, styled));
                        }
                    }
                }
            }
        }
    }
    out
}

fn style_of(ifc: &IfcModel, styled: StepId) -> (String, f64) {
    let styles = ifc.attributes_many(&[styled]).unwrap().remove(0).refs("Styles");
    let style = ifc.attributes_many(&[styles[0]]).unwrap().remove(0);
    let shading = ifc.attributes_many(&style.refs("Styles")).unwrap().remove(0);
    let t = match shading.get("Transparency") {
        AttrValue::Real(t) => *t,
        _ => 0.0,
    };
    (style.string("Name").unwrap().to_owned(), t)
}

#[test]
fn walls_floors_and_windows_are_coloured() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styles.ifc");
    let (mut project, doc) = model(Schema::Ifc4, &path);
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let by = styles_by_class(&ifc);
    assert!(by["IfcWall"].iter().all(|(n, t)| n == "Reviera default: IfcWall" && *t == 0.0), "{by:?}");
    assert!(by["IfcSlab"].iter().all(|(n, _)| n == "Reviera default: IfcSlab"), "{by:?}");
    // The window's frame is opaque and its glass translucent.
    let window = &by["IfcWindow"];
    assert!(window.iter().any(|(n, t)| n == "Reviera default: IfcWindow:frame" && *t == 0.0), "{window:?}");
    assert!(window.iter().all(|(n, _)| n != "Reviera default: IfcWindow"), "{window:?}");
    assert!(window.iter().any(|(n, t)| n == "Reviera default: IfcWindow:glass" && *t > 0.5), "{window:?}");
    // One shared style per colour.
    let walls = ifc.entities_of_type("IfcSurfaceStyle").unwrap().len();
    let styled = ifc.entities_of_type("IfcStyledItem").unwrap().len();

    // Saved again after a change: nothing doubled.
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    let r = registry();
    let wall = doc.store().iter().find(|e| e.class == "IfcWall").unwrap().id();
    ok(&r, &mut doc, "element.move", json!({ "element_ids": [wall.0], "vector": [0.0, 0.5, 0.0] }));
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert_eq!(ifc.entities_of_type("IfcSurfaceStyle").unwrap().len(), walls);
    assert_eq!(ifc.entities_of_type("IfcStyledItem").unwrap().len(), styled);
}

#[test]
fn ifc4x3_models_are_coloured_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styles4x3.ifc");
    let (mut project, doc) = model(Schema::Ifc4x3Add2, &path);
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let by = styles_by_class(&ifc);
    assert!(by["IfcWall"].iter().all(|(n, _)| n == "Reviera default: IfcWall"), "{by:?}");
    assert!(by["IfcWindow"].iter().any(|(n, t)| n == "Reviera default: IfcWindow:glass" && *t > 0.5), "{by:?}");
}
