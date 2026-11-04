// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Component Libraries hosted on FileEngine (COL-FE-013): a FileEngine folder with a
//! `riviera-library.json` is mirrored into the per-user cache, commands run on the mirror, and
//! what a writing command changed is sent back, the manifest merged entry by entry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use riviera_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The mirror's record of where it came from and what it was synced at.
pub const SYNC: &str = ".riviera-sync.json";
const MANIFEST: &str = riviera_core::library::MANIFEST;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Synced {
    pub uid: String,
    pub version: String,
    pub sha256: String,
    /// Not downloaded yet: an empty placeholder stands in for it (a large item, fetched when a
    /// command needs it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

/// Files larger than this are fetched when a command needs them, not when the library is read.
pub const ON_DEMAND: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sync {
    pub connection_id: String,
    pub folder_uid: String,
    /// Files by path (`/`-separated, from the library's root).
    #[serde(default)]
    pub files: BTreeMap<String, Synced>,
    /// Folders by path.
    #[serde(default)]
    pub folders: BTreeMap<String, String>,
    /// The manifest as last synced: the base of a merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Value>,
}

/// What sending a mirror back did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Pushed {
    pub uploaded: Vec<String>,
    pub deleted: Vec<String>,
    /// The manifest was merged with someone else's changes.
    pub merged: bool,
    /// Entries both changed differently (this command's change kept).
    pub conflicts: Vec<String>,
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect()
}

fn io(e: impl std::fmt::Display) -> Error {
    bad("fileengine.cache", e.to_string())
}

/// What marks a file's version in a folder listing (which gives no version stamp): its version
/// count, modification time and size.
fn token(n: &riviera_fileengine::Node) -> String {
    format!("{}#{}#{}", n.version_count, n.modified_at, n.size)
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The mirror folder of a FileEngine library.
pub fn mirror_dir(app: &App, conn: &str, folder_uid: &str) -> Result<PathBuf> {
    Ok(app.fileengine.cache_root()?.join("libraries").join(safe(conn)).join(safe(folder_uid)))
}

/// The record of a mirror, if `root` is one.
pub fn origin(root: &Path) -> Option<Sync> {
    serde_json::from_str(&std::fs::read_to_string(root.join(SYNC)).ok()?).ok()
}

fn save(root: &Path, s: &Sync) -> Result<()> {
    std::fs::write(root.join(SYNC), serde_json::to_string_pretty(s).expect("serialises")).map_err(io)
}

fn manifest_value(root: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(root.join(MANIFEST)).ok()?).ok()
}

/// Brings the mirror at `root` up to date with its FileEngine folder.
pub fn pull(app: &mut App, root: &Path, conn: &str, folder_uid: &str) -> Result<()> {
    std::fs::create_dir_all(root).map_err(io)?;
    let mut s = origin(root).unwrap_or_default();
    s.connection_id = conn.to_owned();
    s.folder_uid = folder_uid.to_owned();
    // The server's tree: (path, node), folders first.
    let mut files = Vec::new();
    let mut folders = BTreeMap::new();
    let mut todo = vec![(String::new(), folder_uid.to_owned())];
    while let Some((at, uid)) = todo.pop() {
        for n in app.fileengine.with(conn, |c| c.list(&uid, false))? {
            if n.deleted {
                continue;
            }
            let path = if at.is_empty() { n.name.clone() } else { format!("{at}/{}", n.name) };
            if n.is_dir() {
                folders.insert(path.clone(), n.uid.clone());
                todo.push((path, n.uid));
            } else {
                files.push((path, n));
            }
        }
    }
    for f in folders.keys() {
        std::fs::create_dir_all(root.join(f)).map_err(io)?;
    }
    let mut synced = BTreeMap::new();
    for (path, n) in files {
        let local = root.join(&path);
        let had = s.files.get(&path).filter(|x| x.uid == n.uid && x.version == token(&n) && local.is_file());
        let entry = match had {
            Some(x) => x.clone(),
            // A large item waits, as an empty placeholder, until a command needs it.
            None if n.size > ON_DEMAND && path != MANIFEST => {
                std::fs::write(&local, b"").map_err(io)?;
                Synced { uid: n.uid.clone(), version: token(&n), sha256: String::new(), pending: true }
            }
            None => {
                let bytes = app.fileengine.with(conn, |c| c.read(&n.uid))?;
                std::fs::write(&local, &bytes).map_err(io)?;
                Synced { uid: n.uid.clone(), version: token(&n), sha256: sha(&bytes), pending: false }
            }
        };
        synced.insert(path, entry);
    }
    // Files removed on the server.
    for path in s.files.keys().filter(|p| !synced.contains_key(*p)) {
        let _ = std::fs::remove_file(root.join(path));
    }
    s.files = synced;
    s.folders = folders;
    s.manifest = manifest_value(root);
    save(root, &s)
}

/// Downloads the items at `paths` (from the library's root) that are still placeholders, with
/// the other placeholders of their folders (an OBJ's MTL and textures, a glTF's buffers).
pub fn fetch(app: &mut App, root: &Path, paths: &[String]) -> Result<usize> {
    let Some(mut s) = origin(root) else { return Ok(0) };
    let folder = |p: &str| p.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_owned());
    let wanted: std::collections::BTreeSet<String> = paths.iter().map(|p| folder(p)).collect();
    let due: Vec<String> = s.files.iter().filter(|(p, x)| x.pending && wanted.contains(&folder(p))).map(|(p, _)| p.clone()).collect();
    let conn = s.connection_id.clone();
    for p in &due {
        let uid = s.files[p].uid.clone();
        let bytes = app.fileengine.with(&conn, |c| c.read(&uid))?;
        std::fs::write(root.join(p), &bytes).map_err(io)?;
        let x = s.files.get_mut(p).expect("listed");
        x.sha256 = sha(&bytes);
        x.pending = false;
    }
    if !due.is_empty() {
        save(root, &s)?;
    }
    Ok(due.len())
}

/// Every file in the mirror, by path (the record left out).
fn local_files(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if rel != SYNC && !rel.starts_with(&format!(".{MANIFEST}.tmp")) {
                    out.insert(rel, p);
                }
            }
        }
    }
    out
}

/// The uid of the FileEngine folder for a path's parent, made as needed.
fn folder_for(app: &mut App, s: &mut Sync, path: &str) -> Result<String> {
    let mut uid = s.folder_uid.clone();
    let mut at = String::new();
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        at = if at.is_empty() { (*part).to_owned() } else { format!("{at}/{part}") };
        uid = match s.folders.get(&at) {
            Some(u) => u.clone(),
            None => {
                let parent = uid.clone();
                let made = app.fileengine.with(&s.connection_id, |c| c.ensure_dir(&parent, part))?;
                s.folders.insert(at.clone(), made.clone());
                made
            }
        };
    }
    Ok(uid)
}

fn upload(app: &mut App, s: &mut Sync, path: &str, local: &Path) -> Result<Synced> {
    let bytes = std::fs::read(local).map_err(io)?;
    let folder = folder_for(app, s, path)?;
    let name = path.rsplit('/').next().unwrap_or(path).to_owned();
    let known = s.files.get(path).map(|x| x.uid.clone());
    let conn = s.connection_id.clone();
    app.fileengine.with(&conn, |c| {
        let uid = match known {
            Some(u) => u,
            None => match c.find(&folder, &name)? {
                Some(n) => n.uid,
                None => c.create_file(&folder, &name)?,
            },
        };
        c.write_file(&uid, local)?;
        let version = token(&c.stat(&uid)?);
        Ok(Synced { uid, version, sha256: sha(&bytes), pending: false })
    })
}

/// Merges `mine` into `theirs` against `base`: what changed from `base` in `mine` is taken, the
/// rest is `theirs`; `items` and `folders` entry by entry. Returns the merge and the entries both
/// changed differently.
pub fn merge(base: &Value, mine: &Value, theirs: &Value) -> (Value, Vec<String>) {
    let mut out = theirs.clone();
    let mut conflicts = Vec::new();
    let empty = serde_json::Map::new();
    let obj = |v: &Value| v.as_object().cloned().unwrap_or_default();
    let (b, m) = (obj(base), obj(mine));
    let Some(o) = out.as_object_mut() else { return (mine.clone(), conflicts) };
    let keys: std::collections::BTreeSet<String> = b.keys().chain(m.keys()).cloned().collect();
    for k in keys {
        if k == "items" || k == "folders" {
            let bm = b.get(&k).and_then(Value::as_object).unwrap_or(&empty);
            let mm = m.get(&k).and_then(Value::as_object).unwrap_or(&empty);
            let tm = o.entry(k.clone()).or_insert_with(|| Value::Object(serde_json::Map::new()));
            let Some(tm) = tm.as_object_mut() else { continue };
            let entries: std::collections::BTreeSet<String> = bm.keys().chain(mm.keys()).cloned().collect();
            for e in entries {
                let (bv, mv) = (bm.get(&e), mm.get(&e));
                if bv == mv {
                    continue;
                }
                let tv = tm.get(&e);
                if tv != bv && tv != mv {
                    conflicts.push(format!("{k}/{e}"));
                }
                match mv {
                    Some(v) => {
                        tm.insert(e, v.clone());
                    }
                    None => {
                        tm.remove(&e);
                    }
                }
            }
        } else {
            let (bv, mv) = (b.get(&k), m.get(&k));
            if bv == mv {
                continue;
            }
            if o.get(&k) != bv && o.get(&k) != mv {
                conflicts.push(k.clone());
            }
            match mv {
                Some(v) => {
                    o.insert(k, v.clone());
                }
                None => {
                    o.remove(&k);
                }
            }
        }
    }
    (out, conflicts)
}

/// Sends what changed in the mirror at `root` back to its FileEngine folder.
pub fn push(app: &mut App, root: &Path) -> Result<Pushed> {
    let mut s = origin(root).ok_or_else(|| bad("fileengine.not_a_mirror", format!("{} is not a FileEngine library", root.display())))?;
    let mut out = Pushed::default();
    let locals = local_files(root);
    for (path, local) in &locals {
        if path == MANIFEST {
            continue;
        }
        // A placeholder is never sent.
        if s.files.get(path).is_some_and(|x| x.pending) {
            continue;
        }
        let bytes = std::fs::read(local).map_err(io)?;
        if s.files.get(path).is_some_and(|x| x.sha256 == sha(&bytes)) {
            continue;
        }
        let synced = upload(app, &mut s, path, local)?;
        s.files.insert(path.clone(), synced);
        out.uploaded.push(path.clone());
    }
    let gone: Vec<String> = s.files.keys().filter(|p| p.as_str() != MANIFEST && !locals.contains_key(*p)).cloned().collect();
    for path in gone {
        if let Some(x) = s.files.remove(&path) {
            let conn = s.connection_id.clone();
            app.fileengine.with(&conn, |c| c.delete_file(&x.uid))?;
            out.deleted.push(path);
        }
    }
    // The manifest: uploaded as it is when nobody changed it on the server, else merged.
    if let Some(mine) = manifest_value(root)
        && s.manifest.as_ref() != Some(&mine)
    {
        let conn = s.connection_id.clone();
        let server = match s.files.get(MANIFEST) {
            Some(x) => Some((x.clone(), app.fileengine.with(&conn, |c| c.stat(&x.uid))?)),
            None => None,
        };
        let final_ = match server {
            Some((x, n)) if token(&n) != x.version => {
                let theirs: Value = serde_json::from_slice(&app.fileengine.with(&conn, |c| c.read(&x.uid))?).map_err(|e| bad("fileengine.bad_manifest", e.to_string()))?;
                let (merged, conflicts) = merge(s.manifest.as_ref().unwrap_or(&Value::Null), &mine, &theirs);
                out.merged = true;
                out.conflicts = conflicts;
                std::fs::write(root.join(MANIFEST), serde_json::to_string_pretty(&merged).expect("serialises")).map_err(io)?;
                merged
            }
            _ => mine,
        };
        let synced = upload(app, &mut s, MANIFEST, &root.join(MANIFEST))?;
        s.files.insert(MANIFEST.into(), synced);
        out.uploaded.push(MANIFEST.into());
        s.manifest = Some(final_);
    }
    save(root, &s)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn manifests_merge_entry_by_entry() {
        let base = json!({ "name": "Lib", "items": { "a.obj": { "name": "A" } } });
        let mine = json!({ "name": "Lib", "items": { "a.obj": { "name": "A" }, "b.obj": { "name": "B" } } });
        let theirs = json!({ "name": "Lib", "items": { "a.obj": { "name": "A" }, "c.obj": { "name": "C" } } });
        let (m, conflicts) = super::merge(&base, &mine, &theirs);
        assert_eq!(m["items"].as_object().unwrap().len(), 3);
        assert!(conflicts.is_empty());
        // Both changed the same entry: mine kept, reported.
        let mine = json!({ "name": "Lib", "items": { "a.obj": { "name": "Mine" } } });
        let theirs = json!({ "name": "Lib", "items": { "a.obj": { "name": "Theirs" } } });
        let (m, conflicts) = super::merge(&base, &mine, &theirs);
        assert_eq!((m["items"]["a.obj"]["name"].as_str(), conflicts), (Some("Mine"), vec!["items/a.obj".to_owned()]));
        // A deletion of mine; their rename of the library kept.
        let mine = json!({ "name": "Lib", "items": {} });
        let theirs = json!({ "name": "Shared", "items": { "a.obj": { "name": "A" } } });
        let (m, _) = super::merge(&base, &mine, &theirs);
        assert_eq!((m["name"].as_str(), m["items"].as_object().unwrap().len()), (Some("Shared"), 0));
    }
}
