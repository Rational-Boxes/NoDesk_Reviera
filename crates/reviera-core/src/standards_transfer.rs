// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Transfer Project Standards, the target side (spec Ch 2 §2.10.9, DM-XFER-001):
//! `standards.apply` writes what `standards.transfer` worked out from the source project into
//! this one as one transaction: new elements (types, with their GlobalIds chosen so references
//! between them hold), replaced parameters of existing ones (their GlobalIds kept, so references
//! in this project stay valid), and settings documents.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Create {
    pub guid: String,
    pub class: String,
    pub name: Option<String>,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Update {
    /// The element in this project (GlobalId).
    pub guid: String,
    pub params: BTreeMap<String, Value>,
}

/// A settings value: `holder` is `project`, `project_settings`, `parameters`, or the class of a
/// standards element (`rxd:standards/line_weights`), made when missing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Setting {
    pub holder: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    #[serde(default)]
    pub creates: Vec<Create>,
    #[serde(default)]
    pub updates: Vec<Update>,
    #[serde(default)]
    pub settings: Vec<Setting>,
}

/// `standards.apply`.
pub(crate) fn apply(txn: &mut Transaction<'_>, p: ApplyParams) -> Result<usize> {
    let mut n = 0;
    for c in p.creates {
        let guid = GlobalId::parse(&c.guid).ok_or_else(|| bad("standards.guid", format!("{:?} is not a GlobalId", c.guid)))?;
        if txn.store().find_by_guid(&guid).is_some() {
            return Err(bad("standards.exists", format!("{} is in the project already", c.guid)));
        }
        txn.create(NewElement { class: c.class, name: c.name, params: c.params, guid: Some(guid) })?;
        n += 1;
    }
    for u in p.updates {
        let id = GlobalId::parse(&u.guid).and_then(|g| txn.store().find_by_guid(&g)).ok_or_else(|| bad("standards.not_found", format!("no element {}", u.guid)))?;
        let params = u.params;
        txn.modify(id, |e| {
            e.params = params;
        })?;
        n += 1;
    }
    for s in p.settings {
        let store = txn.store().clone();
        let id = match s.holder.as_str() {
            "project" => store.iter().find(|e| e.class == "IfcProject").map(|e| e.id()).ok_or_else(|| bad("project.missing", "the model has no project"))?,
            "project_settings" => ElementId::PROJECT_SETTINGS,
            "parameters" => ElementId::PARAMETERS,
            class => match store.iter().find(|e| e.class == class) {
                Some(e) => e.id(),
                None => txn.create(NewElement::new(class))?,
            },
        };
        if !txn.store().contains(id) {
            let class = if id == ElementId::PARAMETERS { crate::parameters::PARAMETERS_CLASS } else { crate::units::SETTINGS_CLASS };
            txn.create_with_id(id, NewElement::new(class).param(&s.key, s.value))?;
        } else {
            let (key, value) = (s.key, s.value);
            txn.modify(id, |e| {
                e.params.insert(key, value);
            })?;
        }
        n += 1;
    }
    Ok(n)
}
