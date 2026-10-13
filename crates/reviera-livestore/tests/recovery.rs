// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Crash recovery acceptance tests (Ch 2b §11). Each scenario runs in a child process (this test
//! binary re-run with `REVIERA_LIVE_CHILD` set) that aborts after editing, as a real crash would.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use reviera_core::{Document, GroupEnd, NewElement, Origin, UndoScope};
use reviera_ifc::Schema;
use reviera_livestore::{Encryption, KeySource, LiveError, LiveStore, Options, RestoreSource};
use reviera_store::LocalProject;

/// Tests encrypt with a passphrase, so they exercise SQLCipher without touching the user's keyring.
fn options(dir: &Path) -> Options {
    Options {
        dir: dir.join("live"),
        group_commit: Duration::from_millis(50),
        encryption: Encryption::Passphrase("test passphrase".into()),
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
    crash_with(scenario, dir, false)
}

fn crash_with(scenario: &str, dir: &Path, keyring: bool) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    if keyring {
        cmd.env("REVIERA_LIVE_KEYRING", "1");
    }
    let status = cmd
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
    let mut opts = options(&dir);
    if std::env::var("REVIERA_LIVE_KEYRING").is_ok() {
        opts.encryption = Encryption::OsKeyring;
    }
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

    let found = reviera_livestore::scan(&opts).unwrap();
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
        reviera_livestore::scan(&opts).unwrap().is_empty(),
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
    let found = reviera_livestore::scan(&opts).unwrap();
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
    let key = reviera_livestore::scan(&opts).unwrap()[0].key.clone();

    // "Truncate the .rvlive file to half its size."
    let db = opts.dir.join(format!("{key}.rvlive"));
    let len = std::fs::metadata(&db).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&db)
        .unwrap()
        .set_len(len / 2)
        .unwrap();

    let found = reviera_livestore::scan(&opts).unwrap();
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
    // Unencrypted, so the test can inspect the tables directly.
    let opts = Options {
        encryption: Encryption::Off,
        ..options(dir.path())
    };
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

#[test]
fn stores_are_encrypted_and_unreadable_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let (mut project, mut doc) =
        LocalProject::new(Schema::Ifc4, &dir.path().join("secret.ifc")).unwrap();
    project.save(&doc).unwrap();
    let store = LiveStore::create(&project, &mut doc, &opts).unwrap();
    create(&mut doc, "Hidden", "IfcWall");
    store.flush().unwrap();
    let status = store.encryption_status();
    assert!(status.enabled);
    assert_eq!(status.key_source, KeySource::Passphrase);

    // "Open the .rvlive file with plain sqlite3: it fails with 'file is not a database'."
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    let err = conn
        .query_row("SELECT count(*) FROM txn", [], |r| r.get::<_, i64>(0))
        .unwrap_err();
    assert!(err.to_string().contains("file is not a database"), "{err}");
    let raw = std::fs::read(store.path()).unwrap();
    assert!(
        !raw.windows(6).any(|w| w == b"Hidden"),
        "no plaintext on disk"
    );
    store.close(&mut doc).unwrap();

    // Policy can require encryption.
    let off = Options {
        encryption: Encryption::Off,
        require_encryption: true,
        ..opts
    };
    assert!(matches!(
        LiveStore::create(&project, &mut doc, &off),
        Err(LiveError::Key(_))
    ));
}

/// Removes every keyring entry named by a key file under `dir` (the test's own stores only).
fn forget_keys(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            forget_keys(&path);
        } else if path.to_string_lossy().ends_with(".keyid")
            && let Ok(id) = std::fs::read_to_string(&path)
            && let Ok(entry) = keyring::Entry::new("org.nodeskreviera.live", id.trim())
        {
            let _ = entry.delete_credential();
        }
    }
}

#[test]
fn keyring_encrypted_store_recovers_and_a_lost_key_is_reported() {
    if keyring::Entry::store_status().is_err() {
        eprintln!("skipped: no OS credential store");
        return;
    }
    let mut opts = options(Path::new("."));
    opts.encryption = Encryption::OsKeyring;

    // The key survives the crash, so restore needs no prompt.
    let first = tempfile::tempdir().unwrap();
    crash_with("wall", first.path(), true);
    let o1 = Options {
        dir: first.path().join("live"),
        ..opts.clone()
    };
    let found = reviera_livestore::scan(&o1).unwrap();
    assert!(found[0].readable);
    let mut r = reviera_livestore::restore(&found[0].key, None, &o1).unwrap();
    assert_eq!(
        names(&r.doc),
        ["Building", "Level 1", "Project", "Site", "Wall 1"]
    );
    assert_eq!(r.store.encryption_status().key_source, KeySource::OsStore);
    r.store.close(&mut r.doc).unwrap();
    forget_keys(first.path());

    // With the credential store entry deleted, the data cannot be decrypted: say so, allow discard.
    let second = tempfile::tempdir().unwrap();
    crash_with("wall", second.path(), true);
    let o2 = Options {
        dir: second.path().join("live"),
        ..opts
    };
    forget_keys(second.path());
    let found = reviera_livestore::scan(&o2).unwrap();
    assert_eq!(found.len(), 1);
    assert!(!found[0].readable);
    assert!(
        found[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("key is not available")
    );
    let err = reviera_livestore::restore(&found[0].key, None, &o2).unwrap_err();
    assert!(matches!(err, LiveError::Key(_)), "{err}");
    reviera_livestore::discard(&found[0].key, &o2.dir).unwrap();
    assert!(reviera_livestore::scan(&o2).unwrap().is_empty());
    // The saved IFC file is unaffected.
    LocalProject::open(&second.path().join("project.ifc")).unwrap();
}
