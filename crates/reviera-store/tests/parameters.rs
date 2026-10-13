// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Categories and parameter definitions (spec Ch 2 §2.2.2, §2.9.1–§2.9.3), on the FZK-Haus model.

use std::path::{Path, PathBuf};

use reviera_core::categories::category_of;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn fzk_haus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc")
}

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn of_category(doc: &Document, key: &str, types: bool) -> Vec<ElementId> {
    doc.store()
        .iter()
        .filter(|e| {
            category_of(e).is_some_and(|c| c.key == key)
                && reviera_core::categories::is_type(e) == types
        })
        .map(|e| e.id())
        .collect()
}

#[test]
fn third_party_elements_get_categories() {
    let (_, doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    assert!(
        !of_category(&doc, "cat.doors", false).is_empty(),
        "IfcDoor without RXD is a door"
    );
    assert!(
        !of_category(&doc, "cat.walls", false).is_empty(),
        "IfcWallStandardCase is a wall"
    );
    assert!(
        !of_category(&doc, "cat.windows", true).is_empty(),
        "types share their occurrences' category"
    );
    assert!(!of_category(&doc, "cat.levels", false).is_empty());
}

#[test]
fn fire_rating_is_a_type_parameter_written_to_the_wall_type() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("fire.ifc");
    let r = registry();
    let (mut project, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let wall = *of_category(&doc, "cat.walls", false)
        .iter()
        .find(|w| doc.store().get(**w).unwrap().params.contains_key("@type"))
        .unwrap();

    let defs = run(&r, &mut doc, "param.list", json!({"category": "cat.walls"})).unwrap();
    let fire = defs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["ref"] == "type.fire_rating")
        .unwrap();
    assert_eq!(fire["binding"], "type");
    assert_eq!(
        fire["ifc_home"],
        json!({"kind": "property", "pset": "Pset_WallCommon", "property": "FireRating"})
    );

    let written = run(
        &r,
        &mut doc,
        "param.set",
        json!({"element_ids": [wall], "param_ref": "type.fire_rating", "value": "EI 60"}),
    )
    .unwrap();
    let wall_type = ElementId(written[0].as_u64().unwrap());
    assert_ne!(wall_type, wall, "written on the type");
    let got = run(
        &r,
        &mut doc,
        "param.get",
        json!({"element_id": wall, "param_ref": "Fire Rating"}),
    )
    .unwrap();
    assert_eq!(
        (got["value"].clone(), got["source"].clone()),
        (json!("EI 60"), json!("type"))
    );
    project.save_as(&doc, &out).unwrap();

    let (_, reopened, _) = LocalProject::open(&out).unwrap();
    assert_eq!(
        reopened.store().get(wall_type).unwrap().params["Pset_WallCommon.FireRating"],
        json!({"type": "IfcLabel", "value": "EI 60"})
    );
    assert!(
        !reopened
            .store()
            .get(wall)
            .unwrap()
            .params
            .contains_key("Pset_WallCommon.FireRating")
    );
}

#[test]
fn computed_parameters_are_read_only() {
    let r = registry();
    let (_, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let wall = of_category(&doc, "cat.walls", false)[0];
    let area = run(
        &r,
        &mut doc,
        "param.get",
        json!({"element_id": wall, "param_ref": "elem.area"}),
    )
    .unwrap();
    assert_eq!(area["read_only"], true);
    assert_eq!(area["source"], "computed");
    assert!(area["value"].as_f64().unwrap() > 0.0, "{area}");
    assert!(area["display"].as_str().unwrap().contains("m²"));
    let e = r
        .execute(
            &mut doc,
            "param.set",
            json!({"element_ids": [wall], "param_ref": "elem.area", "value": 3.0}),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(e.code, "param.read_only");
}

#[test]
fn project_parameters_bind_to_categories() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("project.ifc");
    let r = registry();
    let (mut project, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();

    let e = r
        .execute(&mut doc, "param.project.add", json!({"def": {"name": "Fire Rating", "data_type": "text"}, "categories": ["cat.doors"], "is_instance": false}), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "param.name_clash");
    assert!(e.message.contains("type.fire_rating"));

    let id = run(&r, &mut doc, "param.project.add", json!({
        "def": {"name": "Room Finish Code", "data_type": "text", "group": "identity_data"},
        "categories": ["cat.rooms", "cat.areas"], "is_instance": true, "pset_name": "FZK Parameters"
    })).unwrap();
    for cat in ["cat.rooms", "cat.areas"] {
        let defs = run(&r, &mut doc, "param.list", json!({"category": cat})).unwrap();
        assert!(
            defs.as_array()
                .unwrap()
                .iter()
                .any(|d| d["name"] == "Room Finish Code"),
            "{cat}"
        );
    }
    let room = of_category(&doc, "cat.rooms", false)[0];
    run(
        &r,
        &mut doc,
        "param.set",
        json!({"element_ids": [room], "param_ref": "Room Finish Code", "value": "RF-2"}),
    )
    .unwrap();
    assert_eq!(
        doc.store().get(room).unwrap().params["FZK Parameters.Room Finish Code"],
        json!({"type": "IfcLabel", "value": "RF-2"})
    );
    project.save_as(&doc, &out).unwrap();

    // Definitions travel in the file; values are ordinary IFC properties.
    let (_, mut reopened, _) = LocalProject::open(&out).unwrap();
    let got = run(
        &r,
        &mut reopened,
        "param.get",
        json!({"element_id": room, "param_ref": id}),
    )
    .unwrap();
    assert_eq!(got["value"], "RF-2");
    let removed = run(
        &r,
        &mut reopened,
        "param.project.remove",
        json!({"param_id": id}),
    )
    .unwrap();
    assert_eq!(removed, 1);
    assert!(
        !reopened
            .store()
            .get(room)
            .unwrap()
            .params
            .contains_key("FZK Parameters.Room Finish Code")
    );
}

#[test]
fn shared_parameters_map_onto_standard_properties() {
    let r = registry();
    let (_, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let guid = "6f1c2c1e-6c0a-4d5e-9b1c-2a3b4c5d6e7f";
    run(&r, &mut doc, "param.project.add", json!({
        "def": {"name": "Acoustic Rating", "data_type": "text", "guid": guid}, "categories": ["cat.walls", "cat.doors"], "is_instance": true
    })).unwrap();
    run(&r, &mut doc, "param.set_ifc_home", json!({"param_ref": guid, "category": "cat.walls", "pset_name": "Pset_WallCommon", "property_name": "AcousticRating"})).unwrap();
    run(&r, &mut doc, "param.set_ifc_home", json!({"param_ref": guid, "category": "cat.doors", "pset_name": "Pset_DoorCommon", "property_name": "AcousticRating"})).unwrap();
    let wall = of_category(&doc, "cat.walls", false)[0];
    let door = of_category(&doc, "cat.doors", false)[0];
    run(
        &r,
        &mut doc,
        "param.set",
        json!({"element_ids": [wall, door], "param_ref": "Acoustic Rating", "value": "Rw 52 dB"}),
    )
    .unwrap();
    assert_eq!(
        doc.store().get(wall).unwrap().params["Pset_WallCommon.AcousticRating"]["value"],
        "Rw 52 dB"
    );
    assert_eq!(
        doc.store().get(door).unwrap().params["Pset_DoorCommon.AcousticRating"]["value"],
        "Rw 52 dB"
    );
    let def = run(&r, &mut doc, "param.definition", json!({"param_ref": guid})).unwrap();
    assert_eq!(def["kind"], "shared");

    // A built-in's IFC home cannot be taken.
    let e = r
        .execute(&mut doc, "param.set_ifc_home", json!({"param_ref": guid, "category": "cat.walls", "pset_name": "Pset_WallCommon", "property_name": "FireRating"}), &Caller::user())
        .unwrap_err();
    assert_eq!(e.code, "param.home_clash");
    assert!(e.message.contains("Fire Rating"), "{}", e.message);
}

#[test]
fn ifc_native_properties_are_listed_and_editable() {
    let r = registry();
    let (_, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let wall = of_category(&doc, "cat.walls", false)[0];
    let defs = run(&r, &mut doc, "param.list", json!({"element_id": wall})).unwrap();
    let native = defs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "ifc_native")
        .expect("an IFC-native property")
        .clone();
    let reference = native["ref"].as_str().unwrap().to_owned();
    let current = run(
        &r,
        &mut doc,
        "param.get",
        json!({"element_id": wall, "param_ref": reference}),
    )
    .unwrap();
    assert_eq!(current["source"], "instance");
    if native["data_type"] == "text" {
        run(
            &r,
            &mut doc,
            "param.set",
            json!({"element_ids": [wall], "param_ref": reference, "value": "edited"}),
        )
        .unwrap();
        let got = run(
            &r,
            &mut doc,
            "param.get",
            json!({"element_id": wall, "param_ref": reference}),
        )
        .unwrap();
        assert_eq!(got["value"], "edited");
    }
}

#[test]
fn switching_instance_to_type_needs_a_resolution_when_values_differ() {
    let r = registry();
    let (_, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    // Two windows of the same type.
    let windows = of_category(&doc, "cat.windows", false);
    let type_key = |id: ElementId| doc.store().get(id).unwrap().params.get("@type").cloned();
    let (a, b) = windows
        .iter()
        .flat_map(|x| windows.iter().map(move |y| (*x, *y)))
        .find(|(x, y)| x < y && type_key(*x).is_some() && type_key(*x) == type_key(*y))
        .expect("two windows sharing a type");
    let id = run(&r, &mut doc, "param.project.add", json!({"def": {"name": "Hardware Set", "data_type": "text"}, "categories": ["cat.windows"], "is_instance": true})).unwrap();
    run(
        &r,
        &mut doc,
        "param.set_many",
        json!({"changes": [
            {"element_id": a, "param_ref": "Hardware Set", "value": "HW-1"},
            {"element_id": b, "param_ref": "Hardware Set", "value": "HW-2"}
        ]}),
    )
    .unwrap();
    let e = r
        .execute(
            &mut doc,
            "param.project.modify",
            json!({"param_id": id, "changes": {"is_instance": false}}),
            &Caller::user(),
        )
        .unwrap_err();
    assert_eq!(e.code, "param.resolution_required");
    let report = run(
        &r,
        &mut doc,
        "param.project.modify",
        json!({"param_id": id, "changes": {"is_instance": false, "resolution": "first"}}),
    )
    .unwrap();
    assert_eq!(report["conflicts"], 1);
    let got = run(
        &r,
        &mut doc,
        "param.get",
        json!({"element_id": b, "param_ref": "Hardware Set"}),
    )
    .unwrap();
    assert_eq!(
        (got["value"].clone(), got["source"].clone()),
        (json!("HW-1"), json!("type"))
    );
}
