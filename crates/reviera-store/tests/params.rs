// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter values in IFC property sets (spec Ch 2 §2.3.3, DM-IFC-003).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reviera_core::{Caller, CommandRegistry, Document, Element, ElementId, NewElement, Origin};
use reviera_ifc::{AttrValue, IfcModel, Schema};
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

fn walls(doc: &Document) -> Vec<&Element> {
    doc.store()
        .iter()
        .filter(|e| e.class == "IfcWallStandardCase" || e.class == "IfcWall")
        .collect()
}

fn all_params(doc: &Document) -> BTreeMap<ElementId, BTreeMap<String, Value>> {
    doc.store()
        .iter()
        .map(|e| (e.id(), e.params.clone()))
        .collect()
}

#[test]
fn properties_quantities_attributes_and_types_are_read() {
    let (_, doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let key = "Pset_WallCommon.ThermalTransmittance";
    let wall = *walls(&doc)
        .iter()
        .find(|w| w.params.contains_key(key))
        .unwrap();
    let p = &wall.params;
    assert_eq!(
        p[key],
        json!({"type": "IfcThermalTransmittanceMeasure", "value": 1.5})
    );
    let length = p
        .iter()
        .find(|(k, _)| k.ends_with(".Length") && k.starts_with("BaseQuantities"))
        .unwrap()
        .1;
    assert_eq!(length["type"], "IfcQuantityLength");
    assert!(length["value"].as_f64().unwrap() > 0.0);
    assert!(!p.keys().any(|k| k.starts_with("Reviera_")));

    // Every occurrence linked to a type points at a type element by GlobalId.
    let typed: Vec<_> = doc
        .store()
        .iter()
        .filter(|e| e.params.contains_key("@type"))
        .collect();
    assert!(!typed.is_empty());
    for e in typed {
        let guid = reviera_core::GlobalId::parse(e.params["@type"].as_str().unwrap()).unwrap();
        let t = doc
            .store()
            .get(doc.store().find_by_guid(&guid).unwrap())
            .unwrap();
        assert!(t.class.ends_with("Type"), "{}", t.class);
    }
}

#[test]
fn unchanged_values_round_trip_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("haus.ifc");
    let (mut project, doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    project.save_as(&doc, &out).unwrap();
    let (_, reopened, _) = LocalProject::open(&out).unwrap();
    assert_eq!(all_params(&reopened), all_params(&doc));
}

#[test]
fn setting_and_removing_properties_writes_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("edited.ifc");
    let r = registry();
    let (mut project, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let ids: Vec<ElementId> = walls(&doc).iter().map(|w| w.id()).collect();
    let before_other = doc.store().get(ids[1]).unwrap().params.clone();
    let entities_before = project.ifc().entity_count();

    // An existing standard property, a new property in a standard set, and a new custom set.
    run(&r, &mut doc, "pset.set", json!({"element_id": ids[0], "pset_name": "Pset_WallCommon", "property_name": "IsExternal", "value": false})).unwrap();
    run(&r, &mut doc, "pset.set", json!({"element_id": ids[0], "pset_name": "Pset_WallCommon", "property_name": "FireRating", "value": "EI 60"})).unwrap();
    run(&r, &mut doc, "pset.set", json!({"element_id": ids[0], "pset_name": "Acoustics", "property_name": "AcousticClass", "value": "B"})).unwrap();
    run(&r, &mut doc, "pset.set", json!({"element_id": ids[0], "pset_name": "Acoustics", "property_name": "RatedIndex", "value": 52, "ifc_type": "IfcInteger"})).unwrap();
    project.save_as(&doc, &out).unwrap();

    let (mut project2, mut doc2, _) = LocalProject::open(&out).unwrap();
    let p = &doc2.store().get(ids[0]).unwrap().params;
    assert_eq!(
        p["Pset_WallCommon.IsExternal"],
        json!({"type": "IfcBoolean", "value": false})
    );
    assert_eq!(
        p["Pset_WallCommon.FireRating"],
        json!({"type": "IfcLabel", "value": "EI 60"})
    );
    assert_eq!(
        p["Acoustics.RatedIndex"],
        json!({"type": "IfcInteger", "value": 52})
    );
    assert_eq!(
        doc2.store().get(ids[1]).unwrap().params,
        before_other,
        "other walls are untouched"
    );

    // Removing a whole set removes its property set, relationship and properties.
    let removed = run(
        &r,
        &mut doc2,
        "pset.remove",
        json!({"element_id": ids[0], "pset_name": "Acoustics"}),
    )
    .unwrap();
    assert_eq!(
        removed,
        json!(["Acoustics.AcousticClass", "Acoustics.RatedIndex"])
    );
    run(&r, &mut doc2, "pset.remove", json!({"element_id": ids[0], "pset_name": "Pset_WallCommon", "property_name": "FireRating"})).unwrap();
    project2.save(&doc2).unwrap();
    let (project3, doc3, _) = LocalProject::open(&out).unwrap();
    let p = &doc3.store().get(ids[0]).unwrap().params;
    assert!(
        !p.contains_key("Acoustics.AcousticClass") && !p.contains_key("Pset_WallCommon.FireRating")
    );
    assert_eq!(p["Pset_WallCommon.IsExternal"]["value"], false);
    // What remains beyond the original file is Reviera's own identity data, one set and relationship per element...
    let reviera_sets = project3
        .ifc()
        .entities_of_type("IfcPropertySet")
        .unwrap()
        .iter()
        .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("Reviera_")))
        .count();
    assert!(project3.ifc().entity_count() > entities_before);
    assert!(reviera_sets > 0);
    // ...and no relationship is left without objects.
    for rel in project3
        .ifc()
        .entities_of_type("IfcRelDefinesByProperties")
        .unwrap()
    {
        assert!(!matches!(
            project3.ifc().attribute(rel.id, "RelatedObjects").unwrap(),
            AttrValue::Null
        ));
    }
}

#[test]
fn quantities_and_reserved_sets_cannot_be_set() {
    let r = registry();
    let (_, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let wall = walls(&doc)[0].id();
    let qto_key = doc
        .store()
        .get(wall)
        .unwrap()
        .params
        .keys()
        .find(|k| k.starts_with("BaseQuantities."))
        .unwrap()
        .clone();
    let (set, prop) = qto_key.split_once('.').unwrap();
    let e = run(
        &r,
        &mut doc,
        "pset.set",
        json!({"element_id": wall, "pset_name": set, "property_name": prop, "value": 1.0}),
    )
    .unwrap_err();
    assert_eq!(e.code, "pset.read_only");
    let e = run(&r, &mut doc, "pset.set", json!({"element_id": wall, "pset_name": "Reviera_Element", "property_name": "Data", "value": "x"})).unwrap_err();
    assert_eq!(e.code, "pset.reserved");
    let e = run(&r, &mut doc, "pset.set", json!({"element_id": wall, "pset_name": "P", "property_name": "N", "value": "text", "ifc_type": "IfcInteger"})).unwrap_err();
    assert_eq!(e.code, "args.invalid");
}

#[test]
fn type_properties_go_on_the_type_and_deleting_removes_sets() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("types.ifc");
    let r = registry();
    let (mut project, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let wall_type = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWallType")
        .map(|e| e.id())
        .expect("a wall type");
    run(&r, &mut doc, "pset.set", json!({"element_id": wall_type, "pset_name": "Pset_WallCommon", "property_name": "FireRating", "value": "EI 60"})).unwrap();
    let doomed = walls(&doc)[2].id();
    let doomed_sets = doc
        .store()
        .get(doomed)
        .unwrap()
        .params
        .keys()
        .filter_map(|k| k.split_once('.').map(|s| s.0.to_owned()))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let sets_before = project
        .ifc()
        .entities_of_type("IfcPropertySetDefinition")
        .unwrap()
        .len();
    let mut txn = doc.transaction("Delete", Origin::ui());
    txn.delete(doomed).unwrap();
    txn.commit();
    project.save_as(&doc, &out).unwrap();

    let saved = IfcModel::open(&out).unwrap();
    let ty = saved
        .find_by_guid(doc.store().get(wall_type).unwrap().guid())
        .unwrap();
    let AttrValue::List(sets) = saved.attribute(ty, "HasPropertySets").unwrap() else {
        panic!("no type sets")
    };
    assert_eq!(sets.len(), 2, "the new set and Reviera's type data");
    let (_, reopened, _) = LocalProject::open(&out).unwrap();
    assert_eq!(
        reopened.store().get(wall_type).unwrap().params["Pset_WallCommon.FireRating"]["value"],
        "EI 60"
    );
    // The deleted wall's sets are gone; added are the type's new set and Reviera's own data (one set
    // per element, plus project/identity and the manifest).
    let sets_after = saved
        .entities_of_type("IfcPropertySetDefinition")
        .unwrap()
        .len();
    assert_eq!(
        sets_after,
        sets_before - doomed_sets + 1 + reopened.store().len() + 2
    );
    for rel in saved.entities_of_type("IfcRelDefinesByProperties").unwrap() {
        assert!(!matches!(
            saved.attribute(rel.id, "RelatedObjects").unwrap(),
            AttrValue::Null
        ));
    }
}

#[test]
fn shared_property_sets_are_copied_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut txn = doc.transaction("Proxies", Origin::ui());
    let a = txn
        .create(NewElement::new("IfcBuildingElementProxy").name("A"))
        .unwrap();
    let b = txn
        .create(NewElement::new("IfcBuildingElementProxy").name("B"))
        .unwrap();
    txn.commit();
    project.save(&doc).unwrap();

    // Another tool shares one property set between both proxies.
    let mut m = IfcModel::open(&path).unwrap();
    let (sa, sb) = (
        m.find_by_guid(doc.store().get(a).unwrap().guid()).unwrap(),
        m.find_by_guid(doc.store().get(b).unwrap().guid()).unwrap(),
    );
    let prop = m.create("IfcPropertySingleValue").unwrap();
    m.set_attribute(prop, "Name", &AttrValue::String("Colour".into()))
        .unwrap();
    m.set_attribute(
        prop,
        "NominalValue",
        &AttrValue::Typed {
            type_name: "IfcLabel".into(),
            value: Box::new(AttrValue::String("Red".into())),
        },
    )
    .unwrap();
    let pset = m
        .create_rooted("IfcPropertySet", &reviera_core::GlobalId::new())
        .unwrap();
    m.set_attribute(pset, "Name", &AttrValue::String("Finish".into()))
        .unwrap();
    m.set_attribute(
        pset,
        "HasProperties",
        &AttrValue::List(vec![AttrValue::Ref(prop.0)]),
    )
    .unwrap();
    let rel = m
        .create_rooted("IfcRelDefinesByProperties", &reviera_core::GlobalId::new())
        .unwrap();
    m.set_attribute(
        rel,
        "RelatedObjects",
        &AttrValue::List(vec![AttrValue::Ref(sa.0), AttrValue::Ref(sb.0)]),
    )
    .unwrap();
    m.set_attribute(rel, "RelatingPropertyDefinition", &AttrValue::Ref(pset.0))
        .unwrap();
    m.write(&path).unwrap();

    let r = registry();
    let (mut project, mut doc, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        doc.store().get(b).unwrap().params["Finish.Colour"]["value"],
        "Red"
    );
    run(
        &r,
        &mut doc,
        "pset.set",
        json!({"element_id": a, "pset_name": "Finish", "property_name": "Colour", "value": "Blue"}),
    )
    .unwrap();
    project.save(&doc).unwrap();
    let (_, doc, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        doc.store().get(a).unwrap().params["Finish.Colour"]["value"],
        "Blue"
    );
    assert_eq!(
        doc.store().get(b).unwrap().params["Finish.Colour"]["value"],
        "Red",
        "B keeps the shared value"
    );
}
