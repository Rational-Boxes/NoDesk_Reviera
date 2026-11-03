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

#[test]
fn a_file_saved_before_default_styles_gets_them_unedited() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.ifc");
    let (mut project, doc) = model(Schema::Ifc4, &path);
    project.save(&doc).unwrap();
    // As an older Reviera wrote it: no styles at all.
    let mut ifc = IfcModel::open(&path).unwrap();
    for class in ["IfcStyledItem", "IfcSurfaceStyle", "IfcSurfaceStyleShading"] {
        for e in ifc.entities_of_type(class).unwrap() {
            ifc.remove(e.id).unwrap();
        }
    }
    ifc.write(&path).unwrap();
    assert!(IfcModel::open(&path).unwrap().entities_of_type("IfcStyledItem").unwrap().is_empty());

    // Opened and saved with no edit: coloured, windows translucent.
    let (mut project, doc, _) = LocalProject::open(&path).unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let by = styles_by_class(&ifc);
    assert!(by["IfcWall"].iter().all(|(n, _)| n == "Reviera default: IfcWall"), "{by:?}");
    // The window whole (its parts are split only when it is written again), translucent.
    assert!(by["IfcWindow"].iter().all(|(n, t)| n.starts_with("Reviera default: IfcWindow") && *t > 0.4), "{by:?}");
    let styled = ifc.entities_of_type("IfcStyledItem").unwrap().len();
    // Saved again unedited: nothing added.
    project.save(&doc).unwrap();
    assert_eq!(IfcModel::open(&path).unwrap().entities_of_type("IfcStyledItem").unwrap().len(), styled);
}

/// The classes of the styles Reviera's default surface styles hold.
fn default_style_classes(ifc: &IfcModel) -> Vec<String> {
    let styles: Vec<StepId> = ifc.entities_of_type("IfcSurfaceStyle").unwrap().into_iter().map(|e| e.id).collect();
    let mut out = Vec::new();
    for s in ifc.attributes_many(&styles).unwrap() {
        if s.string("Name").is_some_and(|n| n.starts_with("Reviera default")) {
            for inner in s.refs("Styles") {
                out.push(ifc.class_of(inner).unwrap());
            }
        }
    }
    out
}

#[test]
fn styles_are_renderings_so_viewers_see_glass() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glass.ifc");
    let (mut project, doc) = model(Schema::Ifc4, &path);
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let classes = default_style_classes(&ifc);
    assert!(!classes.is_empty() && classes.iter().all(|c| c.eq_ignore_ascii_case("IfcSurfaceStyleRendering")), "{classes:?}");

    // A file an earlier Reviera styled with plain shadings: they become renderings, colour and
    // transparency kept, nothing left behind.
    let mut ifc = IfcModel::open(&path).unwrap();
    let renderings: Vec<StepId> = ifc.entities_of_type("IfcSurfaceStyleRendering").unwrap().into_iter().map(|e| e.id).collect();
    for rid in renderings {
        let old = ifc.attributes_many(&[rid]).unwrap().remove(0);
        let shading = ifc.create("IfcSurfaceStyleShading").unwrap();
        ifc.set_attribute(shading, "SurfaceColour", &AttrValue::Ref(old.refs("SurfaceColour")[0].0)).unwrap();
        if let AttrValue::Real(t) = old.get("Transparency") {
            ifc.set_attribute(shading, "Transparency", &AttrValue::Real(*t)).unwrap();
        }
        for style in ifc.referenced_by(rid, "IfcSurfaceStyle").unwrap() {
            ifc.set_attribute(style, "Styles", &AttrValue::List(vec![AttrValue::Ref(shading.0)])).unwrap();
        }
        ifc.remove(rid).unwrap();
    }
    ifc.write(&path).unwrap();
    assert!(default_style_classes(&IfcModel::open(&path).unwrap()).iter().all(|c| c.eq_ignore_ascii_case("IfcSurfaceStyleShading")));
    let colours = IfcModel::open(&path).unwrap().entities_of_type("IfcColourRgb").unwrap().len();

    let (mut project, doc, _) = LocalProject::open(&path).unwrap();
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    let classes = default_style_classes(&ifc);
    assert!(classes.iter().all(|c| c.eq_ignore_ascii_case("IfcSurfaceStyleRendering")), "{classes:?}");
    assert!(ifc.entities_of_type("IfcSurfaceStyleShading").unwrap().iter().all(|e| !e.class.eq_ignore_ascii_case("IfcSurfaceStyleShading")), "old shadings removed");
    assert_eq!(ifc.entities_of_type("IfcColourRgb").unwrap().len(), colours, "old colours removed");
    let by = styles_by_class(&ifc);
    assert!(by["IfcWindow"].iter().any(|(n, t)| n == "Reviera default: IfcWindow:glass" && *t > 0.5), "{by:?}");
}

#[test]
fn reviera_default_colours_follow_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recolour.ifc");
    let (mut project, doc) = model(Schema::Ifc4, &path);
    project.save(&doc).unwrap();
    // The wall style as an earlier table had it: red.
    let wall_colour = |ifc: &IfcModel| {
        let style = ifc.entities_of_type("IfcSurfaceStyle").unwrap().into_iter().find(|e| e.name.as_deref() == Some("Reviera default: IfcWall")).unwrap().id;
        let rendering = ifc.attributes_many(&[style]).unwrap().remove(0).refs("Styles")[0];
        let colour = ifc.attributes_many(&[rendering]).unwrap().remove(0).refs("SurfaceColour")[0];
        (colour, ifc.attribute(colour, "Red").unwrap())
    };
    let mut ifc = IfcModel::open(&path).unwrap();
    let (colour, red) = wall_colour(&ifc);
    ifc.set_attribute(colour, "Red", &AttrValue::Real(1.0)).unwrap();
    ifc.write(&path).unwrap();

    let (mut project, doc, _) = LocalProject::open(&path).unwrap();
    project.save(&doc).unwrap();
    assert_eq!(wall_colour(&IfcModel::open(&path).unwrap()).1, red);
}
