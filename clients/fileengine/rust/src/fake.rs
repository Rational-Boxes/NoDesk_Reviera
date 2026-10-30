// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! An in-memory FileEngine REST door on localhost, for tests (feature `fake`). It answers the
//! routes [`crate::Client`] uses the way the live door does: bearer tokens from a service
//! credential that run out, folders and files by uid, every write and restore a new version,
//! soft delete, metadata, renditions (none) and permission checks (a folder can be made
//! read-only).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
struct Entry {
    name: String,
    parent: String,
    dir: bool,
    deleted: bool,
    owner: String,
    /// (timestamp, by, content), oldest first.
    versions: Vec<(String, String, Vec<u8>)>,
    metadata: BTreeMap<String, String>,
    created: i64,
    modified: i64,
}

#[derive(Debug, Default)]
struct State {
    key: String,
    secret: String,
    user: String,
    tenant: String,
    life: u64,
    tokens: HashMap<String, Instant>,
    nodes: BTreeMap<String, Entry>,
    read_only: BTreeSet<String>,
    next: u64,
    clock: u64,
    log: Vec<String>,
}

impl State {
    fn uid(&mut self) -> String {
        self.next += 1;
        format!("00000000-0000-4000-8000-{:012x}", self.next)
    }

    fn stamp(&mut self) -> String {
        self.clock += 1;
        format!("20261030_{:06}.{:03}", 120000 + self.clock / 1000, self.clock % 1000)
    }

    fn issue(&mut self) -> Value {
        self.next += 1;
        let t = format!("tok{}x{}", self.next, self.clock);
        self.tokens.insert(t.clone(), Instant::now() + Duration::from_secs(self.life));
        json!({ "token": t, "token_type": "Bearer", "expires_in": self.life })
    }

    fn live(&self, uid: &str) -> Option<&Entry> {
        self.nodes.get(uid).filter(|e| !e.deleted)
    }

    /// Whether `uid` or a folder above it is read-only.
    fn locked(&self, uid: &str) -> bool {
        let mut uid = uid.to_owned();
        loop {
            if self.read_only.contains(&uid) {
                return true;
            }
            match self.nodes.get(&uid) {
                Some(e) if !e.parent.is_empty() => uid = e.parent.clone(),
                _ => return false,
            }
        }
    }

    fn entry_json(uid: &str, e: &Entry) -> Value {
        json!({
            "uid": uid, "name": e.name, "type": if e.dir { "directory" } else { "file" },
            "size": e.versions.last().map_or(0, |v| v.2.len()), "version_count": e.versions.len().max(1),
            "rendition_count": 0, "has_renditions": false, "deleted": e.deleted,
            "created_at": e.created, "modified_at": e.modified, "owner": e.owner,
            "created_by": e.owner, "modified_by": e.versions.last().map_or(e.owner.as_str(), |v| v.1.as_str()),
        })
    }

    fn stat_json(uid: &str, e: &Entry) -> Value {
        json!({
            "uid": uid, "name": e.name, "parent_uid": e.parent, "type": if e.dir { "directory" } else { "file" },
            "size": e.versions.last().map_or(0, |v| v.2.len()), "owner": e.owner,
            "version": e.versions.last().map_or("", |v| v.0.as_str()),
            "created_at": e.created, "modified_at": e.modified, "rendition_count": 0, "has_renditions": false,
        })
    }

    fn add(&mut self, parent: &str, name: &str, dir: bool, owner: &str) -> Result<String, (u16, String)> {
        if !self.live(parent).is_some_and(|p| p.dir) {
            return Err((404, "Directory does not exist".into()));
        }
        if name.is_empty() || name.contains('/') {
            return Err((400, "invalid name".into()));
        }
        if self.nodes.values().any(|e| e.parent == parent && e.name == name && !e.deleted) {
            return Err((400, "File already exists".into()));
        }
        if self.locked(parent) {
            return Err((403, "permission denied".into()));
        }
        let uid = self.uid();
        let now = 1_793_000_000 + self.clock as i64;
        self.nodes.insert(uid.clone(), Entry { name: name.into(), parent: parent.into(), dir, owner: owner.into(), created: now, modified: now, ..Entry::default() });
        Ok(uid)
    }

    fn write(&mut self, uid: &str, bytes: Vec<u8>, by: &str) -> Result<(), (u16, String)> {
        if !self.live(uid).is_some_and(|e| !e.dir) {
            return Err((404, "File does not exist".into()));
        }
        if self.locked(uid) {
            return Err((403, "permission denied".into()));
        }
        let ts = self.stamp();
        let now = 1_793_000_000 + self.clock as i64;
        let e = self.nodes.get_mut(uid).unwrap();
        e.versions.push((ts, by.into(), bytes));
        e.modified = now;
        Ok(())
    }
}

/// A running fake server.
pub struct Fake {
    /// Its root URL (`http://localhost:<port>/`); the door is under `/api/`.
    pub url: String,
    state: Arc<Mutex<State>>,
}

struct Request {
    method: String,
    path: String,
    query: String,
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
    let (path, query) = target.split_once('?').map_or((target.clone(), String::new()), |(p, q)| (p.to_owned(), q.to_owned()));
    Some(Request { method, path, query, headers, body })
}

fn unpercent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut k = 0;
    while k < b.len() {
        if b[k] == b'%'
            && let Some(v) = s.get(k + 1..k + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            k += 3;
            continue;
        }
        out.push(b[k]);
        k += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn basic(value: &str) -> Option<(String, String)> {
    let enc = value.strip_prefix("Basic ")?.trim();
    // A small base64 decoder (the tests' credentials are ASCII).
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in enc.bytes().filter(|c| *c != b'=') {
        let v = alphabet.iter().position(|a| *a == c)? as u32;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    let (k, s) = text.split_once(':')?;
    Some((k.to_owned(), s.to_owned()))
}

type Answer = (u16, &'static str, Vec<u8>);

fn ok(v: Value) -> Answer {
    (200, "application/json", serde_json::to_vec(&v).unwrap())
}

fn err(status: u16, message: &str) -> Answer {
    (status, "application/json", serde_json::to_vec(&json!({ "error": message })).unwrap())
}

fn done() -> Answer {
    (204, "application/json", Vec::new())
}

fn answer(st: &mut State, req: &Request) -> Answer {
    let Some(route) = req.path.strip_prefix("/api/v1/") else { return err(404, "not found") };
    let seg: Vec<String> = route.split('/').map(unpercent).collect();
    let seg: Vec<&str> = seg.iter().map(String::as_str).collect();
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    let auth = req.headers.get("authorization").cloned().unwrap_or_default();
    st.log.push(format!("{} /{}", req.method, route));

    if seg == ["auth", "token"] && req.method == "POST" {
        return match basic(&auth) {
            Some((k, s)) if k == st.key && s == st.secret => ok(st.issue()),
            _ => err(401, "invalid credentials"),
        };
    }
    let bearer = auth.strip_prefix("Bearer ").unwrap_or_default().to_owned();
    let valid = st.tokens.get(&bearer).is_some_and(|t| Instant::now() < *t);
    if seg == ["auth", "refresh"] && req.method == "POST" {
        return if valid {
            st.tokens.remove(&bearer);
            ok(st.issue())
        } else {
            err(401, "authentication required")
        };
    }
    if !valid {
        return err(401, "authentication required");
    }
    if let Some(t) = req.headers.get("x-tenant")
        && *t != st.tenant
    {
        return err(403, "no access to that tenant");
    }
    let user = st.user.clone();
    let result = |r: Result<(), (u16, String)>| r.map_or_else(|(s, m)| err(s, &m), |()| done());
    match (req.method.as_str(), seg.as_slice()) {
        ("DELETE", ["auth", "token"]) => {
            st.tokens.remove(&bearer);
            done()
        }
        ("GET", ["whoami"]) => ok(json!({ "user": user, "tenant": st.tenant, "roles": ["engineering"] })),
        ("GET", ["tenants"]) => ok(json!({ "tenants": [st.tenant], "current": st.tenant })),
        ("GET", ["dirs", uid]) => {
            if !st.live(uid).is_some_and(|e| e.dir) {
                return err(404, "Directory does not exist");
            }
            let deleted = req.query.contains("deleted=true");
            let entries: Vec<Value> = st.nodes.iter().filter(|(_, e)| e.parent == *uid && (deleted || !e.deleted)).map(|(k, e)| State::entry_json(k, e)).collect();
            ok(json!({ "entries": entries }))
        }
        ("POST", ["dirs", uid]) => match st.add(uid, body["name"].as_str().unwrap_or(""), true, &user) {
            Ok(u) => (201, "application/json", serde_json::to_vec(&json!({ "uid": u })).unwrap()),
            Err((s, m)) => err(s, &m),
        },
        ("POST", ["dirs", uid, "files"]) => match st.add(uid, body["name"].as_str().unwrap_or(""), false, &user) {
            Ok(u) => (201, "application/json", serde_json::to_vec(&json!({ "uid": u })).unwrap()),
            Err((s, m)) => err(s, &m),
        },
        ("DELETE", ["dirs", uid] | ["files", uid]) => {
            let dir = seg[0] == "dirs";
            if !st.live(uid).is_some_and(|e| e.dir == dir) || *uid == crate::ROOT {
                return err(404, "File does not exist");
            }
            if st.locked(uid) {
                return err(403, "permission denied");
            }
            st.nodes.get_mut(*uid).unwrap().deleted = true;
            done()
        }
        ("POST", ["files", uid, "undelete"]) => match st.nodes.get_mut(*uid) {
            Some(e) => {
                e.deleted = false;
                done()
            }
            None => err(404, "File does not exist"),
        },
        ("GET", ["nodes", uid]) => st.live(uid).map_or_else(|| err(404, "File does not exist"), |e| ok(State::stat_json(uid, e))),
        ("GET", ["nodes", uid, "exists"]) => ok(json!({ "exists": st.live(uid).is_some() })),
        ("POST", ["nodes", uid, "rename"]) => {
            if st.locked(uid) {
                return err(403, "permission denied");
            }
            match st.nodes.get_mut(*uid) {
                Some(e) if !e.deleted => {
                    e.name = body["new_name"].as_str().unwrap_or("").to_owned();
                    done()
                }
                _ => err(404, "File does not exist"),
            }
        }
        ("POST", ["nodes", uid, "move"]) => {
            let to = body["destination_parent_uid"].as_str().unwrap_or("").to_owned();
            if !st.live(&to).is_some_and(|e| e.dir) || !st.live(uid).is_some() {
                return err(404, "File does not exist");
            }
            let mut p = to.clone();
            while !p.is_empty() {
                if p == *uid {
                    return err(409, "cannot move a folder into itself");
                }
                p = st.nodes.get(&p).map(|e| e.parent.clone()).unwrap_or_default();
            }
            st.nodes.get_mut(*uid).unwrap().parent = to;
            done()
        }
        ("GET", ["files", uid, "content"]) => {
            let Some(e) = st.live(uid).filter(|e| !e.dir) else { return err(404, "File does not exist") };
            let bytes = e.versions.last().map(|v| v.2.clone()).unwrap_or_default();
            if let Some((a, b)) = req.headers.get("range").and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.split_once('-')) {
                let a: usize = a.parse().unwrap_or(0);
                let b: usize = b.parse::<usize>().unwrap_or(usize::MAX).min(bytes.len().saturating_sub(1));
                return (206, "application/octet-stream", bytes.get(a..=b).map(<[u8]>::to_vec).unwrap_or_default());
            }
            (200, "application/octet-stream", bytes)
        }
        ("PUT", ["files", uid, "content"]) => result(st.write(uid, req.body.clone(), &user)),
        ("GET", ["files", _, "renditions"]) => ok(json!({ "entries": [] })),
        ("GET", ["files", uid, "versions"]) => {
            let Some(e) = st.live(uid) else { return err(404, "File does not exist") };
            let v: Vec<&(String, String, Vec<u8>)> = e.versions.iter().rev().collect();
            ok(json!({ "versions": v.iter().map(|x| &x.0).collect::<Vec<_>>(), "entries": v.iter().map(|x| json!({ "version": x.0, "revised_by": x.1 })).collect::<Vec<_>>() }))
        }
        ("GET", ["files", uid, "versions", ts]) => st.live(uid).and_then(|e| e.versions.iter().find(|v| v.0 == *ts)).map_or_else(|| err(404, "Version does not exist"), |v| (200, "application/octet-stream", v.2.clone())),
        ("POST", ["files", uid, "restore"]) => {
            let ts = body["version_timestamp"].as_str().unwrap_or("").to_owned();
            let Some(old) = st.live(uid).and_then(|e| e.versions.iter().find(|v| v.0 == ts)).map(|v| v.2.clone()) else { return err(404, "Version does not exist") };
            match st.write(uid, old, &user) {
                Ok(()) => ok(json!({ "restored_version": ts })),
                Err((s, m)) => err(s, &m),
            }
        }
        ("GET", ["nodes", uid, "metadata"]) => st.live(uid).map_or_else(|| err(404, "File does not exist"), |e| ok(json!({ "metadata": e.metadata }))),
        ("GET", ["nodes", uid, "metadata", key]) => st.live(uid).and_then(|e| e.metadata.get(*key)).map_or_else(|| err(404, "no such key"), |v| ok(json!({ "value": v }))),
        ("PUT", ["nodes", uid, "metadata", key]) => {
            if st.locked(uid) {
                return err(403, "permission denied");
            }
            match st.nodes.get_mut(*uid) {
                Some(e) => {
                    e.metadata.insert((*key).to_owned(), body["value"].as_str().unwrap_or("").to_owned());
                    done()
                }
                None => err(404, "File does not exist"),
            }
        }
        ("DELETE", ["nodes", uid, "metadata", key]) => {
            if let Some(e) = st.nodes.get_mut(*uid) {
                e.metadata.remove(*key);
            }
            done()
        }
        ("GET", ["nodes", uid, "permissions"]) => {
            let p = req.query.split('&').find_map(|kv| kv.strip_prefix("permission=")).unwrap_or("r").to_ascii_lowercase();
            let write = matches!(p.as_str(), "w" | "write" | "d" | "delete");
            ok(json!({ "has_permission": st.live(uid).is_some() && !(write && st.locked(uid)) }))
        }
        _ => err(404, "not found"),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    }
}

impl std::fmt::Debug for Fake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fake").field("url", &self.url).finish()
    }
}

impl Fake {
    /// Starts a server that accepts the service credential `key`:`secret` for `user`, with an
    /// empty top folder holding `Engineering`.
    pub fn start(key: &str, secret: &str, user: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://localhost:{}/", listener.local_addr().unwrap().port());
        let mut st = State { key: key.into(), secret: secret.into(), user: user.into(), tenant: "acme".into(), life: 900, ..State::default() };
        st.nodes.insert(crate::ROOT.into(), Entry { name: String::new(), dir: true, owner: "admin".into(), ..Entry::default() });
        st.add(crate::ROOT, "Engineering", true, "admin").unwrap();
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

    /// Tokens issued from now on last `secs` seconds.
    pub fn set_token_life(&self, secs: u64) {
        self.state.lock().unwrap().life = secs;
    }

    /// Every token issued so far stops working (as after a server restart).
    pub fn revoke_tokens(&self) {
        self.state.lock().unwrap().tokens.clear();
    }

    /// Writing under `uid` (a folder or file) is refused from now on.
    pub fn make_read_only(&self, uid: &str) {
        self.state.lock().unwrap().read_only.insert(uid.into());
    }

    /// Another user writes a new version of `uid`.
    pub fn write_as(&self, uid: &str, bytes: &[u8], user: &str) {
        self.state.lock().unwrap().write(uid, bytes.to_vec(), user).unwrap();
    }

    /// The uid at `path` from the top folder.
    pub fn uid_of(&self, path: &str) -> Option<String> {
        let st = self.state.lock().unwrap();
        let mut uid = crate::ROOT.to_owned();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            uid = st.nodes.iter().find(|(_, e)| e.parent == uid && e.name == part && !e.deleted)?.0.clone();
        }
        Some(uid)
    }

    /// The newest content of `uid`.
    pub fn content(&self, uid: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().nodes.get(uid).and_then(|e| e.versions.last()).map(|v| v.2.clone())
    }

    /// A node's metadata.
    pub fn metadata(&self, uid: &str) -> BTreeMap<String, String> {
        self.state.lock().unwrap().nodes.get(uid).map(|e| e.metadata.clone()).unwrap_or_default()
    }

    /// The requests answered so far (`GET /v1/whoami` …), and forgets them.
    pub fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut self.state.lock().unwrap().log)
    }
}
