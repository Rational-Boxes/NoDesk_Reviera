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
    // The plan, the section, the elevation and the types legend.
    assert_eq!(kinds.len(), 4, "{kinds:?}");
    assert!(kinds.iter().any(|k| k == "view.legend"));
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
    assert_eq!((sheets[0].viewports.len(), sheets[0].schedules.len()), (4, 2), "plan, section, elevation, legend and both schedules");
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
        // Level 1, Level 2, the scope-boxed East Half and the cropped plan.
        [4, 2, 2, 4, 11],
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
    // Hide and isolate applied to their views are saved with them: the roof hidden, and in
    // Isolate Partition everything but the partition.
    let roof_view = all.iter().find(|(n, _, _)| n == "Hide Roof").unwrap().2;
    let partition = all.iter().find(|(n, _, _)| n == "Isolate Partition").unwrap().2;
    let s = store(&app);
    let roofs: Vec<ElementId> = s.iter().filter(|e| e.class == "IfcRoof" || e.class == "IfcSlab" && reviera_core::roofs::roof_recipe(e).is_some()).map(|e| e.id()).collect();
    let hidden = reviera_core::view_hiding::hidden_in(&s, Some(ElementId(roof_view)));
    assert!(!roofs.is_empty() && roofs.iter().all(|r| hidden.contains(r)), "{roofs:?} {hidden:?}");
    let hidden = reviera_core::view_hiding::hidden_in(&s, Some(ElementId(partition)));
    let walls = s.iter().filter(|e| reviera_core::walls::wall_recipe(e).is_some()).count();
    assert!(hidden.len() >= walls - 1, "all but the partition: {} of {walls} walls and more", hidden.len());
    // Ghosting and translucency are kept as transparency overrides.
    for name in ["Isolate South Wall", "Translucent Walls"] {
        let v = all.iter().find(|(n, _, _)| n == name).unwrap().2;
        assert!(!reviera_core::view_overrides::resolved_in(&s, Some(ElementId(v))).is_empty(), "{name}");
    }
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
    assert_eq!(shown(&app), 15, "notes, tags and the plan's two spots");
    // Door tags off, dimensions on; then everything off; undo brings the classes back.
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "classes": { "tags:Doors": false, "dimensions": true } }));
    assert_eq!(shown(&app), 12 + 13);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "shown": false }));
    assert_eq!(shown(&app), 0);
    ok(&mut app, "edit.undo", json!({}));
    assert_eq!(shown(&app), 25);
    // Labels attach to their elements: a room's 1.2 m above its floor, a door's at its centre
    // (half its height up, on its wall), a window's at the middle of its opening.
    let s = store(&app);
    let tips = reviera_core::annotations_3d::tips(&s);
    let living = tips.iter().find(|t| t.lines.first().map(String::as_str) == Some("Living")).unwrap();
    assert!((living.anchor[2] - 1.2).abs() < 1e-9, "{living:?}");
    for (class, kind) in [("tags:Doors", reviera_core::inserts::Kind::Door), ("tags:Windows", reviera_core::inserts::Kind::Window)] {
        for t in tips.iter().filter(|t| t.class == class) {
            let host = reviera_core::GlobalId::parse(t.host.as_deref().unwrap()).and_then(|g| s.find_by_guid(&g)).unwrap();
            let p = reviera_core::inserts::placed(&s).into_iter().find(|p| p.id == host).unwrap();
            assert_eq!(p.kind, kind);
            let z = p.cut().z;
            assert!((t.anchor[2] - (z[0] + z[1]) / 2.0).abs() < 1e-9, "{class} {t:?} opening {z:?}");
            let c = p.world([0.0, 0.0, 0.0]);
            assert!((t.anchor[0] - c[0]).hypot(t.anchor[1] - c[1]) < 1e-9, "{class} at its insert in plan: {t:?}");
        }
    }
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
    assert_eq!(from, vec![("plans".to_owned(), 28, true), ("sections".to_owned(), 4, false), ("elevations".to_owned(), 5, false)]);
    let shown = |app: &App| {
        let s = store(app);
        reviera_core::annotations_3d::shown(&s, s.get(ElementId(view.as_u64().unwrap())).unwrap())
    };
    assert_eq!(shown(&app).len(), 15, "the plans' notes, tags and spots, as before");
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "sources": { "sections": true, "elevations": true } }));
    // Their two notes and the elevation's spot; their tags' hosts are tagged in the plan
    // already, so labelled once.
    assert_eq!(shown(&app).len(), 18);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "classes": { "dimensions": true } }));
    assert_eq!(shown(&app).len(), 18 + 13 + 4);
    ok(&mut app, "view.set_3d_annotations", json!({ "view_id": view, "sources": { "plans": false } }));
    let tips = shown(&app);
    // Without the plans, the section's and elevation's tags show, at their hosts.
    assert_eq!(tips.len(), 9, "{tips:?}");
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

#[test]
fn the_annotated_plan_reads_its_spots_from_the_model() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let (plan, elev) = (view_id(&mut app, "Level 1", "view.plan"), view_id(&mut app, "South", "view.elevation"));
    let spots = |app: &mut App, view: u64| ok(app, "spot.list", json!({ "view_id": view })).as_array().unwrap().clone();
    let lines = |s: &Value| s["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_owned()).collect::<Vec<_>>();
    // In the plan: the north wall's top at 3 m, and the north-east corner's coordinates, from the
    // project base point (millimetres).
    let p = spots(&mut app, plan);
    assert_eq!(p.len(), 2, "{p:?}");
    let elevation = p.iter().find(|s| s["data"]["spotKind"] == "elevation").unwrap();
    assert_eq!(lines(elevation), ["+3000"]);
    let coordinate = p.iter().find(|s| s["data"]["spotKind"] == "coordinate").unwrap();
    assert_eq!(lines(coordinate), ["N 6000", "E 10000"]);
    // In the elevation: the wall's top and bottom where its face was found along the view.
    let e = spots(&mut app, elev);
    assert_eq!(lines(&e[0]), ["T +3000", "B ±0"], "{e:?}");
    assert!(f(&e[0]["point"][1]) > -0.3 && f(&e[0]["point"][1]) < 0.0, "on the wall's outer face: {}", e[0]["point"]);
    // Drawn with the plan and printed.
    let doc = app.active().unwrap();
    let drawing = app.plan_drawing(doc, ElementId(plan)).unwrap();
    assert!(drawing.labels.iter().any(|l| l.text == "+3000") && drawing.labels.iter().any(|l| l.text == "E 10000"));
    // They follow the model: a taller north wall reads higher; relative to the survey point, the
    // shared height.
    let s = store(&app);
    let north = s.iter().find(|w| reviera_core::walls::wall_recipe(w).is_some_and(|r| near(r.curve.start[1], 6.0, 1e-9) && near(r.curve.end[1], 6.0, 1e-9))).unwrap().id();
    ok(&mut app, "wall.set_constraints", json!({ "wall_id": north, "top_constraint": { "unconnected": 3.5 } }));
    let p = spots(&mut app, plan);
    let elevation = p.iter().find(|s| s["data"]["spotKind"] == "elevation").unwrap().clone();
    assert_eq!(lines(&elevation), ["+3500"]);
    ok(&mut app, "coordinates.set_survey_point", json!({ "point": [0.0, 0.0, 0.0], "shared_coords": [500000.0, 200000.0, 45.0] }));
    ok(&mut app, "spot.set", json!({ "annotation_ids": [elevation["id"]], "base": "survey" }));
    let p = spots(&mut app, plan);
    let elevation = p.iter().find(|s| s["data"]["spotKind"] == "elevation").unwrap().clone();
    assert_eq!(lines(&elevation), ["+48500"]);
    // A spot whose element is no longer under it keeps its place and is orphaned.
    ok(&mut app, "element.move", json!({ "element_ids": [north], "vector": [0.0, 2.0, 0.0] }));
    let p = spots(&mut app, plan);
    let elevation = p.iter().find(|s| s["data"]["spotKind"] == "elevation").unwrap();
    assert_eq!(elevation["orphaned"], true, "{elevation}");
    // Only in plans, sections and elevations, and on model elements.
    let view = ok(&mut app, "view.create_3d", json!({}));
    assert!(app.execute("spot.create_elevation", json!({ "view_id": view, "point": [0, 0, 0] }), &Caller::user()).is_err());
    assert!(app.execute("spot.create_elevation", json!({ "view_id": plan, "ref": plan, "point": [0, 0, 0] }), &Caller::user()).is_err());
    // In 3D, at the point they read.
    let s = store(&app);
    let tips = reviera_core::annotations_3d::tips(&s);
    assert_eq!(tips.iter().filter(|t| t.class == "spots").count(), 3);
}

#[test]
fn the_house_reads_its_roof_slope() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "house");
    let plan = view_id(&mut app, "Level 2", "view.plan");
    let spots = |app: &mut App, view: u64| ok(app, "spot.list", json!({ "view_id": view })).as_array().unwrap().clone();
    // The east hip of the 30° roof, from the plan: 57.7 %, downhill to the east.
    let s = spots(&mut app, plan);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0]["lines"], json!(["57.7 %"]));
    assert!(near(f(&s[0]["downhill"][0]), 1.0, 1e-6) && near(f(&s[0]["downhill"][1]), 0.0, 1e-6), "{}", s[0]["downhill"]);
    ok(&mut app, "spot.set", json!({ "annotation_ids": [s[0]["id"]], "slope_format": "degrees" }));
    assert_eq!(spots(&mut app, plan)[0]["lines"], json!(["30°"]));
    ok(&mut app, "spot.set", json!({ "annotation_ids": [s[0]["id"]], "slope_format": "ratio" }));
    assert_eq!(spots(&mut app, plan)[0]["lines"], json!(["1 : 1.7"]));
    // The triangle is for sections and elevations.
    assert!(app.execute("spot.set", json!({ "annotation_ids": [s[0]["id"]], "representation": "triangle" }), &Caller::user()).is_err());
    // In a section across the house, looking south, the roof's cut profile reads the same
    // slope as a triangle; downhill (east) is to the viewer's left, and down.
    let top = s[0]["point"].as_array().unwrap().iter().map(f).collect::<Vec<_>>();
    let section = ok(&mut app, "view.create_section", json!({ "start": [-2.0, 3.5], "end": [12.0, 3.5] }));
    let roof = s[0]["data"]["element"].as_str().unwrap().to_owned();
    ok(&mut app, "spot.create_slope", json!({ "view_id": section, "ref": roof, "point": [top[0], 3.5, top[2]], "representation": "triangle" }));
    let cut = spots(&mut app, section.as_u64().unwrap());
    assert_eq!(cut[0]["lines"], json!(["57.7 %"]), "{cut:?}");
    assert!(f(&cut[0]["downhill"][0]) < 0.0 && f(&cut[0]["downhill"][1]) < 0.0, "{}", cut[0]["downhill"]);
    // A wall's cut face is vertical: refused.
    let st = store(&app);
    let west = st.iter().find(|w| reviera_core::walls::wall_recipe(w).is_some_and(|r| near(r.curve.start[0], 0.0, 1e-9) && near(r.curve.end[0], 0.0, 1e-9))).unwrap().id();
    let err = app.execute("spot.create_slope", json!({ "view_id": section, "ref": west, "point": [0.0, 3.5, 1.5] }), &Caller::user()).unwrap_err();
    assert!(format!("{err:?}").contains("spot.vertical"), "{err:?}");
    // It follows the roof: a 45° roof reads 100 %.
    ok(&mut app, "roof.set_slope", json!({ "roof_id": roof, "slope": "45°" }));
    assert_eq!(spots(&mut app, plan)[0]["lines"], json!(["1 : 1"]));
}

#[test]
fn the_managed_project_keynotes_its_door_window_and_partition() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "managed_project");
    let plan = view_id(&mut app, "Level 1", "view.plan");
    let list = |app: &mut App| ok(app, "keynote.list", json!({ "view_id": plan })).as_array().unwrap().clone();
    let by_key = |l: &[Value], key: &str| l.iter().find(|k| k["key"] == key).cloned().unwrap_or_else(|| panic!("{key}: {l:?}"));
    let k = list(&mut app);
    assert_eq!(k.len(), 3, "{k:?}");
    // The door's and window's from their types; the partition's picked from the table.
    let door = by_key(&k, "08 11 13");
    assert_eq!((door["lines"].clone(), door["text"].clone(), door["missing"].clone()), (json!(["08 11 13"]), json!("Hollow metal doors and frames"), json!(false)));
    assert_eq!(door["frame"], "box");
    assert_eq!(by_key(&k, "08 51 13")["lines"], json!(["08 51 13"]));
    let user = by_key(&k, "09 21 16");
    assert_eq!((user["data"]["mode"].clone(), user["frame"].clone()), (json!("user"), json!("none")));
    // Drawn with the plan, a leader to each element.
    let doc = app.active().unwrap();
    let drawing = app.plan_drawing(doc, ElementId(plan)).unwrap();
    assert!(drawing.labels.iter().any(|l| l.text == "08 11 13") && drawing.labels.iter().any(|l| l.text == "09 21 16"));
    // Live: the door type's keynote changes, and the text style reads the table, which edits
    // and reloads.
    let host = door["host"].as_u64().unwrap();
    ok(&mut app, "param.set", json!({ "element_ids": [host], "param_ref": "type.keynote", "value": "09 21 16" }));
    ok(&mut app, "keynote.set", json!({ "annotation_ids": [door["id"]], "style": "text" }));
    let k = list(&mut app);
    let door = k.iter().find(|x| x["id"] == door["id"]).unwrap();
    assert_eq!(door["lines"], json!(["Gypsum board assemblies"]));
    // A key the table does not have reads "?" and is listed as missing.
    ok(&mut app, "keynote.set", json!({ "annotation_ids": [user["id"]], "key": "99 99 99" }));
    let user = list(&mut app).into_iter().find(|x| x["id"] == user["id"]).unwrap();
    assert_eq!((user["lines"].clone(), user["missing"].clone()), (json!(["?"]), json!(true)));
    // An element keynote needs its element; a user keynote its key.
    assert!(app.execute("keynote.create", json!({ "view_id": plan, "position": [0, 0] }), &Caller::user()).is_err());
    assert!(app.execute("keynote.create", json!({ "view_id": plan, "mode": "user", "host_ref": host, "position": [0, 0] }), &Caller::user()).is_err());
    // In 3D, key and text at their elements.
    let s = store(&app);
    let tips: Vec<_> = reviera_core::annotations_3d::tips(&s).into_iter().filter(|t| t.class == "keynotes").collect();
    assert_eq!(tips.len(), 3);
    assert!(tips.iter().any(|t| t.lines == ["08 51 13 Aluminium windows"]), "{tips:?}");
}

#[test]
fn the_managed_project_lists_its_keynotes_per_sheet() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "managed_project");
    let s = store(&app);
    let legend = s.iter().find(|e| e.name.as_deref() == Some("Keynote Legend")).expect("the legend").id();
    let sheet = s.iter().find(|e| e.name.as_deref() == Some("Plan and Keynotes")).expect("the sheet").id();
    let rows = |app: &mut App, sheet: Option<ElementId>| {
        let mut args = json!({ "schedule_id": legend });
        if let Some(s) = sheet {
            args["sheet_id"] = json!(s);
        }
        let t = ok(app, "schedule.get_table", args);
        t["rows"].as_array().unwrap().iter().map(|r| r["cells"].as_array().unwrap().iter().map(|c| c["text"].as_str().unwrap().to_owned()).collect::<Vec<_>>()).collect::<Vec<_>>()
    };
    // Every key the plan's keynotes show, in order, with its text.
    let all = rows(&mut app, None);
    assert_eq!(all, vec![
        vec!["08 11 13".to_owned(), "Hollow metal doors and frames".to_owned()],
        vec!["08 51 13".to_owned(), "Aluminium windows".to_owned()],
        vec!["09 21 16".to_owned(), "Gypsum board assemblies".to_owned()],
    ]);
    assert_eq!(rows(&mut app, Some(sheet)), all, "the sheet has the plan");
    // A second sheet with a section and its own keynote lists only that one.
    let plan = view_id(&mut app, "Level 1", "view.plan");
    let section = ok(&mut app, "view.create_section", json!({ "start": [-1.0, 4.0], "end": [13.0, 4.0] }));
    let st = store(&app);
    let partition = st.iter().find(|w| reviera_core::walls::wall_recipe(w).is_some_and(|r| near(r.curve.start[1], 0.0, 1e-9) && near(r.curve.end[1], 8.0, 1e-9))).unwrap().id();
    ok(&mut app, "keynote.create", json!({ "view_id": section, "mode": "user", "key": "09", "host_ref": partition, "position": [9.0, 2.0] }));
    let other = ok(&mut app, "sheet.create", json!({ "name": "Section", "size": "A3" }));
    ok(&mut app, "sheet.place_view", json!({ "sheet_id": other, "view_id": section, "position": [200, 150] }));
    ok(&mut app, "sheet.place_schedule", json!({ "sheet_id": other, "schedule_id": legend, "position": [320, 250] }));
    assert_eq!(rows(&mut app, Some(ElementId(other.as_u64().unwrap()))), vec![vec!["09".to_owned(), "Finishes".to_owned()]]);
    assert_eq!(rows(&mut app, Some(sheet)).len(), 3, "the first sheet's are unchanged");
    assert_eq!(rows(&mut app, None).len(), 4, "on its own it lists them all");
    // Not filtering by sheet, every copy lists them all; its columns are its own.
    ok(&mut app, "view.set_keynote_legend", json!({ "schedule_id": legend, "filter_by_sheet": false }));
    assert_eq!(rows(&mut app, Some(ElementId(other.as_u64().unwrap()))).len(), 4);
    let err = app.execute("schedule.set_fields", json!({ "schedule_id": legend, "fields": ["type"] }), &Caller::user()).unwrap_err();
    assert!(format!("{err:?}").contains("schedule.keynote_legend"), "{err:?}");
    let _ = plan;
}

#[test]
fn the_annotated_plan_legend_draws_its_types() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let legend = view_id(&mut app, "Types Legend", "view.legend");
    let items = |app: &mut App| {
        let l = ok(app, "legend.list", json!({ "view_id": legend }));
        assert_eq!(l["failed"], json!([]), "{l}");
        l["items"].as_array().unwrap().iter().map(|i| {
            let (lo, hi) = (&i["lo"], &i["hi"]);
            (i["id"].as_u64().unwrap(), f(&hi[0]) - f(&lo[0]), f(&hi[1]) - f(&lo[1]))
        }).collect::<Vec<_>>()
    };
    let i = items(&mut app);
    assert_eq!(i.len(), 5, "{i:?}");
    let s = store(&app);
    let wall_type = s.iter().find(|e| e.class == "IfcWallType").unwrap().id();
    let width = reviera_core::walls::regenerate(&s, s.iter().find(|w| reviera_core::walls::wall_recipe(w).is_some()).unwrap()).unwrap().width;
    // The wall in plan: 1.5 m long, as wide as the type; in section: as wide, 3 m high.
    let (_, pw, ph) = i[0];
    assert!(near(pw, 1.5, 0.02) && near(ph, width, 0.02), "plan {pw} × {ph}, the wall {width} wide");
    let (_, sw, sh) = i[1];
    assert!(near(sw, width, 0.02) && near(sh, 3.0, 0.02), "section {sw} × {sh}");
    // The door in plan shows its swing (deeper than the wall); from the front its wall, 3 m high.
    let (_, _, dh) = i[2];
    assert!(dh > width + 0.5, "the swing: {dh}");
    let (_, fw, fh) = i[3];
    assert!(near(fh, 3.0, 0.02) && fw > 1.5, "door front {fw} × {fh}");
    // Drawn on the sheet and in PDF output, and it can go on a second sheet too.
    let doc = app.active().unwrap();
    let drawing = reviera_app::legends::legend_drawing(&s, s.get(ElementId(legend)).unwrap()).unwrap();
    assert!(drawing.drawing.labels.iter().any(|l| l.text == "WALL, DOOR AND WINDOW TYPES"));
    assert!(drawing.drawing.graphics.lines.iter().map(|l| l.segments.len()).sum::<usize>() > 20);
    let other = ok(&mut app, "sheet.create", json!({ "name": "Legends", "size": "A3" }));
    ok(&mut app, "sheet.place_view", json!({ "sheet_id": other, "view_id": legend }));
    assert!(app.sheet_drawing(doc, ElementId(other.as_u64().unwrap())).is_ok());
    // A thicker wall type draws thicker: the type, not a model element.
    ok(&mut app, "compound.set_layers", json!({ "type_id": wall_type, "layers": [
        { "function": "finish1", "thickness": "20 mm" }, { "function": "structure", "thickness": "200 mm" }, { "function": "finish1", "thickness": "20 mm" }] }));
    let (_, pw2, ph2) = items(&mut app)[0];
    assert!(near(pw2, 1.5, 0.02) && near(ph2, 0.24, 0.01), "after layering: {pw2} × {ph2}");
    // Legend components only in legends, of types a legend can draw.
    let plan = view_id(&mut app, "Level 1", "view.plan");
    assert!(app.execute("legend.place_component", json!({ "view_id": plan, "type_id": wall_type, "point": [0, 0] }), &Caller::user()).is_err());
    let level = s.iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    assert!(app.execute("legend.place_component", json!({ "view_id": legend, "type_id": level, "point": [0, 0] }), &Caller::user()).is_err());
}

#[test]
fn cropped_views_draw_only_their_crop_region() {
    let work = tempfile::tempdir().unwrap();
    // The views sample's cropped plan: nothing drawn east of x = 5 m.
    let mut app = open(work.path(), "views_inspection");
    let doc = app.active().unwrap();
    let cropped = view_id(&mut app, "Level 1 - Cropped", "view.plan");
    let d = app.plan_drawing(doc, ElementId(cropped)).unwrap();
    let xs: Vec<f32> = d.graphics.lines.iter().flat_map(|l| l.segments.iter().flat_map(|s| [s[0][0], s[1][0]])).collect();
    assert!(!xs.is_empty() && xs.iter().all(|x| *x >= -1.0 - 1e-4 && *x <= 5.0 + 1e-4), "{:?}", xs.iter().cloned().fold(f32::MIN, f32::max));
    let level1 = view_id(&mut app, "Level 1", "view.plan");
    let whole = app.plan_drawing(doc, ElementId(level1)).unwrap();
    let wx = whole.graphics.lines.iter().flat_map(|l| l.segments.iter().flat_map(|s| [s[0][0], s[1][0]])).fold(f32::MIN, f32::max);
    assert!(wx > 9.0, "the uncropped plan runs east: {wx}");
    // The crop region is drawn (visible), its fills clipped too.
    assert!(d.graphics.poche.iter().flat_map(|m| &m.positions).all(|p| p[0] <= 5.0 + 1e-4));

    // The annotated plan cropped to its living room (0..6 × 0..6): the kitchen's and bedroom's
    // tags go with their rooms; the annotation crop (5 mm at 1 : 100) hides the title note below
    // the plan and the dimension values outside it, and cuts the dimension lines.
    let mut app = open(work.path(), "annotated_plan");
    let doc = app.active().unwrap();
    let plan = view_id(&mut app, "Level 1", "view.plan");
    ok(&mut app, "view.set_crop", json!({ "view_id": plan, "shape": { "min": [0.0, 0.0], "max": [6.0, 6.0] }, "visible": false }));
    let d = app.plan_drawing(doc, ElementId(plan)).unwrap();
    let texts: Vec<&str> = d.labels.iter().map(|l| l.text.as_str()).collect();
    assert!(texts.contains(&"LIVING") || texts.contains(&"Living"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.eq_ignore_ascii_case("kitchen") || t.eq_ignore_ascii_case("bedroom")), "{texts:?}");
    ok(&mut app, "view.set_annotation_crop", json!({ "view_id": plan, "enabled": true, "offsets": [5, 5, 5, 5] }));
    let d = app.plan_drawing(doc, ElementId(plan)).unwrap();
    let texts: Vec<&str> = d.labels.iter().map(|l| l.text.as_str()).collect();
    assert!(!texts.iter().any(|t| t.contains("GROUND FLOOR PLAN")), "the title note is outside: {texts:?}");
    let ys: Vec<f32> = d.annotation.iter().flat_map(|l| l.segments.iter().flat_map(|s| [s[0][1], s[1][1]])).collect();
    assert!(ys.iter().all(|y| *y >= -0.5 - 1e-4 && *y <= 6.5 + 1e-4), "annotation inside the annotation crop");
    // Off again: everything back.
    ok(&mut app, "view.set_crop", json!({ "view_id": plan, "enabled": false }));
    let d = app.plan_drawing(doc, ElementId(plan)).unwrap();
    assert!(d.labels.iter().any(|l| l.text.contains("GROUND FLOOR PLAN")));
    // Reset fits the model; 3D views cannot be cropped.
    ok(&mut app, "view.reset_crop", json!({ "view_id": plan }));
    let s = store(&app);
    let c = reviera_core::crop::crop_of(s.get(ElementId(plan)).unwrap()).unwrap();
    assert!(c.min[0] < -0.5 && c.max[0] > 10.5 && c.max[1] > 6.5, "{c:?}");
    // Size Crop: about the centre, one step, with the annotation crop's offsets.
    let centre = [(c.min[0] + c.max[0]) / 2.0, (c.min[1] + c.max[1]) / 2.0];
    ok(&mut app, "view.size_crop", json!({ "view_id": plan, "width": 4.0, "height": 3.0, "annotation_offsets": [2, 3, 4, 5] }));
    let s = store(&app);
    let e = s.get(ElementId(plan)).unwrap();
    let sized = reviera_core::crop::crop_of(e).unwrap();
    for (i, c) in centre.iter().enumerate() {
        assert!(((sized.min[i] + sized.max[i]) / 2.0 - c).abs() < 1e-9, "{sized:?}");
    }
    assert!((sized.max[0] - sized.min[0] - 4.0).abs() < 1e-9 && (sized.max[1] - sized.min[1] - 3.0).abs() < 1e-9);
    assert_eq!(reviera_core::crop::annotation_crop_of(e).offsets, [2.0, 3.0, 4.0, 5.0]);
    assert!(app.execute("view.size_crop", json!({ "view_id": plan, "width": 0.0 }), &Caller::user()).is_err());
    ok(&mut app, "edit.undo", json!({}));
    assert_eq!(reviera_core::crop::crop_of(store(&app).get(ElementId(plan)).unwrap()).unwrap(), c, "one undo step");
    let v3 = ok(&mut app, "view.create_3d", json!({}));
    assert!(app.execute("view.set_crop", json!({ "view_id": v3, "enabled": true }), &Caller::user()).is_err());
}

#[test]
fn a_plan_assigned_to_a_scope_box_is_cropped_to_it() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "views_inspection");
    let doc = app.active().unwrap();
    // The East Half box runs from x = 4.6 to 11 m (y −1 to 8): its plan draws only there.
    let east = view_id(&mut app, "Level 1 - East Half", "view.plan");
    let xs = |app: &App| {
        let d = app.plan_drawing(doc, ElementId(east)).unwrap();
        d.graphics.lines.iter().flat_map(|l| l.segments.iter().flat_map(|s| [s[0][0], s[1][0]])).fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)))
    };
    let (lo, hi) = xs(&app);
    assert!(lo >= 4.6 - 1e-3 && hi <= 11.0 + 1e-3, "{lo} .. {hi}");
    // Its region cannot be set by hand; switching the crop off shows the whole plan.
    let err = app.execute("view.set_crop", json!({ "view_id": east, "shape": { "min": [0, 0], "max": [3, 3] } }), &Caller::user()).unwrap_err();
    assert!(format!("{err:?}").contains("view.crop_follows_scope_box"), "{err:?}");
    ok(&mut app, "view.set_crop", json!({ "view_id": east, "enabled": false }));
    let (lo, _) = xs(&app);
    assert!(lo < 1.0, "the whole plan: from {lo}");
    // On again, and the box moved west: the crop follows it.
    ok(&mut app, "view.set_crop", json!({ "view_id": east, "enabled": true }));
    let s = store(&app);
    let b = s.iter().find(|e| e.name.as_deref() == Some("East Half")).unwrap().id();
    ok(&mut app, "element.move", json!({ "element_ids": [b], "vector": [-2.0, 0.0, 0.0] }));
    let (lo, hi) = xs(&app);
    assert!(lo >= 2.6 - 1e-3 && hi <= 9.0 + 1e-3 && lo < 4.0, "{lo} .. {hi}");

    // An elevation assigned to the box is cropped to it seen from the side: the South elevation
    // (looking north, x to the east) shows only the box's width, from its bottom to its top.
    let south = view_id(&mut app, "South", "view.elevation");
    let whole = app.section_drawing(doc, ElementId(south)).unwrap();
    let span = |d: &reviera_app::output::PlanDrawing| d.graphics.lines.iter().flat_map(|l| l.segments.iter().flat_map(|s| [s[0][0], s[1][0]])).fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let (wlo, whi) = span(&whole);
    ok(&mut app, "view.assign_scope_box", json!({ "view_ids": [south], "scope_box_id": b }));
    let s = store(&app);
    let e = s.get(ElementId(south)).unwrap();
    assert!(reviera_core::crop::follows_scope_box(&s, e));
    let c = reviera_core::crop::effective(&s, e).unwrap();
    assert!((c.max[0] - c.min[0] - 6.4).abs() < 1e-6, "the box's width: {c:?}");
    let sb = reviera_core::scope_boxes::recipe(s.get(b).unwrap()).unwrap();
    assert!((c.min[1] - sb.bottom).abs() < 1e-6 && (c.max[1] - sb.bottom - sb.height).abs() < 1e-6, "{c:?} {sb:?}");
    let (lo, hi) = span(&app.section_drawing(doc, ElementId(south)).unwrap());
    assert!(lo >= c.min[0] as f32 - 1e-3 && hi <= c.max[0] as f32 + 1e-3, "{lo} .. {hi} in {c:?}");
    assert!(hi - lo < whi - wlo - 1.0, "narrower than the whole elevation ({wlo} .. {whi})");
}

#[test]
fn hide_in_view_is_saved_with_the_view_and_printed_so() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "views_inspection");
    let doc = app.active().unwrap();
    let plan = view_id(&mut app, "Level 1", "view.plan");
    let s = store(&app);
    let walls: Vec<ElementId> = s.iter().filter(|e| reviera_core::walls::wall_recipe(e).is_some()).map(|e| e.id()).collect();
    let doors: Vec<ElementId> = s.iter().filter(|e| reviera_core::inserts::kind_of(e) == Some(reviera_core::inserts::Kind::Door)).map(|e| e.id()).collect();
    assert!(walls.len() > 2 && !doors.is_empty());
    let segments = |app: &App| app.plan_drawing(doc, ElementId(plan)).unwrap().graphics.lines.iter().map(|l| l.segments.len()).sum::<usize>();
    let whole = segments(&app);

    // One wall hidden: the printed plan loses its lines; the view lists it.
    ok(&mut app, "view.hide", json!({ "view_id": plan, "element_ids": [walls[0]] }));
    let one = segments(&app);
    assert!(one < whole, "{one} < {whole}");
    let listed = ok(&mut app, "view.hidden", json!({ "view_id": plan }));
    assert_eq!(listed["count"], 1, "{listed}");
    assert_eq!(listed["elements"][0]["id"], json!(walls[0]));
    // Other views are not touched.
    let level2 = view_id(&mut app, "Level 2", "view.plan");
    assert!(reviera_core::view_hiding::hidden_in(&store(&app), Some(ElementId(level2))).is_empty());

    // The doors' category: every door goes.
    ok(&mut app, "view.hide", json!({ "view_id": plan, "element_ids": [doors[0]], "by_category": true }));
    let hidden = reviera_core::view_hiding::hidden_in(&store(&app), Some(ElementId(plan)));
    assert!(doors.iter().all(|d| hidden.contains(d)) && hidden.contains(&walls[0]));
    assert!(app.execute("view.hide", json!({ "view_id": plan, "categories": ["cat.nonsense"] }), &Caller::user()).is_err());

    // Undo is one step each; Unhide All shows everything.
    ok(&mut app, "edit.undo", json!({}));
    assert_eq!(reviera_core::view_hiding::hidden_in(&store(&app), Some(ElementId(plan))).len(), 1);
    ok(&mut app, "view.unhide", json!({ "view_id": plan, "all": true }));
    assert_eq!(segments(&app), whole);

    // Apply Hide/Isolate to View: a temporary hide becomes Hide in View and the temporary state
    // clears; an isolation hides everything else that has geometry.
    ok(&mut app, "view.temp_hide", json!({ "view_id": plan, "element_ids": [walls[1]] }));
    let applied = ok(&mut app, "view.temp_apply", json!({ "view_id": plan }));
    assert_eq!(applied["hidden"], 1, "{applied}");
    assert!(app.temp_state(doc, Some(ElementId(plan))).is_empty());
    assert!(reviera_core::view_hiding::hidden_in(&store(&app), Some(ElementId(plan))).contains(&walls[1]));
    ok(&mut app, "view.temp_isolate", json!({ "view_id": plan, "element_ids": [walls[2]] }));
    let applied = ok(&mut app, "view.temp_apply", json!({ "view_id": plan }));
    let hidden = reviera_core::view_hiding::hidden_in(&store(&app), Some(ElementId(plan)));
    assert!(!hidden.contains(&walls[2]) && walls.iter().filter(|w| **w != walls[2]).all(|w| hidden.contains(w)), "{applied}");
    assert!(app.execute("view.temp_apply", json!({ "view_id": plan }), &Caller::user()).is_err(), "nothing left to apply");

    // Saved with the view: it comes back on reopen.
    let path = work.path().join("hidden.ifc");
    ok(&mut app, "file.save_as", json!({ "path": path }));
    let mut again = App::new(AppConfig { live: None, untitled_dir: work.path().join("u-again") });
    ok(&mut again, "file.open", json!({ "path": path }));
    let reopened = store(&again);
    let plan = reopened.iter().find(|e| e.name.as_deref() == Some("Level 1") && reviera_core::views::view_data(e).is_some_and(|d| d.kind == "view.plan")).unwrap().id();
    let guids = |s: &reviera_core::Store, ids: &std::collections::HashSet<ElementId>| ids.iter().map(|i| s.get(*i).unwrap().guid().to_string()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(guids(&reopened, &reviera_core::view_hiding::hidden_in(&reopened, Some(plan))), guids(&store(&app), &hidden));
}

#[test]
fn graphic_overrides_are_saved_with_the_view_and_printed_so() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "views_inspection");
    let doc = app.active().unwrap();
    let plan = view_id(&mut app, "Level 1", "view.plan");
    let s = store(&app);
    let walls: Vec<ElementId> = s.iter().filter(|e| reviera_core::walls::wall_recipe(e).is_some()).map(|e| e.id()).collect();
    // The colours the printed plan draws lines in (a faded one for a halftone element).
    let drawing = |app: &App| {
        let g = app.plan_drawing(doc, ElementId(plan)).unwrap().graphics;
        g.lines.iter().filter(|l| !l.segments.is_empty()).map(|l| l.color.map(|c| (c * 1000.0).round() as i32)).collect::<std::collections::BTreeSet<_>>()
    };
    let before = drawing(&app);

    // Halftone on one wall: the printed plan draws its lines fainter; nothing else changes.
    ok(&mut app, "view.override_elements", json!({ "view_id": plan, "element_ids": [walls[0]], "override": { "halftone": true } }));
    let r = reviera_core::view_overrides::resolved_in(&store(&app), Some(ElementId(plan)));
    assert_eq!(r.len(), 1);
    assert!((r[&walls[0]].line_strength() - 0.5).abs() < 1e-6);
    let faded = drawing(&app);
    assert!(faded.difference(&before).next().is_some(), "the halftone wall prints fainter: {faded:?} vs {before:?}");
    // A category override reaches every wall; the wall's own transparency wins over it.
    ok(&mut app, "view.set_category_override", json!({ "view_id": plan, "category": "cat.walls", "override": { "transparency": 40 } }));
    ok(&mut app, "view.override_elements", json!({ "view_id": plan, "element_ids": [walls[0]], "override": { "transparency": 90 } }));
    let r = reviera_core::view_overrides::resolved_in(&store(&app), Some(ElementId(plan)));
    assert!(walls.iter().all(|w| r.contains_key(w)));
    assert!((r[&walls[0]].opacity - 0.1).abs() < 1e-6 && r[&walls[0]].halftone);
    assert!((r[&walls[1]].opacity - 0.6).abs() < 1e-6 && !r[&walls[1]].halftone);
    assert!(app.execute("view.override_elements", json!({ "view_id": plan, "element_ids": [walls[0]], "override": { "transparency": 140 } }), &Caller::user()).is_err());
    let listed = ok(&mut app, "view.overrides", json!({ "view_id": plan }));
    assert_eq!(listed["elements"].as_array().unwrap().len(), 1, "{listed}");
    ok(&mut app, "view.clear_category_override", json!({ "view_id": plan, "category": "cat.walls" }));
    ok(&mut app, "view.clear_element_overrides", json!({ "view_id": plan, "all": true }));
    assert_eq!(drawing(&app), before);

    // Apply Hide/Isolate to View keeps translucency and ghosting as transparency overrides.
    ok(&mut app, "view.temp_translucent", json!({ "view_id": plan, "element_ids": [walls[1]] }));
    let applied = ok(&mut app, "view.temp_apply", json!({ "view_id": plan }));
    assert_eq!(applied["overridden"], 1, "{applied}");
    let r = reviera_core::view_overrides::resolved_in(&store(&app), Some(ElementId(plan)));
    assert!((r[&walls[1]].opacity - reviera_app::inspect::TRANSLUCENT).abs() < 0.01, "{r:?}");
    ok(&mut app, "view.temp_isolate", json!({ "view_id": plan, "element_ids": [walls[2]], "ghost_others": true }));
    ok(&mut app, "view.temp_apply", json!({ "view_id": plan }));
    let r = reviera_core::view_overrides::resolved_in(&store(&app), Some(ElementId(plan)));
    assert!(!r.contains_key(&walls[2]) && r.contains_key(&walls[3]), "the others ghosted for good");
    assert!(app.temp_state(doc, Some(ElementId(plan))).is_empty());
}

#[test]
fn hiding_an_element_in_a_view_hides_its_tags_there() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "annotated_plan");
    let plan = view_id(&mut app, "Level 1", "view.plan");
    let s = store(&app);
    let view = s.get(ElementId(plan)).unwrap().clone();
    let tagged = |s: &reviera_core::Store| reviera_app::notes::view_notes(s, &view, 100.0, 0.0, None).into_iter().filter_map(|n| n.host).collect::<Vec<_>>();
    let before = tagged(&s);
    let door = *before.iter().find(|h| reviera_core::inserts::kind_of(s.get(**h).unwrap()) == Some(reviera_core::inserts::Kind::Door)).expect("a tagged door");
    ok(&mut app, "view.hide", json!({ "view_id": plan, "element_ids": [door] }));
    let after = tagged(&store(&app));
    assert!(!after.contains(&door), "the door's tag goes with it");
    assert_eq!(after.len(), before.iter().filter(|h| **h != door).count(), "the other tags stay");
    // Another view's tags of it are not touched.
    let other = store(&app).iter().filter(|e| reviera_core::views::is_view(e) && e.id() != ElementId(plan)).map(|e| e.id()).collect::<Vec<_>>();
    assert!(other.iter().all(|v| reviera_core::view_hiding::hidden_in(&store(&app), Some(*v)).is_empty()));
}

#[test]
fn the_greenhouses_sample_keeps_its_enclosures() {
    let work = tempfile::tempdir().unwrap();
    let mut app = open(work.path(), "greenhouses");
    let s = store(&app);
    let enclosures: Vec<_> = s.iter().filter_map(|e| reviera_core::enclosures::recipe(e).map(|r| (e.id(), r))).collect();
    assert_eq!(enclosures.len(), 3);
    let mut seen = Vec::new();
    for (id, r) in &enclosures {
        let info = ok(&mut app, "enclosure.get", json!({ "enclosure_id": id }));
        let n = |k: &str| info[k].as_array().map_or(0, Vec::len);
        seen.push((r.form, n("roofs"), n("curtain_walls"), n("walls"), n("gutters")));
        // Every part is still there and names its enclosure.
        assert_eq!(n("roofs") + n("curtain_walls") + n("walls") + n("gutters"), r.parts.len(), "{info}");
    }
    seen.sort_by_key(|s| s.1);
    use reviera_core::enclosures::Form;
    assert_eq!(seen, vec![(Form::Gable, 1, 4, 0, 2), (Form::Multispan, 3, 4, 0, 4), (Form::Venlo, 6, 4, 4, 7)]);
    // The Venlo's end walls rise into its gables.
    let venlo = enclosures.iter().find(|(_, r)| r.form == Form::Venlo).unwrap();
    let ends: Vec<_> = s.iter().filter(|e| reviera_core::curtain::curtain_recipe(e).is_some_and(|c| !c.attach_top.is_empty())).filter(|e| reviera_core::enclosures::enclosure_of(e) == Some(s.get(venlo.0).unwrap().guid().as_str())).collect();
    assert_eq!(ends.len(), 2);
    let g = reviera_core::curtain::regenerate(&s, ends[0]).unwrap();
    // Up under the ridges (5.5 m + 2 m × tan 22°), short of them by the roof's glazing bars.
    assert!(g.z[1] > venlo.1.ridge() - 0.1 && g.z[1] < venlo.1.ridge(), "{:?} vs ridge {}", g.z, venlo.1.ridge());
}
