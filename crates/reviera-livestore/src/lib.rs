// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Live model store: a per-project SQLite database, updated after every committed transaction, from
//! which unsaved work is recovered after a crash (spec Ch 2b).
//!
//! [`LiveStore::create`] attaches to a [`Document`]: every commit, undo, redo and group end is sent
//! to a writer thread that group-commits it (§4). [`scan`] finds stores left by a crash (§6.1) and
//! [`restore`] rebuilds the project from the materialised state, or, if the database is damaged,
//! from the base IFC plus a replay of whatever journal rows can still be read (§6.3).
//!
//! Not yet implemented: encryption at rest (DM-LIVE-016), interactive-session journaling
//! (DM-LIVE-015), housekeeping (DM-LIVE-010), time travel (DM-LIVE-017/018) and BIMserver base
//! revisions (DM-LIVE-012).

mod crypto;
pub mod lock;
mod schema;
mod time;
mod writer;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use reviera_core::{
    ChangeRecord, ChangeSet, DocEvent, Document, Element, NewElement, Origin, UndoItem,
};
use reviera_ifc::StepRow;
use reviera_store::LocalProject;
use schemars::JsonSchema;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub use crypto::{Encryption, EncryptionStatus, KeyError, KeySource};
pub use lock::LockInfo;
pub use schema::Synchronous;

use writer::{Durable, ElementRow, Msg, StateDelta, TxnRow, Writer};

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("the live store {key} is open in another Reviera process (PID {pid} on {host})")]
    Locked { key: String, pid: u32, host: String },
    #[error("the live store {0} holds unsaved work; recover or discard it first")]
    NeedsRecovery(String),
    #[error("nothing could be recovered from {key}: {reason}")]
    Unrecoverable { key: String, reason: String },
    #[error("the live-store writer has stopped")]
    WriterStopped,
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] reviera_store::StoreError),
    #[error(transparent)]
    Ifc(#[from] reviera_ifc::IfcError),
    #[error(transparent)]
    Core(#[from] reviera_core::Error),
    #[error("corrupt journal: {0}")]
    Journal(String),
    #[error(transparent)]
    Key(#[from] KeyError),
}

pub type Result<T, E = LiveError> = std::result::Result<T, E>;

/// Live-store preferences (§5, DM-LIVE-014).
#[derive(Debug, Clone)]
pub struct Options {
    pub dir: PathBuf,
    /// Group-commit window, 50–2,000 ms.
    pub group_commit: Duration,
    pub synchronous: Synchronous,
    /// Undo items rebuilt on recovery.
    pub undo_depth: usize,
    /// How new stores are encrypted (default: a key in the OS credential store).
    pub encryption: Encryption,
    /// Policy: refuse to create a store without a usable key.
    pub require_encryption: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dir: default_dir(),
            group_commit: Duration::from_millis(250),
            synchronous: Synchronous::Normal,
            undo_depth: 200,
            encryption: Encryption::default(),
            require_encryption: false,
        }
    }
}

/// `<data_dir>/Reviera/live` (§2).
pub fn default_dir() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
    };
    base.join("Reviera/live")
}

/// The store key for a project file: a hash of its canonical path. The key stays fixed for a
/// session; Save As to another path re-keys at the next open.
pub fn store_key(project_path: &Path) -> String {
    let canonical = fs::canonicalize(project_path).unwrap_or_else(|_| {
        let parent = project_path
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .unwrap_or_default();
        parent.join(project_path.file_name().unwrap_or_default())
    });
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    hex::encode(&digest[..16])
}

fn db_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.rvlive"))
}

fn lock_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.lock"))
}

fn key_id_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.keyid"))
}

fn companions(dir: &Path, key: &str) -> [PathBuf; 5] {
    let db = db_path(dir, key);
    let suffixed = |s: &str| PathBuf::from(format!("{}{s}", db.display()));
    [
        db.clone(),
        suffixed("-wal"),
        suffixed("-shm"),
        lock_path(dir, key),
        key_id_path(dir, key),
    ]
}

/// The key of an existing store, from its key file and the options.
fn existing_key(dir: &Path, key: &str, options: &Options) -> (crypto::StoreKey, bool) {
    let key_id = crypto::read_key_id(&key_id_path(dir, key));
    let store_key = crypto::existing_key(key_id.as_deref(), &options.encryption);
    // A store with a key file but no key found: its key is lost (or the keyring is unavailable).
    let lost = key_id.is_some() && store_key.source == KeySource::None;
    (store_key, lost)
}

/// An open live store, attached to one document.
#[derive(Debug)]
pub struct LiveStore {
    key: String,
    dir: PathBuf,
    tx: SyncSender<Msg>,
    writer: Option<JoinHandle<rusqlite::Result<()>>>,
    durable: Arc<Mutex<Durable>>,
    pending: Arc<AtomicUsize>,
    store_key: crypto::StoreKey,
    required: bool,
}

/// `live.status` (§2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LiveStatus {
    pub path: PathBuf,
    pub head_seq: i64,
    pub saved_seq: i64,
    /// Change sets queued but not yet written.
    pub pending: usize,
    pub last_commit_at: Option<String>,
    /// Bytes on disk, including the WAL.
    pub size: u64,
}

/// One journal entry.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TxnSummary {
    pub seq: i64,
    pub at: String,
    pub origin: Origin,
    pub name: String,
    /// On the redo stack.
    pub undone: bool,
}

impl LiveStore {
    /// Creates the store for `project` and starts mirroring `doc` into it. Fails if another process
    /// holds the store, or if an earlier session left unsaved work there.
    pub fn create(project: &LocalProject, doc: &mut Document, options: &Options) -> Result<Self> {
        let dir = options.dir.clone();
        create_private_dir(&dir)?;
        let key = store_key(project.path());
        let lock_file = lock_path(&dir, &key);
        if let Some(info) = LockInfo::read(&lock_file)
            && !info.is_stale()
        {
            return Err(LiveError::Locked {
                key,
                pid: info.pid,
                host: info.host,
            });
        }
        let db = db_path(&dir, &key);
        if db.exists() {
            if needs_recovery(&dir, &key, options) {
                return Err(LiveError::NeedsRecovery(key));
            }
            if let Some(old) = crypto::read_key_id(&key_id_path(&dir, &key)) {
                crypto::forget(&old);
            }
            for f in companions(&dir, &key) {
                let _ = fs::remove_file(f);
            }
        }
        let key_id = format!("{key}:{}", uuid::Uuid::new_v4());
        let store_key = crypto::new_key(&key_id, &options.encryption, options.require_encryption)?;
        if store_key.source == KeySource::OsStore {
            fs::write(key_id_path(&dir, &key), &key_id)?;
        }
        let now = lock::now_secs();
        let lock = LockInfo {
            pid: std::process::id(),
            host: lock::hostname(),
            started_at: now,
            heartbeat_at: now,
            project_path: Some(project.path().to_owned()),
        };
        lock::write(&lock_file, &lock)?;

        let mut conn = store_key.open(&db)?;
        restrict_permissions(&db);
        schema::configure(&conn, options.synchronous, store_key.encrypted())?;
        conn.execute_batch(schema::DDL)?;
        let head = doc.version().counter as i64;
        let saved = if project.is_dirty(doc) || !project.path().exists() {
            -1
        } else {
            head
        };
        let tx = conn.transaction()?;
        for (k, v) in [
            ("schema_version", schema::SCHEMA_VERSION.to_owned()),
            ("app_version", env!("CARGO_PKG_VERSION").to_owned()),
            (
                "project_path",
                project.path().to_string_lossy().into_owned(),
            ),
            ("ifc_schema", project.ifc().schema()),
            ("data_id", project.data_id().unwrap_or_default().to_owned()),
            (
                "base_ifc_sha256",
                file_sha256(project.path()).unwrap_or_default(),
            ),
            ("saved_seq", saved.to_string()),
            ("head_seq", head.to_string()),
            ("clean_shutdown", "0".to_owned()),
            ("session_id", uuid::Uuid::new_v4().to_string()),
            ("opened_at", time::iso8601(std::time::SystemTime::now())),
            ("mode", "local".to_owned()),
            ("next_id", doc.next_id().0.to_string()),
        ] {
            schema::set_meta(&tx, k, &v)?;
        }
        writer::write_ifc(
            &tx,
            &project.ifc().header_text()?,
            &project.ifc().step_rows()?,
            head,
        )?;
        let all = StateDelta {
            upserts: doc.store().iter().map(element_row).collect(),
            deletes: Vec::new(),
        };
        writer::apply_delta(&tx, &all, head)?;
        // Existing history (after recovery) gets journal rows, so later undos line up with them.
        let mut seq = 0;
        for item in doc.undo_history().collect::<Vec<_>>().into_iter().rev() {
            seq += 1;
            writer::insert_txn(&tx, &txn_row(seq, item))?;
        }
        for item in doc.redo_history() {
            seq += 1;
            writer::insert_txn(&tx, &txn_row(seq, item))?;
            tx.execute("UPDATE txn SET undone = 1 WHERE seq = ?1", [seq])?;
        }
        tx.commit()?;

        let (sender, rx) = mpsc::sync_channel(64);
        let durable = Arc::new(Mutex::new(Durable {
            head,
            saved,
            last_commit_at: None,
        }));
        let pending = Arc::new(AtomicUsize::new(0));
        let writer = Writer {
            durable: durable.clone(),
            pending: pending.clone(),
            conn,
            rx,
            window: options.group_commit,
            lock_path: lock_file,
            lock,
            head,
        };
        let handle = std::thread::Builder::new()
            .name(format!("live-{key}"))
            .spawn(move || writer.run())?;
        doc.set_observer(observer(
            sender.clone(),
            pending.clone(),
            seq,
            doc.next_id().0 as i64,
        ));
        Ok(Self {
            key,
            dir,
            tx: sender,
            writer: Some(handle),
            durable,
            pending,
            store_key,
            required: options.require_encryption,
        })
    }

    /// `live.encryption.status` (DM-LIVE-016).
    pub fn encryption_status(&self) -> EncryptionStatus {
        EncryptionStatus {
            enabled: self.store_key.source != KeySource::None,
            key_source: self.store_key.source,
            required: self.required,
            note: self.store_key.note.clone(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn path(&self) -> PathBuf {
        db_path(&self.dir, &self.key)
    }

    pub fn status(&self) -> LiveStatus {
        let d = self.durable.lock().unwrap().clone();
        let path = self.path();
        let size = companions(&self.dir, &self.key)[..2]
            .iter()
            .filter_map(|f| fs::metadata(f).ok())
            .map(|m| m.len())
            .sum();
        LiveStatus {
            path,
            head_seq: d.head,
            saved_seq: d.saved,
            pending: self.pending.load(Ordering::SeqCst),
            last_commit_at: d.last_commit_at,
            size,
        }
    }

    /// Journal entries, oldest first; `origin` matches a source (`ui`, `mcp`, …) or a full label
    /// (`mcp:agent`) (DM-LIVE-006).
    pub fn history(&self, since_seq: Option<i64>, origin: Option<&str>) -> Result<Vec<TxnSummary>> {
        let rows = journal(&self.path(), &self.store_key, since_seq.unwrap_or(0))?;
        Ok(rows
            .into_iter()
            .filter(|t| origin.is_none_or(|o| origin_matches(&t.origin, o)))
            .collect())
    }

    /// Forces the pending group commit and returns the head sequence once it is durable (DM-LIVE-003).
    pub fn flush(&self) -> Result<i64> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Msg::Flush(reply))
            .map_err(|_| LiveError::WriterStopped)?;
        rx.recv().map_err(|_| LiveError::WriterStopped)
    }

    /// Records that `project` was saved from `doc`: the materialised IFC is refreshed and
    /// `saved_seq` set (DM-LIVE-009).
    pub fn saved(&self, project: &LocalProject, doc: &Document) -> Result<()> {
        let meta = vec![
            (
                "project_path".to_owned(),
                project.path().to_string_lossy().into_owned(),
            ),
            (
                "data_id".to_owned(),
                project.data_id().unwrap_or_default().to_owned(),
            ),
            (
                "base_ifc_sha256".to_owned(),
                file_sha256(project.path()).unwrap_or_default(),
            ),
            ("ifc_schema".to_owned(), project.ifc().schema()),
        ];
        let msg = Msg::Saved {
            saved_seq: doc.version().counter as i64,
            meta,
            header: project.ifc().header_text()?,
            rows: project.ifc().step_rows()?,
        };
        self.pending.fetch_add(1, Ordering::SeqCst);
        self.tx.send(msg).map_err(|_| LiveError::WriterStopped)?;
        self.flush().map(|_| ())
    }

    /// Clean close: detaches from `doc`, writes everything pending and marks the store clean.
    pub fn close(mut self, doc: &mut Document) -> Result<()> {
        doc.clear_observer();
        self.tx
            .send(Msg::Close)
            .map_err(|_| LiveError::WriterStopped)?;
        match self.writer.take().map(JoinHandle::join) {
            Some(Ok(result)) => Ok(result?),
            _ => Err(LiveError::WriterStopped),
        }
    }
}

fn element_row(e: &Element) -> ElementRow {
    ElementRow {
        id: e.id().0 as i64,
        guid: e.guid().to_string(),
        class: e.class.clone(),
        json: serde_json::to_string(e).expect("elements serialise"),
    }
}

fn delta(changes: &ChangeSet) -> StateDelta {
    let mut d = StateDelta::default();
    for (id, c) in changes.iter() {
        match &c.after {
            Some(e) => d.upserts.push(element_row(e)),
            None => d.deletes.push(id.0 as i64),
        }
    }
    d
}

fn compress_records(records: &[ChangeRecord]) -> Vec<u8> {
    let json = serde_json::to_vec(records).expect("records serialise");
    zstd::encode_all(json.as_slice(), 3).expect("in-memory compression cannot fail")
}

fn txn_row(seq: i64, item: &UndoItem) -> TxnRow {
    let forward = item.changes().to_records();
    let inverse: Vec<_> = forward
        .iter()
        .map(|r| ChangeRecord {
            id: r.id,
            before: r.after.clone(),
            after: r.before.clone(),
        })
        .collect();
    TxnRow {
        seq,
        at: time::iso8601(item.time),
        origin: serde_json::to_string(&item.origin).expect("origins serialise"),
        name: item.name.clone(),
        forward: compress_records(&forward),
        inverse: compress_records(&inverse),
    }
}

/// Turns document events into writer messages. Serialisation happens here, on the committing
/// thread; a full channel blocks the commit briefly (back-pressure, §4).
fn observer(
    tx: SyncSender<Msg>,
    pending: Arc<AtomicUsize>,
    mut seq: i64,
    mut next_id: i64,
) -> impl FnMut(&DocEvent) + Send + 'static {
    move |event| {
        let msg = match event {
            DocEvent::Committed { version, item } => {
                seq += 1;
                let max_id = item
                    .changes()
                    .iter()
                    .map(|(id, _)| id.0 as i64 + 1)
                    .max()
                    .unwrap_or(0);
                next_id = next_id.max(max_id);
                Msg::Commit {
                    txn: txn_row(seq, item),
                    delta: delta(item.changes()),
                    head: version.counter as i64,
                    next_id,
                }
            }
            DocEvent::Undone {
                version,
                steps,
                changes,
            } => Msg::Undo {
                steps: *steps,
                delta: delta(changes),
                head: version.counter as i64,
            },
            DocEvent::Redone {
                version,
                steps,
                changes,
            } => Msg::Redo {
                steps: *steps,
                delta: delta(changes),
                head: version.counter as i64,
            },
            DocEvent::Assimilated { count, item } => Msg::Assimilate {
                count: *count,
                merged: item.as_ref().map(|i| {
                    seq += 1;
                    txn_row(seq, i)
                }),
            },
            DocEvent::RolledBack {
                version,
                count,
                changes,
            } => Msg::Rollback {
                count: *count,
                delta: delta(changes),
                head: version.counter as i64,
            },
        };
        // If the writer has died the store simply stops updating; the project itself is unaffected.
        pending.fetch_add(1, Ordering::SeqCst);
        if tx.send(msg).is_err() {
            pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn journal(db: &Path, key: &crypto::StoreKey, since_seq: i64) -> Result<Vec<TxnSummary>> {
    let conn = key.open(db)?;
    let mut stmt =
        conn.prepare("SELECT seq, at, origin, name, undone FROM txn WHERE seq > ?1 ORDER BY seq")?;
    let rows = stmt.query_map([since_seq], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (seq, at, origin, name, undone) = row?;
        let origin =
            serde_json::from_str(&origin).map_err(|e| LiveError::Journal(e.to_string()))?;
        Ok(TxnSummary {
            seq,
            at,
            origin,
            name,
            undone: undone != 0,
        })
    })
    .collect()
}

fn origin_matches(origin: &Origin, filter: &str) -> bool {
    let label = origin.label();
    label == filter
        || serde_json::to_value(origin.source)
            .ok()
            .and_then(|v| v.as_str().map(|s| s == filter))
            .unwrap_or(false)
}

/// The transactions in store `key` made since the project was last saved (`recovery.show_changes`).
pub fn show_changes(key: &str, options: &Options) -> Result<Vec<TxnSummary>> {
    let (store_key, _) = existing_key(&options.dir, key, options);
    let db = db_path(&options.dir, key);
    let since = {
        let conn = store_key.open(&db)?;
        schema::get_meta(&conn, "saved_txn_seq")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    journal(&db, &store_key, since)
}

/// A store left behind by a session that did not close cleanly (§6.1, the recovery dialog's rows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Recoverable {
    pub key: String,
    pub project_path: Option<PathBuf>,
    pub last_change_at: Option<String>,
    /// Changes since the last save, if the database can be read.
    pub unsaved_changes: Option<i64>,
    /// The latest undo labels, newest first.
    pub last_labels: Vec<String>,
    /// False when the database is damaged; restore then falls back to base IFC plus replay.
    pub readable: bool,
    /// Why it cannot be read, for example a lost encryption key.
    pub reason: Option<String>,
}

struct Summary {
    clean: bool,
    head: i64,
    saved: i64,
    project_path: Option<PathBuf>,
    last_change_at: Option<String>,
    last_labels: Vec<String>,
}

fn summary(db: &Path, key: &crypto::StoreKey) -> rusqlite::Result<Summary> {
    let conn = key.open(db)?;
    let meta = |k: &str| schema::get_meta(&conn, k);
    let num = |k: &str| -> rusqlite::Result<i64> {
        Ok(meta(k)?.and_then(|v| v.parse().ok()).unwrap_or(0))
    };
    let mut stmt =
        conn.prepare("SELECT name, at FROM txn WHERE undone = 0 ORDER BY seq DESC LIMIT 3")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Summary {
        clean: meta("clean_shutdown")?.as_deref() == Some("1"),
        head: num("head_seq")?,
        saved: num("saved_seq")?,
        project_path: meta("project_path")?
            .filter(|p| !p.is_empty())
            .map(PathBuf::from),
        last_change_at: rows.first().map(|(_, at)| at.clone()),
        last_labels: rows.into_iter().map(|(n, _)| n).collect(),
    })
}

/// A store needs recovery when its session ended without a clean close (the lock file remains and
/// is stale) and it holds unsaved work. Unreadable stores are offered too.
fn needs_recovery(dir: &Path, key: &str, options: &Options) -> bool {
    match LockInfo::read(&lock_path(dir, key)) {
        None => return false,
        Some(l) if !l.is_stale() => return false,
        Some(_) => {}
    }
    let (store_key, _) = existing_key(dir, key, options);
    match summary(&db_path(dir, key), &store_key) {
        Ok(s) => !s.clean && (s.head > s.saved || s.saved < 0),
        Err(_) => true,
    }
}

/// Stores in `dir` that need recovery. Stores held by a running process are skipped.
pub fn scan(options: &Options) -> Result<Vec<Recoverable>> {
    let dir = &options.dir;
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rvlive") {
            continue;
        }
        let key = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if !needs_recovery(dir, &key, options) {
            continue;
        }
        let lock_project = LockInfo::read(&lock_path(dir, &key)).and_then(|l| l.project_path);
        let (store_key, lost) = existing_key(dir, &key, options);
        out.push(match summary(&path, &store_key) {
            Ok(s) => Recoverable {
                key,
                project_path: s.project_path.or(lock_project),
                last_change_at: s.last_change_at,
                unsaved_changes: Some(if s.saved < 0 {
                    s.head
                } else {
                    s.head - s.saved
                }),
                last_labels: s.last_labels,
                readable: true,
                reason: None,
            },
            Err(_) => Recoverable {
                key,
                project_path: lock_project,
                last_change_at: None,
                unsaved_changes: None,
                last_labels: Vec::new(),
                readable: false,
                reason: Some(if lost {
                    "the recovery data is encrypted and its key is not available".into()
                } else {
                    "the recovery data is damaged".into()
                }),
            },
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RestoreSource {
    /// Loaded from the materialised tables; no replay needed.
    Materialised,
    /// The database was damaged: the base IFC file plus the journal rows that could be read.
    BaseIfcReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RestoreReport {
    pub source: RestoreSource,
    /// Journal entries recovered: undo and redo items, or replayed transactions.
    pub transactions: usize,
    /// Label and time of the last transaction recovered.
    pub last: Option<(String, String)>,
    pub problems: Vec<String>,
}

#[derive(Debug)]
pub struct Restored {
    pub project: LocalProject,
    pub doc: Document,
    /// The new session store; the old one is in the recovery bin.
    pub store: LiveStore,
    pub report: RestoreReport,
}

/// Restores the project in store `key` (§6.3). With `as_copy`, the project is bound to that path.
pub fn restore(key: &str, as_copy: Option<&Path>, options: &Options) -> Result<Restored> {
    let dir = &options.dir;
    let lock_info = LockInfo::read(&lock_path(dir, key));
    if let Some(l) = &lock_info
        && !l.is_stale()
    {
        return Err(LiveError::Locked {
            key: key.into(),
            pid: l.pid,
            host: l.host.clone(),
        });
    }
    let (store_key, lost) = existing_key(dir, key, options);
    if lost {
        // Without the key nothing in the store can be read; the saved IFC file is unaffected.
        return Err(KeyError::Unavailable(
            "the credential store has no entry for this store; discard it".into(),
        )
        .into());
    }
    let (mut project, mut doc, report) = match restore_materialised(dir, key, options, &store_key) {
        Ok(r) => r,
        Err(first) => {
            let base = lock_info.and_then(|l| l.project_path);
            let mut r = restore_replay(dir, key, base, &store_key)?;
            r.2.problems.insert(
                0,
                format!("the live store could not be used directly: {first}"),
            );
            r
        }
    };
    discard(key, dir)?;
    if let Some(path) = as_copy {
        project.set_path(path);
    }
    let store = LiveStore::create(&project, &mut doc, options)?;
    Ok(Restored {
        project,
        doc,
        store,
        report,
    })
}

fn restore_materialised(
    dir: &Path,
    key: &str,
    options: &Options,
    store_key: &crypto::StoreKey,
) -> Result<(LocalProject, Document, RestoreReport)> {
    let conn = store_key.open(&db_path(dir, key))?;
    let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(LiveError::Journal(check));
    }
    let meta: HashMap<String, String> = conn
        .prepare("SELECT key, value FROM meta")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let project_path = PathBuf::from(meta.get("project_path").cloned().unwrap_or_default());
    let header = meta
        .get("ifc_header")
        .cloned()
        .ok_or_else(|| LiveError::Journal("no IFC header".into()))?;
    let rows: Vec<StepRow> = conn
        .prepare("SELECT sid, ifc_type, global_id, step FROM ifc_entity ORDER BY sid")?
        .query_map([], |r| {
            Ok(StepRow {
                id: reviera_ifc::StepId(r.get(0)?),
                class: r.get(1)?,
                global_id: r.get(2)?,
                attributes: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut elements = Vec::new();
    let mut max_id = 0;
    for json in conn
        .prepare("SELECT json FROM element ORDER BY id")?
        .query_map([], |r| r.get::<_, String>(0))?
    {
        let e: Element =
            serde_json::from_str(&json?).map_err(|e| LiveError::Journal(e.to_string()))?;
        max_id = max_id.max(e.id().0 + 1);
        elements.push((e.id(), new_element(e)));
    }
    let next_id = meta
        .get("next_id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0u64)
        .max(max_id);
    let mut doc = Document::from_loaded(elements, next_id)?;

    let mut undo = Vec::new();
    let mut redo = Vec::new();
    let mut last = None;
    let mut stmt =
        conn.prepare("SELECT at, origin, name, forward, undone FROM txn ORDER BY seq")?;
    let mut q = stmt.query([])?;
    while let Some(r) = q.next()? {
        let (at, origin, name, forward, undone): (String, String, String, Vec<u8>, i64) =
            (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
        let item = undo_item(&at, &origin, &name, &forward)?;
        if undone == 0 {
            last = Some((name, at));
            undo.push(item);
        } else {
            redo.push(item);
        }
    }
    let transactions = undo.len() + redo.len();
    let keep = undo.len().saturating_sub(options.undo_depth);
    doc.restore_history(undo.split_off(keep), redo);

    let base = dir.join(format!(".{key}.base.ifc"));
    fs::write(&base, reviera_ifc::spf_text(&header, &rows))?;
    let restored = LocalProject::restore(&base, &project_path, &doc);
    let _ = fs::remove_file(&base);
    let (project, open_report) = restored?;
    let report = RestoreReport {
        source: RestoreSource::Materialised,
        transactions,
        last,
        problems: open_report.problems,
    };
    Ok((project, doc, report))
}

/// Fallback: the project file plus every journal row that can still be read, replayed as state.
/// Rows hold full before/after element states, so replaying rows that the file already contains is
/// harmless.
fn restore_replay(
    dir: &Path,
    key: &str,
    base: Option<PathBuf>,
    store_key: &crypto::StoreKey,
) -> Result<(LocalProject, Document, RestoreReport)> {
    let unrecoverable = |reason: String| LiveError::Unrecoverable {
        key: key.into(),
        reason,
    };
    let base = base
        .filter(|p| p.exists())
        .ok_or_else(|| unrecoverable("the project file is not available".into()))?;
    let mut problems = Vec::new();
    let mut rows = Vec::new();
    match read_journal(&db_path(dir, key), store_key, &mut rows) {
        Ok(()) => {}
        Err(e) => problems.push(format!("journal read stopped: {e}")),
    }
    if let Ok(conn) = store_key.open(&db_path(dir, key))
        && let Ok(Some(expected)) = schema::get_meta(&conn, "base_ifc_sha256")
        && !expected.is_empty()
        && file_sha256(&base).as_deref() != Some(expected.as_str())
    {
        return Err(unrecoverable(
            "the project file changed since the session started".into(),
        ));
    }

    let (project, mut doc, _) = LocalProject::open(&base)?;
    let mut last = None;
    let mut replayed = 0;
    for (at, origin, name, forward) in rows {
        let Ok(item) = undo_item(&at, &origin, &name, &forward) else {
            problems.push(format!(
                "journal entry \"{name}\" at {at} is unreadable; replay stopped"
            ));
            break;
        };
        let mut txn = doc.transaction(name.clone(), item.origin.clone());
        for (id, change) in item.changes().iter() {
            match &change.after {
                Some(e) => txn.put_element(Element::clone(e))?,
                None if txn.store().contains(id) => txn.delete(id)?,
                None => {}
            }
        }
        txn.commit();
        replayed += 1;
        last = Some((name, at));
    }
    let report = RestoreReport {
        source: RestoreSource::BaseIfcReplay,
        transactions: replayed,
        last,
        problems,
    };
    Ok((project, doc, report))
}

fn read_journal(
    db: &Path,
    key: &crypto::StoreKey,
    out: &mut Vec<(String, String, String, Vec<u8>)>,
) -> rusqlite::Result<()> {
    let conn = key.open(db)?;
    let mut stmt =
        conn.prepare("SELECT at, origin, name, forward FROM txn WHERE undone = 0 ORDER BY seq")?;
    let mut q = stmt.query([])?;
    while let Some(r) = q.next()? {
        out.push((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?));
    }
    Ok(())
}

fn undo_item(at: &str, origin: &str, name: &str, forward: &[u8]) -> Result<UndoItem> {
    let bad = |e: String| LiveError::Journal(format!("\"{name}\": {e}"));
    let origin: Origin = serde_json::from_str(origin).map_err(|e| bad(e.to_string()))?;
    let json = zstd::decode_all(forward).map_err(|e| bad(e.to_string()))?;
    let records: Vec<ChangeRecord> =
        serde_json::from_slice(&json).map_err(|e| bad(e.to_string()))?;
    let time = time::parse_iso8601(at).ok_or_else(|| bad(format!("bad time {at}")))?;
    Ok(UndoItem::new(
        name,
        origin,
        time,
        ChangeSet::from_records(records)?,
    ))
}

fn new_element(e: Element) -> NewElement {
    let guid = e.guid().clone();
    NewElement {
        class: e.class,
        name: e.name,
        params: e.params,
        guid: Some(guid),
    }
}

/// Moves a store to the recovery bin (`<dir>/bin`, kept 7 days) (§6.2 Discard, §6.3 step 6).
pub fn discard(key: &str, dir: &Path) -> Result<()> {
    let bin = dir.join("bin");
    create_private_dir(&bin)?;
    let stamp = lock::now_secs();
    for f in companions(dir, key) {
        if f.exists() {
            let name = format!(
                "{stamp}-{}",
                f.file_name().unwrap_or_default().to_string_lossy()
            );
            fs::rename(&f, bin.join(name))?;
        }
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(hex::encode(Sha256::digest(&bytes)))
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The store inherits the user's permissions only: 0600 on POSIX (§10).
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}
