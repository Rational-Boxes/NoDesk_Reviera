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
        registry.add(UnitsGet),
        registry.add(UnitsSetFormat),
        registry.add(UnitsSetPreset),
        registry.add(UnitsSetNumberFormat),
        registry.add(UnitsFormat),
        registry.add(UnitsParse),
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

// ---- Units (§2.9.5, DM-UNIT-001) ----

use crate::element::NewElement;
use crate::units::{self, Format, Preset, UnitSettings};

/// Stores unit settings on the project-settings element, creating it when needed.
fn write_settings(cx: &mut CommandContext<'_>, settings: UnitSettings) -> Result<()> {
    settings.validate()?;
    let value = serde_json::to_value(&settings).expect("settings serialise");
    let txn = cx.edit()?;
    if txn.store().contains(ElementId::PROJECT_SETTINGS) {
        txn.modify(ElementId::PROJECT_SETTINGS, |e| {
            e.params.insert(units::SETTINGS_KEY.into(), value);
        })
    } else {
        let init = NewElement::new(units::SETTINGS_CLASS)
            .name("Project Settings")
            .param(units::SETTINGS_KEY, value);
        txn.create_with_id(ElementId::PROJECT_SETTINGS, init)
    }
}

const UNITS_EDIT: &[Effect] = &[Effect::Edit, Effect::Settings];

#[derive(Debug)]
struct UnitsGet;

impl Command for UnitsGet {
    type Params = NoParams;
    type Output = UnitSettings;
    const META: CommandMeta = CommandMeta {
        name: "units.get",
        version: 1,
        summary: "Get the project's display units",
        description: "Returns the display format of each data type and the number separators.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<UnitSettings> {
        Ok(units::settings_of(cx.store()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFormatParams {
    pub data_type: String,
    pub format: Format,
}

#[derive(Debug)]
struct UnitsSetFormat;

impl Command for UnitsSetFormat {
    type Params = SetFormatParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "units.set_format",
        version: 1,
        summary: "Set the display format of a data type",
        description: "Sets unit, rounding, symbol and flags for one data type. Stored values do not change.",
        effects: UNITS_EDIT,
        transaction_name: "Project Units",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetFormatParams) -> Result<()> {
        let t = units::data_type(&p.data_type).ok_or_else(|| {
            Error::new(
                "units.unknown_data_type",
                format!("no data type `{}`", p.data_type),
            )
        })?;
        units::validate_format(t, &p.format)?;
        let mut settings = units::settings_of(cx.store());
        settings.formats.insert(p.data_type, p.format);
        write_settings(cx, settings)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetParams {
    pub preset: Preset,
}

#[derive(Debug)]
struct UnitsSetPreset;

impl Command for UnitsSetPreset {
    type Params = PresetParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "units.set_preset",
        version: 1,
        summary: "Apply a units preset",
        description: "Metric (mm), Metric (m), Imperial (ft-in) or Imperial (decimal ft). Separators are kept.",
        effects: UNITS_EDIT,
        transaction_name: "Project Units",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: PresetParams) -> Result<()> {
        let current = units::settings_of(cx.store());
        let settings = UnitSettings {
            decimal_symbol: current.decimal_symbol,
            grouping_symbol: current.grouping_symbol,
            indian_grouping: current.indian_grouping,
            ..UnitSettings::preset(p.preset)
        };
        write_settings(cx, settings)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NumberFormatParams {
    /// `.` or `,`.
    pub decimal_symbol: String,
    /// `,`, `.`, ` ` or `'`.
    pub digit_grouping: String,
    /// Group as `12,34,567`.
    #[serde(default)]
    pub indian_grouping: bool,
}

#[derive(Debug)]
struct UnitsSetNumberFormat;

impl Command for UnitsSetNumberFormat {
    type Params = NumberFormatParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "units.set_number_format",
        version: 1,
        summary: "Set the decimal symbol and digit grouping",
        description: "The two symbols must differ.",
        effects: UNITS_EDIT,
        transaction_name: "Project Units",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: NumberFormatParams) -> Result<()> {
        if ![",", ".", " ", "'"].contains(&p.digit_grouping.as_str()) {
            return Err(Error::new(
                "units.invalid_format",
                "digit grouping is `,`, `.`, a space or `'`",
            ));
        }
        let settings = UnitSettings {
            decimal_symbol: p.decimal_symbol,
            grouping_symbol: p.digit_grouping,
            indian_grouping: p.indian_grouping,
            ..units::settings_of(cx.store())
        };
        write_settings(cx, settings)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormatParams {
    /// SI value.
    pub value: f64,
    pub data_type: String,
    /// Overrides the project format.
    pub format: Option<Format>,
}

#[derive(Debug)]
struct UnitsFormat;

impl Command for UnitsFormat {
    type Params = FormatParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "units.format",
        version: 1,
        summary: "Format an SI value for display",
        description: "Uses the project's format for the data type unless one is given.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: FormatParams) -> Result<String> {
        units::format(
            p.value,
            &p.data_type,
            &units::settings_of(cx.store()),
            p.format.as_ref(),
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParseParams {
    pub text: String,
    pub data_type: String,
}

#[derive(Debug)]
struct UnitsParse;

impl Command for UnitsParse {
    type Params = ParseParams;
    type Output = f64;
    const META: CommandMeta = CommandMeta {
        name: "units.parse",
        version: 1,
        summary: "Parse a value with units into SI",
        description: "Accepts any unit of the data type's dimension; a bare number is in the display unit.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParseParams) -> Result<f64> {
        units::parse(&p.text, &p.data_type, &units::settings_of(cx.store()))
    }
}
