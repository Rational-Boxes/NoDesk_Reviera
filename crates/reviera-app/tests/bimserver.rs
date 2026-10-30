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
    /// Extended data: schemas by name, files, and items as (oid, roid, schema, file).
    schemas: HashMap<String, i64>,
    files: HashMap<i64, Vec<u8>>,
    extended: Vec<(i64, i64, i64, i64)>,
    /// A foreign client's file, checked in just before Reviera's next check-in.
    intrude: Option<Vec<u8>>,
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
        "getExtendedDataSchemaByName" => match f.schemas.get(p["name"].as_str().unwrap()) {
            Some(id) => ok(json!({ "oid": id, "name": p["name"] })),
            None => exception("UserException", "No ExtendedDataSchema found"),
        },
        "addExtendedDataSchema" => {
            f.next += 1;
            let id = 500 + f.next;
            f.schemas.insert(p["extendedDataSchema"]["name"].as_str().unwrap().to_owned(), id);
            ok(json!(id))
        }
        "uploadFile" => {
            f.next += 1;
            let id = 700 + f.next;
            f.files.insert(id, base64::engine::general_purpose::STANDARD.decode(p["file"]["data"].as_str().unwrap()).unwrap());
            ok(json!(id))
        }
        "addExtendedDataToRevision" => {
            f.next += 1;
            let oid = 900 + f.next;
            f.extended.push((oid, p["roid"].as_i64().unwrap(), p["extendedData"]["schemaId"].as_i64().unwrap(), p["extendedData"]["fileId"].as_i64().unwrap()));
            ok(json!({}))
        }
        "getAllExtendedDataOfRevision" => {
            let roid = p["roid"].as_i64().unwrap();
            ok(Value::Array(f.extended.iter().filter(|x| x.1 == roid).map(|x| json!({ "oid": x.0, "revisionId": x.1, "schemaId": x.2, "fileId": x.3, "title": "ownership", "added": 0, "userId": 7 })).collect()))
        }
        "getFile" => ok(json!({ "data": base64::engine::general_purpose::STANDARD.encode(&f.files[&p["fileId"].as_i64().unwrap()]) })),
        "checkinSync" => {
            if let Some(foreign) = f.intrude.take() {
                f.next += 1;
                let roid = 60_000 + f.next;
                f.projects.get_mut(&p["poid"].as_i64().unwrap()).unwrap().2.push((roid, "Another tool".into(), foreign));
            }
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

/// The start of the wall with GlobalId `guid` in the active document.
fn wall_start(app: &App, guid: &str) -> [f64; 2] {
    let store = app.document(app.active().unwrap()).unwrap().doc.store();
    let id = store.find_by_guid(&reviera_core::GlobalId::parse(guid).unwrap()).expect("the wall is there");
    reviera_core::walls::wall_recipe(store.get(id).unwrap()).unwrap().curve.start
}

fn walls(app: &App) -> usize {
    count(app, "IfcWall").0
}

#[test]
fn two_users_synchronise_and_their_changes_merge() {
    let (url, _fake) = server();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let mut a = new_app(dir_a.path());
    let mut b = new_app(dir_b.path());
    let mut ids = Vec::new();
    for app in [&mut a, &mut b] {
        let id = run(app, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
        run(app, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" })).unwrap();
        ids.push(id);
    }
    // A publishes two walls 10 m apart.
    run(&mut a, "file.new", json!({})).unwrap();
    let level = run(&mut a, "level.list", json!({})).unwrap()[0]["id"].clone();
    let wall = |app: &mut App, y: f64| {
        let id = run(app, "wall.create", json!({ "curve": { "line": { "start": [0, y], "end": [6, y] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
        let store = app.document(app.active().unwrap()).unwrap().doc.store();
        (id.clone(), store.get(reviera_core::ElementId(id.as_u64().unwrap())).unwrap().guid().to_string())
    };
    let (w1, g1) = wall(&mut a, 0.0);
    let (_, g2) = wall(&mut a, 10.0);
    run(&mut a, "file.save_as", json!({ "path": dir_a.path().join("a.ifc") })).unwrap();
    run(&mut a, "bimserver.publish_local", json!({ "server_id": ids[0], "poid": 131073, "comment": "Two walls" })).unwrap();
    assert!(run(&mut a, "bimserver.get_latest", json!({})).unwrap()["up_to_date"].as_bool().unwrap());
    run(&mut b, "bimserver.open", json!({ "server_id": ids[1], "poid": 131073 })).unwrap();

    // A moves the first wall and synchronises; B moves the second and synchronises.
    run(&mut a, "element.move", json!({ "element_ids": [w1], "vector": [0, 1, 0] })).unwrap();
    let r = run(&mut a, "bimserver.sync", json!({ "comment": "Wall 1 north" })).unwrap();
    assert!(r["up_to_date"].as_bool().unwrap() && r["checked_in"]["revision"] == 2, "{r}");
    let store = b.document(b.active().unwrap()).unwrap().doc.store();
    let w2b = store.find_by_guid(&reviera_core::GlobalId::parse(&g2).unwrap()).unwrap();
    run(&mut b, "element.move", json!({ "element_ids": [w2b.0], "vector": [0, 1, 0] })).unwrap();
    // B's check-in alone is refused: the server moved on.
    assert_eq!(run(&mut b, "bimserver.check_in", json!({ "comment": "x" })).unwrap_err().code, "bimserver.out_of_date");
    let r = run(&mut b, "bimserver.sync", json!({ "comment": "Wall 2 north" })).unwrap();
    assert!(!r["up_to_date"].as_bool().unwrap() && r["applied"].as_bool().unwrap() && r["conflicts"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(r["checked_in"]["revision"], 3);
    assert_eq!((wall_start(&b, &g1), wall_start(&b, &g2)), ([0.0, 1.0], [0.0, 11.0]), "B has both moves");
    // A gets B's move, keeping its own.
    let r = run(&mut a, "bimserver.get_latest", json!({})).unwrap();
    assert!(r["applied"].as_bool().unwrap() && r["updated"].as_u64().unwrap() >= 1, "{r}");
    assert_eq!((wall_start(&a, &g1), wall_start(&a, &g2)), ([0.0, 1.0], [0.0, 11.0]), "A has both moves");
    // Get Latest is one undoable step.
    assert_eq!(run(&mut a, "bimserver.get_latest", json!({})).unwrap()["up_to_date"], true);

    // Each adds a wall: both arrive, on the one level.
    wall(&mut a, 20.0);
    run(&mut a, "bimserver.sync", json!({ "comment": "Wall 3" })).unwrap();
    wall(&mut b, 30.0);
    let r = run(&mut b, "bimserver.sync", json!({ "comment": "Wall 4" })).unwrap();
    assert!(r["conflicts"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(walls(&b), 4);
    run(&mut a, "bimserver.get_latest", json!({})).unwrap();
    assert_eq!(walls(&a), 4);

    // Both move the first wall, differently: a conflict, and nothing changes until a side is chosen.
    run(&mut a, "element.move", json!({ "element_ids": [w1], "vector": [2, 0, 0] })).unwrap();
    run(&mut a, "bimserver.sync", json!({ "comment": "Wall 1 east" })).unwrap();
    let store = b.document(b.active().unwrap()).unwrap().doc.store();
    let w1b = store.find_by_guid(&reviera_core::GlobalId::parse(&g1).unwrap()).unwrap();
    run(&mut b, "element.move", json!({ "element_ids": [w1b.0], "vector": [-2, 0, 0] })).unwrap();
    let r = run(&mut b, "bimserver.sync", json!({ "comment": "Wall 1 west" })).unwrap();
    let conflicts = r["conflicts"].as_array().unwrap();
    assert!(!conflicts.is_empty() && !r["applied"].as_bool().unwrap() && r["checked_in"].is_null(), "{r}");
    assert_eq!(conflicts[0]["guid"], g1.as_str());
    assert_eq!(wall_start(&b, &g1), [-2.0, 1.0], "unchanged while the conflict waits");
    let r = run(&mut b, "bimserver.get_latest", json!({ "conflicts": "remote" })).unwrap();
    assert!(r["applied"].as_bool().unwrap(), "{r}");
    assert_eq!(wall_start(&b, &g1), [2.0, 1.0], "A's move won");
}

#[test]
fn three_files_merge_into_a_fourth() {
    let dir = tempfile::tempdir().unwrap();
    let f = |n: &str| dir.path().join(n);
    let mut app = new_app(dir.path());
    run(&mut app, "file.new", json!({})).unwrap();
    let level = run(&mut app, "level.list", json!({})).unwrap()[0]["id"].clone();
    let w = run(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let guid = app.document(app.active().unwrap()).unwrap().doc.store().get(reviera_core::ElementId(w.as_u64().unwrap())).unwrap().guid().to_string();
    run(&mut app, "file.save_as", json!({ "path": f("base.ifc") })).unwrap();
    // Local: the wall moved. Remote: another wall added.
    run(&mut app, "element.move", json!({ "element_ids": [w], "vector": [0, 2, 0] })).unwrap();
    run(&mut app, "file.save_as", json!({ "path": f("local.ifc") })).unwrap();
    run(&mut app, "file.open", json!({ "path": f("base.ifc") })).unwrap();
    run(&mut app, "wall.create", json!({ "curve": { "line": { "start": [0, 9], "end": [6, 9] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    run(&mut app, "file.save_as", json!({ "path": f("remote.ifc") })).unwrap();
    let r = run(&mut app, "merge.three_way", json!({ "base_path": f("base.ifc"), "local_path": f("local.ifc"), "remote_path": f("remote.ifc"), "out_path": f("out.ifc") })).unwrap();
    assert!(r["written"].as_bool().unwrap() && r["conflicts"].as_array().unwrap().is_empty() && r["created"].as_u64().unwrap() >= 1, "{r}");
    run(&mut app, "file.open", json!({ "path": f("out.ifc") })).unwrap();
    assert_eq!(walls(&app), 2);
    assert_eq!(wall_start(&app, &guid), [0.0, 2.0]);
}

#[test]
fn revisions_are_compared_and_restored() {
    let (url, _fake) = server();
    let dir = tempfile::tempdir().unwrap();
    let mut a = new_app(dir.path());
    let id = run(&mut a, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut a, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" })).unwrap();
    run(&mut a, "file.new", json!({})).unwrap();
    let level = run(&mut a, "level.list", json!({})).unwrap()[0]["id"].clone();
    let w = run(&mut a, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    run(&mut a, "file.save_as", json!({ "path": dir.path().join("a.ifc") })).unwrap();
    let r1 = run(&mut a, "bimserver.publish_local", json!({ "server_id": id, "poid": 131073, "comment": "One wall" })).unwrap()["roid"].clone();
    run(&mut a, "wall.create", json!({ "curve": { "line": { "start": [0, 9], "end": [6, 9] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let r2 = run(&mut a, "bimserver.check_in", json!({ "comment": "Two walls" })).unwrap()["roid"].clone();
    let c = run(&mut a, "bimserver.compare", json!({ "roid_a": r1, "roid_b": r2 })).unwrap();
    assert_eq!((c["added"].as_array().unwrap().len(), c["deleted"].as_array().unwrap().len()), (1, 0), "{c}");
    assert_eq!(c["added"][0]["class"], "IfcWall");
    // Local changes not yet checked in: the moved wall.
    run(&mut a, "element.move", json!({ "element_ids": [w], "vector": [0, 1, 0] })).unwrap();
    let c = run(&mut a, "bimserver.compare", json!({})).unwrap();
    assert!(c["modified"].as_array().unwrap().iter().any(|m| m["class"] == "IfcWall"), "{c}");
    // A restore over local changes is refused; undone, the first revision is restored as the third.
    assert_eq!(run(&mut a, "bimserver.restore", json!({ "roid": r1, "reason": "back" })).unwrap_err().code, "bimserver.local_changes");
    run(&mut a, "edit.undo", json!({})).unwrap();
    assert_eq!(run(&mut a, "bimserver.restore", json!({ "roid": r1, "reason": " " })).unwrap_err().code, "bimserver.no_comment");
    let r3 = run(&mut a, "bimserver.restore", json!({ "roid": r1, "reason": "the second wall was a mistake" })).unwrap();
    assert_eq!(r3["revision"], 3);
    assert_eq!(walls(&a), 1);
    let h = run(&mut a, "bimserver.history", json!({})).unwrap();
    let h = h.as_array().unwrap();
    assert_eq!(h.len(), 3);
    assert!(h[0]["label"] == "restore" && h[0]["base"] == true && h[0]["comment"].as_str().unwrap().starts_with("[reviera:restore] Restored revision 1: the second wall was a mistake"), "{:?}", h[0]);
    assert!(run(&mut a, "bimserver.compare", json!({ "roid_a": r1, "roid_b": r3["roid"] })).unwrap()["modified"].as_array().unwrap().is_empty());
}

/// A sync lock written into the fake server's latest revision of project 131073 by another
/// session, expiring `expires` (Unix seconds).
fn foreign_lock(fake: &Arc<Mutex<Fake>>, expires: i64) {
    let mut f = fake.lock().unwrap();
    let schema = *f.schemas.entry("org.reviera.ownership/1.0".into()).or_insert(555);
    let roid = f.projects[&131073].2.last().unwrap().0;
    let doc = json!({ "version": 1, "parentVersion": null, "baseRoid": roid, "entries": [], "requests": [], "syncLock": { "owner": { "userId": 9, "userName": "Ben", "session": "other" }, "acquired": 0, "expires": expires } });
    f.files.insert(4242, serde_json::to_vec(&doc).unwrap());
    f.extended.push((1, roid, schema, 4242));
}

#[test]
fn synchronising_takes_the_lock_and_merges_an_intruder() {
    let (url, fake) = server();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let mut a = new_app(dir_a.path());
    let mut b = new_app(dir_b.path());
    let mut ids = Vec::new();
    for app in [&mut a, &mut b] {
        let id = run(app, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
        run(app, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" })).unwrap();
        ids.push(id);
    }
    run(&mut a, "file.new", json!({})).unwrap();
    let level = run(&mut a, "level.list", json!({})).unwrap()[0]["id"].clone();
    let wall = |app: &mut App, y: f64| run(app, "wall.create", json!({ "curve": { "line": { "start": [0, y], "end": [6, y] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    wall(&mut a, 0.0);
    run(&mut a, "file.save_as", json!({ "path": dir_a.path().join("a.ifc") })).unwrap();
    run(&mut a, "bimserver.publish_local", json!({ "server_id": ids[0], "poid": 131073, "comment": "One wall" })).unwrap();
    run(&mut b, "bimserver.open", json!({ "server_id": ids[1], "poid": 131073 })).unwrap();

    // A synchronises: the lock is taken, and released on the new revision.
    wall(&mut a, 5.0);
    let r = run(&mut a, "bimserver.sync", json!({ "comment": "Second wall" })).unwrap();
    assert_eq!(r["lock"], "taken", "{r}");
    assert_eq!(run(&mut a, "bimserver.lock_status", json!({})).unwrap()["locked"], false);
    assert!(fake.lock().unwrap().schemas.contains_key("org.reviera.ownership/1.0"));

    // Ben holds the lock: B's synchronise waits for him (refused, naming him).
    foreign_lock(&fake, 4_000_000_000);
    let st = run(&mut b, "bimserver.lock_status", json!({})).unwrap();
    assert_eq!((st["locked"].as_bool(), st["holder"].as_str(), st["mine"].as_bool()), (Some(true), Some("Ben"), Some(false)));
    wall(&mut b, 10.0);
    let e = run(&mut b, "bimserver.sync", json!({ "comment": "B's wall" })).unwrap_err();
    assert!(e.code == "bimserver.sync_locked" && e.message.contains("Ben"), "{e:?}");
    // His lock lapses (he crashed): B synchronises.
    foreign_lock(&fake, 1);
    let r = run(&mut b, "bimserver.sync", json!({ "comment": "B's wall" })).unwrap();
    assert!(r["lock"] == "taken" && r["checked_in"]["revision"] == 3, "{r}");
    assert_eq!(walls(&b), 3);

    // Another tool checks in while A synchronises: A merges it in as a corrective revision.
    run(&mut a, "bimserver.get_latest", json!({})).unwrap();
    let foreign = {
        // The other tool's model: B's latest plus a wall of its own.
        wall(&mut b, 20.0);
        run(&mut b, "file.save", json!({})).unwrap();
        let path = run(&mut b, "model.mode", json!({})).unwrap()["path"].as_str().unwrap().to_owned();
        let bytes = std::fs::read(&path).unwrap();
        run(&mut b, "edit.undo", json!({})).unwrap();
        bytes
    };
    fake.lock().unwrap().intrude = Some(foreign);
    wall(&mut a, 30.0);
    let r = run(&mut a, "bimserver.sync", json!({ "comment": "A's wall" })).unwrap();
    assert_eq!(r["intruders"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(r["corrective"]["comment"].is_null() && r["corrective"]["revision"] == 6, "{r}");
    assert_eq!(walls(&a), 5, "A's walls, B's, and the other tool's");
    let last = fake.lock().unwrap().projects[&131073].2.last().unwrap().1.clone();
    assert!(last.starts_with("[reviera:merge] Corrective merge of revision 4"), "{last}");
}

#[test]
fn conflicts_are_decided_one_by_one_and_the_losers_stashed() {
    let (url, _fake) = server();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let mut a = new_app(dir_a.path());
    let mut b = new_app(dir_b.path());
    let mut ids = Vec::new();
    for app in [&mut a, &mut b] {
        let id = run(app, "bimserver.add_server", json!({ "name": "Office", "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
        run(app, "bimserver.login", json!({ "server_id": id, "username": "anna@example.org", "password": "right" })).unwrap();
        ids.push(id);
    }
    run(&mut a, "file.new", json!({})).unwrap();
    let level = run(&mut a, "level.list", json!({})).unwrap()[0]["id"].clone();
    let mk = |app: &mut App, y: f64| run(app, "wall.create", json!({ "curve": { "line": { "start": [0, y], "end": [6, y] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let w1 = mk(&mut a, 0.0);
    let w2 = mk(&mut a, 10.0);
    let guid = |app: &App, id: &Value| app.document(app.active().unwrap()).unwrap().doc.store().get(reviera_core::ElementId(id.as_u64().unwrap())).unwrap().guid().to_string();
    let (g1, g2) = (guid(&a, &w1), guid(&a, &w2));
    run(&mut a, "file.save_as", json!({ "path": dir_a.path().join("a.ifc") })).unwrap();
    run(&mut a, "bimserver.publish_local", json!({ "server_id": ids[0], "poid": 131073, "comment": "Two walls" })).unwrap();
    run(&mut b, "bimserver.open", json!({ "server_id": ids[1], "poid": 131073 })).unwrap();
    let id_of = |app: &App, g: &str| app.document(app.active().unwrap()).unwrap().doc.store().find_by_guid(&reviera_core::GlobalId::parse(g).unwrap()).unwrap().0;
    // Both move both walls, differently.
    run(&mut a, "element.move", json!({ "element_ids": [w1, w2], "vector": [0, 1, 0] })).unwrap();
    run(&mut a, "bimserver.sync", json!({ "comment": "A moves north" })).unwrap();
    let (b1, b2) = (id_of(&b, &g1), id_of(&b, &g2));
    run(&mut b, "element.move", json!({ "element_ids": [b1, b2], "vector": [0, -1, 0] })).unwrap();
    assert_eq!(run(&mut b, "merge.list_conflicts", json!({})).unwrap_err().code, "merge.none_pending");

    // B's synchronise stops at the conflicts and keeps them for decisions.
    let r = run(&mut b, "bimserver.sync", json!({ "comment": "B moves south" })).unwrap();
    assert!(r["pending"].as_bool().unwrap() && r["checked_in"].is_null(), "{r}");
    let list = run(&mut b, "merge.list_conflicts", json!({})).unwrap();
    let rows = list.as_array().unwrap().clone();
    assert!(rows.len() >= 2, "{list}");
    let row_of = |g: &str| rows.iter().find(|r| r["guid"] == g).unwrap()["id"].as_str().unwrap().to_owned();
    // Cancel leaves B's working copy as it was.
    run(&mut b, "merge.cancel", json!({})).unwrap();
    assert_eq!((wall_start(&b, &g1), wall_start(&b, &g2)), ([0.0, -1.0], [0.0, 9.0]));
    run(&mut b, "bimserver.sync", json!({ "comment": "B moves south" })).unwrap();
    // Mine for the first wall, theirs for the second; finish checks in.
    run(&mut b, "merge.resolve", json!({ "conflict_id": row_of(&g1), "choice": "mine", "element": true })).unwrap();
    assert_eq!(run(&mut b, "merge.finish", json!({})).unwrap_err().code, "merge.undecided");
    assert_eq!(run(&mut b, "merge.resolve", json!({ "conflict_id": "c99", "choice": "mine" })).unwrap_err().code, "merge.no_conflict");
    run(&mut b, "merge.resolve", json!({ "conflict_id": row_of(&g2), "choice": "theirs", "element": true })).unwrap();
    let decided = run(&mut b, "merge.list_conflicts", json!({})).unwrap();
    assert!(decided.as_array().unwrap().iter().all(|r| !r["decision"].is_null()), "{decided}");
    let r = run(&mut b, "merge.finish", json!({})).unwrap();
    assert!(r["checked_in"]["revision"] == 3, "{r}");
    assert_eq!((wall_start(&b, &g1), wall_start(&b, &g2)), ([0.0, -1.0], [0.0, 11.0]), "mine for the first, theirs for the second");
    // The losers are in the stash: A's move of the first wall, B's own move of the second.
    let stash = run(&mut b, "stash.list", json!({})).unwrap();
    let entries = stash.as_array().unwrap();
    assert!(entries.iter().any(|e| e["guid"] == g1.as_str() && e["side"] == "theirs") && entries.iter().any(|e| e["guid"] == g2.as_str() && e["side"] == "mine"), "{stash}");
    // Re-applying B's own move of the second wall brings it back as an undoable edit.
    let mine = entries.iter().find(|e| e["guid"] == g2.as_str() && e["side"] == "mine").unwrap()["id"].clone();
    run(&mut b, "stash.reapply", json!({ "entry_id": mine, "element": true })).unwrap();
    assert_eq!(wall_start(&b, &g2), [0.0, 9.0]);
    run(&mut b, "edit.undo", json!({})).unwrap();
    assert_eq!(wall_start(&b, &g2), [0.0, 11.0]);
    let left = run(&mut b, "stash.list", json!({})).unwrap().as_array().unwrap().len();
    let ids: Vec<Value> = run(&mut b, "stash.list", json!({})).unwrap().as_array().unwrap().iter().map(|e| e["id"].clone()).collect();
    assert_eq!(run(&mut b, "stash.discard", json!({ "entry_ids": ids })).unwrap(), json!(left));
    assert!(run(&mut b, "stash.list", json!({})).unwrap().as_array().unwrap().is_empty());
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
    // The first computer is behind now: a check-in is refused, synchronising merges B's wall in.
    assert_eq!(run(&mut a, "bimserver.check_in", json!({ "comment": "x" })).unwrap_err().code, "bimserver.out_of_date");
    let level = run(&mut a, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(&mut a, "wall.create", json!({ "curve": { "line": { "start": [30, 40], "end": [34, 40] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
    let r = run(&mut a, "bimserver.sync", json!({ "comment": "Reviera live test: a wall from A" })).unwrap();
    assert_eq!(r["lock"], "taken", "the server keeps the ownership document: {r}");
    assert_eq!(run(&mut a, "bimserver.lock_status", json!({})).unwrap()["locked"], false, "released");
    eprintln!("sync report: created {} updated {} deleted {} remote {} merged {} conflicts {}", r["created"], r["updated"], r["deleted"], r["remote_changes"], r["merged"], r["conflicts"].as_array().map_or(0, Vec::len));
    assert!(r["applied"].as_bool().unwrap() && r["conflicts"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(count(&a, "IfcWall").0, walls.0 + 2, "both computers' walls");
    // B gets A's wall.
    let r = run(&mut b, "bimserver.get_latest", json!({})).unwrap();
    eprintln!("get latest report: created {} updated {} deleted {} remote {} conflicts {}", r["created"], r["updated"], r["deleted"], r["remote_changes"], r["conflicts"].as_array().map_or(0, Vec::len));
    assert_eq!(count(&b, "IfcWall").0, walls.0 + 2);
}
