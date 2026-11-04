// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Issue coordination (COL-BCF-002, COL-BCF-004, COL-FE-009): one set of `issues.*` commands
//! over wherever a model's issues live, and the BCF API servers (`coordination.*`) a model can be
//! bound to.
//!
//! - **Where issues live.** A model bound to a project on a BCF API server (OpenCDE: e.g.
//!   OpenConstructionERP, or any BCF API 3.0 or 2.1 server) has its issues there; a model opened
//!   from FileEngine has them in FileEngine's discussion service ([`crate::model_issues`]);
//!   any other model keeps BCF topics with itself ([`crate::bcf_topics`]). `issues.list` says
//!   which (`source`), and lists them all, open and resolved alike (`resolved` marks the
//!   latter), with filters for status and text.
//! - **Both ways.** Issues are read from the server when asked (the Issues pane asks when a
//!   model opens and every minute while shown), and kept beside the model for reading offline.
//!   Replies, resolving and reopening, new issues (from a 3D view, with a snapshot), field edits,
//!   added views and deletions go straight to the server, as far as it allows. The model's own
//!   topics (made by clash checks, the bathroom checks, a BCF import, or before the model was
//!   bound) are listed beside the server's until `issues.publish` puts them on the server; on
//!   a BCF API server a topic keeps its GUID, so publishing twice does not duplicate it.
//! - **Servers.** Name, address (the BCF API's base, found from the server's root when needed),
//!   version, how to sign in (a token, a user name and password as HTTP Basic, or a login
//!   address taking an e-mail and password, as OpenConstructionERP's) are per-user settings
//!   (`coordination.json`); the secret lives in the OS keychain (or for this session). No
//!   command returns it. Bindings of models (by file) to projects are kept there too, so they
//!   are not project data and do not change the model.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use base64::Engine;
use riviera_core::bcf::{self, Viewpoint};
use riviera_core::issues::{self, Topic, TopicFields, TopicViewpoint};
use riviera_core::{Caller, CommandMeta, Effect, ElementRef, Error, HostCommand, Result};
use riviera_opencde::{Auth, Client, ErrorKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bimserver::Storage;
use crate::{App, DocumentId};

/// The keychain service secrets are stored under (the account is the server's id).
pub const KEYCHAIN_SERVICE: &str = "org.bimriviera.bcf-api";
/// Commands not offered to agents: they remove stored credentials, or delete issues on a server.
pub const NOT_FOR_AGENTS: &[&str] = &["coordination.remove_server", "issues.delete"];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn failed(e: riviera_opencde::Error) -> Error {
    let code = match e.kind {
        ErrorKind::Url => "coordination.bad_url",
        ErrorKind::Transport => "coordination.unreachable",
        ErrorKind::Auth => "coordination.not_authorised",
        ErrorKind::Forbidden => "coordination.forbidden",
        ErrorKind::NotFound => "coordination.not_found",
        ErrorKind::Conflict => "coordination.conflict",
        ErrorKind::Server => "coordination.refused",
        ErrorKind::Protocol => "coordination.protocol",
    };
    Error::new(code, e.message.clone()).with_details(json!({ "status": e.status }))
}

/// Whether a status means the issue is done with.
pub fn is_resolved(status: &str) -> bool {
    matches!(status.trim().to_lowercase().as_str(), "resolved" | "closed" | "done" | "fixed" | "completed")
}

// ---- Servers and bindings ----

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    servers: Vec<Profile>,
    bindings: Vec<Binding>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Profile {
    id: String,
    name: String,
    /// The BCF API's base (without the version).
    url: String,
    version: String,
    /// `none`, `token`, `basic` or `login`.
    auth: String,
    user: Option<String>,
    login_url: Option<String>,
    allow_http: bool,
}

/// A model bound to a project on a server.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Binding {
    /// The model's file.
    pub model: String,
    pub server_id: String,
    pub project_id: String,
    pub project_name: String,
}

/// The BCF API servers' working state on the app.
#[derive(Default)]
pub struct Servers {
    settings: Option<PathBuf>,
    data: Option<PathBuf>,
    session_only: bool,
    session: HashMap<String, String>,
    clients: HashMap<String, Client>,
}

impl std::fmt::Debug for Servers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Servers").field("settings", &self.settings).finish_non_exhaustive()
    }
}

impl Servers {
    /// Keeps the settings in `settings`, the issues read under `data` and the secrets in memory:
    /// for tests and tools.
    pub fn isolated(settings: PathBuf, data: PathBuf) -> Self {
        Self { settings: Some(settings), data: Some(data), session_only: true, ..Self::default() }
    }

    fn file(&self) -> Result<PathBuf> {
        self.settings.clone().or_else(|| crate::subscriptions::config_dir().map(|d| d.join("coordination.json"))).ok_or_else(|| bad("coordination.no_settings", "there is no per-user settings folder"))
    }

    fn cache(&self, server: &str, project: &str) -> Option<PathBuf> {
        let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect::<String>();
        Some(self.data.clone().or_else(|| crate::bimserver::data_dir().map(|d| d.join("coordination")))?.join(safe(server)).join(format!("{}.json", safe(project))))
    }

    fn load(&self) -> Result<Settings> {
        match std::fs::read(self.file()?) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| bad("coordination.settings_invalid", format!("coordination.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(bad("coordination.settings_unreadable", e.to_string())),
        }
    }

    fn save(&self, s: &Settings) -> Result<()> {
        let path = self.file()?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| bad("coordination.settings_unwritable", e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s).expect("settings serialise")).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| bad("coordination.settings_unwritable", e.to_string()))
    }

    fn keychain(&self) -> bool {
        !self.session_only && keyring::Entry::store_status().is_ok()
    }

    fn put_secret(&mut self, id: &str, secret: &str, remember: bool) -> Storage {
        if remember && self.keychain() && keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.set_password(secret)).is_ok() {
            self.session.remove(id);
            return Storage::Keychain;
        }
        self.session.insert(id.to_owned(), secret.to_owned());
        Storage::Session
    }

    fn secret(&self, id: &str) -> Option<String> {
        if let Some(s) = self.session.get(id) {
            return Some(s.clone());
        }
        if !self.keychain() {
            return None;
        }
        crate::keychain::password(KEYCHAIN_SERVICE, id).ok()
    }

    fn storage(&self, id: &str) -> Storage {
        if self.session.contains_key(id) {
            Storage::Session
        } else if self.keychain() && crate::keychain::password(KEYCHAIN_SERVICE, id).is_ok() {
            Storage::Keychain
        } else {
            Storage::None
        }
    }

    fn delete_secret(&mut self, id: &str) {
        self.session.remove(id);
        if self.keychain() {
            crate::keychain::forget(KEYCHAIN_SERVICE, id);
        }
    }

    fn profile(&self, id: &str) -> Result<Profile> {
        self.load()?.servers.into_iter().find(|p| p.id == id).ok_or_else(|| bad("coordination.no_server", format!("no issue server {id:?}; coordination.servers lists them")))
    }

    /// Runs `f` on a client for server `id`, signed in with the stored secret.
    fn with<T>(&mut self, id: &str, f: impl FnOnce(&Client) -> riviera_opencde::Result<T>) -> Result<T> {
        let _p = riviera_core::profiler::scope("BCF API call");
        if !self.clients.contains_key(id) {
            let p = self.profile(id)?;
            let secret = self.secret(id);
            let auth = auth_of(&p, secret).ok_or_else(|| bad("coordination.not_signed_in", format!("sign in to {} first (coordination.sign_in)", p.name)))?;
            let c = Client::new(&p.url, &p.version, auth, p.allow_http).map_err(failed)?;
            self.clients.insert(id.to_owned(), c);
        }
        let r = f(&self.clients[id]).map_err(failed);
        if r.as_ref().is_err_and(|e| e.code == "coordination.not_authorised") {
            // Asked again with the next call, perhaps with a new secret.
            self.clients.remove(id);
        }
        r
    }

    /// The binding of the model at `path`, if any.
    pub(crate) fn binding(&self, path: &Path) -> Option<Binding> {
        let key = model_key(path);
        self.load().ok()?.bindings.into_iter().find(|b| b.model == key)
    }
}

fn auth_of(p: &Profile, secret: Option<String>) -> Option<Auth> {
    let user = p.user.clone().unwrap_or_default();
    Some(match p.auth.as_str() {
        "none" => Auth::None,
        "token" => Auth::Token(secret?),
        "basic" => Auth::Basic { user, password: secret? },
        _ => Auth::Login { url: p.login_url.clone().unwrap_or_else(|| riviera_opencde::OPENCONSTRUCTIONERP_LOGIN.into()), user, password: secret? },
    })
}

fn model_key(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

impl App {
    /// The BCF API servers (tests swap in [`Servers::isolated`]).
    pub fn coordination_mut(&mut self) -> &mut Servers {
        &mut self.coordination
    }
}

macro_rules! host {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, [$($eff:ident),*], $params:ty, $out:ty, |$app:ident, $caller:ident, $p:ident| $body:block) => {
        pub(crate) struct $ty;
        impl HostCommand for $ty {
            type Host = App;
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta { name: $name, version: 1, summary: $summary, description: $desc, effects: &[$(Effect::$eff),*], transaction_name: "" };
            #[allow(unused_variables)]
            fn run(&self, $app: &mut App, $caller: &Caller, $p: $params) -> Result<$out> $body
        }
    };
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ServerOut {
    pub id: String,
    pub name: String,
    /// The BCF API's base.
    pub url: String,
    pub version: String,
    pub auth: String,
    pub user: Option<String>,
    /// Where the secret is kept.
    pub secret: Storage,
}

fn server_out(s: &Servers, p: &Profile) -> ServerOut {
    ServerOut { id: p.id.clone(), name: p.name.clone(), url: p.url.clone(), version: p.version.clone(), auth: p.auth.clone(), user: p.user.clone(), secret: s.storage(&p.id) }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddServerParams {
    pub name: String,
    /// The server's address: its root, or its BCF API's base (`https://host/bcf`).
    pub url: String,
    /// `login` (an e-mail and password posted to `login_url`; OpenConstructionERP's by default),
    /// `token` (a bearer token), `basic` (user name and password) or `none`.
    #[serde(default = "login")]
    pub auth: String,
    pub user: Option<String>,
    /// For `login`: the address that takes the e-mail and password, from the server's root.
    pub login_url: Option<String>,
    /// The password or token.
    pub secret: Option<String>,
    /// Keep the secret in the OS keychain (default); otherwise for this session.
    #[serde(default = "yes")]
    pub remember: bool,
    /// Plain http to a host other than this computer (a trusted local network).
    #[serde(default)]
    pub allow_http: bool,
}

fn login() -> String {
    "login".into()
}

fn yes() -> bool {
    true
}

host!(AddServer, "coordination.add_server", "Add a BCF issue server",
    "Adds a coordination server whose BCF API (3.0, or 2.1) holds issues (COL-BCF-002): OpenConstructionERP \
     or any OpenCDE BCF API server. Finds the API from the server's root when needed, signs in to check, and \
     keeps the address and version found. The secret goes to the OS keychain (`remember`) or is kept for this \
     session; it is never returned.",
    [Network], AddServerParams, ServerOut, |app, caller, p| {
    if !matches!(p.auth.as_str(), "login" | "token" | "basic" | "none") {
        return Err(bad("coordination.invalid_input", "auth is login, token, basic or none"));
    }
    let mut profile = Profile { id: String::new(), name: p.name.trim().to_owned(), url: p.url.trim().to_owned(), version: String::new(), auth: p.auth.clone(), user: p.user.filter(|u| !u.trim().is_empty()), login_url: p.login_url.filter(|u| !u.trim().is_empty()), allow_http: p.allow_http };
    if profile.name.is_empty() {
        return Err(bad("coordination.invalid_input", "give the server a name"));
    }
    let secret = p.secret.filter(|s| !s.is_empty());
    let auth = auth_of(&profile, secret.clone()).ok_or_else(|| bad("coordination.invalid_input", "give the password or token"))?;
    let c = Client::connect(&profile.url, auth, p.allow_http).map_err(failed)?;
    profile.url = c.base_url().to_owned();
    profile.version = c.version().to_owned();
    let s = &mut app.coordination;
    let mut settings = s.load()?;
    profile.id = format!("bcf-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64));
    settings.servers.push(profile.clone());
    s.save(&settings)?;
    if let Some(secret) = secret {
        s.put_secret(&profile.id, &secret, p.remember);
    }
    s.clients.insert(profile.id.clone(), c);
    Ok(server_out(s, &profile))
});

host!(ListServers, "coordination.servers", "BCF issue servers",
    "The coordination servers added (COL-BCF-002): id, name, BCF API address and version, how to sign in and \
     where the secret is kept (never the secret).",
    [Read], (), Vec<ServerOut>, |app, caller, p| {
    let s = &app.coordination;
    Ok(s.load()?.servers.iter().map(|p| server_out(s, p)).collect())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignInParams {
    pub server_id: String,
    pub user: Option<String>,
    /// The password or token.
    pub secret: String,
    #[serde(default = "yes")]
    pub remember: bool,
}

host!(SignIn, "coordination.sign_in", "Sign in to a BCF issue server",
    "Signs in to a coordination server again (a new password or token), checking it there. The secret goes to \
     the OS keychain (`remember`) or is kept for this session.",
    [Network], SignInParams, ServerOut, |app, caller, p| {
    let s = &mut app.coordination;
    let mut settings = s.load()?;
    let profile = settings.servers.iter_mut().find(|x| x.id == p.server_id).ok_or_else(|| bad("coordination.no_server", "no such server"))?;
    if let Some(u) = p.user.filter(|u| !u.trim().is_empty()) {
        profile.user = Some(u);
    }
    let profile = profile.clone();
    let auth = auth_of(&profile, Some(p.secret.clone())).expect("a secret");
    let c = Client::new(&profile.url, &profile.version, auth, profile.allow_http).map_err(failed)?;
    c.current_user().map_err(failed)?;
    s.save(&settings)?;
    s.put_secret(&profile.id, &p.secret, p.remember);
    s.clients.insert(profile.id.clone(), c);
    Ok(server_out(s, &profile))
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServerIdParams {
    pub server_id: String,
}

host!(RemoveServer, "coordination.remove_server", "Remove a BCF issue server",
    "Removes a coordination server, its stored secret and the models bound to it. Issues on the server are not touched.",
    [Read], ServerIdParams, (), |app, caller, p| {
    let s = &mut app.coordination;
    let mut settings = s.load()?;
    settings.servers.retain(|x| x.id != p.server_id);
    settings.bindings.retain(|b| b.server_id != p.server_id);
    s.save(&settings)?;
    s.delete_secret(&p.server_id);
    s.clients.remove(&p.server_id);
    Ok(())
});

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectOut {
    pub project_id: String,
    pub name: String,
}

host!(Projects, "coordination.projects", "Projects on a BCF issue server",
    "The projects on a coordination server that the signed-in user may see (COL-BCF-002).",
    [Network, Read], ServerIdParams, Vec<ProjectOut>, |app, caller, p| {
    let list = app.coordination.with(&p.server_id, Client::projects)?;
    Ok(list.iter().map(|x| ProjectOut { project_id: text(&x["project_id"]), name: text(&x["name"]) }).collect())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindParams {
    pub document_id: Option<DocumentId>,
    pub server_id: String,
    pub project_id: String,
}

host!(Bind, "coordination.bind", "Keep a model's issues on a BCF server",
    "Binds the model (its file) to a project on a coordination server (COL-BCF-002): from then on its issues \
     are that project's, read and written there (issues.*), including for a model opened from FileEngine. \
     The model's own BCF topics stay with it until issues.publish puts them on the server. The model is not changed.",
    [Network], BindParams, Binding, |app, caller, p| {
    let (_, path) = doc_path(app, p.document_id)?;
    let project = app.coordination.with(&p.server_id, |c| c.project(&p.project_id))?;
    let b = Binding { model: model_key(&path), server_id: p.server_id, project_id: p.project_id, project_name: text(&project["name"]) };
    let s = &app.coordination;
    let mut settings = s.load()?;
    settings.bindings.retain(|x| x.model != b.model);
    settings.bindings.push(b.clone());
    s.save(&settings)?;
    Ok(b)
});

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocParams {
    pub document_id: Option<DocumentId>,
}

host!(Unbind, "coordination.unbind", "Stop keeping a model's issues on a BCF server",
    "Ends the model's binding to a coordination server's project. Issues there are not touched.",
    [Read], DocParams, (), |app, caller, p| {
    let (_, path) = doc_path(app, p.document_id)?;
    let key = model_key(&path);
    let s = &app.coordination;
    let mut settings = s.load()?;
    settings.bindings.retain(|x| x.model != key);
    s.save(&settings)
});

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    /// `model`, `fileengine` or `bcf_api`.
    pub source: String,
    /// The server's name (and for FileEngine, the model's).
    pub server: Option<String>,
    pub project: Option<String>,
    pub binding: Option<Binding>,
}

host!(Status, "coordination.status", "Where a model's issues live",
    "Where the model's issues are kept (COL-BCF-004): `bcf_api` (a project on a coordination server), \
     `fileengine` (the model was opened from FileEngine) or `model` (BCF topics kept with the model).",
    [Read], DocParams, StatusOut, |app, caller, p| {
    let (_, source) = source(app, p.document_id)?;
    Ok(match source {
        Source::Model => StatusOut { source: "model".into(), server: None, project: None, binding: None },
        Source::FileEngine(l) => StatusOut { source: "fileengine".into(), server: Some(l.name.clone()), project: None, binding: None },
        Source::Bcf(b) => StatusOut {
            source: "bcf_api".into(),
            server: app.coordination.profile(&b.server_id).ok().map(|p| p.name),
            project: Some(b.project_name.clone()),
            binding: Some(b),
        },
    })
});

// ---- Where a model's issues are ----

enum Source {
    Model,
    FileEngine(crate::fileengine::Link),
    Bcf(Binding),
}

fn doc_path(app: &App, document_id: Option<DocumentId>) -> Result<(DocumentId, PathBuf)> {
    let id = match document_id {
        Some(id) => id,
        None => app.active().ok_or_else(|| bad("document.none", "no document is open"))?,
    };
    Ok((id, app.get(id)?.project.path().to_path_buf()))
}

fn source(app: &App, document_id: Option<DocumentId>) -> Result<(DocumentId, Source)> {
    let (id, path) = doc_path(app, document_id)?;
    if let Some(b) = app.coordination.binding(&path) {
        return Ok((id, Source::Bcf(b)));
    }
    Ok((id, crate::fileengine::link_of(&path).map_or(Source::Model, Source::FileEngine)))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn opt(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

// ---- One shape for every source ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IssueComment {
    pub id: String,
    pub author: String,
    /// ISO 8601.
    pub date: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Issue {
    /// The topic GUID (the model's, or a BCF server's) or the FileEngine thread id.
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// As the source has it (`Open`, `Closed`, `resolved` …).
    pub status: String,
    /// Done with: resolved, closed, done.
    pub resolved: bool,
    pub topic_type: Option<String>,
    pub priority: Option<String>,
    pub assigned_to: Option<String>,
    pub due_date: Option<String>,
    pub stage: Option<String>,
    pub labels: Vec<String>,
    pub opened_by: String,
    pub created: String,
    pub updated: String,
    pub resolved_by: Option<String>,
    /// The model version it was raised on (FileEngine).
    pub version: Option<String>,
    pub comments: Vec<IssueComment>,
    pub viewpoints: usize,
    pub has_snapshot: bool,
    /// The components it is about (IFC GlobalIds).
    pub objects: Vec<String>,
    pub measurements: Vec<String>,
    /// The model changed under its viewpoint (FileEngine).
    pub stale: bool,
    /// `server`, or `model`: kept with the model and not on the server (yet; issues.publish).
    pub kept: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Abilities {
    /// Fields other than the status can be changed (issues.update).
    pub edit_fields: bool,
    /// More viewpoints can be added (issues.add_view).
    pub add_view: bool,
    pub delete: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IssuesOut {
    /// `model`, `fileengine` or `bcf_api`.
    pub source: String,
    pub server: Option<String>,
    pub project: Option<String>,
    /// The model version open here (FileEngine).
    pub version: Option<String>,
    /// The issues the filters let through: open first, then the newest.
    pub issues: Vec<Issue>,
    /// Counts over all of them, before the filters.
    pub total: usize,
    pub open: usize,
    pub resolved: usize,
    /// The model's own topics not on the server.
    pub local: usize,
    /// Read from the copy kept beside the model, the server being out of reach.
    pub offline: bool,
    /// What the server's issues allow (the model's own allow everything).
    pub server_can: Abilities,
    /// The lists for new issues and edits: `topic_types`, `statuses`, `priorities`, `labels`, `users`, `stages`.
    pub lists: Value,
}

fn model_issue(t: &Topic, store: &riviera_core::Store, kept: &str) -> Issue {
    let status = t.status.clone().unwrap_or_else(|| "Open".into());
    let mut objects = Vec::new();
    for v in &t.viewpoints {
        for g in &v.viewpoint.selection {
            if !objects.contains(g) {
                objects.push(g.clone());
            }
        }
    }
    Issue {
        id: t.guid.clone(),
        title: t.title.clone(),
        description: t.description.clone(),
        resolved: is_resolved(&status),
        status,
        topic_type: t.topic_type.clone(),
        priority: t.priority.clone(),
        assigned_to: t.assigned_to.clone(),
        due_date: t.due_date.clone(),
        stage: t.stage.clone(),
        labels: t.labels.clone(),
        opened_by: t.creation_author.clone(),
        created: t.creation_date.clone(),
        updated: t.changed().to_owned(),
        resolved_by: None,
        version: None,
        comments: t.comments.iter().map(|c| IssueComment { id: c.guid.clone(), author: c.author.clone(), date: c.date.clone(), text: c.text.clone() }).collect(),
        viewpoints: t.viewpoints.len(),
        has_snapshot: t.viewpoints.iter().any(|v| issues::snapshot(store, &v.guid).is_some()),
        objects,
        measurements: Vec::new(),
        stale: false,
        kept: kept.into(),
    }
}

fn fileengine_issue(i: crate::model_issues::Issue) -> Issue {
    Issue {
        id: i.thread_id,
        title: i.title,
        description: None,
        resolved: is_resolved(&i.status),
        status: i.status,
        topic_type: None,
        priority: None,
        assigned_to: None,
        due_date: None,
        stage: None,
        labels: Vec::new(),
        opened_by: i.opened_by,
        created: i.created,
        updated: i.updated,
        resolved_by: i.resolved_by,
        version: Some(i.version).filter(|v| !v.is_empty()),
        comments: i.comments.into_iter().map(|c| IssueComment { id: c.id, author: c.author, date: c.date, text: c.text }).collect(),
        viewpoints: usize::from(i.viewpoint.is_some()),
        has_snapshot: i.has_snapshot,
        objects: i.objects,
        measurements: i.measurements,
        stale: i.stale,
        kept: "server".into(),
    }
}

/// A BCF API topic, with its comments and viewpoints, as read.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Remote {
    topic: Value,
    comments: Vec<Value>,
    viewpoints: Vec<Value>,
}

impl Remote {
    fn guid(&self) -> String {
        text(&self.topic["guid"]).to_lowercase()
    }

    fn issue(&self) -> Issue {
        let t = &self.topic;
        let status = opt(&t["topic_status"]).unwrap_or_else(|| "Open".into());
        let mut objects = Vec::new();
        for v in &self.viewpoints {
            for g in bcf::from_json(v).selection {
                if !objects.contains(&g) {
                    objects.push(g);
                }
            }
        }
        Issue {
            id: text(&t["guid"]),
            title: text(&t["title"]),
            description: opt(&t["description"]),
            resolved: is_resolved(&status),
            status,
            topic_type: opt(&t["topic_type"]),
            priority: opt(&t["priority"]),
            assigned_to: opt(&t["assigned_to"]),
            due_date: opt(&t["due_date"]),
            stage: opt(&t["stage"]),
            labels: t["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
            opened_by: text(&t["creation_author"]),
            created: text(&t["creation_date"]),
            updated: opt(&t["modified_date"]).unwrap_or_else(|| text(&t["creation_date"])),
            resolved_by: None,
            version: None,
            comments: self
                .comments
                .iter()
                .map(|c| IssueComment { id: text(&c["guid"]), author: text(&c["author"]), date: text(&c["date"]), text: text(&c["comment"]) })
                .collect(),
            viewpoints: self.viewpoints.len(),
            has_snapshot: !self.viewpoints.is_empty(),
            objects,
            measurements: Vec::new(),
            stale: false,
            kept: "server".into(),
        }
    }

    /// As a BCF topic, for an archive.
    fn topic(&self) -> Topic {
        let i = self.issue();
        Topic {
            guid: i.id.clone(),
            title: i.title,
            description: i.description,
            topic_type: i.topic_type,
            status: Some(i.status),
            priority: i.priority,
            labels: i.labels,
            assigned_to: i.assigned_to,
            due_date: i.due_date,
            stage: i.stage,
            creation_date: i.created,
            creation_author: i.opened_by,
            modified_date: opt(&self.topic["modified_date"]),
            modified_author: opt(&self.topic["modified_author"]),
            comments: self
                .comments
                .iter()
                .map(|c| issues::Comment { guid: text(&c["guid"]), date: text(&c["date"]), author: text(&c["author"]), text: text(&c["comment"]), viewpoint: opt(&c["viewpoint_guid"]), modified_date: opt(&c["modified_date"]), modified_author: opt(&c["modified_author"]) })
                .collect(),
            viewpoints: self.viewpoints.iter().map(|v| TopicViewpoint { guid: text(&v["guid"]), viewpoint: bcf::from_json(v), snapshot: None }).collect(),
            ..Topic::default()
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RemoteCache {
    topics: Vec<Remote>,
    extensions: Value,
}

/// A bound project's topics, from the server (comments always read again; viewpoints when the
/// topic changed), kept beside; else the kept copy.
fn remote(app: &mut App, b: &Binding, refresh: bool) -> Result<(RemoteCache, bool)> {
    let path = app.coordination.cache(&b.server_id, &b.project_id);
    let kept: Option<RemoteCache> = path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    if !refresh && let Some(k) = kept {
        return Ok((k, false));
    }
    let project = b.project_id.clone();
    let before: BTreeMap<String, Remote> = kept.as_ref().map(|k| k.topics.iter().map(|r| (r.guid(), r.clone())).collect()).unwrap_or_default();
    let read = app.coordination.with(&b.server_id, |c| {
        let extensions = c.extensions(&project).unwrap_or(Value::Null);
        let mut out = Vec::new();
        for t in c.topics(&project)? {
            let guid = text(&t["guid"]);
            let comments = c.comments(&project, &guid)?;
            let viewpoints = match before.get(&guid.to_lowercase()) {
                Some(r) if r.topic["modified_date"] == t["modified_date"] && !t["modified_date"].is_null() => r.viewpoints.clone(),
                _ => c.viewpoints(&project, &guid)?,
            };
            out.push(Remote { topic: t, comments, viewpoints });
        }
        Ok(RemoteCache { topics: out, extensions })
    });
    match read {
        Ok(r) => {
            if let Some(p) = &path {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(p, serde_json::to_vec(&r).expect("serialises"));
            }
            Ok((r, false))
        }
        Err(e) if e.code == "coordination.unreachable" => kept.map(|k| (k, true)).ok_or(e),
        Err(e) => Err(e),
    }
}

/// Published model topics on FileEngine: topic GUID → thread id, beside the working copy.
const PUBLISHED: &str = "published-issues.json";

fn published(path: &Path) -> BTreeMap<String, String> {
    std::fs::read(path.with_file_name(PUBLISHED)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn model_topics(app: &App, id: DocumentId) -> Result<Vec<Topic>> {
    Ok(issues::all(app.get(id)?.doc.store()).into_iter().map(|(_, t)| t).collect())
}

fn lists_json(l: &issues::Lists) -> Value {
    json!({ "topic_types": l.topic_types, "statuses": l.statuses, "priorities": l.priorities, "labels": l.labels, "users": l.users, "stages": l.stages })
}

/// The project's lists as the BCF API gives them (3.0 and 2.1 names), else the defaults.
fn server_lists(ext: &Value, topics: &[Topic]) -> Value {
    let mut l = issues::lists_of(topics);
    let list = |names: &[&str]| -> Vec<String> { names.iter().find_map(|n| ext[*n].as_array()).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect() };
    for (dst, names) in [
        (&mut l.topic_types, &["topic_type", "topic_types"][..]),
        (&mut l.statuses, &["topic_status", "topic_statuses"][..]),
        (&mut l.priorities, &["priority", "priorities"][..]),
        (&mut l.labels, &["topic_label", "topic_labels"][..]),
        (&mut l.stages, &["stage", "stages"][..]),
        (&mut l.users, &["users", "user_id_type"][..]),
    ] {
        let from = list(names);
        if !from.is_empty() {
            for x in std::mem::take(dst) {
                if !from.contains(&x) && names[0] != "topic_status" && names[0] != "topic_type" && names[0] != "priority" {
                    // Values in use on topics stay offered.
                    dst.push(x);
                }
            }
            let mut v = from;
            v.append(dst);
            *dst = v;
        }
    }
    lists_json(&l)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub document_id: Option<DocumentId>,
    /// Ask the server again (default true); false reads the copy kept beside the model.
    #[serde(default = "yes")]
    pub refresh: bool,
    /// `all` (default), `open` or `resolved`.
    pub status: Option<String>,
    /// Words to find in the title, description, comments, type, priority, assignee or labels.
    pub text: Option<String>,
}

pub(crate) fn list(app: &mut App, p: ListParams) -> Result<IssuesOut> {
    let (id, source) = source(app, p.document_id)?;
    let path = app.get(id)?.project.path().to_path_buf();
    let topics = model_topics(app, id)?;
    let store = app.get(id)?.doc.store();
    let mut out = match source {
        Source::Model => IssuesOut {
            source: "model".into(),
            server: None,
            project: None,
            version: None,
            issues: topics.iter().map(|t| model_issue(t, store, "model")).collect(),
            total: 0,
            open: 0,
            resolved: 0,
            local: 0,
            offline: false,
            server_can: Abilities { edit_fields: true, add_view: true, delete: true },
            lists: lists_json(&issues::lists(store)),
        },
        Source::FileEngine(_) => {
            let done = published(&path);
            let local: Vec<Issue> = topics.iter().filter(|t| !done.contains_key(&t.guid.to_lowercase())).map(|t| model_issue(t, store, "model")).collect();
            let lists = lists_json(&issues::lists(store));
            let fe = crate::model_issues::list(app, Some(id), p.refresh)?;
            let mut issues: Vec<Issue> = fe.issues.into_iter().map(fileengine_issue).collect();
            issues.extend(local);
            IssuesOut {
                source: "fileengine".into(),
                server: Some("FileEngine".into()),
                project: Some(fe.model),
                version: Some(fe.version),
                issues,
                total: 0,
                open: 0,
                resolved: 0,
                local: 0,
                offline: fe.offline,
                server_can: Abilities { edit_fields: false, add_view: false, delete: false },
                lists,
            }
        }
        Source::Bcf(b) => {
            let local_topics = topics.clone();
            let (r, offline) = remote(app, &b, p.refresh)?;
            let store = app.get(id)?.doc.store();
            let on_server: Vec<String> = r.topics.iter().map(Remote::guid).collect();
            let mut issues: Vec<Issue> = r.topics.iter().map(Remote::issue).collect();
            issues.extend(local_topics.iter().filter(|t| !on_server.contains(&t.guid.to_lowercase())).map(|t| model_issue(t, store, "model")));
            let all: Vec<Topic> = r.topics.iter().map(Remote::topic).collect();
            IssuesOut {
                source: "bcf_api".into(),
                server: app.coordination.profile(&b.server_id).ok().map(|p| p.name),
                project: Some(b.project_name.clone()),
                version: None,
                issues,
                total: 0,
                open: 0,
                resolved: 0,
                local: 0,
                offline,
                server_can: Abilities { edit_fields: true, add_view: true, delete: true },
                lists: server_lists(&r.extensions, &all),
            }
        }
    };
    out.total = out.issues.len();
    out.resolved = out.issues.iter().filter(|i| i.resolved).count();
    out.open = out.total - out.resolved;
    out.local = if out.source == "model" { 0 } else { out.issues.iter().filter(|i| i.kept == "model").count() };
    let words: Vec<String> = p.text.unwrap_or_default().to_lowercase().split_whitespace().map(str::to_owned).collect();
    let status = p.status.unwrap_or_default().to_lowercase();
    out.issues.retain(|i| {
        let by_status = match status.as_str() {
            "open" => !i.resolved,
            "resolved" | "closed" => i.resolved,
            _ => true,
        };
        let hay = || {
            let mut h = format!("{} {} {} {} {} {} {}", i.title, i.description.as_deref().unwrap_or(""), i.topic_type.as_deref().unwrap_or(""), i.priority.as_deref().unwrap_or(""), i.assigned_to.as_deref().unwrap_or(""), i.opened_by, i.labels.join(" "));
            for c in &i.comments {
                h.push(' ');
                h.push_str(&c.text);
            }
            h.to_lowercase()
        };
        by_status && (words.is_empty() || {
            let h = hay();
            words.iter().all(|w| h.contains(w.as_str()))
        })
    });
    // Open first, newest first.
    out.issues.sort_by(|a, b| a.resolved.cmp(&b.resolved).then_with(|| b.updated.cmp(&a.updated)));
    Ok(out)
}

host!(List, "issues.list", "A model's issues, wherever they are kept",
    "Every issue on the model (COL-BCF-004), open and resolved alike (`resolved` marks the latter): from the \
     BCF server project it is bound to (coordination.bind), from FileEngine for a model opened from there, or \
     the BCF topics kept with the model; the model's own topics not on the server yet are listed too \
     (`kept: model`). Filter by `status` (all, open, resolved) and `text`. Each issue has its fields, comments, \
     viewpoints, the components it is about and whether it has a snapshot. Kept beside the model to read \
     offline (`offline`). The model is not changed.",
    [Network, Read], ListParams, IssuesOut, |app, caller, p| {
    list(app, p)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
}

/// Where issue `id` is: on the server, or a model topic.
enum Found {
    Model,
    FileEngine(crate::fileengine::Link),
    Bcf(Binding, Remote),
}

fn find(app: &mut App, document_id: Option<DocumentId>, id: &str) -> Result<(DocumentId, Found)> {
    let (doc, source) = source(app, document_id)?;
    let in_model = model_topics(app, doc)?.iter().any(|t| t.guid.eq_ignore_ascii_case(id));
    let found = match source {
        Source::Model => Found::Model,
        Source::FileEngine(link) => {
            let (threads, _) = crate::model_issues::list(app, Some(doc), false).map(|o| (o.issues, ()))?;
            if threads.iter().any(|t| t.thread_id == id) {
                Found::FileEngine(link)
            } else if in_model {
                Found::Model
            } else {
                // Perhaps raised since the copy was kept.
                let threads = crate::model_issues::list(app, Some(doc), true)?.issues;
                if !threads.iter().any(|t| t.thread_id == id) {
                    return Err(bad("issues.not_found", format!("no issue {id:?}; issues.list lists them")));
                }
                Found::FileEngine(link)
            }
        }
        Source::Bcf(b) => {
            let pick = |r: RemoteCache| r.topics.into_iter().find(|t| t.guid() == id.to_lowercase());
            match pick(remote(app, &b, false)?.0) {
                Some(r) => Found::Bcf(b, r),
                None if in_model => Found::Model,
                None => match pick(remote(app, &b, true)?.0) {
                    Some(r) => Found::Bcf(b, r),
                    None => return Err(bad("issues.not_found", format!("no issue {id:?}; issues.list lists them"))),
                },
            }
        }
    };
    Ok((doc, found))
}

fn one(app: &mut App, doc: DocumentId, id: &str) -> Result<Issue> {
    let out = list(app, ListParams { document_id: Some(doc), refresh: false, status: None, text: None })?;
    out.issues.into_iter().find(|i| i.id.eq_ignore_ascii_case(id)).ok_or_else(|| bad("issues.not_found", format!("issue {id:?} is gone")))
}

/// Reads the server again, then the issue.
fn fresh(app: &mut App, doc: DocumentId, id: &str) -> Result<Issue> {
    list(app, ListParams { document_id: Some(doc), refresh: true, status: None, text: None })?;
    one(app, doc, id)
}

fn activate(app: &mut App, doc: DocumentId) -> Result<()> {
    if app.active() != Some(doc) {
        app.activate(doc)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
    /// The 3D view to show it in; the active one when 3D, else the default 3D view.
    pub target_view_id: Option<ElementRef>,
}

host!(Show, "issues.show", "Show an issue's viewpoint",
    "Shows an issue's (first) viewpoint in a 3D view, wherever the issue is kept (COL-BCF-003): camera and cut \
     as one undoable step, its components hidden or isolated, and the components it is about selected. \
     Returns what it applied and the GlobalIds not in the model.",
    [Edit], ShowParams, crate::bcf_files::ImportReport, |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    activate(app, doc)?;
    match found {
        Found::Model => serde_json::from_value(app.execute("bcf.apply_viewpoint", json!({ "topic_id": p.id, "target_view_id": p.target_view_id }), caller)?.result).map_err(|e| bad("issues.protocol", e.to_string())),
        Found::FileEngine(_) => serde_json::from_value(app.execute("fileengine.show_issue", json!({ "thread_id": p.id, "target_view_id": p.target_view_id }), caller)?.result).map_err(|e| bad("issues.protocol", e.to_string())),
        Found::Bcf(_, r) => {
            let v = r.viewpoints.first().ok_or_else(|| bad("issues.no_viewpoint", "this issue has no viewpoint"))?;
            let mut vp = bcf::from_json(v);
            if vp.selection.is_empty() {
                vp.selection = r.issue().objects;
            }
            crate::bcf_files::show(app, caller, &vp, &p.target_view_id)
        }
    }
});

host!(Snapshot, "issues.snapshot", "An issue's snapshot",
    "The picture taken with an issue's first viewpoint, as base64 PNG or JPEG, or null.",
    [Network, Read], IdParams, Option<String>, |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    let b64 = |b: Vec<u8>| base64::engine::general_purpose::STANDARD.encode(b);
    Ok(match found {
        Found::Model => {
            let t = model_topics(app, doc)?.into_iter().find(|t| t.guid.eq_ignore_ascii_case(&p.id));
            let store = app.get(doc)?.doc.store();
            t.and_then(|t| t.viewpoints.iter().find_map(|v| issues::snapshot(store, &v.guid))).map(b64)
        }
        Found::FileEngine(_) => serde_json::from_value(app.execute("fileengine.issue_snapshot", json!({ "document_id": doc, "thread_id": p.id }), caller)?.result).unwrap_or(None),
        Found::Bcf(b, r) => match r.viewpoints.first().map(|v| text(&v["guid"])) {
            Some(v) => app.coordination.with(&b.server_id, |c| c.snapshot(&b.project_id, r.topic["guid"].as_str().unwrap_or_default(), &v))?.map(b64),
            None => None,
        },
    })
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
    pub text: String,
}

host!(Comment, "issues.comment", "Reply to an issue",
    "Adds a comment to an issue where it is kept: on the server as the signed-in user, or on the model's own \
     topic. Returns the issue as it now is.",
    [Network, Edit], CommentParams, Issue, |app, caller, p| {
    let text = p.text.trim().to_owned();
    if text.is_empty() {
        return Err(bad("issues.invalid_input", "the comment is empty"));
    }
    let (doc, found) = find(app, p.document_id, &p.id)?;
    match found {
        Found::Model => {
            activate(app, doc)?;
            app.execute("bcf.add_comment", json!({ "topic_id": p.id, "text": text }), caller)?;
            one(app, doc, &p.id)
        }
        Found::FileEngine(link) => {
            crate::model_issues::comment(app, &link, &p.id, &text)?;
            fresh(app, doc, &p.id)
        }
        Found::Bcf(b, r) => {
            let guid = text_guid(&r);
            app.coordination.with(&b.server_id, |c| c.add_comment(&b.project_id, &guid, &json!({ "guid": issues::new_guid(), "comment": text })))?;
            fresh(app, doc, &p.id)
        }
    }
});

fn text_guid(r: &Remote) -> String {
    text(&r.topic["guid"])
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetStatusParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
    /// true resolves (closes) it, false opens it again.
    pub resolved: bool,
}

/// The status a server's list offers for resolving (or reopening).
fn status_for(lists: &Value, resolved: bool) -> String {
    let have: Vec<&str> = lists["statuses"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let wanted: &[&str] = if resolved { &["Closed", "Resolved", "Done"] } else { &["Open", "ReOpened", "Reopened", "Active", "In Progress"] };
    wanted
        .iter()
        .find_map(|w| have.iter().find(|h| h.eq_ignore_ascii_case(w)))
        .map(|s| (*s).to_owned())
        .or_else(|| have.iter().find(|h| is_resolved(h) == resolved).map(|s| (*s).to_owned()))
        .unwrap_or_else(|| if resolved { "Closed".into() } else { "Open".into() })
}

host!(SetStatus, "issues.set_status", "Resolve or reopen an issue",
    "Resolves (closes) an issue, or opens it again, where it is kept (COL-BCF-004): FileEngine records the \
     model version open here; a BCF server gets the status its project's list has for it (Closed, Resolved …). \
     Returns the issue.",
    [Network, Edit], SetStatusParams, Issue, |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    match found {
        Found::Model => {
            activate(app, doc)?;
            let lists = lists_json(&issues::lists(app.get(doc)?.doc.store()));
            let status = if p.resolved { "Resolved".to_owned() } else { status_for(&lists, false) };
            app.execute("bcf.update_topic", json!({ "topic_id": p.id, "fields": { "status": status } }), caller)?;
            one(app, doc, &p.id)
        }
        Found::FileEngine(_) => {
            app.execute("fileengine.resolve_issue", json!({ "document_id": doc, "thread_id": p.id, "resolved": p.resolved }), caller)?;
            fresh(app, doc, &p.id)
        }
        Found::Bcf(b, r) => {
            let (cache, _) = remote(app, &b, false)?;
            let lists = server_lists(&cache.extensions, &[]);
            let status = status_for(&lists, p.resolved);
            let guid = text_guid(&r);
            app.coordination.with(&b.server_id, |c| c.update_topic(&b.project_id, &guid, &json!({ "topic_status": status })))?;
            fresh(app, doc, &p.id)
        }
    }
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
    /// The fields to change: title, description, topic_type, status, priority, labels,
    /// assigned_to, due_date (YYYY-MM-DD), stage.
    pub fields: TopicFields,
}

/// Topic fields as the BCF API names them.
fn api_fields(f: &TopicFields) -> Value {
    let mut v = serde_json::Map::new();
    let mut put = |k: &str, x: &Option<String>| {
        if let Some(x) = x {
            v.insert(k.into(), if x.trim().is_empty() { Value::Null } else { json!(x.trim()) });
        }
    };
    put("title", &f.title);
    put("description", &f.description);
    put("topic_type", &f.topic_type);
    put("topic_status", &f.status);
    put("priority", &f.priority);
    put("assigned_to", &f.assigned_to);
    put("due_date", &f.due_date);
    put("stage", &f.stage);
    if let Some(l) = &f.labels {
        v.insert("labels".into(), json!(l));
    }
    if let Some(l) = &f.reference_links {
        v.insert("reference_links".into(), json!(l));
    }
    Value::Object(v)
}

host!(Update, "issues.update", "Change an issue's fields",
    "Changes an issue's title, description, type, status, priority, labels, assignee, due date or stage where \
     it is kept: the model's own topic, or the BCF server's. FileEngine issues have a title and comments only: \
     resolve them with issues.set_status. Returns the issue.",
    [Network, Edit], UpdateParams, Issue, |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    match found {
        Found::Model => {
            activate(app, doc)?;
            app.execute("bcf.update_topic", json!({ "topic_id": p.id, "fields": p.fields }), caller)?;
            one(app, doc, &p.id)
        }
        Found::FileEngine(_) => Err(bad("issues.not_supported", "FileEngine issues have a title and comments only; reply, or resolve with issues.set_status")),
        Found::Bcf(b, r) => {
            if p.fields.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
                return Err(bad("issues.invalid_input", "an issue needs a title"));
            }
            let guid = text_guid(&r);
            let fields = api_fields(&p.fields);
            app.coordination.with(&b.server_id, |c| c.update_topic(&b.project_id, &guid, &fields))?;
            fresh(app, doc, &p.id)
        }
    }
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewParams {
    pub document_id: Option<DocumentId>,
    /// The title; the description's first line when not given.
    pub title: Option<String>,
    /// What is wrong or asked (FileEngine: the opening comment).
    pub text: String,
    /// Type, status, priority, labels, assignee, due date, stage (not on FileEngine).
    #[serde(default)]
    pub fields: TopicFields,
    /// The 3D view whose camera, cut, hidden elements and selection it keeps (the active one when 3D).
    pub view_id: Option<ElementRef>,
    /// Keep it with the model even when the model's issues are on a server.
    #[serde(default)]
    pub in_model: bool,
}

fn title_from(title: Option<String>, text: &str) -> String {
    title.filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_owned()).unwrap_or_else(|| {
        let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Issue");
        if line.chars().count() > 100 { format!("{}…", line.chars().take(99).collect::<String>()) } else { line.to_owned() }
    })
}

/// A viewpoint as the BCF API takes it, with its snapshot.
fn api_viewpoint(guid: &str, vp: &Viewpoint, snapshot: Option<&[u8]>) -> Value {
    let mut v = bcf::to_json(vp);
    v["guid"] = json!(guid);
    if let Some(b) = snapshot {
        let kind = if b.starts_with(&[0xff, 0xd8]) { "jpg" } else { "png" };
        v["snapshot"] = json!({ "snapshot_type": kind, "snapshot_data": base64::engine::general_purpose::STANDARD.encode(b) });
    }
    v
}

host!(New, "issues.new", "Raise an issue",
    "Raises an issue from a 3D view (camera, section box and slices, hidden or isolated elements, the \
     selection) with a snapshot, where the model's issues are kept (COL-BCF-001, COL-BCF-004): a topic on the \
     bound BCF server's project, a thread on FileEngine, or a BCF topic kept with the model (`in_model`, or \
     when the model has no server). Returns the issue.",
    [Network, Edit], NewParams, Issue, |app, caller, p| {
    let text = p.text.trim().to_owned();
    if text.is_empty() && p.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
        return Err(bad("issues.invalid_input", "say what the issue is"));
    }
    let (doc, source) = source(app, p.document_id)?;
    activate(app, doc)?;
    let title = title_from(p.title.clone(), &text);
    match source {
        _ if p.in_model => new_in_model(app, caller, doc, title, text, p),
        Source::Model => new_in_model(app, caller, doc, title, text, p),
        Source::FileEngine(link) => {
            let (vp, snapshot) = capture(app, &p.view_id)?;
            let given = p.title.filter(|t| !t.trim().is_empty()).unwrap_or_default();
            let tid = crate::model_issues::open_thread(app, &link, &given, &text, &vp, snapshot)?;
            fresh(app, doc, &tid)
        }
        Source::Bcf(b) => {
            let (vp, snapshot) = capture(app, &p.view_id)?;
            let snapshot = snapshot.and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
            let guid = issues::new_guid();
            let mut body = api_fields(&p.fields);
            body["guid"] = json!(guid);
            body["title"] = json!(title);
            if !text.is_empty() && text != title {
                body["description"] = json!(text);
            }
            if body["topic_type"].is_null() {
                body["topic_type"] = json!("Issue");
            }
            if body["topic_status"].is_null() {
                body["topic_status"] = json!("Open");
            }
            if let Some(o) = body.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            let vguid = vp.guid.clone().unwrap_or_else(issues::new_guid);
            let viewpoint = api_viewpoint(&vguid, &vp, snapshot.as_deref());
            let made = app.coordination.with(&b.server_id, |c| {
                let t = c.create_topic(&b.project_id, &body)?;
                let g = t["guid"].as_str().unwrap_or(&guid).to_owned();
                c.add_viewpoint(&b.project_id, &g, &viewpoint)?;
                Ok(g)
            })?;
            fresh(app, doc, &made)
        }
    }
});

/// The view's viewpoint and snapshot (base64) for a new issue; with no 3D view, one with no camera.
fn capture(app: &App, view: &Option<ElementRef>) -> Result<(Viewpoint, Option<String>)> {
    match crate::bcf_topics::capture_view(app, view) {
        Ok((v, vp)) => Ok((vp, crate::bcf_topics::snapshot_png(app, v))),
        Err(e) if e.code == "bcf.not_3d" && view.is_none() => Ok((Viewpoint { guid: Some(issues::new_guid()), ..Viewpoint::default() }, None)),
        Err(e) => Err(e),
    }
}

fn new_in_model(app: &mut App, caller: &Caller, doc: DocumentId, title: String, text: String, p: NewParams) -> Result<Issue> {
    let description = (!text.is_empty() && text != title).then_some(text);
    let out = app.execute("bcf.create_topic", json!({ "title": title, "description": description, "fields": p.fields, "view_id": p.view_id }), caller)?;
    let id = riviera_core::ElementId(out.result.as_u64().unwrap_or_default());
    let guid = app.get(doc)?.doc.store().get(id).and_then(issues::topic).map(|t| t.guid).ok_or_else(|| bad("issues.protocol", "the topic was not made"))?;
    one(app, doc, &guid)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddViewParams {
    pub document_id: Option<DocumentId>,
    pub id: String,
    /// The 3D view to capture; the active view when it is 3D.
    pub view_id: Option<ElementRef>,
}

host!(AddView, "issues.add_view", "Add the view to an issue",
    "Adds a 3D view (camera, cut, hidden elements, selection) with its snapshot as another viewpoint of an \
     issue: the model's own topic or the BCF server's (FileEngine issues keep one). Returns the issue.",
    [Network, Edit], AddViewParams, Issue, |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    activate(app, doc)?;
    match found {
        Found::Model => {
            app.execute("bcf.add_viewpoint", json!({ "topic_id": p.id, "view_id": p.view_id }), caller)?;
            one(app, doc, &p.id)
        }
        Found::FileEngine(_) => Err(bad("issues.not_supported", "a FileEngine issue keeps the one viewpoint it was raised with; raise another, or reply")),
        Found::Bcf(b, r) => {
            let (view, vp) = crate::bcf_topics::capture_view(app, &p.view_id)?;
            let snapshot = crate::bcf_topics::snapshot_png(app, view).and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
            let v = api_viewpoint(&vp.guid.clone().unwrap_or_else(issues::new_guid), &vp, snapshot.as_deref());
            let guid = text_guid(&r);
            app.coordination.with(&b.server_id, |c| c.add_viewpoint(&b.project_id, &guid, &v))?;
            fresh(app, doc, &p.id)
        }
    }
});

host!(Delete, "issues.delete", "Delete an issue",
    "Deletes an issue where it is kept: the model's own topic (undoable), or the topic on the BCF server \
     (FileEngine issues are resolved, not deleted).",
    [Network, Edit], IdParams, (), |app, caller, p| {
    let (doc, found) = find(app, p.document_id, &p.id)?;
    match found {
        Found::Model => {
            activate(app, doc)?;
            app.execute("bcf.delete_topic", json!({ "topic_ids": [p.id] }), caller).map(drop)
        }
        Found::FileEngine(_) => Err(bad("issues.not_supported", "FileEngine issues are resolved, not deleted")),
        Found::Bcf(b, r) => {
            let guid = text_guid(&r);
            app.coordination.with(&b.server_id, |c| c.delete_topic(&b.project_id, &guid))?;
            list(app, ListParams { document_id: Some(doc), refresh: true, status: None, text: None }).map(drop)
        }
    }
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishParams {
    pub document_id: Option<DocumentId>,
    /// The model's topics to publish (GUIDs); all not on the server when omitted.
    pub ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PublishOut {
    /// The topics put on the server, and the issue id each has there.
    pub published: Vec<(String, String)>,
    /// Already there.
    pub already: usize,
    /// Topics that could not be put there, and why.
    pub failed: Vec<(String, String)>,
}

/// A comment's text with who wrote it and when, for a server that records the poster only.
fn signed(c: &issues::Comment) -> String {
    format!("{} ({}): {}", c.author, c.date.get(..10).unwrap_or(&c.date), c.text)
}

host!(Publish, "issues.publish", "Put the model's own issues on the server",
    "Puts BCF topics kept with the model (from clash checks, the bathroom checks, a BCF import, or made before \
     the model was bound) on the server its issues are kept on (COL-BCF-004): a BCF server gets each topic with \
     its GUID, fields, viewpoints, snapshots and comments (their authors and dates written into the text); \
     FileEngine gets a thread anchored to the first viewpoint, with the description and comments. Topics \
     already there are skipped. The model's copies stay; issues.list then shows the server's.",
    [Network, Read], PublishParams, PublishOut, |app, caller, p| {
    let (doc, source) = source(app, p.document_id)?;
    let path = app.get(doc)?.project.path().to_path_buf();
    let wanted = |t: &Topic| p.ids.as_ref().is_none_or(|ids| ids.iter().any(|i| i.eq_ignore_ascii_case(&t.guid)));
    let topics: Vec<Topic> = model_topics(app, doc)?.into_iter().filter(|t| wanted(t)).collect();
    let store = app.get(doc)?.doc.store();
    let pictures: BTreeMap<String, Vec<u8>> = topics.iter().flat_map(|t| t.viewpoints.iter()).filter_map(|v| Some((v.guid.clone(), issues::snapshot(store, &v.guid)?))).collect();
    let mut out = PublishOut { published: Vec::new(), already: 0, failed: Vec::new() };
    match source {
        Source::Model => return Err(bad("issues.no_server", "this model's issues are kept with it; bind it to a BCF server (coordination.bind) or open it from FileEngine")),
        Source::FileEngine(link) => {
            let mut done = published(&path);
            for t in &topics {
                let key = t.guid.to_lowercase();
                if done.contains_key(&key) {
                    out.already += 1;
                    continue;
                }
                let vp = t.viewpoints.first().map(|v| v.viewpoint.clone()).unwrap_or_default();
                let snapshot = t.viewpoints.first().and_then(|v| pictures.get(&v.guid)).map(|b| base64::engine::general_purpose::STANDARD.encode(b));
                let mut body = t.description.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| t.title.clone());
                let mut meta = Vec::new();
                for (k, v) in [("Type", &t.topic_type), ("Priority", &t.priority), ("Assigned to", &t.assigned_to), ("Due", &t.due_date)] {
                    if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                        meta.push(format!("{k}: {v}"));
                    }
                }
                if !meta.is_empty() {
                    body.push_str(&format!("\n\n{}", meta.join(" · ")));
                }
                match crate::model_issues::open_thread(app, &link, &t.title, &body, &vp, snapshot) {
                    Ok(tid) => {
                        for c in &t.comments {
                            let _ = crate::model_issues::comment(app, &link, &tid, &signed(c));
                        }
                        if t.status.as_deref().is_some_and(is_resolved) {
                            let _ = app.execute("fileengine.resolve_issue", json!({ "document_id": doc, "thread_id": tid, "resolved": true }), caller);
                        }
                        done.insert(key, tid.clone());
                        out.published.push((t.guid.clone(), tid));
                    }
                    Err(e) => out.failed.push((t.guid.clone(), e.message.clone())),
                }
            }
            let _ = std::fs::write(path.with_file_name(PUBLISHED), serde_json::to_vec_pretty(&done).expect("serialises"));
            crate::model_issues::list(app, Some(doc), true)?;
        }
        Source::Bcf(b) => {
            let (r, _) = remote(app, &b, true)?;
            let there: Vec<String> = r.topics.iter().map(Remote::guid).collect();
            let me = app.coordination.with(&b.server_id, Client::current_user).ok();
            let mine = |a: &str| me.as_ref().is_some_and(|u| u["id"] == a || u["name"] == a);
            for t in &topics {
                if there.contains(&t.guid.to_lowercase()) {
                    out.already += 1;
                    continue;
                }
                let mut body = api_fields(&TopicFields {
                    title: Some(t.title.clone()),
                    description: t.description.clone(),
                    topic_type: t.topic_type.clone(),
                    status: t.status.clone(),
                    priority: t.priority.clone(),
                    labels: Some(t.labels.clone()),
                    assigned_to: t.assigned_to.clone(),
                    due_date: t.due_date.clone(),
                    stage: t.stage.clone(),
                    related_topics: None,
                    reference_links: (!t.reference_links.is_empty()).then(|| t.reference_links.clone()),
                });
                body["guid"] = json!(t.guid);
                if let Some(o) = body.as_object_mut() {
                    o.retain(|_, v| !v.is_null());
                }
                let project = b.project_id.clone();
                let r = app.coordination.with(&b.server_id, |c| {
                    let made = c.create_topic(&project, &body)?;
                    let g = made["guid"].as_str().unwrap_or(&t.guid).to_owned();
                    for v in &t.viewpoints {
                        c.add_viewpoint(&project, &g, &api_viewpoint(&v.guid, &v.viewpoint, pictures.get(&v.guid).map(Vec::as_slice)))?;
                    }
                    for cm in &t.comments {
                        let text = if mine(&cm.author) { cm.text.clone() } else { signed(cm) };
                        c.add_comment(&project, &g, &json!({ "guid": cm.guid, "comment": text, "viewpoint_guid": cm.viewpoint }))?;
                    }
                    Ok(g)
                });
                match r {
                    Ok(g) => out.published.push((t.guid.clone(), g)),
                    Err(e) => out.failed.push((t.guid.clone(), e.message.clone())),
                }
            }
            remote(app, &b, true)?;
        }
    }
    Ok(out)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub document_id: Option<DocumentId>,
    #[schemars(extend("x-riviera-file" = "write"))]
    pub path: String,
    /// The issues to write; all when omitted.
    pub ids: Option<Vec<String>>,
    /// `3.0` (default) or `2.1`.
    pub version: Option<String>,
}

host!(Export, "issues.export_bcf", "Export the model's issues as BCF",
    "Writes the model's issues, wherever they are kept (the BCF server's, FileEngine's, and the model's own), \
     as a BCF archive (3.0 or 2.1) with their viewpoints, snapshots and comments: for tools that read files \
     rather than the server.",
    [Network, FileWrite], ExportParams, crate::bcf_topics::ExportReport, |app, caller, p| {
    use riviera_core::bcf_archive::Version;
    let version = match &p.version {
        Some(v) => Version::parse(v).ok_or_else(|| bad("bcf.bad_version", "version is 2.1 or 3.0"))?,
        None => Version::V3_0,
    };
    let (doc, source) = source(app, p.document_id)?;
    let wanted = |g: &str| p.ids.as_ref().is_none_or(|ids| ids.iter().any(|i| i.eq_ignore_ascii_case(g)));
    let local = model_topics(app, doc)?;
    let store = app.get(doc)?.doc.store();
    let with_pictures = |t: &Topic| -> Vec<(String, Vec<u8>)> { t.viewpoints.iter().filter_map(|v| Some((v.guid.clone(), issues::snapshot(store, &v.guid)?))).collect() };
    let mut topics: Vec<crate::bcf_topics::WithSnapshots> = Vec::new();
    match source {
        Source::Model => topics.extend(local.iter().filter(|t| wanted(&t.guid)).map(|t| (t.clone(), with_pictures(t)))),
        Source::FileEngine(_) => {
            let path = app.get(doc)?.project.path().to_path_buf();
            let done = published(&path);
            topics.extend(local.iter().filter(|t| wanted(&t.guid) && !done.contains_key(&t.guid.to_lowercase())).map(|t| (t.clone(), with_pictures(t))));
            let fe = crate::model_issues::list(app, Some(doc), false)?;
            for i in fe.issues.into_iter().filter(|i| wanted(&i.thread_id)) {
                let picture: Option<String> = app.execute("fileengine.issue_snapshot", json!({ "document_id": doc, "thread_id": i.thread_id }), caller).ok().and_then(|o| o.result.as_str().map(str::to_owned));
                let vguid = i.viewpoint.as_ref().and_then(|v| v.guid.clone()).unwrap_or_else(issues::new_guid);
                let t = Topic {
                    guid: i.thread_id.clone(),
                    title: i.title.clone(),
                    status: Some(if is_resolved(&i.status) { "Closed".into() } else { "Open".into() }),
                    topic_type: Some("Comment".into()),
                    creation_date: i.created.clone(),
                    creation_author: i.opened_by.clone(),
                    modified_date: Some(i.updated.clone()),
                    comments: i.comments.iter().map(|c| issues::Comment { guid: c.id.clone(), date: c.date.clone(), author: c.author.clone(), text: c.text.clone(), viewpoint: Some(vguid.clone()), modified_date: None, modified_author: None }).collect(),
                    viewpoints: i.viewpoint.clone().map(|v| TopicViewpoint { guid: vguid.clone(), viewpoint: v, snapshot: None }).into_iter().collect(),
                    ..Topic::default()
                };
                let pics = picture.and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()).map(|b| vec![(vguid, b)]).unwrap_or_default();
                topics.push((t, pics));
            }
        }
        Source::Bcf(b) => {
            let (r, _) = remote(app, &b, false)?;
            let there: Vec<String> = r.topics.iter().map(Remote::guid).collect();
            let store = app.get(doc)?.doc.store();
            topics.extend(local.iter().filter(|t| wanted(&t.guid) && !there.contains(&t.guid.to_lowercase())).map(|t| (t.clone(), t.viewpoints.iter().filter_map(|v| Some((v.guid.clone(), issues::snapshot(store, &v.guid)?))).collect())));
            for rt in r.topics.iter().filter(|t| wanted(&t.guid())) {
                let t = rt.topic();
                let guid = text_guid(rt);
                let mut pics = Vec::new();
                for v in &t.viewpoints {
                    if let Ok(Some(bytes)) = app.coordination.with(&b.server_id, |c| c.snapshot(&b.project_id, &guid, &v.guid)) {
                        pics.push((v.guid.clone(), bytes));
                    }
                }
                topics.push((t, pics));
            }
        }
    }
    let all: Vec<Topic> = topics.iter().map(|(t, _)| t.clone()).collect();
    crate::bcf_topics::write_archive(&p.path, version, &issues::lists_of(&all), &topics)
});
