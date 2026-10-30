// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `bimserver.*`: BIMserver client mode (spec Ch 6 §6.1–6.2, COL-MODE-001/002, COL-BIM-001…004,
//! COL-OFF-001), over the MIT client `reviera_bimserver`.
//!
//! - **Server profiles** (name, URL, the user name last used and how they log in) are per-user
//!   settings (`bimserver.json` in the settings folder), never project data. The password or
//!   user token lives in the OS keychain when *remember* is on (or for this session where there
//!   is none); no command returns it. A session's token is kept in memory, and when the server
//!   refuses it (expired, restarted) the call logs in again once with what is stored.
//! - **Working copies.** An opened server project is a folder
//!   `<data>/reviera/bimserver/<server-id>/<poid>/` holding `working.ifc` (the model, saved by
//!   Ctrl+S like any file), `base.ifc` (the revision it is based on, for the merges to come) and
//!   `state.json` (server, project, base revision). A document is in BIMserver mode when its
//!   file is a working copy with its `state.json`, so the binding survives a restart and a file
//!   copied elsewhere carries none.
//! - **Check-in** saves the working copy and uploads it as a new revision, refused while the
//!   server has a newer revision than the base (get latest and merge come next). **Publish**
//!   checks a Local model in as a project's first revision (or, with `replace`, a newer one) and
//!   turns it into that project's working copy.
//!
//! - **Get latest** downloads the newest revision as `remote.ifc` and merges it into the open
//!   working copy three-way (`reviera_core::merge`, base = `base.ifc`), as one undoable
//!   transaction; the downloaded revision becomes the base and the merged working copy is
//!   saved. Conflicts are reported and nothing is changed unless `conflicts` says which side
//!   wins. **Synchronise** is get latest (when the server moved on) and check-in.
//!
//! `bimserver.login` refuses a password or token from an agent (MCP), and
//! `bimserver.remove_server` is not offered to agents. Not yet: the sync lock and the race
//! check (COL-OWN-002), conflicts decided one by one (COL-MRG-002), notifications, history and restore,
//! ownership, the streamed upload for large models, the calls off the UI thread, and the File ›
//! Open › From BIMserver dialog.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reviera_bimserver::{Client, ErrorKind, Project, Revision};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result, Source};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

/// The keychain service the secrets are stored under (the account is the server profile's id).
pub const KEYCHAIN_SERVICE: &str = "org.bimreviera.bimserver";
/// Commands not offered to agents: they remove stored credentials.
pub const NOT_FOR_AGENTS: &[&str] = &["bimserver.remove_server"];
/// The working copy's files.
pub const WORKING: &str = "working.ifc";
pub const BASE: &str = "base.ifc";
pub const STATE: &str = "state.json";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn failed(e: reviera_bimserver::Error) -> Error {
    let code = match e.kind {
        ErrorKind::Url => "bimserver.bad_url",
        ErrorKind::Transport => "bimserver.unreachable",
        ErrorKind::Auth => "bimserver.not_authorised",
        ErrorKind::Server => "bimserver.refused",
        ErrorKind::Protocol => "bimserver.protocol",
    };
    Error::new(code, e.message.clone()).with_details(serde_json::json!({ "exception": e.exception }))
}

/// How a profile logs in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoginKind {
    #[default]
    Password,
    /// A user token (BIMserver shows one on the user's page).
    UserToken,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    servers: Vec<Profile>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Profile {
    id: String,
    name: String,
    url: String,
    /// Plain http to a host other than this computer, accepted by the user.
    allow_http: bool,
    username: Option<String>,
    login: LoginKind,
    /// The server's version when last reached.
    version: Option<String>,
}

/// Where a secret is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    Keychain,
    /// Memory, until Reviera closes.
    Session,
    /// Not stored: the next login asks for it.
    None,
}

/// The servers' working state on the app.
#[derive(Default)]
pub struct Servers {
    /// The settings file and the working copies' root; the per-user ones when `None`.
    settings: Option<PathBuf>,
    data: Option<PathBuf>,
    /// Keeps secrets in memory only (tests, or no keychain).
    session_only: bool,
    session: HashMap<String, String>,
    clients: HashMap<String, Client>,
}

impl std::fmt::Debug for Servers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Servers").field("settings", &self.settings).field("data", &self.data).finish_non_exhaustive()
    }
}

/// The per-user data folder: `%LOCALAPPDATA%\Reviera`, else `$XDG_DATA_HOME/reviera` (or
/// `~/.local/share/reviera`).
fn data_dir() -> Option<PathBuf> {
    if let Some(a) = std::env::var_os("LOCALAPPDATA") {
        return Some(Path::new(&a).join("Reviera"));
    }
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))?;
    Some(base.join("reviera"))
}

impl Servers {
    /// Keeps the settings in `settings`, the working copies under `data` and the secrets in
    /// memory: for tests and tools.
    pub fn isolated(settings: PathBuf, data: PathBuf) -> Self {
        Self { settings: Some(settings), data: Some(data), session_only: true, ..Self::default() }
    }

    fn file(&self) -> Result<PathBuf> {
        self.settings.clone().or_else(|| crate::subscriptions::config_dir().map(|d| d.join("bimserver.json"))).ok_or_else(|| bad("bimserver.no_settings", "there is no per-user settings folder"))
    }

    fn root(&self) -> Result<PathBuf> {
        self.data.clone().or_else(|| data_dir().map(|d| d.join("bimserver"))).ok_or_else(|| bad("bimserver.no_settings", "there is no per-user data folder"))
    }

    fn load(&self) -> Result<Settings> {
        match std::fs::read(self.file()?) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| bad("bimserver.settings_invalid", format!("bimserver.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(bad("bimserver.settings_unreadable", e.to_string())),
        }
    }

    fn save(&self, s: &Settings) -> Result<()> {
        let path = self.file()?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| bad("bimserver.settings_unwritable", e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s).expect("settings serialise")).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| bad("bimserver.settings_unwritable", e.to_string()))
    }

    fn keychain(&self) -> bool {
        !self.session_only && keyring::Entry::store_status().is_ok()
    }

    fn put_secret(&mut self, id: &str, secret: &str) -> Storage {
        if self.keychain() && keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.set_password(secret)).is_ok() {
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
        keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.get_password()).ok()
    }

    fn storage(&self, id: &str) -> Storage {
        if self.session.contains_key(id) {
            Storage::Session
        } else if self.keychain() && keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.get_password()).is_ok() {
            Storage::Keychain
        } else {
            Storage::None
        }
    }

    fn delete_secret(&mut self, id: &str) {
        self.session.remove(id);
        if self.keychain() {
            let _ = keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.delete_credential());
        }
    }

    fn profile(&self, id: &str) -> Result<Profile> {
        self.load()?.servers.into_iter().find(|p| p.id == id).ok_or_else(|| bad("bimserver.no_server", format!("no server profile {id:?}; bimserver.list_servers lists them")))
    }

    /// Logs `p` in with its stored secret.
    fn relogin(&mut self, p: &Profile) -> Result<()> {
        let secret = self.secret(&p.id).ok_or_else(|| bad("bimserver.not_logged_in", format!("log in to {} first (bimserver.login)", p.name)))?;
        let c = self.client_for(p)?;
        match p.login {
            LoginKind::Password => c.login(p.username.as_deref().unwrap_or_default(), &secret),
            LoginKind::UserToken => c.login_user_token(&secret),
        }
        .map_err(failed)
    }

    fn client_for(&mut self, p: &Profile) -> Result<&mut Client> {
        if !self.clients.contains_key(&p.id) {
            let c = Client::new(&p.url, p.allow_http).map_err(failed)?;
            self.clients.insert(p.id.clone(), c);
        }
        Ok(self.clients.get_mut(&p.id).expect("inserted"))
    }

    /// Runs `f` on a logged-in client for server `id`, logging in again once if the server
    /// refuses the session.
    fn with<T>(&mut self, id: &str, f: impl Fn(&Client) -> reviera_bimserver::Result<T>) -> Result<T> {
        let p = self.profile(id)?;
        if !self.client_for(&p)?.logged_in() {
            self.relogin(&p)?;
        }
        match f(self.clients.get(&p.id).expect("made")) {
            Err(e) if e.kind == ErrorKind::Auth && self.secret(&p.id).is_some() => {
                self.clients.get_mut(&p.id).expect("made").forget();
                self.relogin(&p)?;
                f(self.clients.get(&p.id).expect("made")).map_err(failed)
            }
            r => r.map_err(failed),
        }
    }

    /// The working-copy folder of project `poid` on server `id`.
    fn folder(&self, id: &str, poid: i64) -> Result<PathBuf> {
        Ok(self.root()?.join(id).join(poid.to_string()))
    }
}

/// A working copy's client state (`state.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkingState {
    pub server_id: String,
    pub url: String,
    pub poid: i64,
    pub project: String,
    pub schema: String,
    /// The revision the working copy is based on, and its number.
    pub base_roid: i64,
    pub base_revision: i64,
    /// Who checked out (the server's user name).
    pub user: String,
    /// Unix seconds of the last download or check-in.
    pub synced_at: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The working-copy state of a document's file, if it is a working copy.
pub fn state_of(path: &Path) -> Option<WorkingState> {
    if path.file_name()? != WORKING {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path.with_file_name(STATE)).ok()?).ok()
}

fn write_state(folder: &Path, s: &WorkingState) -> Result<()> {
    std::fs::write(folder.join(STATE), serde_json::to_vec_pretty(s).expect("serialises")).map_err(|e| bad("bimserver.working_copy", e.to_string()))
}

fn not_for_agents(caller: &Caller, what: &str) -> Result<()> {
    if caller.origin.source == Source::Mcp {
        return Err(Error::new("permission.denied", format!("{what} is for the user (the BIMserver login dialog) and Lua, never for agents")));
    }
    Ok(())
}

impl App {
    /// The BIMserver profiles' state (settings file, secrets, sessions).
    pub fn bimserver_mut(&mut self) -> &mut Servers {
        &mut self.bimserver
    }

    /// The document to act on and its working-copy state.
    fn working_copy(&self, id: Option<DocumentId>) -> Result<(DocumentId, PathBuf, WorkingState)> {
        let id = self.resolve(id)?;
        let path = self.get(id)?.project.path().to_owned();
        let st = state_of(&path).ok_or_else(|| bad("mode.not_available", "the project is not a BIMserver working copy (File › Open › From BIMserver, or Publish to BIMserver)"))?;
        Ok((id, path, st))
    }
}

/// A trailer on every check-in comment: which program made the revision.
fn comment_with_trailer(comment: &str) -> Result<String> {
    let c = comment.trim();
    if c.is_empty() {
        return Err(bad("bimserver.no_comment", "a check-in needs a comment"));
    }
    Ok(format!("{c}\n[reviera] BIM Reviera {}", env!("CARGO_PKG_VERSION")))
}

// ---- Server profiles and login (COL-BIM-001) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct ServerOut {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: Option<String>,
    pub login: LoginKind,
    pub version: Option<String>,
    /// Where its password or token is kept.
    pub secret: Storage,
    pub connected: bool,
}

fn server_out(s: &Servers, p: &Profile) -> ServerOut {
    ServerOut { id: p.id.clone(), name: p.name.clone(), url: p.url.clone(), username: p.username.clone(), login: p.login, version: p.version.clone(), secret: s.storage(&p.id), connected: s.clients.get(&p.id).is_some_and(Client::logged_in) }
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddServerParams {
    pub name: String,
    /// The server's root URL (where its `/json` is), https:// (http:// for localhost).
    pub url: String,
    /// Accept plain http:// to another computer (the password then crosses the network unencrypted).
    #[serde(default)]
    pub allow_http: bool,
}

host!(BimAddServer, "bimserver.add_server", "Add a BIMserver",
    "Adds a server profile (COL-BIM-001): a name and the server's root URL (https://; http:// only for \
     localhost unless `allow_http`). The server is asked for its version first; nothing is saved if it \
     cannot be reached. Profiles are per-user settings, not project data. Returns the profile.",
    [Settings, Network], AddServerParams, ServerOut, |app, caller, p| {
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("bimserver.invalid_input", "a server needs a name"));
    }
    let c = Client::new(&p.url, p.allow_http).map_err(failed)?;
    let info = c.server_info().map_err(failed)?;
    if info.state != "RUNNING" {
        return Err(bad("bimserver.not_running", format!("the server says it is {} (it must be set up and running)", info.state)));
    }
    let s = &mut app.bimserver;
    let mut settings = s.load()?;
    let url = c.url().to_owned();
    if settings.servers.iter().any(|x| x.url == url) {
        return Err(bad("bimserver.duplicate", format!("{url} is already a server profile")));
    }
    // Unique across settings files, so a new profile never finds an old profile's secret in the
    // keychain under its id.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut n = 0u32;
    let id = loop {
        let id = format!("server-{:x}", (nanos as u64) ^ (u64::from(std::process::id()) << 32) ^ u64::from(n));
        if settings.servers.iter().all(|x| x.id != id) {
            break id;
        }
        n += 1;
    };
    s.delete_secret(&id);
    let profile = Profile { id: id.clone(), name, url, allow_http: p.allow_http, username: None, login: LoginKind::Password, version: Some(info.version) };
    settings.servers.push(profile.clone());
    s.save(&settings)?;
    s.clients.insert(id, c);
    Ok(server_out(s, &profile))
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServerIdParams {
    pub server_id: String,
}

host!(BimRemoveServer, "bimserver.remove_server", "Remove a BIMserver",
    "Removes a server profile and its stored password or token (COL-BIM-001). Working copies stay on disk. \
     Not available to MCP.",
    [Settings, Destructive], ServerIdParams, (), |app, caller, p| {
    not_for_agents(caller, "removing a server's stored credentials")?;
    let s = &mut app.bimserver;
    let mut settings = s.load()?;
    let before = settings.servers.len();
    settings.servers.retain(|x| x.id != p.server_id);
    if settings.servers.len() == before {
        return Err(bad("bimserver.no_server", format!("no server profile {:?}", p.server_id)));
    }
    s.save(&settings)?;
    s.delete_secret(&p.server_id);
    s.clients.remove(&p.server_id);
    Ok(())
});

host!(BimListServers, "bimserver.list_servers", "List BIMserver profiles",
    "The server profiles: id, name, URL, user name, how they log in, version, where the secret is kept \
     (keychain, session or none; never the secret) and whether a session is open.",
    [Read], Option<serde_json::Value>, Vec<ServerOut>, |app, caller, p| {
    let s = &app.bimserver;
    Ok(s.load()?.servers.iter().map(|x| server_out(s, x)).collect())
});

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginParams {
    pub server_id: String,
    /// The user name (for a password login); the profile's last one when absent.
    pub username: Option<String>,
    pub password: Option<String>,
    /// A user token instead of a user name and password.
    pub user_token: Option<String>,
    /// Keep the password or token in the OS keychain for the next session (default true).
    #[serde(default = "yes")]
    pub remember: bool,
}

impl std::fmt::Debug for LoginParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginParams").field("server_id", &self.server_id).field("username", &self.username).finish_non_exhaustive()
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionInfo {
    pub server_id: String,
    pub username: String,
    pub name: String,
    /// ADMIN, USER, READ_ONLY …
    pub user_type: String,
    pub version: Option<String>,
    pub secret: Storage,
}

host!(BimLogin, "bimserver.login", "Log in to a BIMserver",
    "Logs in (COL-BIM-001) with a user name and password, or a user token; with neither, with the stored \
     ones. `remember` (default true) keeps them in the OS keychain (this session only where there is \
     none). An agent (MCP) may only log in with stored credentials. Returns the user.",
    [Network, Settings], LoginParams, SessionInfo, |app, caller, p| {
    if p.password.is_some() || p.user_token.is_some() {
        not_for_agents(caller, "a password or token")?;
    }
    let s = &mut app.bimserver;
    let mut settings = s.load()?;
    let at = settings.servers.iter().position(|x| x.id == p.server_id).ok_or_else(|| bad("bimserver.no_server", format!("no server profile {:?}", p.server_id)))?;
    let mut profile = settings.servers[at].clone();
    let fresh = match (p.password.filter(|x| !x.is_empty()), p.user_token.filter(|x| !x.is_empty())) {
        (Some(pw), None) => {
            let user = p.username.clone().or(profile.username.clone()).filter(|u| !u.trim().is_empty()).ok_or_else(|| bad("bimserver.invalid_input", "a password login needs the user name"))?;
            profile.username = Some(user.trim().to_owned());
            profile.login = LoginKind::Password;
            Some(pw)
        }
        (None, Some(t)) => {
            profile.login = LoginKind::UserToken;
            Some(t)
        }
        (Some(_), Some(_)) => return Err(bad("bimserver.invalid_input", "give a password or a user token, not both")),
        (None, None) => None,
    };
    let c = s.client_for(&profile)?;
    match &fresh {
        Some(secret) => match profile.login {
            LoginKind::Password => c.login(profile.username.as_deref().unwrap_or_default(), secret),
            LoginKind::UserToken => c.login_user_token(secret),
        }
        .map_err(failed)?,
        None => s.relogin(&profile)?,
    }
    let c = s.clients.get(&profile.id).expect("made");
    let user = c.logged_in_user().map_err(failed)?;
    if let Ok(info) = c.server_info() {
        profile.version = Some(info.version);
    }
    if profile.login == LoginKind::UserToken {
        profile.username = Some(user.username.clone());
    }
    match fresh {
        Some(secret) if p.remember => {
            s.put_secret(&profile.id, &secret);
        }
        Some(_) => s.delete_secret(&profile.id),
        None => {}
    }
    settings.servers[at] = profile.clone();
    s.save(&settings)?;
    Ok(SessionInfo { server_id: profile.id.clone(), username: user.username, name: user.name, user_type: user.user_type, version: profile.version, secret: s.storage(&profile.id) })
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogoutParams {
    pub server_id: String,
    /// Also forget the stored password or token.
    #[serde(default)]
    pub forget: bool,
}

host!(BimLogout, "bimserver.logout", "Log out of a BIMserver",
    "Ends the session on the server (COL-BIM-001); `forget` also removes the stored password or token.",
    [Network, Settings], LogoutParams, (), |app, caller, p| {
    let s = &mut app.bimserver;
    s.profile(&p.server_id)?;
    if let Some(c) = s.clients.get_mut(&p.server_id) {
        let _ = c.logout();
    }
    if p.forget {
        s.delete_secret(&p.server_id);
    }
    Ok(())
});

// ---- Projects and revisions (COL-BIM-002) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectOut {
    pub poid: i64,
    pub name: String,
    pub schema: String,
    pub parent_poid: Option<i64>,
    pub sub_projects: Vec<i64>,
    pub last_roid: Option<i64>,
    pub revisions: usize,
    /// There is a working copy of it on this computer.
    pub working_copy: bool,
}

fn project_out(s: &Servers, id: &str, p: &Project) -> ProjectOut {
    let working = s.folder(id, p.oid).is_ok_and(|f| f.join(STATE).is_file());
    ProjectOut { poid: p.oid, name: p.name.clone(), schema: p.schema.clone(), parent_poid: p.parent, sub_projects: p.sub_projects.clone(), last_roid: p.last_revision, revisions: p.revisions.len(), working_copy: working }
}

host!(BimListProjects, "bimserver.list_projects", "List a BIMserver's projects",
    "The projects and sub-projects the user can see (COL-BIM-002): poid, name, schema, parent, \
     sub-projects, latest revision, revision count, and whether there is a working copy here.",
    [Read, Network], ServerIdParams, Vec<ProjectOut>, |app, caller, p| {
    let s = &mut app.bimserver;
    let projects = s.with(&p.server_id, Client::projects)?;
    Ok(projects.iter().map(|x| project_out(s, &p.server_id, x)).collect())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectParams {
    pub server_id: String,
    pub poid: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RevisionOut {
    pub roid: i64,
    pub number: i64,
    pub comment: String,
    /// Unix milliseconds.
    pub date: i64,
    pub user: String,
    pub size: i64,
    pub tag: Option<String>,
}

host!(BimListRevisions, "bimserver.list_revisions", "List a project's revisions",
    "A project's revisions, oldest first (COL-BIM-002): roid, number, comment, date (Unix ms), the user \
     who checked it in, size (objects) and tag.",
    [Read, Network], ProjectParams, Vec<RevisionOut>, |app, caller, p| {
    let s = &mut app.bimserver;
    let revs: Vec<Revision> = s.with(&p.server_id, |c| c.revisions(p.poid))?;
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut out = Vec::new();
    for r in revs {
        let user = match names.get(&r.user) {
            Some(n) => n.clone(),
            None => {
                let n = s.with(&p.server_id, |c| c.user_name(r.user)).unwrap_or_else(|_| r.user.to_string());
                names.insert(r.user, n.clone());
                n
            }
        };
        out.push(RevisionOut { roid: r.oid, number: r.number, comment: r.comment, date: r.date, user, size: r.size, tag: r.tag });
    }
    Ok(out)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectParams {
    pub server_id: String,
    pub name: String,
    /// `ifc4` (default) or `ifc2x3tc1`.
    pub schema: Option<String>,
    pub parent_poid: Option<i64>,
}

host!(BimCreateProject, "bimserver.create_project", "Create a BIMserver project",
    "A new project, or a sub-project of `parent_poid` (COL-BIM-002), of `schema` (ifc4 by default). The \
     server decides who may (by default only administrators make top-level projects). Returns the project.",
    [Network], CreateProjectParams, ProjectOut, |app, caller, p| {
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("bimserver.invalid_input", "a project needs a name"));
    }
    let schema = p.schema.unwrap_or_else(|| "ifc4".into()).to_lowercase();
    if !matches!(schema.as_str(), "ifc4" | "ifc2x3tc1") {
        return Err(bad("bimserver.invalid_input", "the schema is ifc4 or ifc2x3tc1"));
    }
    let s = &mut app.bimserver;
    let made = s.with(&p.server_id, |c| c.create_project(&name, &schema, p.parent_poid))?;
    Ok(project_out(s, &p.server_id, &made))
});

// ---- Download and open (COL-BIM-003) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DownloadParams {
    pub server_id: String,
    pub roid: i64,
    pub path: PathBuf,
    /// The revision's schema, for the serialiser (default ifc4).
    pub schema: Option<String>,
}

host!(BimDownload, "bimserver.download_revision", "Download a revision as an IFC file",
    "Writes revision `roid` to `path` as IFC (COL-BIM-003), serialised by the server (its IFC4 \
     serialiser unless `schema` is ifc2x3tc1). Returns the bytes written.",
    [Network, FileWrite], DownloadParams, u64, |app, caller, p| {
    let schema = p.schema.unwrap_or_else(|| "ifc4".into());
    let bytes = app.bimserver.with(&p.server_id, |c| {
        let s = c.ifc_serializer(&schema)?;
        c.download(p.roid, s.oid)
    })?;
    std::fs::write(&p.path, &bytes).map_err(|e| bad("file.write_failed", format!("{}: {e}", p.path.display())))?;
    Ok(bytes.len() as u64)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub server_id: String,
    pub poid: i64,
    /// A revision other than the latest, as a new working copy based on it (only when there is
    /// none yet).
    pub roid: Option<i64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenOut {
    pub document_id: DocumentId,
    pub working_copy: PathBuf,
    pub base_roid: i64,
    pub base_revision: i64,
    /// The existing working copy was opened (its local work kept), not a new download.
    pub existing: bool,
    /// The server has a revision newer than the working copy's base.
    pub newer_on_server: bool,
}

host!(BimOpen, "bimserver.open", "Open a BIMserver project",
    "Opens a server project as a working copy (COL-BIM-003, COL-OFF-001). If this computer has a working \
     copy of it, that is opened with its local work (never overwritten) and `newer_on_server` says whether \
     the server has moved on; otherwise the latest revision (or `roid`) is downloaded into \
     <data>/reviera/bimserver/<server>/<poid>/ as base.ifc and working.ifc and opened.",
    [Network, FileRead, FileWrite], OpenParams, OpenOut, |app, caller, p| {
    let folder = app.bimserver.folder(&p.server_id, p.poid)?;
    let project = app.bimserver.with(&p.server_id, |c| c.project(p.poid))?;
    let working = folder.join(WORKING);
    if let Some(st) = state_of(&working) {
        if p.roid.is_some_and(|r| r != st.base_roid) {
            return Err(bad("bimserver.working_copy_exists", "this project already has a working copy here; open it, or discard it before opening another revision"));
        }
        let id = match app.document_at(&working) {
            Some(id) => id,
            None => app.open(&working, false)?,
        };
        let newer = project.last_revision.is_some_and(|r| r != st.base_roid);
        return Ok(OpenOut { document_id: id, working_copy: working, base_roid: st.base_roid, base_revision: st.base_revision, existing: true, newer_on_server: newer });
    }
    let roid = p.roid.or(project.last_revision).ok_or_else(|| bad("bimserver.empty_project", format!("{} has no revisions yet; publish a model to it (bimserver.publish_local)", project.name)))?;
    let schema = project.schema.clone();
    let (bytes, revs, user) = app.bimserver.with(&p.server_id, |c| {
        let s = c.ifc_serializer(&schema)?;
        Ok((c.download(roid, s.oid)?, c.revisions(p.poid)?, c.logged_in_user()?))
    })?;
    let number = revs.iter().find(|r| r.oid == roid).map_or(0, |r| r.number);
    std::fs::create_dir_all(&folder).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    std::fs::write(folder.join(BASE), &bytes).and_then(|()| std::fs::write(&working, &bytes)).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    let profile = app.bimserver.profile(&p.server_id)?;
    write_state(&folder, &WorkingState { server_id: p.server_id.clone(), url: profile.url, poid: p.poid, project: project.name.clone(), schema, base_roid: roid, base_revision: number, user: user.username, synced_at: now() })?;
    let id = app.open(&working, false)?;
    Ok(OpenOut { document_id: id, working_copy: working, base_roid: roid, base_revision: number, existing: false, newer_on_server: project.last_revision.is_some_and(|r| r != roid) })
});

// ---- Check in and publish (COL-BIM-004, COL-MODE-002) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckInOut {
    pub poid: i64,
    pub roid: i64,
    pub revision: i64,
    pub warnings: Vec<String>,
    pub document_id: DocumentId,
}

/// Uploads the saved working copy at `working` as a new revision and makes it the base.
fn check_in(app: &mut App, id: DocumentId, folder: &Path, mut st: WorkingState, comment: &str) -> Result<CheckInOut> {
    let comment = comment_with_trailer(comment)?;
    let data = std::fs::read(folder.join(WORKING)).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    let poid = st.poid;
    let (r, revs) = app.bimserver.with(&st.server_id, |c| {
        let d = c.deserializer_for(poid)?;
        let r = c.check_in(poid, &comment, WORKING, &data, d.oid)?;
        Ok((r, c.revisions(poid)?))
    })?;
    std::fs::copy(folder.join(WORKING), folder.join(BASE)).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    st.base_roid = r.roid;
    st.base_revision = revs.iter().find(|x| x.oid == r.roid).map_or(st.base_revision + 1, |x| x.number);
    st.synced_at = now();
    write_state(folder, &st)?;
    Ok(CheckInOut { poid, roid: r.roid, revision: st.base_revision, warnings: r.warnings, document_id: id })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckInParams {
    pub comment: String,
    pub document_id: Option<DocumentId>,
}

host!(BimCheckIn, "bimserver.check_in", "Check the working copy in",
    "Saves the working copy and uploads it as a new revision of its project (COL-BIM-004), which becomes \
     its base. The comment is required. Refused while the server has a revision newer than the base: \
     another user's work would be overwritten (get latest and merge come with synchronise). In Local mode \
     it fails with mode.not_available.",
    [Network, FileWrite], CheckInParams, CheckInOut, |app, caller, p| {
    let (id, path, st) = app.working_copy(p.document_id)?;
    comment_with_trailer(&p.comment)?;
    let latest = app.bimserver.with(&st.server_id, |c| c.project(st.poid))?.last_revision;
    if latest.is_some_and(|r| r != st.base_roid) {
        return Err(bad("bimserver.out_of_date", "the server has a newer revision than this working copy; it must be merged first (synchronise, not yet available) or the working copy opened again"));
    }
    app.save(id, None)?;
    let folder = path.parent().expect("a working copy is in its folder").to_owned();
    check_in(app, id, &folder, st, &p.comment)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishParams {
    pub server_id: String,
    /// An existing project, or `new_name` for a new one (under `parent_poid`).
    pub poid: Option<i64>,
    pub new_name: Option<String>,
    pub parent_poid: Option<i64>,
    pub comment: String,
    /// Check in over a project that has revisions (a new revision; its history is kept).
    #[serde(default)]
    pub replace: bool,
    pub document_id: Option<DocumentId>,
}

host!(BimPublish, "bimserver.publish_local", "Publish the project to a BIMserver",
    "Publish to BIMserver (COL-MODE-002): checks the current Local model in to project `poid` (or a new \
     project `new_name`) as its first revision, and makes the open document that project's working copy \
     (saved to <data>/reviera/bimserver/<server>/<poid>/working.ifc). A project that has revisions is \
     refused unless `replace`. The project's schema must be the model's. Returns the project, revision \
     and document.",
    [Network, FileWrite], PublishParams, CheckInOut, |app, caller, p| {
    let id = app.resolve(p.document_id)?;
    if state_of(app.get(id)?.project.path()).is_some() {
        return Err(bad("bimserver.already_published", "the project is already a BIMserver working copy; check it in instead"));
    }
    comment_with_trailer(&p.comment)?;
    let model_schema = app.get(id)?.project.ifc().schema().to_uppercase();
    let want = match model_schema.as_str() {
        "IFC2X3" => "ifc2x3tc1",
        "IFC4" => "ifc4",
        other => return Err(bad("bimserver.schema", format!("BIMserver takes IFC4 or IFC2X3 models; this one is {other}"))),
    };
    let project = match (p.poid, p.new_name.as_deref().map(str::trim).filter(|n| !n.is_empty())) {
        (Some(poid), None) => app.bimserver.with(&p.server_id, |c| c.project(poid))?,
        (None, Some(name)) => app.bimserver.with(&p.server_id, |c| c.create_project(name, want, p.parent_poid))?,
        _ => return Err(bad("bimserver.invalid_input", "give the project's poid or a new_name, not both")),
    };
    if !project.schema.eq_ignore_ascii_case(want) {
        return Err(bad("bimserver.schema", format!("{} is an {} project and the model is {want}; choose or create an {want} project", project.name, project.schema)));
    }
    if project.last_revision.is_some() && !p.replace {
        return Err(bad("bimserver.has_revisions", format!("{} already has revisions; publish with replace to check in a new one over them", project.name)));
    }
    let folder = app.bimserver.folder(&p.server_id, project.oid)?;
    if folder.join(STATE).is_file() {
        return Err(bad("bimserver.working_copy_exists", format!("there is already a working copy of {} here; discard it first", project.name)));
    }
    std::fs::create_dir_all(&folder).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    let user = app.bimserver.with(&p.server_id, Client::logged_in_user)?;
    let profile = app.bimserver.profile(&p.server_id)?;
    let st = WorkingState { server_id: p.server_id.clone(), url: profile.url, poid: project.oid, project: project.name.clone(), schema: project.schema.clone(), base_roid: project.last_revision.unwrap_or(-1), base_revision: project.revisions.len() as i64, user: user.username, synced_at: now() };
    app.save(id, Some(&folder.join(WORKING)))?;
    write_state(&folder, &st)?;
    match check_in(app, id, &folder, st, &p.comment) {
        Ok(out) => Ok(out),
        Err(e) => {
            // Nothing reached the server: the folder is not a working copy.
            let _ = std::fs::remove_file(folder.join(STATE));
            Err(e)
        }
    }
});

// ---- Mode (COL-MODE-001) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeParams {
    pub document_id: Option<DocumentId>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModeOut {
    /// `local` or `bimserver`.
    pub mode: String,
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_copy: Option<WorkingState>,
    /// A session to its server is open.
    pub connected: bool,
}

host!(ModelMode, "model.mode", "Tell the project's mode",
    "Whether the project is Local (an IFC file) or a BIMserver working copy (COL-MODE-001), with the \
     server, project, base revision and whether a session is open.",
    [Read], Option<ModeParams>, ModeOut, |app, caller, p| {
    let id = app.resolve(p.and_then(|p| p.document_id))?;
    let path = app.get(id)?.project.path().to_owned();
    let st = state_of(&path);
    let connected = st.as_ref().is_some_and(|s| app.bimserver.clients.get(&s.server_id).is_some_and(Client::logged_in));
    Ok(ModeOut { mode: if st.is_some() { "bimserver" } else { "local" }.into(), path, working_copy: st, connected })
});

// ---- Get latest, synchronise and merge (COL-BIM-005/006, COL-MRG-001) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncReport {
    /// The base was already the server's latest revision.
    pub up_to_date: bool,
    pub base_roid: i64,
    pub base_revision: i64,
    /// The merge: elements created, updated and deleted in the working copy, the remote
    /// changes taken, the elements merged facet by facet, and the conflicts.
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub remote_changes: usize,
    pub merged: usize,
    pub conflicts: Vec<reviera_core::merge::Conflict>,
    /// The merge was applied (false while conflicts wait for a decision).
    pub applied: bool,
    /// Elements deleted here this session that the merge could not bring back.
    pub skipped: Vec<String>,
    /// Synchronise: the revision checked in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_in: Option<CheckInOut>,
}

fn open_store(path: &Path) -> Result<reviera_core::Store> {
    let (_, doc, _) = reviera_store::LocalProject::open(path).map_err(|e| bad("bimserver.working_copy", format!("{}: {e}", path.display())))?;
    Ok(doc.store().clone())
}

/// Merges the server's latest revision into working copy `id` (saved first).
fn get_latest(app: &mut App, id: DocumentId, path: &Path, mut st: WorkingState, policy: reviera_core::merge::Policy) -> Result<SyncReport> {
    let folder = path.parent().expect("a working copy is in its folder").to_owned();
    let poid = st.poid;
    let project = app.bimserver.with(&st.server_id, |c| c.project(poid))?;
    let mut report = SyncReport { up_to_date: true, base_roid: st.base_roid, base_revision: st.base_revision, created: 0, updated: 0, deleted: 0, remote_changes: 0, merged: 0, conflicts: Vec::new(), applied: false, skipped: Vec::new(), checked_in: None };
    let Some(latest) = project.last_revision.filter(|r| *r != st.base_roid) else { return Ok(report) };
    report.up_to_date = false;
    let schema = st.schema.clone();
    let (bytes, revs) = app.bimserver.with(&st.server_id, |c| {
        let s = c.ifc_serializer(&schema)?;
        Ok((c.download(latest, s.oid)?, c.revisions(poid)?))
    })?;
    let remote_path = folder.join("remote.ifc");
    std::fs::write(&remote_path, &bytes).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    let base = open_store(&folder.join(BASE))?;
    let remote = open_store(&remote_path)?;
    let plan = reviera_core::merge::three_way(&base, app.get(id)?.doc.store(), &remote, policy);
    report.remote_changes = plan.remote_changes;
    report.merged = plan.merged;
    report.conflicts = plan.conflicts.clone();
    if !plan.conflicts.is_empty() && policy == reviera_core::merge::Policy::Fail {
        return Ok(report);
    }
    let number = revs.iter().find(|r| r.oid == latest).map_or(st.base_revision, |r| r.number);
    let doc = &mut app.get_mut(id)?.doc;
    let mut txn = doc.transaction(format!("Get Latest (revision {number})"), reviera_core::Origin::ui());
    let applied = reviera_core::merge::apply(&mut txn, &plan)?;
    txn.commit();
    (report.created, report.updated, report.deleted, report.skipped) = (applied.created, applied.updated, applied.deleted, applied.skipped);
    report.applied = true;
    app.save(id, None)?;
    std::fs::rename(&remote_path, folder.join(BASE)).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    st.base_roid = latest;
    st.base_revision = number;
    st.synced_at = now();
    write_state(&folder, &st)?;
    report.base_roid = latest;
    report.base_revision = number;
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetLatestParams {
    pub document_id: Option<DocumentId>,
    /// Conflicts: `fail` (report them and change nothing, the default), `local` or `remote`
    /// (that side wins).
    #[serde(default)]
    pub conflicts: reviera_core::merge::Policy,
}

host!(BimGetLatest, "bimserver.get_latest", "Get the latest revision",
    "Get Latest (COL-BIM-005): merges the project's newest revision into the open working copy, three-way \
     against its base, element by element and parameter by parameter (recipes key by key, member lists as \
     sets), as one undoable step; local changes stay local (nothing is uploaded). The revision becomes the \
     base and the working copy is saved. Conflicts (both sides changed the same thing differently, or one \
     deleted what the other changed) are reported and nothing changes unless `conflicts` is local or \
     remote. Returns the sync report.",
    [Network, Edit, FileWrite], GetLatestParams, SyncReport, |app, caller, p| {
    let (id, path, st) = app.working_copy(p.document_id)?;
    app.save(id, None)?;
    get_latest(app, id, &path, st, p.conflicts)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncParams {
    pub comment: String,
    pub document_id: Option<DocumentId>,
    #[serde(default)]
    pub conflicts: reviera_core::merge::Policy,
}

host!(BimSync, "bimserver.sync", "Synchronise with the server",
    "Synchronise (COL-BIM-006): saves the working copy, gets the latest revision and merges it (as \
     bimserver.get_latest; stopping at conflicts unless `conflicts` says which side wins), then checks the \
     merged working copy in with the comment. Returns the sync report with the revision checked in.",
    [Network, Edit, FileWrite], SyncParams, SyncReport, |app, caller, p| {
    comment_with_trailer(&p.comment)?;
    let (id, path, st) = app.working_copy(p.document_id)?;
    app.save(id, None)?;
    let mut report = get_latest(app, id, &path, st, p.conflicts)?;
    if !report.up_to_date && !report.applied {
        return Ok(report);
    }
    let st = state_of(&path).expect("still a working copy");
    let folder = path.parent().expect("in its folder").to_owned();
    report.checked_in = Some(check_in(app, id, &folder, st, &p.comment)?);
    let c = report.checked_in.as_ref().expect("set");
    (report.base_roid, report.base_revision) = (c.roid, c.revision);
    Ok(report)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThreeWayParams {
    pub base_path: PathBuf,
    pub local_path: PathBuf,
    pub remote_path: PathBuf,
    pub out_path: PathBuf,
    #[serde(default)]
    pub policy: reviera_core::merge::Policy,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MergeReport {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub remote_changes: usize,
    pub merged: usize,
    pub conflicts: Vec<reviera_core::merge::Conflict>,
    /// The merged model was written (not while conflicts wait under the `fail` policy).
    pub written: bool,
}

host!(MergeThreeWay, "merge.three_way", "Merge three IFC files",
    "Three-way merge of files (COL-MRG-001), for scripts and tests: the changes from `base_path` to \
     `remote_path` merged into `local_path`, element by element and parameter by parameter, written to \
     `out_path`. Conflicts: `policy` fail (report them; nothing written), local or remote.",
    [FileRead, FileWrite], ThreeWayParams, MergeReport, |app, caller, p| {
    let base = open_store(&p.base_path)?;
    let remote = open_store(&p.remote_path)?;
    let (mut project, mut doc, _) = reviera_store::LocalProject::open(&p.local_path).map_err(|e| bad("merge.read", format!("{}: {e}", p.local_path.display())))?;
    let plan = reviera_core::merge::three_way(&base, doc.store(), &remote, p.policy);
    let mut out = MergeReport { created: 0, updated: 0, deleted: 0, remote_changes: plan.remote_changes, merged: plan.merged, conflicts: plan.conflicts.clone(), written: false };
    if !plan.conflicts.is_empty() && p.policy == reviera_core::merge::Policy::Fail {
        return Ok(out);
    }
    let mut txn = doc.transaction("Merge", reviera_core::Origin::ui());
    let a = reviera_core::merge::apply(&mut txn, &plan)?;
    txn.commit();
    (out.created, out.updated, out.deleted) = (a.created, a.updated, a.deleted);
    project.save_as(&doc, &p.out_path).map_err(|e| bad("merge.write", format!("{}: {e}", p.out_path.display())))?;
    out.written = true;
    Ok(out)
});

// ---- History, compare and restore (COL-BIM-008) ----

/// A revision's label from its comment: `restore` or `ownership` for Reviera's own kinds.
fn label(comment: &str) -> Option<String> {
    let c = comment.trim_start();
    c.strip_prefix("[reviera:").and_then(|r| r.split_once(']')).map(|(k, _)| k.to_owned())
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryRow {
    #[serde(flatten)]
    pub revision: RevisionOut,
    /// `restore` for a restored revision (comment `[reviera:restore] …`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The working copy's base.
    pub base: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryParams {
    pub document_id: Option<DocumentId>,
}

host!(BimHistory, "bimserver.history", "List the working copy's revisions",
    "The revisions of the open working copy's project, newest first (COL-BIM-008): number, date, user, \
     comment, size, Reviera's label (restore), and which one is the working copy's base.",
    [Read, Network], Option<HistoryParams>, Vec<HistoryRow>, |app, caller, p| {
    let (_, _, st) = app.working_copy(p.and_then(|p| p.document_id))?;
    let revs = BimListRevisions.run(app, caller, ProjectParams { server_id: st.server_id.clone(), poid: st.poid })?;
    Ok(revs.into_iter().rev().map(|r| HistoryRow { label: label(&r.comment), base: r.roid == st.base_roid, revision: r }).collect())
});

/// Revision `roid` of the working copy's project, downloaded to `path` and read.
fn revision_store(app: &mut App, st: &WorkingState, roid: i64, path: &Path) -> Result<reviera_core::Store> {
    let schema = st.schema.clone();
    let bytes = app.bimserver.with(&st.server_id, |c| {
        let s = c.ifc_serializer(&schema)?;
        c.download(roid, s.oid)
    })?;
    std::fs::write(path, &bytes).map_err(|e| bad("bimserver.working_copy", e.to_string()))?;
    let store = open_store(path);
    let _ = std::fs::remove_file(path);
    store
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareParams {
    /// The older revision; the working copy's base when absent.
    pub roid_a: Option<i64>,
    /// The newer revision; the working copy itself (its local changes) when absent.
    pub roid_b: Option<i64>,
    pub document_id: Option<DocumentId>,
}

host!(BimCompare, "bimserver.compare", "Compare revisions",
    "The changes between two revisions of the working copy's project (COL-BIM-008): elements added, \
     deleted and modified (with the class, name or parameters that changed), by GlobalId. `roid_a` defaults \
     to the working copy's base and `roid_b` to the working copy itself, so with neither it lists the local \
     changes not yet checked in.",
    [Read, Network], CompareParams, reviera_core::merge::ChangeSet, |app, caller, p| {
    let (id, path, st) = app.working_copy(p.document_id)?;
    let folder = path.parent().expect("in its folder").to_owned();
    let a = match p.roid_a.filter(|r| *r != st.base_roid) {
        Some(r) => revision_store(app, &st, r, &folder.join("compare-a.ifc"))?,
        None => open_store(&folder.join(BASE))?,
    };
    let b = match p.roid_b {
        Some(r) => revision_store(app, &st, r, &folder.join("compare-b.ifc"))?,
        None => app.get(id)?.doc.store().clone(),
    };
    Ok(reviera_core::merge::diff(&a, &b))
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    pub roid: i64,
    pub reason: String,
    pub document_id: Option<DocumentId>,
}

host!(BimRestore, "bimserver.restore", "Restore a revision as the latest",
    "Restore as latest (COL-BIM-008): checks revision `roid`'s content in as a new revision, with the \
     comment `[reviera:restore] Restored revision n: <reason>`; the history is kept. The open working copy \
     must be up to date and have no local changes (synchronise or discard them first); it then becomes the \
     restored model, as one undoable step. Returns the new revision.",
    [Network, Edit, FileWrite], RestoreParams, CheckInOut, |app, caller, p| {
    let reason = p.reason.trim().to_owned();
    if reason.is_empty() {
        return Err(bad("bimserver.no_comment", "say why the revision is restored"));
    }
    let (id, path, st) = app.working_copy(p.document_id)?;
    let folder = path.parent().expect("in its folder").to_owned();
    let poid = st.poid;
    let (project, revs) = app.bimserver.with(&st.server_id, |c| Ok((c.project(poid)?, c.revisions(poid)?)))?;
    if project.last_revision.is_some_and(|r| r != st.base_roid) {
        return Err(bad("bimserver.out_of_date", "get the latest revision first: the restore is checked in over it"));
    }
    let base = open_store(&folder.join(BASE))?;
    if !reviera_core::merge::diff(&base, app.get(id)?.doc.store()).is_empty() {
        return Err(bad("bimserver.local_changes", "the working copy has changes not checked in; synchronise them or undo them first"));
    }
    let number = revs.iter().find(|r| r.oid == p.roid).map(|r| r.number).ok_or_else(|| bad("bimserver.no_revision", format!("revision {} is not one of {}'s", p.roid, st.project)))?;
    let restored = revision_store(app, &st, p.roid, &folder.join("restore.ifc"))?;
    // The working copy becomes the restored model (base == local, so the merge takes it all).
    let plan = reviera_core::merge::three_way(&base, app.get(id)?.doc.store(), &restored, reviera_core::merge::Policy::Remote);
    let doc = &mut app.get_mut(id)?.doc;
    let mut txn = doc.transaction(format!("Restore Revision {number}"), reviera_core::Origin::ui());
    reviera_core::merge::apply(&mut txn, &plan)?;
    txn.commit();
    app.save(id, None)?;
    check_in(app, id, &folder, st, &format!("[reviera:restore] Restored revision {number}: {reason}"))
});
