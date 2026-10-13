// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The element store. Persistent maps make a snapshot an O(1) clone, so read-only queries run
//! against the last committed state while a transaction works on its own copy (DM-TXN-001).
//!
//! The mutating API is crate-private: only transactions and undo can change a store (DM-CORE-001).

use std::sync::Arc;

use imbl::{HashMap, OrdMap};

use crate::element::Element;
use crate::id::{ElementId, ElementRef, GlobalId};

#[derive(Debug, Clone, Default)]
pub struct Store {
    elements: OrdMap<ElementId, Arc<Element>>,
    by_guid: HashMap<GlobalId, ElementId>,
}

impl Store {
    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(&id).map(|e| &**e)
    }

    pub fn contains(&self, id: ElementId) -> bool {
        self.elements.contains_key(&id)
    }

    pub fn find_by_guid(&self, guid: &GlobalId) -> Option<ElementId> {
        self.by_guid.get(guid).copied()
    }

    /// Resolves a reference given as an internal ID or a GlobalId to an element in this store.
    pub fn resolve(&self, r: &ElementRef) -> Option<ElementId> {
        match r {
            ElementRef::Id(id) => self.contains(*id).then_some(*id),
            ElementRef::Guid(g) => self.find_by_guid(g),
        }
    }

    /// Elements in internal-ID order.
    pub fn iter(&self) -> impl Iterator<Item = &Element> {
        self.elements.values().map(|e| &**e)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub(crate) fn get_arc(&self, id: ElementId) -> Option<Arc<Element>> {
        self.elements.get(&id).cloned()
    }

    pub(crate) fn put(&mut self, element: Arc<Element>) {
        self.by_guid.insert(element.guid().clone(), element.id());
        self.elements.insert(element.id(), element);
    }

    pub(crate) fn remove(&mut self, id: ElementId) -> Option<Arc<Element>> {
        let element = self.elements.remove(&id)?;
        self.by_guid.remove(element.guid());
        Some(element)
    }
}
