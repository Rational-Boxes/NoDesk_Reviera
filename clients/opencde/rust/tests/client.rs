// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

use reviera_opencde::fake::Fake;
use reviera_opencde::{Auth, Client, ErrorKind, OPENCONSTRUCTIONERP_LOGIN};
use serde_json::json;

fn login(user: &str, password: &str) -> Auth {
    Auth::Login { url: OPENCONSTRUCTIONERP_LOGIN.into(), user: user.into(), password: password.into() }
}

#[test]
fn found_from_the_servers_root_and_used_both_ways() {
    let fake = Fake::start("ann@acme.test", "pw");
    // Wrong password: said so, not "no BCF API here".
    let e = Client::connect(&fake.url, login("ann@acme.test", "nope"), false).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Auth, "{e}");

    let c = Client::connect(&fake.url, login("ann@acme.test", "pw"), false).unwrap();
    assert!(c.base_url().ends_with("/api/v1/bcf/"), "{}", c.base_url());
    assert_eq!(c.version(), "3.0");
    let projects = c.projects().unwrap();
    assert_eq!(projects[0]["name"], "Hall");
    let p = Fake::PROJECT;

    // Someone raised one in the web pages.
    fake.add_topic(
        json!({ "guid": "aaaaaaaa-0000-4000-8000-000000000001", "title": "Duct through beam", "topic_status": "Open", "creation_author": "bob@acme.test",
                "comments": [{ "guid": "c1", "comment": "Please reroute", "author": "bob@acme.test", "date": "2026-11-01T09:00:00Z" }],
                "viewpoints": [{ "guid": "v1", "clipping_planes": [] }] }),
        Some(b"\x89PNG"),
    );
    let topics = c.topics(p).unwrap();
    assert_eq!(topics.len(), 1);
    assert!(topics[0].get("comments").is_none(), "comments are their own list");
    assert_eq!(c.comments(p, "aaaaaaaa-0000-4000-8000-000000000001").unwrap()[0]["comment"], "Please reroute");
    assert_eq!(c.snapshot(p, "aaaaaaaa-0000-4000-8000-000000000001", "v1").unwrap().as_deref(), Some(&b"\x89PNG"[..]));
    assert_eq!(c.snapshot(p, "aaaaaaaa-0000-4000-8000-000000000001", "nope").unwrap(), None);

    // Raised here, with our GUID kept; then changed with only the field given.
    let t = c.create_topic(p, &json!({ "guid": "bbbbbbbb-0000-4000-8000-000000000002", "title": "Fire rating", "topic_type": "Issue", "priority": "High" })).unwrap();
    assert_eq!(t["guid"], "bbbbbbbb-0000-4000-8000-000000000002");
    c.add_comment(p, "bbbbbbbb-0000-4000-8000-000000000002", &json!({ "comment": "Check it" })).unwrap();
    c.add_viewpoint(p, "bbbbbbbb-0000-4000-8000-000000000002", &json!({ "guid": "v2", "snapshot": { "snapshot_type": "png", "snapshot_data": "iVBORw==" } })).unwrap();
    let t = c.update_topic(p, "bbbbbbbb-0000-4000-8000-000000000002", &json!({ "topic_status": "Closed" })).unwrap();
    assert_eq!((t["topic_status"].as_str(), t["priority"].as_str(), t["title"].as_str()), (Some("Closed"), Some("High"), Some("Fire rating")));
    assert_eq!(c.snapshot(p, "bbbbbbbb-0000-4000-8000-000000000002", "v2").unwrap().as_deref(), Some(&[0x89, b'P', b'N', b'G'][..]));

    // A token that stops working is asked for again.
    fake.revoke_tokens();
    assert_eq!(c.topics(p).unwrap().len(), 2);

    // The standard's bare arrays read the same.
    fake.bare_lists();
    assert_eq!(c.topics(p).unwrap().len(), 2);
    c.delete_topic(p, "bbbbbbbb-0000-4000-8000-000000000002").unwrap();
    assert_eq!(fake.topics().len(), 1);
}

#[test]
fn a_versioned_base_is_taken_as_given() {
    let fake = Fake::start("ann@acme.test", "pw");
    let c = Client::connect(&format!("{}api/v1/bcf/3.0", fake.url), login("ann@acme.test", "pw"), false).unwrap();
    assert_eq!(c.version(), "3.0");
    let c = Client::new(&format!("{}api/v1/bcf", fake.url), "3.0", login("ann@acme.test", "pw"), false).unwrap();
    assert_eq!(c.current_user().unwrap()["id"], "ann@acme.test");
}
