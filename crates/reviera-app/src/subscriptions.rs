// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `subscription.*`: remote analysis subscriptions (DM-RPC-011). A subscription is a service URL
//! (`https://` only) and an API key with its secret. The secret lives in the OS keychain
//! (Secret Service, Keychain, Credential Manager), or for this session only where there is none;
//! the URL, name, key and last status are per-user settings (`remote-analysis.json`). The
//! service is a separate project; the client is `reviera_compute_proto::subscription` (MIT).
//!
//! `subscription.list` and `.status` are read-only and open to MCP; `add`, `set`, `remove`,
//! `test` and `set_default` are for the Remote Analysis dialog and Lua, so no agent handles
//! credentials. No command ever returns a secret.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use reviera_compute_proto::subscription::{self as sub, Client, Credentials, Endpoint, Subscription};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result, Source};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

/// The keychain service the secrets are stored under (the account is the subscription id).
pub const KEYCHAIN_SERVICE: &str = "org.bimreviera.remote-analysis";
/// Warn this many days before a subscription's period ends.
const EXPIRY_WARNING_DAYS: i64 = 7;
/// Warn when this share of the allowance is used.
const USAGE_WARNING: f64 = 0.9;

/// Commands that take or test credentials: never offered to agents (MCP).
pub const NOT_FOR_AGENTS: &[&str] =
    &["subscription.add", "subscription.set", "subscription.remove", "subscription.test", "subscription.set_default"];

/// The settings file, in the per-user settings folder.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    subscriptions: Vec<Entry>,
    /// The default subscription overall.
    default: Option<String>,
    /// Defaults per job kind.
    defaults_by_kind: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Entry {
    id: String,
    name: String,
    url: String,
    key: String,
    ca_pem: Option<String>,
    pin: Option<String>,
    development: bool,
    /// The last status the service gave, or why there was none.
    last: Option<Checked>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Checked {
    /// Unix seconds.
    at: i64,
    subscription: Option<Subscription>,
    reason: Option<String>,
    message: Option<String>,
}

/// The subscriptions' working state on the app.
#[derive(Default)]
pub struct Subscriptions {
    /// The settings file; the per-user one when `None`.
    path: Option<PathBuf>,
    /// Keeps secrets in memory only (tests, or no keychain).
    session_only: bool,
    session: HashMap<String, String>,
    /// One client per subscription, holding its token.
    clients: HashMap<String, Client>,
}

impl std::fmt::Debug for Subscriptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriptions").field("path", &self.path).field("session_only", &self.session_only).finish_non_exhaustive()
    }
}

/// The per-user settings folder: `%APPDATA%\Reviera`, else `$XDG_CONFIG_HOME/reviera` (or
/// `~/.config/reviera`), as the window's.
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Some(a) = std::env::var_os("APPDATA") {
        return Some(Path::new(&a).join("Reviera"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("reviera"))
}

impl Subscriptions {
    /// Keeps the settings in `path` and the secrets in memory: for tests and tools.
    pub fn isolated(path: PathBuf) -> Self {
        Self { path: Some(path), session_only: true, ..Self::default() }
    }

    fn file(&self) -> Result<PathBuf> {
        self.path
            .clone()
            .or_else(|| config_dir().map(|d| d.join("remote-analysis.json")))
            .ok_or_else(|| Error::new("subscription.no_settings", "there is no per-user settings folder"))
    }

    fn load(&self) -> Result<Settings> {
        match std::fs::read(self.file()?) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| Error::new("subscription.settings_invalid", format!("remote-analysis.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(Error::new("subscription.settings_unreadable", e.to_string())),
        }
    }

    fn save(&self, s: &Settings) -> Result<()> {
        let path = self.file()?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| Error::new("subscription.settings_unwritable", e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s).expect("settings serialise"))
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| Error::new("subscription.settings_unwritable", e.to_string()))
    }

    fn keychain(&self) -> bool {
        !self.session_only && keyring::Entry::store_status().is_ok()
    }

    /// Stores a secret; says where it went.
    fn put_secret(&mut self, id: &str, secret: &str) -> Storage {
        if self.keychain() && keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.set_password(secret)).is_ok() {
            self.session.remove(id);
            return Storage::Keychain;
        }
        self.session.insert(id.to_owned(), secret.to_owned());
        Storage::Session
    }

    fn secret(&self, id: &str) -> Option<(String, Storage)> {
        if let Some(s) = self.session.get(id) {
            return Some((s.clone(), Storage::Session));
        }
        if !self.keychain() {
            return None;
        }
        keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.get_password()).ok().map(|s| (s, Storage::Keychain))
    }

    fn delete_secret(&mut self, id: &str) {
        self.session.remove(id);
        if self.keychain() {
            let _ = keyring::Entry::new(KEYCHAIN_SERVICE, id).and_then(|e| e.delete_credential());
        }
    }
}

/// Where a secret is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    /// The operating system's keychain.
    Keychain,
    /// Memory, until Reviera closes (no keychain was available).
    Session,
    /// Neither: entered in an earlier session without a keychain; Edit takes it again.
    Missing,
}

fn endpoint(e: &Entry) -> Endpoint {
    Endpoint { url: e.url.clone(), ca_pem: e.ca_pem.clone(), pin_sha256: e.pin.clone(), development: e.development, ..Endpoint::default() }
}

fn failed(e: sub::Error) -> Error {
    let code = format!("subscription.{}", e.reason);
    Error::new(code, e.message).with_details(serde_json::json!({ "reason": e.reason, "service": e.detail }))
}

fn not_for_agents(caller: &Caller, name: &str) -> Result<()> {
    if caller.origin.source == Source::Mcp {
        return Err(Error::new(
            "permission.denied",
            format!("`{name}` handles a subscription's credentials, so it is for the Remote Analysis dialog and Lua only"),
        ));
    }
    Ok(())
}

/// A CA given as PEM text or as the path of a PEM file.
fn ca_text(ca: &str) -> Result<Option<String>> {
    let ca = ca.trim();
    if ca.is_empty() {
        return Ok(None);
    }
    if ca.starts_with("-----BEGIN") {
        return Ok(Some(ca.to_owned()));
    }
    std::fs::read_to_string(ca).map(Some).map_err(|e| Error::new("subscription.invalid_input", format!("the CA file {ca}: {e}")))
}

/// A subscription's status as shown in the dialog and returned by the commands.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Status {
    /// `active`, `trial`, `suspended`, `expired`, `refused` (the key was refused),
    /// `unreachable`, `secret_missing` or `not_checked`.
    pub state: String,
    /// The name the service has for the account.
    pub account: String,
    pub plan: String,
    pub period_start: String,
    pub period_end: String,
    /// 0 means unlimited.
    pub allowance: f64,
    pub usage: f64,
    /// `compute_hours`, `credits` or `jobs`.
    pub unit: String,
    /// What is left of the allowance, when there is one.
    pub remaining: Option<f64>,
    pub job_kinds: Vec<String>,
    pub regions: Vec<String>,
    /// The largest job, bytes; 0 means no stated limit.
    pub max_job_bytes: u64,
    /// Jobs at once; 0 means no stated limit.
    pub concurrent_jobs: u32,
    pub terms_url: String,
    /// When the service was last asked (RFC 3339).
    pub checked_at: Option<String>,
    /// The reason code of the last failure.
    pub reason: Option<String>,
    pub message: Option<String>,
    /// Expiry within 7 days, 90 % of the allowance used.
    pub warnings: Vec<String>,
}

fn status_of(e: &Entry, storage: Storage) -> Status {
    let mut st = Status { state: "not_checked".into(), ..Status::default() };
    if let Some(c) = &e.last {
        st.checked_at = Some(sub::format_time(c.at));
        st.reason.clone_from(&c.reason);
        st.message.clone_from(&c.message);
        if let Some(s) = &c.subscription {
            st.state = s.state.clone();
            st.account = s.name.clone();
            st.plan = s.plan.clone();
            st.period_start = s.period_start.clone();
            st.period_end = s.period_end.clone();
            st.allowance = s.allowance;
            st.usage = s.usage;
            st.unit = s.unit.clone();
            st.remaining = (s.allowance > 0.0).then(|| (s.allowance - s.usage).max(0.0));
            st.job_kinds = s.job_kinds.clone();
            st.regions = s.regions.clone();
            st.max_job_bytes = s.limits.max_job_bytes;
            st.concurrent_jobs = s.limits.concurrent_jobs;
            st.terms_url = s.terms_url.clone();
        }
        match c.reason.as_deref() {
            None => {}
            Some("unauthorised") => st.state = "refused".into(),
            Some("subscription_inactive") => {
                if !matches!(st.state.as_str(), "suspended" | "expired") {
                    st.state = "suspended".into();
                }
            }
            Some("quota_exceeded") => {}
            Some(_) => st.state = "unreachable".into(),
        }
    }
    if storage == Storage::Missing {
        st.state = "secret_missing".into();
        st.message = Some("The secret was kept for an earlier session only (no keychain); Edit takes it again".into());
    }
    if let Some(end) = sub::parse_time(&st.period_end) {
        let days = (end - sub::now()) as f64 / 86_400.0;
        if days >= 0.0 && days <= EXPIRY_WARNING_DAYS as f64 {
            st.warnings.push(format!("The subscription's period ends in {} day(s), on {}", days.ceil() as i64, &st.period_end[..10.min(st.period_end.len())]));
        }
    }
    if st.allowance > 0.0 && st.usage >= USAGE_WARNING * st.allowance {
        st.warnings.push(format!("{:.0} % of the allowance ({} {}) is used", 100.0 * st.usage / st.allowance, st.allowance, st.unit.replace('_', " ")));
    }
    st
}

impl App {
    /// The subscriptions' state (settings file, secrets, clients).
    pub fn subscriptions_mut(&mut self) -> &mut Subscriptions {
        &mut self.subscriptions
    }

    /// Asks the service for a subscription's status, renewing its token as needed, and records
    /// the answer (or the failure) in the settings.
    fn check_subscription(&mut self, settings: &mut Settings, at: usize, fresh: bool) -> Result<()> {
        let e = settings.subscriptions[at].clone();
        let Some((secret, _)) = self.subscriptions.secret(&e.id) else {
            return Ok(());
        };
        let creds = Credentials::new(e.key.clone(), secret);
        if fresh {
            self.subscriptions.clients.remove(&e.id);
        }
        if !self.subscriptions.clients.contains_key(&e.id) {
            match Client::new(&endpoint(&e)) {
                Ok(c) => {
                    self.subscriptions.clients.insert(e.id.clone(), c);
                }
                Err(err) => {
                    settings.subscriptions[at].last = Some(Checked { at: sub::now(), subscription: None, reason: Some(err.reason), message: Some(err.message) });
                    return Ok(());
                }
            }
        }
        let client = &self.subscriptions.clients[&e.id];
        let checked = match client.subscription(&creds) {
            Ok(s) => Checked { at: sub::now(), subscription: Some(s), reason: None, message: None },
            Err(err) => {
                // Keep what the service last said, and why it cannot say more now.
                let last = e.last.and_then(|l| l.subscription);
                Checked { at: sub::now(), subscription: last, reason: Some(err.reason), message: Some(err.message) }
            }
        };
        settings.subscriptions[at].last = Some(checked);
        Ok(())
    }

    fn storage_of(&self, id: &str) -> Storage {
        self.subscriptions.secret(id).map_or(Storage::Missing, |s| s.1)
    }

    fn subscription_out(&self, s: &Settings, e: &Entry) -> SubscriptionOut {
        let storage = self.storage_of(&e.id);
        SubscriptionOut {
            id: e.id.clone(),
            name: e.name.clone(),
            url: e.url.clone(),
            key: e.key.clone(),
            default: s.default.as_deref() == Some(&e.id),
            default_for: s.defaults_by_kind.iter().filter(|(_, v)| **v == e.id).map(|(k, _)| k.clone()).collect(),
            secret_storage: storage,
            ca: e.ca_pem.is_some(),
            pin: e.pin.clone(),
            development: e.development,
            status: status_of(e, storage),
        }
    }
}

fn find(s: &Settings, id: &str) -> Result<usize> {
    s.subscriptions
        .iter()
        .position(|e| e.id == id || e.name == id)
        .ok_or_else(|| Error::new("subscription.not_found", format!("no subscription {id}")))
}

/// A subscription as listed: never its secret.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SubscriptionOut {
    pub id: String,
    pub name: String,
    pub url: String,
    pub key: String,
    /// The default subscription overall.
    pub default: bool,
    /// The job kinds it is the default for.
    pub default_for: Vec<String>,
    pub secret_storage: Storage,
    /// Whether an extra CA certificate is trusted for it.
    pub ca: bool,
    /// The pinned SHA-256 of the service's certificate, if any.
    pub pin: Option<String>,
    /// Whether `http://localhost` is allowed (developing a service).
    pub development: bool,
    pub status: Status,
}

/// `subscription.add`'s parameters. (No `Debug`: it holds the secret.)
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddParams {
    pub name: String,
    /// The service URL, `https://…`.
    pub url: String,
    /// The API key, or `key:secret` when `secret` is not given.
    pub key: String,
    pub secret: Option<String>,
    /// An extra CA certificate: PEM text or the path of a PEM file.
    pub ca: Option<String>,
    /// The SHA-256 of the service's certificate (hex).
    pub pin: Option<String>,
    /// Allows `http://localhost`, for developing a service.
    #[serde(default)]
    pub development: bool,
}

impl std::fmt::Debug for AddParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddParams").field("name", &self.name).field("url", &self.url).finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AddOut {
    pub subscription_id: String,
    pub secret_storage: Storage,
    pub status: Status,
}

pub(crate) struct SubscriptionAdd;

impl HostCommand for SubscriptionAdd {
    type Host = App;
    type Params = AddParams;
    type Output = AddOut;
    const META: CommandMeta = CommandMeta {
        name: "subscription.add",
        version: 1,
        summary: "Add a remote analysis subscription",
        description: "Adds a subscription to a remote analysis service (DM-RPC-011): its name, https:// URL and API \
                      key and secret. The connection is tested first; nothing is saved if it fails. The secret goes \
                      to the OS keychain (or is kept for this session where there is none) and is never returned. \
                      Not available to MCP.",
        effects: &[Effect::Settings, Effect::Network],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: AddParams) -> Result<AddOut> {
        not_for_agents(caller, Self::META.name)?;
        let name = p.name.trim().to_owned();
        if name.is_empty() {
            return Err(Error::new("subscription.invalid_input", "a subscription needs a name"));
        }
        let creds = match p.secret.filter(|s| !s.is_empty()) {
            Some(secret) => Credentials::new(p.key.trim(), secret),
            None => Credentials::parse(&p.key)
                .ok_or_else(|| Error::new("subscription.invalid_input", "give the API key and secret, or `key:secret`"))?,
        };
        if creds.key.is_empty() {
            return Err(Error::new("subscription.invalid_input", "the API key is empty"));
        }
        let mut settings = app.subscriptions.load()?;
        if settings.subscriptions.iter().any(|e| e.name.eq_ignore_ascii_case(&name)) {
            return Err(Error::new("subscription.exists", format!("there is already a subscription named {name}")));
        }
        let mut entry = Entry {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name,
            url: p.url.trim().to_owned(),
            key: creds.key.clone(),
            ca_pem: ca_text(p.ca.as_deref().unwrap_or(""))?,
            pin: p.pin.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
            development: p.development,
            last: None,
        };
        let client = Client::new(&endpoint(&entry)).map_err(failed)?;
        entry.url = client.url().to_owned();
        let s = client.subscription(&creds).map_err(failed)?;
        entry.last = Some(Checked { at: sub::now(), subscription: Some(s), reason: None, message: None });
        let storage = app.subscriptions.put_secret(&entry.id, &creds.secret);
        app.subscriptions.clients.insert(entry.id.clone(), client);
        if settings.subscriptions.is_empty() {
            settings.default = Some(entry.id.clone());
        }
        let out = AddOut { subscription_id: entry.id.clone(), secret_storage: storage, status: status_of(&entry, storage) };
        settings.subscriptions.push(entry);
        app.subscriptions.save(&settings)?;
        Ok(out)
    }
}

/// `subscription.set`'s parameters. (No `Debug`: it may hold the secret.)
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// The subscription's id (or its name).
    pub subscription_id: String,
    pub name: Option<String>,
    pub url: Option<String>,
    /// A new API key, or `key:secret` when `secret` is not given.
    pub key: Option<String>,
    /// A new secret: it replaces the stored one, which is never shown.
    pub secret: Option<String>,
    /// An extra CA certificate (PEM text or a file path); empty removes it.
    pub ca: Option<String>,
    /// The certificate's SHA-256 pin; empty removes it.
    pub pin: Option<String>,
    pub development: Option<bool>,
}

impl std::fmt::Debug for SetParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetParams").field("subscription_id", &self.subscription_id).finish_non_exhaustive()
    }
}

pub(crate) struct SubscriptionSet;

impl HostCommand for SubscriptionSet {
    type Host = App;
    type Params = SetParams;
    type Output = SubscriptionOut;
    const META: CommandMeta = CommandMeta {
        name: "subscription.set",
        version: 1,
        summary: "Change a remote analysis subscription",
        description: "Changes a subscription's name, URL, key, secret, CA certificate, pin or development setting \
                      (DM-RPC-011). When the URL or the credentials change, the connection is tested first and \
                      nothing is saved if it fails. Not available to MCP.",
        effects: &[Effect::Settings, Effect::Network],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: SetParams) -> Result<SubscriptionOut> {
        not_for_agents(caller, Self::META.name)?;
        let mut settings = app.subscriptions.load()?;
        let at = find(&settings, &p.subscription_id)?;
        let mut e = settings.subscriptions[at].clone();
        if let Some(n) = p.name.map(|n| n.trim().to_owned()) {
            if n.is_empty() {
                return Err(Error::new("subscription.invalid_input", "a subscription needs a name"));
            }
            if settings.subscriptions.iter().any(|o| o.id != e.id && o.name.eq_ignore_ascii_case(&n)) {
                return Err(Error::new("subscription.exists", format!("there is already a subscription named {n}")));
            }
            e.name = n;
        }
        let mut secret = p.secret.filter(|s| !s.is_empty());
        if let Some(k) = p.key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) {
            match (secret.is_none(), Credentials::parse(&k)) {
                (true, Some(c)) => {
                    e.key = c.key;
                    secret = Some(c.secret);
                }
                _ => e.key = k,
            }
        }
        if let Some(u) = p.url {
            e.url = u.trim().to_owned();
        }
        if let Some(ca) = p.ca {
            e.ca_pem = ca_text(&ca)?;
        }
        if let Some(pin) = p.pin {
            e.pin = Some(pin.trim().to_owned()).filter(|s| !s.is_empty());
        }
        if let Some(d) = p.development {
            e.development = d;
        }
        let old = &settings.subscriptions[at];
        let connection = e.url != old.url || e.key != old.key || secret.is_some() || e.ca_pem != old.ca_pem || e.pin != old.pin || e.development != old.development;
        if connection {
            let secret = match secret {
                Some(s) => s,
                None => app
                    .subscriptions
                    .secret(&e.id)
                    .map(|s| s.0)
                    .ok_or_else(|| Error::new("subscription.secret_missing", "the secret is not stored; enter it again"))?,
            };
            let creds = Credentials::new(e.key.clone(), secret);
            let client = Client::new(&endpoint(&e)).map_err(failed)?;
            e.url = client.url().to_owned();
            let s = client.subscription(&creds).map_err(failed)?;
            e.last = Some(Checked { at: sub::now(), subscription: Some(s), reason: None, message: None });
            app.subscriptions.put_secret(&e.id, &creds.secret);
            app.subscriptions.clients.insert(e.id.clone(), client);
        }
        settings.subscriptions[at] = e;
        app.subscriptions.save(&settings)?;
        Ok(app.subscription_out(&settings, &settings.subscriptions[at]))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    /// The subscription's id (or its name).
    pub subscription_id: String,
}

pub(crate) struct SubscriptionRemove;

impl HostCommand for SubscriptionRemove {
    type Host = App;
    type Params = IdParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "subscription.remove",
        version: 1,
        summary: "Remove a remote analysis subscription",
        description: "Removes a subscription and deletes its secret from the keychain (DM-RPC-011). Jobs already on \
                      the service keep running there, but can only be fetched after it is added again. Not \
                      available to MCP.",
        effects: &[Effect::Settings, Effect::Destructive],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: IdParams) -> Result<()> {
        not_for_agents(caller, Self::META.name)?;
        let mut settings = app.subscriptions.load()?;
        let at = find(&settings, &p.subscription_id)?;
        let e = settings.subscriptions.remove(at);
        if settings.default.as_deref() == Some(&e.id) {
            settings.default = None;
        }
        settings.defaults_by_kind.retain(|_, v| *v != e.id);
        app.subscriptions.delete_secret(&e.id);
        app.subscriptions.clients.remove(&e.id);
        app.subscriptions.save(&settings)
    }
}

pub(crate) struct SubscriptionList;

impl HostCommand for SubscriptionList {
    type Host = App;
    type Params = Option<serde_json::Value>;
    type Output = Vec<SubscriptionOut>;
    const META: CommandMeta = CommandMeta {
        name: "subscription.list",
        version: 1,
        summary: "List the remote analysis subscriptions",
        description: "Lists the remote analysis subscriptions (DM-RPC-011) with their URL, key, where the secret is \
                      kept, defaults and last status. Never returns a secret and makes no connection.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<serde_json::Value>) -> Result<Vec<SubscriptionOut>> {
        let settings = app.subscriptions.load()?;
        Ok(settings.subscriptions.iter().map(|e| app.subscription_out(&settings, e)).collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {
    /// The subscription's id (or its name).
    pub subscription_id: String,
    /// Asks the service now instead of returning the last status; default false.
    #[serde(default)]
    pub refresh: bool,
}

pub(crate) struct SubscriptionStatus;

impl HostCommand for SubscriptionStatus {
    type Host = App;
    type Params = StatusParams;
    type Output = Status;
    const META: CommandMeta = CommandMeta {
        name: "subscription.status",
        version: 1,
        summary: "A remote analysis subscription's status",
        description: "Returns a subscription's plan, state, period, allowance and usage, job kinds, limits and \
                      warnings (DM-RPC-011): the last status, or with `refresh` the service's answer now. A key \
                      the service refused is not tried again until it is changed.",
        effects: &[Effect::Read, Effect::Network],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: StatusParams) -> Result<Status> {
        let mut settings = app.subscriptions.load()?;
        let at = find(&settings, &p.subscription_id)?;
        let refused = settings.subscriptions[at].last.as_ref().and_then(|l| l.reason.as_deref()) == Some("unauthorised");
        if p.refresh && !refused {
            app.check_subscription(&mut settings, at, false)?;
            app.subscriptions.save(&settings)?;
        }
        let e = &settings.subscriptions[at];
        Ok(status_of(e, app.storage_of(&e.id)))
    }
}

pub(crate) struct SubscriptionTest;

impl HostCommand for SubscriptionTest {
    type Host = App;
    type Params = IdParams;
    type Output = Status;
    const META: CommandMeta = CommandMeta {
        name: "subscription.test",
        version: 1,
        summary: "Test a remote analysis subscription's connection",
        description: "Authenticates afresh with the stored key and secret and asks the service for the \
                      subscription's status (DM-RPC-011); a failure is reported in the status (`reason`, \
                      `message`), for example \"The service refused the key\". Not available to MCP.",
        effects: &[Effect::Read, Effect::Network],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: IdParams) -> Result<Status> {
        not_for_agents(caller, Self::META.name)?;
        let mut settings = app.subscriptions.load()?;
        let at = find(&settings, &p.subscription_id)?;
        app.check_subscription(&mut settings, at, true)?;
        app.subscriptions.save(&settings)?;
        let e = &settings.subscriptions[at];
        Ok(status_of(e, app.storage_of(&e.id)))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DefaultParams {
    /// The subscription's id (or its name).
    pub subscription_id: String,
    /// Makes it the default for this job kind only (for example `analysis.energy`).
    pub job_kind: Option<String>,
}

pub(crate) struct SubscriptionSetDefault;

impl HostCommand for SubscriptionSetDefault {
    type Host = App;
    type Params = DefaultParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "subscription.set_default",
        version: 1,
        summary: "Set the default remote analysis subscription",
        description: "Makes a subscription the default overall, or for one job kind (DM-RPC-011). Not available to MCP.",
        effects: &[Effect::Settings],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: DefaultParams) -> Result<()> {
        not_for_agents(caller, Self::META.name)?;
        let mut settings = app.subscriptions.load()?;
        let id = settings.subscriptions[find(&settings, &p.subscription_id)?].id.clone();
        match p.job_kind.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) {
            Some(k) => {
                settings.defaults_by_kind.insert(k, id);
            }
            None => settings.default = Some(id),
        }
        app.subscriptions.save(&settings)
    }
}
