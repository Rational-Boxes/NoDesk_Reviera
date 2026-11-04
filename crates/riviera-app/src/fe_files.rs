// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Single files kept in step with FileEngine (COL-FE-014): a file opened from FileEngine that
//! Riviera keeps open and saves again (a parameter library) has a stable copy in the per-user
//! cache with a sidecar naming its connection, uid, version and SHA-256. Opening brings the copy
//! up to date (unless it holds changes not yet sent); saving sends it back as a new version, but
//! only over the version it was opened at.

use std::path::{Path, PathBuf};

use riviera_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn io(e: impl std::fmt::Display) -> Error {
    bad("fileengine.cache", e.to_string())
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect()
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A synced file's record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSync {
    pub connection_id: String,
    pub uid: String,
    pub version: String,
    pub sha256: String,
}

fn sidecar(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.riviera-sync.json"))
}

/// The record of a synced file, if `path` is one.
pub fn record(path: &Path) -> Option<FileSync> {
    serde_json::from_str(&std::fs::read_to_string(sidecar(path)).ok()?).ok()
}

fn keep(path: &Path, r: &FileSync) -> Result<()> {
    std::fs::write(sidecar(path), serde_json::to_string_pretty(r).expect("serialises")).map_err(io)?;
    // The content as synced: the base of a merge.
    std::fs::copy(path, base_path(path)).map_err(io)?;
    Ok(())
}

fn base_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.riviera-base"))
}

/// The stable local copy of `name` in FileEngine folder `folder_uid`.
pub fn local_path(app: &App, conn: &str, folder_uid: &str, name: &str) -> Result<PathBuf> {
    Ok(app.fileengine.cache_root()?.join("synced").join(safe(conn)).join(safe(folder_uid)).join(name))
}

/// Brings the local copy of FileEngine file `uid` up to date and returns its path; a copy with
/// changes not yet sent is kept as it is.
pub fn pull(app: &mut App, conn: &str, uid: &str) -> Result<PathBuf> {
    let node = app.fileengine.with(conn, |c| c.stat(uid))?;
    if node.is_dir() {
        return Err(bad("fileengine.not_a_file", format!("{} is a folder", node.name)));
    }
    let path = local_path(app, conn, &node.parent_uid, &node.name)?;
    let had = record(&path).filter(|r| r.uid == uid && path.is_file());
    let unsent = had.as_ref().is_some_and(|r| std::fs::read(&path).map(|b| sha(&b) != r.sha256).unwrap_or(false));
    let current = had.as_ref().is_some_and(|r| r.version == node.version);
    if !unsent && !current {
        let bytes = app.fileengine.with(conn, |c| c.read(uid))?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(io)?;
        }
        std::fs::write(&path, &bytes).map_err(io)?;
        keep(&path, &FileSync { connection_id: conn.to_owned(), uid: uid.to_owned(), version: node.version, sha256: sha(&bytes) })?;
    }
    Ok(path)
}

/// Records `path` as the local copy of FileEngine file `uid` as it is now on the server (after an
/// upload).
pub fn adopt(app: &mut App, path: &Path, conn: &str, uid: &str) -> Result<()> {
    let version = app.fileengine.with(conn, |c| c.stat(uid))?.version;
    let bytes = std::fs::read(path).map_err(io)?;
    keep(path, &FileSync { connection_id: conn.to_owned(), uid: uid.to_owned(), version, sha256: sha(&bytes) })
}

/// Sends a synced file's changes back as a new version, merging them with someone else's newer
/// version with `merge(base, mine, theirs)` (which returns the merged content and the conflicts);
/// the merge is written here too. Returns whether anything was sent, and the conflicts.
pub fn push_merged(app: &mut App, path: &Path, merge: impl FnOnce(&[u8], &[u8], &[u8]) -> Result<(Vec<u8>, Vec<String>)>) -> Result<(bool, Vec<String>)> {
    match push(app, path) {
        Err(e) if e.code == "fileengine.conflict" => {
            let r = record(path).expect("a synced file");
            let base = std::fs::read(base_path(path)).map_err(io)?;
            let mine = std::fs::read(path).map_err(io)?;
            let theirs = app.fileengine.with(&r.connection_id, |c| c.read(&r.uid))?;
            let (merged, conflicts) = merge(&base, &mine, &theirs)?;
            std::fs::write(path, &merged).map_err(io)?;
            app.fileengine.with(&r.connection_id, |c| c.write_file(&r.uid, path))?;
            adopt(app, path, &r.connection_id, &r.uid)?;
            Ok((true, conflicts))
        }
        other => other.map(|sent| (sent, Vec::new())),
    }
}

/// Sends a synced file's changes back as a new version; `false` when `path` is not a synced file
/// or has no changes. Someone else's newer version is never overwritten: the change stays here.
pub fn push(app: &mut App, path: &Path) -> Result<bool> {
    let Some(r) = record(path) else { return Ok(false) };
    let bytes = std::fs::read(path).map_err(io)?;
    if sha(&bytes) == r.sha256 {
        return Ok(false);
    }
    let now = app.fileengine.with(&r.connection_id, |c| c.stat(&r.uid))?.version;
    if now != r.version {
        return Err(bad("fileengine.conflict", format!("{} was changed on FileEngine since it was opened here; this change is kept in {} (open the file again to take the newer one)", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), path.display())));
    }
    app.fileengine.with(&r.connection_id, |c| c.write_file(&r.uid, path))?;
    adopt(app, path, &r.connection_id, &r.uid)?;
    Ok(true)
}
