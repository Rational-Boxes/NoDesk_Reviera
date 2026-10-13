// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The common shape of every stored object (DM-ELEM-001).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{ElementId, GlobalId};

/// A stored element. Its ID and GUID are fixed at creation; everything else is edited through
/// [`Transaction::modify`](crate::Transaction::modify).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    id: ElementId,
    guid: GlobalId,
    /// IFC entity class (for example `IfcWall`) or, for RXD-only elements, the RXD object kind.
    pub class: String,
    pub name: Option<String>,
    /// Parameter values by name. Quantities are stored in SI units.
    pub params: BTreeMap<String, Value>,
}

impl Element {
    pub(crate) fn new(id: ElementId, guid: GlobalId, init: NewElement) -> Self {
        Self {
            id,
            guid,
            class: init.class,
            name: init.name,
            params: init.params,
        }
    }

    pub fn id(&self) -> ElementId {
        self.id
    }

    pub fn guid(&self) -> &GlobalId {
        &self.guid
    }

    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }
}

/// What a command supplies to create an element.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewElement {
    pub class: String,
    pub name: Option<String>,
    pub params: BTreeMap<String, Value>,
    /// Keeps an existing GlobalId (for example when reading an IFC file); a new one is made otherwise.
    pub guid: Option<GlobalId>,
}

impl NewElement {
    pub fn new(class: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            ..Self::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn param(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(name.into(), value.into());
        self
    }

    pub fn guid(mut self, guid: GlobalId) -> Self {
        self.guid = Some(guid);
        self
    }
}
