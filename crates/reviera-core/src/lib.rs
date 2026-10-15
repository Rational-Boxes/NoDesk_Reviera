// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element database, transactions and undo, regeneration graph and the command registry.
//!
//! Every operation is a typed command registered once and surfaced to the UI, Lua and MCP
//! (TECH_STACK §2, spec Ch 2). The element store can only be changed inside a [`Transaction`],
//! and transactions are opened by the command executor (DM-CORE-001 rule 9).

pub mod annotations;
pub mod arrangement;
pub mod bodies;
pub mod builtin;
mod builtin_arc;
pub mod categories;
pub mod change;
pub mod clearances;
pub mod cobie;
pub mod codes;
pub mod columns;
pub mod command;
pub mod compound;
pub mod csg;
pub mod curtain;
pub mod decks;
pub mod document;
pub mod dormers;
pub mod element;
pub mod error;
pub mod floors;
pub mod grids;
pub mod id;
pub mod inserts;
pub mod inspect;
pub mod levels;
pub mod measure;
pub mod modify;
pub mod notes;
pub mod origin;
pub mod overlap;
pub mod overlap_accept;
pub mod parameters;
pub mod plans;
pub mod polygons;
pub mod railings;
pub mod ramps;
pub mod roofs;
pub mod rooms;
pub mod schedules;
pub mod sections;
pub mod sheets;
pub mod sketch;
pub mod sketch_edit;
pub mod skylights;
pub mod slab_openings;
pub mod stairs;
pub mod standards;
pub mod store;
pub mod txn;
pub mod types;
pub mod units;
pub mod vaults;
pub mod views;
pub mod wall_openings;
pub mod walls;

pub use change::{ChangeRecord, ChangeSet, ChangedSummary};
pub use command::{
    Command, CommandContext, CommandMeta, CommandRegistry, CommandSpec, Effect, HostCommand,
    Outcome,
};
pub use document::{DocEvent, Document, GroupEnd, ModelVersion, Snapshot, UndoItem, UndoScope};
pub use element::{Element, NewElement};
pub use error::{Error, Result};
pub use id::{ElementId, ElementRef, GlobalId};
pub use origin::{Caller, Origin, Permission, Source};
pub use store::Store;
pub use txn::{Savepoint, Transaction, Warning};
