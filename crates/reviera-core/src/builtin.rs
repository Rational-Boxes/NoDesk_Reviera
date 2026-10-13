// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Core commands: undo and redo (DM-TXN-002), identity and versions (DM-ID-001, DM-ID-002) and
//! registry introspection (DM-CMD-001).
//!
//! `registry.execute` is [`CommandRegistry::execute`] itself; the Lua and MCP layers expose it as
//! `reviera.run` and `run_command`. The `txn.*` primitives are exposed by the Lua layer, which
//! holds a transaction open across script calls.

use std::time::UNIX_EPOCH;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandContext, CommandMeta, CommandRegistry, CommandSpec, Effect};
use crate::document::{ChangedSince, ModelVersion, UndoScope};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::origin::{Caller, Origin, Permission, Source};

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add_direct(EditUndo),
        registry.add_direct(EditRedo),
        registry.add(EditHistory),
        registry.add(ModelVersionCmd),
        registry.add(ElementChangedSince),
        registry.add(ElementGetIds),
        registry.add(ElementFindByGuid),
        registry.add(RegistryList),
        registry.add(RegistryDescribe),
    ];
    for r in results {
        r.expect("core commands are valid");
    }
}

/// Arguments of a command that takes none.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepsParams {
    #[serde(default = "one")]
    #[schemars(range(min = 1))]
    pub steps: usize,
}

fn one() -> usize {
    1
}

/// MCP sessions below `full_scripting` may only undo their own items (DM-TXN-002).
fn undo_scope(caller: &Caller) -> UndoScope {
    match (&caller.origin.source, &caller.origin.session_id) {
        (Source::Mcp, Some(session)) if caller.permission < Permission::FullScripting => {
            UndoScope::Session(session.clone())
        }
        (Source::Mcp, None) if caller.permission < Permission::FullScripting => {
            UndoScope::Session(String::new())
        }
        _ => UndoScope::All,
    }
}

#[derive(Debug)]
struct EditUndo;

impl Command for EditUndo {
    type Params = StepsParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "edit.undo",
        version: 1,
        summary: "Undo the most recent operations",
        description: "Undoes the newest `steps` items in the undo history as one change.",
        effects: &[Effect::Edit],
        transaction_name: "Undo",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: StepsParams) -> Result<()> {
        let (scope, origin) = (undo_scope(cx.caller()), cx.caller().origin.clone());
        cx.direct_commit = Some(cx.document_mut()?.undo(p.steps, &scope, &origin)?);
        Ok(())
    }
}

#[derive(Debug)]
struct EditRedo;

impl Command for EditRedo {
    type Params = StepsParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "edit.redo",
        version: 1,
        summary: "Redo undone operations",
        description: "Redoes the next `steps` items in the redo history as one change.",
        effects: &[Effect::Edit],
        transaction_name: "Redo",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: StepsParams) -> Result<()> {
        let (scope, origin) = (undo_scope(cx.caller()), cx.caller().origin.clone());
        cx.direct_commit = Some(cx.document_mut()?.redo(p.steps, &scope, &origin)?);
        Ok(())
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryEntry {
    pub name: String,
    pub origin: Origin,
    /// Milliseconds since the Unix epoch.
    pub time: u64,
}

#[derive(Debug)]
struct EditHistory;

impl Command for EditHistory {
    type Params = NoParams;
    type Output = Vec<HistoryEntry>;
    const META: CommandMeta = CommandMeta {
        name: "edit.history",
        version: 1,
        summary: "List the undo history",
        description: "Lists undo items, newest first, with the origin of each.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<Vec<HistoryEntry>> {
        Ok(cx
            .document()?
            .undo_history()
            .map(|i| HistoryEntry {
                name: i.name.clone(),
                origin: i.origin.clone(),
                time: i
                    .time
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            })
            .collect())
    }
}

#[derive(Debug)]
struct ModelVersionCmd;

impl Command for ModelVersionCmd {
    type Params = NoParams;
    type Output = ModelVersion;
    const META: CommandMeta = CommandMeta {
        name: "model.version",
        version: 1,
        summary: "Get the model version",
        description: "Returns the episode GUID and transaction counter, for cheap change detection.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<ModelVersion> {
        Ok(cx.document()?.version())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangedSinceParams {
    pub version: ModelVersion,
}

#[derive(Debug)]
struct ElementChangedSince;

impl Command for ElementChangedSince {
    type Params = ChangedSinceParams;
    type Output = ChangedSince;
    const META: CommandMeta = CommandMeta {
        name: "element.changed_since",
        version: 1,
        summary: "List elements changed since a model version",
        description: "Returns changed and deleted elements; `stale_episode` means re-read everything.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ChangedSinceParams) -> Result<ChangedSince> {
        Ok(cx.document()?.changed_since(p.version))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetIdsParams {
    pub element_ids: Vec<ElementRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IdPair {
    pub id: ElementId,
    pub guid: GlobalId,
}

#[derive(Debug)]
struct ElementGetIds;

impl Command for ElementGetIds {
    type Params = GetIdsParams;
    type Output = Vec<IdPair>;
    const META: CommandMeta = CommandMeta {
        name: "element.get_ids",
        version: 1,
        summary: "Get internal IDs and GUIDs",
        description: "Returns the internal ID and GUID of each element, given either form.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: GetIdsParams) -> Result<Vec<IdPair>> {
        let store = cx.store();
        p.element_ids
            .iter()
            .map(|r| {
                let e = store
                    .resolve(r)
                    .and_then(|id| store.get(id))
                    .ok_or_else(|| {
                        Error::new("element.not_found", "no such element")
                            .with_details(serde_json::json!({ "ref": r }))
                    })?;
                Ok(IdPair {
                    id: e.id(),
                    guid: e.guid().clone(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindByGuidParams {
    pub guid: GlobalId,
}

#[derive(Debug)]
struct ElementFindByGuid;

impl Command for ElementFindByGuid {
    type Params = FindByGuidParams;
    type Output = Option<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "element.find_by_guid",
        version: 1,
        summary: "Find an element by GUID",
        description: "Returns the internal ID of the element with this GUID, or null.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: FindByGuidParams) -> Result<Option<ElementId>> {
        Ok(cx.store().find_by_guid(&p.guid))
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryFilter {
    /// Name prefix, for example `wall.`.
    pub prefix: Option<String>,
    pub effect: Option<Effect>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryListParams {
    #[serde(default)]
    pub filter: Option<RegistryFilter>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommandSummary {
    pub name: String,
    pub version: u32,
    pub summary: String,
}

#[derive(Debug)]
struct RegistryList;

impl Command for RegistryList {
    type Params = RegistryListParams;
    type Output = Vec<CommandSummary>;
    const META: CommandMeta = CommandMeta {
        name: "registry.list",
        version: 1,
        summary: "List commands",
        description: "Lists registered commands, optionally filtered by name prefix or effect.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: RegistryListParams,
    ) -> Result<Vec<CommandSummary>> {
        let f = p.filter.unwrap_or_default();
        Ok(cx
            .registry()
            .iter()
            .filter(|s| {
                f.prefix
                    .as_ref()
                    .is_none_or(|p| s.name.starts_with(p.as_str()))
            })
            .filter(|s| f.effect.is_none_or(|e| s.effects.contains(&e)))
            .map(|s| CommandSummary {
                name: s.name.clone(),
                version: s.version,
                summary: s.summary.clone(),
            })
            .collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeParams {
    pub name: String,
}

#[derive(Debug)]
struct RegistryDescribe;

impl Command for RegistryDescribe {
    type Params = DescribeParams;
    type Output = CommandSpec;
    const META: CommandMeta = CommandMeta {
        name: "registry.describe",
        version: 1,
        summary: "Describe a command",
        description: "Returns a command's full definition, including its parameter and result schemas.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: DescribeParams) -> Result<CommandSpec> {
        cx.registry().get(&p.name).cloned().ok_or_else(|| {
            Error::new(
                "registry.unknown_command",
                format!("no command named `{}`", p.name),
            )
        })
    }
}
