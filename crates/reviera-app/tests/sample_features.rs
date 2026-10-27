// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The ribbon tabs' showcase samples (`test-data/samples`) show what they say, from their saved
//! files: the annotated plan keeps its dimensions, tags, notes, schedules and sheet; the massing
//! and site sample its mass floors, cut and fill, sub-divisions and property line; the views
//! sample its views with their section boxes, slices, scope box and virtual-volume mode; the
//! steel sample its cuts and connections; the managed project its globals, classification,
//! keynotes, selections and location; the concrete sample its bars and precast pieces; the editing
//! tools sample its path array, pin and parts; the fire protection sample its heads, systems,
//! damper and alarm circuit. Their overlap-free state is checked by `samples.rs`.

use std::path::{Path, PathBuf};

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, Element, ElementId};
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples")
}

/// A fresh copy of a sample opened in its own app.
fn open(work: &Path, name: &str) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: work.join(format!("u-{name}")) });
    let copy = work.join(format!("{name}.ifc"));
    std::fs::copy(samples().join(format!("{name}.ifc")), &copy).unwrap();
    ok(&mut app, "file.open", json!({ "path": copy }));
    app
}

fn store(app: &App) -> reviera_core::Store {
    app.document(app.active().unwrap()).unwrap().doc.store().clone()
}

/// The element named `name` that `pick` accepts.
fn named(store: &reviera_core::Store, name: &str, pick: impl Fn(&Element) -> bool) -> ElementId {
    store
        .iter()
        .find(|e| e.name.as_deref() == Some(name) && pick(e))
        .unwrap_or_else(|| panic!("no element {name}"))
        .id()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{v}"))
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// The views of `app` by name, with their kinds.
fn views(app: &mut App) -> Vec<(String, String, u64)> {
    ok(app, "view.list", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["name"].as_str().unwrap().to_owned(), v["kind"].as_str().unwrap().to_owned(), v["id"].as_u64().unwrap()))
        .collect()
}

fn view_id(app: &mut App, name: &str, kind: &str) -> u64 {
    views(app).into_iter().find(|(n, k, _)| n == name && k == kind).unwrap_or_else(|| panic!("no {kind} {name}")).2
}

#[test]
fn the_annotated_plan_keeps_its_documentation() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let kinds: Vec<String> = views(&mut app).into_iter().map(|(_, k, _)| k).collect();
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    let plan = view_id(&mut app, "Level 1", "view.plan");

    let dims = ok(&mut app, "anno.list", json!({ "view_id": plan }));
    let dims = dims.as_array().unwrap();
    assert_eq!(dims.len(), 4, "{dims:#?}");
    let mut totals: Vec<f64> = dims.iter().map(|d| d["measured"]["values"].as_array().unwrap().iter().map(f).sum()).collect();
    totals.sort_by(f64::total_cmp);
    for (got, want) in totals.iter().zip([5.8, 6.2, 10.2, 10.2]) {
        assert!(near(*got, want, 1e-6), "dimension strings {totals:?}");
    }

    let tags = ok(&mut app, "tag.list", json!({ "view_id": plan }));
    assert_eq!(tags.as_array().unwrap().len(), 11, "3 doors, 5 windows, 3 rooms: {tags:#}");
    let notes = ok(&mut app, "text.list", json!({ "view_id": plan }));
    let notes = notes.as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().any(|n| n["data"]["leaders"].as_array().is_some_and(|l| l.len() == 1)), "a note with a leader");

    let s = store(&app);
    let schedules: Vec<ElementId> = s.iter().filter(|e| reviera_core::schedules::is_schedule(e)).map(|e| e.id()).collect();
    assert_eq!(schedules.len(), 2);
    for id in schedules {
        let t = ok(&mut app, "schedule.get_table", json!({ "schedule_id": id.0 }));
        assert_eq!(t["rows"].as_array().unwrap().len(), 3, "{t:#}");
    }
    let sheets: Vec<_> = s.iter().filter_map(reviera_core::sheets::sheet_data).collect();
    assert_eq!(sheets.len(), 1);
    assert_eq!((sheets[0].number.as_str(), sheets[0].size.as_str()), ("A101", "A2"));
    assert_eq!((sheets[0].viewports.len(), sheets[0].schedules.len()), (3, 2), "plan, section, elevation and both schedules");
}

#[test]
fn the_massing_and_site_sample_keeps_its_masses_and_ground() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "massing_site");
    let s = store(&app);
    let any = |_: &Element| true;

    let podium = named(&s, "Podium", any);
    let floors = ok(&mut app, "mass.floors", json!({ "mass_id": podium.0 }));
    let floors = floors.as_array().unwrap();
    assert_eq!(floors.len(), 2, "{floors:#?}");
    assert!(floors.iter().all(|m| near(f(&m["area"]), 336.0, 1e-6)));
    let tower = named(&s, "Tower", any);
    let floors = ok(&mut app, "mass.floors", json!({ "mass_id": tower.0 }));
    assert_eq!(floors.as_array().unwrap().len(), 4, "{floors:#}");

    let graded = named(&s, "Graded Region", any);
    let cf = ok(&mut app, "toposolid.cut_fill", json!({ "toposolid_id": graded.0 }));
    assert!(f(&cf["fill"]) > f(&cf["cut"]) && f(&cf["cut"]) > 0.0, "the pad is mostly fill: {cf}");

    let line = s.iter().find(|e| e.class.contains("PropertyLine") || e.name.as_deref() == Some("Property Line")).expect("a property line").id();
    let area = ok(&mut app, "propertyline.area", json!({ "element_id": line.0 }));
    assert!(near(f(&area), 2128.0, 0.5), "{area}");
}

#[test]
fn the_views_sample_keeps_its_views_and_their_inspection_state() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "views_inspection");
    let all = views(&mut app);
    let count = |k: &str| all.iter().filter(|(_, kind, _)| kind == k).count();
    assert_eq!(
        [count("view.plan"), count("view.ceiling_plan"), count("view.section"), count("view.elevation"), count("view.3d")],
        [3, 2, 2, 4, 11],
        "{all:#?}"
    );
    let boxes = ok(&mut app, "scopebox.list", json!({}));
    assert_eq!(boxes.as_array().unwrap().len(), 1);

    // Read back with `view.get`.
    let mut get = |name: &str, kind: &str| {
        let id = all.iter().find(|(n, k, _)| n == name && k == kind).unwrap_or_else(|| panic!("no {kind} {name}")).2;
        ok(&mut app, "view.get", json!({ "view_id": id }))
    };
    let b = get("Section Box - Kitchen", "view.3d")["sectionBox"].clone();
    assert!(b["enabled"] == true && near(f(&b["min"][0]), 4.9, 1e-6) && near(f(&b["max"][2]), 2.0, 1e-6), "{b}");
    let h = get("Horizontal Slice", "view.3d")["slicePlanes"].clone();
    assert_eq!(h.as_array().unwrap().len(), 1);
    assert!(near(f(&h[0]["origin"][2]), 1.5, 1e-9) && near(f(&h[0]["normal"][2]), -1.0, 1e-9), "{h}");
    let v = get("Vertical Slice", "view.3d")["slicePlanes"].clone();
    assert!(v.as_array().unwrap().len() == 1 && near(f(&v[0]["normal"][1]), 1.0, 1e-9), "{v}");
    assert_eq!(get("Virtual Volumes", "view.3d")["virtualVolumes"], "translucent");
    let scope = boxes[0]["guid"].clone();
    let east = get("Level 1 - East Half", "view.plan");
    assert!(east["scopeBox"].is_string() && (scope.is_null() || east["scopeBox"] == scope), "{east} {boxes}");
    assert_eq!(get("Level 1", "view.plan")["kind"], "view.plan");
}

#[test]
fn the_steel_connections_sample_keeps_its_cuts_and_connections() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "steel_connections");
    let cuts = ok(&mut app, "steel_cut.list", json!({}));
    let cuts = cuts.as_array().unwrap();
    let kinds = |k: &str| cuts.iter().filter(|c| c["cut"]["kind"] == k).count();
    assert_eq!(cuts.len(), 11, "{cuts:#?}");
    assert_eq!((kinds("cope"), kinds("miter")), (2, 2), "{cuts:#?}");
    let numbers = ok(&mut app, "steel_numbering.preview", json!({}));
    assert!(numbers["parts"].as_array().unwrap().len() >= 20 && numbers["assemblies"].as_array().unwrap().len() >= 16, "{numbers:#}");
    // A custom connection type placed twice, its exposed plate thickness set to 12 mm.
    let custom = ok(&mut app, "custom_connection.list", json!({}));
    assert_eq!(custom[0]["instances"].as_array().map_or(0, Vec::len), 2, "{custom:#}");
    assert!(near(f(&custom[0]["exposed"][0]["value"]), 0.012, 1e-9), "{custom:#}");
    let text = std::fs::read_to_string(samples().join("steel_connections.ifc")).unwrap();
    assert_eq!(text.matches("'STRUCTURAL_CONNECTION'").count(), 18, "two custom connections, their end plates and fourteen automated");
}

#[test]
fn the_managed_project_keeps_its_project_settings() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "managed_project");
    let globals = ok(&mut app, "global.list", json!({}));
    let globals = globals.as_array().or_else(|| globals["result"].as_array()).unwrap().clone();
    let global = |n: &str| globals.iter().find(|g| g["name"] == n).unwrap_or_else(|| panic!("no global {n}: {globals:#?}")).clone();
    assert!(near(f(&global("StoreWidth")["value"]), 3.6, 1e-9));
    assert!(near(f(&global("OfficeWidth")["value"]), 8.4, 1e-6), "a reporting parameter");
    assert!(near(f(&global("OfficeArea")["value"]), 67.2, 1e-6), "a formula");
    assert_eq!(global("SillHeight")["uses"], 4);

    let systems = ok(&mut app, "classification.list", json!({}));
    let systems = systems.as_array().unwrap();
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0]["used_by"], 11, "{systems:#?}");
    let keynotes = ok(&mut app, "keynotes.list", json!({}));
    assert_eq!(keynotes["entries"].as_array().unwrap().len(), 5);
    let sets = ok(&mut app, "selectionset.list", json!({}));
    let counts: Vec<u64> = sets.as_array().unwrap().iter().map(|s| s["count"].as_u64().unwrap()).collect();
    assert_eq!(counts.iter().sum::<u64>(), 10, "{sets:#}");
    let loc = ok(&mut app, "location.get", json!({}));
    assert!(near(f(&loc["latitude"]), 45.52, 1e-9) && near(f(&loc["true_north"]), 12.0, 1e-9), "{loc:#}");
    assert_eq!(loc["crs"], "EPSG:32610");
}

/// The IFC entities of `class` (exactly, by name) in a saved sample.
fn ifc_count(name: &str, class: &str) -> usize {
    let text = std::fs::read_to_string(samples().join(format!("{name}.ifc"))).unwrap();
    text.matches(&format!("={}(", class.to_uppercase())).count()
}

#[test]
fn the_concrete_sample_keeps_its_reinforcement_and_precast_pieces() {
    assert_eq!(ifc_count("concrete_reinforcement", "IfcReinforcingBar"), 32);
    assert_eq!(ifc_count("concrete_reinforcement", "IfcReinforcingMesh"), 6);
    assert_eq!(ifc_count("concrete_reinforcement", "IfcMechanicalFastener"), 4, "lifting anchors");
    // Bars are swept disks, not faceted tubes: the file stays small (it was 16 MB with tubes).
    assert!(ifc_count("concrete_reinforcement", "IfcSweptDiskSolid") > 0);
    let size = std::fs::metadata(samples().join("concrete_reinforcement.ifc")).unwrap().len();
    assert!(size < 3_000_000, "{size} bytes");
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "concrete_reinforcement");
    let s = store(&app);
    let pieces: Vec<u64> = s.iter().filter(|e| reviera_core::precast::assembly_recipe(e).is_some()).map(|e| e.id().0).collect();
    let info = ok(&mut app, "precast.info", json!({ "assembly_ids": pieces }));
    let mut marks: Vec<&str> = info.as_array().unwrap().iter().map(|a| a["mark"].as_str().unwrap()).collect();
    marks.sort();
    assert_eq!(marks, ["S1-1", "S1-2", "W1-1", "W1-2"], "{info:#}");
}

#[test]
fn the_editing_tools_sample_keeps_its_array_pin_and_parts() {
    assert_eq!(ifc_count("modify_tools", "IfcBuildingElementPart"), 12);
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "modify_tools");
    let s = store(&app);
    let pinned: Vec<_> = s.iter().filter(|e| reviera_core::modify::is_pinned(e)).collect();
    assert_eq!(pinned.len(), 1, "one wall pinned");
    let arc = named(&s, "Arc colonnade", |_| true);
    let members = ok(&mut app, "array.members", json!({ "array_id": arc.0 }));
    assert_eq!(members.as_array().map_or(0, Vec::len), 5, "{members:#}");
    let grids = ok(&mut app, "grid.list", json!({}));
    assert_eq!(grids.as_array().unwrap().len(), 3);
}

#[test]
fn the_fire_protection_sample_keeps_its_systems_and_alarm_circuit() {
    assert_eq!(ifc_count("mep_fire", "IfcFireSuppressionTerminal"), 14);
    assert_eq!(ifc_count("mep_fire", "IfcDamper"), 1, "a fire damper in the corridor wall");
    assert_eq!(ifc_count("mep_fire", "IfcValve"), 4, "two on each riser");
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "mep_fire");
    let systems = ok(&mut app, "system.list", json!({}));
    let mut kinds: Vec<&str> = systems.as_array().unwrap().iter().filter_map(|s| s["classification"].as_str()).collect();
    kinds.sort();
    assert_eq!(kinds, ["fire_protection_dry", "fire_protection_wet"], "{systems:#}");
    let circuits = ok(&mut app, "circuit.list", json!({}));
    let alarm: Vec<&Value> = circuits.as_array().unwrap().iter().filter(|c| c["name"] == "FACP-1").collect();
    assert_eq!(alarm.len(), 1, "{circuits:#}");
    assert_eq!(alarm[0]["kind"], "fire_alarm");
}

/// The annotated plan's notes, tags and dimensions float in a 3D view (DOC-ANN-025), each class
/// shown or hidden on its own, the view keeping its choice.
#[test]
fn the_annotated_plan_floats_its_annotation_in_3d() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let view = ok(&mut app, "view.create_3d", json!({}));
    let info = ok(&mut app, "view.annotations_3d", json!({ "view_id": view }));
    let count = |info: &Value, c: &str| info["classes"].as_array().unwrap().iter().find(|x| x["class"] == c).map(|x| (x["count"].as_u64().unwrap(), x["visible"].as_bool().unwrap()));
    assert_eq!((info["shown"].as_bool(), info["occlude"].as_bool()), (Some(true), Some(true)), "labels behind the model left out unless set");
    // Counted over the plans, the section and the elevation (each with a note, two dimensions and
    // a tag).
    assert_eq!(count(&info, "text"), Some((4, true)));
    assert_eq!(count(&info, "dimensions"), Some((17, false)), "dimensions hidden unless chosen");
    assert_eq!(count(&info, "tags:Doors"), Some((4, true)));
    assert_eq!(count(&info, "tags:Windows"), Some((6, true)));
    assert_eq!(count(&info, "tags:Rooms"), Some((3, true)));
    let shown = |app: &App| {
        let s = store(app);
        reviera_core::annotations_3d::shown(&s, s.get(ElementId(view.as_u64().unwrap())).unwrap()).len()
    };
    assert_eq!(shown(&app), 13, "notes and tags");
    // Door tags off, dimensions on; then everything off; undo brings the classes back.
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "classes": { "tags:Doors": false, "dimensions": true } }));
    assert_eq!(shown(&app), 10 + 13);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "shown": false }));
    assert_eq!(shown(&app), 0);
    ok(&mut app, "edit.undo", json!({}));
    assert_eq!(shown(&app), 23);
    // Room tags stand over their rooms, a little above the floor.
    let s = store(&app);
    let tips = reviera_core::annotations_3d::tips(&s);
    let living = tips.iter().find(|t| t.lines.first().map(String::as_str) == Some("Living")).unwrap();
    assert!((living.anchor[2] - 0.3).abs() < 1e-9, "{living:?}");
    // Each names the view it is drawn in, which double-clicking its label opens: a plan, or the
    // section or elevation.
    for t in &tips {
        let owner = reviera_core::GlobalId::parse(&t.view).and_then(|g| s.find_by_guid(&g)).unwrap_or_else(|| panic!("{t:?}"));
        let kind = reviera_core::views::view_data(s.get(owner).unwrap()).unwrap().kind;
        let expected = match t.source.as_str() {
            "plans" => reviera_core::plans::is_plan(&kind),
            "sections" => kind == reviera_core::sections::KIND_SECTION,
            _ => kind == reviera_core::sections::KIND_ELEVATION,
        };
        assert!(expected, "{t:?} in {kind}");
    }
    assert!(app.execute("view.set_3d_annotations", json!({ "view_id": view, "classes": { "nonsense": true } }), &Caller::user()).is_err());
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "occlude": false }));
    assert_eq!(ok(&mut app, "view.annotations_3d", json!({ "view_id": view }))["occlude"], false);
}

#[test]
fn the_annotated_plan_floats_its_section_and_elevation_annotation() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let (sec, elev) = (view_id(&mut app, "Section A", "view.section"), view_id(&mut app, "South", "view.elevation"));
    // The section and elevation hold their own dimensions, measured in their frames: the wall's
    // and partition's 3 m height above Level 1, the overall width between the end walls' outer
    // faces and the living room's clear width.
    let values = |app: &mut App, view: u64| -> Vec<f64> {
        let list = ok(app, "anno.list", json!({ "view_id": view }));
        let mut v: Vec<f64> = list.as_array().unwrap().iter().flat_map(|d| d["measured"]["values"].as_array().unwrap().iter().map(f)).collect();
        v.sort_by(f64::total_cmp);
        v
    };
    let ev = values(&mut app, elev);
    assert_eq!(ev.len(), 2, "{ev:?}");
    assert!(near(ev[0], 3.0, 1e-6) && ev[1] > 10.0 && ev[1] < 10.6, "{ev:?}");
    let sv = values(&mut app, sec);
    assert_eq!(sv.len(), 2, "{sv:?}");
    assert!(near(sv[0], 3.0, 1e-6) && sv[1] > 5.4 && sv[1] < 6.0, "{sv:?}");
    // Their tags and notes are drawn with them, for output too.
    let doc = app.active().unwrap();
    let drawing = app.section_drawing(doc, ElementId(sec)).unwrap();
    assert!(drawing.labels.iter().any(|l| l.text == "BEDROOM"), "the section's note");
    let drawing = app.section_drawing(doc, ElementId(elev)).unwrap();
    assert!(drawing.labels.iter().any(|l| l.text == "RENDERED BLOCKWORK"), "the elevation's note");
    // Tag All in the section tags what it shows: the kitchen door, 3 m beyond the cut (the
    // bedroom door has its tag; the front door is behind the section).
    let tagged = ok(&mut app, "tag.tag_all", json!({ "view_id": sec, "categories": ["cat.doors"] }));
    assert_eq!(tagged.as_array().unwrap().len(), 1, "{tagged}");
    ok(&mut app, "edit.undo", json!({}));

    // In 3D, the sections' and elevations' annotation shows when chosen.
    let view = ok(&mut app, "view.create_3d", json!({}));
    let info = ok(&mut app, "view.annotations_3d", json!({ "view_id": view }));
    let from: Vec<(String, u64, bool)> = info["sources"].as_array().unwrap().iter().map(|s| (s["source"].as_str().unwrap().to_owned(), s["count"].as_u64().unwrap(), s["visible"].as_bool().unwrap())).collect();
    assert_eq!(from, vec![("plans".to_owned(), 26, true), ("sections".to_owned(), 4, false), ("elevations".to_owned(), 4, false)]);
    let shown = |app: &App| {
        let s = store(app);
        reviera_core::annotations_3d::shown(&s, s.get(ElementId(view.as_u64().unwrap())).unwrap())
    };
    assert_eq!(shown(&app).len(), 13, "the plans' notes and tags, as before");
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "sources": { "sections": true, "elevations": true } }));
    // Their two notes; their tags' hosts are tagged in the plan already, so labelled once.
    assert_eq!(shown(&app).len(), 15);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "classes": { "dimensions": true } }));
    assert_eq!(shown(&app).len(), 15 + 13 + 4);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "sources": { "plans": false } }));
    let tips = shown(&app);
    // Without the plans, the section's and elevation's tags show, at their hosts.
    assert_eq!(tips.len(), 8, "{tips:?}");
    assert!(tips.iter().all(|t| t.source != "plans"));
    assert_eq!(tips.iter().filter(|t| t.class.starts_with("tags:")).count(), 2);
    assert!(app.execute("view.set_3d_annotations", json!({ "view_id": view, "sources": { "sheets": true } }), &Caller::user()).is_err());

    // The section's note stands on its plane (y = 1.5 m); the elevation's is moved back along its
    // view onto the south wall's outer face, by the ray the window casts.
    let s = store(&app);
    let note = |text: &str| tips.iter().find(|t| t.lines == [text]).unwrap_or_else(|| panic!("{text}: {tips:?}")).clone();
    let bedroom = note("BEDROOM");
    assert!(near(bedroom.anchor[1], 1.5, 1e-6) && bedroom.probe.is_none(), "{bedroom:?}");
    assert_eq!(bedroom.view, s.get(ElementId(sec)).unwrap().guid().to_string(), "double-clicking it opens the section");
    let render = note("RENDERED BLOCKWORK");
    assert!(near(render.anchor[0], 5.0, 1e-6) && near(render.anchor[1], -5.0, 1e-6) && near(render.anchor[2], 2.8, 1e-6), "{render:?}");
    let probe = render.probe.expect("an elevation's label finds the model");
    let scene = app.render_scene(doc).unwrap();
    let bvh = reviera_render::bvh::Bvh::build(&scene);
    let v3 = |p: [f64; 3]| glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let (mesh, t) = bvh.nearest(v3(render.anchor), v3(probe), 1.0, &|_, _| true).expect("the facade");
    let on = v3(render.anchor) + v3(probe) * t;
    let wall = scene.meshes[mesh].element.and_then(|e| s.get(e)).expect("a model element");
    assert!(reviera_core::walls::wall_recipe(wall).is_some(), "{}", wall.class);
    assert!(on.y > -0.3 && on.y < 0.0, "on the south wall's outer face: {on}");
}
