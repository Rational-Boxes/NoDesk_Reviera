// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Three-way merge of models (spec Ch 6 §6.3, COL-MRG-001): the base **B** a working copy was
//! last synchronised with, the local **L** and the remote **R**, element by element.
//!
//! Reviera merges its own element store rather than the IFC files: every element is a GlobalId
//! with a class, a name and its parameters (properties, IFC attributes, its Reviera recipe and
//! other RXD data), which is what both sides edited and what a model re-serialised by a server
//! reads back as unchanged.
//!
//! - Elements are matched by GlobalId and classified by the table of COL-MRG-001: changed on one
//!   side only → that side's; changed on both → merged **facet by facet** (the class, the name,
//!   each parameter); added on one side → added; deleted on one side and unchanged on the other
//!   → deleted; deleted on one side and changed on the other → a conflict.
//! - A parameter changed on both sides is merged further when it is a JSON object (a recipe, an
//!   RXD record): key by key, recursively. A list of strings (GlobalIds of members, parts) is
//!   merged as a **set**: what each side added is added, what each side removed is removed, in
//!   the local order with the remote additions after. Anything else changed differently on both
//!   sides is a conflict.
//! - Numbers are equal within 1e-9 (relative), so a value written as `3.` and read back as
//!   `3.0` is no change.
//! - Conflicts are resolved by the **policy**: `fail` (report them and change nothing),
//!   `local` or `remote` (that side wins every conflict), or one by one by [`Decisions`] (keep
//!   mine, take theirs, or a value of its own).
//!
//! The result is a [`Plan`] of the creates, updates and deletes that turn L into the merged
//! model; applying it is one transaction. Deterministic: the same B, L, R and policy give the
//! same plan, ordered by GlobalId.
//!
//! Not yet: ownership-aware resolution (§6.4), the exclusivity checks (one container, one type),
//! conflicts decided one by one (COL-MRG-002), and elements whose parts are generated (an
//! enclosure changed on both sides may get both sides' parts: regenerate it after).

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::Result;
use crate::id::GlobalId;
use crate::store::Store;
use crate::txn::Transaction;

/// How conflicts are resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    /// Report them; change nothing.
    #[default]
    Fail,
    /// The local side wins.
    Local,
    /// The remote side wins.
    Remote,
}

/// A decision on one conflict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Keep the local side.
    Mine,
    /// Take the remote side.
    Theirs,
    /// A value of its own (a facet only; JSON null removes it).
    Value(Value),
}

/// Decisions by (GlobalId, facet).
pub type Decisions = BTreeMap<(String, String), Choice>;

/// What kind of conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides changed the same facet differently.
    BothChanged,
    /// Deleted locally, changed remotely.
    DeleteModify,
    /// Changed locally, deleted remotely.
    ModifyDelete,
    /// Both added an element with the same GlobalId, differently.
    BothAdded,
}

/// One conflict: the element, the facet (`class`, `name`, a parameter, or a path inside one as
/// `param/key/key`), and the two sides' values.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Conflict {
    pub guid: String,
    pub class: String,
    pub name: Option<String>,
    pub kind: ConflictKind,
    pub facet: String,
    pub local: Option<Value>,
    pub remote: Option<Value>,
    /// The decision taken on it, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Choice>,
}

impl Conflict {
    /// Its key in [`Decisions`].
    pub fn key(&self) -> (String, String) {
        (self.guid.clone(), self.facet.clone())
    }
}

/// An element's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Snap {
    pub class: String,
    pub name: Option<String>,
    pub params: BTreeMap<String, Value>,
}

impl Snap {
    fn of(e: &Element) -> Snap {
        Snap { class: e.class.clone(), name: e.name.clone(), params: e.params.clone() }
    }
}

/// What turns the local model into the merged one.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Plan {
    pub creates: Vec<(String, Snap)>,
    pub updates: Vec<(String, Snap)>,
    pub deletes: Vec<String>,
    pub conflicts: Vec<Conflict>,
    /// Elements changed on the remote side only, and on both (merged without conflict).
    pub remote_changes: usize,
    pub merged: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.creates.is_empty() && self.updates.is_empty() && self.deletes.is_empty()
    }
}

/// Equal, numbers within 1e-9 relative.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

fn same_opt(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn same_snap(a: &Snap, b: &Snap) -> bool {
    a.class == b.class && a.name == b.name && a.params.len() == b.params.len() && a.params.iter().all(|(k, v)| b.params.get(k).is_some_and(|w| same(v, w)))
}

fn strings(v: &Value) -> Option<Vec<&str>> {
    v.as_array()?.iter().map(Value::as_str).collect()
}

/// The merged value of one facet (absent when `None`), or a conflict at `path`.
fn value(b: Option<&Value>, l: Option<&Value>, r: Option<&Value>, path: &str, out: &mut Vec<(String, Option<Value>, Option<Value>)>, decide: &dyn Fn(&str) -> Choice) -> Option<Value> {
    if same_opt(l, r) || same_opt(r, b) {
        return l.cloned();
    }
    if same_opt(l, b) {
        return r.cloned();
    }
    // Changed on both sides, differently.
    if let (Some(Value::Object(lo)), Some(Value::Object(ro))) = (l, r) {
        let bo = b.and_then(Value::as_object);
        let keys: BTreeSet<&String> = lo.keys().chain(ro.keys()).chain(bo.into_iter().flat_map(|o| o.keys())).collect();
        let mut m = serde_json::Map::new();
        for k in keys {
            if let Some(v) = value(bo.and_then(|o| o.get(k)), lo.get(k), ro.get(k), &format!("{path}/{k}"), out, decide) {
                m.insert(k.clone(), v);
            }
        }
        return Some(Value::Object(m));
    }
    if let (Some(ls), Some(rs)) = (l.and_then(strings), r.and_then(strings)) {
        let bs: BTreeSet<&str> = b.and_then(strings).unwrap_or_default().into_iter().collect();
        let (lset, rset): (BTreeSet<&str>, BTreeSet<&str>) = (ls.iter().copied().collect(), rs.iter().copied().collect());
        let removed: BTreeSet<&str> = bs.iter().filter(|x| !lset.contains(*x) || !rset.contains(*x)).copied().collect();
        let mut merged: Vec<&str> = ls.iter().copied().filter(|x| !removed.contains(x)).collect();
        merged.extend(rs.iter().copied().filter(|x| !lset.contains(x) && !removed.contains(x)));
        return Some(Value::Array(merged.into_iter().map(|s| Value::String(s.to_owned())).collect()));
    }
    out.push((path.to_owned(), l.cloned(), r.cloned()));
    match decide(path) {
        Choice::Theirs => r.cloned(),
        Choice::Mine => l.cloned(),
        Choice::Value(Value::Null) => None,
        Choice::Value(v) => Some(v),
    }
}

/// Merges `b`, `l` and `r` element by element.
pub fn three_way(base: &Store, local: &Store, remote: &Store, policy: Policy) -> Plan {
    three_way_decided(base, local, remote, policy, &Decisions::new())
}

/// Merges as [`three_way`], each conflict with a decision in `decisions` decided so (the
/// others by `policy`); every conflict is still listed, with its decision.
pub fn three_way_decided(base: &Store, local: &Store, remote: &Store, policy: Policy, decisions: &Decisions) -> Plan {
    let index = |s: &Store| -> BTreeMap<String, Snap> { s.iter().map(|e| (e.guid().to_string(), Snap::of(e))).collect() };
    let (bi, li, ri) = (index(base), index(local), index(remote));
    let guids: BTreeSet<&String> = bi.keys().chain(li.keys()).chain(ri.keys()).collect();
    let mut plan = Plan::default();
    for g in guids {
        let (b, l, r) = (bi.get(g), li.get(g), ri.get(g));
        let decide = |facet: &str| decisions.get(&(g.clone(), facet.to_owned())).cloned().unwrap_or(if policy == Policy::Remote { Choice::Theirs } else { Choice::Mine });
        let conflict = |kind, facet: &str, lv: Option<Value>, rv: Option<Value>, s: &Snap| Conflict { guid: g.clone(), class: s.class.clone(), name: s.name.clone(), kind, facet: facet.to_owned(), local: lv, remote: rv, decision: decisions.get(&(g.clone(), facet.to_owned())).cloned() };
        let changed = |x: &Snap| b.is_none_or(|b| !same_snap(b, x));
        // The merged element (None: absent).
        let result: Option<Snap> = match (b, l, r) {
            (_, Some(l), Some(r)) if same_snap(l, r) => Some(l.clone()),
            (Some(_), Some(l), Some(r)) if !changed(r) => Some(l.clone()),
            (Some(_), Some(l), Some(r)) if !changed(l) => {
                plan.remote_changes += 1;
                Some(r.clone())
            }
            (Some(b), Some(l), Some(r)) => {
                // Facet by facet.
                let mut found = Vec::new();
                let class = match value(Some(&Value::String(b.class.clone())), Some(&Value::String(l.class.clone())), Some(&Value::String(r.class.clone())), "class", &mut found, &decide) {
                    Some(Value::String(c)) => c,
                    _ => l.class.clone(),
                };
                let opt = |n: &Option<String>| n.clone().map(Value::String);
                let name = value(opt(&b.name).as_ref(), opt(&l.name).as_ref(), opt(&r.name).as_ref(), "name", &mut found, &decide).and_then(|v| v.as_str().map(str::to_owned));
                let keys: BTreeSet<&String> = b.params.keys().chain(l.params.keys()).chain(r.params.keys()).collect();
                let mut params = BTreeMap::new();
                for k in keys {
                    if let Some(v) = value(b.params.get(k), l.params.get(k), r.params.get(k), k, &mut found, &decide) {
                        params.insert(k.clone(), v);
                    }
                }
                for (facet, lv, rv) in found {
                    plan.conflicts.push(conflict(ConflictKind::BothChanged, &facet, lv, rv, l));
                }
                plan.merged += 1;
                Some(Snap { class, name, params })
            }
            // Added on one side, or on both.
            (None, Some(l), None) => Some(l.clone()),
            (None, None, Some(r)) => {
                plan.remote_changes += 1;
                Some(r.clone())
            }
            (None, Some(l), Some(r)) => {
                plan.conflicts.push(conflict(ConflictKind::BothAdded, "element", serde_json::to_value(l).ok(), serde_json::to_value(r).ok(), l));
                Some(if decide("element") == Choice::Theirs { r.clone() } else { l.clone() })
            }
            // Deleted on one side.
            (Some(_), None, None) | (None, None, None) => None,
            (Some(b), None, Some(r)) => {
                if same_snap(b, r) {
                    None
                } else {
                    plan.conflicts.push(conflict(ConflictKind::DeleteModify, "element", None, serde_json::to_value(r).ok(), r));
                    (decide("element") == Choice::Theirs).then(|| r.clone())
                }
            }
            (Some(b), Some(l), None) => {
                if same_snap(b, l) {
                    plan.remote_changes += 1;
                    None
                } else {
                    plan.conflicts.push(conflict(ConflictKind::ModifyDelete, "element", serde_json::to_value(l).ok(), None, l));
                    (decide("element") != Choice::Theirs).then(|| l.clone())
                }
            }
        };
        match (l, result) {
            (None, Some(s)) => plan.creates.push((g.clone(), s)),
            (Some(_), None) => plan.deletes.push(g.clone()),
            (Some(l), Some(s)) if !same_snap(l, &s) => plan.updates.push((g.clone(), s)),
            _ => {}
        }
    }
    plan
}

/// An element in a change set.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Changed {
    pub guid: String,
    pub class: String,
    pub name: Option<String>,
    /// For a modified element: the facets that differ (`class`, `name`, parameter names).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<String>,
}

/// What changed from one model to another (COL-BIM-008).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ChangeSet {
    pub added: Vec<Changed>,
    pub deleted: Vec<Changed>,
    pub modified: Vec<Changed>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.deleted.is_empty() && self.modified.is_empty()
    }
}

/// The changes from `a` to `b`, by GlobalId, ordered by GlobalId.
pub fn diff(a: &Store, b: &Store) -> ChangeSet {
    let index = |s: &Store| -> BTreeMap<String, Snap> { s.iter().map(|e| (e.guid().to_string(), Snap::of(e))).collect() };
    let (ai, bi) = (index(a), index(b));
    let item = |g: &String, s: &Snap, facets: Vec<String>| Changed { guid: g.clone(), class: s.class.clone(), name: s.name.clone(), facets };
    let mut out = ChangeSet::default();
    for g in ai.keys().chain(bi.keys()).collect::<BTreeSet<_>>() {
        match (ai.get(g), bi.get(g)) {
            (None, Some(y)) => out.added.push(item(g, y, Vec::new())),
            (Some(x), None) => out.deleted.push(item(g, x, Vec::new())),
            (Some(x), Some(y)) if !same_snap(x, y) => {
                let mut facets = Vec::new();
                if x.class != y.class {
                    facets.push("class".to_owned());
                }
                if x.name != y.name {
                    facets.push("name".to_owned());
                }
                for k in x.params.keys().chain(y.params.keys()).collect::<BTreeSet<_>>() {
                    if !same_opt(x.params.get(k), y.params.get(k)) {
                        facets.push(k.clone());
                    }
                }
                out.modified.push(item(g, y, facets));
            }
            _ => {}
        }
    }
    out
}

/// What applying a plan did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Applied {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    /// Elements that could not be brought back (deleted here earlier in this session).
    pub skipped: Vec<String>,
}

/// Applies `plan` to the model in `txn`.
pub fn apply(txn: &mut Transaction<'_>, plan: &Plan) -> Result<Applied> {
    let mut out = Applied::default();
    for g in &plan.deletes {
        if let Some(id) = GlobalId::parse(g).and_then(|g| txn.store().find_by_guid(&g)) {
            txn.delete(id)?;
            out.deleted += 1;
        }
    }
    for (g, s) in &plan.updates {
        let Some(id) = GlobalId::parse(g).and_then(|g| txn.store().find_by_guid(&g)) else { continue };
        let s = s.clone();
        txn.modify(id, |e| {
            e.class = s.class;
            e.name = s.name;
            e.params = s.params;
        })?;
        out.updated += 1;
    }
    for (g, s) in &plan.creates {
        let Some(guid) = GlobalId::parse(g) else { continue };
        let mut n = NewElement::new(&s.class).guid(guid);
        n.name = s.name.clone();
        n.params = s.params.clone();
        match txn.create(n) {
            Ok(_) => out.created += 1,
            Err(e) if e.code == "element.duplicate_guid" => out.skipped.push(g.clone()),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_merge_by_key_and_as_sets() {
        let mut c = Vec::new();
        // A recipe: the height changed here, the thickness there.
        let b = json!({ "height": 3.0, "thickness": 0.2, "kind": "w" });
        let l = json!({ "height": 3.5, "thickness": 0.2, "kind": "w" });
        let r = json!({ "height": 3.0, "thickness": 0.25, "kind": "w" });
        assert_eq!(value(Some(&b), Some(&l), Some(&r), "rxd", &mut c, &|_| Choice::Mine), Some(json!({ "height": 3.5, "thickness": 0.25, "kind": "w" })));
        assert!(c.is_empty());
        // Members: each side added one and one removed another.
        let b = json!(["a", "b", "c"]);
        let l = json!(["a", "b", "c", "l"]);
        let r = json!(["a", "c", "r"]);
        assert_eq!(value(Some(&b), Some(&l), Some(&r), "parts", &mut c, &|_| Choice::Mine), Some(json!(["a", "c", "l", "r"])));
        // The same number written differently is no change; a real clash is a conflict.
        assert!(same(&json!(3), &json!(3.0000000000001)));
        assert_eq!(value(Some(&json!(1)), Some(&json!(2)), Some(&json!(3)), "x", &mut c, &|_| Choice::Theirs), Some(json!(3)));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn a_diff_lists_what_was_added_deleted_and_changed() {
        let mut a = crate::document::Document::new();
        let mut t = a.transaction("x", crate::Origin::ui());
        let w = t.create(NewElement::new("IfcWall").name("W").param("Pset_WallCommon.FireRating", json!("1h"))).unwrap();
        let d = t.create(NewElement::new("IfcDoor")).unwrap();
        t.commit();
        let before = a.store().clone();
        let mut t = a.transaction("y", crate::Origin::ui());
        t.modify(w, |e| {
            e.params.insert("Pset_WallCommon.FireRating".into(), json!("2h"));
        })
        .unwrap();
        t.delete(d).unwrap();
        t.create(NewElement::new("IfcWindow")).unwrap();
        t.commit();
        let c = diff(&before, a.store());
        assert_eq!((c.added.len(), c.deleted.len(), c.modified.len()), (1, 1, 1));
        assert_eq!(c.modified[0].facets, vec!["Pset_WallCommon.FireRating".to_owned()]);
        assert_eq!((c.added[0].class.as_str(), c.deleted[0].class.as_str()), ("IfcWindow", "IfcDoor"));
        assert!(diff(&before, &before).is_empty());
    }
}
