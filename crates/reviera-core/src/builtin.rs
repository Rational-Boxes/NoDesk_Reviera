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
        registry.add(ElementDelete),
        registry.add(RegistryList),
        registry.add(RegistryDescribe),
        registry.add(UnitsGet),
        registry.add(UnitsSetFormat),
        registry.add(UnitsSetPreset),
        registry.add(UnitsSetNumberFormat),
        registry.add(UnitsFormat),
        registry.add(UnitsParse),
        registry.add(ParamGet),
        registry.add(ParamSet),
        registry.add(ParamSetMany),
        registry.add(ParamClear),
        registry.add(ParamList),
        registry.add(ParamDefinition),
        registry.add(ParamProjectAdd),
        registry.add(ParamProjectModify),
        registry.add(ParamProjectRemove),
        registry.add(ParamSetIfcHome),
        registry.add(WallCreate),
        registry.add(WallSetLocation),
        registry.add(WallSetJoinAllowed),
        registry.add(WallFlip),
        registry.add(ViewCreate3d),
        registry.add(ViewList),
        registry.add(ViewRename),
        registry.add(ViewDuplicate),
        registry.add(ViewDelete),
        registry.add(ViewSetCamera),
        registry.add(ViewCreatePlan),
        registry.add(ViewSetViewRange),
        registry.add(ViewGetViewRange),
        registry.add(DimCreateAligned),
        registry.add(AnnoList),
        registry.add(LineWeightSet),
        registry.add(LineWeightAddScale),
        registry.add(LineWeightRemoveScale),
        registry.add(ObjectStyleSet),
        registry.add(WallJoinsQuery),
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub element_ids: Vec<ElementRef>,
    /// Also delete the elements that cannot exist without these (instances of a deleted type,
    /// walls on a deleted level). With `false`, the command fails if there are any.
    #[serde(default = "yes")]
    pub confirm_dependents: bool,
}

fn yes() -> bool {
    true
}

/// Elements that depend on `id`: instances of a type, and walls based on or bounded by a level.
fn dependents_of(store: &crate::store::Store, id: ElementId) -> Vec<ElementId> {
    let Some(guid) = store.get(id).map(|e| e.guid().as_str().to_owned()) else {
        return Vec::new();
    };
    store
        .iter()
        .filter(|e| {
            crate::annotations::dimension_data(e).is_some_and(|d| d.view == guid)
                || e.param("@type").and_then(serde_json::Value::as_str) == Some(guid.as_str())
                || crate::views::view_data(e)
                    .is_some_and(|v| v.level.as_deref() == Some(guid.as_str()))
                || crate::walls::wall_recipe(e).is_some_and(|r| {
                    r.base_level == guid
                        || r.top.get("level").and_then(serde_json::Value::as_str)
                            == Some(guid.as_str())
                })
        })
        .map(|e| e.id())
        .collect()
}

#[derive(Debug)]
struct ElementDelete;

impl Command for ElementDelete {
    type Params = DeleteParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "element.delete",
        version: 1,
        summary: "Delete elements",
        description: "Deletes elements and returns every deleted ID, including dependents (instances of a \
                      deleted type, walls on a deleted level). The project and its settings cannot be deleted.",
        effects: &[Effect::Edit, Effect::Destructive],
        transaction_name: "Delete",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: DeleteParams) -> Result<Vec<ElementId>> {
        let txn = cx.edit()?;
        let mut queue = Vec::new();
        for r in &p.element_ids {
            let id = txn
                .store()
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            if id.0 < ElementId::FIRST.0 {
                return Err(Error::new(
                    "element.protected",
                    format!("element {id} is part of the project and cannot be deleted"),
                ));
            }
            queue.push(id);
        }
        let explicit: std::collections::BTreeSet<ElementId> = queue.iter().copied().collect();
        let mut deleted = std::collections::BTreeSet::new();
        while let Some(id) = queue.pop() {
            if !deleted.insert(id) {
                continue;
            }
            for d in dependents_of(txn.store(), id) {
                if !p.confirm_dependents && !explicit.contains(&d) {
                    return Err(Error::new(
                        "element.has_dependents",
                        format!(
                            "element {d} depends on element {id}; pass confirm_dependents to delete it too"
                        ),
                    ));
                }
                queue.push(d);
            }
        }
        for id in &deleted {
            txn.delete(*id)?;
        }
        Ok(deleted.into_iter().collect())
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

// ---- Parameters (§2.9.1–§2.9.3, DM-PARAM-001…003) ----

use crate::parameters::{
    self, Changes, Definition, GroupBehavior, ModifyReport, NewDefinition, ParamValue,
};
use serde_json::Value;

fn resolve_ref(cx: &CommandContext<'_>, r: &ElementRef) -> Result<ElementId> {
    cx.store().resolve(r).ok_or_else(|| {
        Error::new("element.not_found", "no such element")
            .with_details(serde_json::json!({ "ref": r }))
    })
}

fn resolve_refs(cx: &CommandContext<'_>, refs: &[ElementRef]) -> Result<Vec<ElementId>> {
    refs.iter().map(|r| resolve_ref(cx, r)).collect()
}

const PARAM_EDIT: &[Effect] = &[Effect::Edit];
const PROJECT_PARAM_EDIT: &[Effect] = &[Effect::Edit, Effect::Settings];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamGetParams {
    pub element_id: ElementRef,
    /// Built-in key (`elem.mark`), project parameter name or GUID, or `<Pset>.<Property>`.
    pub param_ref: String,
}

#[derive(Debug)]
struct ParamGet;

impl Command for ParamGet {
    type Params = ParamGetParams;
    type Output = ParamValue;
    const META: CommandMeta = CommandMeta {
        name: "param.get",
        version: 1,
        summary: "Get a parameter value",
        description: "Returns the value (SI for measures), its display text and where it comes from: \
                      instance, type, override (an occurrence masking its type) or computed.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamGetParams) -> Result<ParamValue> {
        let id = resolve_ref(cx, &p.element_id)?;
        parameters::get(cx.store(), id, &p.param_ref)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamSetParams {
    pub element_ids: Vec<ElementRef>,
    pub param_ref: String,
    /// Text, number, boolean, or a value with units ("2400 mm"); null stores an empty value.
    pub value: Value,
}

#[derive(Debug)]
struct ParamSet;

impl Command for ParamSet {
    type Params = ParamSetParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "param.set",
        version: 1,
        summary: "Set a parameter on elements",
        description: "Type parameters are written to each element's type (once per type). Returns the \
                      elements written. Computed parameters fail with param.read_only.",
        effects: PARAM_EDIT,
        transaction_name: "Modify parameter",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamSetParams) -> Result<Vec<ElementId>> {
        let ids = resolve_refs(cx, &p.element_ids)?;
        parameters::set(cx.edit()?, &ids, &p.param_ref, Some(&p.value))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamChange {
    pub element_id: ElementRef,
    pub param_ref: String,
    pub value: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamSetManyParams {
    pub changes: Vec<ParamChange>,
}

#[derive(Debug)]
struct ParamSetMany;

impl Command for ParamSetMany {
    type Params = ParamSetManyParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "param.set_many",
        version: 1,
        summary: "Set several parameter values in one step",
        description: "All changes are one undo item; if any fails, none is applied.",
        effects: PARAM_EDIT,
        transaction_name: "Modify parameters",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamSetManyParams) -> Result<Vec<ElementId>> {
        let mut written = Vec::new();
        for c in &p.changes {
            let id = resolve_ref(cx, &c.element_id)?;
            written.extend(parameters::set(
                cx.edit()?,
                &[id],
                &c.param_ref,
                Some(&c.value),
            )?);
        }
        written.sort();
        written.dedup();
        Ok(written)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamClearParams {
    pub element_ids: Vec<ElementRef>,
    pub param_ref: String,
}

#[derive(Debug)]
struct ParamClear;

impl Command for ParamClear {
    type Params = ParamClearParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "param.clear",
        version: 1,
        summary: "Clear a parameter on elements",
        description: "Removes the stored value (for type parameters, from the type).",
        effects: PARAM_EDIT,
        transaction_name: "Clear parameter",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamClearParams) -> Result<Vec<ElementId>> {
        let ids = resolve_refs(cx, &p.element_ids)?;
        parameters::set(cx.edit()?, &ids, &p.param_ref, None)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamListParams {
    /// A category key such as `cat.walls`.
    pub category: Option<String>,
    pub element_id: Option<ElementRef>,
}

#[derive(Debug)]
struct ParamList;

impl Command for ParamList {
    type Params = ParamListParams;
    type Output = Vec<Definition>;
    const META: CommandMeta = CommandMeta {
        name: "param.list",
        version: 1,
        summary: "List parameter definitions",
        description: "Built-in, project and shared parameters for a category or element, with data type and \
                      IFC home; for an element, also its IFC-native properties.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamListParams) -> Result<Vec<Definition>> {
        let element = p
            .element_id
            .as_ref()
            .map(|r| resolve_ref(cx, r))
            .transpose()?;
        parameters::list(cx.store(), p.category.as_deref(), element)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamRefParams {
    pub param_ref: String,
}

#[derive(Debug)]
struct ParamDefinition;

impl Command for ParamDefinition {
    type Params = ParamRefParams;
    type Output = Definition;
    const META: CommandMeta = CommandMeta {
        name: "param.definition",
        version: 1,
        summary: "Get a parameter definition",
        description: "",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ParamRefParams) -> Result<Definition> {
        parameters::definition(cx.store(), &p.param_ref)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectAddParams {
    pub def: NewDefinition,
    pub categories: Vec<String>,
    pub is_instance: bool,
    #[serde(default)]
    pub group_behavior: GroupBehavior,
    /// Defaults to "<Project name> Parameters".
    pub pset_name: Option<String>,
}

#[derive(Debug)]
struct ParamProjectAdd;

impl Command for ParamProjectAdd {
    type Params = ProjectAddParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "param.project.add",
        version: 1,
        summary: "Add a project or shared parameter",
        description: "Binds a new parameter to categories as a type or instance parameter; returns its GUID. \
                      A name that clashes with a built-in or existing parameter of those categories is refused.",
        effects: PROJECT_PARAM_EDIT,
        transaction_name: "Project Parameters",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ProjectAddParams) -> Result<String> {
        parameters::project_add(
            cx.edit()?,
            p.def,
            p.categories,
            p.is_instance,
            p.group_behavior,
            p.pset_name,
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectModifyParams {
    pub param_id: String,
    pub changes: Changes,
}

#[derive(Debug)]
struct ParamProjectModify;

impl Command for ParamProjectModify {
    type Params = ProjectModifyParams;
    type Output = ModifyReport;
    const META: CommandMeta = CommandMeta {
        name: "param.project.modify",
        version: 1,
        summary: "Change a project parameter's binding",
        description: "Renames, regroups, rebinds or switches between type and instance, moving values. \
                      Switching to type when instances differ needs a resolution: most_common, first or clear.",
        effects: PROJECT_PARAM_EDIT,
        transaction_name: "Project Parameters",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ProjectModifyParams) -> Result<ModifyReport> {
        parameters::project_modify(cx.edit()?, &p.param_id, p.changes)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectRemoveParams {
    pub param_id: String,
}

#[derive(Debug)]
struct ParamProjectRemove;

impl Command for ParamProjectRemove {
    type Params = ProjectRemoveParams;
    type Output = usize;
    const META: CommandMeta = CommandMeta {
        name: "param.project.remove",
        version: 1,
        summary: "Remove a project parameter and its values",
        description: "Returns the number of values removed.",
        effects: &[Effect::Edit, Effect::Settings, Effect::Destructive],
        transaction_name: "Project Parameters",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ProjectRemoveParams) -> Result<usize> {
        parameters::project_remove(cx.edit()?, &p.param_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetIfcHomeParams {
    pub param_ref: String,
    /// Omit to map every bound category.
    pub category: Option<String>,
    pub pset_name: String,
    pub property_name: String,
}

#[derive(Debug)]
struct ParamSetIfcHome;

impl Command for ParamSetIfcHome {
    type Params = SetIfcHomeParams;
    type Output = usize;
    const META: CommandMeta = CommandMeta {
        name: "param.set_ifc_home",
        version: 1,
        summary: "Map a parameter onto an IFC property",
        description: "For example a shared Fire Rating onto Pset_WallCommon.FireRating for walls. Existing \
                      values move; returns how many.",
        effects: PROJECT_PARAM_EDIT,
        transaction_name: "Project Parameters",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetIfcHomeParams) -> Result<usize> {
        parameters::set_ifc_home(
            cx.edit()?,
            &p.param_ref,
            p.category.as_deref(),
            &p.pset_name,
            &p.property_name,
        )
    }
}

// ---- Walls (Ch 3 ARC-WALL-002) ----

use crate::walls::{
    self, CreateParams, FlipParams, JoinsQueryParams, SetJoinAllowedParams, SetLocationParams,
};

#[derive(Debug)]
struct WallCreate;

impl Command for WallCreate {
    type Params = CreateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "wall.create",
        version: 1,
        summary: "Create a wall",
        description: "Creates a straight wall along `curve.line` (plan coordinates in metres), from `base_level` \
                      plus `base_offset` up to a level or an unconnected height. Lengths accept units (\"3 m\").",
        effects: &[Effect::Edit],
        transaction_name: "Wall",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: CreateParams) -> Result<ElementId> {
        walls::create(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct WallSetLocation;

impl Command for WallSetLocation {
    type Params = SetLocationParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "wall.set_location",
        version: 1,
        summary: "Move a wall's location line",
        description: "Replaces the wall's location curve; the wall regenerates.",
        effects: &[Effect::Edit],
        transaction_name: "Move wall",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetLocationParams) -> Result<()> {
        walls::set_location(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct WallSetJoinAllowed;

impl Command for WallSetJoinAllowed {
    type Params = SetJoinAllowedParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "wall.set_join_allowed",
        version: 1,
        summary: "Allow or disallow a wall end's join",
        description: "Sets the Join Status of one end of a wall. A disallowed end stops square at its location point.",
        effects: &[Effect::Edit],
        transaction_name: "Allow/Disallow Join",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetJoinAllowedParams) -> Result<()> {
        walls::set_join_allowed(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct WallJoinsQuery;

impl Command for WallJoinsQuery {
    type Params = JoinsQueryParams;
    type Output = Vec<walls::Join>;
    const META: CommandMeta = CommandMeta {
        name: "wall.joins_query",
        version: 1,
        summary: "List a wall's joins",
        description: "Returns the joins of a wall: L joins name the other wall's end, T joins have `other_end` null.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: JoinsQueryParams) -> Result<Vec<walls::Join>> {
        walls::joins_query(cx.store(), p)
    }
}

#[derive(Debug)]
struct WallFlip;

impl Command for WallFlip {
    type Params = FlipParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "wall.flip",
        version: 1,
        summary: "Flip walls",
        description: "Moves the exterior of each wall to the other side of its location line.",
        effects: &[Effect::Edit],
        transaction_name: "Flip Wall",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: FlipParams) -> Result<()> {
        walls::flip(cx.edit()?, p)
    }
}

use crate::views::{
    self, Create3dParams, DuplicateParams, RenameParams, SetCameraParams, ViewIdsParams,
    ViewSummary,
};

#[derive(Debug)]
struct ViewCreate3d;

impl Command for ViewCreate3d {
    type Params = Create3dParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "view.create_3d",
        version: 1,
        summary: "Create a 3D view",
        description: "`kind: default` returns the default 3D view `{3D}`, creating it if needed; `kind: named` \
                      creates a new 3D view (`name`, or `{3D 2}`, `{3D 3}`, …).",
        effects: &[Effect::Edit],
        transaction_name: "3D View",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: Create3dParams) -> Result<ElementId> {
        views::create_3d(cx.edit()?, p)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewListParams {
    /// Only views of this kind (for example `view.3d`).
    pub kind: Option<String>,
}

#[derive(Debug)]
struct ViewList;

impl Command for ViewList {
    type Params = ViewListParams;
    type Output = Vec<ViewSummary>;
    const META: CommandMeta = CommandMeta {
        name: "view.list",
        version: 1,
        summary: "List views",
        description: "Every view of the model, by kind and name.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ViewListParams) -> Result<Vec<ViewSummary>> {
        Ok(views::list(cx.store())
            .into_iter()
            .filter(|v| p.kind.as_ref().is_none_or(|k| *k == v.kind))
            .collect())
    }
}

#[derive(Debug)]
struct ViewRename;

impl Command for ViewRename {
    type Params = RenameParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.rename",
        version: 1,
        summary: "Rename a view",
        description: "View names are unique within a view kind; a duplicate name is refused.",
        effects: &[Effect::Edit],
        transaction_name: "Rename View",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: RenameParams) -> Result<()> {
        views::rename(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewDuplicate;

impl Command for ViewDuplicate {
    type Params = DuplicateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "view.duplicate",
        version: 1,
        summary: "Duplicate a view",
        description: "Copies a view with its settings and saved camera, as `<name> Copy` unless named.",
        effects: &[Effect::Edit],
        transaction_name: "Duplicate View",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: DuplicateParams) -> Result<ElementId> {
        views::duplicate(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewDelete;

impl Command for ViewDelete {
    type Params = ViewIdsParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.delete",
        version: 1,
        summary: "Delete views",
        description: "Deletes views (not model elements).",
        effects: &[Effect::Edit, Effect::Destructive],
        transaction_name: "Delete View",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ViewIdsParams) -> Result<()> {
        views::delete(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewSetCamera;

impl Command for ViewSetCamera {
    type Params = SetCameraParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.set_camera",
        version: 1,
        summary: "Set a view's saved camera",
        description: "Stores the view's camera (eye and target in metres, projection, horizontal field of \
                      view). Unlike navigation, this is an undoable edit that marks the model modified.",
        effects: &[Effect::Edit],
        transaction_name: "View Camera",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetCameraParams) -> Result<()> {
        views::set_camera(cx.edit()?, p)
    }
}

use crate::plans::{self, CreatePlanParams, SetViewRangeParams, ViewRangeReport};

#[derive(Debug)]
struct ViewCreatePlan;

impl Command for ViewCreatePlan {
    type Params = CreatePlanParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "view.create_plan",
        version: 1,
        summary: "Create plan views",
        description: "Creates one floor plan per level, named after the level, with the default view range \
                      (cut 1200 mm above the level). Levels that already have a plan are skipped unless \
                      `allow_duplicates`.",
        effects: &[Effect::Edit],
        transaction_name: "New Plan",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: CreatePlanParams) -> Result<Vec<ElementId>> {
        plans::create_plan(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewSetViewRange;

impl Command for ViewSetViewRange {
    type Params = SetViewRangeParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.set_view_range",
        version: 1,
        summary: "Set a plan's view range",
        description: "Each plane is `{level_ref, offset}`; `level_ref` is `associated`, `above`, `below`, \
                      `unlimited` (top and view depth) or a level. Top ≥ Cut ≥ Bottom ≥ View Depth.",
        effects: &[Effect::Edit],
        transaction_name: "View Range",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetViewRangeParams) -> Result<()> {
        plans::set_view_range(cx.edit()?, p)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetViewRangeParams {
    pub view_id: ElementRef,
}

#[derive(Debug)]
struct ViewGetViewRange;

impl Command for ViewGetViewRange {
    type Params = GetViewRangeParams;
    type Output = ViewRangeReport;
    const META: CommandMeta = CommandMeta {
        name: "view.get_view_range",
        version: 1,
        summary: "A plan's view range",
        description: "The four planes and their resolved elevations in metres (null for unlimited).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: GetViewRangeParams) -> Result<ViewRangeReport> {
        plans::get_view_range(cx.store(), &p.view_id)
    }
}

use crate::annotations::{self, CreateAlignedParams, DimensionInfo, ListParams};

#[derive(Debug)]
struct DimCreateAligned;

impl Command for DimCreateAligned {
    type Params = CreateAlignedParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "dim.create_aligned",
        version: 1,
        summary: "Create an aligned dimension",
        description: "Dimensions two or more parallel references in a view: Reviera wall faces (`face:ext`, \
                      `face:int`) and location lines (`axis`), which follow the walls, or fixed `line`s. \
                      `line_point` places the dimension line.",
        effects: &[Effect::Edit],
        transaction_name: "Dimension",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: CreateAlignedParams) -> Result<ElementId> {
        annotations::create_aligned(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct AnnoList;

impl Command for AnnoList {
    type Params = ListParams;
    type Output = Vec<DimensionInfo>;
    const META: CommandMeta = CommandMeta {
        name: "anno.list",
        version: 1,
        summary: "List a view's annotation",
        description: "The dimensions of a view, with their references and current values (metres).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: ListParams) -> Result<Vec<DimensionInfo>> {
        annotations::list(cx.store(), p)
    }
}

use crate::standards::{self, ScaleParams, SetLineWeightParams, SetObjectStyleParams};

macro_rules! standards_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, $params:ty, $f:path) => {
        #[derive(Debug)]
        struct $ty;

        impl Command for $ty {
            type Params = $params;
            type Output = ();
            const META: CommandMeta = CommandMeta {
                name: $name,
                version: 1,
                summary: $summary,
                description: $desc,
                effects: &[Effect::Edit],
                transaction_name: $txn,
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<()> {
                $f(cx.edit()?, p)
            }
        }
    };
}

standards_command!(
    LineWeightSet,
    "lineweight.set",
    "Set a pen width",
    "Sets the paper width (mm) of a pen (1–16) in the model table (per scale column), the perspective \
     table or the annotation table.",
    "Line Weights",
    SetLineWeightParams,
    standards::set_line_weight
);
standards_command!(
    LineWeightAddScale,
    "lineweight.add_scale",
    "Add a scale column to the model pens",
    "Adds a view scale column to the model pen table, starting as a copy of the nearest column.",
    "Line Weights",
    ScaleParams,
    standards::add_scale
);
standards_command!(
    LineWeightRemoveScale,
    "lineweight.remove_scale",
    "Remove a scale column from the model pens",
    "Removes a view scale column; views at that scale use the nearest remaining column.",
    "Line Weights",
    ScaleParams,
    standards::remove_scale
);
standards_command!(
    ObjectStyleSet,
    "objectstyle.set",
    "Set a category's line weights",
    "Sets the projection and cut pens (1–16) of a category, for example `cat.walls`.",
    "Object Styles",
    SetObjectStyleParams,
    standards::set_object_style
);
