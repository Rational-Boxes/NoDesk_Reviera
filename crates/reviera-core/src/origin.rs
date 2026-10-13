// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Who asked for a change: attribution for undo items and change stamps (DM-TXN-002, DM-ID-002),
//! and the permission level the request runs under (DM-MCP-005).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Ui,
    Lua,
    Addin,
    Mcp,
    /// Changes merged from a BIMserver revision.
    Sync,
}

/// Origin of a transaction. Never holds credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Origin {
    pub source: Source,
    /// Script name, add-in ID, MCP client name or sync author; `user` for the UI.
    pub actor: String,
    pub session_id: Option<String>,
}

impl Origin {
    pub fn ui() -> Self {
        Self {
            source: Source::Ui,
            actor: "user".into(),
            session_id: None,
        }
    }

    pub fn mcp(client: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            source: Source::Mcp,
            actor: client.into(),
            session_id: Some(session_id.into()),
        }
    }

    /// The `last_modified_by` form: `user`, `lua:<script>`, `addin:<id>`, `mcp:<client>`, `sync:<author>`.
    pub fn label(&self) -> String {
        match self.source {
            Source::Ui => "user".into(),
            Source::Lua => format!("lua:{}", self.actor),
            Source::Addin => format!("addin:{}", self.actor),
            Source::Mcp => format!("mcp:{}", self.actor),
            Source::Sync => format!("sync:{}", self.actor),
        }
    }
}

/// Permission levels, ordered from least to most capable (DM-MCP-005).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ReadOnly,
    ModelEdit,
    FullScripting,
}

/// A command request's origin and the permission level it runs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub origin: Origin,
    pub permission: Permission,
}

impl Caller {
    /// The interactive user, who holds every permission.
    pub fn user() -> Self {
        Self {
            origin: Origin::ui(),
            permission: Permission::FullScripting,
        }
    }
}
