// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `fileengine.*`: models kept on a FileEngine (Proxima File Vault) server, phase 1 of
//! `design_documents/FILEENGINE_INTEGRATION.md` (spec Ch 6 §6.12, COL-FE-001…005), over the MIT
//! client `reviera_fileengine`.
//!
//! - **Connections** (name, the tenant's address, a tenant to work in, the user last signed in)
//!   are per-user settings (`fileengine.json` in the settings folder), never project data. The
//!   service credential (`fesk_…` key and its secret) lives in the OS keychain when *remember* is
//!   on (or for this session where there is none); no command returns it, and agents cannot give
//!   one. The bearer it is exchanged for renews itself.
//! - **Working copies.** An opened model is streamed into
//!   `<data>/reviera/fileengine/<connection>/<uid>/<name>` beside `fileengine.json` (connection,
//!   uid, folder, the version opened or last saved). A document is a FileEngine model when its
//!   file is such a working copy, so the link survives a restart. A past version opens read-only
//!   from `<uid>@<version>/`.
//! - **Saving** (`file.save`, Ctrl+S) writes the working copy, then uploads it as a new version,
//!   but only if the server's newest version is still the one opened: otherwise someone else
//!   saved, and the save is kept on this computer while the user chooses (`fileengine.save` with
//!   `on_conflict`: overwrite, their version staying in the history, or a copy beside it). An
//!   upload that fails (offline) is kept and sent by the next save. Each save sets the model's
//!   metadata (`ifc.schema`, `ifc.project_guid`, `reviera.version`, `reviera.levels`, and the
//!   comment as `reviera.comment` and `reviera.comment.<version>`).
//! - `file.open` takes `fileengine://<connection>/<uid>` (`?version=<v>` for a past one) and
//!   `file.save_as` takes `fileengine://<connection>/<folder uid>/<name>.ifc`.
//!
//! Not yet: thumbnails, publishing, BCF, Component Libraries, documentation folders and product
//! data (the later phases), single sign-on, and the transfers off the UI thread.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result, Source};
use reviera_fileengine::{Client, ErrorKind, Node};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

/// The keychain service the credentials are stored under (the account is the connection's id).
pub const KEYCHAIN_SERVICE: &str = "org.bimreviera.fileengine";
/// Commands not offered to agents: they remove stored credentials or delete on the server.
pub const NOT_FOR_AGENTS: &[&str] = &["fileengine.remove"];
/// The working copy's link file.
pub const STATE: &str = "fileengine.json";
/// The URL scheme `file.open` and `file.save_as` take.
pub const SCHEME: &str = "fileengine://";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn failed(e: reviera_fileengine::Error) -> Error {
    let code = match e.kind {
        ErrorKind::Url => "fileengine.bad_url",
        ErrorKind::Transport => "fileengine.unreachable",
        ErrorKind::Auth => "fileengine.not_authorised",
        ErrorKind::Forbidden => "fileengine.forbidden",
        ErrorKind::NotFound => "fileengine.not_found",
        ErrorKind::Conflict => "fileengine.conflict",
        ErrorKind::TooLarge => "fileengine.too_large",
        ErrorKind::Server => "fileengine.refused",
        ErrorKind::Protocol => "fileengine.protocol",
    };
    Error::new(code, e.message.clone()).with_details(serde_json::json!({ "status": e.status }))
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    connections: Vec<Profile>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Profile {
    id: String,
    name: String,
    url: String,
    /// The tenant to work in (`X-Tenant`); the credential's own when `None`.
    tenant: Option<String>,
    /// Plain http to a host other than this computer (a trusted local network).
    allow_http: bool,
    /// Who signed in last, and in which tenant.
    user: Option<String>,
    signed_in_tenant: Option<String>,
}

pub use crate::bimserver::Storage;

/// The connections' working state on the app.
#[derive(Default)]
pub struct Connections {
    settings: Option<PathBuf>,
    data: Option<PathBuf>,
    session_only: bool,
    session: HashMap<String, String>,
    clients: HashMap<String, Client>,
}

impl std::fmt::Debug for Connections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connections").field("settings", &self.settings).field("data", &self.data).finish_non_exhaustive()
    }
}

impl Connections {
    /// Keeps the settings in `settings`, the working copies under `data` and the credentials in
    /// memory: for tests and tools.
    pub fn isolated(settings: PathBuf, data: PathBuf) -> Self {
        Self { settings: Some(settings), data: Some(data), session_only: true, ..Self::default() }
    }

    fn file(&self) -> Result<PathBuf> {
        self.settings.clone().or_else(|| crate::subscriptions::config_dir().map(|d| d.join("fileengine.json"))).ok_or_else(|| bad("fileengine.no_settings", "there is no per-user settings folder"))
    }

    fn root(&self) -> Result<PathBuf> {
        self.data.clone().or_else(|| crate::bimserver::data_dir().map(|d| d.join("fileengine"))).ok_or_else(|| bad("fileengine.no_settings", "there is no per-user data folder"))
    }

    fn load(&self) -> Result<Settings> {
        match std::fs::read(self.file()?) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| bad("fileengine.settings_invalid", format!("fileengine.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(bad("fileengine.settings_unreadable", e.to_string())),
        }
    }

    fn save(&self, s: &Settings) -> Result<()> {
        let path = self.file()?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| bad("fileengine.settings_unwritable", e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s).expect("settings serialise")).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| bad("fileengine.settings_unwritable", e.to_string()))
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
        self.load()?.connections.into_iter().find(|p| p.id == id).ok_or_else(|| bad("fileengine.no_connection", format!("no FileEngine connection {id:?}; fileengine.list_connections lists them")))
    }

    fn client_for(&mut self, p: &Profile) -> Result<&mut Client> {
        if !self.clients.contains_key(&p.id) {
            let mut c = Client::new(&p.url, p.allow_http).map_err(failed)?;
            c.set_tenant(p.tenant.clone());
            self.clients.insert(p.id.clone(), c);
        }
        Ok(self.clients.get_mut(&p.id).expect("inserted"))
    }

    /// Runs `f` on a signed-in client for connection `id`, signing in with the stored credential
    /// first if there is no session.
    fn with<T>(&mut self, id: &str, f: impl FnOnce(&Client) -> reviera_fileengine::Result<T>) -> Result<T> {
        let _p = reviera_core::profiler::scope("FileEngine call");
        let p = self.profile(id)?;
        let stored = self.secret(&p.id);
        let c = self.client_for(&p)?;
        if !c.signed_in() {
            let (key, secret) = stored.as_deref().and_then(|s| s.split_once('\n')).ok_or_else(|| bad("fileengine.not_signed_in", format!("sign in to {} first (fileengine.sign_in)", p.name)))?;
            c.sign_in(key, secret).map_err(failed)?;
        }
        f(c).map_err(failed)
    }

    /// The working-copy folder of file `uid` (at `version`, for a past one).
    fn folder(&self, id: &str, uid: &str, version: Option<&str>) -> Result<PathBuf> {
        let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect::<String>();
        let leaf = match version {
            Some(v) => format!("{}@{}", safe(uid), safe(v)),
            None => safe(uid),
        };
        Ok(self.root()?.join(safe(id)).join(leaf))
    }
}

/// A working copy's link to its file (`fileengine.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    pub connection_id: String,
    pub url: String,
    pub uid: String,
    /// The file's name on the server, and its folder.
    pub name: String,
    pub folder_uid: String,
    /// The version opened or last uploaded (empty for a file with none yet).
    pub version: String,
    /// Who opened it.
    pub user: String,
    /// The user may not write the file: it opened read-only.
    pub read_only: bool,
    /// A past version, opened to look at.
    #[serde(default)]
    pub historic: bool,
    /// Saved on this computer but not uploaded yet (offline, or someone else saved first).
    #[serde(default)]
    pub unsent: bool,
    /// Unix seconds of the last download or upload.
    pub synced_at: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The FileEngine link of a document's file, if it is a FileEngine working copy.
pub fn link_of(path: &Path) -> Option<Link> {
    let l: Link = serde_json::from_slice(&std::fs::read(path.with_file_name(STATE)).ok()?).ok()?;
    (path.file_name()?.to_str()? == l.name).then_some(l)
}

fn write_link(folder: &Path, l: &Link) -> Result<()> {
    std::fs::write(folder.join(STATE), serde_json::to_vec_pretty(l).expect("serialises")).map_err(|e| bad("fileengine.working_copy", e.to_string()))
}

fn not_for_agents(caller: &Caller, what: &str) -> Result<()> {
    if caller.origin.source == Source::Mcp {
        return Err(Error::new("permission.denied", format!("{what} is for the user (the FileEngine sign-in dialog) and Lua, never for agents")));
    }
    Ok(())
}

/// A `fileengine://` address: the connection, the uid (or folder uid and name), the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub connection_id: String,
    pub uid: String,
    pub name: Option<String>,
    pub version: Option<String>,
}

/// Reads `fileengine://<connection>/<uid>[?version=<v>]` or
/// `fileengine://<connection>/<folder uid>/<name>`; `None` for any other path.
pub fn parse_address(path: &Path) -> Option<Result<Address>> {
    let text = path.to_str()?;
    let rest = text.strip_prefix(SCHEME)?;
    let (rest, query) = rest.split_once('?').map_or((rest, None), |(r, q)| (r, Some(q)));
    let version = query.and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("version=")).map(str::to_owned));
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    Some(match parts.as_slice() {
        [c, uid] => Ok(Address { connection_id: (*c).into(), uid: (*uid).into(), name: None, version }),
        [c, folder, name] => Ok(Address { connection_id: (*c).into(), uid: (*folder).into(), name: Some((*name).into()), version }),
        _ => Err(bad("fileengine.bad_address", format!("{text}: a FileEngine address is fileengine://<connection>/<uid>, or fileengine://<connection>/<folder uid>/<name> to save"))),
    })
}

fn is_model(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".ifc") || n.ends_with(".ifczip")
}

impl App {
    /// The FileEngine connections' state (settings file, credentials, sessions).
    pub fn fileengine_mut(&mut self) -> &mut Connections {
        &mut self.fileengine
    }

    /// The document to act on and its link.
    fn linked(&self, id: Option<DocumentId>) -> Result<(DocumentId, PathBuf, Link)> {
        let id = self.resolve(id)?;
        let path = self.get(id)?.project.path().to_owned();
        let l = link_of(&path).ok_or_else(|| bad("fileengine.not_linked", "the project is not a FileEngine model (File › Open From FileEngine, or Save to FileEngine)"))?;
        Ok((id, path, l))
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

// ---- Connections (COL-FE-001) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConnectionOut {
    pub id: String,
    pub name: String,
    /// The REST door's address.
    pub url: String,
    pub tenant: Option<String>,
    /// Who signed in last, and in which tenant.
    pub user: Option<String>,
    pub signed_in_tenant: Option<String>,
    /// Where the credential is kept (never the credential).
    pub secret: Storage,
    /// Whether a session is open.
    pub connected: bool,
}

fn connection_out(s: &Connections, p: &Profile) -> ConnectionOut {
    ConnectionOut {
        id: p.id.clone(),
        name: p.name.clone(),
        url: p.url.clone(),
        tenant: p.tenant.clone(),
        user: p.user.clone(),
        signed_in_tenant: p.signed_in_tenant.clone(),
        secret: s.storage(&p.id),
        connected: s.clients.get(&p.id).is_some_and(Client::signed_in),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    /// The tenant's address, e.g. https://acme.proximafilevault.com (its REST door is under /api/).
    pub url: String,
    /// A name to show (default: the server's host name).
    pub name: Option<String>,
    /// The tenant to work in, when the credential reaches more than one.
    pub tenant: Option<String>,
    /// Accept plain http:// to another computer (a trusted local network only).
    #[serde(default)]
    pub allow_http: bool,
}

host!(FeConnect, "fileengine.connect", "Add a FileEngine connection",
    "Adds a connection to a FileEngine (Proxima File Vault) server (COL-FE-001): its address (https://; \
     http:// only for localhost unless `allow_http`), a name and the tenant to work in. Connections are \
     per-user settings, not project data. Sign in next (fileengine.sign_in). Returns the connection.",
    [Settings], ConnectParams, ConnectionOut, |app, caller, p| {
    let c = Client::new(&p.url, p.allow_http).map_err(failed)?;
    let url = c.url().to_owned();
    let s = &mut app.fileengine;
    let mut settings = s.load()?;
    if settings.connections.iter().any(|x| x.url == url && x.tenant == p.tenant) {
        return Err(bad("fileengine.duplicate", format!("{url} is already a connection")));
    }
    let name = p.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| reviera_fileengine::check_url(&url, true).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| url.clone()));
    // Unique across settings files, so a new connection never finds an old one's credential.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut n = 0u32;
    let id = loop {
        let id = format!("fe-{:x}", (nanos as u64) ^ (u64::from(std::process::id()) << 32) ^ u64::from(n));
        if settings.connections.iter().all(|x| x.id != id) {
            break id;
        }
        n += 1;
    };
    s.delete_secret(&id);
    let profile = Profile { id: id.clone(), name, url, tenant: p.tenant.filter(|t| !t.trim().is_empty()), allow_http: p.allow_http, user: None, signed_in_tenant: None };
    settings.connections.push(profile.clone());
    s.save(&settings)?;
    Ok(connection_out(s, &profile))
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectionIdParams {
    pub connection_id: String,
}

host!(FeList, "fileengine.list_connections", "List FileEngine connections",
    "The connections: id, name, address, tenant, who signed in last, where the credential is kept \
     (keychain, session or none; never the credential) and whether a session is open.",
    [Read], Option<serde_json::Value>, Vec<ConnectionOut>, |app, caller, p| {
    let s = &app.fileengine;
    Ok(s.load()?.connections.iter().map(|x| connection_out(s, x)).collect())
});

host!(FeRemove, "fileengine.remove", "Remove a FileEngine connection",
    "Removes a connection and its stored credential (COL-FE-001). Working copies stay on disk. Not \
     available to MCP.",
    [Settings, Destructive], ConnectionIdParams, (), |app, caller, p| {
    not_for_agents(caller, "removing a connection's stored credential")?;
    let s = &mut app.fileengine;
    let mut settings = s.load()?;
    let before = settings.connections.len();
    settings.connections.retain(|x| x.id != p.connection_id);
    if settings.connections.len() == before {
        return Err(bad("fileengine.no_connection", format!("no FileEngine connection {:?}", p.connection_id)));
    }
    s.save(&settings)?;
    s.delete_secret(&p.connection_id);
    if let Some(c) = s.clients.remove(&p.connection_id) {
        let _ = c.sign_out();
    }
    Ok(())
});

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignInParams {
    pub connection_id: String,
    /// The service credential's key id (`fesk_…`) and its secret (`fesks_…`), as FileEngine
    /// generated them; or `key` as `fesk_…:fesks_…` alone. With neither, the stored one.
    pub key: Option<String>,
    pub secret: Option<String>,
    /// Keep the credential in the OS keychain (default true; this session only where there is none).
    #[serde(default = "yes")]
    pub remember: bool,
}

impl std::fmt::Debug for SignInParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInParams").field("connection_id", &self.connection_id).field("remember", &self.remember).finish_non_exhaustive()
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Identity {
    pub connection_id: String,
    pub user: String,
    pub tenant: String,
    pub roles: Vec<String>,
    pub secret: Storage,
}

host!(FeSignIn, "fileengine.sign_in", "Sign in to FileEngine",
    "Signs in (COL-FE-001) with a service credential generated on FileEngine for this app (a `fesk_…` key \
     id and its `fesks_…` secret); with none, with the stored one. `remember` (default true) keeps it in \
     the OS keychain. The short-lived token it is exchanged for renews itself. An agent (MCP) may only \
     sign in with the stored credential. Returns who is signed in.",
    [Network, Settings], SignInParams, Identity, |app, caller, p| {
    let fresh = match (p.key.as_deref().map(str::trim).filter(|k| !k.is_empty()), p.secret.as_deref().map(str::trim).filter(|s| !s.is_empty())) {
        (Some(k), Some(s)) => Some((k.to_owned(), s.to_owned())),
        (Some(k), None) => Some(k.split_once(':').map(|(a, b)| (a.to_owned(), b.to_owned())).ok_or_else(|| bad("fileengine.invalid_input", "give the key and its secret (or key as fesk_…:fesks_…)"))?),
        (None, Some(_)) => return Err(bad("fileengine.invalid_input", "a secret needs its key")),
        (None, None) => None,
    };
    if fresh.is_some() {
        not_for_agents(caller, "a credential")?;
    }
    let s = &mut app.fileengine;
    let mut settings = s.load()?;
    let at = settings.connections.iter().position(|x| x.id == p.connection_id).ok_or_else(|| bad("fileengine.no_connection", format!("no FileEngine connection {:?}", p.connection_id)))?;
    let mut profile = settings.connections[at].clone();
    let (key, secret) = match &fresh {
        Some(ks) => ks.clone(),
        None => s.secret(&profile.id).and_then(|x| x.split_once('\n').map(|(a, b)| (a.to_owned(), b.to_owned()))).ok_or_else(|| bad("fileengine.not_signed_in", format!("no credential is stored for {}; give the key and secret", profile.name)))?,
    };
    let who = s.client_for(&profile)?.sign_in(&key, &secret).map_err(failed)?;
    match fresh {
        Some((k, sec)) if p.remember => {
            s.put_secret(&profile.id, &format!("{k}\n{sec}"));
        }
        Some(_) => s.delete_secret(&profile.id),
        None => {}
    }
    profile.user = Some(who.user.clone());
    profile.signed_in_tenant = Some(who.tenant.clone());
    settings.connections[at] = profile.clone();
    s.save(&settings)?;
    Ok(Identity { connection_id: profile.id.clone(), user: who.user, tenant: who.tenant, roles: who.roles, secret: s.storage(&profile.id) })
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignOutParams {
    pub connection_id: String,
    /// Also forget the stored credential.
    #[serde(default)]
    pub forget: bool,
}

host!(FeSignOut, "fileengine.sign_out", "Sign out of FileEngine",
    "Ends the session (COL-FE-001); `forget` also removes the stored credential.",
    [Network, Settings], SignOutParams, (), |app, caller, p| {
    let s = &mut app.fileengine;
    s.profile(&p.connection_id)?;
    if let Some(c) = s.clients.remove(&p.connection_id) {
        let _ = c.sign_out();
    }
    if p.forget {
        s.delete_secret(&p.connection_id);
    }
    Ok(())
});

host!(FeWhoami, "fileengine.whoami", "Who is signed in to FileEngine",
    "The user, tenant and roles of the connection's session (COL-FE-001), signing in with the stored \
     credential if there is no session.",
    [Network, Read], ConnectionIdParams, Identity, |app, caller, p| {
    let who = app.fileengine.with(&p.connection_id, Client::whoami)?;
    Ok(Identity { connection_id: p.connection_id.clone(), user: who.user, tenant: who.tenant, roles: who.roles, secret: app.fileengine.storage(&p.connection_id) })
});

// ---- Browsing (COL-FE-002) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryOut {
    pub uid: String,
    pub name: String,
    pub folder: bool,
    /// An IFC model Reviera can open.
    pub model: bool,
    pub size: u64,
    /// The listing's version count. Proxima File Vault reports 1 however many versions a
    /// file has, so `fileengine.versions` is the one to trust.
    pub versions: u64,
    pub owner: String,
    pub modified_by: String,
    /// Unix seconds.
    pub modified_at: i64,
    /// Whether this computer has a working copy of it.
    pub working_copy: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowseOut {
    pub folder_uid: String,
    pub folder_name: String,
    /// The folder's parent (empty at the top).
    pub parent_uid: String,
    /// Whether the user may write in the folder.
    pub writable: bool,
    pub entries: Vec<EntryOut>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowseParams {
    pub connection_id: String,
    /// The folder (default: the top folder).
    pub folder_uid: Option<String>,
    /// Or the folder's path from the top, e.g. `Engineering/Projects`.
    pub path: Option<String>,
    /// Folders and IFC models only (default true).
    #[serde(default = "yes")]
    pub models_only: bool,
}

host!(FeBrowse, "fileengine.browse", "List a FileEngine folder",
    "A folder's contents (COL-FE-002): folders first, then files, each with its size, version count, owner \
     and who changed it last; `models_only` (default true) keeps folders and .ifc / .ifczip models. Give \
     `folder_uid` or a `path` from the top (`Engineering/Projects`); neither lists the top folder.",
    [Network, Read], BrowseParams, BrowseOut, |app, caller, p| {
    let root = app.fileengine.root().ok();
    let conn = p.connection_id.clone();
    let (folder, entries, writable) = app.fileengine.with(&p.connection_id, |c| {
        let folder = match (&p.folder_uid, &p.path) {
            (Some(uid), _) => c.stat(uid)?,
            (None, Some(path)) => c.resolve(path)?,
            (None, None) => Node { uid: reviera_fileengine::ROOT.into(), kind: reviera_fileengine::Kind::Directory, ..Node::default() },
        };
        if !folder.is_dir() {
            return Err(reviera_fileengine::Error { kind: ErrorKind::Conflict, status: None, message: format!("{} is a file, not a folder", folder.name) });
        }
        let entries = c.list(&folder.uid, false)?;
        let writable = c.can(&folder.uid, "w").unwrap_or(false);
        Ok((folder, entries, writable))
    })?;
    let mut out: Vec<EntryOut> = entries
        .into_iter()
        .filter(|e| !e.deleted && (e.is_dir() || !p.models_only || is_model(&e.name)))
        .map(|e| EntryOut {
            working_copy: !e.is_dir() && root.as_ref().is_some_and(|r| r.join(&conn).join(&e.uid).join(STATE).is_file()),
            model: !e.is_dir() && is_model(&e.name),
            folder: e.is_dir(),
            versions: e.version_count,
            uid: e.uid,
            name: e.name,
            size: e.size,
            owner: e.owner,
            modified_by: e.modified_by,
            modified_at: e.modified_at,
        })
        .collect();
    out.sort_by(|a, b| b.folder.cmp(&a.folder).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(BrowseOut { folder_name: if folder.uid == reviera_fileengine::ROOT { String::new() } else { folder.name }, parent_uid: folder.parent_uid, folder_uid: folder.uid, writable, entries: out })
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MakeFolderParams {
    pub connection_id: String,
    pub parent_uid: String,
    pub name: String,
}

host!(FeMakeFolder, "fileengine.make_folder", "Make a FileEngine folder",
    "Makes a folder in `parent_uid` (COL-FE-002), or finds the one of that name already there. Returns its uid.",
    [Network], MakeFolderParams, String, |app, caller, p| {
    let name = p.name.trim().to_owned();
    if name.is_empty() || name.contains('/') {
        return Err(bad("fileengine.invalid_input", "a folder needs a name without '/'"));
    }
    app.fileengine.with(&p.connection_id, |c| c.ensure_dir(&p.parent_uid, &name))
});

// ---- Opening (COL-FE-002) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenOut {
    pub document_id: DocumentId,
    pub working_copy: PathBuf,
    pub link: Link,
    /// The working copy was already here and opened as it was: it holds work not uploaded yet.
    pub existing: bool,
    /// The server has a newer version than the working copy's.
    pub newer_on_server: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub connection_id: String,
    /// The model's uid.
    pub uid: String,
    /// A past version, opened read-only.
    pub version: Option<String>,
    /// Open read-only even if the user may write it.
    #[serde(default)]
    pub read_only: bool,
}

/// Opens file `uid` of connection `conn`: downloads it into its working copy and opens that.
pub(crate) fn open(app: &mut App, conn: &str, uid: &str, version: Option<&str>, read_only: bool) -> Result<OpenOut> {
    let folder = app.fileengine.folder(conn, uid, version)?;
    let (node, who, can_write) = app.fileengine.with(conn, |c| Ok((c.stat(uid)?, c.whoami()?, c.can(uid, "w").unwrap_or(false))))?;
    if node.is_dir() {
        return Err(bad("fileengine.not_a_model", format!("{} is a folder", node.name)));
    }
    if !node.name.to_ascii_lowercase().ends_with(".ifc") {
        return Err(bad("fileengine.not_a_model", format!("{}: Reviera opens .ifc models from FileEngine (.ifczip is not supported yet)", node.name)));
    }
    let working = folder.join(&node.name);
    // Unsent work here is never overwritten: it opens as it is.
    if version.is_none()
        && let Some(l) = link_of(&working)
        && l.unsent
    {
        let id = match app.document_at(&working) {
            Some(id) => id,
            None => app.open(&working, false)?,
        };
        let newer = node.version != l.version;
        return Ok(OpenOut { document_id: id, working_copy: working, link: l, existing: true, newer_on_server: newer });
    }
    if let Some(id) = app.document_at(&working) {
        if app.get(id)?.is_dirty() {
            return Err(bad("fileengine.open_with_changes", format!("{} is open with unsaved changes; save or close it first", node.name)).with_details(serde_json::json!({ "document_id": id })));
        }
        app.close(id, crate::commands::SaveChoice::No)?;
    }
    let bytes = app.fileengine.with(conn, |c| match version {
        Some(v) => c.read_version(uid, v),
        None => c.read(uid),
    })?;
    std::fs::create_dir_all(&folder).map_err(|e| bad("fileengine.working_copy", e.to_string()))?;
    std::fs::write(&working, &bytes).map_err(|e| bad("fileengine.working_copy", e.to_string()))?;
    let profile = app.fileengine.profile(conn)?;
    let historic = version.is_some();
    let link = Link {
        connection_id: conn.to_owned(),
        url: profile.url,
        uid: uid.to_owned(),
        name: node.name.clone(),
        folder_uid: node.parent_uid.clone(),
        version: version.map_or(node.version.clone(), str::to_owned),
        user: who.user,
        read_only: historic || !can_write,
        historic,
        unsent: false,
        synced_at: now(),
    };
    write_link(&folder, &link)?;
    let id = app.open(&working, read_only || link.read_only)?;
    Ok(OpenOut { document_id: id, working_copy: working, existing: false, newer_on_server: historic && version != Some(node.version.as_str()), link })
}

host!(FeOpen, "fileengine.open", "Open a model from FileEngine",
    "Opens a FileEngine model (COL-FE-002): streams it into a working copy under \
     <data>/reviera/fileengine/<connection>/<uid>/ and opens that; Ctrl+S then saves it back as a new \
     version. A working copy holding work not uploaded yet is opened as it is, never overwritten. A model \
     the user may not write opens read-only, and so does a past `version`. Same as file.open with \
     fileengine://<connection>/<uid>.",
    [Network, FileRead, FileWrite], OpenParams, OpenOut, |app, caller, p| {
    open(app, &p.connection_id, &p.uid, p.version.as_deref(), p.read_only)
});

// ---- Saving (COL-FE-003, COL-FE-005) ----

/// What to do when someone else saved since the model was opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// Keep the save on this computer and report the conflict (the default).
    #[default]
    Refuse,
    /// Upload over it; their version stays in the history.
    Overwrite,
    /// Upload as a new file beside it, which the document then is.
    Copy,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Uploaded {
    pub connection_id: String,
    pub uid: String,
    pub name: String,
    /// The version the upload made.
    pub version: String,
    pub bytes: u64,
    /// Saved as a copy beside the original (the original's uid).
    pub copy_of: Option<String>,
}

/// The metadata a save writes on the model's file.
fn model_metadata(app: &App, id: DocumentId) -> Result<BTreeMap<String, String>> {
    let open = app.get(id)?;
    let store = open.doc.store();
    let mut m = BTreeMap::new();
    m.insert("ifc.schema".into(), open.project.ifc().schema().to_uppercase());
    if let Some(p) = store.iter().find(|e| e.class == "IfcProject") {
        m.insert("ifc.project_guid".into(), p.guid().to_string());
    }
    m.insert("reviera.version".into(), env!("CARGO_PKG_VERSION").into());
    let levels: Vec<String> = reviera_core::levels::list(store).into_iter().map(|l| l.name).collect();
    m.insert("reviera.levels".into(), serde_json::to_string(&levels).expect("serialises"));
    Ok(m)
}

/// A free name in `folder` for a copy of `name` saved by `user`.
fn copy_name(c: &Client, folder: &str, name: &str, user: &str) -> reviera_fileengine::Result<String> {
    let taken: Vec<String> = c.list(folder, false)?.into_iter().filter(|e| !e.deleted).map(|e| e.name).collect();
    let (stem, ext) = name.rsplit_once('.').map_or((name, ""), |(s, e)| (s, e));
    let who = user.split('@').next().unwrap_or(user);
    let dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let mut k = 1;
    loop {
        let n = if k == 1 { format!("{stem} ({who}'s copy){dot}") } else { format!("{stem} ({who}'s copy {k}){dot}") };
        if !taken.contains(&n) {
            return Ok(n);
        }
        k += 1;
    }
}

/// Uploads the saved working copy of document `id` (already written on this computer).
pub(crate) fn upload(app: &mut App, id: DocumentId, on_conflict: OnConflict, comment: Option<&str>) -> Result<Uploaded> {
    let (_, path, mut link) = app.linked(Some(id))?;
    if link.read_only {
        return Err(bad("fileengine.read_only", if link.historic { "a past version opened read-only; Save As to FileEngine to keep it" } else { "you may not write this model on FileEngine; Save As to a folder you can write" }));
    }
    let folder = path.parent().expect("a working copy is in a folder").to_owned();
    let bytes = std::fs::read(&path).map_err(|e| bad("fileengine.working_copy", e.to_string()))?;
    let mut meta = model_metadata(app, id)?;
    let comment = comment.map(str::trim).filter(|c| !c.is_empty());
    // Kept unsent until the server has it.
    link.unsent = true;
    write_link(&folder, &link)?;
    let expected = link.version.clone();
    let result = app.fileengine.with(&link.connection_id, |c| match on_conflict {
        OnConflict::Refuse => c.save_checked(&link.uid, &bytes, &expected).map(|v| (link.uid.clone(), link.name.clone(), v)),
        OnConflict::Overwrite => {
            c.write(&link.uid, &bytes)?;
            Ok((link.uid.clone(), link.name.clone(), c.stat(&link.uid)?.version))
        }
        OnConflict::Copy => {
            let name = copy_name(c, &link.folder_uid, &link.name, &link.user)?;
            let uid = c.create_file(&link.folder_uid, &name)?;
            let v = c.save_checked(&uid, &bytes, "")?;
            Ok((uid, name, v))
        }
    });
    let (uid, name, version) = match result {
        Ok(x) => x,
        Err(e) if e.code == "fileengine.conflict" => {
            let theirs = app.fileengine.with(&link.connection_id, |c| c.versions(&link.uid)).ok().and_then(|v| v.into_iter().next());
            return Err(Error::new("fileengine.conflict", format!("Saved on this computer, not uploaded: {}", e.message)).with_details(serde_json::json!({
                "their_version": theirs.as_ref().map(|v| v.version.clone()),
                "saved_by": theirs.map(|v| v.revised_by),
                "offer": ["overwrite", "copy", "open_theirs"],
            })));
        }
        Err(e) => return Err(Error::new(e.code.clone(), format!("Saved on this computer, not uploaded (the next save sends it): {}", e.message))),
    };
    if let Some(c) = comment {
        meta.insert("reviera.comment".into(), c.to_owned());
        meta.insert(format!("reviera.comment.{version}"), c.to_owned());
    }
    // The metadata is best effort: the model is uploaded.
    let _ = app.fileengine.with(&link.connection_id, |c| meta.iter().try_for_each(|(k, v)| c.set_metadata(&uid, k, v)));
    let copy_of = (uid != link.uid).then(|| link.uid.clone());
    if copy_of.is_some() {
        // The document becomes the copy: its own working copy, and the original's stays as it was
        // opened (its unsent work now lives in the copy).
        link.unsent = false;
        write_link(&folder, &link)?;
        let new_folder = app.fileengine.folder(&link.connection_id, &uid, None)?;
        std::fs::create_dir_all(&new_folder).map_err(|e| bad("fileengine.working_copy", e.to_string()))?;
        let new_path = new_folder.join(&name);
        let new_link = Link { uid: uid.clone(), name: name.clone(), version: version.clone(), unsent: false, synced_at: now(), ..link.clone() };
        write_link(&new_folder, &new_link)?;
        app.save(id, Some(&new_path))?;
    } else {
        link.version = version.clone();
        link.unsent = false;
        link.synced_at = now();
        write_link(&folder, &link)?;
    }
    Ok(Uploaded { connection_id: link.connection_id, uid, name, version, bytes: bytes.len() as u64, copy_of })
}

/// After `file.save` wrote a document: uploads it when it is a FileEngine model.
pub(crate) fn after_save(app: &mut App, id: DocumentId) -> Result<Option<Uploaded>> {
    let path = app.get(id)?.project.path().to_owned();
    match link_of(&path) {
        Some(l) if !l.read_only => upload(app, id, OnConflict::Refuse, None).map(Some),
        _ => Ok(None),
    }
}

/// `file.save_as` to `fileengine://<connection>/<folder uid>/<name>`: a new file there (or a new
/// version of the file of that name, with `replace`), which the document then is.
pub(crate) fn save_as(app: &mut App, id: DocumentId, conn: &str, folder_uid: &str, name: &str, replace: bool, comment: Option<&str>) -> Result<Uploaded> {
    let name = name.trim();
    if !name.to_ascii_lowercase().ends_with(".ifc") || name.contains('/') {
        return Err(bad("fileengine.invalid_input", "the model's name ends .ifc and has no '/'"));
    }
    let (node, who, existing) = app.fileengine.with(conn, |c| Ok((c.stat(folder_uid)?, c.whoami()?, c.find(folder_uid, name)?)))?;
    if !node.is_dir() {
        return Err(bad("fileengine.invalid_input", format!("{} is a file, not a folder", node.name)));
    }
    let (uid, version) = match existing {
        Some(e) if e.is_dir() => return Err(bad("fileengine.exists", format!("{name} is a folder"))),
        Some(_) if !replace => return Err(bad("fileengine.exists", format!("{name} is already in {}; save with replace to add a new version of it", node.name))),
        Some(e) => {
            let v = app.fileengine.with(conn, |c| c.stat(&e.uid))?.version;
            (e.uid, v)
        }
        None => (app.fileengine.with(conn, |c| c.create_file(folder_uid, name))?, String::new()),
    };
    let folder = app.fileengine.folder(conn, &uid, None)?;
    std::fs::create_dir_all(&folder).map_err(|e| bad("fileengine.working_copy", e.to_string()))?;
    let profile = app.fileengine.profile(conn)?;
    let link = Link { connection_id: conn.to_owned(), url: profile.url, uid, name: name.to_owned(), folder_uid: folder_uid.to_owned(), version, user: who.user, read_only: false, historic: false, unsent: true, synced_at: now() };
    write_link(&folder, &link)?;
    app.save(id, Some(&folder.join(name)))?;
    upload(app, id, OnConflict::Overwrite, comment)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    pub document_id: Option<DocumentId>,
    /// When someone else saved since it was opened: refuse (default), overwrite, or copy.
    #[serde(default)]
    pub on_conflict: OnConflict,
    /// Kept with the version (`reviera.comment`).
    pub comment: Option<String>,
}

host!(FeSave, "fileengine.save", "Save to FileEngine",
    "Saves the FileEngine model (COL-FE-003): writes the working copy, then uploads it as a new version if \
     the server's newest version is still the one opened. If someone else saved first, the save stays on \
     this computer and the error says who; save again with `on_conflict` overwrite (their version stays \
     in the history) or copy (a new file beside it, which the document becomes). A `comment` is kept with \
     the version. Ctrl+S (file.save) does the same with the default.",
    [Network, FileWrite], SaveParams, Uploaded, |app, caller, p| {
    let (id, _, _) = app.linked(p.document_id)?;
    if app.get(id)?.is_dirty() {
        app.save(id, None)?;
    }
    upload(app, id, p.on_conflict, p.comment.as_deref())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveAsParams {
    pub document_id: Option<DocumentId>,
    pub connection_id: String,
    /// The folder to save in.
    pub folder_uid: String,
    /// The model's name there (ending .ifc).
    pub name: String,
    /// Add a new version to the file of that name if there is one.
    #[serde(default)]
    pub replace: bool,
    pub comment: Option<String>,
}

host!(FeSaveAs, "fileengine.save_as", "Save As to FileEngine",
    "Saves the project as a model in a FileEngine folder (COL-FE-003): a new file `name` there (refused if \
     one is there, unless `replace`), uploaded as its first version, with the model's metadata. The \
     document then is that model, and Ctrl+S saves new versions of it. Same as file.save_as with \
     fileengine://<connection>/<folder uid>/<name>.",
    [Network, FileWrite], SaveAsParams, Uploaded, |app, caller, p| {
    let id = app.resolve(p.document_id)?;
    save_as(app, id, &p.connection_id, &p.folder_uid, &p.name, p.replace, p.comment.as_deref())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {
    pub document_id: Option<DocumentId>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    /// The document is a FileEngine model.
    pub linked: bool,
    pub link: Option<Link>,
    pub connected: bool,
}

host!(FeStatus, "fileengine.status", "Tell whether the project is on FileEngine",
    "Whether the document is a FileEngine model (COL-FE-002) and its link: connection, uid, name, folder, \
     the version opened or last saved, read-only, a past version, and work not uploaded yet.",
    [Read], Option<StatusParams>, StatusOut, |app, caller, p| {
    let id = app.resolve(p.and_then(|p| p.document_id))?;
    let link = link_of(app.get(id)?.project.path());
    let connected = link.as_ref().is_some_and(|l| app.fileengine.clients.get(&l.connection_id).is_some_and(Client::signed_in));
    Ok(StatusOut { linked: link.is_some(), link, connected })
});

// ---- Versions (COL-FE-004) ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct VersionOut {
    pub version: String,
    pub saved_by: String,
    /// The save comment Reviera kept with it.
    pub comment: Option<String>,
    /// The version the open working copy is at.
    pub current: bool,
    pub newest: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionsParams {
    /// The open FileEngine model (default: the active document), or a file by connection and uid.
    pub document_id: Option<DocumentId>,
    pub connection_id: Option<String>,
    pub uid: Option<String>,
}

fn target(app: &App, document_id: Option<DocumentId>, connection_id: Option<String>, uid: Option<String>) -> Result<(String, String, Option<Link>)> {
    match (connection_id, uid) {
        (Some(c), Some(u)) => Ok((c, u, None)),
        (None, None) => {
            let (_, _, l) = app.linked(document_id)?;
            Ok((l.connection_id.clone(), l.uid.clone(), Some(l)))
        }
        _ => Err(bad("fileengine.invalid_input", "give the connection_id and uid together, or neither for the open model")),
    }
}

host!(FeVersions, "fileengine.versions", "List a model's FileEngine versions",
    "The file's versions, newest first (COL-FE-004): each version's timestamp, who saved it, the comment \
     Reviera kept with it, and which one the open working copy is at.",
    [Network, Read], Option<VersionsParams>, Vec<VersionOut>, |app, caller, p| {
    let p = p.unwrap_or(VersionsParams { document_id: None, connection_id: None, uid: None });
    let (conn, uid, link) = target(app, p.document_id, p.connection_id, p.uid)?;
    let (versions, meta) = app.fileengine.with(&conn, |c| Ok((c.versions(&uid)?, c.metadata(&uid).unwrap_or_default())))?;
    Ok(versions
        .into_iter()
        .enumerate()
        .map(|(k, v)| VersionOut {
            comment: meta.get(&format!("reviera.comment.{}", v.version)).cloned(),
            current: link.as_ref().is_some_and(|l| l.version == v.version),
            newest: k == 0,
            saved_by: v.revised_by,
            version: v.version,
        })
        .collect())
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    pub document_id: Option<DocumentId>,
    pub connection_id: Option<String>,
    pub uid: Option<String>,
    pub version: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RestoreOut {
    /// The version restored, and the new version it made.
    pub restored: String,
    pub version: String,
    /// The open model, reopened at the new version.
    pub document_id: Option<DocumentId>,
}

host!(FeRestore, "fileengine.restore_version", "Restore a FileEngine version",
    "Makes a past version the newest again (COL-FE-004): FileEngine saves its content as a new version, so \
     nothing is lost. The open model (refused while it has unsaved changes or work not uploaded) is \
     reopened at the new version.",
    [Network, FileWrite], RestoreParams, RestoreOut, |app, caller, p| {
    let (conn, uid, link) = target(app, p.document_id, p.connection_id, p.uid)?;
    let open_doc = link.as_ref().and_then(|_| app.resolve(p.document_id).ok());
    if let (Some(id), Some(l)) = (open_doc, &link) {
        if app.get(id)?.is_dirty() || l.unsent {
            return Err(bad("fileengine.open_with_changes", "the model has changes not saved to FileEngine; save them (or close without saving) before restoring a version"));
        }
        if l.read_only {
            return Err(bad("fileengine.read_only", "you may not write this model on FileEngine"));
        }
    }
    let restored = app.fileengine.with(&conn, |c| c.restore(&uid, &p.version))?;
    let version = app.fileengine.with(&conn, |c| c.stat(&uid))?.version;
    let document_id = match open_doc {
        Some(id) => {
            app.close(id, crate::commands::SaveChoice::No)?;
            Some(open(app, &conn, &uid, None, false)?.document_id)
        }
        None => None,
    };
    Ok(RestoreOut { restored, version, document_id })
});
