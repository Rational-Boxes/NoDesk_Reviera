// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element and category graphic overrides in one view (spec Ch 5, DOC-GFX-007 and DOC-GFX-001
//! layers 3 and 5): saved with the view, undoable, and printed so. Built so far are two of the
//! override attributes:
//!
//! - **Surface transparency** (0–100 %): a 3D view draws the element translucent; plans,
//!   sections and elevations fade its lines toward the paper by as much.
//! - **Halftone**: plans, sections and elevations draw the element's lines at half strength.
//!
//! Kept in the view's payload: `elementOverrides` `{GlobalId: {transparency?, halftone?}}` and
//! `categoryOverrides` `{category key: {transparency?, halftone?}}`. For each attribute the
//! element's own override wins over its category's; halftone is on if either sets it.
//!
//! Not yet: lines, patterns, cut lines and cut patterns, and the Visibility/Graphics dialog.

use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::views::VIEW_KEY;

const ELEMENTS: &str = "elementOverrides";
const CATEGORIES: &str = "categoryOverrides";
/// How strongly a halftone element's lines are drawn.
pub const HALFTONE: f32 = 0.5;

/// One override: each attribute set or left to the layer below.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// Surface transparency, 0–100 %.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halftone: Option<bool>,
}

impl Override {
    pub fn is_empty(&self) -> bool {
        self.transparency.is_none() && self.halftone.is_none()
    }

    /// `self` with the attributes `top` sets taken from it.
    fn under(self, top: Override) -> Override {
        Override { transparency: top.transparency.or(self.transparency), halftone: top.halftone.or(self.halftone) }
    }

    fn check(&self) -> Result<()> {
        if self.transparency.is_some_and(|t| !(0.0..=100.0).contains(&t)) {
            return Err(Error::new("view.bad_transparency", "transparency is 0 to 100 %"));
        }
        Ok(())
    }
}

/// An element's resolved override in a view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolved {
    /// How opaque its surfaces are (1 for none).
    pub opacity: f32,
    pub halftone: bool,
}

impl Resolved {
    /// How strongly its lines are drawn in a plan, section or elevation.
    pub fn line_strength(&self) -> f32 {
        self.opacity * if self.halftone { HALFTONE } else { 1.0 }
    }
}

fn map_of(e: &Element, key: &str) -> BTreeMap<String, Override> {
    e.param(VIEW_KEY)
        .and_then(|v| v.get(key))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// A view's element overrides, by GlobalId.
pub fn element_overrides(e: &Element) -> BTreeMap<String, Override> {
    map_of(e, ELEMENTS)
}

/// A view's category overrides, by category key.
pub fn category_overrides(e: &Element) -> BTreeMap<String, Override> {
    map_of(e, CATEGORIES)
}

/// The overridden elements of `view` and what they resolve to (none for `None`).
pub fn resolved_in(store: &Store, view: Option<ElementId>) -> HashMap<ElementId, Resolved> {
    let Some(v) = view.and_then(|v| store.get(v)) else { return HashMap::new() };
    let (elements, categories) = (element_overrides(v), category_overrides(v));
    if elements.is_empty() && categories.is_empty() {
        return HashMap::new();
    }
    store
        .iter()
        .filter_map(|e| {
            let by_category = (!categories.is_empty())
                .then(|| crate::categories::category_of(e).and_then(|c| categories.get(c.key)).copied())
                .flatten()
                .unwrap_or_default();
            let own = elements.get(e.guid().as_str()).copied().unwrap_or_default();
            let o = by_category.under(own);
            let halftone = own.halftone.unwrap_or(false) || by_category.halftone.unwrap_or(false);
            (!o.is_empty() && (o.transparency.is_some_and(|t| t > 0.0) || halftone)).then(|| {
                let opacity = 1.0 - o.transparency.unwrap_or(0.0) as f32 / 100.0;
                (e.id(), Resolved { opacity: opacity.max(0.0), halftone })
            })
        })
        .collect()
}

/// The overrides of the view whose GlobalId is `guid`.
pub fn resolved_in_guid(store: &Store, guid: Option<&str>) -> HashMap<ElementId, Resolved> {
    let id = guid.and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g));
    resolved_in(store, id)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, key: &str, map: &BTreeMap<String, Override>) -> Result<()> {
    let value = serde_json::to_value(map).expect("serialises");
    txn.modify(id, |e| {
        if let Some(Value::Object(o)) = e.params.get_mut(VIEW_KEY) {
            if map.is_empty() {
                o.remove(key);
            } else {
                o.insert(key.to_owned(), value);
            }
        }
    })
}

fn view(store: &Store, r: &ElementRef) -> Result<ElementId> {
    Ok(crate::views::resolve_view(store, r)?.0)
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideElementsParams {
    pub view_id: ElementRef,
    pub element_ids: Vec<ElementRef>,
    /// The attributes to set; those left out keep what the elements had.
    #[serde(rename = "override")]
    pub with: Override,
}

/// `view.override_elements`.
pub(crate) fn override_elements(txn: &mut Transaction<'_>, p: OverrideElementsParams) -> Result<()> {
    p.with.check()?;
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let mut map = element_overrides(store.get(id).expect("resolved"));
    for g in guids(&store, &p.element_ids)? {
        let o = map.entry(g).or_default();
        *o = o.under(p.with);
    }
    write(txn, id, ELEMENTS, &map)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearElementOverridesParams {
    pub view_id: ElementRef,
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    /// Clear every element override of the view.
    #[serde(default)]
    pub all: bool,
}

/// `view.clear_element_overrides`.
pub(crate) fn clear_element_overrides(txn: &mut Transaction<'_>, p: ClearElementOverridesParams) -> Result<()> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let mut map = element_overrides(store.get(id).expect("resolved"));
    if p.all {
        map.clear();
    } else {
        for g in guids(&store, &p.element_ids)? {
            map.remove(&g);
        }
    }
    write(txn, id, ELEMENTS, &map)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CategoryOverrideParams {
    pub view_id: ElementRef,
    /// A category key (`cat.walls`; see `category.list`).
    pub category: String,
    #[serde(rename = "override")]
    pub with: Override,
}

/// `view.set_category_override`.
pub(crate) fn set_category_override(txn: &mut Transaction<'_>, p: CategoryOverrideParams) -> Result<()> {
    p.with.check()?;
    if crate::categories::category(&p.category).is_none() {
        return Err(Error::new("view.bad_category", format!("no category {}", p.category)));
    }
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let mut map = category_overrides(store.get(id).expect("resolved"));
    let o = map.entry(p.category).or_default();
    *o = o.under(p.with);
    write(txn, id, CATEGORIES, &map)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearCategoryOverrideParams {
    pub view_id: ElementRef,
    pub category: String,
}

/// `view.clear_category_override`.
pub(crate) fn clear_category_override(txn: &mut Transaction<'_>, p: ClearCategoryOverrideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let mut map = category_overrides(store.get(id).expect("resolved"));
    map.remove(&p.category);
    write(txn, id, CATEGORIES, &map)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverridesParams {
    pub view_id: ElementRef,
}

/// An element override as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ElementOverride {
    pub guid: String,
    /// `None` when the element is no longer in the model.
    pub id: Option<ElementId>,
    #[serde(rename = "override")]
    pub with: Override,
}

/// What `view.overrides` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Overrides {
    pub elements: Vec<ElementOverride>,
    pub categories: BTreeMap<String, Override>,
}

/// `view.overrides`.
pub(crate) fn overrides(store: &Store, p: OverridesParams) -> Result<Overrides> {
    let id = view(store, &p.view_id)?;
    let e = store.get(id).expect("resolved");
    let elements = element_overrides(e)
        .into_iter()
        .map(|(guid, with)| {
            let id = GlobalId::parse(&guid).and_then(|g| store.find_by_guid(&g));
            ElementOverride { guid, id, with }
        })
        .collect();
    Ok(Overrides { elements, categories: category_overrides(e) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elements_own_override_wins_and_halftone_adds_up() {
        let category = Override { transparency: Some(50.0), halftone: Some(true) };
        let own = Override { transparency: Some(80.0), halftone: None };
        let o = category.under(own);
        assert_eq!(o, Override { transparency: Some(80.0), halftone: Some(true) });
        let r = Resolved { opacity: 0.2, halftone: true };
        assert!((r.line_strength() - 0.1).abs() < 1e-6);
        assert!(Override { transparency: Some(120.0), halftone: None }.check().is_err());
    }
}
