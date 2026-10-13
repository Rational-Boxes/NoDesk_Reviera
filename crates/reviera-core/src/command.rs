// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The single command registry. Each command is described once and surfaced automatically to the
//! ribbon and shortcuts, to Lua as `reviera.<command>(...)`, and to MCP as one tool per command.

use std::collections::BTreeMap;

use serde::Serialize;

/// How a command interacts with the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoBehaviour {
    /// Runs in a transaction and records one undo step.
    Undoable,
    /// Changes no model state (queries, view navigation).
    ReadOnly,
    /// Joins the enclosing transaction group of a multi-step tool.
    Grouped,
}

/// Declarative description of a command, the source of its UI, Lua and MCP surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSpec {
    /// Dotted canonical name, e.g. `wall.create`.
    pub name: &'static str,
    pub summary: &'static str,
    /// JSON Schema of the parameters.
    pub params: serde_json::Value,
    /// JSON Schema of the result.
    pub returns: serde_json::Value,
    pub undo: UndoBehaviour,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("command `{0}` is already registered")]
    Duplicate(&'static str),
    #[error("command name `{0}` is not a dotted lowercase identifier")]
    InvalidName(&'static str),
}

#[derive(Debug, Default)]
pub struct CommandRegistry {
    commands: BTreeMap<&'static str, CommandSpec>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: CommandSpec) -> Result<(), RegistryError> {
        let valid = spec.name.split('.').count() >= 2
            && spec.name.split('.').all(|part| {
                part.starts_with(|c: char| c.is_ascii_lowercase())
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            });
        if !valid {
            return Err(RegistryError::InvalidName(spec.name));
        }
        if self.commands.contains_key(spec.name) {
            return Err(RegistryError::Duplicate(spec.name));
        }
        self.commands.insert(spec.name, spec);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.get(name)
    }

    /// Commands in name order.
    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.commands.values()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(name: &'static str) -> CommandSpec {
        CommandSpec {
            name,
            summary: "",
            params: json!({"type": "object"}),
            returns: json!({}),
            undo: UndoBehaviour::Undoable,
        }
    }

    #[test]
    fn registers_and_rejects_duplicates() {
        let mut r = CommandRegistry::new();
        r.register(spec("wall.create")).unwrap();
        assert_eq!(
            r.register(spec("wall.create")),
            Err(RegistryError::Duplicate("wall.create"))
        );
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn rejects_malformed_names() {
        let mut r = CommandRegistry::new();
        for bad in ["wall", "Wall.create", "wall..create", "wall.2d"] {
            assert_eq!(r.register(spec(bad)), Err(RegistryError::InvalidName(bad)));
        }
        r.register(spec("view.set_scale")).unwrap();
    }
}
