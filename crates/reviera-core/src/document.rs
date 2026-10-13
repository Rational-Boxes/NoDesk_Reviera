// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! A model document: the committed store, version stamps (DM-ID-002), undo and redo
//! (DM-TXN-002) and transaction groups (DM-TXN-001).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::change::{ChangeSet, ChangedSummary};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::origin::Origin;
use crate::store::Store;
use crate::txn::{Transaction, Warning};

/// Cheap change detection: an episode GUID (new for each open or session) plus a counter that
/// grows by one for every committed transaction, undo and redo included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelVersion {
    pub episode: uuid::Uuid,
    pub counter: u64,
}

/// The last change to an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub counter: u64,
    /// [`Origin::label`] of the transaction.
    pub by: String,
    pub at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tombstone {
    guid: GlobalId,
    counter: u64,
}

/// The result of [`Document::changed_since`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ChangedSince {
    pub changed: Vec<ElementId>,
    pub deleted: Vec<ElementId>,
    /// The version came from another episode, so the caller must re-read everything.
    pub stale_episode: bool,
}

/// What a commit, undo, redo or group rollback did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub version: ModelVersion,
    pub changed: ChangedSummary,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone)]
pub struct UndoItem {
    pub name: String,
    pub origin: Origin,
    pub time: SystemTime,
    changes: ChangeSet,
}

impl UndoItem {
    pub fn changes(&self) -> &ChangeSet {
        &self.changes
    }
}

/// Which undo items a caller may undo or redo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoScope {
    All,
    /// Only items made in this session (MCP sessions below `full_scripting`).
    Session(String),
}

impl UndoScope {
    fn allows(&self, item: &UndoItem) -> bool {
        match self {
            UndoScope::All => true,
            UndoScope::Session(s) => item.origin.session_id.as_deref() == Some(s),
        }
    }
}

/// How a transaction group ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupEnd {
    /// Merge the group's transactions into one undo item named after the group.
    Assimilate,
    /// Keep them as separate undo items.
    Commit,
    /// Undo all of them.
    Rollback,
}

#[derive(Debug, Clone)]
struct OpenGroup {
    name: String,
    origin: Origin,
    /// Undo stack length when the group began.
    mark: usize,
}

/// Whether new GUIDs are random (UUID v4) or time-ordered (UUID v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuidMode {
    #[default]
    Random,
    TimeOrdered,
}

/// A read-only view of the last committed state, safe to send to other threads.
#[derive(Debug, Clone)]
pub struct Snapshot {
    store: Store,
    version: ModelVersion,
}

impl Snapshot {
    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn version(&self) -> ModelVersion {
        self.version
    }
}

#[derive(Debug)]
pub struct Document {
    store: Store,
    next_id: u64,
    episode: uuid::Uuid,
    counter: u64,
    guid_mode: GuidMode,
    stamps: HashMap<ElementId, Stamp>,
    tombstones: BTreeMap<ElementId, Tombstone>,
    undo: VecDeque<UndoItem>,
    redo: Vec<UndoItem>,
    max_undo_items: usize,
    group: Option<OpenGroup>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub const DEFAULT_MAX_UNDO_ITEMS: usize = 100;

    pub fn new() -> Self {
        Self {
            store: Store::default(),
            next_id: ElementId::FIRST.0,
            episode: uuid::Uuid::new_v4(),
            counter: 0,
            guid_mode: GuidMode::default(),
            stamps: HashMap::new(),
            tombstones: BTreeMap::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_undo_items: Self::DEFAULT_MAX_UNDO_ITEMS,
            group: None,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Builds a document from elements read from a file, keeping their IDs and GUIDs. Loading is not
    /// a transaction: there is no undo item and the version counter starts at 0. The next new ID is
    /// at least `next_id` and above every loaded ID.
    pub fn from_loaded(
        elements: impl IntoIterator<Item = (ElementId, NewElement)>,
        next_id: u64,
    ) -> Result<Self> {
        let mut doc = Self::new();
        doc.next_id = next_id.max(ElementId::FIRST.0);
        for (id, init) in elements {
            let guid = init.guid.clone().ok_or_else(|| {
                Error::new("element.no_guid", format!("element {id} has no GlobalId"))
            })?;
            if id.0 == 0 || doc.store.contains(id) {
                return Err(Error::new(
                    "element.duplicate_id",
                    format!("element ID {id} is zero or repeated"),
                ));
            }
            if doc.store.find_by_guid(&guid).is_some() {
                return Err(Error::new(
                    "element.duplicate_guid",
                    format!("GlobalId {guid} is repeated"),
                ));
            }
            doc.next_id = doc.next_id.max(id.0 + 1);
            doc.store.put(Arc::new(Element::new(id, guid, init)));
        }
        Ok(doc)
    }

    /// The ID the next created element will get.
    pub fn next_id(&self) -> ElementId {
        ElementId(self.next_id)
    }

    pub fn version(&self) -> ModelVersion {
        ModelVersion {
            episode: self.episode,
            counter: self.counter,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            store: self.store.clone(),
            version: self.version(),
        }
    }

    pub fn set_guid_mode(&mut self, mode: GuidMode) {
        self.guid_mode = mode;
    }

    pub fn set_max_undo_items(&mut self, n: usize) {
        self.max_undo_items = n;
        self.trim_undo();
    }

    /// Opens a transaction. Only one can be open at a time, which the borrow enforces.
    pub fn transaction(&mut self, name: impl Into<String>, origin: Origin) -> Transaction<'_> {
        Transaction::new(self, name.into(), origin)
    }

    pub fn stamp(&self, id: ElementId) -> Option<&Stamp> {
        self.stamps.get(&id)
    }

    pub fn changed_since(&self, since: ModelVersion) -> ChangedSince {
        if since.episode != self.episode {
            return ChangedSince {
                stale_episode: true,
                ..ChangedSince::default()
            };
        }
        let mut changed: Vec<_> = self
            .stamps
            .iter()
            .filter(|(_, s)| s.counter > since.counter)
            .map(|(id, _)| *id)
            .collect();
        changed.sort();
        let deleted = self
            .tombstones
            .iter()
            .filter(|(_, t)| t.counter > since.counter)
            .map(|(id, _)| *id)
            .collect();
        ChangedSince {
            changed,
            deleted,
            stale_episode: false,
        }
    }

    /// Undo items, newest first.
    pub fn undo_history(&self) -> impl Iterator<Item = &UndoItem> {
        self.undo.iter().rev()
    }

    /// Redo items, next first.
    pub fn redo_history(&self) -> impl Iterator<Item = &UndoItem> {
        self.redo.iter().rev()
    }

    /// Undoes the newest `steps` items as one change. Fails without changing anything if there are
    /// fewer items, a group is open, or an item is outside `scope`.
    pub fn undo(&mut self, steps: usize, scope: &UndoScope, origin: &Origin) -> Result<Commit> {
        self.check_steps(steps, scope, self.undo.iter().rev(), "undo")?;
        let items: Vec<_> = (0..steps).filter_map(|_| self.undo.pop_back()).collect();
        let changes = items
            .iter()
            .fold(ChangeSet::default(), |acc, i| acc.then(i.changes.inverse()));
        self.redo.extend(items);
        Ok(self.apply(changes, origin, Vec::new()))
    }

    pub fn redo(&mut self, steps: usize, scope: &UndoScope, origin: &Origin) -> Result<Commit> {
        self.check_steps(steps, scope, self.redo.iter().rev(), "redo")?;
        let items: Vec<_> = (0..steps).filter_map(|_| self.redo.pop()).collect();
        let changes = items
            .iter()
            .fold(ChangeSet::default(), |acc, i| acc.then(i.changes.clone()));
        self.undo.extend(items);
        self.trim_undo();
        Ok(self.apply(changes, origin, Vec::new()))
    }

    pub fn group_begin(&mut self, name: impl Into<String>, origin: Origin) -> Result<()> {
        if self.group.is_some() {
            return Err(Error::new(
                "txn.group_open",
                "a transaction group is already open",
            ));
        }
        self.group = Some(OpenGroup {
            name: name.into(),
            origin,
            mark: self.undo.len(),
        });
        Ok(())
    }

    pub fn group_end(&mut self, mode: GroupEnd) -> Result<Commit> {
        let group = self
            .group
            .take()
            .ok_or_else(|| Error::new("txn.no_group", "no transaction group is open"))?;
        let unchanged = Commit {
            version: self.version(),
            changed: ChangedSummary::default(),
            warnings: Vec::new(),
        };
        let result = match mode {
            GroupEnd::Commit => unchanged,
            GroupEnd::Assimilate => {
                let changes = self.drain_group(group.mark);
                if !changes.is_empty() {
                    let time = SystemTime::now();
                    self.undo.push_back(UndoItem {
                        name: group.name,
                        origin: group.origin,
                        time,
                        changes,
                    });
                }
                unchanged
            }
            GroupEnd::Rollback => {
                let changes = self.drain_group(group.mark);
                self.apply(changes.inverse(), &group.origin, Vec::new())
            }
        };
        self.trim_undo();
        Ok(result)
    }

    pub(crate) fn allocate_id(&mut self) -> ElementId {
        let id = ElementId(self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) fn new_guid(&self) -> GlobalId {
        match self.guid_mode {
            GuidMode::Random => GlobalId::new(),
            GuidMode::TimeOrdered => GlobalId::new_time_ordered(),
        }
    }

    pub(crate) fn is_tombstoned(&self, guid: &GlobalId) -> bool {
        self.tombstones.values().any(|t| &t.guid == guid)
    }

    pub(crate) fn commit_changes(
        &mut self,
        name: String,
        origin: Origin,
        changes: ChangeSet,
        warnings: Vec<Warning>,
    ) -> Commit {
        if changes.is_empty() {
            return Commit {
                version: self.version(),
                changed: ChangedSummary::default(),
                warnings,
            };
        }
        let commit = self.apply(changes.clone(), &origin, warnings);
        self.redo.clear();
        self.undo.push_back(UndoItem {
            name,
            origin,
            time: SystemTime::now(),
            changes,
        });
        self.trim_undo();
        commit
    }

    /// Applies a change set as one new model version and stamps every element it touches.
    fn apply(&mut self, changes: ChangeSet, origin: &Origin, warnings: Vec<Warning>) -> Commit {
        if changes.is_empty() {
            return Commit {
                version: self.version(),
                changed: ChangedSummary::default(),
                warnings,
            };
        }
        changes.apply_to(&mut self.store);
        self.counter += 1;
        let (by, at) = (origin.label(), SystemTime::now());
        for (id, c) in changes.iter() {
            match (&c.before, &c.after) {
                (_, Some(_)) => {
                    self.tombstones.remove(&id);
                    self.stamps.insert(
                        id,
                        Stamp {
                            counter: self.counter,
                            by: by.clone(),
                            at,
                        },
                    );
                }
                (Some(before), None) => {
                    self.stamps.remove(&id);
                    self.tombstones.insert(
                        id,
                        Tombstone {
                            guid: before.guid().clone(),
                            counter: self.counter,
                        },
                    );
                }
                (None, None) => {}
            }
        }
        Commit {
            version: self.version(),
            changed: changes.summary(),
            warnings,
        }
    }

    fn check_steps<'a>(
        &self,
        steps: usize,
        scope: &UndoScope,
        mut items: impl Iterator<Item = &'a UndoItem>,
        what: &str,
    ) -> Result<()> {
        if self.group.is_some() {
            return Err(Error::new(
                "txn.group_open",
                format!("cannot {what} while a transaction group is open"),
            ));
        }
        for _ in 0..steps {
            let item = items
                .next()
                .ok_or_else(|| Error::new(format!("{what}.empty"), format!("nothing to {what}")))?;
            if !scope.allows(item) {
                return Err(Error::new(
                    format!("{what}.not_owned"),
                    format!("\"{}\" was made by another session", item.name),
                ));
            }
        }
        Ok(())
    }

    fn drain_group(&mut self, mark: usize) -> ChangeSet {
        let mark = mark.min(self.undo.len());
        self.undo
            .drain(mark..)
            .fold(ChangeSet::default(), |acc, i| acc.then(i.changes))
    }

    /// Drops the oldest items beyond the limit, except while a group is open.
    fn trim_undo(&mut self) {
        if self.group.is_none() {
            while self.undo.len() > self.max_undo_items {
                self.undo.pop_front();
            }
        }
    }
}
