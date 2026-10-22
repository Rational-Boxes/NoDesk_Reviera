// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reading classification tables and keynote files (DM-CLS-001): `classification.load` reads a
//! `code,parent_code,title` CSV or an IFC file's `IfcClassification` tree and adds the system to
//! the project (`classification.create`); `keynotes.load` reads a tab-separated keynote file
//! (kept relative to the project when it lies beside it) and `keynotes.reload` reads it again.
//!
//! Not built yet: the buildingSMART Data Dictionary (online) as a source.

use std::path::{Path, PathBuf};

use reviera_core::classification::Item;
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use reviera_ifc::{AttrValue, IfcModel, StepId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

const fn meta(name: &'static str, summary: &'static str, description: &'static str, effects: &'static [Effect]) -> CommandMeta {
    CommandMeta { name, version: 1, summary, description, effects, transaction_name: "" }
}

fn text(ifc: &IfcModel, id: StepId, attr: &str) -> String {
    match ifc.attribute(id, attr) {
        Ok(AttrValue::String(s)) => s,
        _ => String::new(),
    }
}

/// An IFC file's first (or named) classification: its name, source, edition and items.
fn read_ifc(path: &Path, want: Option<&str>) -> Result<(String, String, String, Vec<Item>)> {
    let ifc = IfcModel::open(path).map_err(|e| bad("classification.read", e.to_string()))?;
    let classes = ifc.entities_of_type("IfcClassification").map_err(|e| bad("classification.read", e.to_string()))?;
    let root = classes
        .iter()
        .find(|c| want.is_none_or(|w| text(&ifc, c.id, "Name").eq_ignore_ascii_case(w)))
        .ok_or_else(|| bad("classification.none", format!("{} has no IfcClassification{}", path.display(), want.map(|w| format!(" named {w}")).unwrap_or_default())))?
        .id;
    let refs = ifc.entities_of_type("IfcClassificationReference").map_err(|e| bad("classification.read", e.to_string()))?;
    let source_of = |id: StepId| match ifc.attribute(id, "ReferencedSource") {
        Ok(AttrValue::Ref(r)) => Some(StepId(r)),
        _ => None,
    };
    let code_of = |id: StepId| {
        let c = text(&ifc, id, "Identification");
        if c.is_empty() { text(&ifc, id, "ItemReference") } else { c }
    };
    // References whose chain of sources reaches the root.
    let under = |mut id: StepId| {
        for _ in 0..64 {
            match source_of(id) {
                Some(s) if s == root => return true,
                Some(s) => id = s,
                None => return false,
            }
        }
        false
    };
    let mut items = Vec::new();
    for r in refs.iter().filter(|r| under(r.id)) {
        let parent = source_of(r.id).filter(|s| *s != root).map(code_of);
        items.push(Item { code: code_of(r.id), parent, title: text(&ifc, r.id, "Name") });
    }
    // Parents before children.
    let mut ordered: Vec<Item> = Vec::new();
    while !items.is_empty() {
        let before = items.len();
        items.retain(|i| {
            if i.parent.as_ref().is_none_or(|p| ordered.iter().any(|o| &o.code == p)) {
                ordered.push(i.clone());
                false
            } else {
                true
            }
        });
        if items.len() == before {
            ordered.append(&mut items);
        }
    }
    Ok((text(&ifc, root, "Name"), text(&ifc, root, "Source"), text(&ifc, root, "Edition"), ordered))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadParams {
    /// A `.csv` (`code,parent_code,title`) or `.ifc` file.
    pub path_or_url: String,
    /// The system's name (a CSV's file name by default; for an IFC file, which classification).
    pub name: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub edition: Option<String>,
}

pub(crate) struct ClassificationLoad;

impl HostCommand for ClassificationLoad {
    type Host = App;
    type Params = LoadParams;
    type Output = String;
    const META: CommandMeta = meta(
        "classification.load",
        "Load a classification system",
        "Loads a classification table into the open project (DM-CLS-001): a CSV of `code,parent_code,title` (named `name`, else after the file) or an IFC file's IfcClassification tree (the one named `name`, else the first). A system of the same name is replaced. Returns its id.",
        &[Effect::FileRead, Effect::Edit],
    );

    fn run(&self, app: &mut App, caller: &Caller, p: LoadParams) -> Result<String> {
        if p.path_or_url.starts_with("http://") || p.path_or_url.starts_with("https://") {
            return Err(bad("classification.url", "loading classifications from the Data Dictionary or a URL is not built yet; download the file"));
        }
        let path = PathBuf::from(&p.path_or_url);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let (name, source, edition, items) = if ext == "ifc" || ext == "ifczip" {
            let (n, s, e, items) = read_ifc(&path, p.name.as_deref())?;
            (p.name.clone().unwrap_or(n), p.source.clone().unwrap_or(s), p.edition.clone().unwrap_or(e), items)
        } else {
            let t = std::fs::read_to_string(&path).map_err(|e| bad("classification.read", format!("{}: {e}", path.display())))?;
            let items = reviera_core::classification::parse_csv(&t)?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Classification").to_owned();
            (p.name.clone().unwrap_or(stem), p.source.clone().unwrap_or_default(), p.edition.clone().unwrap_or_default(), items)
        };
        let out = app.execute(
            "classification.create",
            json!({ "name": name, "source": source, "edition": edition, "location": p.path_or_url, "items": items }),
            caller,
        )?;
        Ok(out.result.as_str().unwrap_or_default().to_owned())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeynotesParams {
    /// A tab-separated keynote file (`key`, `text`, `parent_key`).
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeynotesReport {
    /// As kept: relative to the project when beside it.
    pub path: String,
    pub entries: usize,
}

fn project_dir(app: &App) -> Option<PathBuf> {
    let d = app.document(app.active()?)?;
    if d.untitled {
        return None;
    }
    d.project.path().parent().map(Path::to_path_buf)
}

fn load_keynotes(app: &mut App, caller: &Caller, given: &str) -> Result<KeynotesReport> {
    let dir = project_dir(app);
    let path = match &dir {
        Some(d) if Path::new(given).is_relative() => d.join(given),
        _ => PathBuf::from(given),
    };
    let text = std::fs::read_to_string(&path).map_err(|e| bad("keynotes.read", format!("{}: {e}", path.display())))?;
    let entries = reviera_core::classification::parse_keynotes(&text)?;
    let kept = match dir.as_deref().and_then(|d| path.strip_prefix(d).ok()) {
        Some(rel) => rel.display().to_string(),
        None => path.display().to_string(),
    };
    let n = entries.len();
    app.execute("keynotes.set", json!({ "path": kept, "entries": entries }), caller)?;
    Ok(KeynotesReport { path: kept, entries: n })
}

pub(crate) struct KeynotesLoad;

impl HostCommand for KeynotesLoad {
    type Host = App;
    type Params = KeynotesParams;
    type Output = KeynotesReport;
    const META: CommandMeta = meta(
        "keynotes.load",
        "Load a keynote file",
        "Reads a tab-separated keynote file (`key`, `text`, `parent_key` per line; `#` lines skipped) into the open project, keeping its path relative to the project when it lies beside it (absolute otherwise).",
        &[Effect::FileRead, Effect::Edit],
    );

    fn run(&self, app: &mut App, caller: &Caller, p: KeynotesParams) -> Result<KeynotesReport> {
        load_keynotes(app, caller, &p.path)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

pub(crate) struct KeynotesReload;

impl HostCommand for KeynotesReload {
    type Host = App;
    type Params = Empty;
    type Output = KeynotesReport;
    const META: CommandMeta = meta("keynotes.reload", "Reload the keynote file", "Reads the project's keynote file again (after it changed on disk).", &[Effect::FileRead, Effect::Edit]);

    fn run(&self, app: &mut App, caller: &Caller, _: Empty) -> Result<KeynotesReport> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let path = reviera_core::classification::keynotes(app.document(doc).expect("active").doc.store()).path;
        if path.is_empty() {
            return Err(bad("keynotes.none", "the project has no keynote file; load one first"));
        }
        load_keynotes(app, caller, &path)
    }
}
