// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reinforcement numbering and partitions (STR-RBR-016).
//!
//! Every rebar set and fabric sheet (placed alone, or laid by a fabric area) has a **Rebar
//! Number**. Elements identical in bar or sheet type, shape and segment values after rounding
//! share a number within their **partition** (free text on the element, `rxd:rebar_partition`;
//! a fabric area's sheets take the area's). Rebar and fabric have their own sequences per
//! partition (couplers will have theirs).
//!
//! A sequence (`rxd:rebar_numbers` on the project) keeps each identity's number, a start number
//! and the locked numbers. An identity met for the first time takes the next free number in
//! creation order and keeps it: numbers do not move when other elements go, so gaps can
//! appear, and an element whose identity changes takes its new identity's number. *Remove gaps*
//! closes them in number order; *Renumber* numbers again in creation order; both keep locked
//! numbers and skip them.
//!
//! The number is written to the bar's or sheet's IFC `Tag`.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::rebar;
use crate::rebar_systems as rs;
use crate::store::Store;

pub const PARTITION_KEY: &str = "rxd:rebar_partition";
pub const NUMBERS_KEY: &str = "rxd:rebar_numbers";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Rebar,
    Fabric,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sequence {
    #[serde(default = "one")]
    pub start: u32,
    #[serde(default)]
    pub numbers: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub locked: BTreeSet<String>,
}

fn one() -> u32 {
    1
}

type Sequences = BTreeMap<String, BTreeMap<Kind, Sequence>>;

fn stored(store: &Store) -> Sequences {
    store
        .iter()
        .find(|e| e.class == "IfcProject")
        .and_then(|p| p.param(NUMBERS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// An element's partition ("" for none).
pub fn partition(e: &Element) -> String {
    e.param(PARTITION_KEY).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn mm(store: &Store, v: f64) -> i64 {
    let s = rebar::settings(store);
    let v = if s.rounding.on { s.rounding.round(v, s.rounding.segment_length) } else { v };
    (v * 1000.0).round() as i64
}

/// One numbered item: a set, a sheet, or one sheet of a fabric area.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Item {
    pub element: ElementId,
    /// A fabric area's sheet, by its place in the area's list.
    pub sheet: Option<usize>,
    /// A varying set's length group, by its place in the set's groups (STR-RBR-005).
    pub group: Option<usize>,
    pub kind: Kind,
    pub partition: String,
    pub identity: String,
    /// A short description: bar type, shape and dimensions, or sheet type.
    pub summary: String,
}

/// Every numbered item, in creation order.
pub fn items(store: &Store) -> Vec<Item> {
    let mut out = Vec::new();
    let mut ids: Vec<&Element> = store.iter().filter(|e| crate::rebar_views::is_reinforcement(e)).collect();
    ids.sort_by_key(|e| e.id());
    for e in ids {
        let part = partition(e);
        if let Some(r) = rebar::rebar_recipe(e) {
            let Ok(g) = rebar::geometry(store, &r) else { continue };
            let dims: Vec<String> = g.dimensions.iter().map(|(k, v)| format!("{k}{}", mm(store, *v))).collect();
            let bar = rebar::bar_types(store).into_iter().find(|b| b.id == r.bar_type).map_or(r.bar_type.clone(), |b| b.name);
            // Each length of a varying set is its own identity, as a standard set of that
            // length would be.
            if r.varying {
                for (k, (l, _)) in rebar::length_groups(store, &g).into_iter().enumerate() {
                    let dims = format!("A{}", mm(store, l));
                    out.push(Item {
                        element: e.id(),
                        sheet: None,
                        group: Some(k),
                        kind: Kind::Rebar,
                        partition: part.clone(),
                        identity: format!("{}|{}|{dims}", r.bar_type, r.shape),
                        summary: format!("{bar} shape {} {dims} (varying)", r.shape),
                    });
                }
                continue;
            }
            out.push(Item {
                element: e.id(),
                sheet: None,
                group: None,
                kind: Kind::Rebar,
                partition: part,
                identity: format!("{}|{}|{}", r.bar_type, r.shape, dims.join(" ")),
                summary: format!("{bar} shape {} {}", r.shape, dims.join(" ")),
            });
        } else if let Some(identity) = crate::precast_fabric::identity(store, e) {
            // A custom sheet: identical sheets (wires, lengths, places) share a number.
            out.push(Item { element: e.id(), sheet: None, group: None, kind: Kind::Fabric, partition: part.clone(), identity, summary: "custom sheet".into() });
        } else if let Some((t, sheets)) = sheets_of(store, e) {
            for (k, s) in sheets.iter().enumerate() {
                // A cut sheet is told apart by its wires' lengths.
                let shape = if s.cut {
                    let l: Vec<String> = s.major.iter().chain(&s.minor).map(|w| mm(store, w.length()).to_string()).collect();
                    format!("cut {:016x}", fxhash(&l.join(",")))
                } else {
                    "whole".to_owned()
                };
                out.push(Item {
                    element: e.id(),
                    sheet: rs::fabric_area_recipe(e).is_some().then_some(k),
                    group: None,
                    kind: Kind::Fabric,
                    partition: part.clone(),
                    identity: format!("{t}|{shape}"),
                    summary: format!("{t} {}", if s.cut { "cut" } else { "whole" }),
                });
            }
        }
    }
    out
}

fn sheets_of(store: &Store, e: &Element) -> Option<(String, Vec<rs::Sheet>)> {
    if let Some(r) = rs::fabric_area_recipe(e) {
        return Some((r.sheet_type.clone(), rs::fabric_area_sheets(store, &r).ok()?));
    }
    let r = rs::fabric_sheet_recipe(e)?;
    Some((r.sheet_type.clone(), vec![rs::fabric_sheet(store, &r).ok()?]))
}

fn fxhash(s: &str) -> u64 {
    // FNV-1a: stable across runs and platforms.
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The sequences with every identity in use numbered: stored numbers kept, new identities the
/// next free numbers from the start, in creation order.
fn complete(store: &Store, items: &[Item]) -> Sequences {
    let mut seqs = stored(store);
    for i in items {
        let seq = seqs.entry(i.partition.clone()).or_default().entry(i.kind).or_insert_with(|| Sequence { start: 1, ..Sequence::default() });
        if seq.numbers.contains_key(&i.identity) {
            continue;
        }
        let used: BTreeSet<u32> = seq.numbers.values().copied().collect();
        let mut n = seq.start.max(1);
        while used.contains(&n) {
            n += 1;
        }
        seq.numbers.insert(i.identity.clone(), n);
    }
    seqs
}

/// Each item's number.
pub fn numbers(store: &Store) -> Vec<(Item, u32)> {
    let items = items(store);
    let seqs = complete(store, &items);
    items
        .into_iter()
        .map(|i| {
            let n = seqs[&i.partition][&i.kind].numbers[&i.identity];
            (i, n)
        })
        .collect()
}

/// The number of an element (a set or single sheet), or of each of a fabric area's sheets.
pub fn number_of(store: &Store, id: ElementId) -> Vec<u32> {
    numbers(store).into_iter().filter(|(i, _)| i.element == id).map(|(_, n)| n).collect()
}

/// Keeps the numbers of identities now in use, so later changes elsewhere do not move them.
pub(crate) fn persist(txn: &mut crate::txn::Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    let seqs = complete(&store, &items(&store));
    if seqs == stored(&store) {
        return Ok(());
    }
    save(txn, &seqs)
}

fn save(txn: &mut crate::txn::Transaction<'_>, seqs: &Sequences) -> Result<()> {
    let store = txn.store().clone();
    let pid = store.iter().find(|e| e.class == "IfcProject").map(|e| e.id()).ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.set_param(pid, NUMBERS_KEY, serde_json::to_value(seqs).expect("serialises"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PartitionParams {
    pub ids: Vec<ElementRef>,
    /// The partition; empty for none.
    pub name: String,
}

/// Puts rebar sets, sheets and fabric areas in a partition (`rebar.set_partition`).
pub(crate) fn set_partition(txn: &mut crate::txn::Transaction<'_>, p: PartitionParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let numbered = rebar::rebar_recipe(e).is_some() || rs::fabric_area_recipe(e).is_some() || rs::fabric_sheet_recipe(e).is_some();
        if !numbered {
            return Err(Error::new("rebar.not_numbered", format!("element {id} is not a rebar set, fabric sheet or fabric area")));
        }
        let name = p.name.trim();
        txn.set_param(id, PARTITION_KEY, if name.is_empty() { Value::Null } else { json!(name) })?;
    }
    persist(txn)
}

/// One row of a sequence: a number with its identity and elements.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Row {
    pub number: u32,
    pub identity: String,
    pub summary: String,
    /// Items with this number (sets, sheets).
    pub count: usize,
    pub element_ids: Vec<ElementId>,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SequenceInfo {
    pub partition: String,
    pub kind: Kind,
    pub start: u32,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListParams {
    /// One partition ("" for none); every one when omitted.
    #[serde(default)]
    pub partition: Option<String>,
}

/// The sequences with the numbers in use (`reinforcement_numbering.list`).
pub fn list(store: &Store, p: ListParams) -> Vec<SequenceInfo> {
    let items = items(store);
    let seqs = complete(store, &items);
    let mut out = Vec::new();
    for (part, kinds) in &seqs {
        if p.partition.as_ref().is_some_and(|q| q != part) {
            continue;
        }
        for (kind, seq) in kinds {
            let mut rows: Vec<Row> = Vec::new();
            for i in items.iter().filter(|i| &i.partition == part && i.kind == *kind) {
                match rows.iter_mut().find(|r| r.identity == i.identity) {
                    Some(r) => {
                        r.count += 1;
                        if !r.element_ids.contains(&i.element) {
                            r.element_ids.push(i.element);
                        }
                    }
                    None => rows.push(Row {
                        number: seq.numbers[&i.identity],
                        identity: i.identity.clone(),
                        summary: i.summary.clone(),
                        count: 1,
                        element_ids: vec![i.element],
                        locked: seq.locked.contains(&i.identity),
                    }),
                }
            }
            if rows.is_empty() {
                continue;
            }
            rows.sort_by_key(|r| r.number);
            out.push(SequenceInfo { partition: part.clone(), kind: *kind, start: seq.start, rows });
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RenumberParams {
    /// "" for none.
    #[serde(default)]
    pub partition: String,
    pub kind: Kind,
    #[serde(default)]
    pub start: Option<u32>,
    /// Close the gaps in number order instead of numbering again in creation order.
    #[serde(default)]
    pub remove_gaps: bool,
}

/// Numbers a sequence again (`reinforcement_numbering.renumber`): in creation order, or closing
/// the gaps in number order; locked numbers stay and are skipped. Numbers of identities no
/// longer in use are dropped.
pub(crate) fn renumber(txn: &mut crate::txn::Transaction<'_>, p: RenumberParams) -> Result<()> {
    let store = txn.store().clone();
    let items = items(&store);
    let mut seqs = complete(&store, &items);
    let seq = seqs.entry(p.partition.clone()).or_default().entry(p.kind).or_insert_with(|| Sequence { start: 1, ..Sequence::default() });
    if let Some(s) = p.start {
        if s == 0 {
            return Err(Error::new("reinforcement_numbering.start", "a start number is 1 or more"));
        }
        seq.start = s;
    }
    // Identities in use, in the order they will be numbered.
    let mut order: Vec<String> = Vec::new();
    for i in items.iter().filter(|i| i.partition == p.partition && i.kind == p.kind) {
        if !order.contains(&i.identity) {
            order.push(i.identity.clone());
        }
    }
    if p.remove_gaps {
        order.sort_by_key(|id| seq.numbers.get(id).copied().unwrap_or(u32::MAX));
    }
    let locked: BTreeMap<String, u32> = seq.locked.iter().filter_map(|id| seq.numbers.get(id).map(|n| (id.clone(), *n))).collect();
    let taken: BTreeSet<u32> = locked.values().copied().collect();
    let mut numbers: BTreeMap<String, u32> = locked.clone();
    let mut n = seq.start;
    for id in order {
        if locked.contains_key(&id) {
            continue;
        }
        while taken.contains(&n) {
            n += 1;
        }
        numbers.insert(id, n);
        n += 1;
    }
    seq.numbers = numbers;
    save(txn, &seqs)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetNumberParams {
    #[serde(default)]
    pub partition: String,
    pub kind: Kind,
    pub identity: String,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub locked: Option<bool>,
}

/// Gives an identity a number or locks it (`reinforcement_numbering.set`); a number another
/// identity in use has in the sequence is refused, naming it.
pub(crate) fn set_number(txn: &mut crate::txn::Transaction<'_>, p: SetNumberParams) -> Result<()> {
    let store = txn.store().clone();
    let items = items(&store);
    let mut seqs = complete(&store, &items);
    let seq = seqs
        .get_mut(&p.partition)
        .and_then(|k| k.get_mut(&p.kind))
        .filter(|s| s.numbers.contains_key(&p.identity))
        .ok_or_else(|| Error::new("reinforcement_numbering.unknown", format!("no {:?} identity {} in partition \"{}\"", p.kind, p.identity, p.partition)))?;
    if let Some(n) = p.number {
        if n == 0 {
            return Err(Error::new("reinforcement_numbering.number", "a number is 1 or more"));
        }
        let in_use: BTreeSet<&str> = items.iter().filter(|i| i.partition == p.partition && i.kind == p.kind).map(|i| i.identity.as_str()).collect();
        if let Some((other, _)) = seq.numbers.iter().find(|(id, m)| **m == n && **id != p.identity && in_use.contains(id.as_str())) {
            return Err(Error::new("reinforcement_numbering.taken", format!("number {n} belongs to another identity ({other})"))
                .with_details(json!({ "identity": other })));
        }
        // A number left by an identity no longer in use is free again.
        seq.numbers.retain(|id, m| *m != n || id == &p.identity);
        seq.numbers.insert(p.identity.clone(), n);
    }
    if let Some(l) = p.locked {
        if l {
            seq.locked.insert(p.identity.clone());
        } else {
            seq.locked.remove(&p.identity);
        }
    }
    save(txn, &seqs)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RowEdit {
    pub identity: String,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub locked: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyParams {
    #[serde(default)]
    pub partition: String,
    pub kind: Kind,
    #[serde(default)]
    pub start: Option<u32>,
    #[serde(default)]
    pub rows: Vec<RowEdit>,
    /// Then close the gaps, or number again in creation order.
    #[serde(default)]
    pub remove_gaps: bool,
    #[serde(default)]
    pub renumber: bool,
}

/// The Reinforcement Numbers dialog's edits in one step (`reinforcement_numbering.apply`):
/// numbers and locks row by row, then the start number, then Remove gaps or Renumber.
pub(crate) fn apply(txn: &mut crate::txn::Transaction<'_>, p: ApplyParams) -> Result<()> {
    for r in &p.rows {
        set_number(
            txn,
            SetNumberParams { partition: p.partition.clone(), kind: p.kind, identity: r.identity.clone(), number: r.number, locked: r.locked },
        )?;
    }
    if p.start.is_some() || p.remove_gaps || p.renumber {
        renumber(txn, RenumberParams { partition: p.partition.clone(), kind: p.kind, start: p.start, remove_gaps: p.remove_gaps && !p.renumber })?;
    }
    Ok(())
}
