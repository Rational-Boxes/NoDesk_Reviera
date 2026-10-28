// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Hiding elements and categories in one view (spec Ch 5, DOC-GFX-007 *Hide in View*): saved
//! with the view, undoable, and drawn, printed and exported alike, unlike the temporary hiding of
//! DOC-GFX-008 (which *Apply Hide/Isolate to View* turns into this).
//!
//! Kept in the view's payload: `hidden` `{elements: [GlobalId], categories: [category key]}`.
//! An element is hidden in the view when it is listed, or its category is.
//!
//! An element's tags, keynotes and spots in the view, and its floating labels in a 3D view, are
//! hidden with it (in `reviera_app::notes` and the window).
//!
//! Not yet: hiding by filter, hiding dimensions that reference only hidden elements, and Reveal
//! Hidden.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::views::VIEW_KEY;

const KEY: &str = "hidden";

/// What a view hides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hidden {
    /// The hidden elements' GlobalIds.
    #[serde(default)]
    pub elements: Vec<String>,
    /// The hidden categories' keys (`cat.walls`).
    #[serde(default)]
    pub categories: Vec<String>,
}

impl Hidden {
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.categories.is_empty()
    }
}

/// What view `e` hides.
pub fn hidden_of(e: &Element) -> Hidden {
    e.param(VIEW_KEY)
        .and_then(|v| v.get(KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The elements hidden in `view` (none for `None`, the unsaved default 3D view).
pub fn hidden_in(store: &Store, view: Option<ElementId>) -> HashSet<ElementId> {
    let Some(h) = view.and_then(|v| store.get(v)).map(hidden_of).filter(|h| !h.is_empty()) else {
        return HashSet::new();
    };
    let guids: HashSet<&str> = h.elements.iter().map(String::as_str).collect();
    let categories: HashSet<&str> = h.categories.iter().map(String::as_str).collect();
    store
        .iter()
        .filter(|e| {
            guids.contains(e.guid().as_str())
                || (!categories.is_empty()
                    && crate::categories::category_of(e).is_some_and(|c| categories.contains(c.key)))
        })
        .map(Element::id)
        .collect()
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let (id, _) = crate::views::resolve_view(store, r)?;
    Ok(id)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, h: &Hidden) -> Result<()> {
    let value = serde_json::to_value(h).expect("serialises");
    txn.modify(id, |e| {
        if let Some(Value::Object(o)) = e.params.get_mut(VIEW_KEY) {
            o.insert(KEY.to_owned(), value);
        }
    })
}

fn guids(store: &Store, ids: &[ElementRef]) -> Result<Vec<String>> {
    ids.iter()
        .map(|r| {
            store
                .resolve(r)
                .and_then(|id| store.get(id))
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
        })
        .collect()
}

fn check_categories(keys: &[String]) -> Result<()> {
    match keys.iter().find(|k| crate::categories::category(k).is_none()) {
        Some(k) => Err(Error::new("view.bad_category", format!("no category {k}"))),
        None => Ok(()),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HideParams {
    pub view_id: ElementRef,
    /// Elements to hide in the view.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    /// Categories to hide (keys such as `cat.walls`; see `category.list`).
    #[serde(default)]
    pub categories: Vec<String>,
    /// Hide the categories of these elements rather than the elements themselves.
    #[serde(default)]
    pub by_category: bool,
}

/// `view.hide`.
pub(crate) fn hide(txn: &mut Transaction<'_>, p: HideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    check_categories(&p.categories)?;
    let mut h = hidden_of(store.get(id).expect("resolved"));
    let mut categories = p.categories;
    if p.by_category {
        for r in &p.element_ids {
            let e = store.resolve(r).and_then(|i| store.get(i)).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let c = crate::categories::category_of(e).ok_or_else(|| Error::new("view.no_category", format!("element {} has no category", e.id())))?;
            categories.push(c.key.to_owned());
        }
    } else {
        for g in guids(&store, &p.element_ids)? {
            if !h.elements.contains(&g) {
                h.elements.push(g);
            }
        }
    }
    for c in categories {
        if !h.categories.contains(&c) {
            h.categories.push(c);
        }
    }
    write(txn, id, &h)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnhideParams {
    pub view_id: ElementRef,
    /// Elements to show again.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    /// Categories to show again.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Show everything the view hides.
    #[serde(default)]
    pub all: bool,
}

/// `view.unhide`.
pub(crate) fn unhide(txn: &mut Transaction<'_>, p: UnhideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    let mut h = hidden_of(store.get(id).expect("resolved"));
    if p.all {
        h = Hidden::default();
    } else {
        let gone = guids(&store, &p.element_ids)?;
        h.elements.retain(|g| !gone.contains(g));
        h.categories.retain(|c| !p.categories.contains(c));
    }
    write(txn, id, &h)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HiddenParams {
    pub view_id: ElementRef,
}

/// A hidden element as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HiddenElement {
    pub guid: String,
    /// `None` when the element is no longer in the model.
    pub id: Option<ElementId>,
    pub name: Option<String>,
}

/// What `view.hidden` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HiddenList {
    pub elements: Vec<HiddenElement>,
    /// Category keys and names.
    pub categories: Vec<(String, String)>,
    /// How many elements the view hides in all (listed and by category).
    pub count: usize,
}

/// `view.hidden`.
pub(crate) fn hidden(store: &Store, p: HiddenParams) -> Result<HiddenList> {
    let id = resolve(store, &p.view_id)?;
    let h = hidden_of(store.get(id).expect("resolved"));
    let elements = h
        .elements
        .iter()
        .map(|g| {
            let e = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i));
            HiddenElement { guid: g.clone(), id: e.map(Element::id), name: e.and_then(|e| e.name.clone()) }
        })
        .collect();
    let categories = h
        .categories
        .iter()
        .map(|k| (k.clone(), crate::categories::category(k).map_or_else(|| k.clone(), |c| c.name.to_owned())))
        .collect();
    Ok(HiddenList { elements, categories, count: hidden_in(store, Some(id)).len() })
}
