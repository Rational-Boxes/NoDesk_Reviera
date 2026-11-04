// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Riviera contributors

//! An in-memory BCF API 3.0 server on localhost, for tests (feature `fake`). It answers as
//! OpenConstructionERP's does: the API under `/api/v1/bcf/3.0/`, signing in by e-mail and
//! password at `/api/v1/users/auth/login/` (a bearer that runs out, and a refresh token), lists
//! wrapped as `{"items": […]}` unless [`Fake::bare_lists`] says otherwise, and topic GUIDs given
//! by the client kept. A test can play someone working in the server's web pages.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

#[derive(Debug, Default)]
struct State {
    user: String,
    password: String,
    tokens: Vec<String>,
    refresh: Vec<String>,
    next: u64,
    clock: u64,
    bare: bool,
    projects: Vec<Value>,
    /// (project, topic) with its `comments` and `viewpoints` inside, as kept.
    topics: Vec<(String, Value)>,
    /// Snapshots by viewpoint GUID.
    snapshots: HashMap<String, Vec<u8>>,
    log: Vec<String>,
}

impl State {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("00000000-0000-4000-8000-{:012x}", self.next)
    }

    fn now(&mut self) -> String {
        self.clock += 1;
        format!("2026-11-03T10:{:02}:{:02}Z", (self.clock / 60) % 60, self.clock % 60)
    }

    fn list(&self, v: Vec<Value>) -> Value {
        if self.bare { Value::Array(v) } else { json!({ "items": v }) }
    }

    fn topic_view(t: &Value) -> Value {
        let mut t = t.clone();
        if let Some(o) = t.as_object_mut() {
            o.remove("comments");
            o.remove("viewpoints");
        }
        t
    }
}

/// A running fake server.
pub struct Fake {
    /// Its root (`http://localhost:<port>/`); the BCF API is under `api/v1/bcf/`.
    pub url: String,
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for Fake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fake").field("url", &self.url).finish()
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut first = String::new();
    r.read_line(&mut first).ok()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let len = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    let path = target.split_once('?').map_or(target.clone(), |(p, _)| p.to_owned());
    Some(Request { method, path, headers, body })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        422 => "Unprocessable Content",
        _ => "Error",
    }
}

/// Standard base64 (the snapshots).
fn unbase64(s: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let Some(v) = alphabet.iter().position(|&a| a == c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

type Answer = (u16, &'static str, Vec<u8>);

fn ok(v: Value) -> Answer {
    (200, "application/json", v.to_string().into_bytes())
}

fn err(status: u16, msg: &str) -> Answer {
    (status, "application/json", json!({ "detail": msg }).to_string().into_bytes())
}

fn answer(st: &mut State, req: &Request) -> Answer {
    st.log.push(format!("{} {}", req.method, req.path));
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/api/v1/users/auth/login/") => {
            if body["email"].as_str() != Some(st.user.as_str()) || body["password"].as_str() != Some(st.password.as_str()) {
                return err(401, "Incorrect email or password");
            }
            let (t, r) = (format!("tok-{}", st.id()), format!("ref-{}", st.id()));
            st.tokens.push(t.clone());
            st.refresh.push(r.clone());
            return ok(json!({ "access_token": t, "refresh_token": r, "token_type": "bearer", "expires_in": 3600 }));
        }
        ("POST", "/api/v1/users/auth/refresh/") => {
            if !st.refresh.iter().any(|r| Some(r.as_str()) == body["refresh_token"].as_str()) {
                return err(401, "Invalid refresh token");
            }
            let t = format!("tok-{}", st.id());
            st.tokens.push(t.clone());
            return ok(json!({ "access_token": t, "refresh_token": body["refresh_token"], "expires_in": 3600 }));
        }
        _ => {}
    }
    let Some(rest) = req.path.strip_prefix("/api/v1/bcf/3.0/") else { return err(404, "Not Found") };
    let bearer = req.headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !st.tokens.iter().any(|t| t == bearer) {
        return err(401, "Not authenticated");
    }
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let user = st.user.clone();
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["current-user"]) => ok(json!({ "id": user, "name": user })),
        ("GET", ["projects"]) => ok(Value::Array(st.projects.clone())),
        ("GET", ["projects", p]) => match st.projects.iter().find(|x| x["project_id"] == *p) {
            Some(x) => ok(x.clone()),
            None => err(404, "Project not found"),
        },
        ("GET", ["projects", _, "extensions"]) => ok(json!({
            "topic_type": ["Issue", "Clash", "Request"], "topic_status": ["Open", "In Progress", "Resolved", "Closed"],
            "priority": ["High", "Normal", "Low"], "topic_label": [], "stage": [], "users": [user]
        })),
        (_, ["projects", p, ..]) if !st.projects.iter().any(|x| x["project_id"] == *p) => err(404, "Project not found"),
        ("GET", ["projects", p, "topics"]) => {
            let v: Vec<Value> = st.topics.iter().filter(|(q, _)| q == p).map(|(_, t)| State::topic_view(t)).collect();
            ok(st.list(v))
        }
        ("POST", ["projects", p, "topics"]) => {
            if body["title"].as_str().is_none_or(|t| t.trim().is_empty()) {
                return err(422, "title is required");
            }
            let guid = body["guid"].as_str().map(str::to_lowercase).unwrap_or_else(|| st.id());
            if st.topics.iter().any(|(q, t)| q == p && t["guid"] == guid.as_str()) {
                return err(409, "a topic with this guid exists");
            }
            let now = st.now();
            let mut t = body.clone();
            t["guid"] = json!(guid);
            t["creation_author"] = json!(user);
            t["creation_date"] = json!(now);
            t["modified_date"] = json!(now);
            t["comments"] = json!([]);
            t["viewpoints"] = json!([]);
            if t["topic_status"].is_null() {
                t["topic_status"] = json!("Open");
            }
            st.topics.push(((*p).to_owned(), t.clone()));
            (201, "application/json", State::topic_view(&t).to_string().into_bytes())
        }
        (m, ["projects", p, "topics", g, tail @ ..]) => {
            let Some(k) = st.topics.iter().position(|(q, t)| q == p && t["guid"] == *g) else { return err(404, "Topic not found") };
            match (m, tail) {
                ("GET", []) => ok(State::topic_view(&st.topics[k].1)),
                ("PUT", []) => {
                    let now = st.now();
                    let t = &mut st.topics[k].1;
                    let keep: Vec<(&str, Value)> = ["guid", "creation_author", "creation_date", "comments", "viewpoints"].iter().map(|f| (*f, t[*f].clone())).collect();
                    *t = body.clone();
                    for (f, v) in keep {
                        t[f] = v;
                    }
                    t["modified_author"] = json!(user);
                    t["modified_date"] = json!(now);
                    ok(State::topic_view(t))
                }
                ("DELETE", []) => {
                    st.topics.remove(k);
                    (204, "application/json", Vec::new())
                }
                ("GET", ["comments"]) => {
                    let c = st.topics[k].1["comments"].as_array().cloned().unwrap_or_default();
                    ok(st.list(c))
                }
                ("POST", ["comments"]) => {
                    let id = body["guid"].as_str().map(str::to_owned).unwrap_or_else(|| st.id());
                    let now = st.now();
                    let c = json!({ "guid": id, "date": now, "author": user, "comment": body["comment"], "topic_guid": g, "viewpoint_guid": body["viewpoint_guid"] });
                    let t = &mut st.topics[k].1;
                    t["comments"].as_array_mut().unwrap().push(c.clone());
                    t["modified_date"] = json!(now);
                    (201, "application/json", c.to_string().into_bytes())
                }
                ("GET", ["viewpoints"]) => {
                    let v = st.topics[k].1["viewpoints"].as_array().cloned().unwrap_or_default();
                    ok(st.list(v))
                }
                ("POST", ["viewpoints"]) => {
                    let id = body["guid"].as_str().map(str::to_owned).unwrap_or_else(|| st.id());
                    let mut v = body.clone();
                    v["guid"] = json!(id);
                    if let Some(data) = v["snapshot"]["snapshot_data"].as_str() {
                        let data = data.split_once("base64,").map_or(data, |(_, b)| b);
                        st.snapshots.insert(id.clone(), unbase64(data));
                    }
                    if let Some(o) = v.as_object_mut() {
                        o.remove("snapshot");
                    }
                    st.topics[k].1["viewpoints"].as_array_mut().unwrap().push(v.clone());
                    (201, "application/json", v.to_string().into_bytes())
                }
                ("GET", ["viewpoints", v, "snapshot"]) => match st.snapshots.get(*v) {
                    Some(b) => (200, "image/png", b.clone()),
                    None => err(404, "No snapshot"),
                },
                _ => err(404, "Not Found"),
            }
        }
        _ => err(404, "Not Found"),
    }
}

impl Fake {
    /// Starts a server where `user` signs in with `password`, holding one empty project,
    /// `Hall` (id `11111111-1111-4111-8111-111111111111`).
    pub fn start(user: &str, password: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://localhost:{}/", listener.local_addr().unwrap().port());
        let st = State {
            user: user.into(),
            password: password.into(),
            projects: vec![json!({ "project_id": Self::PROJECT, "name": "Hall", "authorization": { "project_actions": ["update", "createTopic"] } })],
            ..State::default()
        };
        let state = Arc::new(Mutex::new(st));
        let s2 = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let s3 = s2.clone();
                std::thread::spawn(move || {
                    let Some(req) = read_request(&stream) else { return };
                    let (status, ctype, out) = answer(&mut s3.lock().unwrap(), &req);
                    let _ = write!(stream, "HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reason(status), out.len());
                    let _ = stream.write_all(&out);
                });
            }
        });
        Self { url, state }
    }

    /// The project every fake starts with.
    pub const PROJECT: &'static str = "11111111-1111-4111-8111-111111111111";

    /// Lists answered as bare arrays, as the standard has them.
    pub fn bare_lists(&self) {
        self.state.lock().unwrap().bare = true;
    }

    /// Every token issued so far stops working.
    pub fn revoke_tokens(&self) {
        self.state.lock().unwrap().tokens.clear();
    }

    /// Someone raises a topic in the server's web pages: `topic` with its `comments` and
    /// `viewpoints` inside; a `snapshot` (bytes) for its first viewpoint.
    pub fn add_topic(&self, topic: Value, snapshot: Option<&[u8]>) {
        let mut st = self.state.lock().unwrap();
        if let (Some(b), Some(v)) = (snapshot, topic["viewpoints"][0]["guid"].as_str()) {
            st.snapshots.insert(v.to_owned(), b.to_vec());
        }
        st.topics.push((Self::PROJECT.into(), topic));
    }

    /// Someone changes a topic's field in the web pages.
    pub fn set_field(&self, guid: &str, field: &str, value: Value) {
        let mut st = self.state.lock().unwrap();
        if let Some((_, t)) = st.topics.iter_mut().find(|(_, t)| t["guid"] == guid) {
            t[field] = value;
        }
    }

    /// The topics as the server holds them (comments and viewpoints inside).
    pub fn topics(&self) -> Vec<Value> {
        self.state.lock().unwrap().topics.iter().map(|(_, t)| t.clone()).collect()
    }

    /// The requests answered, as `METHOD path`.
    pub fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }
}
