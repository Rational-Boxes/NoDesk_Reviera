// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazed enclosures (ARC-GLZ-001/002): gable, multispan and Venlo greenhouses made of roofs,
//! curtain walls, knee walls and gutters, rebuilt on change, and saved to IFC.

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

fn count(v: &Value, key: &str) -> usize {
    v[key].as_array().map_or(0, Vec::len)
}

#[test]
fn a_venlo_range_is_roofs_walls_and_gutters_rebuilt_on_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("venlo.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // 6 trusses of 8 m (two 4 m roofs each), 80 m long in 4 m bays.
    let id = ElementId(
        run(&r, &mut doc, "enclosure.create", json!({ "form": "venlo", "level_id": level.0, "span": 8.0, "spans": 6, "length": 80.0, "bay": 4.0 }))
            .unwrap()
            .as_u64()
            .unwrap(),
    );
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    assert_eq!(count(&info, "roofs"), 12, "{info}");
    // Ridge vents reach the 18 % target, and not by much more than a pane per roof.
    let vents = info["vent_percent"].as_f64().unwrap();
    assert!((18.0..24.0).contains(&vents), "ridge vents {vents} %");
    assert_eq!(count(&info, "gutters"), 13, "2 eaves and 11 valleys");
    assert_eq!(count(&info, "curtain_walls"), 4);
    assert!((info["width"].as_f64().unwrap() - 48.0).abs() < 1e-9 && (info["floor_area"].as_f64().unwrap() - 3840.0).abs() < 1e-6);
    // The outer roofs' ridges at 5.5 m + (4 m − half the 125 mm valley gutter) / 2 × tan 22°;
    // the end walls fill up into them.
    let ridge = 5.5 + (4.0 - 0.0625) / 2.0 * 22f64.to_radians().tan();
    assert!((info["ridge"].as_f64().unwrap() - ridge).abs() < 1e-9);
    let store = doc.store();
    let end = info["curtain_walls"][1].as_u64().unwrap();
    let g = reviera_core::curtain::regenerate(store, store.get(ElementId(end)).unwrap()).unwrap();
    // Up into the gables, under the roofs' glazing bars (40 mm deep across the 22° slope).
    let bars = 0.04 / 22f64.to_radians().cos();
    assert!((g.z[1] - (ridge - bars)).abs() < 0.01, "the end wall reaches the ridges: {:?}", g.z);
    // Every part names its enclosure; the enclosure is a GlazedEnclosure assembly.
    let e = store.get(id).unwrap();
    assert_eq!(e.class, "IfcElementAssembly");
    assert_eq!(reviera_core::categories::category_of(e).map(|c| c.key), Some("cat.glazed_enclosures"));
    let guid = e.guid().to_string();
    // Posts at the 21 bay lines (80 m in 4 m bays) on the 7 truss and outer lines.
    assert_eq!(count(&info, "posts"), 21 * 7);
    assert!(store.get(ElementId(info["posts"][0].as_u64().unwrap())).is_some_and(|c| c.class == "IfcColumn"));
    // A gutter beam on each line of posts.
    assert_eq!(count(&info, "beams"), 7);
    assert_eq!(store.iter().filter(|p| reviera_core::enclosures::enclosure_of(p) == Some(guid.as_str())).count(), 12 + 13 + 4 + 21 * 7 + 7);

    // Three trusses and a knee wall: the parts are made again.
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "spans": 3, "knee": 0.6 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    // Knee walls along the sides only: the end walls stand on the floor with their doors.
    assert_eq!((count(&info, "roofs"), count(&info, "gutters"), count(&info, "walls")), (6, 7, 2), "{info}");
    assert_eq!(count(&info, "posts"), 21 * 4);
    assert_eq!(doc.store().iter().filter(|p| reviera_core::enclosures::enclosure_of(p) == Some(guid.as_str())).count(), 6 + 7 + 2 + 4 + 21 * 4 + 4, "the old parts are gone");
    for k in [1, 3] {
        let cw = ElementId(info["curtain_walls"][k].as_u64().unwrap());
        let g = reviera_core::curtain::regenerate(doc.store(), doc.store().get(cw).unwrap()).unwrap();
        let doors: Vec<_> = g.parts.iter().filter(|p| p.panel == reviera_core::curtain::Panel::Door && !p.mullion).collect();
        assert_eq!(doors.len(), 1, "one door in each end wall");
        let d = doors[0];
        assert!(d.x[1] - d.x[0] >= 1.4 && d.z[1] - d.z[0] >= 2.0 && d.z[0].abs() < 1e-9, "{d:?}");
        // Centred: its middle within half a pane of the wall's.
        assert!(((d.x[0] + d.x[1]) / 2.0 - g.length() / 2.0).abs() < 0.45, "{d:?} in {}", g.length());
    }
    // No vents when asked for none.
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "vents": 0 })).unwrap();
    let none = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    assert_eq!(none["vent_area"].as_f64().unwrap(), 0.0);
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "vents": 18 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": id.0 })).unwrap();
    // A detached gutter stays when the enclosure changes.
    let kept = info["gutters"][0].as_u64().unwrap();
    run(&r, &mut doc, "enclosure.detach", json!({ "part_ids": [kept] })).unwrap();
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "eave": 6.0 })).unwrap();
    assert!(doc.store().contains(ElementId(kept)));
    // Undo puts the eave back; bad values are refused.
    run(&r, &mut doc, "edit.undo", json!({})).ok();
    assert!(run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": id.0, "pitch": "75°" })).is_err());

    // Saved: the assembly, its roofs' glazing and the curtain walls' clipped gables are in IFC.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("GlazedEnclosure"));
    assert!(text.matches("IFCPLATE(").count() > 100, "glazing panels");
}

#[test]
fn a_gable_greenhouse_has_one_span() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("g.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let v = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": "20 m", "bay": 3.0, "eave": 3.0 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": v })).unwrap();
    assert_eq!((count(&info, "roofs"), count(&info, "gutters")), (1, 2));
    assert_eq!(count(&info, "warnings"), 0, "{info}");
    // A 4° roof is below twin-wall polycarbonate's 5° and flat enough to drip; at 7° glass may
    // be laid (2.5°) but still drips.
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": v, "pitch": "4°", "roof_glazing": "Twin-Wall Polycarbonate 10 mm" })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": v })).unwrap();
    assert_eq!(count(&info, "warnings"), 2, "{info}");
    assert!(info["warnings"][0].as_str().unwrap().contains("below the 5.0°"), "{info}");
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": v, "pitch": "7°", "roof_glazing": "" })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": v })).unwrap();
    assert_eq!(count(&info, "warnings"), 1, "{info}");
    assert!(run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "spans": 2 })).is_err());
    let forms = run(&r, &mut doc, "enclosure.forms", json!({})).unwrap();
    assert_eq!(forms.as_array().unwrap().len(), 6);
}

#[test]
fn a_sawtooth_range_has_vertical_vent_faces() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("s.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // The defaults: three 6.4 m spans at 25° from 4 m gutters, here 20 m long in 4 m bays.
    let v = run(&r, &mut doc, "enclosure.create", json!({ "form": "sawtooth", "level_id": level.0, "length": 20.0 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": v })).unwrap();
    // A gutter at each span's low eave; two side walls, two end walls and two inner faces; posts
    // at 6 bay lines on the 4 gutter lines, a beam on each.
    assert_eq!(
        (count(&info, "roofs"), count(&info, "gutters"), count(&info, "curtain_walls"), count(&info, "posts"), count(&info, "beams")),
        (3, 3, 6, 24, 4),
        "{info}"
    );
    let rise = 6.4 * 25f64.to_radians().tan();
    assert!((info["ridge"].as_f64().unwrap() - (4.0 + rise)).abs() < 1e-9);
    // One slope each: low at the span's start, high at its end.
    let store = doc.store();
    let first = store.get(ElementId(info["roofs"][0].as_u64().unwrap())).unwrap();
    let g = reviera_core::roofs::regenerate(store, first).unwrap();
    assert!((g.underside([10.0, 0.5]).unwrap() - (4.0 + 0.5 * rise / 6.4)).abs() < 0.1);
    assert!((g.underside([10.0, 6.3]).unwrap() - (4.0 + 6.3 * rise / 6.4)).abs() < 0.1);
    // The faces' vent bands: 18 % of each span's floor (1.15 m along their tops).
    let vents = info["vent_percent"].as_f64().unwrap();
    assert!((16.0..19.0).contains(&vents), "face vents {vents} %");
    // An inner face stands on the eave and reaches up under the roof's bars.
    let faces: Vec<_> = info["curtain_walls"].as_array().unwrap().iter().filter_map(|i| store.get(ElementId(i.as_u64().unwrap()))).filter(|e| reviera_core::curtain::curtain_recipe(e).is_some_and(|c| (c.curve.start[1] - 6.4).abs() < 1e-9)).collect();
    assert_eq!(faces.len(), 1);
    let cw = reviera_core::curtain::regenerate(store, faces[0]).unwrap();
    assert!((cw.z[0] - 4.0).abs() < 1e-9 && cw.z[1] < 4.0 + rise - 0.04 && cw.z[1] > 4.0 + rise - 0.15, "{:?}", cw.z);
    assert!(cw.parts.iter().any(|p| p.panel == reviera_core::curtain::Panel::Vent));
}

#[test]
fn hoop_and_gothic_tunnels_are_arched_film_roofs() {
    let dir = tempfile::tempdir().unwrap();
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("t.ifc")).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 9.1 m hoop on 1.2 m sidewalls, 30 m long, hoops at most 1.5 m apart.
    let hoop = run(&r, &mut doc, "enclosure.create", json!({ "form": "hoop", "level_id": level.0, "span": 9.1, "eave": 1.2, "length": 30.0 })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": hoop })).unwrap();
    assert!((info["ridge"].as_f64().unwrap() - 5.75).abs() < 1e-9, "{info}");
    assert_eq!((count(&info, "roofs"), count(&info, "gutters"), count(&info, "posts"), count(&info, "curtain_walls")), (1, 0, 0, 4));
    let roof = ElementId(info["roofs"][0].as_u64().unwrap());
    let store = doc.store();
    let e = store.get(roof).unwrap();
    assert!(reviera_core::roofs::roof_recipe(e).unwrap().extrusion.is_some(), "a roof by extrusion");
    let g = reviera_core::roofs::regenerate(store, e).unwrap();
    let top = g.faces.iter().flat_map(|f| f.bottom.iter().map(|p| p[2])).fold(f64::MIN, f64::max);
    assert!((top - 5.75).abs() < 0.05, "the crown: {top}");
    // 20 bays of film between 21 hoops on every facet.
    let gl = reviera_core::roofs::glazing::of(store, e).unwrap();
    assert!(gl.planes.iter().all(|p| p.grid2.len() == 19), "{:?}", gl.planes.iter().map(|p| p.grid2.len()).collect::<Vec<_>>());
    // The end walls rise into the arch.
    let end = ElementId(info["curtain_walls"][1].as_u64().unwrap());
    let cw = reviera_core::curtain::regenerate(store, store.get(end).unwrap()).unwrap();
    assert!(cw.z[1] > 5.5 && cw.z[1] < 5.75, "{:?}", cw.z);
    // A gothic tunnel beside it: a pointed crown, more height 1 m in from the side.
    let gothic = run(&r, &mut doc, "enclosure.create", json!({ "form": "gothic", "level_id": level.0, "span": 9.1, "eave": 1.2, "length": 30.0, "origin": [0, 20] })).unwrap();
    let ginfo = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gothic })).unwrap();
    assert!(ginfo["ridge"].as_f64().unwrap() > 5.75, "{ginfo}");
    let height_at = |info: &Value, y: f64| {
        let roof = ElementId(info["roofs"][0].as_u64().unwrap());
        let g = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
        g.underside([15.0, y]).unwrap()
    };
    assert!(height_at(&ginfo, 21.0) > height_at(&info, 1.0), "the gothic is fuller near its sides");
}

#[test]
fn glazing_types_carry_their_values_to_ifc_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gl.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let listing = run(&r, &mut doc, "glazing.list_types", json!({})).unwrap();
    assert!(listing["library"].as_array().unwrap().len() >= 10);
    // A gable greenhouse under twin-wall polycarbonate, glass walls.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "roof_glazing": "Twin-Wall Polycarbonate 10 mm" })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gh })).unwrap();
    let roof = ElementId(info["roofs"][0].as_u64().unwrap());
    let store = doc.store();
    let ty = store.get(store.find_by_guid(&reviera_core::GlobalId::parse(store.get(roof).unwrap().param("@type").unwrap().as_str().unwrap()).unwrap()).unwrap()).unwrap();
    let g = reviera_core::glazing_library::of_type(ty).unwrap();
    assert!((g.u - 3.2).abs() < 1e-9 && (g.light - 0.88).abs() < 1e-9, "{g:?}");
    // The energy model reads its U-value.
    assert_eq!(ty.param("Pset_RoofCommon.ThermalTransmittance").and_then(|v| v["value"].as_f64()), Some(3.2));
    // Changed by hand, then saved and read back.
    let ty_id = ty.id();
    assert!(run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": gh, "roof_glazing": "Unobtainium" })).is_err());
    run(&r, &mut doc, "glazing.set_type", json!({ "type_id": ty_id.0, "values": { "u": 3.0, "haze": 0.5 } })).unwrap();
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Reviera_GlazingUnit") && text.contains("ParTransmittance"));
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| e.name.as_deref() == Some("Twin-Wall Polycarbonate 10 mm (roof)")).expect("the type comes back");
    let g = reviera_core::glazing_library::of_type(back).unwrap_or_else(|| panic!("{:?}", back.params.iter().filter(|(k, _)| k.starts_with("Reviera_")).collect::<Vec<_>>()));
    assert!((g.u - 3.0).abs() < 1e-9 && (g.haze - 0.5).abs() < 1e-9, "{g:?}");
}

#[test]
fn frame_systems_set_the_bars_and_come_back_from_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fr.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let listing = run(&r, &mut doc, "frame.list_types", json!({})).unwrap();
    assert_eq!(listing["library"].as_array().unwrap().len(), 5);
    // A gable greenhouse on timber bars, 50 × 100 mm: both its glazing types carry the frame and
    // are made with its bars; the bars shade more of the roof than the default aluminium's.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0 })).unwrap();
    let before = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gh })).unwrap()["frame_shading"].as_f64().unwrap();
    assert!(run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": gh, "frame_system": "Bamboo" })).is_err());
    run(&r, &mut doc, "enclosure.set", json!({ "enclosure_id": gh, "frame_system": "Timber Glazing Bar 50×100" })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gh })).unwrap();
    let after = info["frame_shading"].as_f64().unwrap();
    // 40 mm bars on a 1.125 × 2.0 m grid shade about 5.5 %; 50 mm ones about 6.8 %.
    assert!((before - 0.0545).abs() < 0.001 && (after - 0.0677).abs() < 0.001, "{before} {after}");
    let store = doc.store();
    let type_of = |id: u64| {
        let e = store.get(ElementId(id)).unwrap();
        store.get(store.find_by_guid(&reviera_core::GlobalId::parse(e.param("@type").unwrap().as_str().unwrap()).unwrap()).unwrap()).unwrap()
    };
    let roof_type = type_of(info["roofs"][0].as_u64().unwrap());
    assert_eq!(roof_type.name.as_deref(), Some("Horticultural Float 4 mm on Timber Glazing Bar 50×100 (roof)"));
    let f = reviera_core::frame_library::of_type(roof_type).unwrap();
    assert_eq!((f.material, f.bar_depth), (reviera_core::frame_library::Material::Timber, 0.10));
    let wall_type = type_of(info["curtain_walls"][0].as_u64().unwrap());
    assert_eq!(reviera_core::curtain::curtain_type(wall_type).unwrap().interior.unwrap().depth, 0.10);
    // A frame type of its own, changed by hand, saved and read back.
    let ft = run(&r, &mut doc, "frame.create_type", json!({ "frame": "uPVC Glazing Bar 60×90" })).unwrap();
    assert_eq!(run(&r, &mut doc, "frame.create_type", json!({ "frame": "uPVC Glazing Bar 60×90" })).unwrap(), ft);
    run(&r, &mut doc, "frame.set_type", json!({ "type_id": ft, "values": { "deflection_ratio": 240.0, "finish": "anthracite" } })).unwrap();
    assert!(run(&r, &mut doc, "frame.set_type", json!({ "type_id": ft, "values": { "bar_width": 2.0 } })).is_err());
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Reviera_FrameSystem") && text.contains("IFCMEMBERTYPE"));
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().iter().find(|e| e.name.as_deref() == Some("uPVC Glazing Bar 60×90")).expect("the type comes back");
    let f = reviera_core::frame_library::of_type(back).unwrap();
    assert!((f.deflection_ratio - 240.0).abs() < 1e-9 && f.finish == "anthracite" && f.material == reviera_core::frame_library::Material::Upvc, "{f:?}");
}

#[test]
fn side_vents_and_vent_values_reach_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vents.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    // A 9.6 × 20 m gable house, its eaves at 3 m: ridge vents 18 %, side vents 10 %.
    let gh = run(&r, &mut doc, "enclosure.create", json!({ "form": "gable", "level_id": level.0, "length": 20.0, "eave": 3.0 })).unwrap();
    let before = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gh })).unwrap();
    assert_eq!(before["side_vent_area"].as_f64(), Some(0.0));
    run(&r, &mut doc, "enclosure.set_vents", json!({ "enclosure_id": gh, "kind": "side", "target_percent": 10.0, "opening_angle": "45°", "actuator": "chain" })).unwrap();
    let info = run(&r, &mut doc, "enclosure.get", json!({ "enclosure_id": gh })).unwrap();
    // Two bands of 10 % × 192 m² / 2 / 20 m = 0.48 m along the walls' tops.
    let side = info["side_vent_area"].as_f64().unwrap();
    assert!((side - 19.2).abs() < 0.5, "{info}");
    // Open at 45°, the free area is less than the panes, the effective less again (× 0.6).
    let (pane, free, eff) = (info["vent_area"].as_f64().unwrap(), info["free_vent_area"].as_f64().unwrap(), info["effective_vent_area"].as_f64().unwrap());
    assert!(free > 0.0 && free < pane && (eff - 0.6 * free).abs() < 1e-9, "{info}");
    // The hosts carry their vents' totals; the enclosure its design values.
    let store = doc.store();
    let wall = store.get(ElementId(info["curtain_walls"][0].as_u64().unwrap())).unwrap();
    assert_eq!(wall.param("Reviera_Vent.Actuator").and_then(|v| v["value"].as_str()), Some("chain"));
    assert!(wall.param("Reviera_Vent.VentCount").and_then(|v| v["value"].as_u64()).unwrap() > 0);
    let end = store.get(ElementId(info["curtain_walls"][1].as_u64().unwrap())).unwrap();
    assert!(end.param("Reviera_Vent.VentCount").is_none(), "an end wall has no vents");
    let e = store.get(ElementId(gh.as_u64().unwrap())).unwrap();
    assert_eq!(e.param("Reviera_EnclosureDesign.SideVentTarget").and_then(|v| v["value"].as_f64()), Some(0.1));
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("'Reviera_Vent'") && text.contains("'EffectiveArea'") && text.contains("'Reviera_EnclosureDesign'"));
    // Each vent window has its own set.
    let windows = text.matches("IFCWINDOW(").count();
    assert!(windows > 0 && text.matches("'Reviera_Vent'").count() >= windows, "{windows} vent windows");
    let (_p2, doc2, _) = LocalProject::open(&path).unwrap();
    let back = doc2.store().get(ElementId(info["curtain_walls"][0].as_u64().unwrap())).map(|e| e.guid().to_string());
    let again = doc2.store().iter().find(|x| Some(x.guid().to_string()) == back).expect("the side wall comes back");
    assert!(reviera_core::vents::settings_of(again).is_some_and(|s| (s.opening_angle - 45f64.to_radians()).abs() < 1e-9));
}
