// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Saved selections (spec Ch 3 §3.14, ARC-SEL-002): named sets of elements, kept on the project
//! (`rxd:selection_sets`) as lists of GlobalIds. Deleted members drop out silently; loading a set
//! changes only the session's selection.
//!
//! With the project option *Selection sets as IfcGroup* (`selectionset.set_export`), each set is
//! also written on save as an `IfcGroup` (ObjectType "SelectionSet") whose
//! `IfcRelAssignsToGroup` lists its members; it is not read back as an element.
//!
//! Not built yet: the shared name space with view filters (there are no view filters yet), sets
//! as view filters.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:selection_sets";
/// On the project: whether sets are exported as `IfcGroup`s.
pub const EXPORT_KEY: &str = "rxd:selection_sets_as_groups";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SelectionSet {
    pub id: String,
    pub name: String,
    /// GlobalIds.
    pub members: Vec<String>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn all(store: &Store) -> Vec<SelectionSet> {
    project(store)
        .and_then(|p| p.param(KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn write(txn: &mut Transaction<'_>, list: &[SelectionSet]) -> Result<()> {
    let p = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(list).expect("serialises");
    txn.modify(p, |e| {
        e.params.insert(KEY.into(), v);
    })
}

fn find<'a>(list: &'a mut [SelectionSet], r: &str) -> Result<&'a mut SelectionSet> {
    list.iter_mut()
        .find(|s| s.id == r || s.name == r)
        .ok_or_else(|| Error::new("selectionset.not_found", format!("no selection set {r}")))
}

/// The members that still exist.
fn living(store: &Store, s: &SelectionSet) -> Vec<ElementId> {
    s.members
        .iter()
        .filter_map(|g| store.find_by_guid(&GlobalId::parse(g)?))
        .collect()
}

fn guids(store: &Store, ids: &[ElementRef]) -> Result<Vec<String>> {
    ids.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            // Members of a group instance join as their group.
            if let Some(g) = crate::groups::instance_of(e) {
                return Ok(g);
            }
            Ok(e.guid().to_string())
        })
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    pub name: String,
    pub element_ids: Vec<ElementRef>,
    /// Replace a set of that name (else a taken name is refused).
    #[serde(default)]
    pub replace: bool,
}

/// `selectionset.save`: its key.
pub(crate) fn save(txn: &mut Transaction<'_>, p: SaveParams) -> Result<String> {
    let store = txn.store().clone();
    let mut list = all(&store);
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(Error::new(
            "selectionset.name",
            "a selection set needs a name",
        ));
    }
    let mut members = guids(&store, &p.element_ids)?;
    members.sort();
    members.dedup();
    if let Some(s) = list.iter_mut().find(|s| s.name == name) {
        if !p.replace {
            return Err(Error::new(
                "selectionset.name_taken",
                format!("a selection set is called {name}; replace it or choose another name"),
            ));
        }
        s.members = members;
        let id = s.id.clone();
        write(txn, &list)?;
        return Ok(id);
    }
    let id = GlobalId::new().to_string();
    list.push(SelectionSet {
        id: id.clone(),
        name,
        members,
    });
    write(txn, &list)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// The set's key or name.
    pub id: String,
}

/// `selectionset.load`: the members that still exist (the caller selects them).
pub(crate) fn load(store: &Store, p: SetParams) -> Result<Vec<ElementId>> {
    let mut list = all(store);
    let s = find(&mut list, &p.id)?;
    Ok(living(store, s))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub id: String,
    #[serde(default)]
    pub add: Vec<ElementRef>,
    #[serde(default)]
    pub remove: Vec<ElementRef>,
}

/// `selectionset.edit`.
pub(crate) fn edit(txn: &mut Transaction<'_>, p: EditParams) -> Result<()> {
    let store = txn.store().clone();
    let mut list = all(&store);
    let add = guids(&store, &p.add)?;
    let remove = guids(&store, &p.remove)?;
    let s = find(&mut list, &p.id)?;
    for g in add {
        if !s.members.contains(&g) {
            s.members.push(g);
        }
    }
    s.members.retain(|g| !remove.contains(g));
    write(txn, &list)
}

/// `selectionset.delete`.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let mut list = all(txn.store());
    let id = find(&mut list, &p.id)?.id.clone();
    list.retain(|s| s.id != id);
    write(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub id: String,
    pub name: String,
}

/// `selectionset.rename`.
pub(crate) fn rename(txn: &mut Transaction<'_>, p: RenameParams) -> Result<()> {
    let mut list = all(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || list.iter().any(|s| s.name == name) {
        return Err(Error::new("selectionset.name_taken", "that name is taken"));
    }
    find(&mut list, &p.id)?.name = name;
    write(txn, &list)
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SetInfo {
    pub id: String,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

/// `selectionset.list`: by name, with the members that still exist counted.
pub(crate) fn list(store: &Store, _: ListParams) -> Result<Vec<SetInfo>> {
    let mut out: Vec<SetInfo> = all(store)
        .iter()
        .map(|s| SetInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            count: living(store, s).len(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The members of every set that still exist, by set (for export).
pub fn resolved(store: &Store) -> Vec<(SelectionSet, Vec<ElementId>)> {
    all(store).into_iter().map(|s| {
        let m = living(store, &s);
        (s, m)
    }).collect()
}

/// Whether sets are exported as `IfcGroup`s.
pub fn exported(store: &Store) -> bool {
    project(store).and_then(|p| p.param(EXPORT_KEY)).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// Write each set as an `IfcGroup` (ObjectType "SelectionSet") on save.
    pub as_ifc_group: bool,
}

/// `selectionset.set_export`: the project option *Selection sets as IfcGroup*.
pub(crate) fn set_export(txn: &mut Transaction<'_>, p: ExportParams) -> Result<()> {
    let id = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.modify(id, |e| {
        if p.as_ifc_group {
            e.params.insert(EXPORT_KEY.into(), serde_json::Value::Bool(true));
        } else {
            e.params.remove(EXPORT_KEY);
        }
    })
}
