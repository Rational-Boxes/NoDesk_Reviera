// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Classification systems and keynotes (spec Ch 2 §2.10.10, DM-CLS-001).
//!
//! - **Systems** (Uniclass, OmniClass, MasterFormat, national tables…) are loaded into the
//!   project (`rxd:classifications` on the project): a name, source, edition, location and the
//!   table of codes, each with its parent and title. Reviera ships no tables; they are the
//!   publishers'.
//! - An element (a type or an occurrence) has at most one code per system
//!   (`rxd:classification`: system id → code). On export each system is an `IfcClassification`,
//!   each code used an `IfcClassificationReference` (its parent its ReferencedSource), associated
//!   with its elements by `IfcRelAssociatesClassification`.
//! - **Keynotes** are a tab-separated table (`key`, `text`, `parent_key`) kept on the project with
//!   the file it came from (`rxd:keynotes`), so keynote values (`type.keynote`) can be looked up.
//!
//! Reading files is an application command (`classification.load`, `keynotes.load`), which hands
//! the parsed table to the commands here.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const SYSTEMS_KEY: &str = "rxd:classifications";
pub const ASSIGN_KEY: &str = "rxd:classification";
pub const KEYNOTES_KEY: &str = "rxd:keynotes";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct System {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub edition: String,
    /// Where it came from (a file or URL).
    #[serde(default)]
    pub location: String,
    pub items: Vec<Item>,
}

impl System {
    pub fn item(&self, code: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.code == code)
    }
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn systems(store: &Store) -> Vec<System> {
    project(store).and_then(|p| p.param(SYSTEMS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn save(txn: &mut Transaction<'_>, key: &str, v: Value) -> Result<()> {
    let p = project(txn.store()).map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.modify(p, |e| {
        e.params.insert(key.into(), v);
    })
}

/// An element's codes, by system id.
pub fn codes(e: &Element) -> BTreeMap<String, String> {
    e.param(ASSIGN_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Reads a `code,parent_code,title` table (a header row, if any, is skipped; fields may be quoted).
pub fn parse_csv(text: &str) -> Result<Vec<Item>> {
    fn fields(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => out.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        out.push(cur);
        out.into_iter().map(|f| f.trim().to_owned()).collect()
    }
    let mut items: Vec<Item> = Vec::new();
    for (k, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let f = fields(line);
        if k == 0 && f.first().is_some_and(|c| c.eq_ignore_ascii_case("code")) {
            continue;
        }
        let code = f.first().cloned().unwrap_or_default();
        if code.is_empty() {
            return Err(bad("classification.csv", format!("line {}: no code", k + 1)));
        }
        let parent = f.get(1).filter(|p| !p.is_empty()).cloned();
        items.push(Item { code, parent, title: f.get(2).cloned().unwrap_or_default() });
    }
    check_items(&items)?;
    Ok(items)
}

fn check_items(items: &[Item]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for i in items {
        if !seen.insert(i.code.as_str()) {
            return Err(bad("classification.duplicate", format!("the code {} is listed twice", i.code)));
        }
    }
    for i in items {
        if let Some(p) = &i.parent
            && !seen.contains(p.as_str())
        {
            return Err(bad("classification.parent", format!("{}'s parent {p} is not in the table", i.code)));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub location: String,
    pub items: Vec<Item>,
}

/// `classification.create`: a system with its table; one of the same name is replaced (its
/// assignments kept where the codes remain). Returns its id.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<String> {
    if p.name.trim().is_empty() {
        return Err(bad("classification.name", "name the classification system"));
    }
    check_items(&p.items)?;
    let mut list = systems(txn.store());
    let id = match list.iter().position(|s| s.name.eq_ignore_ascii_case(p.name.trim())) {
        Some(k) => {
            let id = list[k].id.clone();
            list[k] = System { id: id.clone(), name: p.name.trim().into(), source: p.source, edition: p.edition, location: p.location, items: p.items };
            id
        }
        None => {
            let id = GlobalId::new().to_string();
            list.push(System { id: id.clone(), name: p.name.trim().into(), source: p.source, edition: p.edition, location: p.location, items: p.items });
            id
        }
    };
    save(txn, SYSTEMS_KEY, serde_json::to_value(&list).expect("serialises"))?;
    Ok(id)
}

fn find<'a>(list: &'a [System], r: &str) -> Result<&'a System> {
    list.iter().find(|s| s.id == r || s.name.eq_ignore_ascii_case(r)).ok_or_else(|| bad("classification.not_found", format!("no classification system {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignParams {
    /// Types or occurrences.
    pub element_ids: Vec<ElementRef>,
    /// The system's id or name.
    pub system_id: String,
    pub code: String,
}

/// `classification.assign`: the elements take the code (replacing their code in that system).
pub(crate) fn assign(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    let list = systems(&store);
    let s = find(&list, &p.system_id)?;
    if s.item(&p.code).is_none() {
        return Err(bad("classification.code", format!("{} has no code {:?}", s.name, p.code)));
    }
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        let mut c = codes(store.get(id).expect("resolved"));
        c.insert(s.id.clone(), p.code.clone());
        let v = serde_json::to_value(&c).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(ASSIGN_KEY.into(), v);
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnassignParams {
    pub element_ids: Vec<ElementRef>,
    pub system_id: String,
}

/// `classification.unassign`: the elements lose their code in the system.
pub(crate) fn unassign(txn: &mut Transaction<'_>, p: UnassignParams) -> Result<()> {
    let store = txn.store().clone();
    let list = systems(&store);
    let sid = find(&list, &p.system_id)?.id.clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        let mut c = codes(store.get(id).expect("resolved"));
        if c.remove(&sid).is_none() {
            continue;
        }
        let v = serde_json::to_value(&c).expect("serialises");
        txn.modify(id, |e| {
            if c.is_empty() {
                e.params.remove(ASSIGN_KEY);
            } else {
                e.params.insert(ASSIGN_KEY.into(), v);
            }
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub system_id: String,
}

/// `classification.delete`: the system and every assignment to it.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: DeleteParams) -> Result<usize> {
    let store = txn.store().clone();
    let mut list = systems(&store);
    let sid = find(&list, &p.system_id)?.id.clone();
    list.retain(|s| s.id != sid);
    save(txn, SYSTEMS_KEY, serde_json::to_value(&list).expect("serialises"))?;
    let ids: Vec<ElementId> = store.iter().filter(|e| codes(e).contains_key(&sid)).map(Element::id).collect();
    let n = ids.len();
    unassign_ids(txn, &ids, &sid)?;
    Ok(n)
}

fn unassign_ids(txn: &mut Transaction<'_>, ids: &[ElementId], sid: &str) -> Result<()> {
    for id in ids {
        let mut c = codes(txn.store().get(*id).expect("listed"));
        c.remove(sid);
        let v = serde_json::to_value(&c).expect("serialises");
        txn.modify(*id, |e| {
            if c.is_empty() {
                e.params.remove(ASSIGN_KEY);
            } else {
                e.params.insert(ASSIGN_KEY.into(), v);
            }
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SystemInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    pub edition: String,
    pub location: String,
    pub codes: usize,
    /// Elements classified in it.
    pub used_by: usize,
}

/// `classification.list`.
pub(crate) fn list(store: &Store, _: crate::builtin::NoParams) -> Result<Vec<SystemInfo>> {
    Ok(systems(store)
        .into_iter()
        .map(|s| SystemInfo {
            used_by: store.iter().filter(|e| codes(e).contains_key(&s.id)).count(),
            codes: s.items.len(),
            id: s.id,
            name: s.name,
            source: s.source,
            edition: s.edition,
            location: s.location,
        })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    pub system_id: String,
    /// In the code or title; every item when empty.
    #[serde(default)]
    pub text: String,
    /// At most this many (500 by default).
    pub limit: Option<usize>,
}

/// `classification.search`: a system's codes matching the text, in table order.
pub(crate) fn search(store: &Store, p: SearchParams) -> Result<Vec<Item>> {
    let list = systems(store);
    let s = find(&list, &p.system_id)?;
    let t = p.text.to_lowercase();
    Ok(s.items.iter().filter(|i| t.is_empty() || i.code.to_lowercase().contains(&t) || i.title.to_lowercase().contains(&t)).take(p.limit.unwrap_or(500)).cloned().collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OfParams {
    pub element_id: ElementRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Classified {
    pub system_id: String,
    pub system: String,
    pub code: String,
    pub title: String,
}

/// `classification.of`: an element's codes with their systems and titles.
pub(crate) fn of(store: &Store, p: OfParams) -> Result<Vec<Classified>> {
    let id = store.resolve(&p.element_id).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let list = systems(store);
    Ok(codes(store.get(id).expect("resolved"))
        .into_iter()
        .map(|(sid, code)| {
            let s = list.iter().find(|s| s.id == sid);
            Classified {
                system: s.map(|s| s.name.clone()).unwrap_or_default(),
                title: s.and_then(|s| s.item(&code)).map(|i| i.title.clone()).unwrap_or_default(),
                system_id: sid,
                code,
            }
        })
        .collect())
}

/// What the IFC writer needs: each system with the codes used and the elements using each.
pub fn used(store: &Store) -> Vec<(System, BTreeMap<String, Vec<ElementId>>)> {
    let list = systems(store);
    list.into_iter()
        .map(|s| {
            let mut by: BTreeMap<String, Vec<ElementId>> = BTreeMap::new();
            for e in store.iter() {
                if let Some(c) = codes(e).get(&s.id)
                    && s.item(c).is_some()
                {
                    by.entry(c.clone()).or_default().push(e.id());
                }
            }
            (s, by)
        })
        .collect()
}

// ---- Keynotes ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Keynote {
    pub key: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KeynoteTable {
    /// The file, relative to the project when it was beside it.
    pub path: String,
    pub entries: Vec<Keynote>,
}

/// Reads a tab-separated keynote file: `key`, `text`, `parent_key` per line (lines starting with
/// `#` and blank lines skipped).
pub fn parse_keynotes(text: &str) -> Result<Vec<Keynote>> {
    let mut out: Vec<Keynote> = Vec::new();
    for (k, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let key = f[0].trim();
        if key.is_empty() {
            return Err(bad("keynotes.format", format!("line {}: no key", k + 1)));
        }
        if out.iter().any(|x| x.key == key) {
            return Err(bad("keynotes.duplicate", format!("line {}: the key {key} is listed twice", k + 1)));
        }
        out.push(Keynote { key: key.into(), text: f.get(1).map_or("", |s| s.trim()).into(), parent: f.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_owned) });
    }
    Ok(out)
}

pub fn keynotes(store: &Store) -> KeynoteTable {
    project(store).and_then(|p| p.param(KEYNOTES_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A keynote's text.
pub fn keynote_text(store: &Store, key: &str) -> Option<String> {
    keynotes(store).entries.into_iter().find(|k| k.key == key).map(|k| k.text)
}

/// `keynotes.set`: the project's keynote table (from `keynotes.load`).
pub(crate) fn set_keynotes(txn: &mut Transaction<'_>, p: KeynoteTable) -> Result<()> {
    save(txn, KEYNOTES_KEY, serde_json::to_value(&p).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeynoteListParams {
    /// In the key or text; every entry when empty.
    #[serde(default)]
    pub text: String,
}

/// `keynotes.list`.
pub(crate) fn list_keynotes(store: &Store, p: KeynoteListParams) -> Result<KeynoteTable> {
    let mut t = keynotes(store);
    let q = p.text.to_lowercase();
    t.entries.retain(|k| q.is_empty() || k.key.to_lowercase().contains(&q) || k.text.to_lowercase().contains(&q));
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        let items = parse_csv("code,parent_code,title\nSs,,Systems\nSs_25,Ss,Wall and barrier systems\n\"Ss_25_10\",Ss_25,\"Framed wall systems, ventilated\"\n").unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].title, "Framed wall systems, ventilated");
        assert_eq!(items[1].parent.as_deref(), Some("Ss"));
        assert!(parse_csv("A,B,x\n").is_err(), "an unknown parent");
        let k = parse_keynotes("# keynotes\nA\tGeneral\t\nA10\tConcrete wall\tA\n").unwrap();
        assert_eq!(k[1].parent.as_deref(), Some("A"));
        assert!(parse_keynotes("A\tx\nA\ty\n").is_err());
    }
}
