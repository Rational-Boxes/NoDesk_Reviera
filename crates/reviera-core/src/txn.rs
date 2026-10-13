// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Transactions and savepoints (DM-TXN-001).
//!
//! A transaction edits a private copy of the store. Committing turns the edits into a change set
//! and applies it to the document; dropping the transaction without committing rolls it back,
//! including when a command returns an error or panics.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::change::ChangeSet;
use crate::document::{Commit, Document};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::origin::Origin;
use crate::store::Store;

/// A problem reported by a command that does not stop the transaction committing (DM-FAIL-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    /// Failure definition ID, for example `arc.wall.overlap`.
    pub id: String,
    pub message: String,
    pub element_ids: Vec<ElementId>,
}

/// Token for [`Transaction::rollback_to`]. Savepoints never appear in the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Savepoint(usize);

#[derive(Debug)]
pub struct Transaction<'d> {
    doc: &'d mut Document,
    name: String,
    origin: Origin,
    working: Store,
    touched: BTreeSet<ElementId>,
    savepoints: Vec<(Store, BTreeSet<ElementId>)>,
    warnings: Vec<Warning>,
}

impl<'d> Transaction<'d> {
    pub(crate) fn new(doc: &'d mut Document, name: String, origin: Origin) -> Self {
        let working = doc.store().clone();
        Self {
            doc,
            name,
            origin,
            working,
            touched: BTreeSet::new(),
            savepoints: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The store as this transaction currently sees it.
    pub fn store(&self) -> &Store {
        &self.working
    }

    pub fn create(&mut self, init: NewElement) -> Result<ElementId> {
        let guid = match &init.guid {
            Some(g) if self.working.find_by_guid(g).is_some() || self.doc.is_tombstoned(g) => {
                return Err(Error::new(
                    "element.duplicate_guid",
                    format!("GlobalId {g} is already in use"),
                ));
            }
            Some(g) => g.clone(),
            None => self.doc.new_guid(),
        };
        // IDs are taken from the document, so a rolled-back transaction never hands its IDs out again.
        let id = self.doc.allocate_id();
        self.working.put(Arc::new(Element::new(id, guid, init)));
        self.touched.insert(id);
        Ok(id)
    }

    /// Edits an element's class, name or parameters.
    pub fn modify<R>(&mut self, id: ElementId, edit: impl FnOnce(&mut Element) -> R) -> Result<R> {
        let current = self.working.get_arc(id).ok_or_else(|| not_found(id))?;
        let mut element = Element::clone(&current);
        let r = edit(&mut element);
        self.working.put(Arc::new(element));
        self.touched.insert(id);
        Ok(r)
    }

    /// Sets an element to exactly `element`, creating it if it does not exist. For journal replay and
    /// merges, where the ID and GUID come from the record.
    pub fn put_element(&mut self, element: Element) -> Result<()> {
        if let Some(other) = self.working.find_by_guid(element.guid())
            && other != element.id()
        {
            return Err(Error::new(
                "element.duplicate_guid",
                format!("GlobalId {} belongs to element {other}", element.guid()),
            ));
        }
        self.doc.reserve_id(element.id());
        self.touched.insert(element.id());
        self.working.put(Arc::new(element));
        Ok(())
    }

    pub fn set_param(
        &mut self,
        id: ElementId,
        name: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<()> {
        let (name, value) = (name.into(), value.into());
        self.modify(id, |e| {
            e.params.insert(name, value);
        })
    }

    pub fn delete(&mut self, id: ElementId) -> Result<()> {
        self.working.remove(id).ok_or_else(|| not_found(id))?;
        self.touched.insert(id);
        Ok(())
    }

    pub fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }

    pub fn savepoint(&mut self) -> Savepoint {
        self.savepoints
            .push((self.working.clone(), self.touched.clone()));
        Savepoint(self.savepoints.len() - 1)
    }

    /// Undoes everything since `sp`. The savepoint stays valid; later savepoints are discarded.
    pub fn rollback_to(&mut self, sp: Savepoint) -> Result<()> {
        let (store, touched) = self.savepoints.get(sp.0).cloned().ok_or_else(|| {
            Error::new(
                "txn.unknown_savepoint",
                "the savepoint is not part of this transaction",
            )
        })?;
        self.working = store;
        self.touched = touched;
        self.savepoints.truncate(sp.0 + 1);
        Ok(())
    }

    /// What committing now would change, without committing.
    pub fn pending_changes(&self) -> crate::change::ChangedSummary {
        ChangeSet::diff(
            self.doc.store(),
            &self.working,
            self.touched.iter().copied(),
        )
        .summary()
    }

    /// Warnings raised so far.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Commits the transaction. A transaction whose edits cancel out changes nothing and adds no undo item.
    pub fn commit(self) -> Commit {
        let changes = ChangeSet::diff(self.doc.store(), &self.working, self.touched);
        self.doc
            .commit_changes(self.name, self.origin, changes, self.warnings)
    }

    pub fn rollback(self) {}
}

fn not_found(id: ElementId) -> Error {
    Error::new("element.not_found", format!("element {id} does not exist"))
}
