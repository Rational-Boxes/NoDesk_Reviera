// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Local mode acceptance: identity survives save and reopen (DM-ID-001), third-party files keep
//! every entity (DM-CORE-001), RXD is embedded deterministically (Ch 2a §11).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reviera_core::{Document, ElementId, GlobalId, NewElement, Origin};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;

fn fzk_haus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc")
}

fn identities(doc: &Document) -> BTreeMap<GlobalId, (ElementId, String, Option<String>)> {
    doc.store()
        .iter()
        .map(|e| (e.guid().clone(), (e.id(), e.class.clone(), e.name.clone())))
        .collect()
}

#[test]
fn ten_thousand_elements_keep_guids_and_ids_across_two_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut txn = doc.transaction("Place proxies", Origin::ui());
    for i in 0..10_000 {
        txn.create(NewElement::new("IfcBuildingElementProxy").name(format!("Proxy {i}")))
            .unwrap();
    }
    txn.commit();
    let report = project.save(&doc).unwrap();
    assert_eq!(
        (report.created, report.element_data_written),
        (10_000, 10_001)
    );
    let before = identities(&doc);

    let (mut project, doc2, open) = LocalProject::open(&path).unwrap();
    assert_eq!(
        (open.ids_from_rxd, open.ids_assigned),
        (10_001, 0),
        "{:?}",
        open.problems
    );
    assert_eq!(identities(&doc2), before);
    assert_eq!(doc2.next_id(), ElementId(11_000));
    project.save(&doc2).unwrap();

    let (_, doc3, _) = LocalProject::open(&path).unwrap();
    assert_eq!(identities(&doc3), before);
}

#[test]
fn third_party_file_gets_ids_in_step_order_and_keeps_them() {
    let (project, doc, report) = LocalProject::open(&fzk_haus()).unwrap();
    assert_eq!(report.ids_from_rxd, 0);
    assert_eq!(report.ids_assigned, report.elements);
    assert!(report.problems.is_empty(), "{:?}", report.problems);

    // The project takes the reserved ID 1; the rest ascend with the STEP instance number.
    let project_elem = doc.store().get(ElementId::PROJECT).unwrap();
    assert_eq!(project_elem.class, "IfcProject");
    let mut by_step: Vec<_> = doc
        .store()
        .iter()
        .filter(|e| e.id() != ElementId::PROJECT)
        .map(|e| (project.step_of(e.id()).unwrap(), e.id()))
        .collect();
    by_step.sort();
    assert!(
        by_step.windows(2).all(|w| w[0].1 < w[1].1),
        "IDs follow STEP order"
    );

    // Deterministic for the same file.
    let (_, again, _) = LocalProject::open(&fzk_haus()).unwrap();
    assert_eq!(identities(&again), identities(&doc));

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("haus.ifc");
    let (mut project, doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    project.save_as(&doc, &out).unwrap();
    let (_, reopened, report) = LocalProject::open(&out).unwrap();
    assert_eq!(
        (report.ids_from_rxd, report.ids_assigned),
        (report.elements, 0),
        "{:?}",
        report.problems
    );
    assert_eq!(identities(&reopened), identities(&doc));

    // Every original rooted entity is still there with its class; only Reviera_* data was added.
    let original = IfcModel::open(&fzk_haus()).unwrap();
    let saved = IfcModel::open(&out).unwrap();
    let classes = |m: &IfcModel| -> BTreeMap<GlobalId, String> {
        m.rooted()
            .unwrap()
            .into_iter()
            .filter_map(|e| Some((e.global_id?, e.class)))
            .collect()
    };
    let (a, b) = (classes(&original), classes(&saved));
    assert!(a.iter().all(|(g, c)| b.get(g) == Some(c)));
    let added: Vec<_> = b.keys().filter(|g| !a.contains_key(*g)).collect();
    // A property set per element plus a relationship for each non-type element (type objects use
    // HasPropertySets), and the same for project/identity and the manifest.
    let types = original.entities_of_type("IfcTypeObject").unwrap().len();
    assert_eq!(added.len(), 2 * report.elements - types + 4);
}

#[test]
fn edits_deletions_and_stamps_are_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.ifc");
    let (mut project, mut doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    let walls: Vec<_> = doc
        .store()
        .iter()
        .filter(|e| e.class == "IfcWallStandardCase" || e.class == "IfcWall")
        .map(|e| e.id())
        .collect();
    assert!(walls.len() >= 2);
    let mut txn = doc.transaction("Rename and delete", Origin::mcp("agent", "s1"));
    txn.modify(walls[0], |e| e.name = Some("Renamed wall".into()))
        .unwrap();
    txn.delete(walls[1]).unwrap();
    let new_id = txn
        .create(NewElement::new("IfcBuildingElementProxy").name("New proxy"))
        .unwrap();
    txn.commit();
    let report = project.save_as(&doc, &path).unwrap();
    assert_eq!((report.created, report.updated, report.removed), (1, 1, 1));

    let (project, reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(
        reopened.store().get(walls[0]).unwrap().name.as_deref(),
        Some("Renamed wall")
    );
    assert!(reopened.store().get(walls[1]).is_none());
    assert_eq!(
        reopened.store().get(new_id).unwrap().class,
        "IfcBuildingElementProxy"
    );
    let identity = project
        .element_data(walls[0])
        .unwrap()
        .identity()
        .unwrap()
        .unwrap();
    assert_eq!(identity.last_modified_by.as_deref(), Some("mcp:agent"));
    assert_eq!(identity.iid, walls[0].0);
}

#[test]
fn resaving_an_unchanged_project_writes_identical_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same.ifc");
    let (mut project, doc, _) = LocalProject::open(&fzk_haus()).unwrap();
    project.save_as(&doc, &path).unwrap();
    let (mut first, doc1, _) = LocalProject::open(&path).unwrap();
    let payloads = |p: &LocalProject, d: &Document| -> Vec<_> {
        d.store()
            .iter()
            .map(|e| p.element_payload(e.id()))
            .collect()
    };
    let before = payloads(&first, &doc1);
    let report = first.save(&doc1).unwrap();
    assert_eq!(report.element_data_written, 0);
    assert_eq!(report.data_revision, 2);
    let (second, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(payloads(&second, &doc2), before);
    assert_eq!(second.data_revision(), 2);
}

#[test]
fn rxd_only_elements_are_refused_rather_than_lost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rxd.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let mut txn = doc.transaction("View", Origin::ui());
    txn.create(NewElement::new("view.plan").name("Level 1"))
        .unwrap();
    txn.commit();
    assert!(project.save(&doc).is_err());
    assert!(!path.exists(), "nothing is written when a save fails");
}
