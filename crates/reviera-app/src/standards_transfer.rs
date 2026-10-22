// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Transfer Project Standards (spec Ch 2 §2.10.9, DM-XFER-001): copies types and settings from
//! another open project into the active one.
//!
//! - **Kinds**: every element type (walls, floors, roofs, ceilings, stairs, railings, curtain
//!   walls, ducts, pipes, components…; one kind per class, each type an item) and the settings
//!   documents: project units, line weights, object styles, project parameters and global
//!   parameters (item by item), classification systems (item by item), keynotes, building and
//!   space types, MEP, electrical and rebar settings, colour fill schemes, area schemes, tag
//!   settings, the building code, structural load settings, COBie settings and custom
//!   connection types.
//! - **Dependencies**: a type's parameters are searched for GlobalIds of other types in the
//!   source (a stacked wall's row types, a curtain wall's panel and mullion types…); each comes
//!   along, matched in the target by GlobalId, then by class and name, before it is copied.
//!   Copies get new GlobalIds and the references among them are rewritten.
//! - **Conflicts** (a type of the same class and name, a list item of the same key or name):
//!   `overwrite` replaces the target's values and keeps its GlobalId, so references in the target
//!   stay valid; `new_only` skips it; `rename` copies it under a new name.
//! - The changes are applied to the target by `standards.apply`, one undo step.
//!
//! Not built yet: materials with their assets and fill patterns, line patterns, dimension and
//! text types, view templates and filters, phases, print and export setups and title blocks as
//! transferable kinds (they are not project elements yet).

use std::collections::{BTreeMap, BTreeSet};

use reviera_core::{Caller, CommandMeta, Effect, Element, ElementId, Error, GlobalId, HostCommand, Result, Store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{App, DocumentId};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A settings document: kind, label, holder (as `standards.apply` takes it), key, and the field
/// that identifies list items (none: the document is one item).
struct Doc {
    kind: &'static str,
    label: &'static str,
    holder: &'static str,
    key: &'static str,
    item_key: Option<(&'static str, &'static str)>,
}

const DOCS: &[Doc] = &[
    Doc { kind: "project_units", label: "Project units", holder: "project_settings", key: "units", item_key: None },
    Doc { kind: "project_parameters", label: "Project parameters", holder: "parameters", key: "parameters", item_key: Some(("guid", "name")) },
    Doc { kind: "global_parameters", label: "Global parameters", holder: "parameters", key: "globals", item_key: Some(("id", "name")) },
    Doc { kind: "classifications", label: "Classification systems", holder: "project", key: "rxd:classifications", item_key: Some(("id", "name")) },
    Doc { kind: "keynotes", label: "Keynotes", holder: "project", key: "rxd:keynotes", item_key: None },
    Doc { kind: "space_types", label: "Building and space types", holder: "project", key: "rxd:space_types", item_key: None },
    Doc { kind: "mep_settings", label: "MEP settings", holder: "project", key: "rxd:mep_settings", item_key: None },
    Doc { kind: "electrical_settings", label: "Electrical settings", holder: "project", key: "rxd:electrical", item_key: None },
    Doc { kind: "rebar_settings", label: "Reinforcement settings", holder: "project", key: "rxd:rebar", item_key: None },
    Doc { kind: "color_schemes", label: "Colour fill schemes", holder: "project", key: "rxd:color_schemes", item_key: None },
    Doc { kind: "area_schemes", label: "Area schemes", holder: "project", key: "rxd:area_schemes", item_key: None },
    Doc { kind: "tag_settings", label: "Tag settings", holder: "project", key: "rxd:tags", item_key: None },
    Doc { kind: "building_code", label: "Building code", holder: "project", key: "rxd:building_code", item_key: None },
    Doc { kind: "load_settings", label: "Structural load settings", holder: "project", key: "rxd:analysis", item_key: None },
    Doc { kind: "cobie_settings", label: "COBie settings", holder: "project", key: "rxd:cobie", item_key: None },
    Doc { kind: "custom_connection_types", label: "Custom connection types", holder: "project", key: "rxd:custom_connection_types", item_key: None },
];

/// Standards elements copied whole (all their parameters).
const ELEMENT_DOCS: &[(&str, &str, &str)] = &[
    ("line_weights", "Line weights", reviera_core::standards::LINE_WEIGHTS_CLASS),
    ("object_styles", "Object styles", reviera_core::standards::OBJECT_STYLES_CLASS),
];

fn holder<'a>(store: &'a Store, h: &str) -> Option<&'a Element> {
    match h {
        "project" => store.iter().find(|e| e.class == "IfcProject"),
        "project_settings" => store.get(ElementId::PROJECT_SETTINGS),
        "parameters" => store.get(ElementId::PARAMETERS),
        class => store.iter().find(|e| e.class == class),
    }
}

fn is_type(e: &Element) -> bool {
    e.class.ends_with("Type") && e.class.starts_with("Ifc")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KindItems {
    pub kind: String,
    pub label: String,
    /// Item ids (a type's GlobalId, a list item's key) and names; one item for whole documents.
    pub items: Vec<ItemInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ItemInfo {
    pub id: String,
    pub name: String,
}

fn kinds(store: &Store) -> Vec<KindItems> {
    let mut out = Vec::new();
    let mut by_class: BTreeMap<String, Vec<ItemInfo>> = BTreeMap::new();
    for e in store.iter().filter(|e| is_type(e)) {
        by_class.entry(e.class.clone()).or_default().push(ItemInfo { id: e.guid().to_string(), name: e.name.clone().unwrap_or_default() });
    }
    for (class, items) in by_class {
        out.push(KindItems { label: class.trim_start_matches("Ifc").trim_end_matches("Type").to_owned() + " types", kind: class, items });
    }
    for (kind, label, class) in ELEMENT_DOCS {
        if store.iter().any(|e| e.class == *class) {
            out.push(KindItems { kind: (*kind).into(), label: (*label).into(), items: vec![ItemInfo { id: (*kind).into(), name: (*label).into() }] });
        }
    }
    for d in DOCS {
        let Some(v) = holder(store, d.holder).and_then(|h| h.param(d.key)) else { continue };
        let items = match d.item_key {
            Some((k, n)) => v.as_array().into_iter().flatten().map(|i| ItemInfo { id: i[k].as_str().unwrap_or("").into(), name: i[n].as_str().unwrap_or("").into() }).collect(),
            None => vec![ItemInfo { id: d.kind.into(), name: d.label.into() }],
        };
        out.push(KindItems { kind: d.kind.into(), label: d.label.into(), items });
    }
    out
}

fn source(app: &App, id: DocumentId) -> Result<Store> {
    app.document(id).map(|d| d.doc.store().clone()).ok_or_else(|| bad("document.not_found", format!("no open document {id}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub source_document_id: DocumentId,
}

pub(crate) struct StandardsList;

impl HostCommand for StandardsList {
    type Host = App;
    type Params = ListParams;
    type Output = Vec<KindItems>;
    const META: CommandMeta = CommandMeta {
        name: "standards.list",
        version: 1,
        summary: "What a project's standards are",
        description: "The kinds of standards an open project has, each with its items (types by GlobalId and name; list items by key and name; whole documents as one item), for Transfer Project Standards (DM-XFER-001).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ListParams) -> Result<Vec<KindItems>> {
        Ok(kinds(&source(app, p.source_document_id)?))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    Overwrite,
    #[default]
    NewOnly,
    Rename,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub kind: String,
    /// Items by id; every item of the kind when omitted.
    pub item_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferParams {
    pub source_document_id: DocumentId,
    pub selection: Vec<Selection>,
    #[serde(default)]
    pub on_conflict: OnConflict,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TransferReport {
    /// "kind: name" of each.
    pub copied: Vec<String>,
    pub overwritten: Vec<String>,
    pub renamed: Vec<String>,
    pub skipped: Vec<String>,
    /// Dependencies found already in the target and used as they are.
    pub matched: Vec<String>,
}

/// Every GlobalId string inside a value.
fn guids_in(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) => {
            if s.len() == 22 && GlobalId::parse(s).is_some() {
                out.insert(s.clone());
            }
        }
        Value::Array(a) => a.iter().for_each(|x| guids_in(x, out)),
        Value::Object(m) => m.values().for_each(|x| guids_in(x, out)),
        _ => {}
    }
}

fn remap(v: &Value, map: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| remap(x, map)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), remap(x, map))).collect()),
        v => v.clone(),
    }
}

fn unique_name(target: &Store, class: &str, name: &str) -> String {
    (2..).map(|n| format!("{name} ({n})")).find(|c| !target.iter().any(|e| e.class == class && e.name.as_deref() == Some(c.as_str()))).expect("a free name")
}

pub(crate) struct StandardsTransfer;

impl HostCommand for StandardsTransfer {
    type Host = App;
    type Params = TransferParams;
    type Output = TransferReport;
    const META: CommandMeta = CommandMeta {
        name: "standards.transfer",
        version: 1,
        summary: "Transfer project standards",
        description: "Copies the selected kinds and items (`selection` [{kind, item_ids?}], as `standards.list` gives them) from the open project `source_document_id` into the active project, with the types they reference (matched in the target by GlobalId, then by class and name, before being copied). Items already in the target (same class and name, or same key or name) are overwritten keeping the target's GlobalId (`overwrite`), skipped (`new_only`, the default) or copied under a new name (`rename`). One undo step in the target. Returns what was copied, overwritten, renamed, skipped and matched.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: TransferParams) -> Result<TransferReport> {
        let target_id = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        if target_id == p.source_document_id {
            return Err(bad("standards.same", "choose another open project as the source"));
        }
        let src = source(app, p.source_document_id)?;
        let tgt = app.document(target_id).expect("active").doc.store().clone();
        let mut report = TransferReport::default();
        let mut creates: Vec<Value> = Vec::new();
        let mut updates: Vec<Value> = Vec::new();
        let mut settings: Vec<Value> = Vec::new();

        // Types: the selected ones and what they reference.
        let mut wanted: Vec<ElementId> = Vec::new();
        for s in &p.selection {
            if !s.kind.starts_with("Ifc") {
                continue;
            }
            for e in src.iter().filter(|e| e.class == s.kind && is_type(e)) {
                if s.item_ids.as_ref().is_none_or(|ids| ids.contains(&e.guid().to_string())) {
                    wanted.push(e.id());
                }
            }
        }
        let selected: BTreeSet<ElementId> = wanted.iter().copied().collect();
        let mut k = 0;
        while k < wanted.len() {
            let e = src.get(wanted[k]).expect("listed");
            let mut refs = BTreeSet::new();
            for v in e.params.values() {
                guids_in(v, &mut refs);
            }
            for g in refs {
                if let Some(d) = GlobalId::parse(&g).and_then(|g| src.find_by_guid(&g))
                    && d != e.id()
                    && src.get(d).is_some_and(is_type)
                    && !wanted.contains(&d)
                {
                    wanted.push(d);
                }
            }
            k += 1;
        }
        // Each type's GlobalId in the target: matched, overwritten (the target's), or new.
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        let mut plan: Vec<(ElementId, Option<ElementId>, bool)> = Vec::new();
        for id in &wanted {
            let e = src.get(*id).expect("listed");
            let label = format!("{}: {}", e.class, e.name.clone().unwrap_or_default());
            let by_guid = tgt.find_by_guid(e.guid());
            let by_name = tgt.iter().find(|t| t.class == e.class && t.name == e.name).map(Element::id);
            let existing = by_guid.or(by_name);
            let explicit = selected.contains(id);
            match (existing, explicit) {
                (Some(t), false) => {
                    map.insert(e.guid().to_string(), tgt.get(t).expect("found").guid().to_string());
                    report.matched.push(label);
                }
                (Some(t), true) => match p.on_conflict {
                    OnConflict::NewOnly => {
                        map.insert(e.guid().to_string(), tgt.get(t).expect("found").guid().to_string());
                        report.skipped.push(label);
                    }
                    OnConflict::Overwrite => {
                        map.insert(e.guid().to_string(), tgt.get(t).expect("found").guid().to_string());
                        plan.push((*id, Some(t), false));
                        report.overwritten.push(label);
                    }
                    OnConflict::Rename => {
                        map.insert(e.guid().to_string(), GlobalId::new().to_string());
                        plan.push((*id, None, true));
                        report.renamed.push(label);
                    }
                },
                (None, _) => {
                    map.insert(e.guid().to_string(), GlobalId::new().to_string());
                    plan.push((*id, None, false));
                    report.copied.push(label);
                }
            }
        }
        for (id, existing, rename) in plan {
            let e = src.get(id).expect("listed");
            let params: BTreeMap<String, Value> = e.params.iter().map(|(k, v)| (k.clone(), remap(v, &map))).collect();
            match existing {
                Some(t) => updates.push(json!({ "guid": tgt.get(t).expect("found").guid().to_string(), "params": params })),
                None => {
                    let name = match (&e.name, rename) {
                        (Some(n), true) => Some(unique_name(&tgt, &e.class, n)),
                        (n, _) => n.clone(),
                    };
                    creates.push(json!({ "guid": map[&e.guid().to_string()], "class": e.class, "name": name, "params": params }));
                }
            }
        }

        // Standards elements copied whole.
        for (kind, label, class) in ELEMENT_DOCS {
            if !p.selection.iter().any(|s| s.kind == *kind) {
                continue;
            }
            let Some(e) = src.iter().find(|e| e.class == *class) else { continue };
            let exists = tgt.iter().any(|t| t.class == *class);
            if exists && p.on_conflict == OnConflict::NewOnly {
                report.skipped.push((*label).to_string());
                continue;
            }
            for (k, v) in &e.params {
                settings.push(json!({ "holder": class, "key": k, "value": v }));
            }
            if exists { report.overwritten.push((*label).into()) } else { report.copied.push((*label).into()) }
        }

        // Settings documents.
        for d in DOCS {
            let Some(sel) = p.selection.iter().find(|s| s.kind == d.kind) else { continue };
            let Some(v) = holder(&src, d.holder).and_then(|h| h.param(d.key)).cloned() else { continue };
            let theirs = holder(&tgt, d.holder).and_then(|h| h.param(d.key)).cloned();
            match d.item_key {
                None => {
                    if theirs.is_some() && p.on_conflict == OnConflict::NewOnly {
                        report.skipped.push(d.label.into());
                        continue;
                    }
                    settings.push(json!({ "holder": d.holder, "key": d.key, "value": v }));
                    if theirs.is_some() { report.overwritten.push(d.label.into()) } else { report.copied.push(d.label.into()) }
                }
                Some((key, name)) => {
                    let mut list: Vec<Value> = theirs.and_then(|t| t.as_array().cloned()).unwrap_or_default();
                    for item in v.as_array().into_iter().flatten() {
                        let id = item[key].as_str().unwrap_or("").to_owned();
                        if sel.item_ids.as_ref().is_some_and(|ids| !ids.contains(&id)) {
                            continue;
                        }
                        let mut item = item.clone();
                        // A global's associations name elements of the source.
                        if d.kind == "global_parameters" {
                            item["associations"] = json!([]);
                        }
                        let label = format!("{}: {}", d.label, item[name].as_str().unwrap_or(""));
                        let at = list.iter().position(|x| x[key] == item[key] || x[name] == item[name]);
                        match (at, p.on_conflict) {
                            (Some(_), OnConflict::NewOnly) => report.skipped.push(label),
                            (Some(k), OnConflict::Overwrite) => {
                                // The target's key stays.
                                item[key] = list[k][key].clone();
                                list[k] = item;
                                report.overwritten.push(label);
                            }
                            (Some(_), OnConflict::Rename) => {
                                let base = item[name].as_str().unwrap_or("").to_owned();
                                let n = (2..).map(|n| format!("{base} ({n})")).find(|c| !list.iter().any(|x| x[name] == json!(c))).expect("a free name");
                                item[name] = json!(n);
                                item[key] = json!(match d.kind {
                                    "project_parameters" => uuid::Uuid::new_v4().to_string(),
                                    _ => GlobalId::new().to_string(),
                                });
                                list.push(item);
                                report.renamed.push(label);
                            }
                            (None, _) => {
                                list.push(item);
                                report.copied.push(label);
                            }
                        }
                    }
                    settings.push(json!({ "holder": d.holder, "key": d.key, "value": list }));
                }
            }
        }
        if !creates.is_empty() || !updates.is_empty() || !settings.is_empty() {
            app.execute("standards.apply", json!({ "creates": creates, "updates": updates, "settings": settings }), caller)?;
        }
        Ok(report)
    }
}
