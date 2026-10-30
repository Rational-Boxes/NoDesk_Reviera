// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Against a real BIMserver, when one is named: `REVIERA_BIMSERVER_URL`, `REVIERA_BIMSERVER_USER`
//! with `REVIERA_BIMSERVER_PASSWORD`, and/or `REVIERA_BIMSERVER_TOKEN` (a user token). Skipped
//! otherwise. Checking in (a new revision of `REVIERA_BIMSERVER_POID`, else the first project)
//! needs `REVIERA_BIMSERVER_CHECKIN=1` as well.

use reviera_bimserver::{Client, ErrorKind};

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

#[test]
fn a_real_server_lists_checks_in_and_downloads() {
    let Some(url) = var("REVIERA_BIMSERVER_URL") else {
        eprintln!("REVIERA_BIMSERVER_URL is not set: skipped");
        return;
    };
    let mut c = Client::new(&url, false).unwrap();
    let info = c.server_info().unwrap();
    assert_eq!(info.state, "RUNNING", "{info:?}");
    assert!(info.version.starts_with("1."), "{info:?}");
    // A wrong password is an authentication failure.
    if let Some(user) = var("REVIERA_BIMSERVER_USER") {
        assert_eq!(c.login(&user, "not the password").unwrap_err().kind, ErrorKind::Auth);
    }
    let mut logged = false;
    if let (Some(user), Some(pass)) = (var("REVIERA_BIMSERVER_USER"), var("REVIERA_BIMSERVER_PASSWORD")) {
        c.login(&user, &pass).unwrap();
        assert_eq!(c.logged_in_user().unwrap().username, user);
        logged = true;
    }
    if let Some(token) = var("REVIERA_BIMSERVER_TOKEN") {
        c.login_user_token(&token).unwrap();
        assert!(!c.logged_in_user().unwrap().username.is_empty());
        logged = true;
    }
    if !logged {
        eprintln!("no credentials: only the server info was checked");
        return;
    }
    let projects = c.projects().unwrap();
    let poid = var("REVIERA_BIMSERVER_POID").and_then(|p| p.parse().ok()).or_else(|| projects.first().map(|p| p.oid)).expect("a project to use");
    let project = c.project(poid).unwrap();
    let before = c.revisions(poid).unwrap();
    if var("REVIERA_BIMSERVER_CHECKIN").as_deref() == Some("1") {
        let d = c.deserializer_for(poid).unwrap();
        let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../test-data/samples/daylight_studio.ifc")).unwrap();
        let r = c.check_in(poid, "Reviera client live test", "daylight_studio.ifc", &file, d.oid).unwrap();
        let after = c.revisions(poid).unwrap();
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(after.last().unwrap().oid, r.roid);
        assert_eq!(c.project(poid).unwrap().last_revision, Some(r.roid));
    }
    if let Some(last) = c.project(poid).unwrap().last_revision {
        let s = c.ifc_serializer(&project.schema).unwrap();
        let bytes = c.download(last, s.oid).unwrap();
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).to_string();
        assert!(head.starts_with("ISO-10303-21;") && head.to_uppercase().contains("FILE_SCHEMA"), "{head}");
        let user = c.revisions(poid).unwrap().last().unwrap().user;
        assert!(!c.user_name(user).unwrap().is_empty());
    }
    c.logout().unwrap();
    assert_eq!(c.projects().unwrap_err().kind, ErrorKind::Auth);
}
