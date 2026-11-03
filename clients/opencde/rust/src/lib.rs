// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! A blocking client for buildingSMART's BCF API (3.0, with 2.1 as the fallback): the issue
//! servers of coordination platforms (OpenCDE). It reads and writes projects, topics, comments,
//! viewpoints and snapshots, as JSON values shaped as the standard has them.
//!
//! - **Address.** The BCF API's base, the part before the version (`https://host/bcf`, or
//!   `https://host/api/v1/bcf` on OpenConstructionERP). [`Client::connect`] finds it from a
//!   server's root too: the Foundation API's `foundation/versions` when the server has one, else
//!   the usual places, and settles the version (3.0 when offered, else 2.1).
//! - **Signing in.** A bearer token given as it is (from the server's web pages or an OAuth
//!   flow done elsewhere); a user name and password sent as HTTP Basic; or a user name (e-mail)
//!   and password posted as JSON to a login address that answers `access_token` (and maybe
//!   `refresh_token` and `expires_in`), as OpenConstructionERP's does. A token that runs out is
//!   renewed, or asked for again.
//! - **Leniency.** Lists come as arrays (the standard) or wrapped as `{"items": […]}`; paging
//!   (`$top`, `$skip`) is used where the server takes it.
//!
//! No credential is written anywhere by this crate.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Serialize;
use serde_json::{Value, json};

#[cfg(feature = "fake")]
pub mod fake;

/// The versions this client speaks, preferred first.
pub const VERSIONS: &[&str] = &["3.0", "2.1"];

/// Where OpenConstructionERP signs in, from the server's root.
pub const OPENCONSTRUCTIONERP_LOGIN: &str = "api/v1/users/auth/login/";

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The URL is not one the client will use.
    Url,
    /// The connection failed, or the server answered with an unexpected HTTP error.
    Transport,
    /// Not signed in, or the credential was refused.
    Auth,
    /// The server refused the operation for this user (HTTP 403).
    Forbidden,
    /// No such project, topic or viewpoint (HTTP 404).
    NotFound,
    /// The topic changed meanwhile (HTTP 409, 412).
    Conflict,
    /// The server refused the request as invalid (HTTP 400, 422), or failed (HTTP 5xx).
    Server,
    /// The answer is not what the BCF API answers.
    Protocol,
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub kind: ErrorKind,
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

/// How to sign in.
#[derive(Clone, Default)]
pub enum Auth {
    /// No credential (a server open to all, or one behind a trusted proxy).
    #[default]
    None,
    /// A bearer token, sent as it is.
    Token(String),
    /// HTTP Basic.
    Basic { user: String, password: String },
    /// A user name and password posted as JSON (`{"email", "password"}`) to `url` (relative to
    /// the server's root, or absolute), which answers a bearer as `access_token`.
    Login { url: String, user: String, password: String },
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Auth::None => "None",
            Auth::Token(_) => "Token(…)",
            Auth::Basic { .. } => "Basic(…)",
            Auth::Login { .. } => "Login(…)",
        })
    }
}

struct Session {
    token: String,
    refresh: Option<String>,
    renew_at: Option<Instant>,
}

/// One BCF API server, and the session on it.
pub struct Client {
    /// The BCF API's base, ending `/` (without the version).
    base: Url,
    version: String,
    http: reqwest::blocking::Client,
    auth: Auth,
    session: Mutex<Option<Session>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base.as_str()).field("version", &self.version).finish_non_exhaustive()
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
        s => return Err(Error::new(ErrorKind::Url, format!("{s}:// is not a BCF API address; use https://"))),
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

/// The error an HTTP answer stands for, with the server's own words when it gave them
/// (`message`, `detail`, `error`).
fn failure(status: u16, body: &str, what: &str) -> Error {
    let v = serde_json::from_str::<Value>(body).ok();
    let said = v.as_ref().and_then(|v| {
        ["message", "detail", "error"].iter().find_map(|k| match &v[*k] {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Null => None,
            other if !other.is_object() || !other.as_object().is_some_and(|o| o.is_empty()) => Some(other.to_string()),
            _ => None,
        })
    });
    let kind = match status {
        401 => ErrorKind::Auth,
        403 => ErrorKind::Forbidden,
        404 => ErrorKind::NotFound,
        409 | 412 => ErrorKind::Conflict,
        400 | 422 | 500..=599 => ErrorKind::Server,
        _ => ErrorKind::Transport,
    };
    let message = match (kind, said) {
        (ErrorKind::Auth, s) => format!("the BCF server refused the sign-in{}", s.map(|s| format!(": {s}")).unwrap_or_default()),
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

/// A list as the standard gives it (an array) or wrapped (`items`, `value`, `data`).
pub fn items(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a,
        Value::Object(mut o) => ["items", "value", "data", "topics", "comments", "viewpoints", "projects"]
            .iter()
            .find_map(|k| match o.remove(*k) {
                Some(Value::Array(a)) => Some(a),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn with_slash(mut u: Url) -> Url {
    let p = u.path().trim_end_matches('/').to_owned();
    u.set_path(&format!("{p}/"));
    u.set_query(None);
    u
}

/// `url` without a trailing version segment (`…/bcf/3.0` → `…/bcf/`), and that version.
fn split_version(u: &Url) -> (Url, Option<String>) {
    let path = u.path().trim_end_matches('/');
    match path.rsplit_once('/') {
        Some((head, last)) if VERSIONS.contains(&last) => {
            let mut b = u.clone();
            b.set_path(&format!("{head}/"));
            (b, Some(last.to_owned()))
        }
        _ => (with_slash(u.clone()), None),
    }
}

impl Client {
    /// A client for the BCF API at `base` (without the version) speaking `version`, 60 s per call.
    pub fn new(base: &str, version: &str, auth: Auth, allow_http: bool) -> Result<Self> {
        let (b, v) = split_version(&check_url(base, allow_http)?);
        let version = v.unwrap_or_else(|| version.to_owned());
        if !VERSIONS.contains(&version.as_str()) {
            return Err(Error::new(ErrorKind::Url, format!("BCF API {version} is not one Reviera speaks (3.0, 2.1)")));
        }
        Ok(Self { base: b, version, http: http_client(Duration::from_secs(60))?, auth, session: Mutex::new(None) })
    }

    /// Finds the BCF API from `url` (its base, a versioned base, or the server's root), signs in
    /// and settles the version: the newest both sides speak.
    pub fn connect(url: &str, auth: Auth, allow_http: bool) -> Result<Self> {
        let given = check_url(url, allow_http)?;
        let (given_base, given_version) = split_version(&given);
        let probe = Self { base: given_base.clone(), version: VERSIONS[0].into(), http: http_client(Duration::from_secs(30))?, auth: auth.clone(), session: Mutex::new(None) };
        // The Foundation API names the BCF API's address and versions.
        let mut candidates: Vec<(Url, Vec<String>)> = Vec::new();
        for root in [given_base.clone(), probe.origin()] {
            if let Ok(found) = probe.foundation_versions(&root) {
                candidates.extend(found);
            }
        }
        // Otherwise the usual places.
        let versions: Vec<String> = given_version.clone().map_or_else(|| VERSIONS.iter().map(|v| (*v).to_owned()).collect(), |v| vec![v]);
        for rel in ["", "bcf/", "api/v1/bcf/", "api/bcf/"] {
            if let Ok(u) = given_base.join(rel) {
                candidates.push((u, versions.clone()));
            }
        }
        let mut last = None;
        for (base, versions) in candidates {
            for v in versions.iter().filter(|v| VERSIONS.contains(&v.as_str())) {
                let c = Self { base: base.clone(), version: v.clone(), http: probe.http.clone(), auth: auth.clone(), session: Mutex::new(None) };
                match c.current_user() {
                    Ok(_) => return Ok(Self { http: http_client(Duration::from_secs(60))?, ..c }),
                    // There, but the sign-in failed: say so rather than look further.
                    Err(e) if matches!(e.kind, ErrorKind::Auth | ErrorKind::Forbidden) => return Err(e),
                    Err(e) => last = Some(e),
                }
            }
        }
        Err(Error::new(
            ErrorKind::NotFound,
            format!("no BCF API (3.0 or 2.1) was found at {url}{}", last.map(|e| format!(" ({})", e.message)).unwrap_or_default()),
        ))
    }

    /// `foundation/versions` at `root`: the BCF API's bases and their versions.
    fn foundation_versions(&self, root: &Url) -> Result<Vec<(Url, Vec<String>)>> {
        let u = root.join("foundation/versions").map_err(|e| Error::new(ErrorKind::Url, e.to_string()))?;
        let r = self.http.get(u).send().map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))?;
        if !r.status().is_success() {
            return Err(Error::new(ErrorKind::NotFound, "no Foundation API"));
        }
        let v: Value = r.json().map_err(|_| Error::new(ErrorKind::Protocol, "not JSON"))?;
        let mut out: Vec<(Url, Vec<String>)> = Vec::new();
        let mut vs: Vec<&Value> = v["versions"].as_array().into_iter().flatten().filter(|e| e["api_id"] == "bcf").collect();
        vs.sort_by_key(|e| std::cmp::Reverse(e["version_id"].as_str().unwrap_or("").to_owned()));
        for e in vs {
            let Some(version) = e["version_id"].as_str() else { continue };
            let base = match e["api_base_url"].as_str().and_then(|s| Url::parse(s).ok()) {
                Some(b) => split_version(&b).0,
                None => root.join("bcf/").map_err(|e| Error::new(ErrorKind::Url, e.to_string()))?,
            };
            match out.iter_mut().find(|(b, _)| *b == base) {
                Some((_, list)) => list.push(version.to_owned()),
                None => out.push((base, vec![version.to_owned()])),
            }
        }
        Ok(out)
    }

    fn origin(&self) -> Url {
        let mut u = self.base.clone();
        u.set_path("/");
        u
    }

    /// The BCF API's base (without the version).
    pub fn base_url(&self) -> &str {
        self.base.as_str()
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    fn endpoint(&self, path: &str) -> Url {
        self.base.join(&format!("{}/{}", self.version, path.trim_start_matches('/'))).expect("a relative path")
    }

    /// Signs in with the login address, or renews the token with the refresh token.
    fn login(&self, refresh: Option<&str>) -> Result<Session> {
        let Auth::Login { url, user, password } = &self.auth else { unreachable!("only login sessions are asked for") };
        let login = Url::parse(url).or_else(|_| self.origin().join(url.trim_start_matches('/'))).map_err(|e| Error::new(ErrorKind::Url, format!("the login address {url:?}: {e}")))?;
        let (target, body) = match refresh {
            Some(t) => (login.join("../refresh/").unwrap_or(login.clone()), json!({ "refresh_token": t })),
            None => (login, json!({ "email": user.trim(), "username": user.trim(), "password": password })),
        };
        let r = self.http.post(target.clone()).json(&body).send().map_err(|e| Error::new(ErrorKind::Transport, format!("{target}: {e}")))?;
        let status = r.status().as_u16();
        let text = r.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            let mut e = failure(status, &text, "signing in");
            if matches!(status, 400 | 401 | 403 | 422) {
                e.kind = ErrorKind::Auth;
            }
            return Err(e);
        }
        let v: Value = serde_json::from_str(&text).map_err(|_| Error::new(ErrorKind::Protocol, format!("{target} did not answer with JSON")))?;
        let token = ["access_token", "token", "accessToken"]
            .iter()
            .find_map(|k| v[*k].as_str())
            .ok_or_else(|| Error::new(ErrorKind::Protocol, "signing in: the answer has no access_token"))?
            .to_owned();
        let renew_at = v["expires_in"].as_u64().map(|life| Instant::now() + Duration::from_secs(life - 60.min(life / 2)));
        Ok(Session { token, refresh: v["refresh_token"].as_str().map(str::to_owned), renew_at })
    }

    /// The Authorization to send; `force_new` after the server refused the last one.
    fn authorise(&self, b: RequestBuilder, force_new: bool) -> Result<RequestBuilder> {
        Ok(match &self.auth {
            Auth::None => b,
            Auth::Token(t) => b.bearer_auth(t.trim()),
            Auth::Basic { user, password } => b.basic_auth(user.trim(), Some(password)),
            Auth::Login { .. } => {
                let mut session = self.session.lock().unwrap();
                let due = session.as_ref().is_none_or(|s| force_new || s.renew_at.is_some_and(|t| Instant::now() >= t));
                if due {
                    let renewed = session.as_ref().filter(|_| !force_new).and_then(|s| s.refresh.clone()).and_then(|r| self.login(Some(&r)).ok());
                    *session = Some(match renewed {
                        Some(s) => s,
                        None => self.login(None)?,
                    });
                }
                b.bearer_auth(&session.as_ref().expect("signed in").token)
            }
        })
    }

    /// Sends a request; once more with a new token if refused.
    fn send(&self, what: &str, build: impl Fn() -> RequestBuilder) -> Result<Response> {
        let mut force = false;
        loop {
            let b = self.authorise(build(), force)?;
            let r = b.send().map_err(|e| Error::new(ErrorKind::Transport, format!("{what}: {e}")))?;
            let status = r.status().as_u16();
            if status == 401 && !force && matches!(self.auth, Auth::Login { .. }) {
                force = true;
                continue;
            }
            if (200..300).contains(&status) {
                return Ok(r);
            }
            return Err(failure(status, &r.text().unwrap_or_default(), what));
        }
    }

    fn json_of(r: Response, what: &str) -> Result<Value> {
        let text = r.text().unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|_| Error::new(ErrorKind::Protocol, format!("{what}: the answer is not JSON")))
    }

    fn get(&self, path: &str, what: &str) -> Result<Value> {
        Self::json_of(self.send(what, || self.http.get(self.endpoint(path)))?, what)
    }

    fn post(&self, path: &str, body: &Value, what: &str) -> Result<Value> {
        Self::json_of(self.send(what, || self.http.post(self.endpoint(path)).json(body))?, what)
    }

    fn put(&self, path: &str, body: &Value, what: &str) -> Result<Value> {
        Self::json_of(self.send(what, || self.http.put(self.endpoint(path)).json(body))?, what)
    }

    /// Who is signed in (`id`, `name`).
    pub fn current_user(&self) -> Result<Value> {
        self.get("current-user", "who is signed in")
    }

    /// The projects the user may see (`project_id`, `name`, `authorization`).
    pub fn projects(&self) -> Result<Vec<Value>> {
        Ok(items(self.get("projects", "the projects")?))
    }

    pub fn project(&self, project: &str) -> Result<Value> {
        self.get(&format!("projects/{}", percent(project)), "the project")
    }

    /// The project's lists: `topic_type`, `topic_status`, `priority`, `topic_label`, `stage`,
    /// `users` (3.0: `user_id_type`), and the actions allowed.
    pub fn extensions(&self, project: &str) -> Result<Value> {
        self.get(&format!("projects/{}/extensions", percent(project)), "the project's lists")
    }

    /// Every topic of the project, a page at a time where the server pages.
    pub fn topics(&self, project: &str) -> Result<Vec<Value>> {
        const PAGE: usize = 500;
        let path = format!("projects/{}/topics", percent(project));
        let mut out = Vec::new();
        loop {
            let mut u = self.endpoint(&path);
            u.query_pairs_mut().append_pair("$top", &PAGE.to_string()).append_pair("$skip", &out.len().to_string());
            let page = match self.send("the topics", || self.http.get(u.clone())) {
                Ok(r) => items(Self::json_of(r, "the topics")?),
                // A server that does not page.
                Err(e) if e.kind == ErrorKind::Server && out.is_empty() => return Ok(items(self.get(&path, "the topics")?)),
                Err(e) => return Err(e),
            };
            let n = page.len();
            // A server that ignores paging answers everything at once (and again for the next
            // page): stop at the first repeat.
            let repeat = page.first().is_some_and(|t| out.iter().any(|o: &Value| o["guid"] == t["guid"]));
            if repeat {
                break;
            }
            out.extend(page);
            if n < PAGE {
                break;
            }
        }
        Ok(out)
    }

    pub fn topic(&self, project: &str, topic: &str) -> Result<Value> {
        self.get(&format!("projects/{}/topics/{}", percent(project), percent(topic)), "the topic")
    }

    /// Creates a topic (`title`, `topic_type`, `topic_status`, … and, 3.0, `guid`). Returns it.
    pub fn create_topic(&self, project: &str, body: &Value) -> Result<Value> {
        self.post(&format!("projects/{}/topics", percent(project)), body, "creating the topic")
    }

    /// Changes a topic: the fields given. BCF's PUT replaces the topic, so the fields not given
    /// are taken from the server's copy first. Returns it.
    pub fn update_topic(&self, project: &str, topic: &str, fields: &Value) -> Result<Value> {
        let mut body = self.topic(project, topic)?;
        if let (Some(b), Some(f)) = (body.as_object_mut(), fields.as_object()) {
            for k in ["guid", "server_assigned_id", "creation_author", "creation_date", "modified_author", "modified_date", "authorization", "index"] {
                b.remove(k);
            }
            for (k, v) in f {
                b.insert(k.clone(), v.clone());
            }
            b.retain(|_, v| !v.is_null());
        }
        self.put(&format!("projects/{}/topics/{}", percent(project), percent(topic)), &body, "changing the topic")
    }

    pub fn delete_topic(&self, project: &str, topic: &str) -> Result<()> {
        self.send("deleting the topic", || self.http.delete(self.endpoint(&format!("projects/{}/topics/{}", percent(project), percent(topic))))).map(drop)
    }

    pub fn comments(&self, project: &str, topic: &str) -> Result<Vec<Value>> {
        Ok(items(self.get(&format!("projects/{}/topics/{}/comments", percent(project), percent(topic)), "the comments")?))
    }

    /// Adds a comment (`comment`, optionally `viewpoint_guid`, and 3.0 `guid`). Returns it.
    pub fn add_comment(&self, project: &str, topic: &str, body: &Value) -> Result<Value> {
        self.post(&format!("projects/{}/topics/{}/comments", percent(project), percent(topic)), body, "commenting")
    }

    pub fn viewpoints(&self, project: &str, topic: &str) -> Result<Vec<Value>> {
        Ok(items(self.get(&format!("projects/{}/topics/{}/viewpoints", percent(project), percent(topic)), "the viewpoints")?))
    }

    /// Adds a viewpoint (BCF API JSON, a `snapshot` of `snapshot_type` and base64
    /// `snapshot_data` beside it). Returns it.
    pub fn add_viewpoint(&self, project: &str, topic: &str, body: &Value) -> Result<Value> {
        self.post(&format!("projects/{}/topics/{}/viewpoints", percent(project), percent(topic)), body, "adding the viewpoint")
    }

    /// A viewpoint's snapshot (PNG or JPEG), or `None` when it has none.
    pub fn snapshot(&self, project: &str, topic: &str, viewpoint: &str) -> Result<Option<Vec<u8>>> {
        let path = format!("projects/{}/topics/{}/viewpoints/{}/snapshot", percent(project), percent(topic), percent(viewpoint));
        match self.send("the snapshot", || self.http.get(self.endpoint(&path))) {
            Ok(r) => Ok(Some(r.bytes().map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))?.to_vec()).filter(|b| !b.is_empty())),
            Err(e) if e.kind == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_come_off_the_base() {
        let (b, v) = split_version(&Url::parse("https://erp.example.com/api/v1/bcf/3.0/").unwrap());
        assert_eq!((b.as_str(), v.as_deref()), ("https://erp.example.com/api/v1/bcf/", Some("3.0")));
        let (b, v) = split_version(&Url::parse("https://erp.example.com/bcf").unwrap());
        assert_eq!((b.as_str(), v), ("https://erp.example.com/bcf/", None));
    }

    #[test]
    fn lists_bare_or_wrapped() {
        assert_eq!(items(json!([1, 2])).len(), 2);
        assert_eq!(items(json!({ "items": [1] })).len(), 1);
        assert!(items(json!({ "other": 1 })).is_empty());
    }

    #[test]
    fn plain_http_only_on_this_computer() {
        assert!(check_url("http://erp.example.com/bcf", false).is_err());
        assert!(check_url("http://localhost:8080/bcf", false).is_ok());
        assert!(check_url("https://erp.example.com/bcf", false).is_ok());
    }
}
