// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element database, transactions and undo, regeneration graph and the command registry.
//!
//! Every operation is a typed command registered once and surfaced to the UI, Lua and MCP
//! (TECH_STACK §2, spec Ch 2).

pub mod command;
pub mod id;
