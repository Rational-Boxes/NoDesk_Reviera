// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BIMserver client mode (COL-BIM-001…004, COL-MODE-001/002): profiles and login, the project
//! and revision lists, publishing a Local model, opening it from the server as a working copy,
//! and checking edits in. Against a small fake BIMserver here; against a real one in
//! `a_real_server_round_trip` when `REVIERA_BIMSERVER_URL` and credentials are set.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use base64::Engine;
use reviera_app::bimserver::Servers;
use reviera_app::{App, AppConfig};
use reviera_core::{Caller, Origin};
use serde_json::{Value, json};

/// A revision: roid, comment, the file.
type Rev = (i64, String, Vec<u8>);

#[derive(Default)]
struct Fake {
    /// poid → (name, schema, revisions).
    projects: HashMap<i64, (String, String, Vec<Rev>)>,
    next: i64,
    /// Topic → bytes prepared for download.
    topics: HashMap<i64, Vec<u8>>,
    tokens: Vec<String>,
    /// Refuse the next call's token once (as a restarted server does).
    expire_once: bool,
    calls: Vec<String>,
}

fn ok(v: Value) -> Value {
    json!({ "response": { "result": v } })
}

fn exception(ty: &str, m: &str) -> Value {
    json!({ "response": { "exception": { "__type": ty, "message": m } } })
}

fn answer(f: &mut Fake, body: &Value) -> Value {
    let r = &body["request"];
    let (m, p) = (r["method"].as_str().unwrap_or_default().to_owned(), &r["parameters"]);
    f.calls.push(m.clone());
    let token = body["token"].as_str().map(str::to_owned);
    let project = |f: &Fake, poid: i64| {
        let (name, schema, revs) = &f.projects[&poid];
        json!({ "oid": poid, "name": name, "schema": schema, "parentId": -1, "subProjects": [], "lastRevisionId": revs.last().map_or(-1, |r| r.0), "revisions": revs.iter().map(|r| r.0).collect::<Vec<_>>() })
    };
    match m.as_str() {
        "getServerInfo" => return ok(json!({ "serverState": "RUNNING", "version": { "fullString": "1.6.1-fake" } })),
        "login" if p["username"] == "anna@example.org" && p["password"] == "right" => {
            f.next += 1;
            let t = format!("tok{}", f.next);
            f.tokens.push(t.clone());
            return ok(json!(t));
        }
        "login" => return exception("UserException", "Invalid username/password combination"),
        "loginUserToken" if p["token"] == "USERTOKEN" => {
            f.next += 1;
            let t = format!("tok{}", f.next);
            f.tokens.push(t.clone());
            return ok(json!(t));
        }
        "loginUserToken" => return exception("UserException", "Invalid token"),
        _ => {}
    }
    if !token.as_ref().is_some_and(|t| f.tokens.contains(t)) {
        return exception("AuthenticationException", "Invalid token");
    }
    if f.expire_once {
        f.expire_once = false;
        f.tokens.clear();
        return exception("AuthenticationException", "Invalid token");
    }
    match m.as_str() {
        "getLoggedInUser" => ok(json!({ "oid": 7, "username": "anna@example.org", "name": "Anna", "userType": "USER" })),
        "getUserByUoid" => ok(json!({ "oid": 7, "username": "anna@example.org", "name": "Anna" })),
        "logout" => {
            f.tokens.retain(|t| Some(t) != token.as_ref());
            ok(Value::Null)
        }
        "getAllProjects" => {
            let mut ids: Vec<i64> = f.projects.keys().copied().collect();
            ids.sort();
            ok(Value::Array(ids.into_iter().map(|i| project(f, i)).collect()))
        }
        "getProjectByPoid" => ok(project(f, p["poid"].as_i64().unwrap())),
        "addProject" => {
            f.next += 1;
            let poid = 1000 + f.next;
            f.projects.insert(poid, (p["projectName"].as_str().unwrap().to_owned(), p["schema"].as_str().unwrap().to_owned(), vec![]));
            ok(project(f, poid))
        }
        "getAllRevisionsOfProject" => {
            let revs = &f.projects[&p["poid"].as_i64().unwrap()].2;
            ok(Value::Array(revs.iter().enumerate().map(|(i, r)| json!({ "oid": r.0, "id": i + 1, "comment": r.1, "date": 1_700_000_000_000i64, "userId": 7, "size": r.2.len() })).collect()))
        }
        "getSuggestedDeserializerForExtension" => ok(json!({ "oid": 11, "name": "Ifc4 (Streaming)" })),
        "getSerializerByName" => ok(json!({ "oid": 12, "name": p["serializerName"] })),
        "checkinSync" => {
            let data = base64::engine::general_purpose::STANDARD.decode(p["data"].as_str().unwrap()).unwrap();
            assert_eq!(p["merge"], false);
            assert_eq!(p["fileSize"].as_u64().unwrap() as usize, data.len());
            f.next += 1;
            let roid = 60_000 + f.next;
            f.projects.get_mut(&p["poid"].as_i64().unwrap()).unwrap().2.push((roid, p["comment"].as_str().unwrap().to_owned(), data));
            ok(json!({ "__type": "SLongCheckinActionState", "state": "FINISHED", "roid": roid, "errors": [], "warnings": [] }))
        }
        "download" => {
            let roid = p["roids"][0].as_i64().unwrap();
            let data = f.projects.values().flat_map(|x| &x.2).find(|r| r.0 == roid).unwrap().2.clone();
            f.next += 1;
            f.topics.insert(f.next, data);
            ok(json!(f.next))
        }
        "cleanupLongAction" => {
            f.topics.remove(&p["topicId"].as_i64().unwrap());
            ok(json!({}))
        }
        other => exception("ServerException", &format!("no method {other}")),
    }
}

/// A fake BIMserver on localhost: its URL and state.
fn server() -> (String, Arc<Mutex<Fake>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://localhost:{}/", listener.local_addr().unwrap().port());
    let st = Arc::new(Mutex::new(Fake { projects: HashMap::from([(131073, ("Hospital".to_owned(), "ifc4".to_owned(), vec![]))]), ..Fake::default() }));
    let s2 = st.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            if r.read_line(&mut first).is_err() {
                continue;
            }
            let mut len = 0;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let (ctype, out): (&str, Vec<u8>) = if first.starts_with("GET /download") {
                let topic: i64 = first.split("topicId=").nth(1).and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next()).and_then(|t| t.parse().ok()).unwrap_or(-1);
                ("application/octet-stream", s2.lock().unwrap().topics.get(&topic).cloned().unwrap_or_default())
            } else {
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                ("application/json", serde_json::to_vec(&answer(&mut s2.lock().unwrap(), &v)).unwrap())
            };
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len());
            let _ = stream.write_all(&out);
        }
    });
    (url, st)
}

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.bimserver_mut() = Servers::isolated(dir.join("bimserver.json"), dir.join("copies"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

/// The active document's elements of `class`, and how many of them have their Reviera recipe.
fn count(app: &App, class: &str) -> (usize, usize) {
    let store = app.document(app.active().unwrap()).unwrap().doc.store();
    let all: Vec<_> = store.iter().filter(|e| e.class == class).collect();
    (all.len(), all.iter().filter(|e| e.param(reviera_core::walls::RECIPE_KEY).is_some()).count())
}

#[test]
fn publish_open_and_check_in_through_a_server() {
    let dir = tempfile::tempdir().unwrap();
    let (url, fake) = server();
    let mut app = new_app(dir.path());
    // Plain http to another computer is refused; an unreachable server is not saved.
    assert_eq!(run(&mut app, "bimserver.add_server", json!({ "name": "Office", "url": "http://bim.example.org" })).unwrap_err().code, "bimserver.bad_url");
    assert_eq!(run(&mut app, "bimserver.add_server", json!({ "name": "Gone", "url": "http://localhost:9" })).unwrap_err().code, "bimserver.unreachable");
    let s = run(&mut app, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap();
    assert_eq!(s["version"], "1.6.1-fake");
    let id = s["id"].as_str().unwrap().to_owned();
    assert_eq!(run(&mut app, "bimserver.add_server", json!({ "name": "Again", "url": url })).unwrap_err().code, "bimserver.duplicate");

    // Not logged in, nothing stored: a call says so.
    assert_eq!(run(&mut app, "bimserver.list_projects", json!({ "server_id": id })).unwrap_err().code, "bimserver.not_logged_in");
    assert_eq!(run(&mut app, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "wrong" })).unwrap_err().code, "bimserver.not_authorised");
    // An agent may not hand over a password.
    let mcp = Caller { origin: Origin::mcp("probe", "s1"), ..Caller::user() };
    let e = app.execute("bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" }), &mcp).unwrap_err();
    assert_eq!(e.code, "permission.denied");
    let who = run(&mut app, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" })).unwrap();
    assert_eq!((who["name"].as_str(), who["secret"].as_str()), (Some("Anna"), Some("session")));
    // The settings file has the profile and user name, never the password.
    let settings = std::fs::read_to_string(dir.path().join("bimserver.json")).unwrap();
    assert!(settings.contains("anna@example.org") && !settings.contains("right"), "{settings}");
    // With stored credentials an agent can log in again.
    app.execute("bimserver.login", json!({ "server_id": id }), &mcp).unwrap();

    let projects = run(&mut app, "bimserver.list_projects", json!({ "server_id": id })).unwrap();
    assert_eq!((projects[0]["name"].as_str(), projects[0]["revisions"].as_u64(), projects[0]["working_copy"].as_bool()), (Some("Hospital"), Some(0), Some(false)));
    assert_eq!(run(&mut app, "bimserver.open", json!({ "server_id": id, "poid": 131073 })).unwrap_err().code, "bimserver.empty_project");

    // A Local model: a wall, published as the project's first revision.
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    run(&mut app, "file.save_as", json!({ "path": dir.path().join("local.ifc") })).unwrap();
    assert_eq!(run(&mut app, "model.mode", json!({})).unwrap()["mode"], "local");
    assert_eq!(run(&mut app, "bimserver.check_in", json!({ "comment": "x" })).unwrap_err().code, "mode.not_available");
    assert_eq!(run(&mut app, "bimserver.publish_local", json!({ "server_id": id, "poid": 131073, "comment": " " })).unwrap_err().code, "bimserver.no_comment");
    let published = run(&mut app, "bimserver.publish_local", json!({ "server_id": id, "poid": 131073, "comment": "First issue" })).unwrap();
    let first = published["roid"].as_i64().unwrap();
    assert_eq!(published["revision"], 1);
    let mode = run(&mut app, "model.mode", json!({})).unwrap();
    assert_eq!((mode["mode"].as_str(), mode["working_copy"]["base_roid"].as_i64(), mode["connected"].as_bool()), (Some("bimserver"), Some(first), Some(true)));
    let comment = fake.lock().unwrap().projects[&131073].2[0].1.clone();
    assert!(comment.starts_with("First issue\n[reviera] BIM Reviera "), "{comment}");
    // Publishing again over its revisions needs `replace`; it is a working copy now anyway.
    assert_eq!(run(&mut app, "bimserver.publish_local", json!({ "server_id": id, "poid": 131073, "comment": "again" })).unwrap_err().code, "bimserver.already_published");

    // A second user's computer opens it from the server: the wall arrives with its recipe.
    let dir2 = tempfile::tempdir().unwrap();
    let mut other = new_app(dir2.path());
    let id2 = run(&mut other, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut other, "bimserver.login", json!({ "server_id": id2, "user_token": "USERTOKEN" })).unwrap();
    let opened = run(&mut other, "bimserver.open", json!({ "server_id": id2, "poid": 131073 })).unwrap();
    assert_eq!((opened["existing"].as_bool(), opened["base_roid"].as_i64()), (Some(false), Some(first)));
    assert_eq!(count(&other, "IfcWall"), (1, 1), "the wall and its recipe");
    let revs = run(&mut other, "bimserver.list_revisions", json!({ "server_id": id2, "poid": 131073 })).unwrap();
    assert_eq!((revs[0]["user"].as_str(), revs[0]["number"].as_i64()), (Some("Anna"), Some(1)));

    // They add a wall and check it in; the first user's working copy is now behind.
    run(&mut other, "wall.create", json!({ "curve": { "line": { "start": [6, 0], "end": [6, 4] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    assert_eq!(run(&mut other, "bimserver.check_in", json!({ "comment": "" })).unwrap_err().code, "bimserver.no_comment");
    // The session lapsed meanwhile: logged in again with the stored token, once.
    fake.lock().unwrap().expire_once = true;
    let second = run(&mut other, "bimserver.check_in", json!({ "comment": "East wall" })).unwrap();
    assert_eq!(second["revision"], 2);
    let st: Value = serde_json::from_slice(&std::fs::read(dir2.path().join("copies").join(&id2).join("131073").join("state.json")).unwrap()).unwrap();
    assert_eq!(st["base_roid"], second["roid"]);
    // A check-in from the first user would overwrite it: refused.
    assert_eq!(run(&mut app, "bimserver.check_in", json!({ "comment": "mine" })).unwrap_err().code, "bimserver.out_of_date");
    // Opening the project again opens the working copy, never overwriting it, and says the
    // server has moved on.
    run(&mut app, "file.close", json!({ "document_id": published["document_id"], "save": "yes" })).unwrap();
    let again = run(&mut app, "bimserver.open", json!({ "server_id": id, "poid": 131073 })).unwrap();
    assert_eq!((again["existing"].as_bool(), again["newer_on_server"].as_bool()), (Some(true), Some(true)));
    // A revision downloaded as a file.
    let n = run(&mut app, "bimserver.download_revision", json!({ "server_id": id, "roid": second["roid"], "path": dir.path().join("r2.ifc") })).unwrap();
    assert!(n.as_u64().unwrap() > 1000);
    assert!(std::fs::read_to_string(dir.path().join("r2.ifc")).unwrap().starts_with("ISO-10303-21;"));

    // Removing the profile is not for agents; logging out ends the session.
    assert_eq!(app.execute("bimserver.remove_server", json!({ "server_id": id }), &mcp).unwrap_err().code, "permission.denied");
    run(&mut app, "bimserver.logout", json!({ "server_id": id, "forget": true })).unwrap();
    assert_eq!(run(&mut app, "bimserver.list_servers", json!({})).unwrap()[0]["secret"], "none");
    assert_eq!(run(&mut app, "bimserver.list_projects", json!({ "server_id": id })).unwrap_err().code, "bimserver.not_logged_in");
    run(&mut app, "bimserver.remove_server", json!({ "server_id": id })).unwrap();
    assert_eq!(run(&mut app, "bimserver.list_servers", json!({})).unwrap().as_array().map(Vec::len), Some(0));
    assert!(fake.lock().unwrap().calls.iter().any(|c| c == "cleanupLongAction"));
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

/// Against a real server: `REVIERA_BIMSERVER_URL`, `REVIERA_BIMSERVER_USER` and
/// `REVIERA_BIMSERVER_PASSWORD` (or `REVIERA_BIMSERVER_TOKEN`), the project
/// `REVIERA_BIMSERVER_POID` (else the first), and `REVIERA_BIMSERVER_CHECKIN=1` to publish over
/// it and check an edit in. Skipped otherwise.
#[test]
fn a_real_server_round_trip() {
    let Some(url) = var("REVIERA_BIMSERVER_URL") else {
        eprintln!("REVIERA_BIMSERVER_URL is not set: skipped");
        return;
    };
    let login = |app: &mut App, id: &str| match (var("REVIERA_BIMSERVER_USER"), var("REVIERA_BIMSERVER_PASSWORD"), var("REVIERA_BIMSERVER_TOKEN")) {
        (Some(u), Some(p), _) => run(app, "bimserver.login", json!({ "server_id": id, "username": u, "password": p })),
        (_, _, Some(t)) => run(app, "bimserver.login", json!({ "server_id": id, "user_token": t })),
        _ => panic!("set REVIERA_BIMSERVER_USER and _PASSWORD, or _TOKEN"),
    };
    let dir = tempfile::tempdir().unwrap();
    let mut a = new_app(dir.path());
    let id = run(&mut a, "bimserver.add_server", json!({ "name": "Live", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    login(&mut a, &id).unwrap();
    let projects = run(&mut a, "bimserver.list_projects", json!({ "server_id": id })).unwrap();
    let poid = var("REVIERA_BIMSERVER_POID").and_then(|p| p.parse::<i64>().ok()).unwrap_or_else(|| projects[0]["poid"].as_i64().unwrap());
    if var("REVIERA_BIMSERVER_CHECKIN").as_deref() != Some("1") {
        eprintln!("REVIERA_BIMSERVER_CHECKIN is not 1: listed only");
        return;
    }
    // Publish a sample over the project's revisions, as a new one.
    let sample = dir.path().join("energy_house.ifc");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data/samples/energy_house.ifc"), &sample).unwrap();
    run(&mut a, "file.open", json!({ "path": sample })).unwrap();
    let (walls, windows) = (count(&a, "IfcWall"), count(&a, "IfcWindow"));
    assert!(walls.0 > 0);
    let published = run(&mut a, "bimserver.publish_local", json!({ "server_id": id, "poid": poid, "comment": "Reviera live test: energy house", "replace": true })).unwrap();
    // Another computer opens it: the same walls and windows, rebuilt from their recipes.
    let dir2 = tempfile::tempdir().unwrap();
    let mut b = new_app(dir2.path());
    let id2 = run(&mut b, "bimserver.add_server", json!({ "name": "Live", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    login(&mut b, &id2).unwrap();
    let opened = run(&mut b, "bimserver.open", json!({ "server_id": id2, "poid": poid })).unwrap();
    assert_eq!(opened["base_roid"], published["roid"]);
    assert_eq!((count(&b, "IfcWall"), count(&b, "IfcWindow")), (walls, windows), "the same elements, with their recipes");
    // An edit checked in from there.
    let level = run(&mut b, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(&mut b, "wall.create", json!({ "curve": { "line": { "start": [30, 30], "end": [34, 30] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let r = run(&mut b, "bimserver.check_in", json!({ "comment": "Reviera live test: a wall" })).unwrap();
    assert!(r["revision"].as_i64().unwrap() > published["revision"].as_i64().unwrap());
    // The first computer is behind now.
    assert_eq!(run(&mut a, "bimserver.check_in", json!({ "comment": "x" })).unwrap_err().code, "bimserver.out_of_date");
}
