// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! FileEngine wherever a file is read or written (COL-FE-012).
//!
//! A command parameter that is a path carries `x-riviera-file` in its schema: `read`, `read_set`
//! (the file and the other files of its folder), `write`, `write_folder` or `write_any`. Where
//! such a parameter holds a `fileengine://` address, the address is resolved here before the
//! command runs: files to read are downloaded into the per-user cache, files and folders to
//! write get a fresh local folder. After the command, what it wrote is uploaded into the
//! FileEngine folder (a new version of a file of the same name there, or a new file), with
//! metadata naming the command and the model it came from. Commands see only local paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use riviera_core::{Caller, Error, Outcome, Result};
use serde::Serialize;
use serde_json::{Value, json};

use crate::App;
use crate::fileengine::{Address, SCHEME, parse_address};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// How a marked parameter is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Read,
    ReadSet,
    Write,
    WriteFolder,
    WriteAny,
}

impl Role {
    fn of(schema: &Value) -> Option<Role> {
        Some(match schema.get("x-riviera-file")?.as_str()? {
            "read" => Role::Read,
            "read_set" => Role::ReadSet,
            "write" => Role::Write,
            "write_folder" => Role::WriteFolder,
            "write_any" => Role::WriteAny,
            _ => return None,
        })
    }
}

/// `s`, following a local `$ref`.
fn deref<'a>(root: &'a Value, s: &'a Value) -> &'a Value {
    match s.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix('#')) {
        Some(p) => root.pointer(p).unwrap_or(s),
        None => s,
    }
}

/// `s` and the schemas it combines (`allOf`, `anyOf`, `oneOf`), each followed.
fn parts<'a>(root: &'a Value, s: &'a Value, out: &mut Vec<&'a Value>) {
    let s = deref(root, s);
    out.push(s);
    for k in ["allOf", "anyOf", "oneOf"] {
        for sub in s.get(k).and_then(Value::as_array).into_iter().flatten() {
            parts(root, sub, out);
        }
    }
}

fn all<'a>(root: &'a Value, s: &'a Value) -> Vec<&'a Value> {
    let mut v = Vec::new();
    parts(root, s, &mut v);
    v
}

/// Every marked parameter in `value` holding a FileEngine address: its JSON pointer, role and
/// address.
pub fn marked(root: &Value, schema: &Value, value: &Value, at: &str, out: &mut Vec<(String, Role, String)>) {
    let schemas = all(root, schema);
    match value {
        Value::String(s) if s.starts_with(SCHEME) => {
            if let Some(r) = schemas.iter().find_map(|s| Role::of(s)) {
                out.push((at.to_owned(), r, s.clone()));
            }
        }
        Value::Array(items) => {
            if let Some(it) = schemas.iter().find_map(|s| s.get("items")) {
                for (i, v) in items.iter().enumerate() {
                    marked(root, it, v, &format!("{at}/{i}"), out);
                }
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                if let Some(p) = schemas.iter().find_map(|s| s.get("properties").and_then(|p| p.get(k))) {
                    marked(root, p, v, &format!("{at}/{}", k.replace('~', "~0").replace('/', "~1")), out);
                }
            }
        }
        _ => {}
    }
}

/// Every marked parameter in a schema, by its path (`a.b`, `a[]` for an array's items), for
/// listing which commands take FileEngine addresses.
pub fn marked_parameters(root: &Value, schema: &Value, at: &str, out: &mut Vec<(String, Role)>, depth: usize) {
    if depth > 8 {
        return;
    }
    let schemas = all(root, schema);
    if let Some(r) = schemas.iter().find_map(|s| Role::of(s)) {
        out.push((at.to_owned(), r));
        return;
    }
    for s in &schemas {
        if let Some(p) = s.get("properties").and_then(Value::as_object) {
            for (k, v) in p {
                let name = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                marked_parameters(root, v, &name, out, depth + 1);
            }
        }
        if let Some(it) = s.get("items") {
            marked_parameters(root, it, &format!("{at}[]"), out, depth + 1);
        }
    }
}

/// What one upload made.
#[derive(Debug, Clone, Serialize)]
pub struct Upload {
    /// The parameter (a JSON pointer into the arguments).
    pub parameter: String,
    pub uid: String,
    pub name: String,
    pub version: String,
    /// A new version of a file already there.
    pub replaced: bool,
}

/// A write still to upload: the local file or folder, and the FileEngine folder.
struct Outgoing {
    parameter: String,
    connection: String,
    folder_uid: String,
    local: PathBuf,
    /// A single file (else every file in `local`).
    file: bool,
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect()
}

fn address(text: &str) -> Result<Address> {
    parse_address(Path::new(text)).unwrap_or_else(|| Err(bad("fileengine.bad_address", format!("{text} is not a FileEngine address"))))
}

/// Downloads `uid` (at `version`, or its latest) into `dir` under its own name, unless that
/// version is there already; returns the local path.
fn download(app: &mut App, conn: &str, uid: &str, version: Option<&str>, dir: &Path) -> Result<(PathBuf, String)> {
    let node = app.fileengine.with(conn, |c| c.stat(uid))?;
    if node.is_dir() {
        return Err(bad("fileengine.not_a_file", format!("{} is a folder", node.name)));
    }
    let local = dir.join(&node.name);
    let stamp = dir.join(format!(".{}.version", safe(&node.name)));
    let want = version.map_or(node.version.clone(), str::to_owned);
    let cached = local.is_file() && std::fs::read_to_string(&stamp).is_ok_and(|v| v == want);
    if !cached {
        let bytes = app.fileengine.with(conn, |c| match version {
            Some(v) => c.read_version(uid, v),
            None => c.read(uid),
        })?;
        std::fs::create_dir_all(dir).map_err(|e| bad("fileengine.cache", e.to_string()))?;
        std::fs::write(&local, bytes).map_err(|e| bad("fileengine.cache", e.to_string()))?;
        std::fs::write(&stamp, &want).map_err(|e| bad("fileengine.cache", e.to_string()))?;
    }
    Ok((local, node.parent_uid))
}

/// The uid of the file `name` in folder `folder`.
fn find(app: &mut App, conn: &str, folder: &str, name: &str) -> Result<Option<String>> {
    app.fileengine.with(conn, |c| Ok(c.list(folder, false)?.into_iter().find(|e| !e.deleted && !e.is_dir() && e.name == name).map(|e| e.uid)))
}

fn nonce() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}-{}", d.as_secs(), d.subsec_nanos())
}

/// Runs `name` with the FileEngine addresses in its marked parameters resolved (COL-FE-012).
/// `None` when it has none, for the caller to run it as it is.
pub(crate) fn execute(app: &mut App, name: &str, args: &Value, caller: &Caller) -> Option<Result<Outcome>> {
    let spec = app.registry().get(name)?;
    let root = spec.params.clone();
    let mut found = Vec::new();
    marked(&root, &root, args, "", &mut found);
    if found.is_empty() {
        return None;
    }
    Some(run(app, name, args.clone(), caller, found))
}

/// Downloads the FileEngine file at `text` (and, with `set`, the other files of its folder) into
/// the per-user cache, unless that version is there already, and returns its local path: for
/// commands' marked parameters, and for the window to look at a file before a command reads it.
pub fn fetch(app: &mut App, text: &str, set: bool) -> Result<PathBuf> {
    let a = address(text)?;
    app.fileengine.known(&a.connection_id)?;
    let cache = app.fileengine.cache_root()?;
    let uid = match &a.name {
        Some(n) => find(app, &a.connection_id, &a.uid, n)?.ok_or_else(|| bad("fileengine.not_found", format!("{n} is not in that FileEngine folder")))?,
        None => a.uid.clone(),
    };
    let dir = cache.join("files").join(safe(&a.connection_id)).join(match &a.version {
        Some(v) => format!("{}@{}", safe(&uid), safe(v)),
        None => safe(&uid),
    });
    let (path, parent) = download(app, &a.connection_id, &uid, a.version.as_deref(), &dir)?;
    if set {
        // The other files of its folder, at their latest versions: an OBJ's MTL and textures, a
        // glTF's buffers, a script's modules.
        let siblings = app.fileengine.with(&a.connection_id, |c| c.list(&parent, false))?;
        for s in siblings.into_iter().filter(|s| !s.deleted && !s.is_dir() && s.uid != uid) {
            download(app, &a.connection_id, &s.uid, None, &dir)?;
        }
    }
    Ok(path)
}

fn run(app: &mut App, name: &str, mut args: Value, caller: &Caller, found: Vec<(String, Role, String)>) -> Result<Outcome> {
    let cache = app.fileengine.cache_root()?;
    let mut outgoing = Vec::new();
    for (at, role, text) in found {
        let a = address(&text)?;
        app.fileengine.known(&a.connection_id)?;
        let local = match role {
            Role::Read | Role::ReadSet => fetch(app, &text, role == Role::ReadSet)?,
            Role::Write | Role::WriteFolder | Role::WriteAny => {
                let dir = cache.join("outgoing").join(nonce());
                std::fs::create_dir_all(&dir).map_err(|e| bad("fileengine.cache", e.to_string()))?;
                let file = match (role, &a.name) {
                    (Role::WriteFolder, Some(_)) => return Err(bad("fileengine.bad_address", format!("{text}: a folder to write into is fileengine://<connection>/<folder uid>"))),
                    (Role::Write, None) => return Err(bad("fileengine.bad_address", format!("{text}: a file to write is fileengine://<connection>/<folder uid>/<name>"))),
                    (_, n) => n.clone(),
                };
                let local = match &file {
                    Some(n) => dir.join(n),
                    None => dir.clone(),
                };
                outgoing.push(Outgoing { parameter: at.clone(), connection: a.connection_id.clone(), folder_uid: a.uid.clone(), local: local.clone(), file: file.is_some() });
                local
            }
        };
        if let Some(v) = args.pointer_mut(&at) {
            *v = Value::String(local.to_string_lossy().into_owned());
        }
    }

    let mut out = app.execute_local(name, args, caller)?;
    if outgoing.is_empty() {
        return Ok(out);
    }
    let metadata = metadata(app, name);
    let mut uploads = Vec::new();
    for o in &outgoing {
        let files: Vec<PathBuf> = if o.file {
            vec![o.local.clone()].into_iter().filter(|p| p.is_file()).collect()
        } else {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&o.local).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect()).unwrap_or_default();
            v.sort();
            v
        };
        for f in files {
            match upload(app, o, &f, &metadata) {
                Ok(u) => uploads.push(u),
                Err(e) => {
                    return Err(bad("fileengine.upload_failed", format!("the command ran, but {} was not uploaded ({}); it is kept at {}", f.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), e.message, f.display())).with_details(json!({ "local_path": f, "uploaded": uploads })));
                }
            }
        }
        let root = if o.file { o.local.parent().map(Path::to_path_buf) } else { Some(o.local.clone()) };
        if let Some(r) = root {
            let _ = std::fs::remove_dir_all(r);
        }
    }
    if let Value::Object(m) = &mut out.result {
        m.insert("fileengine".into(), serde_json::to_value(&uploads).expect("serialises"));
    }
    Ok(out)
}

/// The metadata every uploaded output gets.
fn metadata(app: &App, command: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("riviera.output".into(), command.to_owned());
    m.insert("riviera.published_at".into(), crate::fileengine::now().to_string());
    if let Some(open) = app.active().and_then(|id| app.document(id)) {
        let path = open.project.path();
        if let Some(l) = crate::fileengine::link_of(path) {
            m.insert("riviera.source_uid".into(), l.uid);
            m.insert("riviera.source_version".into(), l.version);
            m.insert("riviera.source_name".into(), l.name);
        } else if let Some(n) = path.file_name() {
            m.insert("riviera.source_name".into(), n.to_string_lossy().into_owned());
        }
        if let Some(p) = open.doc.store().iter().find(|e| e.class == "IfcProject") {
            m.insert("ifc.project_guid".into(), p.guid().to_string());
        }
    }
    m
}

fn upload(app: &mut App, o: &Outgoing, file: &Path, metadata: &BTreeMap<String, String>) -> Result<Upload> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let there = find(app, &o.connection, &o.folder_uid, &name)?;
    app.fileengine.with(&o.connection, |c| {
        let replaced = there.is_some();
        let uid = match there {
            Some(u) => u,
            None => c.create_file(&o.folder_uid, &name)?,
        };
        c.write_file(&uid, file)?;
        // The metadata is best effort: the file is uploaded.
        for (k, v) in metadata {
            let _ = c.set_metadata(&uid, k, v);
        }
        let version = c.stat(&uid)?.version;
        Ok(Upload { parameter: o.parameter.clone(), uid, name: name.clone(), version, replaced })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_parameters_are_found_through_refs_options_and_arrays() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "x-riviera-file": "write" },
                "maybe": { "anyOf": [{ "$ref": "#/$defs/P" }, { "type": "null" }] },
                "files": { "type": "array", "items": { "type": "string", "x-riviera-file": "read" } },
                "plain": { "type": "string" }
            },
            "$defs": { "P": { "type": "string", "x-riviera-file": "read_set" } }
        });
        let args = json!({ "path": "fileengine://c/f/a.csv", "maybe": "fileengine://c/u", "files": ["x", "fileengine://c/v"], "plain": "fileengine://c/w" });
        let mut out = Vec::new();
        marked(&schema, &schema, &args, "", &mut out);
        out.sort();
        assert_eq!(out, vec![("/files/1".to_owned(), Role::Read, "fileengine://c/v".to_owned()), ("/maybe".to_owned(), Role::ReadSet, "fileengine://c/u".to_owned()), ("/path".to_owned(), Role::Write, "fileengine://c/f/a.csv".to_owned())]);
        let mut params = Vec::new();
        marked_parameters(&schema, &schema, "", &mut params, 0);
        params.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(params.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["files[]", "maybe", "path"]);
    }
}
