// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Types (spec Ch 2 §2.2.4, DM-TYPE-001; Ch 3 ARC-TOOL-002): duplicating, renaming and editing
//! types, and changing the type of elements.
//!
//! A type's *family* is its IFC class plus, for Reviera system types, its recipe kind (wall types,
//! grid types): names are unique within a family, and an element can only take a type of the
//! family it already has.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::{categories, grids, inserts, walls};

/// The family of a type element: its class and recipe kind; `None` for non-types.
pub fn family(e: &Element) -> Option<(String, Option<String>)> {
    if !categories::is_type(e) {
        return None;
    }
    let recipe = e
        .param(walls::RECIPE_KEY)
        .and_then(|r| r.get("kind")?.as_str().map(str::to_owned));
    Some((e.class.clone(), recipe))
}

/// One type, for listings.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    pub class: String,
    /// The recipe kind of a Reviera system type (`recipe.wall_type`, `recipe.grid_type`).
    pub recipe: Option<String>,
    /// How many elements use it.
    pub instances: usize,
}

/// The elements of type `t` (walls and other occurrences by `@type`, grids by their record).
pub fn instances(store: &Store, t: &Element) -> Vec<ElementId> {
    let guid = t.guid().as_str();
    store
        .iter()
        .filter(|e| {
            e.param("@type").and_then(Value::as_str) == Some(guid)
                || grids::grid_data(e).is_some_and(|d| d.type_guid.as_deref() == Some(guid))
        })
        .map(Element::id)
        .collect()
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such type"))?;
    if family(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "type.not_a_type",
            format!("element {id} is not a type"),
        ));
    }
    Ok(id)
}

fn check_name(
    store: &Store,
    id: ElementId,
    name: &str,
    except: Option<ElementId>,
) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::new("type.bad_name", "a type needs a name"));
    }
    let fam = family(store.get(id).expect("resolved"));
    if store
        .iter()
        .any(|e| Some(e.id()) != except && family(e) == fam && e.name.as_deref() == Some(name))
    {
        return Err(Error::new(
            "type.name_taken",
            format!("another type of this family is named {name}"),
        ));
    }
    Ok(name.to_owned())
}

/// The name a duplicate of `name` is offered: `<name> 2`, or the next free number.
pub fn duplicate_name(store: &Store, id: ElementId) -> String {
    let Some(e) = store.get(id) else {
        return String::new();
    };
    let base = e.name.clone().unwrap_or_default();
    let fam = family(e);
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| {
            !store
                .iter()
                .any(|o| family(o) == fam && o.name.as_deref() == Some(n.as_str()))
        })
        .expect("a free name")
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only types of this class (`IfcWallType`, `IfcTypeObject`, …).
    pub class: Option<String>,
    /// Only Reviera system types with this recipe kind (`recipe.grid_type`, …).
    pub recipe: Option<String>,
}

pub(crate) fn list(store: &Store, p: ListParams) -> Vec<TypeInfo> {
    let mut out: Vec<TypeInfo> = store
        .iter()
        .filter_map(|e| {
            let (class, recipe) = family(e)?;
            if p.class.as_ref().is_some_and(|c| *c != class)
                || p.recipe
                    .as_ref()
                    .is_some_and(|k| recipe.as_ref() != Some(k))
            {
                return None;
            }
            Some(TypeInfo {
                id: e.id(),
                name: e.name.clone().unwrap_or_default(),
                class,
                recipe,
                instances: instances(store, e).len(),
            })
        })
        .collect();
    out.sort_by(|a, b| (&a.class, &a.name).cmp(&(&b.class, &b.name)));
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateParams {
    pub type_id: ElementRef,
    /// Unique within the type's family; omitted, `<name> 2` (the next free number).
    pub new_name: Option<String>,
}

/// Copies a type with every value under a new name and GlobalId.
pub(crate) fn duplicate(
    txn: &mut crate::txn::Transaction<'_>,
    p: DuplicateParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = resolve_type(&store, &p.type_id)?;
    let name = match &p.new_name {
        Some(n) => check_name(&store, id, n, None)?,
        None => duplicate_name(&store, id),
    };
    let e = store.get(id).expect("resolved");
    let mut copy = NewElement::new(e.class.clone()).name(name);
    // Identity data stays with the original.
    copy.params = e
        .params
        .iter()
        .filter(|(k, _)| k.as_str() != "rxd:identity")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    txn.create(copy)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub type_id: ElementRef,
    pub name: String,
}

pub(crate) fn rename(txn: &mut crate::txn::Transaction<'_>, p: RenameParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve_type(&store, &p.type_id)?;
    let name = check_name(&store, id, &p.name, Some(id))?;
    txn.modify(id, |e| e.name = Some(name))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParamParams {
    pub type_id: ElementRef,
    /// A grid type's value (`symbol`, `center_segment`, `end_pen`, …), a Reviera wall type's
    /// `width`, or any parameter of the type (as `param.set`).
    pub param: String,
    pub value: Value,
}

/// Sets a type parameter; every instance regenerates from it.
pub(crate) fn set_param(txn: &mut crate::txn::Transaction<'_>, p: SetParamParams) -> Result<()> {
    let id = resolve_type(txn.store(), &p.type_id)?;
    let e = txn.store().get(id).expect("resolved");
    if grids::is_grid_type(e) {
        return grids::set_type_param(txn, id, &p.param, &p.value);
    }
    if inserts::insert_type(e).is_some() {
        return inserts::set_type_param(txn, id, &p.param, &p.value);
    }
    if crate::columns::column_type(e).is_some() {
        return crate::columns::set_type_param(txn, id, &p.param, &p.value);
    }
    if crate::curtain::curtain_type(e).is_some() {
        return crate::curtain::set_type_param(txn, id, &p.param, &p.value);
    }
    if (crate::floors::type_thickness(e).is_some() || crate::roofs::is_roof_type(e))
        && p.param == "thickness"
    {
        return crate::floors::set_type_thickness(txn, id, &p.value);
    }
    // A Reviera wall type's width is part of its recipe.
    if walls::type_width(e).is_some() && p.param == "width" && crate::compound::is_layered(e) {
        return Err(Error::new(
            "type.layered",
            "a layered wall type's width is the total of its layers: change them in Edit Assembly (compound.set_layers)",
        ));
    }
    if walls::type_width(e).is_some() && p.param == "width" {
        let width = match &p.value {
            Value::Number(n) => n.as_f64().unwrap_or(-1.0),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(txn.store()))?
            }
            _ => -1.0,
        };
        if !(width > 0.0 && width.is_finite()) {
            return Err(Error::new(
                "type.invalid_value",
                "the width must be greater than 0",
            ));
        }
        let mut recipe = e
            .param(walls::RECIPE_KEY)
            .cloned()
            .expect("a wall type recipe");
        recipe["width"] = width.into();
        return txn.modify(id, |e| {
            e.params.insert(walls::RECIPE_KEY.into(), recipe);
        });
    }
    let value = (!p.value.is_null()).then_some(&p.value);
    crate::parameters::set(txn, &[id], &p.param, value).map(|_| ())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub element_ids: Vec<ElementRef>,
    pub type_id: ElementRef,
}

/// Changes the type of elements (type swap): grids take another grid type, doors and windows
/// another type of their kind (they must still fit), Reviera walls another wall type with a
/// width (they keep their location line).
pub(crate) fn set_type(txn: &mut crate::txn::Transaction<'_>, p: SetTypeParams) -> Result<()> {
    let store = txn.store().clone();
    let t = resolve_type(&store, &p.type_id)?;
    let ty = store.get(t).expect("resolved");
    let mut grid_ids = Vec::new();
    for r in &p.element_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        if grids::is_grid(e) {
            if !grids::is_grid_type(ty) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("grid {id} needs a grid type"),
                ));
            }
            grid_ids.push(id);
        } else if let Some(kind) = inserts::kind_of(e) {
            if inserts::insert_type(ty).and_then(|t| t.kind()) != Some(kind) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} needs a {} type", kind.class()),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
            let placed = inserts::resolve(txn.store(), txn.store().get(id).expect("exists"))?;
            inserts::check_fit(txn.store(), &placed)?;
        } else if crate::roofs::roof_recipe(e).is_some() {
            if !crate::roofs::is_roof_type(ty) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("roof {id} needs a roof type"),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
        } else if let Some((kind, _)) = crate::floors::slab_recipe(e) {
            if crate::floors::type_kind(ty) != Some(kind) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("{} {id} needs a {} type", kind.noun(), kind.noun()),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
        } else if crate::curtain::curtain_recipe(e).is_some() {
            if crate::curtain::curtain_type(ty).is_none() {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("curtain wall {id} needs a curtain wall type"),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
        } else if crate::columns::column_recipe(e).is_some() {
            if crate::columns::column_type(ty).is_none() {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("column {id} needs a column type"),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
            crate::columns::regenerate(txn.store(), txn.store().get(id).expect("exists"))?;
        } else if walls::wall_recipe(e).is_some() {
            if walls::type_width(ty).is_none() {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("wall {id} needs a wall type with a width"),
                ));
            }
            let guid = ty.guid().to_string();
            txn.modify(id, |w| {
                w.params.insert("@type".into(), Value::String(guid));
            })?;
        } else {
            return Err(Error::new(
                "type.unsupported",
                format!(
                    "the type of element {id} ({}) cannot be changed yet",
                    e.class
                ),
            ));
        }
    }
    if grid_ids.is_empty() {
        return Ok(());
    }
    grids::set_type(txn, &grid_ids, t)
}
