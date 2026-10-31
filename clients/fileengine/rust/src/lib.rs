// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! A client for FileEngine's REST door (Proxima File Vault's `http_bridge`), spec Ch 6 and
//! `design_documents/FILEENGINE_INTEGRATION.md`.
//!
//! - **Server**: the tenant's address, e.g. `https://acme.proximafilevault.com`; the REST door is
//!   under `/api/` there (a URL that already names a path is used as given).
//! - **Signing in** with a service credential: `POST /v1/auth/token` with HTTP Basic
//!   `fesk_…:fesks_…` answers a bearer for 900 s. The client renews it at `/v1/auth/refresh` a
//!   minute before it runs out, and asks for a new one with the credential when renewal is
//!   refused, so callers never see an expiry. The credential is held in memory only.
//! - **Folders and files** by uid (`root` is the top): list, stat, create, rename, move, copy,
//!   soft delete and undelete; content streamed both ways, with `Range` for a part.
//! - **Versions**: every write and every restore is a new version, named by its timestamp
//!   (`20261030_195013.009`); a file's `version` is its newest. [`Client::save_checked`] writes
//!   only when the file is still at the version the caller opened, which is how a save finds out
//!   that someone else saved first (the door has no ETag or lock; the check is made just before
//!   writing).
//! - **Metadata** (string key/values on any node) and **permission** checks.
//!
//! This crate is MIT licensed (Reviera's interface components are), and talks to FileEngine
//! (AGPL-3.0) over the network only.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::blocking::{RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[cfg(feature = "fake")]
pub mod fake;

/// The filesystem's top folder.
pub const ROOT: &str = "root";

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The URL is not one the client will use.
    Url,
    /// The connection or TLS failed, or the server answered with an unexpected HTTP error.
    Transport,
    /// Not signed in, or the credential was refused (wrong, revoked, expired).
    Auth,
    /// The server refused the operation for this user (HTTP 403).
    Forbidden,
    /// No such file or folder (HTTP 404).
    NotFound,
    /// The file changed since it was opened, or the move or copy is impossible (HTTP 409).
    Conflict,
    /// The upload is larger than the server accepts (HTTP 413).
    TooLarge,
    /// The server refused the request as invalid (HTTP 400), or failed (HTTP 5xx).
    Server,
    /// The answer was not what the API describes.
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// The HTTP status, when the server answered.
    pub status: Option<u16>,
    pub message: String,
}

impl Error {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, status: None, message: message.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Who is signed in (`GET /v1/whoami`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub user: String,
    pub tenant: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// The tenants the user may use, and the one in use (`GET /v1/tenants`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenants {
    #[serde(default)]
    pub tenants: Vec<String>,
    #[serde(default)]
    pub current: String,
}

/// `file`, `directory` or `symlink`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    File,
    Directory,
    Symlink,
    #[serde(other)]
    Other,
}

/// A folder's entry (`GET /v1/dirs/{uid}`) or a node's stat (`GET /v1/nodes/{uid}`); the fields a
/// listing does not give (`parent_uid`, `version`) are empty there, and the stat lacks the
/// listing's `version_count`, `deleted` and the `*_by` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Node {
    pub uid: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: Kind,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub parent_uid: String,
    /// The newest version's timestamp (stat only; empty before the first write).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub version_count: u64,
    #[serde(default)]
    pub deleted: bool,
    /// Unix seconds.
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub modified_at: i64,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub modified_by: String,
    /// Hidden renditions (PDF, preview, thumbnail …) the server made of the file.
    #[serde(default)]
    pub rendition_count: u64,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Directory
    }
}

/// One version of a file, newest first in [`Client::versions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Its timestamp, `YYYYMMDD_HHMMSS.mmm` (UTC); sorts as text.
    pub version: String,
    #[serde(default)]
    pub revised_by: String,
}

/// The bearer and when it must be renewed.
struct Session {
    token: String,
    renew_at: Instant,
}

/// One server, and the session on it. Calls take `&self`; the session renews itself.
pub struct Client {
    base: Url,
    http: reqwest::blocking::Client,
    credential: Mutex<Option<(String, String)>>,
    session: Mutex<Option<Session>>,
    tenant: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base.as_str()).field("signed_in", &self.signed_in()).finish()
    }
}

/// Whether `url` may be used: https, or http for loopback (or anything, when `allow_http`).
pub fn check_url(url: &str, allow_http: bool) -> Result<Url> {
    let u = Url::parse(url.trim()).map_err(|e| Error::new(ErrorKind::Url, format!("{url:?} is not a URL: {e}")))?;
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"));
    match u.scheme() {
        "https" => {}
        "http" if loopback || allow_http => {}
        "http" => return Err(Error::new(ErrorKind::Url, "use https:// for a server that is not on this computer (plain http is for localhost)")),
        s => return Err(Error::new(ErrorKind::Url, format!("{s}:// is not a FileEngine address; use https://"))),
    }
    if u.host_str().is_none() {
        return Err(Error::new(ErrorKind::Url, "the URL names no host"));
    }
    Ok(u)
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(Vec::new(), provider.clone())
        .map_err(|e| Error::new(ErrorKind::Transport, format!("the certificates could not be loaded: {e}")))?;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::blocking::Client::builder()
        .tls_backend_preconfigured(tls)
        .timeout(timeout)
        .user_agent(concat!("BIM-Reviera/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::new(ErrorKind::Transport, format!("the connection could not be set up: {e}")))
}

/// The error an HTTP answer stands for: the server's `{"error": …}` text, by status.
fn failure(status: u16, body: &str, what: &str) -> Error {
    let said = serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"].as_str().map(str::to_owned)).filter(|s| !s.is_empty());
    let kind = match status {
        401 => ErrorKind::Auth,
        403 => ErrorKind::Forbidden,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        413 => ErrorKind::TooLarge,
        400 | 500..=599 => ErrorKind::Server,
        _ => ErrorKind::Transport,
    };
    let message = match (kind, said) {
        (ErrorKind::Auth, s) => format!("FileEngine refused the sign-in{}", s.map(|s| format!(": {s}")).unwrap_or_default()),
        (_, Some(s)) => format!("{what}: {s}"),
        (_, None) => format!("{what}: the server answered {status}"),
    };
    Error { kind, status: Some(status), message }
}

fn percent(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Text a JSON answer holds under `key`.
fn field(v: &Value, key: &str, what: &str) -> Result<String> {
    v[key].as_str().map(str::to_owned).ok_or_else(|| Error::new(ErrorKind::Protocol, format!("{what}: the answer has no {key}")))
}

impl Client {
    /// A client for the server at `url`, 120 s per call.
    pub fn new(url: &str, allow_http: bool) -> Result<Self> {
        Self::with_timeout(url, allow_http, Duration::from_secs(120))
    }

    pub fn with_timeout(url: &str, allow_http: bool, timeout: Duration) -> Result<Self> {
        let mut base = check_url(url, allow_http)?;
        let path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&if path.is_empty() { "/api/".to_owned() } else { format!("{path}/") });
        Ok(Self { base, http: http_client(timeout)?, credential: Mutex::new(None), session: Mutex::new(None), tenant: None })
    }

    /// The REST door's address (ending `/api/` for a server given by its root).
    pub fn url(&self) -> &str {
        self.base.as_str()
    }

    /// Works in `tenant` (sent as `X-Tenant`) instead of the credential's own.
    pub fn set_tenant(&mut self, tenant: Option<String>) {
        self.tenant = tenant.filter(|t| !t.trim().is_empty());
    }

    pub fn signed_in(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    fn endpoint(&self, path: &str) -> Url {
        self.base.join(path.trim_start_matches('/')).expect("a relative path")
    }

    fn token_request(&self, path: &str, auth: impl FnOnce(RequestBuilder) -> RequestBuilder) -> Result<Session> {
        let r = auth(self.http.post(self.endpoint(path))).send().map_err(|e| Error::new(ErrorKind::Transport, format!("{}: {e}", self.base)))?;
        let status = r.status().as_u16();
        let body = r.text().unwrap_or_default();
        if status != 200 {
            return Err(failure(status, &body, "signing in"));
        }
        let v: Value = serde_json::from_str(&body).map_err(|_| Error::new(ErrorKind::Protocol, format!("{} did not answer with JSON; is this a FileEngine server?", self.endpoint(path))))?;
        let token = field(&v, "token", "signing in")?;
        let life = v["expires_in"].as_u64().unwrap_or(900);
        // Renewed a minute early (or at half its life, for a short one).
        let early = 60.min(life / 2);
        Ok(Session { token, renew_at: Instant::now() + Duration::from_secs(life - early) })
    }

    /// Signs in with a service credential (`fesk_…` key id and its secret); who is signed in.
    pub fn sign_in(&self, key: &str, secret: &str) -> Result<Identity> {
        let s = self.token_request("v1/auth/token", |b| b.basic_auth(key.trim(), Some(secret.trim())))?;
        *self.session.lock().unwrap() = Some(s);
        *self.credential.lock().unwrap() = Some((key.trim().to_owned(), secret.trim().to_owned()));
        self.whoami()
    }

    /// Ends the session on the server and forgets the credential.
    pub fn sign_out(&self) -> Result<()> {
        let s = self.session.lock().unwrap().take();
        *self.credential.lock().unwrap() = None;
        if let Some(s) = s {
            let r = self.http.delete(self.endpoint("v1/auth/token")).bearer_auth(&s.token).send().map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))?;
            let status = r.status().as_u16();
            if !(200..300).contains(&status) && status != 401 {
                return Err(failure(status, &r.text().unwrap_or_default(), "signing out"));
            }
        }
        Ok(())
    }

    /// The bearer to send: renewed when due, a new one asked for when renewal is refused.
    fn bearer(&self, force_new: bool) -> Result<String> {
        let mut session = self.session.lock().unwrap();
        let due = session.as_ref().is_none_or(|s| force_new || Instant::now() >= s.renew_at);
        if !due {
            return Ok(session.as_ref().unwrap().token.clone());
        }
        if let Some(old) = session.as_ref().filter(|_| !force_new)
            && let Ok(s) = self.token_request("v1/auth/refresh", |b| b.bearer_auth(&old.token))
        {
            let t = s.token.clone();
            *session = Some(s);
            return Ok(t);
        }
        let Some((key, secret)) = self.credential.lock().unwrap().clone() else {
            *session = None;
            return Err(Error::new(ErrorKind::Auth, "not signed in to FileEngine"));
        };
        let s = self.token_request("v1/auth/token", |b| b.basic_auth(&key, Some(&secret)))?;
        let t = s.token.clone();
        *session = Some(s);
        Ok(t)
    }

    /// Sends a request with the bearer (and tenant); once more with a new bearer if refused.
    fn send(&self, what: &str, build: impl Fn() -> RequestBuilder) -> Result<Response> {
        let mut force = false;
        loop {
            let token = self.bearer(force)?;
            let mut b = build().bearer_auth(token);
            if let Some(t) = &self.tenant {
                b = b.header("X-Tenant", t);
            }
            let r = b.send().map_err(|e| Error::new(ErrorKind::Transport, format!("{what}: {e}")))?;
            let status = r.status().as_u16();
            if status == 401 && !force && self.credential.lock().unwrap().is_some() {
                force = true;
                continue;
            }
            if (200..300).contains(&status) {
                return Ok(r);
            }
            return Err(failure(status, &r.text().unwrap_or_default(), what));
        }
    }

    fn get_json(&self, path: &str, what: &str) -> Result<Value> {
        let r = self.send(what, || self.http.get(self.endpoint(path)))?;
        r.json().map_err(|_| Error::new(ErrorKind::Protocol, format!("{what}: the answer is not JSON")))
    }

    fn post_json(&self, path: &str, body: Value, what: &str) -> Result<Value> {
        let r = self.send(what, || self.http.post(self.endpoint(path)).json(&body))?;
        let text = r.text().unwrap_or_default();
        Ok(if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).map_err(|_| Error::new(ErrorKind::Protocol, format!("{what}: the answer is not JSON")))? })
    }

    /// Who is signed in, in which tenant, with which roles.
    pub fn whoami(&self) -> Result<Identity> {
        serde_json::from_value(self.get_json("v1/whoami", "who am I")?).map_err(|e| Error::new(ErrorKind::Protocol, format!("who am I: {e}")))
    }

    pub fn tenants(&self) -> Result<Tenants> {
        serde_json::from_value(self.get_json("v1/tenants", "the tenants")?).map_err(|e| Error::new(ErrorKind::Protocol, format!("the tenants: {e}")))
    }

    fn entries(v: Value, what: &str) -> Result<Vec<Node>> {
        serde_json::from_value(v["entries"].clone()).map_err(|e| Error::new(ErrorKind::Protocol, format!("{what}: {e}")))
    }

    /// A folder's contents; with `deleted`, its soft-deleted entries too.
    pub fn list(&self, dir: &str, deleted: bool) -> Result<Vec<Node>> {
        let q = if deleted { "?deleted=true" } else { "" };
        Self::entries(self.get_json(&format!("v1/dirs/{}{q}", percent(dir)), "listing the folder")?, "listing the folder")
    }

    /// A file's or folder's details, with its newest version.
    pub fn stat(&self, uid: &str) -> Result<Node> {
        serde_json::from_value(self.get_json(&format!("v1/nodes/{}", percent(uid)), "the file's details")?).map_err(|e| Error::new(ErrorKind::Protocol, format!("the file's details: {e}")))
    }

    pub fn exists(&self, uid: &str) -> Result<bool> {
        Ok(self.get_json(&format!("v1/nodes/{}/exists", percent(uid)), "checking the file")?["exists"] == true)
    }

    /// The entry called `name` in `dir` (not deleted), if any.
    pub fn find(&self, dir: &str, name: &str) -> Result<Option<Node>> {
        Ok(self.list(dir, false)?.into_iter().find(|n| n.name == name && !n.deleted))
    }

    /// The node at `path` (`Engineering/Reviera test/model.ifc`), from the top folder.
    pub fn resolve(&self, path: &str) -> Result<Node> {
        let mut node = Node { uid: ROOT.to_owned(), kind: Kind::Directory, ..Node::default() };
        for part in path.split('/').filter(|p| !p.is_empty()) {
            node = self.find(&node.uid, part)?.ok_or_else(|| Error::new(ErrorKind::NotFound, format!("{path}: there is no {part:?} in {}", if node.name.is_empty() { "the top folder" } else { &node.name })))?;
        }
        Ok(node)
    }

    /// Makes a folder in `parent`; its uid.
    pub fn create_dir(&self, parent: &str, name: &str) -> Result<String> {
        field(&self.post_json(&format!("v1/dirs/{}", percent(parent)), json!({ "name": name }), "making the folder")?, "uid", "making the folder")
    }

    /// The folder `name` in `parent`, made if it is not there; its uid.
    pub fn ensure_dir(&self, parent: &str, name: &str) -> Result<String> {
        match self.find(parent, name)? {
            Some(n) if n.is_dir() => Ok(n.uid),
            Some(_) => Err(Error::new(ErrorKind::Conflict, format!("{name:?} is a file, not a folder"))),
            None => self.create_dir(parent, name),
        }
    }

    /// Makes an empty file in `parent`; its uid.
    pub fn create_file(&self, parent: &str, name: &str) -> Result<String> {
        field(&self.post_json(&format!("v1/dirs/{}/files", percent(parent)), json!({ "name": name }), "making the file")?, "uid", "making the file")
    }

    pub fn rename(&self, uid: &str, new_name: &str) -> Result<()> {
        self.post_json(&format!("v1/nodes/{}/rename", percent(uid)), json!({ "new_name": new_name }), "renaming").map(drop)
    }

    pub fn move_to(&self, uid: &str, parent: &str) -> Result<()> {
        self.post_json(&format!("v1/nodes/{}/move", percent(uid)), json!({ "destination_parent_uid": parent }), "moving").map(drop)
    }

    pub fn copy_to(&self, uid: &str, parent: &str) -> Result<()> {
        self.post_json(&format!("v1/nodes/{}/copy", percent(uid)), json!({ "destination_parent_uid": parent }), "copying").map(drop)
    }

    /// Soft-deletes a file (it can be undeleted).
    pub fn delete_file(&self, uid: &str) -> Result<()> {
        self.send("deleting the file", || self.http.delete(self.endpoint(&format!("v1/files/{}", percent(uid))))).map(drop)
    }

    pub fn undelete(&self, uid: &str) -> Result<()> {
        self.post_json(&format!("v1/files/{}/undelete", percent(uid)), Value::Null, "undeleting").map(drop)
    }

    /// Soft-deletes a folder.
    pub fn delete_dir(&self, uid: &str) -> Result<()> {
        self.send("deleting the folder", || self.http.delete(self.endpoint(&format!("v1/dirs/{}", percent(uid))))).map(drop)
    }

    /// Streams the newest content into `out`; the bytes written.
    pub fn read_to(&self, uid: &str, out: &mut dyn Write) -> Result<u64> {
        let mut r = self.send("downloading", || self.http.get(self.endpoint(&format!("v1/files/{}/content", percent(uid)))))?;
        std::io::copy(&mut r, out).map_err(|e| Error::new(ErrorKind::Transport, format!("downloading: {e}")))
    }

    /// The newest content.
    pub fn read(&self, uid: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.read_to(uid, &mut out)?;
        Ok(out)
    }

    /// `len` bytes of the newest content from `start` (fewer at the end of the file).
    pub fn read_range(&self, uid: &str, start: u64, len: u64) -> Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let range = format!("bytes={start}-{}", start + len - 1);
        let mut r = self.send("downloading", || self.http.get(self.endpoint(&format!("v1/files/{}/content", percent(uid)))).header("Range", &range))?;
        let partial = r.status().as_u16() == 206;
        let mut out = Vec::new();
        r.read_to_end(&mut out).map_err(|e| Error::new(ErrorKind::Transport, format!("downloading: {e}")))?;
        if !partial {
            // A server that ignores Range sends it all.
            out = out.into_iter().skip(start as usize).take(len as usize).collect();
        }
        Ok(out)
    }

    /// Writes `bytes` as the file's new version.
    pub fn write(&self, uid: &str, bytes: &[u8]) -> Result<()> {
        let body = bytes.to_vec();
        self.send("uploading", || self.http.put(self.endpoint(&format!("v1/files/{}/content", percent(uid)))).header("Content-Type", "application/octet-stream").body(body.clone())).map(drop)
    }

    /// Writes the file at `path` as the new version, streamed from disk.
    pub fn write_file(&self, uid: &str, path: &std::path::Path) -> Result<()> {
        let io = |e: std::io::Error| Error::new(ErrorKind::Transport, format!("{}: {e}", path.display()));
        let len = std::fs::metadata(path).map_err(io)?.len();
        self.send("uploading", || {
            let file = std::fs::File::open(path).expect("checked above");
            self.http.put(self.endpoint(&format!("v1/files/{}/content", percent(uid)))).header("Content-Type", "application/octet-stream").body(reqwest::blocking::Body::sized(file, len))
        })
        .map(drop)
    }

    /// Writes `bytes` only if the file is still at `expected` (the version it was opened at, or
    /// empty for a file not written yet); the new version. [`ErrorKind::Conflict`] names the
    /// version someone else saved.
    pub fn save_checked(&self, uid: &str, bytes: &[u8], expected: &str) -> Result<String> {
        let now = self.stat(uid)?;
        if now.version != expected {
            let by = self.versions(uid).ok().and_then(|v| v.into_iter().next()).map(|v| v.revised_by).filter(|b| !b.is_empty());
            return Err(Error {
                kind: ErrorKind::Conflict,
                status: None,
                message: format!("the file was saved by {} since it was opened (version {}; opened at {})", by.as_deref().unwrap_or("someone else"), now.version, if expected.is_empty() { "none" } else { expected }),
            });
        }
        self.write(uid, bytes)?;
        Ok(self.stat(uid)?.version)
    }

    /// The file's versions, newest first.
    pub fn versions(&self, uid: &str) -> Result<Vec<Version>> {
        let v = self.get_json(&format!("v1/files/{}/versions", percent(uid)), "the versions")?;
        if let Some(entries) = v["entries"].as_array() {
            return serde_json::from_value(Value::Array(entries.clone())).map_err(|e| Error::new(ErrorKind::Protocol, format!("the versions: {e}")));
        }
        // An older door lists the timestamps only.
        Ok(v["versions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|t| Version { version: t.to_owned(), revised_by: String::new() }).collect())
    }

    /// One version's content.
    pub fn read_version(&self, uid: &str, version: &str) -> Result<Vec<u8>> {
        let mut r = self.send("downloading the version", || self.http.get(self.endpoint(&format!("v1/files/{}/versions/{}", percent(uid), percent(version)))))?;
        let mut out = Vec::new();
        r.read_to_end(&mut out).map_err(|e| Error::new(ErrorKind::Transport, format!("downloading the version: {e}")))?;
        Ok(out)
    }

    /// Makes `version`'s content the newest again (as a new version); the version restored.
    pub fn restore(&self, uid: &str, version: &str) -> Result<String> {
        let v = self.post_json(&format!("v1/files/{}/restore", percent(uid)), json!({ "version_timestamp": version }), "restoring the version")?;
        Ok(v["restored_version"].as_str().unwrap_or(version).to_owned())
    }

    /// The hidden renditions (PDF, preview, thumbnail) the server made of a file.
    pub fn renditions(&self, uid: &str) -> Result<Vec<Node>> {
        Self::entries(self.get_json(&format!("v1/files/{}/renditions", percent(uid)), "the renditions")?, "the renditions")
    }
    // ---- The conversion service's headless routes (`/csai/`, beside the REST door) ----

    fn csai(&self, path: &str) -> Url {
        // The service sits beside the door: `<server>/csai/…` for a door at `<server>/api/`.
        self.base.join("../csai/").and_then(|b| b.join(path.trim_start_matches('/'))).expect("a relative path")
    }

    /// Reads values out of documents (`POST /csai/v1/extract`, answered as one JSON object):
    /// `body` holds `files` or `folder_uid`, the JSON Schema of the values wanted, and optionally
    /// `instructions` and `cache`. The answer has `run_id`, `plan`, `items` and `summary`. An
    /// extraction reads each document whole, so it may take minutes: it waits up to `timeout`.
    pub fn extract(&self, body: &Value, timeout: Duration) -> Result<Value> {
        let r = self.send("extracting from the documents", || self.http.post(self.csai("v1/extract")).header("Accept", "application/json").timeout(timeout).json(body))?;
        r.json().map_err(|_| Error::new(ErrorKind::Protocol, "extracting from the documents: the answer is not JSON"))
    }

    /// The key's usage of the conversion service (`GET /csai/v1/usage`): `state` (ok, warning,
    /// limited), `limits` and a `message`.
    pub fn usage(&self) -> Result<Value> {
        let r = self.send("the usage", || self.http.get(self.csai("v1/usage")))?;
        r.json().map_err(|_| Error::new(ErrorKind::Protocol, "the usage: the answer is not JSON"))
    }

    // ---- The discussion service (`/discuss/`, beside the REST door): comments and issues ----

    fn discuss(&self, path: &str) -> Url {
        self.base.join("../discuss/").and_then(|b| b.join(path.trim_start_matches('/'))).expect("a relative path")
    }

    fn discuss_json(&self, what: &str, build: impl Fn() -> RequestBuilder) -> Result<Value> {
        let r = self.send(what, build)?;
        r.json().map_err(|_| Error::new(ErrorKind::Protocol, format!("{what}: the answer is not JSON")))
    }

    /// A file's discussion threads (`GET /discuss/files/{uid}/threads`), each with its comments
    /// and, for an issue on a model, its `anchor` (`kind: model-viewpoint`, a BCF-shaped
    /// `viewpoint`, `object_refs`, `measurements`).
    pub fn threads(&self, file_uid: &str) -> Result<Vec<Value>> {
        let v = self.discuss_json("the file's threads", || self.http.get(self.discuss(&format!("files/{}/threads", percent(file_uid)))))?;
        Ok(v["threads"].as_array().cloned().unwrap_or_default())
    }

    /// Opens a thread on a file: `title`, the opening comment's `body`, the `version` it was
    /// raised on, and an `anchor` (a model viewpoint). Returns the thread.
    pub fn open_thread(&self, file_uid: &str, body: &Value) -> Result<Value> {
        self.discuss_json("opening the thread", || self.http.post(self.discuss(&format!("files/{}/threads", percent(file_uid)))).json(body))
    }

    /// Adds a comment to a thread. Returns the comment.
    pub fn add_comment(&self, thread_id: &str, text: &str) -> Result<Value> {
        let body = json!({ "body": text });
        self.discuss_json("commenting", || self.http.post(self.discuss(&format!("threads/{}/comments", percent(thread_id)))).json(&body))
    }

    /// Resolves a thread (`resolved`, with the version it was resolved on) or opens it again
    /// (`open`). Returns the thread.
    pub fn set_thread_status(&self, thread_id: &str, status: &str, version: Option<&str>) -> Result<Value> {
        let body = json!({ "status": status, "resolved_version": version });
        self.discuss_json("setting the thread's status", || self.http.patch(self.discuss(&format!("threads/{}", percent(thread_id)))).json(&body))
    }


    pub fn metadata(&self, uid: &str) -> Result<BTreeMap<String, String>> {
        let v = self.get_json(&format!("v1/nodes/{}/metadata", percent(uid)), "the metadata")?;
        Ok(v["metadata"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_owned))).collect())
    }

    pub fn set_metadata(&self, uid: &str, key: &str, value: &str) -> Result<()> {
        let body = json!({ "value": value });
        self.send("setting the metadata", || self.http.put(self.endpoint(&format!("v1/nodes/{}/metadata/{}", percent(uid), percent(key)))).json(&body)).map(drop)
    }

    pub fn delete_metadata(&self, uid: &str, key: &str) -> Result<()> {
        self.send("deleting the metadata", || self.http.delete(self.endpoint(&format!("v1/nodes/{}/metadata/{}", percent(uid), percent(key))))).map(drop)
    }

    /// Whether the signed-in user holds `permission` (`r`, `w`, `d` … or `READ`, `WRITE` …).
    pub fn can(&self, uid: &str, permission: &str) -> Result<bool> {
        Ok(self.get_json(&format!("v1/nodes/{}/permissions?permission={}", percent(uid), percent(permission)), "the permission")?["has_permission"] == true)
    }
}
