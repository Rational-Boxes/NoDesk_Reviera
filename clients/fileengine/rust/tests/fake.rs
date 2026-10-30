// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! The client against the in-memory FileEngine: signing in and renewing, folders, files and
//! their versions, the checked save, metadata and permissions.

use reviera_fileengine::fake::Fake;
use reviera_fileengine::{Client, ErrorKind, ROOT};

fn signed_in(fake: &Fake) -> Client {
    let c = Client::new(&fake.url, false).unwrap();
    let who = c.sign_in("fesk_test", "fesks_secret").unwrap();
    assert_eq!((who.user.as_str(), who.tenant.as_str()), ("ann@acme.test", "acme"));
    c
}

#[test]
fn addresses_and_sign_in() {
    assert_eq!(Client::new("http://files.example.org", false).unwrap_err().kind, ErrorKind::Url);
    assert_eq!(Client::new("ftp://files.example.org", false).unwrap_err().kind, ErrorKind::Url);
    assert_eq!(Client::new("https://acme.proximafilevault.com", false).unwrap().url(), "https://acme.proximafilevault.com/api/");
    assert_eq!(Client::new("https://files.example.org/fe/api/", false).unwrap().url(), "https://files.example.org/fe/api/");

    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let c = Client::new(&fake.url, false).unwrap();
    assert_eq!(c.whoami().unwrap_err().kind, ErrorKind::Auth, "not signed in");
    assert_eq!(c.sign_in("fesk_test", "wrong").unwrap_err().kind, ErrorKind::Auth);
    assert!(!c.signed_in());
    let c = signed_in(&fake);
    assert_eq!(c.tenants().unwrap().current, "acme");
    c.sign_out().unwrap();
    assert_eq!(c.whoami().unwrap_err().kind, ErrorKind::Auth, "signed out");
}

#[test]
fn an_expired_or_revoked_token_is_replaced() {
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    fake.set_token_life(2);
    let c = signed_in(&fake);
    // Due for renewal at half its life: renewed, not re-issued.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fake.take_log();
    c.whoami().unwrap();
    assert_eq!(fake.take_log(), ["POST /auth/refresh", "GET /whoami"]);
    // Revoked on the server: refused once, a new token from the credential, the call again.
    fake.set_token_life(900);
    c.whoami().unwrap();
    fake.revoke_tokens();
    fake.take_log();
    assert_eq!(c.whoami().unwrap().user, "ann@acme.test");
    assert_eq!(fake.take_log(), ["GET /whoami", "POST /auth/token", "GET /whoami"]);
}

#[test]
fn folders_files_and_versions() {
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let c = signed_in(&fake);
    let eng = c.resolve("Engineering").unwrap();
    assert!(eng.is_dir());
    let test = c.ensure_dir(&eng.uid, "Reviera test").unwrap();
    assert_eq!(c.ensure_dir(&eng.uid, "Reviera test").unwrap(), test, "found, not made again");
    assert_eq!(c.resolve("Engineering/Reviera test").unwrap().uid, test);
    assert_eq!(c.resolve("Engineering/Nowhere").unwrap_err().kind, ErrorKind::NotFound);

    let f = c.create_file(&test, "tower.ifc").unwrap();
    assert_eq!(c.create_file(&test, "tower.ifc").unwrap_err().kind, ErrorKind::Server, "the name is taken");
    assert_eq!(c.stat(&f).unwrap().version, "", "no version before the first write");
    let v1 = c.save_checked(&f, b"ISO-10303-21; one", "").unwrap();
    let v2 = c.save_checked(&f, b"ISO-10303-21; two two", &v1).unwrap();
    assert!(v2 > v1, "{v1} {v2}");
    assert_eq!(c.read(&f).unwrap(), b"ISO-10303-21; two two");
    assert_eq!(c.read_range(&f, 14, 3).unwrap(), b"two");
    let mut streamed = Vec::new();
    assert_eq!(c.read_to(&f, &mut streamed).unwrap(), 21);

    // Someone else saves: the save from the old version is refused, naming them.
    fake.write_as(&f, b"ISO-10303-21; bob's", "bob@acme.test");
    let e = c.save_checked(&f, b"mine", &v2).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Conflict);
    assert!(e.message.contains("bob@acme.test"), "{e}");
    let versions = c.versions(&f).unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!((versions[0].revised_by.as_str(), versions[2].version.as_str()), ("bob@acme.test", v1.as_str()));
    assert_eq!(c.read_version(&f, &v1).unwrap(), b"ISO-10303-21; one");
    // A restore is a new version with the old content.
    assert_eq!(c.restore(&f, &v1).unwrap(), v1);
    assert_eq!((c.versions(&f).unwrap().len(), c.read(&f).unwrap()), (4, b"ISO-10303-21; one".to_vec()));

    // From a file on disk.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.ifc");
    std::fs::write(&path, vec![b'x'; 300_000]).unwrap();
    c.write_file(&f, &path).unwrap();
    assert_eq!(c.stat(&f).unwrap().size, 300_000);

    c.set_metadata(&f, "ifc.schema", "IFC4").unwrap();
    c.set_metadata(&f, "reviera.comment", "Level 3 slab").unwrap();
    assert_eq!(c.metadata(&f).unwrap().get("ifc.schema").map(String::as_str), Some("IFC4"));
    c.delete_metadata(&f, "reviera.comment").unwrap();
    assert_eq!(fake.metadata(&f).len(), 1);

    c.rename(&f, "tower-a.ifc").unwrap();
    let sub = c.create_dir(&test, "Archive").unwrap();
    c.move_to(&f, &sub).unwrap();
    assert_eq!(c.resolve("Engineering/Reviera test/Archive/tower-a.ifc").unwrap().uid, f);
    assert_eq!(c.move_to(&test, &sub).unwrap_err().kind, ErrorKind::Conflict, "into itself");
    c.delete_file(&f).unwrap();
    assert!(!c.exists(&f).unwrap());
    assert!(c.list(&sub, false).unwrap().is_empty());
    assert!(c.list(&sub, true).unwrap()[0].deleted);
    c.undelete(&f).unwrap();
    assert!(c.exists(&f).unwrap());
    assert_eq!(c.stat("no-such-uid").unwrap_err().kind, ErrorKind::NotFound);
    assert!(c.list(ROOT, false).unwrap().iter().any(|n| n.name == "Engineering"));
}

#[test]
fn a_read_only_folder_refuses_writes() {
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let c = signed_in(&fake);
    let eng = fake.uid_of("Engineering").unwrap();
    let f = c.create_file(&eng, "a.ifc").unwrap();
    assert!(c.can(&f, "w").unwrap());
    fake.make_read_only(&eng);
    assert!(!c.can(&f, "w").unwrap());
    assert!(c.can(&f, "r").unwrap());
    let e = c.write(&f, b"x").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(e.message.contains("permission denied"), "{e}");
    assert_eq!(c.create_dir(&eng, "B").unwrap_err().kind, ErrorKind::Forbidden);
}
