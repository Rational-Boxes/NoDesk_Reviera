// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Change sets: the before and after state of every element a transaction touched. They drive
//! undo (as inverses), events, change stamps and sync (DM-TXN-001, DM-TXN-002).

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;

/// One element's change. `before == None` is an addition, `after == None` a deletion.
#[derive(Debug, Clone)]
pub struct Change {
    pub before: Option<Arc<Element>>,
    pub after: Option<Arc<Element>>,
}

impl Change {
    fn is_noop(&self) -> bool {
        match (&self.before, &self.after) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

/// A change in serialisable form, for journals (Ch 2b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub id: ElementId,
    pub before: Option<Element>,
    pub after: Option<Element>,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeSet {
    changes: BTreeMap<ElementId, Change>,
}

impl ChangeSet {
    /// The changes between `base` and `working` for the elements in `touched`. Elements whose
    /// final state equals their original state are left out.
    pub(crate) fn diff(
        base: &Store,
        working: &Store,
        touched: impl IntoIterator<Item = ElementId>,
    ) -> Self {
        let changes = touched
            .into_iter()
            .map(|id| {
                (
                    id,
                    Change {
                        before: base.get_arc(id),
                        after: working.get_arc(id),
                    },
                )
            })
            .filter(|(_, c)| !c.is_noop())
            .collect();
        Self { changes }
    }

    pub fn to_records(&self) -> Vec<ChangeRecord> {
        self.iter()
            .map(|(id, c)| ChangeRecord {
                id,
                before: c.before.as_deref().cloned(),
                after: c.after.as_deref().cloned(),
            })
            .collect()
    }

    /// Rebuilds a change set from journal records. Each record's elements must carry its ID.
    pub fn from_records(records: Vec<ChangeRecord>) -> Result<Self> {
        let mut changes = BTreeMap::new();
        for r in records {
            if [&r.before, &r.after]
                .into_iter()
                .flatten()
                .any(|e| e.id() != r.id)
            {
                return Err(Error::new(
                    "journal.corrupt",
                    format!("change record {} holds another element", r.id),
                ));
            }
            changes.insert(
                r.id,
                Change {
                    before: r.before.map(Arc::new),
                    after: r.after.map(Arc::new),
                },
            );
        }
        Ok(Self { changes })
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ElementId, &Change)> {
        self.changes.iter().map(|(id, c)| (*id, c))
    }

    pub fn summary(&self) -> ChangedSummary {
        let mut s = ChangedSummary::default();
        for (id, c) in self.iter() {
            match (&c.before, &c.after) {
                (None, Some(_)) => s.added.push(id),
                (Some(_), Some(_)) => s.modified.push(id),
                (Some(_), None) => s.deleted.push(id),
                (None, None) => {}
            }
        }
        s
    }

    pub(crate) fn inverse(&self) -> Self {
        let changes = self
            .changes
            .iter()
            .map(|(id, c)| {
                (
                    *id,
                    Change {
                        before: c.after.clone(),
                        after: c.before.clone(),
                    },
                )
            })
            .collect();
        Self { changes }
    }

    /// This change set followed by `later`, as one change set.
    pub(crate) fn then(mut self, later: ChangeSet) -> Self {
        for (id, c) in later.changes {
            match self.changes.entry(id) {
                Entry::Occupied(mut o) => o.get_mut().after = c.after,
                Entry::Vacant(v) => {
                    v.insert(c);
                }
            }
        }
        self.changes.retain(|_, c| !c.is_noop());
        self
    }

    pub(crate) fn apply_to(&self, store: &mut Store) {
        for (id, c) in &self.changes {
            match &c.after {
                Some(e) => store.put(e.clone()),
                None => {
                    store.remove(*id);
                }
            }
        }
    }
}

/// The `changed` part of a command result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChangedSummary {
    pub added: Vec<ElementId>,
    pub modified: Vec<ElementId>,
    pub deleted: Vec<ElementId>,
}

impl ChangedSummary {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}
