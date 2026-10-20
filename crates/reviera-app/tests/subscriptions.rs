// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Remote analysis subscriptions (DM-RPC-011) against a mock service on localhost: secure URLs
//! only, nothing saved when the test fails, no secret in the settings or any result, refusals,
//! warnings, and the credential commands closed to MCP.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use reviera_app::subscriptions::Subscriptions;
use reviera_app::{App, AppConfig};
use reviera_core::{Caller, Origin};
use serde_json::{Value, json};

struct Service {
    /// The accepted `key:secret`.
    accept: String,
    usage: f64,
    requests: usize,
}

fn service() -> (u16, Arc<Mutex<Service>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let st = Arc::new(Mutex::new(Service { accept: "ak_1:right".into(), usage: 12.0, requests: 0 }));
    let s2 = st.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).is_err() {
                continue;
            }
            let (mut auth, mut len) = (String::new(), 0);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap_or(("", ""));
                match k.to_ascii_lowercase().as_str() {
                    "authorization" => auth = v.trim().to_owned(),
                    "content-length" => len = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let mut s = s2.lock().unwrap();
            s.requests += 1;
            use base64::Engine;
            let basic = base64::engine::general_purpose::STANDARD.encode(&s.accept);
            let (code, body) = if line.starts_with("POST /v1/auth/token") {
                if auth == format!("Basic {basic}") {
                    (200, json!({"access_token": "t1", "expires_in": 3600, "subscription_id": "sub_1"}))
                } else {
                    (401, json!({"reason": "unauthorised"}))
                }
            } else if line.starts_with("GET /v1/subscription") && auth == "Bearer t1" {
                let end = reviera_compute_proto::subscription::format_time(reviera_compute_proto::subscription::now() + 3 * 86_400);
                (200, json!({"name": "Acme", "plan": "Team", "state": "active", "period_end": end, "allowance": 200.0, "usage": s.usage, "unit": "compute_hours", "job_kinds": ["analysis.energy"]}))
            } else {
                (401, json!({"reason": "unauthorised"}))
            };
            let b = body.to_string();
            let _ = write!(stream, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
        }
    });
    (port, st)
}

fn app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.subscriptions_mut() = Subscriptions::isolated(dir.join("remote-analysis.json"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

#[test]
fn subscriptions_are_tested_saved_without_secrets_and_closed_to_agents() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("remote-analysis.json");
    let mut app = app(dir.path());
    let (port, svc) = service();
    let local = format!("http://localhost:{port}");

    // Plain http is refused; nothing is saved.
    let e = run(&mut app, "subscription.add", json!({"name": "A", "url": "http://analysis.example.com", "key": "ak_1:right"})).unwrap_err();
    assert_eq!(e.code, "subscription.insecure_url");
    let e = run(&mut app, "subscription.add", json!({"name": "A", "url": local, "key": "ak_1:right"})).unwrap_err();
    assert_eq!(e.code, "subscription.insecure_url", "localhost needs the development setting");
    // https to a server that does not speak TLS fails; nothing is saved.
    let e = run(&mut app, "subscription.add", json!({"name": "A", "url": format!("https://localhost:{port}"), "key": "ak_1:right"})).unwrap_err();
    assert!(matches!(e.code.as_ref(), "subscription.tls" | "subscription.unreachable"), "{e:?}");
    // A wrong secret: refused, with the message, and nothing saved.
    let e = run(&mut app, "subscription.add", json!({"name": "A", "url": local, "key": "ak_1", "secret": "wrong", "development": true})).unwrap_err();
    assert_eq!(e.message, "The service refused the key; check it or ask your administrator");
    assert!(!settings.exists());

    // The right one.
    let out = run(&mut app, "subscription.add", json!({"name": "Acme", "url": local, "key": "ak_1:right", "development": true})).unwrap();
    assert_eq!(out["secret_storage"], "session");
    assert_eq!(out["status"]["state"], "active");
    assert_eq!(out["status"]["remaining"], 188.0);
    assert!(out["status"]["warnings"][0].as_str().unwrap().contains("ends in 3 day"), "{out}");
    let id = out["subscription_id"].as_str().unwrap().to_owned();
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("ak_1") && !text.contains("right"), "{text}");
    let list = run(&mut app, "subscription.list", Value::Null).unwrap();
    assert_eq!((list[0]["key"].as_str(), list[0]["default"].as_bool()), (Some("ak_1"), Some(true)));
    assert!(!list.to_string().contains("right"));

    // Usage near the allowance warns.
    svc.lock().unwrap().usage = 190.0;
    let st = run(&mut app, "subscription.status", json!({"subscription_id": id, "refresh": true})).unwrap();
    assert!(st["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("95 %")), "{st}");

    // An agent can list and read the status, but not add, change, test or remove.
    let mcp = Caller { origin: Origin::mcp("probe", "s1"), ..Caller::user() };
    assert!(app.execute("subscription.list", Value::Null, &mcp).is_ok());
    for (c, a) in [
        ("subscription.add", json!({"name": "B", "url": local, "key": "k:s", "development": true})),
        ("subscription.set", json!({"subscription_id": id, "name": "C"})),
        ("subscription.test", json!({"subscription_id": id})),
        ("subscription.remove", json!({"subscription_id": id})),
        ("subscription.set_default", json!({"subscription_id": id})),
    ] {
        assert_eq!(app.execute(c, a, &mcp).unwrap_err().code, "permission.denied", "{c}");
    }

    // The service stops accepting the secret: Test says so; a refreshed status does not try it again.
    svc.lock().unwrap().accept = "ak_1:rotated".into();
    let st = run(&mut app, "subscription.test", json!({"subscription_id": id})).unwrap();
    assert_eq!((st["state"].as_str(), st["reason"].as_str()), (Some("refused"), Some("unauthorised")));
    let before = svc.lock().unwrap().requests;
    run(&mut app, "subscription.status", json!({"subscription_id": id, "refresh": true})).unwrap();
    assert_eq!(svc.lock().unwrap().requests, before, "a refused key is not retried");
    // A wrong new secret is not saved; the right one is.
    assert!(run(&mut app, "subscription.set", json!({"subscription_id": id, "secret": "nope"})).is_err());
    let out = run(&mut app, "subscription.set", json!({"subscription_id": id, "secret": "rotated", "name": "Acme EU"})).unwrap();
    assert_eq!((out["name"].as_str(), out["status"]["state"].as_str()), (Some("Acme EU"), Some("active")));

    run(&mut app, "subscription.set_default", json!({"subscription_id": id, "job_kind": "analysis.energy"})).unwrap();
    let list = run(&mut app, "subscription.list", Value::Null).unwrap();
    assert_eq!(list[0]["default_for"], json!(["analysis.energy"]));
    run(&mut app, "subscription.remove", json!({"subscription_id": "Acme EU"})).unwrap();
    assert_eq!(run(&mut app, "subscription.list", Value::Null).unwrap(), json!([]));
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(!text.contains("rotated") && !text.contains("ak_1"), "{text}");
}
