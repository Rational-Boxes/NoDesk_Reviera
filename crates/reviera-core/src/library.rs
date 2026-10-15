// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Component Libraries (DM-LIB-001 … 004): a library is a folder of ordinary 3D files that is
//! opened, with one JSON manifest, `reviera-library.json`, in its root.
//!
//! - Its sub-directories are its folders; its items are the files in them that Reviera reads:
//!   mesh files (OBJ with its MTL and textures, STL, PLY, glTF, GLB), placed as custom objects
//!   (DM-IMP-015), and IFC files (components that are model fragments, DM-LIB-003).
//! - The manifest holds the library's identity, default settings, settings per folder (inherited
//!   by sub-folders) and details per item, keyed by path relative to the root: what a file's own
//!   format cannot hold (name, category, tags, units, up axis, insertion point, product data).
//!   Files without an entry use their folder's settings. Keys are written sorted, so the manifest
//!   diffs cleanly; writes are atomic.
//! - **Import into Library** copies files in (an OBJ with its MTL and textures, a glTF with its
//!   buffers and images) and adds their entries. Deleted items go to the library's `.deleted`
//!   folder, from which they can be restored.
//! - **Place Component** (`component.place`) places a library item, or a type already in the
//!   project, at one or more points: an item is loaded into the project as a type the first time
//!   and reused after.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::{self, Category, ImportOptions};
use crate::store::Store;

pub const MANIFEST: &str = "reviera-library.json";
pub const FORMAT: &str = "reviera-library";
/// Where deleted items wait to be restored.
pub const DELETED: &str = ".deleted";

/// Settings a folder gives its items (and sub-folders, unless they override them).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FolderSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// Import options for mesh files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportOptions>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// An item's details: what its file's format may not hold.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ItemDetails {
    /// Stable identity, so placed types find their item after it moves.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportOptions>,
    /// Manufacturer, model, reference, URL, cost, notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<BTreeMap<String, Value>>,
    /// Author, source URL, licence, date added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<BTreeMap<String, Value>>,
    /// Further properties, by property set and name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, BTreeMap<String, Value>>>,
    /// The file's SHA-256 when its entry was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// `reviera-library.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default)]
    pub defaults: FolderSettings,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<String, FolderSettings>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub items: BTreeMap<String, ItemDetails>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub smart_folders: BTreeMap<String, Value>,
}

impl Manifest {
    pub fn new(name: &str) -> Self {
        Self {
            format: FORMAT.into(),
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            description: None,
            author: None,
            licence: None,
            homepage: None,
            defaults: FolderSettings::default(),
            folders: BTreeMap::new(),
            items: BTreeMap::new(),
            smart_folders: BTreeMap::new(),
        }
    }

    /// Reads a library's manifest; `None` when the folder has none.
    pub fn read(root: &Path) -> Result<Option<Self>> {
        let path = root.join(MANIFEST);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(&path, e)),
        };
        let m: Self = serde_json::from_str(&text)
            .map_err(|e| Error::new("library.bad_manifest", format!("{}: {e}", path.display())))?;
        if m.format != FORMAT || m.version != 1 {
            return Err(Error::new(
                "library.bad_manifest",
                format!(
                    "{} is not a version 1 Reviera library manifest",
                    path.display()
                ),
            ));
        }
        Ok(Some(m))
    }

    /// Writes the manifest atomically, keys in a stable order.
    pub fn write(&self, root: &Path) -> Result<()> {
        let path = root.join(MANIFEST);
        let tmp = root.join(format!(".{MANIFEST}.tmp"));
        let mut text = serde_json::to_string_pretty(self).expect("serialises");
        text.push('\n');
        std::fs::write(&tmp, text).map_err(|e| io(&tmp, e))?;
        std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
    }

    /// The settings that apply in `folder` (relative path, `""` for the root).
    pub fn settings(&self, folder: &str) -> FolderSettings {
        let mut s = self.defaults.clone();
        let mut prefix = String::new();
        for part in folder.split('/').filter(|p| !p.is_empty()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some(f) = self.folders.get(&prefix) {
                if f.category.is_some() {
                    s.category = f.category;
                }
                if f.import.is_some() {
                    s.import = f.import;
                }
                for t in &f.tags {
                    if !s.tags.contains(t) {
                        s.tags.push(t.clone());
                    }
                }
            }
        }
        s
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::new("library.io", format!("{}: {e}", path.display()))
}

/// An item's kind, by its file's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A mesh file, placed as a custom object.
    Object,
    /// An IFC file: a model fragment.
    Fragment,
}

pub fn kind_of(path: &Path) -> Option<ItemKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if crate::mesh_io::EXTENSIONS.contains(&ext.as_str()) {
        Some(ItemKind::Object)
    } else if ext == "ifc" || ext == "ifczip" {
        Some(ItemKind::Fragment)
    } else {
        None
    }
}

/// A path relative to the library root, with `/` separators; refuses escapes.
fn relative(rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(Error::new(
            "library.bad_path",
            format!("{rel} is not a path inside the library"),
        ));
    }
    Ok(p.to_path_buf())
}

fn rel_string(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| io(path, e))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The manifest of the library at `root`, or an error naming the folder.
fn open(root: &Path) -> Result<Manifest> {
    Manifest::read(root)?.ok_or_else(|| {
        Error::new(
            "library.no_manifest",
            format!(
                "{} is not a Reviera library (it has no {MANIFEST}); create one with library.create",
                root.display()
            ),
        )
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The library's folder; made when missing. A folder that already has files becomes a
    /// library as it is.
    pub path: String,
    pub name: String,
}

/// A library's summary.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryInfo {
    pub path: String,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub items: usize,
    pub writable: bool,
}

fn info(root: &Path, m: &Manifest) -> LibraryInfo {
    let mut items = 0;
    walk(root, root, &mut |_, _| items += 1);
    LibraryInfo {
        path: root.to_string_lossy().into_owned(),
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        items,
        writable: std::fs::metadata(root).is_ok_and(|md| !md.permissions().readonly()),
    }
}

/// Makes a library (`library.create`): writes its manifest, keeping one that is already there.
pub fn create(_: &Store, p: CreateParams) -> Result<LibraryInfo> {
    let root = PathBuf::from(&p.path);
    std::fs::create_dir_all(&root).map_err(|e| io(&root, e))?;
    let m = match Manifest::read(&root)? {
        Some(m) => m,
        None => {
            let m = Manifest::new(&p.name);
            m.write(&root)?;
            m
        }
    };
    Ok(info(&root, &m))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub path: String,
}

/// Opens a library (`library.open`): its summary. A folder without a manifest is refused; the
/// window offers to create one.
pub fn open_library(_: &Store, p: OpenParams) -> Result<LibraryInfo> {
    let root = PathBuf::from(&p.path);
    let m = open(&root)?;
    Ok(info(&root, &m))
}

/// Calls `f(relative path, kind)` for every item under `dir`, skipping hidden folders.
fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&Path, ItemKind)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, f);
        } else if let Some(k) = kind_of(&path) {
            f(path.strip_prefix(root).unwrap_or(&path), k);
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The library's folder.
    pub library: String,
    /// A folder in it (relative, `/` separated); the root when omitted.
    #[serde(default)]
    pub folder: String,
    /// Every item below the folder, not only its own.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
    /// Items in it and below it.
    pub items: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ItemInfo {
    /// Relative to the library root.
    pub path: String,
    pub kind: ItemKind,
    pub format: String,
    /// The item's name: its manifest name, or the file's name without its extension.
    pub name: String,
    pub category: Category,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub size_bytes: u64,
    /// The item has its own details in the manifest.
    pub has_details: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Listing {
    pub folders: Vec<FolderInfo>,
    pub items: Vec<ItemInfo>,
}

fn item_info(root: &Path, m: &Manifest, rel: &Path, kind: ItemKind) -> ItemInfo {
    let key = rel_string(rel);
    let folder = rel.parent().map(rel_string).unwrap_or_default();
    let settings = m.settings(&folder);
    let d = m.items.get(&key);
    let mut tags = settings.tags.clone();
    for t in d.map(|d| d.tags.as_slice()).unwrap_or_default() {
        if !tags.contains(t) {
            tags.push(t.clone());
        }
    }
    ItemInfo {
        name: d.and_then(|d| d.name.clone()).unwrap_or_else(|| {
            rel.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        category: d
            .and_then(|d| d.category)
            .or(settings.category)
            .unwrap_or_default(),
        tags,
        description: d.and_then(|d| d.description.clone()),
        size_bytes: std::fs::metadata(root.join(rel))
            .map(|md| md.len())
            .unwrap_or(0),
        has_details: d.is_some(),
        format: rel
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default(),
        kind,
        path: key,
    }
}

/// A folder's sub-folders (with their item counts) and items (`library.list`).
pub fn list(_: &Store, p: ListParams) -> Result<Listing> {
    let root = PathBuf::from(&p.library);
    let m = open(&root)?;
    let dir = root.join(relative_or_root(&p.folder)?);
    let mut folders = Vec::new();
    let mut items = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| io(&dir, e))?;
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            let mut n = 0;
            walk(&root, &path, &mut |rel, k| {
                n += 1;
                if p.recursive {
                    items.push(item_info(&root, &m, rel, k));
                }
            });
            folders.push(FolderInfo {
                path: rel_string(path.strip_prefix(&root).unwrap_or(&path)),
                name,
                items: n,
            });
        } else if let Some(k) = kind_of(&path) {
            items.push(item_info(
                &root,
                &m,
                path.strip_prefix(&root).unwrap_or(&path),
                k,
            ));
        }
    }
    Ok(Listing { folders, items })
}

fn relative_or_root(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() {
        Ok(PathBuf::new())
    } else {
        relative(rel)
    }
}

/// The files an item needs beside it, relative to its folder: an OBJ's MTL libraries and their
/// textures, a glTF's buffers and images.
pub fn companions(file: &Path) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |rel: &str| {
        if let Ok(p) = relative(rel.trim())
            && !out.contains(&p)
            && dir.join(&p).is_file()
        {
            out.push(p);
        }
    };
    match ext.as_str() {
        "obj" => {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            for line in text.lines() {
                if let Some(libs) = line.trim().strip_prefix("mtllib ") {
                    for lib in libs.split_whitespace() {
                        add(lib);
                        let mtl = std::fs::read_to_string(dir.join(lib)).unwrap_or_default();
                        for l in mtl.lines() {
                            let l = l.trim();
                            if l.starts_with("map_")
                                || l.starts_with("bump ")
                                || l.starts_with("disp ")
                            {
                                // The texture file is the last word (options come first).
                                if let Some(tex) = l.split_whitespace().last() {
                                    add(tex);
                                }
                            }
                        }
                    }
                }
            }
        }
        "gltf" => {
            let json: Value = std::fs::read(file)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Value::Null);
            for key in ["buffers", "images"] {
                for b in json
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(uri) = b.get("uri").and_then(Value::as_str)
                        && !uri.starts_with("data:")
                    {
                        add(uri);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportFilesParams {
    /// The library's folder.
    pub library: String,
    /// The folder to copy into (relative); the root when omitted. Made when missing.
    #[serde(default)]
    pub folder: String,
    /// The files to import, from anywhere.
    pub files: Vec<String>,
    /// Details given to every file imported (a name only when one file is imported).
    #[serde(default)]
    pub details: ItemDetails,
    /// Import a file whose content is already in the library (otherwise it is skipped).
    #[serde(default)]
    pub import_duplicates: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImportedItem {
    /// The source file.
    pub file: String,
    /// The item's path in the library.
    pub path: String,
    /// The companion files copied with it.
    pub companions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Skipped {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImportReport {
    pub imported: Vec<ImportedItem>,
    pub skipped: Vec<Skipped>,
}

/// A free name in `dir` for `name`: `chair.obj`, else `chair (2).obj`, …
fn free_name(dir: &Path, name: &Path) -> PathBuf {
    if !dir.join(name).exists() {
        return name.to_path_buf();
    }
    let stem = name
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = name
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|i| PathBuf::from(format!("{stem} ({i}){ext}")))
        .find(|n| !dir.join(n).exists())
        .expect("a free name")
}

/// Import into Library (`library.import_files`): copies files in a format Reviera reads into a
/// library folder, with the files they need beside them, and adds their entries to the manifest.
pub fn import_files(_: &Store, p: ImportFilesParams) -> Result<ImportReport> {
    let root = PathBuf::from(&p.library);
    let mut m = open(&root)?;
    let folder = relative_or_root(&p.folder)?;
    let dest = root.join(&folder);
    std::fs::create_dir_all(&dest).map_err(|e| io(&dest, e))?;
    let known: BTreeMap<String, String> = m
        .items
        .iter()
        .filter_map(|(k, d)| Some((d.sha256.clone()?, k.clone())))
        .collect();
    let mut report = ImportReport {
        imported: Vec::new(),
        skipped: Vec::new(),
    };
    for file in &p.files {
        let src = PathBuf::from(file);
        let skip = |reason: String| Skipped {
            file: file.clone(),
            reason,
        };
        if kind_of(&src).is_none() {
            report.skipped.push(skip(
                "not a format a library holds (OBJ, STL, PLY, glTF, GLB, IFC)".into(),
            ));
            continue;
        }
        let sha = match sha256(&src) {
            Ok(s) => s,
            Err(e) => {
                report.skipped.push(skip(e.message));
                continue;
            }
        };
        if let Some(existing) = known.get(&sha)
            && !p.import_duplicates
        {
            report
                .skipped
                .push(skip(format!("already in the library as {existing}")));
            continue;
        }
        // Companions keep their names; one that would overwrite a different file is refused.
        let src_dir = src.parent().unwrap_or(Path::new("."));
        let comps = companions(&src);
        let clash = comps.iter().find(|c| {
            let to = dest.join(c);
            to.exists() && sha256(&to).ok() != sha256(&src_dir.join(c)).ok()
        });
        if let Some(c) = clash {
            report.skipped.push(skip(format!(
                "{} would overwrite a different file of that name in the folder; import into another folder",
                c.display()
            )));
            continue;
        }
        let name = free_name(&dest, Path::new(src.file_name().unwrap_or_default()));
        for c in &comps {
            let to = dest.join(c);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
            }
            std::fs::copy(src_dir.join(c), &to).map_err(|e| io(&to, e))?;
        }
        std::fs::copy(&src, dest.join(&name)).map_err(|e| io(&dest.join(&name), e))?;
        let key = rel_string(&folder.join(&name));
        let mut d = p.details.clone();
        if p.files.len() > 1 {
            d.name = None;
        }
        d.guid = uuid::Uuid::new_v4().to_string();
        d.sha256 = Some(sha);
        let mut prov = d.provenance.take().unwrap_or_default();
        prov.entry("source_file".into())
            .or_insert_with(|| Value::String(src.to_string_lossy().into_owned()));
        d.provenance = Some(prov);
        m.items.insert(key.clone(), d);
        report.imported.push(ImportedItem {
            file: file.clone(),
            path: key,
            companions: comps.iter().map(|c| rel_string(&folder.join(c))).collect(),
        });
    }
    m.write(&root)?;
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetMetaParams {
    pub library: String,
    /// The item's path in the library.
    pub item: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ItemMeta {
    pub info: ItemInfo,
    /// Its own details in the manifest (empty when it has none).
    pub details: ItemDetails,
    /// The settings it inherits from its folder.
    pub folder: FolderSettings,
}

/// An item's details and what it inherits (`library.get_meta`).
pub fn get_meta(_: &Store, p: GetMetaParams) -> Result<ItemMeta> {
    let root = PathBuf::from(&p.library);
    let m = open(&root)?;
    let rel = relative(&p.item)?;
    let kind = kind_of(&rel)
        .filter(|_| root.join(&rel).is_file())
        .ok_or_else(|| {
            Error::new(
                "library.no_item",
                format!("{} is not an item of the library", p.item),
            )
        })?;
    let folder = rel.parent().map(rel_string).unwrap_or_default();
    Ok(ItemMeta {
        info: item_info(&root, &m, &rel, kind),
        details: m.items.get(&rel_string(&rel)).cloned().unwrap_or_default(),
        folder: m.settings(&folder),
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMetaParams {
    pub library: String,
    /// Items' paths, and folders' paths (ending in `/`) for folder settings.
    pub paths: Vec<String>,
    /// For items: the details to set (fields given replace the item's; `null` clears).
    #[serde(default)]
    pub details: serde_json::Map<String, Value>,
    /// For folders: the settings to set.
    #[serde(default)]
    pub settings: Option<FolderSettings>,
}

/// Edits items' details or folders' settings in the manifest (`library.set_meta`); the files are
/// not touched.
pub fn set_meta(_: &Store, p: SetMetaParams) -> Result<()> {
    let root = PathBuf::from(&p.library);
    let mut m = open(&root)?;
    for path in &p.paths {
        if let Some(folder) = path.strip_suffix('/') {
            let key = rel_string(&relative_or_root(folder)?);
            let s = p.settings.clone().unwrap_or_default();
            if key.is_empty() {
                m.defaults = s;
            } else {
                m.folders.insert(key, s);
            }
            continue;
        }
        let rel = relative(path)?;
        if !root.join(&rel).is_file() {
            return Err(Error::new(
                "library.no_item",
                format!("{path} is not an item of the library"),
            ));
        }
        let key = rel_string(&rel);
        let current = m.items.get(&key).cloned().unwrap_or_default();
        let mut v = serde_json::to_value(&current).expect("serialises");
        let obj = v.as_object_mut().expect("an object");
        for (k, val) in &p.details {
            if k == "guid" || k == "sha256" {
                continue;
            }
            if val.is_null() {
                obj.remove(k);
            } else {
                obj.insert(k.clone(), val.clone());
            }
        }
        let mut d: ItemDetails = serde_json::from_value(v)
            .map_err(|e| Error::new("library.bad_details", e.to_string()))?;
        if d.guid.is_empty() {
            d.guid = uuid::Uuid::new_v4().to_string();
        }
        if d.sha256.is_none() {
            d.sha256 = sha256(&root.join(&rel)).ok();
        }
        m.items.insert(key, d);
    }
    m.write(&root)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteItemsParams {
    pub library: String,
    /// Items' paths.
    pub items: Vec<String>,
}

/// Deletes items (`library.delete_items`): each file and its companions move to the library's
/// `.deleted` folder, keeping their paths, and their entries go with them; `library.restore_items`
/// brings them back.
pub fn delete_items(_: &Store, p: DeleteItemsParams) -> Result<Vec<String>> {
    move_items(&p.library, &p.items, true)
}

/// Restores deleted items (`library.restore_items`).
pub fn restore_items(_: &Store, p: DeleteItemsParams) -> Result<Vec<String>> {
    move_items(&p.library, &p.items, false)
}

fn move_items(library: &str, items: &[String], delete: bool) -> Result<Vec<String>> {
    let root = PathBuf::from(library);
    let mut m = open(&root)?;
    let trash = root.join(DELETED);
    let (from, to) = if delete {
        (root.clone(), trash.clone())
    } else {
        (trash.clone(), root.clone())
    };
    // Deleted items' entries are kept under `.deleted/<path>` so they come back with them.
    let entry_key = |rel: &str, deleted: bool| {
        if deleted {
            format!("{DELETED}/{rel}")
        } else {
            rel.to_owned()
        }
    };
    let mut moved = Vec::new();
    for item in items {
        let rel = relative(item)?;
        let src = from.join(&rel);
        if !src.is_file() {
            return Err(Error::new(
                "library.no_item",
                format!("{item} is not there to move"),
            ));
        }
        let mut files = companions(&src)
            .into_iter()
            .map(|c| rel.parent().unwrap_or(Path::new("")).join(c))
            .collect::<Vec<_>>();
        files.push(rel.clone());
        for f in &files {
            let (a, b) = (from.join(f), to.join(f));
            if b.exists() {
                continue; // a shared companion already moved, or restored over nothing
            }
            if let Some(parent) = b.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
            }
            // A companion another item still uses stays where it is when deleting.
            if delete && f != &rel && shared_companion(&root, f, &rel) {
                continue;
            }
            std::fs::rename(&a, &b).map_err(|e| io(&a, e))?;
        }
        let key = rel_string(&rel);
        if let Some(d) = m.items.remove(&entry_key(&key, !delete)) {
            m.items.insert(entry_key(&key, delete), d);
        }
        moved.push(key);
    }
    m.write(&root)?;
    Ok(moved)
}

/// Whether another item in `f`'s folder needs companion `f`.
fn shared_companion(root: &Path, f: &Path, item: &Path) -> bool {
    let Some(dir) = f.parent() else { return false };
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p != root.join(item)
            && kind_of(&p).is_some()
            && companions(&p)
                .iter()
                .any(|c| root.join(dir).join(c) == root.join(f))
    })
}

// ---- Placing ----

/// Where a placed type came from, kept in its recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
/// No folder paths are kept, so a model shared with others does not carry the author's folders:
/// the library is found again by its id among the libraries that are open.
pub struct LibraryLink {
    pub library_id: String,
    /// The library's name when loaded.
    pub library: String,
    /// The item's path in it.
    pub item: String,
    /// The item's GUID, when it has one in the manifest.
    #[serde(default)]
    pub item_guid: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A type already in the project.
    pub type_id: Option<ElementRef>,
    /// Or a library item: the library's folder …
    pub library: Option<String>,
    /// … and the item's path in it.
    pub item: Option<String>,
    /// One copy at each point (plan, metres).
    pub points: Vec<[f64; 2]>,
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    #[serde(default)]
    pub mirror: bool,
}

/// The project's type loaded from this library item, if any.
fn loaded_type(store: &Store, library_id: &str, item: &str, guid: &str) -> Option<ElementId> {
    mesh_objects::types(store)
        .into_iter()
        .find_map(|(id, t, _)| {
            let l = t.library.as_ref()?;
            let same = l.library_id == library_id
                && ((!guid.is_empty() && l.item_guid == guid) || l.item == item);
            same.then_some(id)
        })
}

/// Loads a library item into the project as a type, or finds it loaded already
/// (`library.load`'s work, shared by Place Component).
pub(crate) fn load_item(
    txn: &mut crate::txn::Transaction<'_>,
    library: &str,
    item: &str,
) -> Result<ElementId> {
    let root = PathBuf::from(library);
    let m = open(&root)?;
    let rel = relative(item)?;
    let path = root.join(&rel);
    match kind_of(&rel) {
        Some(ItemKind::Object) if path.is_file() => {}
        Some(ItemKind::Fragment) => {
            return Err(Error::new(
                "library.fragment_not_placeable",
                "placing IFC components (model fragments) is not built yet; mesh components can be placed",
            ));
        }
        _ => {
            return Err(Error::new(
                "library.no_item",
                format!("{item} is not an item of the library"),
            ));
        }
    }
    let key = rel_string(&rel);
    let details = m.items.get(&key).cloned().unwrap_or_default();
    if let Some(id) = loaded_type(txn.store(), &m.id, &key, &details.guid) {
        return Ok(id);
    }
    let folder = rel.parent().map(rel_string).unwrap_or_default();
    let settings = m.settings(&folder);
    let id = mesh_objects::import(
        txn,
        mesh_objects::ImportParams {
            path: path.to_string_lossy().into_owned(),
            name: Some(details.name.clone().unwrap_or_else(|| {
                rel.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })),
            category: details.category.or(settings.category).unwrap_or_default(),
            options: details.import.or(settings.import).unwrap_or_default(),
            allow_large: true,
        },
    )?;
    let link = LibraryLink {
        library_id: m.id.clone(),
        library: m.name.clone(),
        item: key,
        item_guid: details.guid.clone(),
    };
    mesh_objects::set_library_link(txn, id, link)?;
    // Nor the file's full path (Reload from File finds it through the library).
    mesh_objects::forget_source_path(txn, id)?;
    Ok(id)
}

/// Place Component (`component.place`): one copy of a type, or of a library item (loaded as a
/// type the first time), at each point.
pub(crate) fn place(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceParams,
) -> Result<Vec<ElementId>> {
    let type_id = match (&p.type_id, &p.library, &p.item) {
        (Some(t), None, None) => {
            let id = txn
                .store()
                .resolve(t)
                .ok_or_else(|| Error::new("element.not_found", "no such type"))?;
            ElementRef::Id(id)
        }
        (None, Some(lib), Some(item)) => ElementRef::Id(load_item(txn, lib, item)?),
        _ => {
            return Err(Error::new(
                "component.what",
                "give a type_id, or a library and an item",
            ));
        }
    };
    if p.points.is_empty() {
        return Err(Error::new("component.no_points", "give at least one point"));
    }
    p.points
        .iter()
        .map(|point| {
            mesh_objects::place(
                txn,
                mesh_objects::PlaceParams {
                    type_id: type_id.clone(),
                    point: *point,
                    level: p.level.clone(),
                    offset: p.offset,
                    rotation: p.rotation,
                    mirror: p.mirror,
                    scale: 1.0,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_settings_inherit_and_override() {
        let mut m = Manifest::new("t");
        m.defaults.category = Some(Category::Furniture);
        m.folders.insert(
            "Plants".into(),
            FolderSettings {
                category: Some(Category::Planting),
                tags: vec!["green".into()],
                ..Default::default()
            },
        );
        m.folders.insert(
            "Plants/Trees".into(),
            FolderSettings {
                tags: vec!["tree".into()],
                ..Default::default()
            },
        );
        let s = m.settings("Plants/Trees");
        assert_eq!(s.category, Some(Category::Planting));
        assert_eq!(s.tags, vec!["green".to_owned(), "tree".to_owned()]);
        assert_eq!(m.settings("Chairs").category, Some(Category::Furniture));
    }

    #[test]
    fn paths_stay_inside() {
        assert!(relative("../x.obj").is_err());
        assert!(relative("/etc/passwd").is_err());
        assert!(relative("Chairs/a.obj").is_ok());
    }
}
