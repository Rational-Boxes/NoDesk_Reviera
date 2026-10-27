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
        registry.add(WallSetConstraints),
        registry.add(WallSetJoinAllowed),
        registry.add(WallFlip),
        registry.add(ViewCreate3d),
        registry.add(ViewList),
        registry.add(ViewGet),
        registry.add(ViewAnnotations3d),
        registry.add(ViewSet3dAnnotations),
        registry.add(ViewApplyViewpoint),
        registry.add(BcfPutTopic),
        registry.add(BcfUpdateTopic),
        registry.add(BcfAddComment),
        registry.add(BcfPutViewpoint),
        registry.add(BcfDeleteTopic),
        registry.add(BcfList),
        registry.add(BcfGet),
        registry.add(BcfMergeTopics),
        registry.add(BcfLists),
        registry.add(BcfSnapshot),
        registry.add(ViewRename),
        registry.add(ViewDuplicate),
        registry.add(ViewDelete),
        registry.add(ViewSetCamera),
        registry.add(ViewSetSectionBox),
        registry.add(ViewSetSlicePlanes),
        registry.add(RoofCreate),
        registry.add(RoofEditFootprint),
        registry.add(RoofSetParams),
        registry.add(RoofSetEdge),
        registry.add(RoofSetEaves),
        registry.add(ClearanceSetRuleSet),
        registry.add(CodeGet),
        registry.add(CodeSet),
        registry.add(OpeningCreateVertical),
        registry.add(OpeningCreateByFace),
        registry.add(OpeningSetLoops),
        registry.add(StairHeadroomCheck),
        registry.add(StairCreateOpenings),
        registry.add(ViewSetVirtualVolumes),
        registry.add(ClearanceList),
        registry.add(OverlapAccept),
        registry.add(ClashRecordRun),
        registry.add(ClashSetStatus),
        registry.add(GeometryBoolean),
        registry.add(GeometryRemoveBoolean),
        registry.add(OverlapAccepted),
        registry.add(RoofCreateArched),
        registry.add(RoofSetArch),
        registry.add(RoofCreateDome),
        registry.add(RoofSetDome),
        registry.add(RoofEaves),
        registry.add(RoofSetSlope),
        registry.add(RoofSetShape),
        registry.add(RoofAttachWalls),
        registry.add(DormerCreate),
        registry.add(DormerSet),
        registry.add(DormerMove),
        registry.add(SketchMoveCurve),
        registry.add(SketchMoveCorner),
        registry.add(StairCreate),
        registry.add(RailingCreate),
        registry.add(DeckCreate),
        registry.add(DeckSetBoards),
        registry.add(DeckSetFraming),
        registry.add(DeckBoards),
        registry.add(DeckCutList),
        registry.add(RailingCreateOnEdges),
        registry.add(RailingCreateOnStair),
        registry.add(RailingAddOpening),
        registry.add(RailingAddGate),
        registry.add(RailingMoveFeature),
        registry.add(RailingRemoveFeature),
        registry.add(RailingFeatures),
        registry.add(StairSetParams),
        registry.add(WallAttach),
        registry.add(ScheduleCreate),
        registry.add(CobieSetSettings),
        registry.add(ScheduleSetFields),
        registry.add(ScheduleSetSort),
        registry.add(ScheduleSetFilters),
        registry.add(ScheduleSetCell),
        registry.add(ScheduleFields),
        registry.add(SheetCreate),
        registry.add(SheetSet),
        registry.add(SheetDelete),
        registry.add(SheetPlaceView),
        registry.add(SheetPlaceSchedule),
        registry.add(ViewportMove),
        registry.add(ViewportRemove),
        registry.add(ViewCreateSection),
        registry.add(ViewCreateElevation),
        registry.add(SectionEdit),
        registry.add(RoomPlace),
        registry.add(RoomPlaceAutomatically),
        registry.add(RoomSet),
        registry.add(WallDetach),
        registry.add(ViewCreatePlan),
        registry.add(ViewSetViewRange),
        registry.add(ViewGetViewRange),
        registry.add(DimCreateAligned),
        registry.add(AnnoList),
        registry.add(TagCreate),
        registry.add(TagCreateRoom),
        registry.add(TagTagAll),
        registry.add(TagSetLeader),
        registry.add(TagSetType),
        registry.add(TagSetDefaultType),
        registry.add(TagList),
        registry.add(TextCreate),
        registry.add(SpotCreateElevation),
        registry.add(SpotCreateCoordinate),
        registry.add(SpotSet),
        registry.add(SpotPutSlope),
        registry.add(TextSetContent),
        registry.add(TextSet),
        registry.add(TextAddLeader),
        registry.add(TextRemoveLeaders),
        registry.add(TextList),
        registry.add(LineWeightSet),
        registry.add(LineWeightAddScale),
        registry.add(LineWeightRemoveScale),
        registry.add(ObjectStyleSet),
        registry.add(WallJoinsQuery),
        registry.add(LevelCreate),
        registry.add(LevelSetElevation),
        registry.add(LevelRename),
        registry.add(LevelList),
        registry.add(GridCreate),
        registry.add(GridRename),
        registry.add(GridSetCurve),
        registry.add(GridList),
        registry.add(TypeList),
        registry.add(TypeDuplicate),
        registry.add(TypeRename),
        registry.add(TypeSetParam),
        registry.add(ElementSetType),
        registry.add(DoorPlace),
        registry.add(WindowPlace),
        registry.add(InsertFlipHand),
        registry.add(InsertFlipFacing),
        registry.add(InsertSetPosition),
        registry.add(InsertSetSill),
        registry.add(InsertSetSize),
        registry.add(ElementMove),
        registry.add(ElementCopy),
        registry.add(ElementRotate),
        registry.add(ElementMirror),
        registry.add(ElementOffset),
        registry.add(ElementSplit),
        registry.add(ElementSplitWithGap),
        registry.add(WallSplitHorizontal),
        registry.add(ElementTrimCorner),
        registry.add(ElementTrimExtend),
        registry.add(ElementAlign),
        registry.add(ElementPin),
        registry.add(ElementUnpin),
        registry.add(FloorCreate),
        registry.add(FloorEditBoundary),
        registry.add(FloorSetLevel),
        registry.add(CeilingCreate),
        registry.add(CeilingCreateAuto),
        registry.add(CeilingEditBoundary),
    ];
    for r in results {
        r.expect("core commands are valid");
    }
    crate::builtin_arc::register(registry);
    crate::builtin_mep::register(registry);
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

/// Elements that depend on `id`: instances of a type, walls based on or bounded by a level, the
/// plan views of a level, the annotation of a view, the doors and windows of a wall and the
/// opening of a door or window.
pub fn dependents_of(store: &crate::store::Store, id: ElementId) -> Vec<ElementId> {
    let Some(guid) = store.get(id).map(|e| e.guid().as_str().to_owned()) else {
        return Vec::new();
    };
    store
        .iter()
        .filter(|e| {
            e.class
                .strip_prefix(crate::annotations::CLASS_PREFIX)
                .is_some_and(|rest| rest.starts_with(&format!("{guid}/")))
                || e.param("@type").and_then(serde_json::Value::as_str) == Some(guid.as_str())
                || crate::inserts::insert_recipe(e).is_some_and(|r| r.host == guid)
                || crate::mep_coverings::covering_recipe(e).is_some_and(|r| r.host == guid)
                || crate::floors::slab_recipe(e).is_some_and(|(_, r)| r.level == guid)
                || crate::system_zones::recipe(e).is_some_and(|r| r.level == guid)
                || crate::rooms::room_recipe(e).or_else(|| crate::mep_spaces::space_recipe(e)).is_some_and(|r| {
                    r.level == guid || r.upper_level.as_deref() == Some(guid.as_str())
                })
                || crate::railings::railing_recipe(e).is_some_and(|r| match r.path {
                    crate::railings::RailPath::Free { level, .. } => level == guid,
                    crate::railings::RailPath::Host { host, .. } => host == guid,
                    crate::railings::RailPath::Stair { stair, .. } => stair == guid,
                })
                || crate::railings::gate_of(e).is_some_and(|(r, _)| r == guid)
                || crate::dormers::dormer_of(e) == Some(guid.as_str())
                || crate::dormers::dormer_recipe(e).is_some_and(|r| r.roof == guid)
                || crate::stairs::stair_recipe(e).is_some_and(|r| {
                    r.base_level == guid || r.top == crate::stairs::StairTop::Level(guid.clone())
                })
                || crate::roofs::roof_recipe(e).is_some_and(|r| {
                    r.level == guid
                        || r.cutoff.as_ref().is_some_and(|c| c.level == guid)
                        || r.extrusion.as_ref().is_some_and(|x| {
                            crate::work_planes::host_of(&x.plane) == Some(guid.as_str())
                        })
                })
                || crate::inserts::opening_of(e).is_some_and(|i| i == guid)
                || crate::slab_openings::opening_recipe(e).is_some_and(|r| r.host == guid)
                || crate::skylights::skylight_recipe(e).is_some_and(|r| r.roof == guid)
                || crate::ramps::ramp_recipe(e).is_some_and(|r| r.extent.uses_level(&guid))
                || crate::curtain::curtain_recipe(e).is_some_and(|r| r.extent.uses_level(&guid))
                || crate::columns::column_recipe(e).is_some_and(|r| r.extent.uses_level(&guid))
                || crate::beams::beam_recipe(e)
                    .is_some_and(|r| r.level == guid || r.system.is_some_and(|s| s.system == guid))
                || crate::beams::system_recipe(e).is_some_and(|r| r.level == guid)
                || crate::beams::truss_recipe(e).is_some_and(|r| r.level == guid)
                || crate::foundations::uses(e, &guid)
                || crate::connections::uses(e, &guid)
                || crate::steel_fab::uses(e, &guid)
                || crate::steel_detail::uses(e, &guid)
                || crate::steel_cuts::uses(e, &guid)
                || crate::custom_connections::uses(e, &guid)
                || crate::precast::uses(e, &guid)
                || crate::precast_rebar::uses(e, &guid)
                || crate::precast_fabric::uses(e, &guid)
                || crate::precast_parts::uses(e, &guid)
                || crate::rebar::uses(e, &guid)
                || crate::rebar_systems::uses(e, &guid)
                || crate::rebar_docs::uses(store, e, &guid)
                || crate::rebar_edit::uses(e, &guid)
                || crate::analytical::uses(e, &guid)
                || crate::analytical_panels::uses(e, &guid)
                || crate::loads::uses(e, &guid)
                || crate::mesh_objects::mesh_object_recipe(e).is_some_and(|r| r.level == guid)
                || crate::model_lines::recipe(e)
                    .is_some_and(|r| crate::model_lines::uses(&r, &guid))
                || crate::slab_edges::recipe(e).is_some_and(|r| crate::slab_edges::uses(&r, &guid))
                || crate::groups::instance_of(e).as_deref() == Some(guid.as_str())
                || crate::in_place::void_recipe(e).is_some_and(|v| v.of == guid)
                || crate::room_separators::recipe(e).is_some_and(|r| r.level == guid)
                || crate::toposolids::recipe(e)
                    .is_some_and(|r| r.parent.as_deref() == Some(guid.as_str()))
                || crate::toposolids::cut_recipe(e).is_some_and(|c| c.of == guid)
                || crate::parts::depends_on(store, e, &guid)
                || crate::stair_marks::numbers_recipe(e)
                    .is_some_and(|n| n.stair == guid || n.view == guid)
                || crate::wall_sweeps::recipe(e).is_some_and(|r| {
                    r.level == guid || (r.walls.len() == 1 && crate::wall_sweeps::uses(&r, &guid))
                })
                || crate::constraints::data(e).is_some_and(|c| crate::constraints::uses(&c, &guid))
                || crate::path_arrays::path_array_recipe(e)
                    .is_some_and(|r| crate::path_arrays::uses(&r, &guid))
                || crate::slab_openings::shaft_recipe(e).is_some_and(|r| r.extent.uses_level(&guid))
                || crate::wall_openings::wall_opening_recipe(e)
                    .is_some_and(|r| r.host == guid || r.extent.uses_level(&guid))
                || crate::grids::grid_data(e).is_some_and(|g| {
                    g.system == guid || g.type_guid.as_deref() == Some(guid.as_str())
                })
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

/// Everything `element.delete` removes with `ids`: the elements and, transitively, their
/// dependents, and grid systems whose last line goes.
pub fn delete_closure(
    store: &crate::store::Store,
    ids: &[ElementId],
) -> std::collections::BTreeSet<ElementId> {
    let mut queue = ids.to_vec();
    let mut deleted = std::collections::BTreeSet::new();
    loop {
        while let Some(id) = queue.pop() {
            if deleted.insert(id) {
                queue.extend(dependents_of(store, id));
            }
        }
        // A grid system goes with its last line; analytical pipe connections with their
        // equipment or pipe (MEP-PIPE-009).
        queue = crate::grids::emptied_systems(store, &deleted);
        queue.extend(crate::mep_analytical::orphaned(store, &deleted));
        if queue.is_empty() {
            return deleted;
        }
    }
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
            crate::modify::check_unpinned(txn.store(), id)?;
            if id.0 < ElementId::FIRST.0 {
                return Err(Error::new(
                    "element.protected",
                    format!("element {id} is part of the project and cannot be deleted"),
                ));
            }
            queue.push(id);
        }
        let explicit: std::collections::BTreeSet<ElementId> = queue.iter().copied().collect();
        if !p.confirm_dependents {
            for &id in &explicit {
                if let Some(d) = dependents_of(txn.store(), id)
                    .into_iter()
                    .find(|d| !explicit.contains(d))
                {
                    return Err(Error::new(
                        "element.has_dependents",
                        format!(
                            "element {d} depends on element {id}; pass confirm_dependents to delete it too"
                        ),
                    ));
                }
            }
        }
        let deleted = delete_closure(txn.store(), &queue);
        // Braces attached to what goes keep their ends where they are (STR-BRACE-001).
        crate::beams::release_attachments(txn, &deleted)?;
        crate::foundations::release(txn, &deleted)?;
        crate::steel_detail::release(txn, &deleted)?;
        // Control devices on a wall that goes stay where they are (MEP-DUCT-012).
        crate::mep_controls::release(txn, &deleted)?;
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
        let usage = match (p.structural, p.structural_usage) {
            (_, Some(u)) => Some(u),
            (true, None) => Some(crate::structural::WallUsage::Bearing),
            (false, None) => None,
        };
        let txn = cx.edit()?;
        let id = walls::create(txn, p)?;
        if let Some(u) = usage {
            crate::structural::apply_wall_usage(txn, id, u)?;
        }
        Ok(id)
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
struct WallSetConstraints;

impl Command for WallSetConstraints {
    type Params = walls::SetConstraintsParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "wall.set_constraints",
        version: 1,
        summary: "Change a wall's base and top",
        description: "Sets a wall's base level and offset, and its top: up to a level plus a top offset, or an unconnected height. Omitted fields keep their values; the top must stay above the base.",
        effects: &[Effect::Edit],
        transaction_name: "Wall Constraints",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: walls::SetConstraintsParams) -> Result<()> {
        walls::set_constraints(cx.edit()?, p)
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
struct ViewGet;

impl Command for ViewGet {
    type Params = views::ViewIdParams;
    type Output = views::ViewDetail;
    const META: CommandMeta = CommandMeta {
        name: "view.get",
        version: 1,
        summary: "Get a view's settings",
        description: "A view in full: its kind, name and GlobalId, camera and Home (3D), level and view range \
                      (plans), section box and slice planes (3D, DOC-VIEW-008, DOC-VIEW-014), section line and \
                      depth (sections and elevations), colour and area schemes, detail level, the scope box it \
                      is assigned to, how it shows virtual volumes and whether it shows parts. Temporary \
                      hide and isolate are not saved, so they are not here (`view.get_temp_state`).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: views::ViewIdParams) -> Result<views::ViewDetail> {
        views::detail(cx.store(), &p.view_id)
    }
}

#[derive(Debug)]
struct BcfPutTopic;

impl Command for BcfPutTopic {
    type Params = crate::issues::PutParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "bcf.put_topic",
        version: 1,
        summary: "Add or replace a BCF topic",
        description: "Stores a whole BCF topic (COL-BCF-001) with the model, new or replacing the one with its GUID: title, description, topic_type, status, priority, labels, assigned_to, due_date, stage, authors and dates, related_topics, reference_links, comments and viewpoints. A missing GUID, creation date or author is filled in. Returns its element id. `bcf.create_topic` makes one from the active view.",
        effects: &[Effect::Edit],
        transaction_name: "Issue",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::PutParams) -> Result<ElementId> {
        crate::issues::put(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfUpdateTopic;

impl Command for BcfUpdateTopic {
    type Params = crate::issues::UpdateParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "bcf.update_topic",
        version: 1,
        summary: "Change a BCF topic",
        description: "Sets a topic's fields (title, description, topic_type, status, priority, labels, assigned_to, due_date, stage, related_topics, reference_links; an empty text clears one) by GUID or element id, recording the modification date and author.",
        effects: &[Effect::Edit],
        transaction_name: "Edit Issue",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::UpdateParams) -> Result<()> {
        crate::issues::update(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfAddComment;

impl Command for BcfAddComment {
    type Params = crate::issues::CommentParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "bcf.add_comment",
        version: 1,
        summary: "Comment on a BCF topic",
        description: "Adds a comment to a topic, optionally referring to one of its viewpoints; returns the comment's GUID.",
        effects: &[Effect::Edit],
        transaction_name: "Comment",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::CommentParams) -> Result<String> {
        crate::issues::add_comment(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfPutViewpoint;

impl Command for BcfPutViewpoint {
    type Params = crate::issues::AddViewpointParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "bcf.put_viewpoint",
        version: 1,
        summary: "Add a viewpoint to a BCF topic",
        description: "Adds a viewpoint (camera, clipping planes, components, as `view.apply_viewpoint` takes) to a topic, replacing one with the same GUID; returns its GUID. `bcf.add_viewpoint` captures one from a view.",
        effects: &[Effect::Edit],
        transaction_name: "Add Viewpoint",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::AddViewpointParams) -> Result<String> {
        crate::issues::add_viewpoint(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfDeleteTopic;

impl Command for BcfDeleteTopic {
    type Params = crate::issues::DeleteParams;
    type Output = usize;
    const META: CommandMeta = CommandMeta {
        name: "bcf.delete_topic",
        version: 1,
        summary: "Delete BCF topics",
        description: "Deletes topics by GUID or element id; returns how many.",
        effects: &[Effect::Edit],
        transaction_name: "Delete Issues",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::DeleteParams) -> Result<usize> {
        crate::issues::delete(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfList;

impl Command for BcfList {
    type Params = crate::issues::ListParams;
    type Output = Vec<crate::issues::Summary>;
    const META: CommandMeta = CommandMeta {
        name: "bcf.list",
        version: 1,
        summary: "List BCF topics",
        description: "The model's topics by creation date: title, type, status, priority, labels, assignee, due date, stage, last change and counts of comments and viewpoints, filtered by status, type, priority, assignee, label or text.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::ListParams) -> Result<Vec<crate::issues::Summary>> {
        Ok(crate::issues::list(cx.store(), &p))
    }
}

#[derive(Debug)]
struct BcfGet;

impl Command for BcfGet {
    type Params = BcfGetParams;
    type Output = Value;
    const META: CommandMeta = CommandMeta {
        name: "bcf.get",
        version: 1,
        summary: "Get a BCF topic",
        description: "A topic in full: its fields, comments and viewpoints, and its element id.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: BcfGetParams) -> Result<Value> {
        crate::issues::get(cx.store(), &p.topic_id)
    }
}

#[derive(Debug)]
struct BcfMergeTopics;

impl Command for BcfMergeTopics {
    type Params = crate::issues::MergeParams;
    type Output = crate::issues::MergeReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.merge_topics",
        version: 1,
        summary: "Merge BCF topics",
        description: "Adds topics, or merges them into those with their GUIDs (the fields of whichever changed last; comments and viewpoints joined by GUID, a comment taken when changed later), as one step; `bcf.import` reads them from a .bcf file.",
        effects: &[Effect::Edit],
        transaction_name: "Import Issues",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::issues::MergeParams) -> Result<crate::issues::MergeReport> {
        crate::issues::merge_all(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct BcfLists;

impl Command for BcfLists {
    type Params = NoParams;
    type Output = crate::issues::Lists;
    const META: CommandMeta = CommandMeta {
        name: "bcf.lists",
        version: 1,
        summary: "BCF lists",
        description: "The topic types, statuses, priorities, labels, users and stages the project offers: Reviera's defaults and every value in use.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<crate::issues::Lists> {
        Ok(crate::issues::lists(cx.store()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BcfSnapshotParams {
    /// The viewpoint's GUID.
    pub viewpoint_id: String,
}

#[derive(Debug)]
struct BcfSnapshot;

impl Command for BcfSnapshot {
    type Params = BcfSnapshotParams;
    type Output = Value;
    const META: CommandMeta = CommandMeta {
        name: "bcf.snapshot",
        version: 1,
        summary: "A BCF viewpoint's snapshot",
        description: "The snapshot kept for a topic's viewpoint (by its GUID): `{ \"png_base64\": … }` (PNG, or JPEG from another tool), or an error when it has none.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: BcfSnapshotParams) -> Result<Value> {
        use base64::Engine;
        let bytes = crate::issues::snapshot(cx.store(), &p.viewpoint_id).ok_or_else(|| Error::new("bcf.no_snapshot", format!("viewpoint {} has no snapshot", p.viewpoint_id)))?;
        Ok(serde_json::json!({ "png_base64": base64::engine::general_purpose::STANDARD.encode(bytes) }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BcfGetParams {
    pub topic_id: crate::issues::TopicRef,
}

#[derive(Debug)]
struct ViewAnnotations3d;

impl Command for ViewAnnotations3d {
    type Params = crate::annotations_3d::InfoParams;
    type Output = crate::annotations_3d::Info;
    const META: CommandMeta = CommandMeta {
        name: "view.annotations_3d",
        version: 1,
        summary: "A 3D view's annotation settings",
        description: "Whether a 3D view shows the plans' annotation as floating labels (DOC-ANN-025), and each class \
                      of it (text notes, dimensions, tags by the category they tag) with its count in the model and \
                      whether the view shows it.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::annotations_3d::InfoParams) -> Result<crate::annotations_3d::Info> {
        crate::annotations_3d::info(cx.store(), p)
    }
}

#[derive(Debug)]
struct ViewSet3dAnnotations;

impl Command for ViewSet3dAnnotations {
    type Params = crate::annotations_3d::SetParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.set_3d_annotations",
        version: 1,
        summary: "Show or hide annotation in a 3D view",
        description: "Turns a 3D view's floating annotation labels on or off (`shown`) and shows or hides each class \
                      independently (`classes`: `text`, `dimensions`, `tags:Doors`, `tags:Windows`, `tags:Rooms`, … \
                      to true or false), kept with the view (DOC-ANN-025).",
        effects: &[Effect::Edit],
        transaction_name: "Annotation in 3D",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::annotations_3d::SetParams) -> Result<()> {
        crate::annotations_3d::set(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewApplyViewpoint;

impl Command for ViewApplyViewpoint {
    type Params = crate::bcf::ApplyParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "view.apply_viewpoint",
        version: 1,
        summary: "Set a 3D view's camera and cut from a BCF viewpoint",
        description: "Sets a 3D view's camera, section box and slice planes from a BCF viewpoint (COL-BCF-003): \
                      `camera` (perspective: eye, direction, up, vertical field of view in degrees, aspect \
                      ratio; orthogonal: view_to_world_scale, the view's height in metres) and \
                      `clipping_planes` (each a location and a direction pointing to the hidden side): six \
                      that form a box become its section box, the rest slice planes (at most eight). The \
                      viewpoint's components are not applied here: `bcf.import_viewpoint` reads a .bcfv \
                      file and hides, isolates and selects them too.",
        effects: &[Effect::Edit],
        transaction_name: "Apply Viewpoint",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: crate::bcf::ApplyParams) -> Result<ElementId> {
        crate::bcf::apply(cx.edit()?, &p.view_id, &p.viewpoint)
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
        description: "View names are unique within a view kind; a duplicate name is refused. Schedules and sheets are renamed \
                      the same way.",
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
        description: "Deletes views, schedules and sheets (not model elements). The viewports of a deleted view go with it.",
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

#[derive(Debug)]
struct ViewSetSectionBox;

impl Command for ViewSetSectionBox {
    type Params = views::SetSectionBoxParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.set_section_box",
        version: 1,
        summary: "Turn a 3D view's section box on or off, or change its extents",
        description: "Sets the section box of a 3D view (DOC-VIEW-008): enabled, its corners min and max \
                      in the box's coordinates (metres) and an optional transform (x, y, z axes, origin). \
                      Omitted values are kept; turning the box off keeps its extents.",
        effects: &[Effect::Edit],
        transaction_name: "Section Box",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: views::SetSectionBoxParams) -> Result<()> {
        views::set_section_box(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ViewSetSlicePlanes;

impl Command for ViewSetSlicePlanes {
    type Params = views::SetSlicePlanesParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "view.set_slice_planes",
        version: 1,
        summary: "Set the slice planes of a 3D view",
        description: "Replaces the slice planes of a 3D view (DOC-VIEW-014), at most eight. Each plane has \
                      an origin and a normal pointing to the side that is kept (metres), and may be \
                      disabled. An empty list removes them.",
        effects: &[Effect::Edit],
        transaction_name: "Slice Planes",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: views::SetSlicePlanesParams) -> Result<()> {
        views::set_slice_planes(cx.edit()?, p)
    }
}

// ---- Levels (Ch 3 ARC-LVL-001) ----

use crate::levels::{self, LevelInfo};

#[derive(Debug)]
struct LevelCreate;

impl Command for LevelCreate {
    type Params = levels::CreateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "level.create",
        version: 1,
        summary: "Create a level",
        description: "Creates a story level at `elevation` (accepts units, \"3 m\"). Without a name it continues \
                      the series of the level below (Level 2 → Level 3). With `make_plan_view` (default) \
                      a floor plan named after the level is created too.",
        effects: &[Effect::Edit],
        transaction_name: "Level",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: levels::CreateParams) -> Result<ElementId> {
        levels::create(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct LevelSetElevation;

impl Command for LevelSetElevation {
    type Params = levels::SetElevationParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "level.set_elevation",
        version: 1,
        summary: "Change a level's elevation",
        description: "Moves the level. Walls based on it move with it and walls whose top is the level \
                      change height, keeping their offsets; refused if a wall's top would reach its base.",
        effects: &[Effect::Edit],
        transaction_name: "Level Elevation",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: levels::SetElevationParams) -> Result<()> {
        levels::set_elevation(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct LevelRename;

impl Command for LevelRename {
    type Params = levels::RenameParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "level.rename",
        version: 1,
        summary: "Rename a level",
        description: "Renames a level (names are unique among levels). With `rename_views` (default) its \
                      plan views named after it are renamed too; returns those views.",
        effects: &[Effect::Edit],
        transaction_name: "Rename Level",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: levels::RenameParams) -> Result<Vec<ElementId>> {
        levels::rename(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct LevelList;

impl Command for LevelList {
    type Params = NoParams;
    type Output = Vec<LevelInfo>;
    const META: CommandMeta = CommandMeta {
        name: "level.list",
        version: 1,
        summary: "List levels",
        description: "Lists the model's levels, lowest first, with elevations in metres.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<Vec<LevelInfo>> {
        Ok(levels::list(cx.store()))
    }
}

// ---- Grids (Ch 3 ARC-GRID-001) ----

use crate::grids::{self, GridInfo};

#[derive(Debug)]
struct GridCreate;

impl Command for GridCreate {
    type Params = grids::CreateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "grid.create",
        version: 1,
        summary: "Create a grid",
        description: "Creates a straight grid line along `curve.line` (plan, metres). Without a name it \
                      continues the series of the last grid (1 → 2, A → B). Names are unique among grids.",
        effects: &[Effect::Edit],
        transaction_name: "Grid",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: grids::CreateParams) -> Result<ElementId> {
        grids::create(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct GridRename;

impl Command for GridRename {
    type Params = grids::RenameParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "grid.rename",
        version: 1,
        summary: "Rename a grid",
        description: "Renames a grid line (its IFC axis tag); the grid keeps its GlobalId and dimensions.",
        effects: &[Effect::Edit],
        transaction_name: "Rename Grid",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: grids::RenameParams) -> Result<()> {
        grids::rename(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct GridSetCurve;

impl Command for GridSetCurve {
    type Params = grids::SetCurveParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "grid.set_curve",
        version: 1,
        summary: "Move a grid",
        description: "Replaces the grid's line; dimensions to it follow.",
        effects: &[Effect::Edit],
        transaction_name: "Move Grid",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: grids::SetCurveParams) -> Result<()> {
        grids::set_curve(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct GridList;

impl Command for GridList {
    type Params = NoParams;
    type Output = Vec<GridInfo>;
    const META: CommandMeta = CommandMeta {
        name: "grid.list",
        version: 1,
        summary: "List grids",
        description: "Lists the grid lines in creation order, with their lines in plan (metres).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<Vec<GridInfo>> {
        Ok(grids::list(cx.store()))
    }
}

// ---- Types (Ch 2 DM-TYPE-001, Ch 3 ARC-TOOL-002) ----

use crate::types::{self, TypeInfo};

#[derive(Debug)]
struct TypeList;

impl Command for TypeList {
    type Params = types::ListParams;
    type Output = Vec<TypeInfo>;
    const META: CommandMeta = CommandMeta {
        name: "type.list",
        version: 1,
        summary: "List types",
        description: "Lists the model's types with how many elements use each, optionally only one class \
                      or Reviera recipe kind (`recipe.grid_type`, `recipe.wall_type`).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: types::ListParams) -> Result<Vec<TypeInfo>> {
        Ok(types::list(cx.store(), p))
    }
}

#[derive(Debug)]
struct TypeDuplicate;

impl Command for TypeDuplicate {
    type Params = types::DuplicateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "type.duplicate",
        version: 1,
        summary: "Duplicate a type",
        description: "Copies a type with every value under a new name (unique in its family; default \
                      `<name> 2`).",
        effects: &[Effect::Edit],
        transaction_name: "Duplicate Type",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: types::DuplicateParams) -> Result<ElementId> {
        types::duplicate(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct TypeRename;

impl Command for TypeRename {
    type Params = types::RenameParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "type.rename",
        version: 1,
        summary: "Rename a type",
        description: "Renames a type; names are unique within its family.",
        effects: &[Effect::Edit],
        transaction_name: "Rename Type",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: types::RenameParams) -> Result<()> {
        types::rename(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct TypeSetParam;

impl Command for TypeSetParam {
    type Params = types::SetParamParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "type.set_param",
        version: 1,
        summary: "Set a type parameter",
        description: "Sets a value of a type; all its instances change. Grid types: symbol (bubble, none), \
                      center_segment (continuous, none, custom), center_pen / end_pen (1-16), \
                      center_colour / end_colour (#RRGGBB), center_pattern / end_pattern (solid, dash, \
                      center, dash_dot, dot), end_length (paper mm), bubble_start, bubble_end. Wall \
                      types: width. Other parameters as param.set.",
        effects: &[Effect::Edit],
        transaction_name: "Edit Type",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: types::SetParamParams) -> Result<()> {
        types::set_param(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct ElementSetType;

impl Command for ElementSetType {
    type Params = types::SetTypeParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "element.set_type",
        version: 1,
        summary: "Change elements' type",
        description: "Gives elements another type of their family: grids a grid type, Reviera walls a wall \
                      type (they keep their location line).",
        effects: &[Effect::Edit],
        transaction_name: "Change Type",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: types::SetTypeParams) -> Result<()> {
        types::set_type(cx.edit()?, p)
    }
}

// ---- Doors and windows (Ch 3 ARC-DOOR-001, ARC-WIN-001, ARC-HOST-001) ----

use crate::inserts::{self, Kind};

#[derive(Debug)]
struct DoorPlace;

impl Command for DoorPlace {
    type Params = inserts::PlaceParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "door.place",
        version: 1,
        summary: "Place a door",
        description: "Places a door in a Reviera wall: its centre `point_along` the wall from its start (or \
                      at `point` projected onto it), its bottom `sill_height` above the wall's base level \
                      (default 0). Facing is the wall's exterior unless `flip_facing`. The door cuts the wall \
                      and gets the next Mark. `width` and `height` give it its own size. Unless `fit` is \
                      false it fits the wall where it lands: it slides or shrinks to the free gap, and rises \
                      onto the wall's open face and shrinks under its top (a dormer's front wall).",
        effects: &[Effect::Edit],
        transaction_name: "Door",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::PlaceParams) -> Result<ElementId> {
        inserts::place(cx.edit()?, Kind::Door, p)
    }
}

#[derive(Debug)]
struct WindowPlace;

impl Command for WindowPlace {
    type Params = inserts::PlaceParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "window.place",
        version: 1,
        summary: "Place a window",
        description: "Places a window in a Reviera wall, like door.place; the sill defaults to the type's \
                      Default Sill Height.",
        effects: &[Effect::Edit],
        transaction_name: "Window",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::PlaceParams) -> Result<ElementId> {
        inserts::place(cx.edit()?, Kind::Window, p)
    }
}

#[derive(Debug)]
struct InsertFlipHand;

impl Command for InsertFlipHand {
    type Params = inserts::FlipParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "insert.flip_hand",
        version: 1,
        summary: "Flip hand",
        description: "Moves the hinge of doors to the other side.",
        effects: &[Effect::Edit],
        transaction_name: "Flip Hand",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::FlipParams) -> Result<()> {
        inserts::flip(cx.edit()?, p, true)
    }
}

#[derive(Debug)]
struct InsertFlipFacing;

impl Command for InsertFlipFacing {
    type Params = inserts::FlipParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "insert.flip_facing",
        version: 1,
        summary: "Flip facing",
        description: "Turns doors and windows to face the wall's other side (a door's swing side).",
        effects: &[Effect::Edit],
        transaction_name: "Flip Facing",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::FlipParams) -> Result<()> {
        inserts::flip(cx.edit()?, p, false)
    }
}

#[derive(Debug)]
struct InsertSetPosition;

impl Command for InsertSetPosition {
    type Params = inserts::SetPositionParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "insert.set_position",
        version: 1,
        summary: "Place a door or window along its wall",
        description: "Moves a door or window along its host wall so that its centre is `along` from the wall's \
                      start (or `from_end` from its end), measured along the wall (curved walls: along the \
                      curve). It must still fit in the wall.",
        effects: &[Effect::Edit],
        transaction_name: "Insert Position",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::SetPositionParams) -> Result<()> {
        inserts::set_position(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct InsertSetSill;

impl Command for InsertSetSill {
    type Params = inserts::SetSillParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "insert.set_sill_height",
        version: 1,
        summary: "Set sill height",
        description: "Moves doors and windows up or down: their bottom is `sill_height` above the host's base \
                      level. They are not resized.",
        effects: &[Effect::Edit],
        transaction_name: "Sill Height",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::SetSillParams) -> Result<()> {
        inserts::set_sill(cx.edit()?, p)
    }
}

#[derive(Debug)]
struct InsertSetSize;

impl Command for InsertSetSize {
    type Params = inserts::SetSizeParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "insert.set_size",
        version: 1,
        summary: "Resize doors and windows",
        description: "Sets doors' and windows' own `width` and `height` (`null` returns one to the type's), \
                      centred where they are. With `fit`, each then fits its wall: it slides or shrinks to \
                      the free gap between its neighbours and the wall's ends, and onto the wall's open face \
                      (above a roof the wall rises from, under the roof it is attached to).",
        effects: &[Effect::Edit],
        transaction_name: "Resize",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: inserts::SetSizeParams) -> Result<()> {
        inserts::set_size(cx.edit()?, p)
    }
}

// ---- Modify tools (Ch 3 §3.12, ARC-MOD-001 … 013) ----

use crate::modify;

macro_rules! modify_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, $params:ty, $out:ty, $f:expr) => {
        #[derive(Debug)]
        struct $ty;

        impl Command for $ty {
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta {
                name: $name,
                version: 1,
                summary: $summary,
                description: $desc,
                effects: &[Effect::Edit],
                transaction_name: $txn,
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&mut crate::txn::Transaction<'_>, $params) -> Result<$out> = $f;
                f(cx.edit()?, p)
            }
        }
    };
}

modify_command!(
    ElementMove,
    "element.move",
    "Move elements",
    "Moves elements by a vector (metres): Reviera walls and grids in plan (their doors and windows go with \
     them); doors and windows along their host by the vector's component along the wall, and up or down by \
     its z. Pinned elements are refused.",
    "Move",
    modify::MoveParams,
    Vec<ElementId>,
    modify::move_
);
modify_command!(
    ElementCopy,
    "element.copy",
    "Copy elements",
    "Copies elements by each vector; returns one list of copies per vector. Copied walls take their doors \
     and windows; copied doors and windows stay in their wall; copied grids get the next name.",
    "Copy",
    modify::CopyParams,
    Vec<Vec<ElementId>>,
    modify::copy
);
modify_command!(
    ElementRotate,
    "element.rotate",
    "Rotate elements",
    "Rotates walls and grids about a plan point, counter-clockwise by `angle` (accepts \"30°\"); with \
     `copy`, rotates copies.",
    "Rotate",
    modify::RotateParams,
    Vec<ElementId>,
    modify::rotate
);
modify_command!(
    ElementMirror,
    "element.mirror",
    "Mirror elements",
    "Mirrors walls and grids about an axis: two plan points `{start, end}` or `{element}` (a wall's \
     location line or a grid). Copies by default; mirrored walls keep their exterior on the mirrored side \
     and their doors turn with them.",
    "Mirror",
    modify::MirrorParams,
    Vec<ElementId>,
    modify::mirror
);
modify_command!(
    ElementOffset,
    "element.offset",
    "Offset elements",
    "Offsets walls and grids by `distance` toward `side_point`, as copies by default.",
    "Offset",
    modify::OffsetParams,
    Vec<ElementId>,
    modify::offset
);
modify_command!(
    ElementSplit,
    "element.split",
    "Split a wall",
    "Splits a Reviera wall at a point into two walls of the same type; doors and windows go to the part \
     that contains them. Returns both walls.",
    "Split",
    modify::SplitParams,
    Vec<ElementId>,
    modify::split
);
modify_command!(
    ElementSplitWithGap,
    "element.split_with_gap",
    "Split a wall with a gap",
    "Splits a straight Reviera wall at a point and cuts out a gap of `gap` (1.6 mm to 305 mm) centred on \
     it (ARC-WALL-011); the new ends do not join. Returns both walls.",
    "Split with Gap",
    modify::SplitGapParams,
    Vec<ElementId>,
    modify::split_with_gap
);
modify_command!(
    WallSplitHorizontal,
    "wall.split_horizontal",
    "Split a wall horizontally",
    "Splits a Reviera wall at a height `z` or a level (ARC-WALL-011): the lower wall's top stops there \
     (at the level, or an unconnected height), the upper wall starts there (on the level, or a base \
     offset) and keeps the top. Doors and windows go to the part they are in; one crossing the height \
     is refused. Returns the lower and the upper wall.",
    "Split Horizontally",
    modify::SplitHorizontalParams,
    Vec<ElementId>,
    modify::split_horizontal
);
modify_command!(
    ElementTrimCorner,
    "element.trim_corner",
    "Trim/extend to corner",
    "Trims or extends two walls to their intersection, keeping the parts with the picked points, making \
     an L corner. Doors and windows left outside a wall are deleted with a warning.",
    "Trim/Extend to Corner",
    modify::TrimCornerParams,
    (),
    modify::trim_corner
);
modify_command!(
    ElementTrimExtend,
    "element.trim_extend",
    "Trim/extend to a boundary",
    "Trims or extends a wall to a boundary (a wall's location line or a grid), keeping the picked part.",
    "Trim/Extend",
    modify::TrimExtendParams,
    (),
    modify::trim_extend
);
modify_command!(
    ElementAlign,
    "element.align",
    "Align",
    "Moves elements so that each target line (a wall face or location line, a grid) lies on the parallel \
     reference line. References are as for dim.create_aligned.",
    "Align",
    modify::AlignParams,
    Vec<ElementId>,
    modify::align
);
modify_command!(
    ElementPin,
    "element.pin",
    "Pin elements",
    "Pins elements: they refuse moves, rotations, mirrors, flips and deletes until unpinned.",
    "Pin",
    modify::PinParams,
    (),
    |t, p| modify::pin(t, p, true)
);
modify_command!(
    ElementUnpin,
    "element.unpin",
    "Unpin elements",
    "Unpins elements.",
    "Unpin",
    modify::PinParams,
    (),
    |t, p| modify::pin(t, p, false)
);

// ---- Floors (Ch 3 ARC-FLR-001) ----

use crate::floors;

modify_command!(
    FloorCreate,
    "floor.create",
    "Create a floor",
    "Creates a floor from boundary curves (`loops`; each `{line, lock?}`): outer loops are the floor, \
     inner loops holes. Its top is `offset` above the level and it hangs down by its type's thickness. \
     Curves picked from walls are trimmed or extended to meet; with `lock: {element, sub}` they follow \
     the wall face. Invalid sketches are refused (sketch.open, sketch.intersect, ...) with the points.",
    "Floor",
    floors::CreateParams,
    ElementId,
    floors::create
);
modify_command!(
    FloorEditBoundary,
    "floor.edit_boundary",
    "Edit a floor's boundary",
    "Replaces a floor's boundary curves (as floor.create's loops).",
    "Edit Boundary",
    floors::EditBoundaryParams,
    (),
    floors::edit_boundary
);
modify_command!(
    FloorSetLevel,
    "floor.set_level",
    "Set floors' level and offset",
    "Sets floors' level and/or the height offset of their top above it (accepts units).",
    "Floor Level",
    floors::SetLevelParams,
    (),
    floors::set_level
);

modify_command!(
    CeilingCreate,
    "ceiling.create",
    "Create a ceiling",
    "Creates a sketched ceiling from boundary curves (as floor.create): its bottom is `offset` above the \
     level (default 2600 mm) and it builds up by its type's thickness.",
    "Ceiling",
    floors::CreateParams,
    ElementId,
    |t, p| floors::create_kind(t, floors::SlabKind::Ceiling, p)
);
modify_command!(
    CeilingCreateAuto,
    "ceiling.create_auto",
    "Create an automatic ceiling",
    "Creates a ceiling in the room around `point`: its boundary is the loop of wall faces around the point \
     at the ceiling's height, locked to those faces so the ceiling follows the walls.",
    "Ceiling",
    floors::CreateAutoParams,
    ElementId,
    floors::create_auto
);
modify_command!(
    CeilingEditBoundary,
    "ceiling.edit_boundary",
    "Edit a ceiling's boundary",
    "Replaces a ceiling's boundary curves (as floor.edit_boundary, with `ceiling_id`).",
    "Edit Boundary",
    floors::EditBoundaryParams,
    (),
    floors::edit_boundary
);

use crate::roofs;

modify_command!(
    CobieSetSettings,
    "cobie.set_settings",
    "Set the project's COBie settings",
    "Stores the COBie settings (DOC-OUT-015): contacts (the first creates the data set), the categories \
     exported as components, the room parameters giving zones and space types, and whether repeated names \
     are made unique.",
    "COBie Settings",
    crate::cobie::SetSettingsParams,
    (),
    crate::cobie::set_settings
);

modify_command!(
    ScheduleCreate,
    "schedule.create",
    "Create a schedule",
    "Creates a component schedule of a category (`cat.doors`, `cat.rooms`, …) with the given field keys, or \
     the category's usual fields, sorted by the first field. schedule.fields lists the field keys.",
    "New Schedule",
    crate::schedules::CreateParams,
    ElementId,
    crate::schedules::create
);
modify_command!(
    ScheduleSetFields,
    "schedule.set_fields",
    "Set a schedule's fields",
    "Sets a schedule's columns (field keys in order); sort levels on removed fields are dropped.",
    "Schedule Fields",
    crate::schedules::SetFieldsParams,
    (),
    crate::schedules::set_fields
);
modify_command!(
    ScheduleSetSort,
    "schedule.set_sort",
    "Set a schedule's sorting",
    "Sets a schedule's sort levels (`Sort by`, then `Then by`, each ascending or descending), Itemize every \
     instance, and grand totals.",
    "Schedule Sorting",
    crate::schedules::SetSortParams,
    (),
    crate::schedules::set_sort
);
modify_command!(
    ScheduleSetFilters,
    "schedule.set_filters",
    "Set a schedule's filters",
    "Sets a schedule's filters: rows show only elements whose `field` (a field of the schedule's category) is equals, not_equals, contains, greater_than or less_than `value` (numbers by value, text ignoring case); every filter must pass. An empty list removes them.",
    "Schedule Filters",
    crate::schedules::SetFiltersParams,
    (),
    crate::schedules::set_filters
);
modify_command!(
    ScheduleSetCell,
    "schedule.set_cell",
    "Edit a schedule cell",
    "Writes a schedule cell: the field's parameter on the row's elements (text with units for measures). \
     Computed fields cannot be edited.",
    "Edit Schedule",
    crate::schedules::SetCellParams,
    (),
    crate::schedules::set_cell
);

#[derive(Debug)]
struct ScheduleFields;

impl Command for ScheduleFields {
    type Params = crate::schedules::FieldsParams;
    type Output = Vec<crate::schedules::FieldInfo>;
    const META: CommandMeta = CommandMeta {
        name: "schedule.fields",
        version: 1,
        summary: "The fields a category's schedules can show",
        description: "Lists the field keys and names a schedule of the category can show: computed fields \
                      (count, type, level, areas, sizes) and every parameter of the category.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::schedules::FieldsParams,
    ) -> Result<Vec<crate::schedules::FieldInfo>> {
        Ok(crate::schedules::fields(cx.store(), p))
    }
}

modify_command!(
    SheetCreate,
    "sheet.create",
    "Create a sheet",
    "Creates a sheet with Reviera's ISO title block (`A0` … `A4`, landscape; default `A1`). The number \
     defaults to the last sheet number incremented (A101 → A102).",
    "New Sheet",
    crate::sheets::CreateParams,
    ElementId,
    crate::sheets::create
);
modify_command!(
    SheetSet,
    "sheet.set_parameter",
    "Set a sheet's number, name, size or title-block values",
    "Sets a sheet's number (unique), name, paper size and title-block values (`Issue Date`, `Designed By`, \
     `Drawn By`, `Checked By`, `Approved By`; an empty text clears one).",
    "Sheet Properties",
    crate::sheets::SetParams,
    (),
    crate::sheets::set
);
modify_command!(
    SheetDelete,
    "sheet.delete",
    "Delete sheets",
    "Deletes sheets with their viewports and placed schedules; the views and schedules are kept.",
    "Delete Sheet",
    crate::sheets::DeleteParams,
    (),
    crate::sheets::delete
);
modify_command!(
    SheetPlaceView,
    "sheet.place_view",
    "Place a view on a sheet",
    "Places a plan, section or elevation on a sheet at its view scale, centred at `position` (paper \
     millimetres from the lower-left corner). A view can be on one sheet only. Returns the viewport id.",
    "Place View",
    crate::sheets::PlaceViewParams,
    u32,
    crate::sheets::place_view
);
modify_command!(
    SheetPlaceSchedule,
    "sheet.place_schedule",
    "Place a schedule on a sheet",
    "Places a schedule's table on a sheet with its top-left corner at `position` (paper millimetres). A \
     schedule may be placed on several sheets. Returns the placed schedule's id.",
    "Place Schedule",
    crate::sheets::PlaceScheduleParams,
    u32,
    crate::sheets::place_schedule
);
modify_command!(
    ViewportMove,
    "viewport.move",
    "Move a viewport on its sheet",
    "Moves a viewport (its centre) or a placed schedule (its top-left corner) to `position`, paper millimetres.",
    "Move Viewport",
    crate::sheets::MoveParams,
    (),
    crate::sheets::move_viewport
);
modify_command!(
    ViewportRemove,
    "viewport.remove",
    "Remove viewports from a sheet",
    "Removes viewports or placed schedules from a sheet; the views and schedules themselves are kept.",
    "Remove from Sheet",
    crate::sheets::RemoveParams,
    (),
    crate::sheets::remove_viewports
);
modify_command!(
    ViewCreateSection,
    "view.create_section",
    "Create a section view",
    "Creates a section view along the line `start`–`end` (plan, metres). The view looks to the line's \
     right-hand side and sees `far_offset` beyond it (default 30 m): what the line's plane cuts draws as cut, \
     what lies beyond in projection.",
    "Section",
    crate::sections::CreateSectionParams,
    ElementId,
    crate::sections::create_section
);
modify_command!(
    ViewCreateElevation,
    "view.create_elevation",
    "Create an elevation view",
    "Creates an elevation view whose view plane passes through `point` and looks along `look` (plan), \
     `width` wide (default 60 m) and seeing `far_offset` deep (default 120 m).",
    "Elevation",
    crate::sections::CreateElevationParams,
    ElementId,
    crate::sections::create_elevation
);
modify_command!(
    SectionEdit,
    "section.edit",
    "Move, deepen or flip a section or elevation",
    "Changes a section's or elevation's line (`start`, `end`), its far clip (`far_offset`), or turns it to \
     look the other way (`flip: true`).",
    "Edit Section",
    crate::sections::EditParams,
    (),
    crate::sections::edit
);

modify_command!(
    RoomPlace,
    "room.place",
    "Place a room",
    "Places a room in the region of wall faces around `point` on a level (ARC-ROOM-001). The room reaches \
     `limit_offset` (default 4000 mm) above its upper limit (default its own level). Number defaults to one \
     more than the highest in use, Name to \"Room\". A point inside a wall is refused; an open region gives \
     a Not Enclosed room with a warning, and a second room in a region a Redundant one.",
    "Room",
    crate::rooms::PlaceParams,
    ElementId,
    crate::rooms::place
);
modify_command!(
    RoomPlaceAutomatically,
    "room.place_automatically",
    "Place rooms in every enclosed region of a level",
    "Places a room in every region of a level enclosed by walls that has no room yet and is larger than \
     0.023 m², numbered in sequence. Returns the new rooms.",
    "Place Rooms Automatically",
    crate::rooms::PlaceAutoParams,
    Vec<ElementId>,
    crate::rooms::place_automatically
);
modify_command!(
    RoomSet,
    "room.set",
    "Set rooms' number, name and limits",
    "Sets rooms' Number (one room at a time), Name, upper limit level (`null`: the room's own level), limit \
     offset and base offset (lengths accept units).",
    "Room Properties",
    crate::rooms::SetParams,
    (),
    crate::rooms::set
);

modify_command!(
    WallAttach,
    "wall.attach",
    "Attach walls' tops to a roof",
    "Attaches the walls' tops (`end: top`) to a roof (`target_id`): each wall's top follows the roof's \
     underside over the wall, rising to the ridge. Walls the roof does not wholly cover are refused \
     (wall.attach_not_covered). Bases, floors, ceilings and reference planes come later.",
    "Attach Top/Base",
    crate::walls::attach::AttachParams,
    (),
    crate::walls::attach::attach
);
modify_command!(
    WallDetach,
    "wall.detach",
    "Detach walls' tops",
    "Detaches the walls' tops from the given targets (`target_ids`), or from all of them; each wall returns \
     to its own top constraint.",
    "Detach Top/Base",
    crate::walls::attach::DetachParams,
    (),
    crate::walls::attach::detach
);

modify_command!(
    RoofCreate,
    "roof.create_footprint",
    "Create a roof by footprint",
    "Creates a roof from a closed footprint of curves (each `{line, lock?, defines_slope?, slope?, \
     overhang?}`; curves are keyed 1, 2, ... in order). Slope-defining curves rise inward at their slope \
     (default the roof's `slope`, 30°); the others are gables. The plate is `base_offset` above the level; \
     overhangs reach past the curves and down their slope. An optional cutoff level ends the roof flat. \
     Footprints that cannot be roofed are refused with roof.cannot_generate.",
    "Roof",
    roofs::CreateParams,
    ElementId,
    roofs::create
);
modify_command!(
    RoofEditFootprint,
    "roof.edit_footprint",
    "Edit a roof's footprint",
    "Replaces a roof's footprint curves and their slope and overhang data (as roof.create_footprint's curves).",
    "Edit Footprint",
    roofs::EditFootprintParams,
    (),
    roofs::edit_footprint
);
modify_command!(
    RoofSetParams,
    "roof.set_params",
    "Set roofs' level, offset, slope and cutoff",
    "Sets roofs' base level, base offset (plate height above the level), slope (every slope-defining \
     curve; curves' own slopes are cleared) and cutoff level and offset (`cutoff_level_id: null` removes \
     the cutoff). Lengths and angles accept units.",
    "Roof Properties",
    roofs::SetParams,
    (),
    roofs::set_params
);
modify_command!(
    RoofSetSlope,
    "roof.set_slope",
    "Set a roof's slope",
    "Sets the slope of a roof (radians or text such as \"35°\"): of the curves `edge_keys` only, or of \
     every slope-defining curve when they are omitted.",
    "Roof Slope",
    roofs::SetSlopeParams,
    (),
    roofs::set_slope
);
modify_command!(
    RoofSetEdge,
    "roof.set_edge",
    "Set roof footprint curves' slope and overhang",
    "Sets footprint curves' Defines Slope, own Slope (`null` returns to the roof's) and Overhang, by curve \
     key.",
    "Roof Edge",
    roofs::SetEdgeParams,
    (),
    roofs::set_edge
);
modify_command!(
    RoofSetEaves,
    "roof.set_eaves",
    "Set a roof's eaves and wall bearing",
    "Sets eaves on footprint curves (`edges`; omitted, every slope-defining edge) (ARC-ROOF-010): the \
     `bearing` on the wall below (`exterior` face, `core` centre or `interior` face), a `heel` height, the \
     `overhang` from the wall's exterior face, the rafter `cut` (`plumb`, `square`, `two_cut`) with its \
     `fascia_depth`, a `fascia`, and the `soffit` (`boxed`, `sloped`, `open`) returning to the wall's face. \
     The perimeter walls under the roof are attached to it unless `attach_walls` is false. `remove` turns \
     the edges back into plain overhangs.",
    "Roof Eaves",
    roofs::eaves::SetEavesParams,
    (),
    roofs::eaves::set_eaves
);

modify_command!(
    RoofCreateArched,
    "roof.create_arched",
    "Create an arched roof",
    "Creates a smooth arched (barrel or segmental) roof over a four-sided footprint (ARC-ROOF-008), springing \
     from two opposite curves (`spring_edges`, default the longer pair) at the base offset. `rise` (default a \
     quarter of the span) or `radius` sets the arch at the first end; `profile` is `segment`, `semicircle`, \
     `elliptical` or `catenary`. Over a trapezoid the vault tapers: `taper` keeps the rise (`constant_rise`) \
     or scales it with the span (`constant_slope`). `overhang` continues the arch past the spring edges, \
     `end_overhang` past the ends.",
    "Arched Roof",
    roofs::curved::CreateArchedParams,
    ElementId,
    roofs::curved::create_arched
);
modify_command!(
    RoofSetArch,
    "roof.set_arch",
    "Set an arched roof's arch",
    "Makes a footprint roof arched, or changes its arch: spring edges, rise or radius, profile, taper and \
     overhangs (ARC-ROOF-008).",
    "Arch",
    roofs::curved::SetArchParams,
    (),
    roofs::curved::set_arch
);
modify_command!(
    RoofCreateDome,
    "roof.create_dome",
    "Create a dome",
    "Creates a dome (ARC-ROOF-009) at `center` over a `plan`: `{\"circle\": {\"radius\"}}`, \
     `{\"ellipse\": {\"radii\", \"rotation\"}}` or `{\"polygon\": {\"radius\", \"sides\", \"rotation\"}}` (a \
     cloister vault). `profile` is `hemisphere` (rise = radius), `segmental` (a lower `rise`), `pointed` (arcs \
     of `ratio` × the span, default 1) or `onion` (`rise`, bulge `ratio`, default 1.25). `oculus` leaves an \
     opening of that diameter at the crown; `faceted` makes flat facets.",
    "Dome",
    roofs::curved::CreateDomeParams,
    ElementId,
    roofs::curved::create_dome
);
modify_command!(
    RoofSetDome,
    "roof.set_dome",
    "Change a dome",
    "Changes a dome's centre, plan, rise, profile, ratio, oculus or faceting (ARC-ROOF-009).",
    "Dome",
    roofs::curved::SetDomeParams,
    (),
    roofs::curved::set_dome
);

modify_command!(
    ClearanceSetRuleSet,
    "clearance.set_rule_set",
    "Set the accessibility rule set",
    "Sets the project's accessibility rule set (ARC-MOD-031): `none` (door swings only) or `ada_2010` \
     (adds door maneuvering clearances: 1525 mm deep on the pull side and 455 mm past the latch, 1220 mm on \
     the push side). The clearances are virtual volumes that `clearance.check` tests for intrusion.",
    "Accessibility Rules",
    crate::clearances::RuleSetParams,
    (),
    crate::clearances::set_rule_set
);
modify_command!(
    CodeSet,
    "code.set",
    "Set the model's building code",
    "Sets the building code the model follows by default (DM-CODE-001): `ibc`, `irc`, `uk_adk`, \
     `nbc_canada`, `ncc_australia`, `din_18065` or `custom`; the occupancy (`dwelling` or `other`, where the \
     code tells them apart); and the model's own stair headroom in place of the code's (`null` returns to \
     it). Tools use these values unless told to use another code.",
    "Building Code",
    crate::codes::SetParams,
    (),
    crate::codes::set
);

modify_command!(
    OpeningCreateVertical,
    "opening.create_vertical",
    "Cut a floor or ceiling plumb",
    "Cuts a Reviera floor or ceiling by closed loops in plan (ARC-OPEN-002), plumb through its whole \
     thickness; a loop partly outside notches its edge. The opening is an IfcOpeningElement voiding the \
     host; it moves with the host and goes with it. Returns the opening.",
    "Vertical Opening",
    crate::slab_openings::CreateParams,
    ElementId,
    crate::slab_openings::create
);

modify_command!(
    OpeningCreateByFace,
    "opening.create_by_face",
    "Cut an opening square to a face",
    "Cuts a Reviera floor, ceiling or footprint roof by closed loops sketched on its face (ARC-OPEN-001), \
     given as seen from above: the opening's sides are square to the face, so on a sloped roof they lean \
     with it. The opening is an IfcOpeningElement voiding the host, its body extruded along the face \
     normal; it moves with the host and goes with it. Returns the opening.",
    "Opening by Face",
    crate::slab_openings::CreateParams,
    ElementId,
    crate::slab_openings::create_by_face
);

modify_command!(
    OpeningSetLoops,
    "opening.set_loops",
    "Change a vertical opening's loops",
    "Gives a vertical opening new loops in plan (its grips drag corners and edges); they must still cut \
     its host.",
    "Edit Opening",
    crate::slab_openings::SetLoopsParams,
    (),
    crate::slab_openings::set_loops
);

modify_command!(
    StairCreateOpenings,
    "stair.create_openings",
    "Open the floors over a stair",
    "Opens every Reviera floor and ceiling over a stair that leaves less than the headroom (ARC-STAIR-007): \
     the model's building code by default (`code.get`), another `code`, or a `headroom` of its own. Each \
     opening covers the treads and landings short of headroom over the stair's full width and records the \
     stair, headroom and clause; an opening made for the stair before takes the new outline. Returns the \
     openings.",
    "Stair Openings",
    crate::slab_openings::HeadroomParams,
    Vec<ElementId>,
    crate::slab_openings::create_for_stair
);

#[derive(Debug)]
struct StairHeadroomCheck;

impl Command for StairHeadroomCheck {
    type Params = crate::slab_openings::HeadroomParams;
    type Output = crate::slab_openings::HeadroomReport;
    const META: CommandMeta = CommandMeta {
        name: "stair.headroom_check",
        version: 1,
        summary: "Check a stair's headroom",
        description: "The floors and ceilings over a stair that leave less than the headroom (ARC-STAIR-007), \
                      measured from the nosing line or a landing's floor: each one's least clearance, the \
                      region to open, the area it still covers, and any opening made for the stair. Uses the \
                      model's building code unless given another `code` or a `headroom`.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::slab_openings::HeadroomParams,
    ) -> Result<crate::slab_openings::HeadroomReport> {
        crate::slab_openings::check(cx.store(), p)
    }
}

#[derive(Debug)]
struct CodeGet;

impl Command for CodeGet {
    type Params = Option<Value>;
    type Output = crate::codes::CodeReport;
    const META: CommandMeta = CommandMeta {
        name: "code.get",
        version: 1,
        summary: "The model's building code",
        description: "The model's building code settings, the stair headroom in force with the clause it comes \
                      from, and every code Reviera knows with its headroom (DM-CODE-001).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        _: Option<Value>,
    ) -> Result<crate::codes::CodeReport> {
        Ok(crate::codes::report(cx.store()))
    }
}

modify_command!(
    ViewSetVirtualVolumes,
    "view.set_virtual_volumes",
    "Show or hide virtual volumes in a view",
    "Sets how a view shows virtual volumes (door swings, accessibility clearances; ARC-MOD-031): `hidden`, \
     `translucent` or `solid`.",
    "Virtual Volumes",
    crate::clearances::ViewModeParams,
    (),
    crate::clearances::set_view_mode
);

modify_command!(
    OverlapAccept,
    "overlap.accept",
    "Accept an overlap as intended",
    "Marks the overlap between `a` and `b` as intended (ARC-MOD-032): overlap checks leave the pair out. \
     It is kept on the project by GlobalId with who accepted it, when and an optional `note`. \
     `accepted: false` takes it back.",
    "Accept Overlap",
    crate::overlap_accept::AcceptParams,
    (),
    crate::overlap_accept::accept
);

modify_command!(
    ClashRecordRun,
    "clash.record_run",
    "Record a clash run",
    "Merges a clash run's results into the model's clash results (COL-CLASH-002), matched by their elements' \
     GlobalIds: first found are *new*, found again become *active* (reviewed and approved keep their status), \
     and results the run no longer finds become *resolved*. Also stores the test that ran. Used by \
     `clash.collect`.",
    "Clash Run",
    crate::clashes::RecordRunParams,
    crate::clashes::RunSummary,
    crate::clashes::record_run
);

modify_command!(
    ClashSetStatus,
    "clash.set_status",
    "Set clash results' status",
    "Sets the status of clash results (`new`, `active`, `reviewed`, `approved`, `resolved`) and adds an \
     optional comment to each, with who wrote it and when (COL-CLASH-002).",
    "Clash Status",
    crate::clashes::SetStatusParams,
    (),
    crate::clashes::set_status
);

modify_command!(
    GeometryBoolean,
    "geometry.boolean",
    "Boolean between elements",
    "Shapes `target_id` by `tool_ids` (ARC-MOD-028): `subtract` removes the tools' volume from the target, \
     `intersect` keeps only what they share, `union` fuses them into the target. The relation is kept and \
     re-applied whenever either element changes; the target keeps its category, type and parameters. A \
     union hides the tools' own bodies; otherwise `tools: hide` does.",
    "Boolean",
    crate::csg::BooleanParams,
    (),
    crate::csg::boolean
);
modify_command!(
    GeometryRemoveBoolean,
    "geometry.remove_boolean",
    "Remove a boolean",
    "Removes the target's booleans (with `tool_ids`, only those tools'); its own body returns.",
    "Remove Boolean",
    crate::csg::RemoveParams,
    (),
    crate::csg::remove
);

#[derive(Debug)]
struct OverlapAccepted;

impl Command for OverlapAccepted {
    type Params = Option<NoParams>;
    type Output = Vec<crate::overlap_accept::Accepted>;
    const META: CommandMeta = CommandMeta {
        name: "overlap.accepted",
        version: 1,
        summary: "List accepted overlaps",
        description: "The overlaps accepted as intended (ARC-MOD-032): each pair's GlobalIds, who accepted it, when \
                      and why.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        _: Option<NoParams>,
    ) -> Result<Vec<crate::overlap_accept::Accepted>> {
        Ok(crate::overlap_accept::accepted(cx.store()))
    }
}

#[derive(Debug)]
struct ClearanceList;

impl Command for ClearanceList {
    type Params = Option<crate::clearances::ListParams>;
    type Output = Vec<crate::clearances::ClearanceInfo>;
    const META: CommandMeta = CommandMeta {
        name: "clearance.list",
        version: 1,
        summary: "List virtual volumes",
        description: "Lists the virtual volumes (ARC-MOD-031), or those of `element_ids`: each one's element, kind \
                      (door swing, pull-side or push-side clearance), rule and box.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: Option<crate::clearances::ListParams>,
    ) -> Result<Vec<crate::clearances::ClearanceInfo>> {
        crate::clearances::list(cx.store(), p.unwrap_or_default())
    }
}

#[derive(Debug)]
struct RoofEaves;

impl Command for RoofEaves {
    type Params = roofs::eaves::EavesParams;
    type Output = Vec<roofs::eaves::EaveRow>;
    const META: CommandMeta = CommandMeta {
        name: "roof.eaves",
        version: 1,
        summary: "A roof's eaves",
        description: "Every eave of a roof: its footprint curve's key, its overhang from the wall's exterior \
                      face, and its bearing, heel, rafter cut, fascia and soffit.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: roofs::eaves::EavesParams,
    ) -> Result<Vec<roofs::eaves::EaveRow>> {
        roofs::eaves::eaves_of(cx.store(), p)
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

modify_command!(
    DeckCreate,
    "deck.create",
    "Create a deck",
    "Creates a deck (ARC-DECK-001): a floor `height` above the level (default 600 mm) over `outline` (loops of \
     curves, as for floors), with its boards laid out one by one (courses, joints on joists, staggered, \
     picture frame) and its framing (joists, beams, posts, footings). `settings` overrides the defaults: \
     boardWidth, boardThickness, gap, direction (radians), stockLengths, pictureFrame, balanceCourses, \
     stagger (longest|stair_step|random), minStagger, repeatAfter, seed, joistSpacing, joist, \
     beamSpacing, beam, postSpacing, post, footing.",
    "Deck",
    crate::decks::CreateParams,
    ElementId,
    crate::decks::create
);
modify_command!(
    DeckSetBoards,
    "deck.set_boards",
    "Change a deck's board layout",
    "Changes a deck's board settings (`settings`: the fields to change, as in deck.create); the boards are laid \
     out again.",
    "Deck Boards",
    crate::decks::SetParams,
    (),
    crate::decks::set
);
modify_command!(
    DeckSetFraming,
    "deck.set_framing",
    "Change a deck's framing",
    "Changes a deck's framing settings (joistSpacing, joist, beamSpacing, beam, postSpacing, post, footing); the \
     framing and the board joints follow.",
    "Deck Framing",
    crate::decks::SetParams,
    (),
    crate::decks::set
);

#[derive(Debug)]
struct DeckBoards;

impl Command for DeckBoards {
    type Params = crate::decks::QueryParams;
    type Output = Vec<crate::decks::Board>;
    const META: CommandMeta = CommandMeta {
        name: "deck.boards",
        version: 1,
        summary: "A deck's boards",
        description: "Every board of a deck: its key, course (none for frame boards), outline, length, width, the \
                      stock length it is cut from and the board whose offcut it uses.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::decks::QueryParams,
    ) -> Result<Vec<crate::decks::Board>> {
        crate::decks::boards(cx.store(), p)
    }
}

#[derive(Debug)]
struct DeckCutList;

impl Command for DeckCutList {
    type Params = crate::decks::QueryParams;
    type Output = crate::decks::CutList;
    const META: CommandMeta = CommandMeta {
        name: "deck.cut_list",
        version: 1,
        summary: "A deck's cut list and quantities",
        description: "The board count and total length, the stock lengths to buy, the area, the joists, beams, posts \
                      and footings, the screws, and any layout warnings.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::decks::QueryParams,
    ) -> Result<crate::decks::CutList> {
        crate::decks::cut_list(cx.store(), p)
    }
}

modify_command!(
    RailingCreate,
    "railing.create",
    "Create a railing along a path",
    "Creates a guard railing along `path` (plan points; `closed` joins the last to the first) at `base_level` \
     plus `base_offset`: a top and a bottom rail, posts at the ends and corners, and balusters at the type's \
     spacing (ARC-RAIL-001/002).",
    "Railing",
    crate::railings::CreateParams,
    ElementId,
    crate::railings::create
);
modify_command!(
    RailingCreateOnEdges,
    "railing.create_on_edges",
    "Railings along a floor's, deck's or flat roof's edges",
    "Creates railings along a host's footprint edges (`edges`: curve keys in order along one chain), set in by \
     `setback` (default 50 mm), on the host's top. Without `edges`, guards every open edge: one railing per \
     chain of edges with no wall along them. The railings follow the host's outline.",
    "Railing",
    crate::railings::CreateOnEdgesParams,
    Vec<ElementId>,
    crate::railings::create_on_edges
);
modify_command!(
    RailingCreateOnStair,
    "railing.create_on_stair",
    "Railings on a stair",
    "Creates railings on a stair's sides (`sides`: `left`, `right`, going up; default both), `inset` in from \
     its edges (default 50 mm). Each follows the nosing line at the type's height, crosses the landings at \
     their height (round the outside of a turn, straight across the inside) and follows the stair when it \
     changes. `stair.create` places them unless `railings` is false.",
    "Railing",
    crate::railings::CreateOnStairParams,
    Vec<ElementId>,
    crate::railings::create_on_stair
);
modify_command!(
    RailingAddOpening,
    "railing.add_opening",
    "Add an opening to a railing",
    "Adds an opening to a laid-out railing (ARC-RAIL-003): its centre at `at` (a distance along the path) or \
     the nearest point of the path to `point`, `width` clear between the posts (default 900 mm). A post goes \
     each side and the rails and balusters stop there. The opening must lie within one straight run, clear of \
     the corner posts and other openings. Returns the opening's key.",
    "Railing Opening",
    crate::railings::AddOpeningParams,
    u32,
    crate::railings::add_opening
);
modify_command!(
    RailingAddGate,
    "railing.add_gate",
    "Add a gate to a railing",
    "Adds a gate to a laid-out railing (ARC-RAIL-003): an opening as `railing.add_opening` (default width 1 m, \
     or 1.2 m double) filled with one leaf or two (`leaves`), swinging in or out (`swing_out`), a single leaf \
     hinged at the start side or the end (`hinge_end`). The gate is its own `IfcDoor` (GATE) element, hosted \
     by the railing, that follows it. Returns the gate.",
    "Railing Gate",
    crate::railings::AddGateParams,
    ElementId,
    crate::railings::add_gate
);
modify_command!(
    RailingMoveFeature,
    "railing.move_feature",
    "Move or resize a railing opening or gate",
    "Moves an opening or gate (`key`) along its railing to `at` or `point`, and/or changes its `width`.",
    "Move Railing Opening",
    crate::railings::MoveFeatureParams,
    (),
    crate::railings::move_feature
);
modify_command!(
    RailingRemoveFeature,
    "railing.remove_feature",
    "Remove a railing opening or gate",
    "Removes an opening or gate (`key`) and the gate's element; the railing closes over it.",
    "Remove Railing Opening",
    crate::railings::FeatureParams,
    (),
    crate::railings::remove_feature
);

#[derive(Debug)]
struct RailingFeatures;

impl Command for RailingFeatures {
    type Params = crate::railings::FeaturesParams;
    type Output = Vec<crate::railings::RailFeature>;
    const META: CommandMeta = CommandMeta {
        name: "railing.features",
        version: 1,
        summary: "A railing's openings and gates",
        description: "Every opening and gate of a railing: its key, centre (distance along the path), clear \
                      width, and for a gate its leaves, swing, hinge side and gate element.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::railings::FeaturesParams,
    ) -> Result<Vec<crate::railings::RailFeature>> {
        crate::railings::features(cx.store(), p)
    }
}

modify_command!(
    StairCreate,
    "stair.create",
    "Create a stair",
    "Creates a cast-in-place stair of straight runs from `base_level` to `top` (a level, default the next \
     one up, or `{\"unconnected\": height}`). Each run goes from `start` (the middle of its first riser) \
     toward `end` and holds as many risers as its length allows at the tread depth (or `risers`); an \
     automatic landing joins each run to the next. The riser count is the fewest that keep each riser \
     at most the type's maximum (or `desired_risers`); the riser height is the height over that count.",
    "Stair",
    crate::stairs::CreateParams,
    ElementId,
    crate::stairs::create
);
modify_command!(
    StairSetParams,
    "stair.set_params",
    "Set stairs' levels, risers, tread depth and width",
    "Sets stairs' base and top (level or unconnected height) with offsets, the desired number of risers \
     (0 for the fewest allowed), the tread depth and the width of every run. When the riser count \
     changes, the last run takes the difference (unless `rebalance` is false).",
    "Stair Properties",
    crate::stairs::SetParams,
    (),
    crate::stairs::set_params
);
modify_command!(
    SketchMoveCurve,
    "sketch.move_curve",
    "Move a footprint curve",
    "Moves one curve of a roof's, floor's or ceiling's footprint across itself by `vector` (only the part \
     across the curve counts); its neighbours keep their directions and stretch or shorten to meet it. \
     The moved curve leaves the wall it was locked to. Keys are the curves' 1-based order.",
    "Move Footprint Edge",
    crate::sketch_edit::MoveCurveParams,
    (),
    crate::sketch_edit::move_curve_command
);
modify_command!(
    SketchMoveCorner,
    "sketch.move_corner",
    "Move a footprint corner",
    "Moves the corner of a roof's, floor's or ceiling's footprint at `from` to `to`; the curves ending \
     there follow and leave the walls they were locked to.",
    "Move Footprint Corner",
    crate::sketch_edit::MoveCornerParams,
    (),
    crate::sketch_edit::move_corner_command
);
modify_command!(
    DormerCreate,
    "dormer.create",
    "Add a dormer to a roof",
    "Places a dormer on a sloped face of a Reviera footprint roof, its front's middle at `point`, running up the \
     face (ARC-ROOF-007). Styles: `gable` (pitched, ridge into the roof), `hip`, `shed` (a flat lift \
     shallower than the roof) and `flat`. Makes the dormer roof and the front and cheek walls that fill \
     the gap down to the main roof (their tops attached to the dormer roof). Defaults: 1.8 m wide, eave \
     0.9 m above the roof, the main roof's slope (a shed a third of it), 300 mm overhang.",
    "Dormer",
    crate::dormers::CreateParams,
    ElementId,
    crate::dormers::create
);
modify_command!(
    DormerSet,
    "dormer.set",
    "Change a dormer",
    "Changes a dormer's style, width, front height, slope or overhang; its roof and walls are rebuilt.",
    "Dormer",
    crate::dormers::SetParams,
    (),
    crate::dormers::set
);
modify_command!(
    DormerMove,
    "dormer.move",
    "Move a dormer",
    "Moves a dormer to `point` (the middle of its front) on its roof; its roof and walls are rebuilt there.",
    "Move Dormer",
    crate::dormers::MoveParams,
    (),
    crate::dormers::move_dormer
);
modify_command!(
    RoofAttachWalls,
    "roof.attach_walls",
    "Attach the walls below a roof",
    "Attaches the tops of every Reviera wall the roof covers (and that starts below it) to the roof, so gable \
     walls rise to the ridge and the others meet the roof's underside. Returns the walls attached.",
    "Attach Walls",
    crate::walls::attach::AttachBelowParams,
    Vec<ElementId>,
    crate::walls::attach::attach_below
);
modify_command!(
    RoofSetShape,
    "roof.set_shape",
    "Set a roof's shape",
    "Sets which footprint curves slope from a shape: `hip` (all), `gable` (the curves along the longest one; \
     gables at the ends), `gable_rotated` (the curves across it), `shed` (the longest curve only), \
     `shed_reversed` (the curve opposite it) or `flat` (none). Slopes are kept; `overhang` sets every \
     curve's overhang.",
    "Roof Shape",
    crate::roofs::SetShapeParams,
    (),
    crate::roofs::set_shape
);
modify_command!(
    TagCreate,
    "tag.create",
    "Tag an element",
    "Tags an element in a plan with values read live from it (DOC-ANN-010): `Door Tag` and `Window Tag` show \
     the Mark, `Room Tag` the room's name and number (`Room Tag With Area` adds the area), `Wall Tag` the \
     type, and `Multi-Category Tag` the Mark of any element. The tag type defaults to the category's \
     (tag.set_default_type). The tag follows its host; `leader` draws a line to it.",
    "Tag",
    crate::notes::CreateTagParams,
    ElementId,
    crate::notes::create_tag
);
modify_command!(
    TagCreateRoom,
    "tag.create_room",
    "Tag a room",
    "Places a room tag (`Room Tag` or `Room Tag With Area`) in a plan, at the room's tag point unless a \
     position is given.",
    "Tag Room",
    crate::notes::CreateRoomTagParams,
    ElementId,
    crate::notes::create_room_tag
);
modify_command!(
    TagTagAll,
    "tag.tag_all",
    "Tag all not tagged",
    "Tags every element of the categories on the plan's level (scope `view`) or among `element_ids` \
     (scope `selection`) that has no tag of its category's default tag type in the view. Running it again \
     creates no duplicates. Returns the new tags.",
    "Tag All",
    crate::notes::TagAllParams,
    Vec<ElementId>,
    crate::notes::tag_all
);
modify_command!(
    TagSetLeader,
    "tag.set_leader",
    "Set a tag's leader",
    "Sets how a tag's leader ends: `none`, `attached` (at the host, following it) or `free` (at the last of \
     `points`).",
    "Tag Leader",
    crate::notes::SetLeaderParams,
    (),
    crate::notes::set_leader
);
modify_command!(
    TagSetType,
    "tag.set_type",
    "Change tags' type",
    "Changes the tag type (it must tag the hosts' category) and optionally the orientation of tags.",
    "Tag Type",
    crate::notes::SetTagTypeParams,
    (),
    crate::notes::set_tag_type
);
modify_command!(
    TagSetDefaultType,
    "tag.set_default_type",
    "Set a category's default tag",
    "Loaded Tags: the tag type that Tag by Category and Tag All use for a category.",
    "Loaded Tags",
    crate::notes::SetDefaultTypeParams,
    (),
    crate::notes::set_default_type
);

#[derive(Debug)]
struct TagList;

impl Command for TagList {
    type Params = crate::notes::ListParams;
    type Output = Vec<crate::notes::ResolvedTag>;
    const META: CommandMeta = CommandMeta {
        name: "tag.list",
        version: 1,
        summary: "List a view's tags",
        description: "The tags of a view with their host, type, current text and position, and whether they are \
                      orphaned (host deleted).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::notes::ListParams,
    ) -> Result<Vec<crate::notes::ResolvedTag>> {
        crate::notes::list_tags(cx.store(), p)
    }
}

modify_command!(
    TextCreate,
    "text.create",
    "Place a text note",
    "Places a text note in a plan with its top-left corner at `position` (DOC-ANN-009). Types: `Text 2.5 mm`, \
     `Text 3.5 mm`, `Text 5 mm` (text height on paper). `width` wraps the text (paper mm); `\\n` starts a \
     new line.",
    "Text",
    crate::notes::CreateTextParams,
    ElementId,
    crate::notes::create_text
);
modify_command!(
    SpotCreateElevation,
    "spot.create_elevation",
    "Place a spot elevation",
    "Places a spot elevation in a plan, section or elevation (DOC-ANN-006): the height of `point` (model \
     coordinates) on the element `ref`, measured from the project base point (`base: \"project\"`, the default), \
     the survey point (`\"survey\"`, shared height) or a level (`{\"level\": id}`). It follows the element's \
     surface as the model changes. `display`: `actual` (the point), `top`, `bottom` or `top_and_bottom` of the \
     element there. The text stands at `leader_point` (the view's 2D coordinates: plan x, y, or a section's x \
     and height).",
    "Spot Elevation",
    crate::spots::CreateSpotParams,
    ElementId,
    crate::spots::create_elevation
);
modify_command!(
    SpotCreateCoordinate,
    "spot.create_coordinate",
    "Place a spot coordinate",
    "Places a spot coordinate in a plan, section or elevation (DOC-ANN-007): north and east of `point` on the \
     element `ref`, from the project base point along project north (`base: \"project\"`, the default) or in \
     shared coordinates from the survey point (`\"survey\"`), turned with true north; `include_elevation` adds \
     its elevation.",
    "Spot Coordinate",
    crate::spots::CreateSpotParams,
    ElementId,
    crate::spots::create_coordinate
);
modify_command!(
    SpotPutSlope,
    "spot.put_slope",
    "Store a spot slope as given",
    "Stores a spot slope at `point` on `ref` without checking the model's faces (DOC-ANN-008); \
     `spot.create_slope` checks that the face there has a slope first and is the command to use. \
     `representation`: `arrow` or `triangle` (sections and elevations); `slope_format`: `percent`, \
     `ratio` or `degrees`.",
    "Spot Slope",
    crate::spots::CreateSpotParams,
    ElementId,
    crate::spots::put_slope
);
modify_command!(
    SpotSet,
    "spot.set",
    "Change spots",
    "Sets spots' base, display (elevations), include_elevation (coordinates), prefix and suffix, or moves their \
     text to `leader_point`.",
    "Spot Properties",
    crate::spots::SetSpotParams,
    (),
    crate::spots::set
);
modify_command!(
    TextSetContent,
    "text.set_content",
    "Edit a text note",
    "Replaces a text note's text; a note left empty is deleted.",
    "Edit Text",
    crate::notes::SetContentParams,
    (),
    crate::notes::set_content
);
modify_command!(
    TextSet,
    "text.set",
    "Change text notes' type, width or rotation",
    "Sets the text type, the wrapping width (paper mm; 0 unwraps) and the rotation of text notes.",
    "Text Properties",
    crate::notes::SetTextParams,
    (),
    crate::notes::set_text
);
modify_command!(
    TextAddLeader,
    "text.add_leader",
    "Add a leader to a text note",
    "Adds a straight leader from the note's side (default: the side facing the end) through an optional \
     elbow to an arrow at the last point.",
    "Add Leader",
    crate::notes::AddLeaderParams,
    (),
    crate::notes::add_leader
);
modify_command!(
    TextRemoveLeaders,
    "text.remove_leaders",
    "Remove a text note's leaders",
    "Removes all leaders of a text note.",
    "Remove Leaders",
    crate::notes::RemoveLeadersParams,
    (),
    crate::notes::remove_leaders
);

#[derive(Debug)]
struct TextList;

impl Command for TextList {
    type Params = crate::notes::ListParams;
    type Output = Vec<crate::notes::TextInfo>;
    const META: CommandMeta = CommandMeta {
        name: "text.list",
        version: 1,
        summary: "List a view's text notes",
        description: "The text notes of a view with their text, type, position, width, rotation and leaders.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::notes::ListParams,
    ) -> Result<Vec<crate::notes::TextInfo>> {
        crate::notes::list_texts(cx.store(), p)
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
