// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Against a real FileEngine, when one is named: `REVIERA_FILEENGINE_URL` (the tenant's address)
//! with `REVIERA_FILEENGINE_KEY` and `REVIERA_FILEENGINE_SECRET` (a service credential). Skipped
//! otherwise. Writes only inside `REVIERA_FILEENGINE_FOLDER` (default `Engineering/Reviera test`,
//! which must exist), in a folder of its own that it deletes at the end.

use reviera_fileengine::{Client, ErrorKind};

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

#[test]
fn a_real_server_signs_in_writes_versions_and_restores() {
    let (Some(url), Some(key), Some(secret)) = (var("REVIERA_FILEENGINE_URL"), var("REVIERA_FILEENGINE_KEY"), var("REVIERA_FILEENGINE_SECRET")) else {
        eprintln!("REVIERA_FILEENGINE_URL, _KEY and _SECRET are not all set: skipped");
        return;
    };
    let c = Client::new(&url, false).unwrap();
    assert_eq!(c.sign_in(&key, "not the secret").unwrap_err().kind, ErrorKind::Auth);
    let who = c.sign_in(&key, &secret).unwrap();
    assert!(!who.user.is_empty() && !who.tenant.is_empty(), "{who:?}");
    let folder = var("REVIERA_FILEENGINE_FOLDER").unwrap_or_else(|| "Engineering/Reviera test".to_owned());
    let base = c.resolve(&folder).unwrap();
    assert!(base.is_dir() && c.can(&base.uid, "w").unwrap(), "{folder} must be a folder we may write in");

    let run = format!("client {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let dir = c.create_dir(&base.uid, &run).unwrap();
    let f = c.create_file(&dir, "probe.ifc").unwrap();
    let v1 = c.save_checked(&f, b"ISO-10303-21;\nEND-ISO-10303-21;\n", "").unwrap();
    // Version stamps have millisecond resolution.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let v2 = c.save_checked(&f, b"ISO-10303-21;\n/* two */\nEND-ISO-10303-21;\n", &v1).unwrap();
    assert!(v2 > v1, "{v1} {v2}");
    assert_eq!(c.save_checked(&f, b"stale", &v1).unwrap_err().kind, ErrorKind::Conflict);
    assert_eq!(c.read_range(&f, 14, 9).unwrap(), b"/* two */");
    let versions = c.versions(&f).unwrap();
    assert_eq!(versions.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), [v2.as_str(), v1.as_str()]);
    assert_eq!(versions[0].revised_by, who.user);
    assert_eq!(c.read_version(&f, &v1).unwrap(), b"ISO-10303-21;\nEND-ISO-10303-21;\n");
    c.restore(&f, &v1).unwrap();
    assert_eq!(c.versions(&f).unwrap().len(), 3);
    c.set_metadata(&f, "ifc.schema", "IFC4").unwrap();
    assert_eq!(c.metadata(&f).unwrap().get("ifc.schema").map(String::as_str), Some("IFC4"));

    c.delete_file(&f).unwrap();
    c.delete_dir(&dir).unwrap();
    assert!(c.find(&base.uid, &run).unwrap().is_none());
    c.sign_out().unwrap();
}
