// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Keynote tags (spec Ch 5 §5.13, DOC-ANN-014): view annotation (`anno.keynote`, see
//! `annotations`) showing a key of the project's keynote table (`classification::keynotes`).
//!
//! - **Element keynotes** show the keynote of the element's type (`type.keynote`), read live:
//!   changing the type's keynote, or the table, changes every keynote showing it.
//! - **User keynotes** show a key picked from the table, attached to an element for their leader.
//! - **Styles:** the key in a box (default), the key alone, or the keynote's text.
//! - A key the table does not have (or an element whose type has none) shows `?` and is listed
//!   as missing by `keynote.list`.
//! - Like tags, a keynote stands at a position in the view's 2D coordinates (plans, sections and
//!   elevations), with an optional leader to its element.
//!
//! Not yet: material keynotes (materials carry no keynote yet), numbering by sheet and keynote
//! legends (DOC-VIEW-010), external keynote files kept linked rather than embedded.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::notes::Frame;
use crate::store::Store;

pub const KIND_KEYNOTE: &str = "anno.keynote";

/// Where a keynote's key comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The element's type's keynote.
    #[default]
    Element,
    /// A key picked from the table.
    User,
}

/// How a keynote reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// The key in a box.
    #[default]
    BoxedNumber,
    /// The key alone.
    Number,
    /// The keynote's text.
    Text,
}

/// Stored data of a keynote tag (`anno.keynote`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeynoteData {
    pub kind: String,
    /// The view (GlobalId) that owns it.
    pub view: String,
    pub mode: Mode,
    /// The element it annotates (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A user keynote's key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The centre of its text, in the view's 2D coordinates.
    pub position: [f64; 2],
    /// A leader to the element.
    #[serde(default)]
    pub leader: bool,
    #[serde(default)]
    pub style: Style,
}

pub fn keynote_data(e: &Element) -> Option<KeynoteData> {
    if !is_annotation(e) {
        return None;
    }
    let v = e.param(ANNO_KEY)?;
    if v.get("kind")?.as_str()? != KIND_KEYNOTE {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

/// The keynote tags of a view (by its GlobalId).
pub fn of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, KeynoteData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = keynote_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

/// A keynote tag as it reads now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Resolved {
    pub id: ElementId,
    pub data: KeynoteData,
    /// The element, when it exists.
    pub host: Option<ElementId>,
    /// The key shown, if there is one.
    pub key: Option<String>,
    /// The key's text in the table.
    pub text: Option<String>,
    /// What it shows, one line.
    pub lines: Vec<String>,
    pub frame: Frame,
    /// Where its leader ends: the element's tag point in the view.
    pub anchor: Option<[f64; 2]>,
    /// No key, or one the table does not have.
    pub missing: bool,
}

/// The key an element's type gives it (`type.keynote`), if set.
pub fn element_key(store: &Store, element: ElementId) -> Option<String> {
    crate::parameters::get(store, element, "type.keynote")
        .ok()
        .and_then(|v| v.value.as_str().map(str::to_owned))
        .filter(|k| !k.trim().is_empty())
}

/// Resolves a keynote tag against the model and the keynote table.
pub fn resolve(store: &Store, id: ElementId, data: KeynoteData) -> Resolved {
    let host = data.host.as_deref().and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g));
    let key = match data.mode {
        Mode::Element => host.and_then(|h| element_key(store, h)),
        Mode::User => data.key.clone().filter(|k| !k.trim().is_empty()),
    };
    let text = key.as_deref().and_then(|k| crate::classification::keynote_text(store, k));
    let missing = text.is_none();
    let shown = match (data.style, &key, &text) {
        (_, _, None) => "?".to_owned(),
        (Style::Text, _, Some(t)) => t.clone(),
        (_, Some(k), Some(_)) => k.clone(),
        (_, None, Some(_)) => "?".to_owned(),
    };
    let anchor = host.and_then(|h| store.get(h)).and_then(|h| crate::notes::host_anchor_in(store, h, &data.view));
    let frame = if data.style == Style::BoxedNumber { Frame::Box } else { Frame::None };
    Resolved { id, host, key, text, lines: vec![shown], frame, anchor, missing, data }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A floor or ceiling plan, a section or an elevation.
    pub view_id: ElementRef,
    /// `element` (the element's type's keynote, the default) or `user` (a key from the table).
    pub mode: Option<Mode>,
    /// The element it annotates (required for element keynotes; for a user keynote's leader).
    pub host_ref: Option<ElementRef>,
    /// A user keynote's key.
    pub key: Option<String>,
    /// The centre of its text, in the view's 2D coordinates; default on the element.
    pub position: Option<[f64; 2]>,
    /// `boxed_number` (default), `number` or `text`.
    pub style: Option<Style>,
    /// A leader to the element; default on when there is an element and a position.
    pub leader: Option<bool>,
}

/// `keynote.create`.
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view_id, data) = crate::views::resolve_view(&store, &p.view_id)?;
    if !crate::plans::is_plan(&data.kind) && !crate::sections::is_section(&data.kind) {
        return Err(Error::new("anno.view_not_supported", "keynotes are placed in plans, sections and elevations"));
    }
    let view = store.get(view_id).expect("resolved").guid().to_string();
    let mode = p.mode.unwrap_or_default();
    let host = match &p.host_ref {
        Some(r) => {
            let e = store.resolve(r).and_then(|id| store.get(id)).ok_or_else(|| Error::new("element.not_found", "no such element to keynote"))?;
            if is_annotation(e) || crate::views::view_data(e).is_some() {
                return Err(Error::new("keynote.bad_host", "a keynote annotates a model element"));
            }
            Some(e)
        }
        None => None,
    };
    let key = match mode {
        Mode::Element => {
            if host.is_none() {
                return Err(Error::new("keynote.no_host", "an element keynote needs the element (host_ref)"));
            }
            if p.key.is_some() {
                return Err(Error::new("keynote.bad_option", "an element keynote shows its type's key; give key for a user keynote"));
            }
            None
        }
        Mode::User => Some(p.key.clone().filter(|k| !k.trim().is_empty()).ok_or_else(|| Error::new("keynote.no_key", "a user keynote needs a key"))?),
    };
    let anchor = host.and_then(|h| crate::notes::host_anchor_in(&store, h, &view));
    let position = match (p.position, anchor) {
        (Some(p), _) => p,
        (None, Some(a)) => a,
        (None, None) => return Err(Error::new("keynote.no_position", "give the keynote's position")),
    };
    let data = KeynoteData {
        kind: KIND_KEYNOTE.into(),
        view: view.clone(),
        mode,
        host: host.map(|h| h.guid().to_string()),
        key,
        position,
        leader: p.leader.unwrap_or(p.position.is_some() && anchor.is_some()),
        style: p.style.unwrap_or_default(),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{view}/{}", guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, serde_json::to_value(&data).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub annotation_ids: Vec<ElementRef>,
    pub style: Option<Style>,
    /// A user keynote's key.
    pub key: Option<String>,
    pub leader: Option<bool>,
    /// Moves it, in the view's 2D coordinates.
    pub position: Option<[f64; 2]>,
}

/// `keynote.set`.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.annotation_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("keynote.not_found", "no such keynote"))?;
        let mut d = keynote_data(store.get(id).expect("resolved")).ok_or_else(|| Error::new("keynote.not_found", format!("element {id} is not a keynote")))?;
        if let Some(k) = &p.key {
            if d.mode != Mode::User {
                return Err(Error::new("keynote.bad_option", "only a user keynote has its own key"));
            }
            d.key = Some(k.clone());
        }
        if let Some(v) = p.style {
            d.style = v;
        }
        if let Some(v) = p.leader {
            d.leader = v;
        }
        if let Some(v) = p.position {
            d.position = v;
        }
        let value = serde_json::to_value(&d).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(ANNO_KEY.into(), value);
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// A view; omitted, every keynote of the model.
    pub view_id: Option<ElementRef>,
}

/// `keynote.list`: keynote tags as they read now, with those whose key is missing marked.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<Resolved>> {
    let view = match &p.view_id {
        Some(r) => {
            let (id, _) = crate::views::resolve_view(store, r)?;
            Some(store.get(id).expect("resolved").guid().to_string())
        }
        None => None,
    };
    Ok(store
        .iter()
        .filter_map(|e| Some((e.id(), keynote_data(e)?)))
        .filter(|(_, d)| view.as_ref().is_none_or(|v| *v == d.view))
        .map(|(id, d)| resolve(store, id, d))
        .collect())
}
