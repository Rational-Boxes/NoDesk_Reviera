// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Publish sets (COL-FE-015): outputs a project publishes together to a FileEngine folder, kept
//! with the model. Each output is a command that writes a file, its arguments apart from the
//! destination, and the file's name; `publish.run` (in the application) runs them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::store::Store;
use crate::txn::Transaction;

/// The element holding the model's publish sets.
pub const CLASS: &str = "rxd:project/publish_sets";
pub const KEY: &str = "rxd:publish_sets";

/// One output of a set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Output {
    /// A command that writes a file (`schedule.export`, `export.cobie`, `bcf.export`, …).
    pub command: String,
    /// Its arguments, apart from the destination.
    #[serde(default)]
    pub args: Map<String, Value>,
    /// The file's name in the folder (ignored for a folder of outputs).
    pub name: String,
}

/// A publish set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishSet {
    pub id: String,
    pub name: String,
    pub connection_id: String,
    pub folder_uid: String,
    /// The folder's name, as shown when the set was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_name: Option<String>,
    pub outputs: Vec<Output>,
}

/// The model's publish sets.
pub fn sets(store: &Store) -> Vec<PublishSet> {
    store.iter().find(|e| e.class == CLASS).and_then(|e| serde_json::from_value(e.param(KEY)?.clone()).ok()).unwrap_or_default()
}

fn store_sets(txn: &mut Transaction<'_>, sets: &[PublishSet]) -> Result<()> {
    let value = serde_json::to_value(sets).expect("serialises");
    let existing = txn.store().iter().find(|e| e.class == CLASS).map(Element::id);
    match existing {
        Some(id) => txn.modify(id, |e| {
            e.params.insert(KEY.into(), value);
        }),
        None => txn.create(NewElement::new(CLASS).name("Publish Sets").param(KEY, value)).map(|_| ()),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    /// Replaces the set with this id; omitted, a new set.
    #[serde(default)]
    pub set_id: Option<String>,
    pub name: String,
    pub connection_id: String,
    pub folder_uid: String,
    #[serde(default)]
    pub folder_name: Option<String>,
    pub outputs: Vec<Output>,
}

/// `publish.save_set`.
pub(crate) fn save(txn: &mut Transaction<'_>, p: SaveParams) -> Result<PublishSet> {
    if p.name.trim().is_empty() {
        return Err(Error::new("publish.invalid", "a publish set needs a name"));
    }
    if p.outputs.is_empty() {
        return Err(Error::new("publish.invalid", "a publish set needs at least one output"));
    }
    if let Some(o) = p.outputs.iter().find(|o| o.name.trim().is_empty() || o.name.contains('/')) {
        return Err(Error::new("publish.invalid", format!("{}: each output needs a file name without '/'", o.command)));
    }
    let mut all = sets(txn.store());
    let set = PublishSet {
        id: p.set_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: p.name.trim().to_owned(),
        connection_id: p.connection_id,
        folder_uid: p.folder_uid,
        folder_name: p.folder_name,
        outputs: p.outputs,
    };
    match all.iter_mut().find(|s| s.id == set.id) {
        Some(s) => *s = set.clone(),
        None if p.set_id.is_some() => return Err(Error::new("publish.not_found", "no such publish set")),
        None => all.push(set.clone()),
    }
    store_sets(txn, &all)?;
    Ok(set)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// `publish.list_sets`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<PublishSet>> {
    Ok(sets(store))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub set_id: String,
}

/// `publish.remove_set`.
pub(crate) fn remove(txn: &mut Transaction<'_>, p: RemoveParams) -> Result<usize> {
    let mut all = sets(txn.store());
    let before = all.len();
    all.retain(|s| s.id != p.set_id);
    if all.len() == before {
        return Err(Error::new("publish.not_found", "no such publish set"));
    }
    store_sets(txn, &all)?;
    Ok(before - all.len())
}
