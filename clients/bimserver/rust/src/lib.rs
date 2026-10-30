// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! A client for the Open Source BIMserver's JSON API (TECH_STACK §3.7.1, spec Ch 6 §6.2).
//!
//! Every call is an HTTP `POST` to `<server>/json` of
//! `{"token": …, "request": {"interface": …, "method": …, "parameters": {…}}}`, answered by
//! `{"response": {"result": …}}` or `{"response": {"exception": {"__type": …, "message": …}}}`.
//! Downloads are fetched from `<server>/download?token=…&topicId=…`.
//!
//! - **Login** by user name and password (`AuthInterface.login`) or by a user's token
//!   (`AuthInterface.loginUserToken`); the session token is kept on the client and sent with
//!   every later call. An expired or refused token is [`ErrorKind::Auth`], so the caller can log
//!   in again with what it has stored.
//! - **TLS** 1.2 or newer, verified against the platform's certificate store. Plain `http://` is
//!   accepted for `localhost` and loopback addresses only, unless the caller allows it.
//! - The calls used are BIMserver 1.5's, checked against 1.6.1: `AdminInterface.getServerInfo`,
//!   `AuthInterface.getLoggedInUser`, `ServiceInterface.getAllProjects`, `getProjectByPoid`,
//!   `getAllRevisionsOfProject`, `getUserByUoid`, `addProject`, `addProjectAsSubProject`,
//!   `getSuggestedDeserializerForExtension`, `getSerializerByName`, `checkinSync`, `download`
//!   and `cleanupLongAction`.
//!
//! This crate is MIT licensed (Reviera's interface components are), and talks to BIMserver
//! (AGPL-3.0) over the network only.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The URL is not one the client will use.
    Url,
    /// The connection or TLS failed, or the server answered with an HTTP error.
    Transport,
    /// Not logged in, or the token was refused (expired, logged out, wrong credentials).
    Auth,
    /// The server refused the request (`UserException`, `ServerException`, …).
    Server,
    /// The answer was not what the API describes.
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// The server's exception type (`UserException` …), when it gave one.
    pub exception: Option<String>,
    pub message: String,
}

impl Error {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, exception: None, message: message.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The server's state and version (`AdminInterface.getServerInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// RUNNING, NOT_SETUP, MIGRATION_REQUIRED, …
    pub state: String,
    /// The full version, e.g. `1.5.188` or `1.6.1-SNAPSHOT`.
    pub version: String,
}

/// A user (`SUser`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub oid: i64,
    pub username: String,
    pub name: String,
    /// ADMIN, USER, READ_ONLY, ANONYMOUS, SYSTEM.
    pub user_type: String,
}

/// A project (`SProject`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub oid: i64,
    pub name: String,
    /// `ifc2x3tc1`, `ifc4` …
    pub schema: String,
    /// The parent project's oid, for a sub-project.
    pub parent: Option<i64>,
    pub sub_projects: Vec<i64>,
    /// The latest revision's oid, if it has any.
    pub last_revision: Option<i64>,
    pub revisions: Vec<i64>,
}

/// A revision (`SRevision`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub oid: i64,
    /// Its number in the project, from 1.
    pub number: i64,
    pub comment: String,
    /// Milliseconds since the Unix epoch.
    pub date: i64,
    /// The user who checked it in (oid).
    pub user: i64,
    /// Objects in the revision.
    pub size: i64,
    pub tag: Option<String>,
}

/// A plug-in configuration: a serialiser or a deserialiser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub oid: i64,
    pub name: String,
}

/// What a check-in reported (`SLongCheckinActionState`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckIn {
    /// The new revision.
    pub roid: i64,
    pub warnings: Vec<String>,
}

/// Extended data attached to a revision (`SExtendedData`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtendedData {
    pub oid: i64,
    pub schema: i64,
    pub file: i64,
    pub title: String,
    /// Milliseconds since the Unix epoch.
    pub added: i64,
    pub user: i64,
}

/// Whether `url` may be used: https, or http for loopback (or anything, when `allow_http`).
pub fn check_url(url: &str, allow_http: bool) -> Result<Url> {
    let u = Url::parse(url.trim()).map_err(|e| Error::new(ErrorKind::Url, format!("{url:?} is not a URL: {e}")))?;
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"));
    match u.scheme() {
        "https" => {}
        "http" if loopback || allow_http => {}
        "http" => return Err(Error::new(ErrorKind::Url, "use https:// for a server that is not on this computer (plain http is for localhost)")),
        s => return Err(Error::new(ErrorKind::Url, format!("{s}:// is not a BIMserver address; use https://"))),
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

/// One server, and the session on it.
pub struct Client {
    base: Url,
    http: reqwest::blocking::Client,
    token: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base.as_str()).field("logged_in", &self.token.is_some()).finish()
    }
}

fn int(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(Value::as_i64).unwrap_or(-1)
}

fn text(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn ids(v: &Value, k: &str) -> Vec<i64> {
    v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

fn project_of(v: &Value) -> Project {
    let some = |k: &str| Some(int(v, k)).filter(|o| *o > 0);
    Project { oid: int(v, "oid"), name: text(v, "name"), schema: text(v, "schema"), parent: some("parentId"), sub_projects: ids(v, "subProjects"), last_revision: some("lastRevisionId"), revisions: ids(v, "revisions") }
}

fn revision_of(v: &Value) -> Revision {
    Revision { oid: int(v, "oid"), number: int(v, "id"), comment: text(v, "comment"), date: int(v, "date"), user: int(v, "userId"), size: int(v, "size"), tag: v.get("tag").and_then(Value::as_str).map(str::to_owned) }
}

fn plugin_of(v: &Value) -> Plugin {
    Plugin { oid: int(v, "oid"), name: text(v, "name") }
}

/// A login the server refused (BIMserver says so with a `UserException`) is an authentication
/// failure.
fn refused(e: Error) -> Error {
    if e.kind == ErrorKind::Server { Error { kind: ErrorKind::Auth, ..e } } else { e }
}

impl Client {
    /// A client for the server at `url` (its root, where `/json` is), 60 s per call.
    pub fn new(url: &str, allow_http: bool) -> Result<Self> {
        Self::with_timeout(url, allow_http, Duration::from_secs(60))
    }

    pub fn with_timeout(url: &str, allow_http: bool, timeout: Duration) -> Result<Self> {
        let mut base = check_url(url, allow_http)?;
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        Ok(Self { base, http: http_client(timeout)?, token: None })
    }

    pub fn url(&self) -> &str {
        self.base.as_str()
    }

    pub fn logged_in(&self) -> bool {
        self.token.is_some()
    }

    /// Forgets the session without telling the server.
    pub fn forget(&mut self) {
        self.token = None;
    }

    fn endpoint(&self, path: &str) -> Url {
        self.base.join(path).expect("a relative path")
    }

    fn post(&self, body: &Value) -> Result<Value> {
        // ngrok's free tunnels show a browser warning page unless this header is sent; other
        // servers ignore it.
        let r = self
            .http
            .post(self.endpoint("json"))
            .header("ngrok-skip-browser-warning", "1")
            .json(body)
            .send()
            .map_err(|e| Error::new(ErrorKind::Transport, format!("{}: {e}", self.base)))?;
        let status = r.status();
        let text = r.text().map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))?;
        if !status.is_success() {
            return Err(Error::new(ErrorKind::Transport, format!("{}json answered {status}", self.base)));
        }
        serde_json::from_str(&text).map_err(|_| Error::new(ErrorKind::Protocol, format!("{}json did not answer with JSON; is this a BIMserver?", self.base)))
    }

    /// Calls `interface.method` with `parameters`; the result, or the server's exception.
    pub fn call(&self, interface: &str, method: &str, parameters: Value) -> Result<Value> {
        let mut body = json!({ "request": { "interface": interface, "method": method, "parameters": parameters } });
        if let Some(t) = &self.token {
            body["token"] = json!(t);
        }
        let v = self.post(&body)?;
        let r = v.get("response").ok_or_else(|| Error::new(ErrorKind::Protocol, "the answer has no response"))?;
        if let Some(x) = r.get("exception") {
            let ty = text(x, "__type");
            let message = x.get("message").and_then(Value::as_str).unwrap_or("the server refused the request").to_owned();
            let kind = if ty == "AuthenticationException" || (ty == "AccessDeniedException" && self.token.is_none()) { ErrorKind::Auth } else { ErrorKind::Server };
            return Err(Error { kind, exception: Some(ty), message });
        }
        Ok(r.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Logs in with a user name and password.
    pub fn login(&mut self, username: &str, password: &str) -> Result<()> {
        self.token = None;
        let t = self.call("AuthInterface", "login", json!({ "username": username, "password": password })).map_err(refused)?;
        self.token = Some(t.as_str().filter(|t| !t.is_empty()).ok_or_else(|| Error::new(ErrorKind::Protocol, "the login gave no token"))?.to_owned());
        Ok(())
    }

    /// Logs in with a user's token (as BIMserver shows on the user's page).
    pub fn login_user_token(&mut self, token: &str) -> Result<()> {
        self.token = None;
        let t = self.call("AuthInterface", "loginUserToken", json!({ "token": token })).map_err(refused)?;
        self.token = Some(t.as_str().filter(|t| !t.is_empty()).ok_or_else(|| Error::new(ErrorKind::Protocol, "the login gave no token"))?.to_owned());
        Ok(())
    }

    /// Ends the session on the server.
    pub fn logout(&mut self) -> Result<()> {
        if self.token.is_some() {
            let r = self.call("AuthInterface", "logout", json!({}));
            self.token = None;
            r?;
        }
        Ok(())
    }

    fn session(&self) -> Result<()> {
        if self.token.is_none() {
            return Err(Error::new(ErrorKind::Auth, "not logged in"));
        }
        Ok(())
    }

    /// The server's state and version; needs no login.
    pub fn server_info(&self) -> Result<ServerInfo> {
        let v = self.call("AdminInterface", "getServerInfo", json!({}))?;
        let version = v.get("version").map(|s| text(s, "fullString")).unwrap_or_default();
        Ok(ServerInfo { state: text(&v, "serverState"), version })
    }

    pub fn logged_in_user(&self) -> Result<User> {
        self.session()?;
        let v = self.call("AuthInterface", "getLoggedInUser", json!({}))?;
        Ok(User { oid: int(&v, "oid"), username: text(&v, "username"), name: text(&v, "name"), user_type: text(&v, "userType") })
    }

    pub fn user_name(&self, uoid: i64) -> Result<String> {
        self.session()?;
        let v = self.call("ServiceInterface", "getUserByUoid", json!({ "uoid": uoid }))?;
        let name = text(&v, "name");
        Ok(if name.is_empty() { text(&v, "username") } else { name })
    }

    /// The active projects and sub-projects the user can see.
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.session()?;
        let v = self.call("ServiceInterface", "getAllProjects", json!({ "onlyTopLevel": false, "onlyActive": true }))?;
        Ok(v.as_array().map(|a| a.iter().map(project_of).collect()).unwrap_or_default())
    }

    pub fn project(&self, poid: i64) -> Result<Project> {
        self.session()?;
        Ok(project_of(&self.call("ServiceInterface", "getProjectByPoid", json!({ "poid": poid }))?))
    }

    /// A project's revisions, oldest first.
    pub fn revisions(&self, poid: i64) -> Result<Vec<Revision>> {
        self.session()?;
        let v = self.call("ServiceInterface", "getAllRevisionsOfProject", json!({ "poid": poid }))?;
        let mut out: Vec<Revision> = v.as_array().map(|a| a.iter().map(revision_of).collect()).unwrap_or_default();
        out.sort_by_key(|r| r.number);
        Ok(out)
    }

    /// A new project (or sub-project of `parent`) of `schema` (`ifc4`, `ifc2x3tc1`).
    pub fn create_project(&self, name: &str, schema: &str, parent: Option<i64>) -> Result<Project> {
        self.session()?;
        let v = match parent {
            Some(p) => self.call("ServiceInterface", "addProjectAsSubProject", json!({ "projectName": name, "parentPoid": p, "schema": schema }))?,
            None => self.call("ServiceInterface", "addProject", json!({ "projectName": name, "schema": schema }))?,
        };
        Ok(project_of(&v))
    }

    /// The deserialiser the server suggests for `.ifc` files into project `poid`.
    pub fn deserializer_for(&self, poid: i64) -> Result<Plugin> {
        self.session()?;
        let v = self.call("ServiceInterface", "getSuggestedDeserializerForExtension", json!({ "extension": "ifc", "poid": poid }))?;
        if v.is_null() {
            return Err(Error::new(ErrorKind::Server, "the server has no IFC deserialiser for this project's schema"));
        }
        Ok(plugin_of(&v))
    }

    /// The serialiser named `name` (`Ifc4`, `Ifc2x3tc1` …).
    pub fn serializer(&self, name: &str) -> Result<Plugin> {
        self.session()?;
        let v = self.call("ServiceInterface", "getSerializerByName", json!({ "serializerName": name }))?;
        if v.is_null() {
            return Err(Error::new(ErrorKind::Server, format!("the server has no serialiser named {name:?}")));
        }
        Ok(plugin_of(&v))
    }

    /// The IFC serialiser for a project of `schema`.
    pub fn ifc_serializer(&self, schema: &str) -> Result<Plugin> {
        let name = if schema.eq_ignore_ascii_case("ifc2x3tc1") { "Ifc2x3tc1" } else { "Ifc4" };
        self.serializer(name)
    }

    /// Checks `data` (an IFC file named `file_name`) in to project `poid` as a new revision,
    /// waiting until the server has stored it. Never asks the server to merge.
    pub fn check_in(&self, poid: i64, comment: &str, file_name: &str, data: &[u8], deserializer: i64) -> Result<CheckIn> {
        self.session()?;
        let before = self.project(poid)?.last_revision;
        let v = self.call(
            "ServiceInterface",
            "checkinSync",
            json!({
                "poid": poid,
                "comment": comment,
                "deserializerOid": deserializer,
                "fileSize": data.len(),
                "fileName": file_name,
                "data": base64::engine::general_purpose::STANDARD.encode(data),
                "merge": false,
            }),
        )?;
        let messages = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
        let errors = messages("errors");
        if text(&v, "state") == "AS_ERROR" || !errors.is_empty() {
            return Err(Error::new(ErrorKind::Server, format!("the check-in failed: {}", if errors.is_empty() { "the server gave no reason".into() } else { errors.join("; ") })));
        }
        let mut roid = int(&v, "roid");
        // The server may answer before it has stored the revision: follow the check-in's progress,
        // then take the project's new latest revision.
        if roid <= 0 && int(&v, "topicId") > 0 {
            roid = self.wait_for_revision(poid, int(&v, "topicId"), before)?;
        }
        if roid <= 0 {
            return Err(Error::new(ErrorKind::Protocol, format!("the check-in gave no revision (state {}, stage {}, progress {})", text(&v, "state"), int(&v, "stage"), int(&v, "progress"))));
        }
        Ok(CheckIn { roid, warnings: messages("warnings") })
    }

    /// Polls check-in `topic` (up to 5 minutes) until it ends; the project's new latest revision.
    fn wait_for_revision(&self, poid: i64, topic: i64, before: Option<i64>) -> Result<i64> {
        let started = std::time::Instant::now();
        loop {
            let p = self.call("NotificationRegistryInterface", "getProgress", json!({ "topicId": topic }))?;
            let state = text(&p, "state");
            if state == "AS_ERROR" {
                let errors: Vec<String> = p.get("errors").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
                return Err(Error::new(ErrorKind::Server, format!("the check-in failed: {}", errors.join("; "))));
            }
            if state == "FINISHED" || p.is_null() {
                let latest = self.project(poid)?.last_revision;
                if let Some(r) = latest.filter(|r| Some(*r) != before) {
                    return Ok(r);
                }
            }
            if started.elapsed() > Duration::from_secs(300) {
                return Err(Error::new(ErrorKind::Protocol, "the check-in did not finish within 5 minutes"));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The extended-data schema named `name`, if the server has it.
    pub fn extended_schema(&self, name: &str) -> Result<Option<i64>> {
        self.session()?;
        match self.call("ServiceInterface", "getExtendedDataSchemaByName", json!({ "name": name })) {
            Ok(v) => Ok(v.get("oid").and_then(Value::as_i64)),
            // BIMserver says "No ExtendedDataSchema found" as a user exception.
            Err(e) if e.kind == ErrorKind::Server => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Registers an extended-data schema; its oid.
    pub fn add_extended_schema(&self, name: &str, content_type: &str, description: &str) -> Result<i64> {
        self.session()?;
        let v = self.call("ServiceInterface", "addExtendedDataSchema", json!({ "extendedDataSchema": { "__type": "SExtendedDataSchema", "name": name, "contentType": content_type, "description": description } }))?;
        v.as_i64().ok_or_else(|| Error::new(ErrorKind::Protocol, "the schema got no id"))
    }

    /// Attaches `data` to revision `roid` as extended data of `schema` (no new revision).
    pub fn add_extended_data(&self, roid: i64, schema: i64, title: &str, file_name: &str, mime: &str, data: &[u8]) -> Result<()> {
        self.session()?;
        let f = self.call(
            "ServiceInterface",
            "uploadFile",
            json!({ "file": { "__type": "SFile", "data": base64::engine::general_purpose::STANDARD.encode(data), "filename": file_name, "mime": mime, "size": data.len() } }),
        )?;
        let file = f.as_i64().ok_or_else(|| Error::new(ErrorKind::Protocol, "the upload gave no file"))?;
        self.call("ServiceInterface", "addExtendedDataToRevision", json!({ "roid": roid, "extendedData": { "__type": "SExtendedData", "title": title, "schemaId": schema, "fileId": file } }))?;
        Ok(())
    }

    /// Every extended data item of revision `roid`, oldest first.
    pub fn extended_data(&self, roid: i64) -> Result<Vec<ExtendedData>> {
        self.session()?;
        let v = self.call("ServiceInterface", "getAllExtendedDataOfRevision", json!({ "roid": roid }))?;
        let mut out: Vec<ExtendedData> = v
            .as_array()
            .map(|a| a.iter().map(|x| ExtendedData { oid: int(x, "oid"), schema: int(x, "schemaId"), file: int(x, "fileId"), title: text(x, "title"), added: int(x, "added"), user: int(x, "userId") }).collect())
            .unwrap_or_default();
        out.sort_by_key(|x| x.oid);
        Ok(out)
    }

    /// A stored file's content.
    pub fn file(&self, file_id: i64) -> Result<Vec<u8>> {
        self.session()?;
        let v = self.call("ServiceInterface", "getFile", json!({ "fileId": file_id }))?;
        let data = v.get("data").and_then(Value::as_str).ok_or_else(|| Error::new(ErrorKind::Protocol, "the file has no data"))?;
        base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| Error::new(ErrorKind::Protocol, format!("the file's data is not base64: {e}")))
    }

    /// Revision `roid` serialised by `serializer`, as bytes.
    pub fn download(&self, roid: i64, serializer: i64) -> Result<Vec<u8>> {
        self.session()?;
        let topic = self.call("ServiceInterface", "download", json!({ "roids": [roid], "query": "{}", "serializerOid": serializer, "sync": true }))?;
        let topic = topic.as_i64().ok_or_else(|| Error::new(ErrorKind::Protocol, "the download gave no topic"))?;
        let mut url = self.endpoint("download");
        url.query_pairs_mut().append_pair("token", self.token.as_deref().unwrap_or_default()).append_pair("topicId", &topic.to_string());
        let got = self.http.get(url).header("ngrok-skip-browser-warning", "1").send().map_err(|e| Error::new(ErrorKind::Transport, e.to_string())).and_then(|r| {
            let status = r.status();
            if !status.is_success() {
                return Err(Error::new(ErrorKind::Transport, format!("the download answered {status}")));
            }
            r.bytes().map(|b| b.to_vec()).map_err(|e| Error::new(ErrorKind::Transport, e.to_string()))
        });
        // The server keeps the prepared download until told to drop it.
        let _ = self.call("ServiceInterface", "cleanupLongAction", json!({ "topicId": topic }));
        got
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// A server answering each request with the next of `answers` (status, body), recording
    /// the request bodies.
    fn serve(answers: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/", l.local_addr().unwrap().port());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let (mut s, _) = l.accept().unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let (mut line, mut len, mut first) = (String::new(), 0usize, String::new());
                r.read_line(&mut first).unwrap();
                loop {
                    line.clear();
                    r.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut b = vec![0; len];
                r.read_exact(&mut b).unwrap();
                tx.send(format!("{}{}", first.trim(), String::from_utf8(b).unwrap())).unwrap();
                write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (url, rx)
    }

    #[test]
    fn urls_are_https_except_on_this_computer() {
        assert!(check_url("https://bim.example.org/", false).is_ok());
        assert!(check_url("http://localhost:8082", false).is_ok());
        assert_eq!(check_url("http://bim.example.org", false).unwrap_err().kind, ErrorKind::Url);
        assert!(check_url("http://bim.example.org", true).is_ok());
        assert!(check_url("ftp://bim.example.org", true).is_err());
    }

    #[test]
    fn a_login_keeps_its_token_and_exceptions_are_typed() {
        let (url, rx) = serve(vec![
            (200, r#"{"response":{"result":"T0K"}}"#.into()),
            (200, r#"{"response":{"result":[{"oid":5,"name":"Hospital","schema":"ifc4","parentId":-1,"subProjects":[7],"lastRevisionId":-1,"revisions":[]}]}}"#.into()),
            (200, r#"{"response":{"exception":{"__type":"AuthenticationException","message":"Invalid token"}}}"#.into()),
            (200, r#"{"response":{"exception":{"__type":"UserException","message":"Only administrators can create new projects"}}}"#.into()),
            (502, "bad gateway".into()),
        ]);
        let mut c = Client::new(&url, false).unwrap();
        assert_eq!(c.projects().unwrap_err().kind, ErrorKind::Auth, "no call before login");
        c.login("anna@example.org", "secret").unwrap();
        let login = rx.recv().unwrap();
        assert!(login.starts_with("POST /json") && login.contains(r#""method":"login""#) && !login.contains("\"token\""), "{login}");
        let p = c.projects().unwrap();
        assert_eq!((p[0].oid, p[0].parent, p[0].sub_projects.clone(), p[0].last_revision), (5, None, vec![7], None));
        assert!(rx.recv().unwrap().contains(r#""token":"T0K""#));
        let e = c.projects().unwrap_err();
        assert_eq!((e.kind, e.exception.as_deref()), (ErrorKind::Auth, Some("AuthenticationException")));
        let e = c.create_project("X", "ifc4", None).unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), (ErrorKind::Server, "Only administrators can create new projects"));
        assert_eq!(c.server_info().unwrap_err().kind, ErrorKind::Transport);
    }

    #[test]
    fn a_check_in_sends_the_file_and_reads_the_revision() {
        let project = |last: i64| format!(r#"{{"response":{{"result":{{"oid":131073,"name":"P","schema":"ifc4","parentId":-1,"subProjects":[],"lastRevisionId":{last},"revisions":[]}}}}}}"#);
        let (url, rx) = serve(vec![
            (200, r#"{"response":{"result":"T"}}"#.into()),
            (200, project(-1)),
            (200, r#"{"response":{"result":{"__type":"SLongCheckinActionState","errors":[],"warnings":["w"],"roid":65539,"state":"FINISHED"}}}"#.into()),
            (200, project(65539)),
            (200, r#"{"response":{"result":{"errors":["bad file"],"state":"AS_ERROR","roid":-1}}}"#.into()),
            // Answered before the revision is stored: followed to the end.
            (200, project(65539)),
            (200, r#"{"response":{"result":{"errors":[],"warnings":[],"roid":-1,"state":"STARTED","topicId":5}}}"#.into()),
            (200, r#"{"response":{"result":{"state":"STARTED"}}}"#.into()),
            (200, r#"{"response":{"result":{"state":"FINISHED"}}}"#.into()),
            (200, project(70000)),
        ]);
        let mut c = Client::new(&url, false).unwrap();
        c.login_user_token("USERTOKEN").unwrap();
        assert!(rx.recv().unwrap().contains(r#""method":"loginUserToken""#));
        let r = c.check_in(131073, "first", "m.ifc", b"ISO-10303-21;", 9).unwrap();
        assert_eq!((r.roid, r.warnings), (65539, vec!["w".to_owned()]));
        let _ = rx.recv().unwrap();
        let sent = rx.recv().unwrap();
        assert!(sent.contains(r#""data":"SVNPLTEwMzAzLTIxOw==""#) && sent.contains(r#""merge":false"#) && sent.contains(r#""fileSize":13"#), "{sent}");
        assert!(c.check_in(131073, "x", "m.ifc", b"x", 9).unwrap_err().message.contains("bad file"));
        assert_eq!(c.check_in(131073, "late", "m.ifc", b"x", 9).unwrap().roid, 70000);
    }
}
