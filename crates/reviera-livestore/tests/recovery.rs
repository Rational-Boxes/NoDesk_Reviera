// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Crash recovery acceptance tests (Ch 2b §11). Each scenario runs in a child process (this test
//! binary re-run with `REVIERA_LIVE_CHILD` set) that aborts after editing, as a real crash would.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use reviera_core::{Document, GroupEnd, NewElement, Origin, UndoScope};
use reviera_ifc::Schema;
use reviera_livestore::{LiveError, LiveStore, Options, RestoreSource};
use reviera_store::LocalProject;

fn options(dir: &Path) -> Options {
    Options {
        dir: dir.join("live"),
        group_commit: Duration::from_millis(50),
        ..Options::default()
    }
}

fn create(doc: &mut Document, name: &str, class: &str) {
    let mut txn = doc.transaction(name, Origin::ui());
    txn.create(NewElement::new(class).name(name)).unwrap();
    txn.commit();
}

/// Runs a scenario in a child process and waits for it to crash.
fn crash(scenario: &str, dir: &Path) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("REVIERA_LIVE_CHILD", scenario)
        .env("REVIERA_LIVE_DIR", dir)
        .status()
        .unwrap();
    assert!(!status.success(), "the child should have aborted");
}

/// The child side. A no-op in a normal test run.
#[test]
fn child_process() {
    let (Ok(scenario), Some(dir)) = (
        std::env::var("REVIERA_LIVE_CHILD"),
        std::env::var_os("REVIERA_LIVE_DIR"),
    ) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let opts = options(&dir);
    match scenario.as_str() {
        "wall" => {
            let (mut project, mut doc) =
                LocalProject::new(Schema::Ifc4, &dir.join("project.ifc")).unwrap();
            project.save(&doc).unwrap();
            let store = LiveStore::create(&project, &mut doc, &opts).unwrap();
            store.saved(&project, &doc).unwrap();
            create(&mut doc, "Wall 1", "IfcWall");
            // "Kill the process 1 s after creating a wall."
            std::thread::sleep(Duration::from_secs(1));
        }
        "untitled" => {
            let (project, mut doc) =
                LocalProject::new(Schema::Ifc4, &dir.join("untitled.ifc")).unwrap();
            let store = LiveStore::create(&project, &mut doc, &opts).unwrap();
            for i in 0..17 {
                create(&mut doc, &format!("Proxy {i}"), "IfcBuildingElementProxy");
            }
            doc.group_begin("Batch", Origin::ui()).unwrap();
            for i in 17..20 {
                create(&mut doc, &format!("Proxy {i}"), "IfcBuildingElementProxy");
            }
            doc.group_end(GroupEnd::Assimilate).unwrap();
            store.flush().unwrap();
        }
        "edits" => {
            let (mut project, mut doc) =
                LocalProject::new(Schema::Ifc4, &dir.join("project.ifc")).unwrap();
            project.save(&doc).unwrap();
            let store = LiveStore::create(&project, &mut doc, &opts).unwrap();
            store.saved(&project, &doc).unwrap();
            for i in 0..3 {
                create(&mut doc, &format!("Proxy {i}"), "IfcBuildingElementProxy");
            }
            store.flush().unwrap();
        }
        other => panic!("unknown scenario {other}"),
    }
    std::process::abort();
}

fn names(doc: &Document) -> Vec<String> {
    let mut v: Vec<_> = doc.store().iter().filter_map(|e| e.name.clone()).collect();
    v.sort();
    v
}

#[test]
fn crash_after_creating_a_wall_is_restored_and_undoable() {
    let dir = tempfile::tempdir().unwrap();
    crash("wall", dir.path());
    let opts = options(dir.path());

    let found = reviera_livestore::scan(&opts.dir).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].last_labels, ["Wall 1"]);
    assert_eq!(found[0].unsaved_changes, Some(1));
    assert_eq!(
        found[0].project_path.as_deref(),
        Some(dir.path().join("project.ifc").as_path())
    );

    let mut r = reviera_livestore::restore(&found[0].key, None, &opts).unwrap();
    assert_eq!(r.report.source, RestoreSource::Materialised);
    assert_eq!(r.report.last.as_ref().map(|l| l.0.as_str()), Some("Wall 1"));
    assert_eq!(
        names(&r.doc),
        ["Building", "Level 1", "Project", "Site", "Wall 1"]
    );
    assert!(r.project.is_dirty(&r.doc), "a recovered project is unsaved");

    // Undo still works after recovery, and redo brings the wall back.
    r.doc.undo(1, &UndoScope::All, &Origin::ui()).unwrap();
    assert_eq!(names(&r.doc), ["Building", "Level 1", "Project", "Site"]);
    r.doc.redo(1, &UndoScope::All, &Origin::ui()).unwrap();
    r.project.save(&r.doc).unwrap();
    r.store.saved(&r.project, &r.doc).unwrap();
    r.store.close(&mut r.doc).unwrap();

    let (_, reopened, _) = LocalProject::open(&dir.path().join("project.ifc")).unwrap();
    assert_eq!(
        names(&reopened),
        ["Building", "Level 1", "Project", "Site", "Wall 1"]
    );
    assert!(
        reviera_livestore::scan(&opts.dir).unwrap().is_empty(),
        "a clean close leaves nothing to recover"
    );
    assert!(
        opts.dir.join("bin").read_dir().unwrap().count() > 0,
        "the old store is kept in the recovery bin"
    );
}

#[test]
fn untitled_project_with_twenty_edits_is_recovered_and_saved_as_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    crash("untitled", dir.path());
    let opts = options(dir.path());
    let found = reviera_livestore::scan(&opts.dir).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].unsaved_changes, Some(20));
    assert_eq!(found[0].last_labels[0], "Batch");

    let copy = dir.path().join("copy.ifc");
    let mut r = reviera_livestore::restore(&found[0].key, Some(&copy), &opts).unwrap();
    assert_eq!(r.doc.store().len(), 24);
    assert_eq!(
        r.report.transactions, 18,
        "17 single edits and one assimilated group"
    );

    // The group is one undo step.
    r.doc.undo(1, &UndoScope::All, &Origin::ui()).unwrap();
    assert_eq!(r.doc.store().len(), 21);
    r.doc.redo(1, &UndoScope::All, &Origin::ui()).unwrap();

    r.project.save(&r.doc).unwrap();
    r.store.close(&mut r.doc).unwrap();
    let (_, reopened, _) = LocalProject::open(&copy).unwrap();
    assert_eq!(reopened.store().len(), 24);
    assert!(!dir.path().join("untitled.ifc").exists());
}

#[test]
fn damaged_store_falls_back_to_base_ifc_and_journal_replay() {
    let dir = tempfile::tempdir().unwrap();
    crash("edits", dir.path());
    let opts = options(dir.path());
    let key = reviera_livestore::scan(&opts.dir).unwrap()[0].key.clone();

    // "Truncate the .rvlive file to half its size."
    let db = opts.dir.join(format!("{key}.rvlive"));
    let len = std::fs::metadata(&db).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&db)
        .unwrap()
        .set_len(len / 2)
        .unwrap();

    let found = reviera_livestore::scan(&opts.dir).unwrap();
    assert_eq!(
        found.len(),
        1,
        "a damaged store is still offered for recovery"
    );
    let mut r = reviera_livestore::restore(&key, None, &opts).unwrap();
    eprintln!("fallback report: {:?}", r.report);
    assert_eq!(r.report.source, RestoreSource::BaseIfcReplay);
    assert!(!r.report.problems.is_empty());
    assert!(r.report.transactions <= 3);
    let mut expected: Vec<String> = ["Building", "Level 1", "Project", "Site"]
        .map(String::from)
        .into();
    expected.extend((0..r.report.transactions).map(|i| format!("Proxy {i}")));
    expected.sort();
    assert_eq!(names(&r.doc), expected);
    if let Some((label, _)) = &r.report.last {
        assert_eq!(label, &format!("Proxy {}", r.report.transactions - 1));
    }
    r.store.close(&mut r.doc).unwrap();
}

#[test]
fn a_second_instance_is_refused_while_the_store_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let path = dir.path().join("shared.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    project.save(&doc).unwrap();
    let store = LiveStore::create(&project, &mut doc, &opts).unwrap();

    let (other, mut other_doc, _) = LocalProject::open(&path).unwrap();
    match LiveStore::create(&other, &mut other_doc, &opts) {
        Err(LiveError::Locked { pid, .. }) => assert_eq!(pid, std::process::id()),
        other => panic!("expected Locked, got {other:?}"),
    }
    store.close(&mut doc).unwrap();
    LiveStore::create(&other, &mut other_doc, &opts)
        .unwrap()
        .close(&mut other_doc)
        .unwrap();
}

#[test]
fn journal_never_holds_credentials_and_mirrors_history() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let (mut project, mut doc) =
        LocalProject::new(Schema::Ifc4, &dir.path().join("p.ifc")).unwrap();
    project.save(&doc).unwrap();
    let store = LiveStore::create(&project, &mut doc, &opts).unwrap();
    create(&mut doc, "A", "IfcWall");
    create(&mut doc, "B", "IfcWall");
    doc.undo(1, &UndoScope::All, &Origin::ui()).unwrap();
    create(&mut doc, "C", "IfcWall");
    store.flush().unwrap();

    let conn = rusqlite::Connection::open(store.path()).unwrap();
    let rows: Vec<(String, i64, Option<String>)> = conn
        .prepare("SELECT name, undone, params FROM txn ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // B was undone, then C cleared the redo stack.
    assert_eq!(rows, [("A".into(), 0, None), ("C".into(), 0, None)]);
    let elements: i64 = conn
        .query_row("SELECT count(*) FROM element", [], |r| r.get(0))
        .unwrap();
    assert_eq!(elements, 6, "the four template elements plus A and C");
    drop(conn);
    store.close(&mut doc).unwrap();
}
