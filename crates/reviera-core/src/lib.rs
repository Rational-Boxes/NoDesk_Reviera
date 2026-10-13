// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element database, transactions and undo, regeneration graph and the command registry.
//!
//! Every operation is a typed command registered once and surfaced to the UI, Lua and MCP
//! (TECH_STACK §2, spec Ch 2). The element store can only be changed inside a [`Transaction`],
//! and transactions are opened by the command executor (DM-CORE-001 rule 9).

pub mod builtin;
pub mod change;
pub mod command;
pub mod document;
pub mod element;
pub mod error;
pub mod id;
pub mod origin;
pub mod store;
pub mod txn;

pub use change::{ChangeSet, ChangedSummary};
pub use command::{
    Command, CommandContext, CommandMeta, CommandRegistry, CommandSpec, Effect, Outcome,
};
pub use document::{Document, GroupEnd, ModelVersion, Snapshot, UndoScope};
pub use element::{Element, NewElement};
pub use error::{Error, Result};
pub use id::{ElementId, ElementRef, GlobalId};
pub use origin::{Caller, Origin, Permission, Source};
pub use store::Store;
pub use txn::{Savepoint, Transaction, Warning};
