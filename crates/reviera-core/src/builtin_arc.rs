// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architecture commands added for the first pre-release: wall openings and shafts
//! (ARC-OPEN-003, ARC-OPEN-004), and the tools after them. Registered with the core commands
//! by [`crate::builtin::register`].

use crate::command::{Command, CommandContext, CommandMeta, CommandRegistry, Effect};
use crate::error::Result;
use crate::id::ElementId;

/// A command that edits the model through one function of a transaction.
macro_rules! edit_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, [$($eff:expr),*], $params:ty, $out:ty, $f:expr) => {
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
                effects: &[$($eff),*],
                transaction_name: $txn,
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&mut crate::txn::Transaction<'_>, $params) -> Result<$out> = $f;
                f(cx.edit()?, p)
            }
        }
    };
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, $params:ty, $out:ty, $f:expr) => {
        edit_command!($ty, $name, $summary, $desc, $txn, [Effect::Edit], $params, $out, $f);
    };
}

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add(OpeningCreateWall),
        registry.add(OpeningEditWall),
        registry.add(OpeningCreateShaft),
        registry.add(OpeningEditShaft),
        registry.add(ColumnPlace),
        registry.add(ColumnPlaceStructural),
        registry.add(ColumnPlaceAtGrids),
        registry.add(ColumnPlaceSlanted),
        registry.add(ColumnSetSlant),
        registry.add(ColumnLocationMark),
        registry.add(ColumnLoadSection),
        registry.add(StructuralCatalogs),
        registry.add(StructuralMaterials),
        registry.add(ColumnCheck),
        registry.add(StructuralStandards),
        registry.add(BeamCreate),
        registry.add(BraceCreate),
        registry.add(AnalyticalMemberCreate),
        registry.add(AnalyticalMemberCreateVertical),
        registry.add(AnalyticalMemberSetReleases),
        registry.add(AnalyticalMemberSet),
        registry.add(AnalyticalNodeMerge),
        registry.add(AnalyticalNodeMove),
        registry.add(AnalyticalModelReset),
        registry.add(AnalyticalPanelCreateByBoundary),
        registry.add(AnalyticalPanelCreateByExtrusion),
        registry.add(AnalyticalPanelSplit),
        registry.add(AnalyticalPanelSetCladding),
        registry.add(AnalyticalPanelSet),
        registry.add(AnalyticalOpeningCreate),
        registry.add(AnalyticalOpeningRemove),
        registry.add(AnalyticalAssociationAdd),
        registry.add(AnalyticalAssociationRemove),
        registry.add(AnalyticalPhysicalToAnalytical),
        registry.add(AnalyticalAnalyticalToPhysical),
        registry.add(AnalyticalDisconnect),
        registry.add(AnalyticalModelInfo),
        registry.add(ConnectionTypeList),
        registry.add(RebarPlace),
        registry.add(RebarSetLayout),
        registry.add(RebarSet),
        registry.add(RebarRehost),
        registry.add(RebarInfoCmd),
        registry.add(RebarCanHost),
        registry.add(RebarBarTypeList),
        registry.add(RebarBarTypeCreate),
        registry.add(RebarHookTypeList),
        registry.add(RebarHookTypeCreate),
        registry.add(RebarShapeList),
        registry.add(ReinforcementSettingsGet),
        registry.add(ReinforcementSettingsSet),
        registry.add(RebarCoverTypeCreate),
        registry.add(RebarCoverTypeSet),
        registry.add(RebarCoverTypeDelete),
        registry.add(RebarCoverAssign),
        registry.add(AreaReinforcementCreate),
        registry.add(AreaReinforcementSet),
        registry.add(AreaReinforcementRemoveSystem),
        registry.add(PathReinforcementCreate),
        registry.add(PathReinforcementSet),
        registry.add(FabricAreaCreate),
        registry.add(FabricAreaSet),
        registry.add(FabricSheetPlace),
        registry.add(FabricWireTypeList),
        registry.add(FabricWireTypeCreate),
        registry.add(FabricSheetTypeList),
        registry.add(FabricSheetTypeCreate),
        registry.add(ReinforcementSystemInfo),
        registry.add(RebarSetViewVisibility),
        registry.add(RebarSetPresentation),
        registry.add(RebarViewStates),
        registry.add(RebarSetPartition),
        registry.add(ReinforcementNumberingList),
        registry.add(ReinforcementNumberingRenumber),
        registry.add(ReinforcementNumberingSet),
        registry.add(ReinforcementNumberingApply),
        registry.add(RebarTagPlace),
        registry.add(MultiRebarAnnotationPlace),
        registry.add(BendingDetailPlace),
        registry.add(RebarAnnotationList),
        registry.add(RebarScheduleCreate),
        registry.add(RebarSetSegmentLength),
        registry.add(RebarMoveBar),
        registry.add(RebarRemoveBar),
        registry.add(RebarResetBars),
        registry.add(RebarSplitSet),
        registry.add(RebarConvertToVarying),
        registry.add(RebarSketch),
        registry.add(PrecastConfigGet),
        registry.add(PrecastConfigSet),
        registry.add(PrecastConfigApply),
        registry.add(PrecastConfigExport),
        registry.add(PrecastConfigImport),
        registry.add(PrecastConfigAffected),
        registry.add(PrecastSplit),
        registry.add(PrecastResplit),
        registry.add(PrecastSetAutoUpdate),
        registry.add(PrecastInfo),
        registry.add(PrecastReinforce),
        registry.add(PrecastCustomFabric),
        registry.add(PrecastMountingParts),
        registry.add(PrecastShopDrawings),
        registry.add(PrecastCamExport),
        registry.add(CamWriterList),
        registry.add(MountingPartPlace),
        registry.add(PrecastReinforcementTypes),
        registry.add(RebarPlaceFreeForm),
        registry.add(RebarDistributeAlongPath),
        registry.add(RebarDistributeMorphed),
        registry.add(RebarConstrain),
        registry.add(RebarResetConstraints),
        registry.add(RebarConstraints),
        registry.add(RebarConvertToStandard),
        registry.add(RebarPropagate),
        registry.add(RebarPickNewHost),
        registry.add(RebarSplice),
        registry.add(RebarCouplerPlace),
        registry.add(RebarCouplerInfo),
        registry.add(CustomCustomize),
        registry.add(CustomAdd),
        registry.add(CustomRemove),
        registry.add(CustomExpose),
        registry.add(CustomSet),
        registry.add(CustomList),
        registry.add(CheckSetForces),
        registry.add(CheckRecord),
        registry.add(CheckStatus),
        registry.add(NumberingRun),
        registry.add(NumberingPreview),
        registry.add(CutCope),
        registry.add(CutMiter),
        registry.add(CutSaw),
        registry.add(CutThrough),
        registry.add(CutBy),
        registry.add(CutUncope),
        registry.add(CutList),
        registry.add(ModShorten),
        registry.add(ModCopeSkewed),
        registry.add(ModContourCut),
        registry.add(ModCornerCut),
        registry.add(PlateCreate),
        registry.add(PlateSet),
        registry.add(PlateInfo),
        registry.add(BoltPatternCreate),
        registry.add(AnchorPatternCreate),
        registry.add(HolePatternCreate),
        registry.add(StudPatternCreate),
        registry.add(PatternSet),
        registry.add(PatternInfo),
        registry.add(WeldCreate),
        registry.add(WeldSet),
        registry.add(WeldInfo),
        registry.add(WeldEdges),
        registry.add(WeldReview),
        registry.add(WeldTables),
        registry.add(FaceList),
        registry.add(ConnectionRuleSets),
        registry.add(ConnectionReportCsv),
        registry.add(ConnectionAutomate),
        registry.add(ConnectionApplyAutomation),
        registry.add(ConnectionSetRuleSet),
        registry.add(ConnectionTypeEditParameters),
        registry.add(StructuralConnectionSetType),
        registry.add(StructuralConnectionOverride),
        registry.add(ResultsPresets),
        registry.add(ResultsExtremes),
        registry.add(ResultsOverlay),
        registry.add(ResultsPackages),
        registry.add(ResultsShow),
        registry.add(ResultsApplyPreset),
        registry.add(ResultsHide),
        registry.add(ResultsSavePreset),
        registry.add(ResultsDeletePreset),
        registry.add(ResultsDeletePackage),
        registry.add(AnalyticalModelCheckSupports),
        registry.add(AnalyticalModelCheckConsistency),
        registry.add(AnalyticalModelCheck),
        registry.add(AnalyticalModelIgnoreIssue),
        registry.add(StructuralCodeSet),
        registry.add(LoadNatureCreate),
        registry.add(LoadCaseCreate),
        registry.add(LoadCaseDelete),
        registry.add(LoadCombinationCreate),
        registry.add(LoadCombinationGenerate),
        registry.add(LoadCreatePoint),
        registry.add(LoadCreateLine),
        registry.add(LoadCreateArea),
        registry.add(LoadSet),
        registry.add(LoadCaseSet),
        registry.add(LoadCombinationSet),
        registry.add(LoadCombinationDelete),
        registry.add(BoundaryConditionCreate),
        registry.add(BoundaryConditionAtColumnBases),
        registry.add(BoundaryConditionRemove),
        registry.add(LoadCaseList),
        registry.add(StructuralAnalysisRecord),
        registry.add(StructuralAnalysisResults),
        registry.add(ConnectionCreate),
        registry.add(ConnectionSet),
        registry.add(ConnectionInfo),
        registry.add(BeamSystemCreateSketch),
        registry.add(BeamSystemCreateAuto),
        registry.add(BeamSystemSetLayout),
        registry.add(BeamSystemRemove),
        registry.add(BeamSystemInfo),
        registry.add(TrussCreate),
        registry.add(TrussSet),
        registry.add(TrussEditProfile),
        registry.add(TrussAttach),
        registry.add(TrussDetach),
        registry.add(TrussRemove),
        registry.add(TrussInfo),
        registry.add(FootingPlace),
        registry.add(PilePlace),
        registry.add(PileCapPlace),
        registry.add(WallSetStructuralUsage),
        registry.add(StructuralSettingsSet),
        registry.add(StructuralSettingsGet),
        registry.add(ViewSetDetailLevel),
        registry.add(FloorSetStructural),
        registry.add(FloorSetSpanDirection),
        registry.add(FootingPlaceAtColumns),
        registry.add(FootingPlaceAtGrids),
        registry.add(WallFoundationCreate),
        registry.add(FoundationSlabCreate),
        registry.add(FoundationInfo),
        registry.add(BraceLoadSection),
        registry.add(BeamCreateOnGrids),
        registry.add(BeamSetUsage),
        registry.add(BeamSet),
        registry.add(BeamLoadSection),
        registry.add(BeamInfo),
        registry.add(FramingSetJustification),
        registry.add(FramingSetExtension),
        registry.add(FramingSetCutback),
        registry.add(FramingDisjoin),
        registry.add(FramingJoin),
        registry.add(ColumnSet),
        registry.add(ColumnAttach),
        registry.add(ColumnDetach),
        registry.add(ElementArrayLinear),
        registry.add(ElementArrayRadial),
        registry.add(CompoundGet),
        registry.add(CompoundSetLayers),
        registry.add(CurtainCreate),
        registry.add(CurtainSet),
        registry.add(CeilingSetShape),
        registry.add(RampCreate),
        registry.add(RampSet),
        registry.add(MeasureBetween),
        registry.add(MeasureChain),
        registry.add(MeasureAlong),
        registry.add(SkylightCreate),
        registry.add(SkylightSet),
        registry.add(MeshObjectInspect),
        registry.add(MeshObjectImport),
        registry.add(MeshObjectReload),
        registry.add(MeshObjectPlace),
        registry.add(MeshObjectSet),
        registry.add(MeshObjectSetType),
        registry.add(MeshObjectTypes),
        registry.add(LibraryCreate),
        registry.add(LibraryOpen),
        registry.add(LibraryList),
        registry.add(LibraryImportFiles),
        registry.add(LibraryGetMeta),
        registry.add(LibrarySetMeta),
        registry.add(LibraryDeleteItems),
        registry.add(LibraryRestoreItems),
        registry.add(ComponentPlace),
        registry.add(ArrayAlongPath),
        registry.add(ArraySet),
        registry.add(ArrayExcludeMember),
        registry.add(ArrayRestoreMember),
        registry.add(ArrayMembers),
        registry.add(ArrayUnarray),
        registry.add(RefPlaneCreate),
        registry.add(RefPlaneRename),
        registry.add(RefPlaneSet),
        registry.add(RefPlaneList),
        registry.add(WorkPlaneSet),
        registry.add(WorkPlaneGet),
        registry.add(WorkPlaneShow),
        registry.add(WorkPlaneList),
        registry.add(ElementSetWorkPlane),
        registry.add(ElementDissociateWorkPlane),
        registry.add(ModelLineCreate),
        registry.add(ModelLineSet),
        registry.add(ModelLineList),
        registry.add(ModelLineStyles),
        registry.add(DatumSetExtent),
        registry.add(DatumResetTo3d),
        registry.add(DatumMaximize3d),
        registry.add(DatumPropagateExtents),
        registry.add(DatumSetBubble),
        registry.add(DatumAddElbow),
        registry.add(DatumAssignScopeBox),
        registry.add(DatumGet),
        registry.add(ScopeBoxCreate),
        registry.add(ScopeBoxSetExtents),
        registry.add(ScopeBoxList),
        registry.add(ViewAssignScopeBox),
        registry.add(ConstraintLockDimension),
        registry.add(ConstraintUnlockDimension),
        registry.add(ConstraintSetEquality),
        registry.add(ConstraintLockAlignment),
        registry.add(ConstraintDelete),
        registry.add(ConstraintList),
        registry.add(DimTemporaryQuery),
        registry.add(DimTemporarySet),
        registry.add(DimMakePermanent),
        registry.add(ViewRevealConstraints),
        registry.add(RoofCreateExtrusion),
        registry.add(RoofSetExtrusion),
        registry.add(RoofSetSlopeArrow),
        registry.add(RoofRemoveSlopeArrow),
        registry.add(RoofJoin),
        registry.add(RoofUnjoin),
        registry.add(SlabEdgeCreate),
        registry.add(SlabEdgeSet),
        registry.add(HostSweepAddSegments),
        registry.add(HostSweepRemoveSegments),
        registry.add(HostSweepFlip),
        registry.add(ShapeAddPoint),
        registry.add(ShapeAddSplitLine),
        registry.add(ShapeMove),
        registry.add(ShapeSetElevation),
        registry.add(ShapeReset),
        registry.add(ShapeGet),
        registry.add(WallSetCrossSection),
        registry.add(StackedWallSetRows),
        registry.add(StackedWallCreateType),
        registry.add(WallBreakUp),
        registry.add(WallEditProfile),
        registry.add(WallSweepCreate),
        registry.add(CompoundSetExtendable),
        registry.add(CompoundSplitRegion),
        registry.add(CompoundSetRegions),
        registry.add(CompoundMergeRegions),
        registry.add(CompoundSetRegionAnchor),
        registry.add(CompoundAssignLayer),
        registry.add(CompoundSetWrapping),
        registry.add(WallSetExtension),
        registry.add(WallSetEndWrap),
        registry.add(WallSetLocationLine),
        registry.add(CurtainAddGrid),
        registry.add(CurtainSetGridSegments),
        registry.add(CurtainMoveGrid),
        registry.add(CurtainDeleteGrid),
        registry.add(CurtainSetPanelType),
        registry.add(CurtainSetLayout),
        registry.add(RoofCreateGlazingType),
        registry.add(PartsCreate),
        registry.add(PartsDivide),
        registry.add(PartsMerge),
        registry.add(PartsExclude),
        registry.add(PartsRestore),
        registry.add(PartsSetOverrides),
        registry.add(ViewSetPartsVisibility),
        registry.add(PartsInfo),
        registry.add(ToposolidCreate),
        registry.add(ToposolidSetPoints),
        registry.add(ToposolidSubdivide),
        registry.add(ToposolidGraded),
        registry.add(ToposolidSimplify),
        registry.add(SiteSetSettings),
        registry.add(PropertyLineCreate),
        registry.add(SiteComponentPlace),
        registry.add(ToposolidCutFill),
        registry.add(ToposolidInfo),
        registry.add(PropertyLineArea),
        registry.add(MassCreate),
        registry.add(MassAddForm),
        registry.add(MassSetForm),
        registry.add(MassSetFloors),
        registry.add(WallCreateByFace),
        registry.add(FloorCreateByMassFloor),
        registry.add(RoofCreateByFace),
        registry.add(CurtainSystemCreateByFace),
        registry.add(ElementUpdateToFace),
        registry.add(MassFloors),
        registry.add(MassFaces),
        registry.add(MassSelectDependents),
        registry.add(ModelTextCreate),
        registry.add(StairNumberTreads),
        registry.add(RoomSeparatorCreate),
        registry.add(InPlaceCreate),
        registry.add(InPlaceAddForm),
        registry.add(InPlaceRemoveForm),
        registry.add(ModelTextSet),
        registry.add(GroupCreate),
        registry.add(GroupPlace),
        registry.add(GroupUngroup),
        registry.add(GroupSwapType),
        registry.add(GroupDuplicateType),
        registry.add(GroupRenameType),
        registry.add(GroupEdit),
        registry.add(GroupAddMembers),
        registry.add(GroupRemoveMembers),
        registry.add(GroupFinish),
        registry.add(GroupCancel),
        registry.add(GroupExcludeMember),
        registry.add(GroupRestoreMember),
        registry.add(GroupMoveMemberToProject),
        registry.add(SelectionSetSave),
        registry.add(SelectionSetEdit),
        registry.add(SelectionSetDelete),
        registry.add(SelectionSetRename),
        registry.add(GroupList),
        registry.add(SelectionSetLoad),
        registry.add(SelectionSetList),
        registry.add(SelectionSetSetExport),
        registry.add(ColorSchemeCreate),
        registry.add(ColorSchemeSetRows),
        registry.add(ColorSchemeDuplicate),
        registry.add(ColorSchemeDelete),
        registry.add(ColorSchemeLegend),
        registry.add(ViewSetColorScheme),
        registry.add(AreaSchemeCreate),
        registry.add(AreaSchemeSetRules),
        registry.add(AreaSchemeList),
        registry.add(AreaPlanCreate),
        registry.add(AreaBoundaryCreate),
        registry.add(AreaPlace),
        registry.add(AreaSetType),
        registry.add(AreaList),
        registry.add(RoofSetGlazingType),
        registry.add(WallCreateBySegment),
        registry.add(WallCreateByRoom),
        registry.add(WallSetHost),
        registry.add(WallRemoveHost),
        registry.add(WallSelectMainHost),
        registry.add(GeometryJoin),
        registry.add(GeometryUnjoin),
        registry.add(WallRevealCreate),
        registry.add(WallSweepAddWalls),
        registry.add(WallSweepRemoveWalls),
        registry.add(WallSweepSet),
        registry.add(WallResetProfile),
        registry.add(WallGetProfile),
    ];
    for r in results {
        r.expect("architecture commands are valid");
    }
}

edit_command!(
    OpeningCreateWall,
    "opening.create_wall",
    "Cut a rectangular opening through a wall",
    "Cuts a plain rectangular opening through a Reviera wall, straight or curved (ARC-WALL-012, \
     ARC-OPEN-003): from `u0` to `u1` along its location curve from its start, from its base level (the \
     wall's by default) and offset to a top level and offset or a height (2.1 m by default). It goes \
     through the whole thickness, square to the curve; it follows its levels, slides along the wall, \
     moves and is copied with the wall, and goes with it. It may not overlap the wall's doors, windows or \
     other openings. Written as an IfcOpeningElement voiding the wall. Returns the opening.",
    "Wall Opening",
    crate::wall_openings::CreateParams,
    ElementId,
    crate::wall_openings::create
);

edit_command!(
    OpeningEditWall,
    "opening.edit_wall",
    "Change a wall opening",
    "Changes a wall opening's span along its wall (`u0`, `u1`) or its extent (base level and offset, top \
     constraint and offset); omitted values are kept.",
    "Edit Wall Opening",
    crate::wall_openings::EditParams,
    (),
    crate::wall_openings::edit
);

edit_command!(
    OpeningCreateShaft,
    "opening.create_shaft",
    "Make a shaft through several levels",
    "Makes a shaft (ARC-OPEN-004): closed `loops` in plan (corners), or sketch `curves`, from a base level \
     and offset to a top level and offset or a height. It cuts every Reviera floor, ceiling and footprint \
     roof its extent reaches into, plumb, including ones added later; walls are not cut. In IFC it is an \
     IfcBuildingElementProxy (PROVISIONFORVOID) with each host voided by its own IfcOpeningElement. \
     Returns the shaft.",
    "Shaft Opening",
    crate::slab_openings::CreateShaftParams,
    ElementId,
    crate::slab_openings::create_shaft
);

edit_command!(
    OpeningEditShaft,
    "opening.edit_shaft",
    "Change a shaft",
    "Changes a shaft's loops or extent (base level and offset, top constraint and offset); omitted values \
     are kept.",
    "Edit Shaft",
    crate::slab_openings::EditShaftParams,
    (),
    crate::slab_openings::edit_shaft
);

edit_command!(
    ColumnPlace,
    "column.place_architectural",
    "Place an architectural column",
    "Places a non-structural column (ARC-COL-001) of a column type (the default 400 × 400 mm type when \
     omitted) centred at `point`, rotated by `rotation`, from a base level and offset to a top level \
     and offset or a height. Written as an IfcColumn (Pset_ColumnCommon.LoadBearing false). Returns \
     the column.",
    "Column",
    crate::columns::PlaceParams,
    ElementId,
    crate::columns::place
);

edit_command!(
    ColumnPlaceStructural,
    "column.place",
    "Place a structural column",
    "Places a load-bearing column (STR-COL-001) of a structural column type (the default 400 × 400 mm \
     concrete type when omitted, made on first use) centred at `point`, rotated by `rotation`, from a \
     base level and offset to a top level and offset or a height. Structural types carry a section \
     (rectangular, round, I or H, channel, angle, tee, rectangular or round hollow), a structural \
     material and a designation. Written as an IfcColumn with Pset_ColumnCommon.LoadBearing true. \
     Returns the column.",
    "Structural Column",
    crate::columns::PlaceParams,
    ElementId,
    crate::columns::place_structural
);

edit_command!(
    ColumnPlaceAtGrids,
    "column.place_at_grids",
    "Place structural columns at grid crossings",
    "Places a structural column at every point where two of `grid_ids` cross (STR-COL-001), turned to \
     the grids' direction plus `rotation`; one undo step. Returns the columns.",
    "Columns at Grids",
    crate::columns::PlaceAtGridsParams,
    Vec<ElementId>,
    crate::columns::place_at_grids
);

edit_command!(
    ColumnPlaceSlanted,
    "column.place_slanted",
    "Place a slanted structural column",
    "Places an inclined structural column (STR-COL-002) between `p1` and `p2` (plan points), each at \
     its level and offset; the lower end is the base (the ends are swapped, with a warning, if `p1` is \
     higher). `style` end_point (the top stays at its point when levels change) or angle (the angle \
     from horizontal stays, the top moves). Ends are cut horizontally. Returns the column.",
    "Slanted Column",
    crate::columns::PlaceSlantedParams,
    ElementId,
    crate::columns::place_slanted
);

edit_command!(
    ColumnSetSlant,
    "column.set_slant",
    "Set columns' slant",
    "Makes columns vertical, slanted by end point (`top_point`), or slanted by angle (`angle` from \
     horizontal toward `azimuth`); the base stays put. Omitted values keep the column's current lean.",
    "Column Slant",
    crate::columns::SetSlantParams,
    (),
    crate::columns::set_slant
);

edit_command!(
    ColumnSet,
    "column.set",
    "Change columns",
    "Changes columns' centre, rotation, base level and offset, or top constraint and offset; omitted \
     values are kept.",
    "Edit Column",
    crate::columns::SetParams,
    (),
    crate::columns::set
);

edit_command!(
    ColumnAttach,
    "column.attach",
    "Attach columns' tops",
    "Attaches columns' tops to a Reviera roof, floor or ceiling above them (ARC-COL-001): the top \
     follows the target's underside over the column's section at its lowest point (`minimum`), middle \
     (`midline`) or highest point (`maximum`, the default), plus `offset`; with `maximum` and \
     `midline` the top is cut by the underside.",
    "Attach Column",
    crate::columns::AttachParams,
    (),
    crate::columns::attach
);

edit_command!(
    ColumnDetach,
    "column.detach",
    "Detach columns' tops",
    "Detaches columns' tops: they go back to their top constraint.",
    "Detach Column",
    crate::columns::DetachParams,
    (),
    crate::columns::detach
);

edit_command!(
    ElementArrayLinear,
    "element.array_linear",
    "Array elements in a row",
    "Copies elements into a linear array (ARC-MOD-026): `count` members including the original, the \
     `vector` reaching the second member (`move_to: second`) or the last (`move_to: last`). The members \
     are independent copies (Group and Associate is not available yet). Returns the copies, member by \
     member.",
    "Array",
    crate::modify::ArrayLinearParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_linear
);

edit_command!(
    ElementArrayRadial,
    "element.array_radial",
    "Array elements round a centre",
    "Copies elements into a radial array (ARC-MOD-026) round `center`: `count` members including the \
     original, `angle` reaching the second member (`second`) or the last (`last`; a full turn spaces \
     them evenly without doubling the original). Doors and windows cannot be rotated off their wall. \
     Returns the copies, member by member.",
    "Radial Array",
    crate::modify::ArrayRadialParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_radial
);

#[derive(Debug)]
struct CompoundGet;

impl Command for CompoundGet {
    type Params = crate::compound::GetParams;
    type Output = crate::compound::CompoundReport;
    const META: CommandMeta = CommandMeta {
        name: "compound.get",
        version: 1,
        summary: "A wall type's layers",
        description: "A wall type's compound structure (ARC-CMPD-001): its layers, exterior first (function, \
                      priority, material, thickness, wraps), its core and its total thickness. A type without \
                      layers of its own reads as one Structure layer of its width.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::compound::GetParams,
    ) -> Result<crate::compound::CompoundReport> {
        crate::compound::get(cx.store(), p)
    }
}

edit_command!(
    CompoundSetLayers,
    "compound.set_layers",
    "Set a wall type's layers",
    "Gives a wall type its layers (ARC-CMPD-001), exterior first: each a `function` (structure, substrate, \
     thermal_air, finish1, finish2, membrane), an optional `priority` (1 highest … 9; the function's \
     default), a `material` name, a `thickness` (accepts units), and `wraps`, `structural_material` and \
     `variable` flags; and the core `core_start..core_end` (by default the Structure layers). They are \
     validated (thicknesses, priorities not rising away from the core, one variable and one structural \
     layer at most). The type's width becomes their total and its walls regenerate; IFC gets an \
     IfcMaterialLayerSet.",
    "Edit Assembly",
    crate::compound::SetLayersParams,
    (),
    crate::compound::set_layers
);

edit_command!(
    CurtainCreate,
    "curtain.create",
    "Make a curtain wall",
    "Makes a curtain wall (ARC-CW-001/002) along a straight base line, from a base level and offset to a \
     top level and offset or a height, of a curtain wall type (the default Storefront when omitted: \
     vertical grid lines at most 1.5 m apart, a transom at 2.4 m, 50 × 150 mm mullions, glazed panels). \
     Drawn along a basic wall, it cuts its opening through it (ARC-WALL-013). IFC: an IfcCurtainWall \
     aggregating IfcPlate panels and IfcMember mullions. Returns the curtain wall.",
    "Curtain Wall",
    crate::curtain::CreateParams,
    ElementId,
    crate::curtain::create
);

edit_command!(
    CurtainSet,
    "curtain.set",
    "Change curtain walls",
    "Changes curtain walls' base line, base and top, and the justification (`beginning`, `center`, \
     `end`) and offset of their vertical (`u_*`) and horizontal (`v_*`) grid layouts; omitted values are \
     kept. The type's layout rules, mullions and panels are type parameters (`type.set_param`).",
    "Edit Curtain Wall",
    crate::curtain::SetParams,
    (),
    crate::curtain::set
);

edit_command!(
    CeilingSetShape,
    "ceiling.set_shape",
    "Make ceilings flat or follow the roof",
    "Makes ceilings `flat`, or `follow_roof`: shaped to the underside of the roof over them (vaulted under \
     a pitched roof, curved under an arched roof or a dome), piece by piece under its faces; what the roof \
     does not cover stays flat. The roof is `roof_id`, or by default the roof over each ceiling.",
    "Ceiling Shape",
    crate::vaults::SetShapeParams,
    (),
    crate::vaults::set_shape
);

edit_command!(
    RampCreate,
    "ramp.create",
    "Make a ramp",
    "Makes a ramp (ARC-RAMP-001) along a centreline `path` (from the bottom up; each segment a run, a \
     square landing at each corner), `width` wide (1.2 m by default), from a base level and offset to a \
     top level and offset or a height, of a ramp type (the default 1:12, 9 m maximum incline, 150 mm \
     thick). Runs longer than the maximum incline length get landings as deep as the ramp is wide. A \
     ramp steeper than its type allows is made, with a warning. Returns the ramp.",
    "Ramp",
    crate::ramps::CreateParams,
    ElementId,
    crate::ramps::create
);

edit_command!(
    RampSet,
    "ramp.set",
    "Change ramps",
    "Changes ramps' path, width, base and top; omitted values are kept. The slope limit, maximum incline \
     length, thickness and shape (thick, solid) are type parameters (`type.set_param`).",
    "Edit Ramp",
    crate::ramps::SetParams,
    (),
    crate::ramps::set
);

/// A read-only command of a pure function.
macro_rules! read_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, [$($eff:expr),*], $params:ty, $out:ty, $f:expr) => {
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
                effects: &[$($eff),*],
                transaction_name: "",
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&crate::store::Store, $params) -> Result<$out> = $f;
                f(cx.store(), p)
            }
        }
    };
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $params:ty, $out:ty, $f:expr) => {
        read_command!($ty, $name, $summary, $desc, [Effect::Read], $params, $out, $f);
    };
}

edit_command!(
    ColumnLoadSection,
    "column.load_section",
    "Load a standard section as a column type",
    "Makes a structural column type from a standard section in Reviera's catalogues (STR-COL-001), \
     for example `W12x40` (AISC) or `HEB 300` (Euronorm), with its exact dimensions, designation and \
     the family's usual steel grade as its material (or `material`). An existing type of the same \
     designation and material is reused. Returns the type.",
    "Load Section",
    crate::steel_sections::LoadParams,
    ElementId,
    crate::steel_sections::load_column_section
);

edit_command!(
    BeamCreate,
    "beam.create",
    "Place a beam",
    "Places a structural beam (STR-BEAM-001) along `curve` (plan, metres) at `reference_level`, each \
     end `start_offset` / `end_offset` above it (so it can slope). The type defaults to the region's \
     usual rolled beam (W12X26, IPE 300 or UB 305x165x40), made on first use. By default the location \
     line is the section's top centre (`z_justification` top, `y_justification` centre). Each end joins \
     what supports it (a column or wall it ends in, or a beam it frames into), stopping at its face less \
     the cutback (12.5 mm for steel). Usage `automatic` follows the supports: on columns or walls a \
     girder, on girders a joist, on joists a purlin; purlins and horizontal bracing are written as \
     IfcMember, others as IfcBeam. Returns the beam.",
    "Beam",
    crate::beams::CreateParams,
    ElementId,
    crate::beams::create
);

edit_command!(
    BeamSystemCreateSketch,
    "beam_system.create_sketch",
    "Create a beam system from a boundary",
    "Creates a beam system (STR-BSYS-001), an IfcElementAssembly (BEAM_GRID), that fills `boundary` \
     (plan corners) with parallel beams running along edge `direction_edge`, at `level` + `offset`. \
     `layout_rule`: fixed_distance (`value` the centre spacing), fixed_number (`value` beams), \
     maximum_spacing (the fewest beams keeping the spacing at or below `value`) or clear_spacing \
     (`value` between faces); `justification` beginning, end or centre for spaced rules. Edges on \
     beams or walls are locked to them, so the beams follow when the supports move. Its beams are \
     ordinary beams that join their supports (usually joists). Returns the system.",
    "Beam System",
    crate::beams::SystemSketchParams,
    ElementId,
    crate::beams::create_system_sketch
);

edit_command!(
    BeamSystemCreateAuto,
    "beam_system.create_auto",
    "Create a beam system in a bay",
    "Creates a beam system (STR-BSYS-001) in the bay around `point`: the smallest loop of beams at \
     `level` + `offset` and walls through that height, its edges locked to them, the beams parallel \
     to the support nearest the point. Layout as beam_system.create_sketch. Returns the system.",
    "Beam System",
    crate::beams::SystemAutoParams,
    ElementId,
    crate::beams::create_system_auto
);

edit_command!(
    BeamSystemSetLayout,
    "beam_system.set_layout",
    "Change a beam system's layout",
    "Changes a beam system's layout rule, value, justification or beam type (any omitted are kept) and \
     lays its beams out afresh for its bay as it is now. Returns the new beams.",
    "Beam System Layout",
    crate::beams::SetLayoutParams,
    Vec<ElementId>,
    crate::beams::set_layout
);

edit_command!(
    BeamSystemRemove,
    "beam_system.remove",
    "Remove a beam system, keeping its beams",
    "Removes a beam system (STR-BSYS-001) and keeps its beams where they are, as independent beams. \
     Deleting the system with element.delete deletes its beams too. Returns the beams kept.",
    "Remove Beam System",
    crate::beams::SystemParams,
    Vec<ElementId>,
    crate::beams::remove_system
);

edit_command!(
    TrussCreate,
    "truss.create",
    "Create a truss",
    "Creates a truss (STR-TRUSS-001) from `start` to `end` (plan) at `level` + `offset`: an \
     IfcElementAssembly (TRUSS) whose chords (IfcMember CHORD) and webs (STRUT) are laid out by \
     `layout` (pratt, howe or warren) in `panels` panels, `height` deep, the bearing chord (bottom or \
     top) on the line. The members follow the truss when it is stretched (keeping the panel count). \
     Returns the truss.",
    "Truss",
    crate::beams::TrussParams,
    ElementId,
    crate::beams::create_truss
);

edit_command!(
    TrussEditProfile,
    "truss.edit_profile",
    "Edit a truss's profile",
    "Edit Profile (STR-TRUSS-001): the chords' shapes as points (fraction along the span 0 to 1, height \
     above the truss's base in metres); the top chord must stay above the bottom one. The profile becomes \
     Custom and the members are made afresh, the chords split at each point.",
    "Edit Truss Profile",
    crate::beams::TrussProfileParams,
    (),
    crate::beams::edit_truss_profile
);

edit_command!(
    TrussAttach,
    "truss.attach",
    "Attach a truss's top chord to a roof",
    "Attaches a truss's top chord to a roof that covers it (STR-TRUSS-001): the chord follows the roof's \
     underside, less half its depth, at each panel point, and keeps following it when the roof changes.",
    "Attach Truss",
    crate::beams::TrussAttachParams,
    (),
    crate::beams::attach_truss
);

edit_command!(
    TrussDetach,
    "truss.detach",
    "Detach a truss's top chord",
    "Detaches a truss's top chord from its roof; it returns to the truss's profile.",
    "Detach Truss",
    crate::beams::TrussDetachParams,
    (),
    crate::beams::detach_truss
);

edit_command!(
    TrussSet,
    "truss.set",
    "Change a truss",
    "Changes a truss's line (it stretches, keeping its panels), height, bearing chord or offset (the \
     members follow), or its layout (pratt, howe, warren, fink, king_post, queen_post), panel count, \
     profile (flat, gable, mono, scissor, bowstring), heel height or a scissor's bottom rise (the members \
     are made afresh).",
    "Edit Truss",
    crate::beams::TrussSetParams,
    (),
    crate::beams::set_truss
);

edit_command!(
    TrussRemove,
    "truss.remove",
    "Remove a truss, keeping its members",
    "Removes a truss (STR-TRUSS-001) and keeps its members where they are, as plain framing. Deleting \
     the truss with element.delete deletes its members too. Returns the members kept.",
    "Remove Truss",
    crate::beams::TrussIdParams,
    Vec<ElementId>,
    crate::beams::remove_truss
);

edit_command!(
    ConnectionAutomate,
    "structural_connection.automate",
    "Automate connections",
    "Places library connections on many joins at once (STR-STL-004). Joins are found in the framing \
     (`element_ids` limits them to those members' joins): each column's foot where no column stands \
     below, each beam end on the column or girder it rests on, and each brace end at the column (else \
     beam) its axis meets. Each gets the type of the first rule that matches (`rule_set` by name, \
     Default when omitted, or `rules` given here); a rule without a depth range uses its type's range \
     of applicability. Only steel members are connected. A join with a connection on the same members \
     is skipped, or with `existing: replace_generic` a generic one gets the type. Where the generator \
     fails a generic connection marked Failed holds the join. With `preview` nothing changes. Returns \
     the report: a line per join with its kind, point, members, type, status (placed, skipped, failed) \
     and reason, and the counts. The run is one undo step and is logged with the project.",
    "Automate Connections",
    crate::steel_auto::AutomateParams,
    crate::steel_auto::Report,
    crate::steel_auto::automate
);

edit_command!(
    ConnectionApplyAutomation,
    "structural_connection.apply_automation",
    "Apply an automation preview",
    "Applies a preview's report from `structural_connection.automate` as it stands (STR-STL-004): \
     its placed lines are placed (skipped again where a member has gone or a detailed connection has \
     appeared since). Returns the report of what was done.",
    "Automate Connections",
    crate::steel_auto::ApplyParams,
    crate::steel_auto::Report,
    crate::steel_auto::apply_report
);

edit_command!(
    ConnectionSetRuleSet,
    "structural_connection.set_rule_set",
    "Save a connection rule set",
    "Saves one of the project's connection automation rule sets by `name` (STR-STL-004): an ordered \
     list of `rules`, each a library `type_id`, a `join` (column_base, beam_column, beam_beam, brace), \
     and optional `main_sections`, `sections` (i_shape, channel, angle, tee, rectangular_hollow, \
     circular_hollow, …), `depth_min` and `depth_max` (metres, of the connected member). Null `rules` \
     removes it. Returns how many rule sets the project has.",
    "Connection Rule Set",
    crate::steel_auto::SetRuleSetParams,
    usize,
    crate::steel_auto::set_rule_set
);

edit_command!(
    ConnectionCreate,
    "structural_connection.create",
    "Place a structural connection",
    "Places a generic structural connection (STR-CONN-001) at the join of `member_ids` (two or more \
     beams, braces, truss members or columns whose axes meet within 500 mm; the first is the main \
     member): an IfcElementAssembly (USERDEFINED, STRUCTURAL_CONNECTION) carrying the connection's \
     information. It follows its members and goes when any of them goes. Plans draw it as a circle with \
     a spoke toward each member. Returns the connection.",
    "Structural Connection",
    crate::connections::CreateParams,
    ElementId,
    crate::connections::create
);

edit_command!(
    ConnectionSet,
    "structural_connection.set",
    "Set connections' information",
    "Sets structural connections' approval status, code-check status (not_checked, passed, failed), \
     fabricator and override-by-instance flag; an empty text clears it.",
    "Connection Information",
    crate::connections::SetParams,
    (),
    crate::connections::set
);

edit_command!(
    AnalyticalMemberCreate,
    "analytical_member.create",
    "Create an analytical member",
    "Creates an analytical member (STR-AM-001), an IfcStructuralCurveMember, from `start` to `end` \
     (metres): its nodes are made there, or existing nodes within the node merge distance are used; \
     `role` beam, column, brace, member or tie; a column or beam `section_type_id`; `material`; both \
     ends' `releases` (fixed or pinned). It needs no physical element. Returns the member.",
    "Analytical Member",
    crate::analytical::CreateParams,
    ElementId,
    crate::analytical::create
);

edit_command!(
    AnalyticalMemberCreateVertical,
    "analytical_member.create_vertical",
    "Create a vertical analytical member",
    "Creates a vertical analytical member (a column) at `point` from `level` down to the level below \
     (STR-AM-001). Returns the member.",
    "Analytical Member",
    crate::analytical::VerticalParams,
    ElementId,
    crate::analytical::create_vertical
);

edit_command!(
    AnalyticalMemberSetReleases,
    "analytical_member.set_releases",
    "Set analytical members' end releases",
    "Sets members' end releases (STR-AM-001) at `end` (start, end or both): `preset` fixed or pinned, or \
     six `flags` (translations x, y, z, rotations x, y, z; true is fixed). A member pinned at both ends \
     is a PIN_JOINED_MEMBER.",
    "End Releases",
    crate::analytical::ReleaseParams,
    (),
    crate::analytical::set_releases
);

edit_command!(
    AnalyticalMemberSet,
    "analytical_member.set",
    "Change analytical members",
    "Changes analytical members' section type, material, role, Analyze As (releases, tension_only, \
     compression_only) or cross-section rotation. Physical elements change only through \
     analytical_automation.analytical_to_physical.",
    "Analytical Member",
    crate::analytical::MemberSetParams,
    (),
    crate::analytical::set_member
);

edit_command!(
    AnalyticalPanelCreateByBoundary,
    "analytical_panel.create_by_boundary",
    "Create an analytical panel by boundary",
    "A planar analytical panel (STR-AM-002) on `boundary` (corner points in metres, planar within 1 mm, \
     not crossing itself): an IfcStructuralSurfaceMember whose corners are nodes, merged with existing \
     nodes within the node merge distance so it connects to members and panels there. `thickness` \
     (default 200 mm), `material`, `role` (floor, wall or roof) and `analyze_as` (shell, membrane or \
     bending_plate). Returns the panel.",
    "Analytical Panel",
    crate::analytical_panels::BoundaryParams,
    ElementId,
    crate::analytical_panels::create_by_boundary
);

edit_command!(
    AnalyticalPanelCreateByExtrusion,
    "analytical_panel.create_by_extrusion",
    "Create an analytical panel by extrusion",
    "An analytical panel (STR-AM-002) extruded upwards by `depth` (default 3 m) from a profile on a \
     horizontal work plane at `elevation`: a `line` (a planar panel) or an `arc` from `start` through \
     `through` to `end` (a curved panel on a cylinder). Role wall by default. Returns the panel.",
    "Analytical Panel",
    crate::analytical_panels::ExtrusionParams,
    ElementId,
    crate::analytical_panels::create_by_extrusion
);

edit_command!(
    AnalyticalPanelSplit,
    "analytical_panel.split",
    "Split an analytical panel",
    "Splits a planar analytical panel along a line through two points (STR-AM-002): two panels sharing \
     new nodes where the line crosses the boundary, each with the properties, association and the \
     openings on its side. A line crossing an opening, or crossing a non-convex panel more than once, \
     is refused. Returns both panels (the original first).",
    "Split Panel",
    crate::analytical_panels::SplitParams,
    Vec<ElementId>,
    crate::analytical_panels::split
);

edit_command!(
    AnalyticalPanelSetCladding,
    "analytical_panel.set_cladding",
    "Mark an analytical panel as cladding",
    "Cladding panels (STR-AM-002) carry wind and snow loads to their supports but add no stiffness: \
     written as PredefinedType USERDEFINED with ObjectType CLADDING, and reported to analysis providers \
     with their stiffness excluded.",
    "Cladding",
    crate::analytical_panels::CladdingParams,
    (),
    crate::analytical_panels::set_cladding
);

edit_command!(
    AnalyticalPanelSet,
    "analytical_panel.set",
    "Change an analytical panel",
    "Changes an analytical panel's thickness, material, role (floor, wall, roof) or Analyze As (shell, \
     membrane, bending_plate).",
    "Analytical Panel",
    crate::analytical_panels::SetParams,
    (),
    crate::analytical_panels::set
);

edit_command!(
    AnalyticalOpeningCreate,
    "analytical_opening.create",
    "Create an analytical opening",
    "An opening in a planar analytical panel (STR-AM-002): its points are put on the panel's plane and \
     must lie inside the panel and clear of its other openings. Written as an inner bound of the \
     panel's face. Returns the panel and the opening's index.",
    "Analytical Opening",
    crate::analytical_panels::OpeningParams,
    crate::analytical_panels::OpeningOut,
    crate::analytical_panels::create_opening
);

edit_command!(
    AnalyticalOpeningRemove,
    "analytical_opening.remove",
    "Remove an analytical opening",
    "Removes opening `opening` (its index) from an analytical panel.",
    "Remove Analytical Opening",
    crate::analytical_panels::RemoveOpeningParams,
    (),
    crate::analytical_panels::remove_opening
);

edit_command!(
    AnalyticalModelReset,
    "analytical_model.reset",
    "Reset analytical elements",
    "Reset (STR-AM-008): analytical members and panels (or those of the given physical elements) back to \
     the position their physical element gives them, keeping their GlobalIds, sections, releases, loads \
     and supports, and clearing the manually adjusted flag. A node shared with elements not being reset \
     stays where it is and the reset element gets its own; those nodes are reported. Elements with no \
     physical element are refused, by name.",
    "Reset Analytical",
    crate::analytical::ResetParams,
    crate::analytical::ResetReport,
    crate::analytical::reset
);

edit_command!(
    AnalyticalNodeMove,
    "analytical_node.move",
    "Move analytical nodes",
    "Moves analytical nodes by `delta` (metres, project axes), or each by its own vector (`moves`), \
     as one undo step, and flags them as manually adjusted (STR-AM-008): the members using them move \
     with them, their loads and boundary conditions follow, and physical elements stay where they are. \
     Each node keeps its level. With `merge`, nodes then within the merge distance are merged (a node \
     dropped on another joins it). Returns the nodes.",
    "Adjust Analytical Model",
    crate::analytical::NodeMoveParams,
    Vec<ElementId>,
    crate::analytical::move_nodes
);

edit_command!(
    AnalyticalNodeMerge,
    "analytical_node.merge",
    "Merge analytical nodes",
    "Merges analytical nodes within `tolerance` (default the Structural Settings' node merge distance), \
     re-pointing their members, and removes nodes no member uses (STR-AM-003).",
    "Merge Nodes",
    Option<crate::analytical::MergeParams>,
    crate::analytical::MergeReport,
    |t, p| crate::analytical::merge(t, p.unwrap_or_default())
);

edit_command!(
    AnalyticalAssociationAdd,
    "analytical_association.add",
    "Associate physical and analytical elements",
    "Associates `physical_ids` with the analytical members `analytical_ids` (STR-AM-004). An element \
     already in an association is refused, and the association is named. Returns the members.",
    "Association",
    crate::analytical::AssociationParams,
    Vec<ElementId>,
    crate::analytical::associate
);

edit_command!(
    AnalyticalAssociationRemove,
    "analytical_association.remove",
    "Remove associations",
    "Removes the associations of the given analytical members or physical elements (all, when \
     omitted), leaving both models as they are. Returns the members disconnected.",
    "Remove Association",
    crate::analytical::DisconnectParams,
    Vec<ElementId>,
    crate::analytical::disconnect
);

edit_command!(
    AnalyticalPhysicalToAnalytical,
    "analytical_automation.physical_to_analytical",
    "Make the analytical model from the physical one",
    "Physical to analytical (STR-AM-006): an analytical member for each structural column, beam, brace \
     and truss member (`element_ids`, or every one without an association) along its axis (a column's \
     axis, a beam's location line), its nodes merged within the node merge distance, associated with it; \
     braces pinned, others fixed. Returns the report: created, updated, skipped with reasons.",
    "Physical to Analytical",
    Option<crate::analytical::AutomationParams>,
    crate::analytical::AutomationReport,
    |t, p| crate::analytical::physical_to_analytical(t, p.unwrap_or_default())
);

edit_command!(
    AnalyticalAnalyticalToPhysical,
    "analytical_automation.analytical_to_physical",
    "Update the physical model from the analytical one",
    "Analytical to physical (STR-AM-006): each associated analytical member's section type is given to \
     its physical elements where it differs (a beam type to beams, a column type to columns), for \
     example a size changed by analysis. Returns the report.",
    "Analytical to Physical",
    Option<crate::analytical::AutomationParams>,
    crate::analytical::AutomationReport,
    |t, p| crate::analytical::analytical_to_physical(t, p.unwrap_or_default())
);

edit_command!(
    AnalyticalDisconnect,
    "analytical_automation.disconnect",
    "Disconnect the analytical model",
    "Removes associations between the analytical and physical models (of the given elements, or all), \
     leaving both in place (STR-AM-006). Returns the members disconnected.",
    "Disconnect",
    crate::analytical::DisconnectParams,
    Vec<ElementId>,
    crate::analytical::disconnect
);

edit_command!(
    StructuralCodeSet,
    "structural_code.set",
    "Choose the load code template",
    "Chooses the code template for load natures and combinations (STR-LC-001): `asce7` (ASCE/SEI 7-22 \
     strength design) or `en1990` (EN 1990 expression 6.10 with the recommended factors). Its natures \
     replace the code natures; user natures stay.",
    "Load Code",
    crate::loads::CodeParams,
    (),
    crate::loads::set_code
);

edit_command!(
    LoadNatureCreate,
    "load_nature.create",
    "Add a load nature",
    "Adds a user load nature: name, symbol, action type (permanent, variable, extraordinary) and IFC \
     action source (DEAD_LOAD_G, LIVE_LOAD_Q, SNOW_S, WIND_W, …).",
    "Load Nature",
    crate::loads::NatureParams,
    (),
    crate::loads::create_nature
);

edit_command!(
    LoadCaseCreate,
    "load_case.create",
    "Create a load case",
    "Creates a load case (STR-LC-001), an IfcStructuralLoadCase of a nature (name or symbol), with a \
     category and a self-weight flag. Names are unique and contain no *, + or -. Returns the case.",
    "Load Case",
    crate::loads::CaseParams,
    ElementId,
    crate::loads::create_case
);

edit_command!(
    LoadCaseDelete,
    "load_case.delete",
    "Delete a load case",
    "Deletes a load case: its loads go with it, and it is taken out of the combinations that use it. \
     Returns how many loads and combinations were affected.",
    "Delete Load Case",
    crate::loads::DeleteCaseParams,
    crate::loads::CaseDeletion,
    crate::loads::delete_case
);

edit_command!(
    LoadCombinationCreate,
    "load_combination.create",
    "Create a load combination",
    "Creates a load combination (STR-LC-001), an IfcStructuralLoadGroup (LOAD_COMBINATION): a `formula` \
     such as `1.35*G1 + 1.5*Q1` of cases and other combinations (expanded; circular references refused), \
     a `limit_state` (ultimate, serviceability, quasi_permanent, frequent, rare, accidental, special) \
     and `usages`. Returns the combination.",
    "Load Combination",
    crate::loads::CombinationParams,
    ElementId,
    crate::loads::create_combination
);

edit_command!(
    LoadCombinationGenerate,
    "load_combination.generate",
    "Generate load combinations",
    "Generates combinations offline from a code template (default the project's): ASCE 7-22 §2.3 \
     strength combinations over the cases by nature (alternatives such as Lr, S or R one each), or EN \
     1990 expression 6.10 with each variable nature leading in turn plus the characteristic and \
     quasi-permanent SLS combinations. Cases of unknown natures are flagged. `add` creates them.",
    "Generate Combinations",
    Option<crate::loads::GenerateParams>,
    crate::loads::Generated,
    |t, p| crate::loads::generate(t, p.unwrap_or_default())
);

edit_command!(
    LoadCreatePoint,
    "load.create_point",
    "Place a point load",
    "Places a point load (STR-LOAD-001), an IfcStructuralPointAction, on an analytical node or at \
     `position` (0 to 1, default mid-span) along a member, in a case: Fx, Fy, Fz (N), Mx, My, Mz (N·m) \
     in project axes (gravity is negative Fz). It goes with its host or case. Returns the load.",
    "Point Load",
    crate::loads::PointParams,
    ElementId,
    crate::loads::create_point
);

edit_command!(
    LoadCreateArea,
    "load.create_area",
    "Place an area load",
    "Places a uniform area load (STR-LOAD-001) on an analytical panel: `value` Fx, Fy, Fz in N/m², in \
     project axes (gravity is negative z) or the panel's local axes (z its normal), on its true or \
     projected area. An IfcStructuralPlanarAction with an IfcStructuralLoadPlanarForce, its host by \
     IfcRelConnectsStructuralActivity, its case by IfcRelAssignsToGroup. For analysis it is carried to \
     the members lying on the panel by their tributary widths across its span. Returns the load.",
    "Area Load",
    crate::loads::AreaParams,
    ElementId,
    crate::loads::create_area
);

edit_command!(
    LoadSet,
    "load.set",
    "Change a load",
    "Changes a point, line or area load: its case (its nature follows), values (point `forces` and \
     `position`, line `start` and `end`, area `value`), `orientation` (project or host_local), \
     `projected` and `is_reaction`.",
    "Load",
    crate::loads::SetLoadParams,
    (),
    crate::loads::set_load
);

edit_command!(
    LoadCaseSet,
    "load_case.set",
    "Change a load case",
    "Changes a load case's name (combination formulas that use it follow), nature (its IFC action type \
     and source follow), category or self-weight flag.",
    "Load Case",
    crate::loads::SetCaseParams,
    (),
    crate::loads::set_case
);

edit_command!(
    LoadCombinationSet,
    "load_combination.set",
    "Change a load combination",
    "Changes a load combination's name (formulas that use it follow), formula (checked as on creation: \
     known names, numeric factors, not circular), limit state or usages (tags such as Gravity, Lateral, \
     Combined).",
    "Load Combination",
    crate::loads::SetCombinationParams,
    (),
    crate::loads::set_combination
);

edit_command!(
    LoadCombinationDelete,
    "load_combination.delete",
    "Delete a load combination",
    "Deletes a load combination; one another combination's formula uses is refused, naming it.",
    "Delete Load Combination",
    crate::loads::DeleteCombinationParams,
    (),
    crate::loads::delete_combination
);

edit_command!(
    LoadCreateLine,
    "load.create_line",
    "Place a line load",
    "Places a line load (STR-LOAD-001), an IfcStructuralCurveAction, along an analytical member in a \
     case: force per length (N/m, project axes) at the start and, for a linear load, the end; \
     `projected` for the projected length. Returns the load.",
    "Line Load",
    crate::loads::LineParams,
    ElementId,
    crate::loads::create_line
);

edit_command!(
    BoundaryConditionCreate,
    "boundary_condition.create",
    "Place a boundary condition",
    "Places a point boundary condition (STR-BC-001) at a node, or a member's start or end: Fixed, \
     Pinned, Roller (vertical translation only) or User (six dof, each fixed, released or a spring in \
     N/m or N·m/rad). Returns the node.",
    "Boundary Condition",
    crate::loads::BoundaryParams,
    ElementId,
    crate::loads::create_boundary
);

edit_command!(
    BoundaryConditionAtColumnBases,
    "boundary_condition.at_column_bases",
    "Support the analytical columns' feet",
    "Places a point boundary condition (default pinned) at the foot of every analytical column that no \
     member reaches from below. Returns the nodes.",
    "Column Base Supports",
    crate::loads::ColumnBasesParams,
    Vec<ElementId>,
    crate::loads::at_column_bases
);

edit_command!(
    BoundaryConditionRemove,
    "boundary_condition.remove",
    "Remove boundary conditions",
    "Removes the point boundary conditions of the given nodes (or members' start nodes).",
    "Remove Boundary Condition",
    crate::loads::RemoveBoundaryParams,
    (),
    crate::loads::remove_boundary
);

edit_command!(
    AnalyticalModelIgnoreIssue,
    "analytical_model.ignore_issue",
    "Ignore an analytical check issue",
    "Ignores (or with `ignore: false` un-ignores) an analytical check issue by its key (STR-AM-007), \
     with an optional reason. The ignore survives re-checks and saves, and lapses when any element the \
     issue involves changes geometry.",
    "Ignore Issue",
    crate::analytical_checks::IgnoreParams,
    (),
    crate::analytical_checks::ignore_issue
);

edit_command!(
    ResultsShow,
    "results.show",
    "Show results in a view",
    "Shows analysis results in a view (STR-RES-001): `result_type` (axial, shear_y, shear_z, torsion, \
     moment_y, moment_z, deformed, reactions) of `load_group` (a case or combination by name; default the \
     view's current one, else the first), `scale` (a multiplier on the automatic scale), `deformation` \
     (the deformed shape under a force diagram), `filter` (all, columns, beams, braces, transfer_beams) and \
     `labels`. Kept with the view; results never change the model.",
    "Show Results",
    crate::results::ShowParams,
    crate::results::Display,
    crate::results::show
);

edit_command!(
    ResultsApplyPreset,
    "results.apply_preset",
    "Show a results preset in a view",
    "Shows a preset (`results.presets`) in a view, for `load_group` (default the view's current one). The \
     panel presets are refused with the reason.",
    "Results Preset",
    crate::results::PresetParams,
    crate::results::Display,
    crate::results::apply_preset
);

edit_command!(
    ResultsHide,
    "results.hide",
    "Hide results in a view",
    "Stops showing analysis results in a view.",
    "Hide Results",
    crate::results::HideParams,
    (),
    crate::results::hide
);

edit_command!(
    ResultsSavePreset,
    "results.save_preset",
    "Save a results preset",
    "Saves the display a view shows as a project preset named `name` (its load case is not kept). Returns \
     the preset's id.",
    "Save Results Preset",
    crate::results::SavePresetParams,
    String,
    crate::results::save_preset
);

edit_command!(
    ResultsDeletePreset,
    "results.delete_preset",
    "Delete a results preset",
    "Deletes a project results preset; built-in presets cannot be deleted.",
    "Delete Results Preset",
    crate::results::DeletePresetParams,
    (),
    crate::results::delete_preset
);

edit_command!(
    ResultsDeletePackage,
    "results.delete_package",
    "Delete a result set",
    "Deletes the analysis results kept with the model (the Results Manager); views showing them stop.",
    "Delete Results",
    crate::results::DeletePackageParams,
    (),
    crate::results::delete_package
);

edit_command!(
    ConnectionTypeEditParameters,
    "connection_type.edit_parameters",
    "Change a connection type's parameters",
    "Changes a steel connection type's parameters for this project (metres or counts; null returns one to \
     the region's default); every connection of the type is made afresh. Returns how many.",
    "Connection Type Parameters",
    crate::steel_fab::EditTypeParams,
    usize,
    crate::steel_fab::edit_type
);

edit_command!(
    StructuralConnectionSetType,
    "structural_connection.set_type",
    "Give connections a type",
    "Gives connections a steel connection type (or `generic`): their members must fit the type's input \
     signature (count and roles); its plates, bolts, anchor rods and welds are made as fabrication \
     elements (IfcPlate, IfcMechanicalFastener, IfcFastener) aggregated by the connection.",
    "Connection Type",
    crate::steel_fab::SetTypeParams,
    Vec<ElementId>,
    crate::steel_fab::set_type
);

edit_command!(
    StructuralConnectionOverride,
    "structural_connection.override",
    "Override a connection's parameters",
    "Gives one connection its own parameter values (null returns one to the type's); its fabrication \
     elements are made afresh and it is flagged as overridden.",
    "Connection Parameters",
    crate::steel_fab::OverrideParams,
    (),
    crate::steel_fab::override_params
);

edit_command!(
    StructuralAnalysisRecord,
    "structural_analysis.record",
    "Keep an analysis's results",
    "Keeps a finished structural analysis's results with the model (ANA-FEA-001), keys mapped back to \
     GlobalIds; `structural_analysis.collect` calls it. The previous results are replaced.",
    "Analysis Results",
    crate::structural_model::RecordParams,
    (),
    crate::structural_model::record
);

edit_command!(
    BraceCreate,
    "brace.create",
    "Place a brace",
    "Places a brace (STR-BRACE-001), an IfcMember (BRACE), from `start_point` at `start_level` + \
     `start_offset` to `end_point` at `end_level` + `end_offset`. An end on a column's or beam's axis \
     is attached to it at its ratio along it, and follows it when it moves or changes level (`attach` \
     false: free ends). The type defaults to the region's usual brace section (HSS6X6X3/8, HEA 160 or \
     UC 152x152x23). Usage: horizontal bracing when both ends are level, otherwise other. Ends join and \
     cut back like beams' (the framing.* commands). Returns the brace.",
    "Brace",
    crate::beams::BraceParams,
    ElementId,
    crate::beams::create_brace
);

edit_command!(
    StructuralSettingsSet,
    "structural_settings.set",
    "Change structural settings",
    "Changes the project's structural settings (STR-SET-001): `key` and `value`, or several `values`, \
     with the dialog's rules (offsets and cutbacks 0–100 mm on paper; analytical tolerances above 0 \
     and at most 2000 mm, the node merge distance below the support distance; spacing and load scales \
     above 0). An error names the key and the allowed range. Keys: brace_symbol (parallel_line or \
     none), brace_offset, kicker_symbol, coarse_cutback, point_to_point_steel (steel members are not \
     shortened where they join), member_supports_check, consistency_check, circular_references_check, \
     support_distance, analytical_distance, node_merge_distance, alignment_distance, boundary_spacing, \
     force_scale, linear_force_scale, area_force_scale, moment_scale, label_loads.",
    "Structural Settings",
    crate::structural::SetSettingsParams,
    (),
    crate::structural::set_settings
);

edit_command!(
    ViewSetDetailLevel,
    "view.set_detail_level",
    "Set a view's detail level",
    "Sets a view's detail level (DOC-GFX-010): coarse, medium or fine. In Coarse plans structural \
     framing is drawn as single lines, braces with their Structural Settings symbol (STR-SET-001).",
    "Detail Level",
    crate::views::SetDetailLevelParams,
    (),
    crate::views::set_detail_level
);

edit_command!(
    WallSetStructuralUsage,
    "wall.set_structural_usage",
    "Set walls' structural usage",
    "Sets walls' structural usage (STR-WALL-001): bearing, shear or structural_combined make them \
     structural walls (IfcWall SHEAR for shear, SOLIDWALL otherwise, Pset_WallCommon.LoadBearing true, \
     the exact usage kept in RXD); non_bearing makes them ordinary walls again. `wall.create` takes \
     `structural` and `structural_usage` too.",
    "Structural Usage",
    crate::structural::WallUsageParams,
    (),
    crate::structural::set_wall_usage
);

edit_command!(
    FloorSetStructural,
    "floor.set_structural",
    "Make floors structural",
    "Makes floors structural (STR-FLOOR-001: Pset_SlabCommon.LoadBearing true, a span direction along \
     the boundary's first edge unless one is set) or not.",
    "Structural Floor",
    crate::structural::FloorStructuralParams,
    (),
    crate::structural::set_floor_structural
);

edit_command!(
    FloorSetSpanDirection,
    "floor.set_span_direction",
    "Set a floor's span direction",
    "Sets a structural floor's span direction (STR-FLOOR-001) along one of its outline's edges \
     (`edge_index`) or a plan `vector`; a floor that is not structural becomes structural.",
    "Span Direction",
    crate::structural::SpanParams,
    (),
    crate::structural::set_span
);

edit_command!(
    PilePlace,
    "pile.place",
    "Place a pile",
    "Places a single pile (STR-FND-004), an IfcPile (BORED or DRIVEN by its type), its top centred at \
     `point` at `level` + `offset`, running down its `length` (default its type's). The default type is \
     a 450 mm bored pile 12 m long (18\", 40 ft in US models). Returns the pile.",
    "Pile",
    crate::foundations::PileParams,
    ElementId,
    crate::foundations::place_pile
);

edit_command!(
    PileCapPlace,
    "pile_cap.place",
    "Place a pile cap with its piles",
    "Places a pile cap (STR-FND-004): an IfcFooting (PILE_CAP) of a pile cap type, its top at `level` + \
     `offset`, with its type's grid of piles laid out under it (IfcPile, aggregated by the cap in IFC). \
     The piles follow the cap and go with it. The default type has four piles at three diameters. \
     Returns the cap.",
    "Pile Cap",
    crate::foundations::PlaceParams,
    ElementId,
    crate::foundations::place_pile_cap
);

edit_command!(
    FootingPlace,
    "footing.place",
    "Place an isolated footing",
    "Places an isolated footing (STR-FND-001), an IfcFooting (PAD_FOOTING), centred at `point`, its top \
     at `level` + `offset` and its thickness below, turned by `rotation`. The type defaults to \
     1800 × 1800 × 500 mm (6'-0\" × 6'-0\" × 1'-6\" in US models) in the region's concrete. Returns the \
     footing.",
    "Isolated Footing",
    crate::foundations::PlaceParams,
    ElementId,
    crate::foundations::place
);

edit_command!(
    FootingPlaceAtColumns,
    "footing.place_at_columns",
    "Place footings under columns",
    "Places an isolated footing under each of `column_ids` (STR-FND-001), joined to it: centred under it, \
     turned with it, its top at the column's base, following the column (and staying where it was if \
     the column is deleted). Columns that already have one are skipped; one undo step.",
    "Footings at Columns",
    crate::foundations::AtColumnsParams,
    Vec<ElementId>,
    crate::foundations::place_at_columns
);

edit_command!(
    FootingPlaceAtGrids,
    "footing.place_at_grids",
    "Place footings at grid crossings",
    "Places an isolated footing at every crossing of `grid_ids` (STR-FND-001), turned to the grids, its \
     top at `level` + `offset`; one undo step.",
    "Footings at Grids",
    crate::foundations::AtGridsParams,
    Vec<ElementId>,
    crate::foundations::place_at_grids
);

edit_command!(
    WallFoundationCreate,
    "wall_foundation.create",
    "Place wall foundations",
    "Places a wall foundation (STR-FND-002), an IfcFooting (STRIP_FOOTING), under each of `wall_ids`: \
     along the wall's length plus the type's end extension, its top at the wall's base. Bearing types \
     are a width centred on the wall; retaining types a toe beyond the exterior face and a heel beyond \
     the interior face. It follows the wall and is deleted with it. The default type is a bearing \
     footing 600 × 300 mm (24\" × 12\"). Walls that already have one are skipped.",
    "Wall Foundation",
    crate::foundations::WallFoundationParams,
    Vec<ElementId>,
    crate::foundations::create_wall_foundations
);

edit_command!(
    FoundationSlabCreate,
    "foundation_slab.create",
    "Create a foundation slab",
    "Creates a foundation slab (STR-FND-003): an IfcSlab (BASESLAB) bounded by `boundary` (outer loops \
     the slab, inner loops openings), its top at `level` + `offset` and its thickness below. The type \
     defaults to a 300 mm (12\") foundation slab type. Returns the slab.",
    "Foundation Slab",
    crate::foundations::SlabParams,
    ElementId,
    crate::foundations::create_slab
);

edit_command!(
    BraceLoadSection,
    "brace.load_section",
    "Load a standard section as a brace type",
    "Makes a brace type (IfcMemberType, BRACE) from a standard section in Reviera's catalogues \
     (STR-SEC-001), for example `HSS6X6X3/8` or `L4X4X3/8`, with its usual steel grade (or `material`). \
     Returns the type.",
    "Load Brace Section",
    crate::steel_sections::LoadParams,
    ElementId,
    crate::beams::load_brace_section
);

edit_command!(
    BeamCreateOnGrids,
    "beam.create_on_grids",
    "Place beams along grid lines",
    "Along each of `grid_ids`, places a beam in every bay between consecutive columns on the line that \
     reach `level` (STR-BEAM-001), their tops at the level plus `offset`; one undo step. Returns the \
     beams.",
    "Beams on Grids",
    crate::beams::OnGridsParams,
    Vec<ElementId>,
    crate::beams::create_on_grids
);

edit_command!(
    BeamSetUsage,
    "beam.set_usage",
    "Set beams' structural usage",
    "Sets and locks beams' structural usage (girder, joist, purlin, horizontal_bracing, other), or \
     `automatic` to follow their supports again. The IFC PredefinedType and ObjectType follow; the IFC \
     class (IfcBeam or IfcMember) is the one the beam was made with.",
    "Beam Usage",
    crate::beams::SetUsageParams,
    (),
    crate::beams::set_usage
);

edit_command!(
    BeamSet,
    "beam.set",
    "Change beams",
    "Changes beams' reference level (keeping their ends in place), end offsets, cross-section rotation, \
     orientation (normal or horizontal), y and z justification and offsets, extensions or usage; \
     omitted values are kept.",
    "Edit Beam",
    crate::beams::SetParams,
    (),
    crate::beams::set
);

edit_command!(
    BeamLoadSection,
    "beam.load_section",
    "Load a standard section as a beam type",
    "Makes a beam type from a standard section in Reviera's catalogues (STR-SEC-001), for example \
     `W12X26` (AISC) or `IPE 300` (Euronorm), with its dimensions, designation and usual steel grade \
     (or `material`). An existing type of the same designation and material is reused. Returns the \
     type.",
    "Load Beam Section",
    crate::steel_sections::LoadParams,
    ElementId,
    crate::beams::load_section
);

edit_command!(
    FramingSetJustification,
    "framing.set_justification",
    "Set framing justification",
    "Sets where beams' location lines run across their sections (STR-BEAM-002): `y_just` origin, \
     left, centre or right with `y_offset` (positive to the left), and `z_just` origin, top, centre or \
     bottom with `z_offset` (positive up).",
    "Justification",
    crate::beams::JustificationParams,
    (),
    crate::beams::set_justification
);

edit_command!(
    FramingSetExtension,
    "framing.set_extension",
    "Extend a framing end",
    "Lengthens (or, negative, shortens) a beam's `end` (start or end) beyond its join by `length` \
     (STR-BEAM-002); the location line stays.",
    "Extension",
    crate::beams::EndLengthParams,
    (),
    crate::beams::set_extension
);

edit_command!(
    FramingSetCutback,
    "framing.set_cutback",
    "Set a framing end's cutback",
    "Sets the gap a beam's joined `end` leaves at its support's face (STR-BEAM-002); by default \
     12.5 mm for steel, none for others.",
    "Cutback",
    crate::beams::EndLengthParams,
    (),
    crate::beams::set_cutback
);

edit_command!(
    FramingDisjoin,
    "framing.disjoin",
    "Disjoin a framing end",
    "Releases a beam's `end` from its join (STR-BEAM-002): it runs to its location line's end, with no \
     cutback. `framing.join` joins it again.",
    "Disjoin",
    crate::beams::EndParams,
    (),
    crate::beams::disjoin
);

edit_command!(
    FramingJoin,
    "framing.join",
    "Join a framing end again",
    "Joins a disjoined beam `end` to its support again (STR-BEAM-002).",
    "Join",
    crate::beams::EndParams,
    (),
    crate::beams::join
);

read_command!(
    ColumnCheck,
    "column.check",
    "Slenderness advisories for structural columns",
    "For each structural column (or `column_ids`), its height over its section's least radius of \
     gyration (K = 1), and the public guidance for its material and the model's region it exceeds: \
     steel L/r preferably not over 200 (AISC 360 user note); concrete k·lu/r over 22 (ACI 318, sway \
     frames) in US and Canadian models; under the Eurocodes λ is reported, its limit depending on the \
     axial load. Advisories for modelling, not design checks.",
    Option<crate::columns::CheckParams>,
    Vec<crate::columns::Advisory>,
    |s, p| crate::columns::check(s, &p.unwrap_or_default())
);

read_command!(
    StructuralStandards,
    "structural.standards",
    "The model's structural defaults and standards",
    "From the model's building code (Model Properties): its region, the section catalogues offered first, \
     the default steel and concrete, and the design standards that apply there (loads, steel, concrete, \
     timber). The standards are named for reference; design checks need an analysis provider.",
    Option<serde_json::Value>,
    crate::codes::StructuralDefaults,
    |s, _| Ok(crate::codes::settings(s).code.structural())
);

read_command!(
    StructuralCatalogs,
    "structural.catalogs",
    "List standard section catalogues",
    "The standard section catalogues built into Reviera (STR-COL-001): each one's family, standard, \
     shape, usual steel grade and designations. Their numbers come from public sources recorded in the \
     research log.",
    Option<crate::steel_sections::ListParams>,
    Vec<crate::steel_sections::CatalogInfo>,
    |s, p| Ok(crate::steel_sections::list(s, &p.unwrap_or_default()))
);

read_command!(
    StructuralMaterials,
    "structural.materials",
    "List structural materials",
    "The structural materials library (STR-SEC-001): steel grades (fy, fu, E), concrete classes (fck, \
     fctm, Ecm) and timber grades and strength classes (bending, compression, E, density), with their \
     standards and public sources; the model's region's first (`preferred`). Values in MPa and kg/m³, \
     before design factors. Types name their material by `name`.",
    Option<crate::materials_library::ListParams>,
    Vec<crate::materials_library::Listed>,
    |s, p| Ok(crate::materials_library::list(s, &p.unwrap_or_default()))
);

read_command!(
    ColumnLocationMark,
    "column.location_mark",
    "Columns' location marks",
    "Each column's location mark (STR-COL-001): the grid crossing nearest it, as its lines' names \
     (\"C-4\"), with the offset in millimetres when it is off the crossing. Computed, never stored.",
    crate::columns::MarkParams,
    Vec<crate::columns::Mark>,
    |s, p| crate::columns::location_marks(s, &p)
);

read_command!(
    FoundationInfo,
    "foundation.info",
    "Footings' and wall foundations' elevations and sizes",
    "For each footing or wall foundation: its top, its Elevation at Bottom, its plan area and volume, and \
     the column or wall it is joined to.",
    crate::foundations::InfoParams,
    Vec<crate::foundations::Info>,
    |s, p| crate::foundations::info(s, &p)
);

read_command!(
    BeamSystemInfo,
    "beam_system.info",
    "A beam system's bay, layout and beams",
    "A beam system's boundary as it is now (edges on their supports), beam direction, layout rule and \
     value, its beams, and the centre spacing between its layout lines.",
    crate::beams::SystemParams,
    crate::beams::SystemInfo,
    |s, p| crate::beams::system_info(s, &p)
);

read_command!(
    TrussInfo,
    "truss.info",
    "A truss's layout and members",
    "A truss's layout, panels, height and span, and its chords and webs.",
    crate::beams::TrussIdParams,
    crate::beams::TrussInfo,
    |s, p| crate::beams::truss_info(s, &p)
);

read_command!(
    StructuralSettingsGet,
    "structural_settings.get",
    "The structural settings",
    "The project's structural settings (STR-SET-001): symbolic representation (brace and kicker \
     symbols, brace offset and coarse cutback in paper millimetres), point-to-point steel, analytical \
     checks and tolerances (metres), boundary condition spacing and load display scales.",
    Option<serde_json::Value>,
    crate::structural::Settings,
    |s, _| Ok(crate::structural::settings(s))
);

read_command!(
    ConnectionInfo,
    "structural_connection.info",
    "Structural connections' information",
    "For each connection: its type, join point, members (main first), approval status, code-check \
     status, fabricator and override flag.",
    crate::connections::InfoParams,
    Vec<crate::connections::Info>,
    |s, p| crate::connections::info(s, &p)
);

read_command!(
    AnalyticalModelInfo,
    "analytical_model.info",
    "The analytical model",
    "The analytical model's members (role, PredefinedType, ends, nodes, section type, material, releases, \
     associated physical elements), its node count, and the nodes only one member uses.",
    Option<serde_json::Value>,
    crate::analytical::ModelInfo,
    |s, _| Ok(crate::analytical::info(s))
);

read_command!(
    AnalyticalModelCheckSupports,
    "analytical_model.check_supports",
    "Check the analytical model's supports",
    "The support check (STR-AM-007): S1 members not supported at an end that needs it (a column's \
     lower end, both ends of a beam or brace) within the support distance; S2 groups of members that \
     carry one another without reaching a boundary condition (when circular references are checked); \
     S3 no boundary condition at all. `scope` is \"model\" (default) or a list of elements. Read-only: \
     returns `{run_at, tolerances, issues: [{key, code, severity, message, element_ids, measured?, \
     tolerance?, min?, max?, ignored}], counts_by_severity}`.",
    Option<crate::analytical_checks::CheckParams>,
    crate::analytical_checks::Report,
    |s, p| crate::analytical_checks::run(s, true, false, &p.unwrap_or_default().scope)
);

read_command!(
    AnalyticalModelCheckConsistency,
    "analytical_model.check_consistency",
    "Check the analytical model's consistency",
    "The consistency check (STR-AM-007), with the criteria turned on in Structural Settings: C1 \
     disconnected nodes (a gap above the merge distance, an end on another member's span with no node \
     there, free ends) and loads off their host; C2 isolated parts with no boundary condition (error); \
     C3 analytical members away from their physical element's axis beyond the analytical-to-physical \
     distance, or with a different section type or material; C4 members leaving the physical body; C5 \
     elements without a counterpart; C7 instability hints; C8 missing section or material. Read-only; \
     the same report as `analytical_model.check_supports`.",
    Option<crate::analytical_checks::CheckParams>,
    crate::analytical_checks::Report,
    |s, p| crate::analytical_checks::run(s, false, true, &p.unwrap_or_default().scope)
);

read_command!(
    AnalyticalModelCheck,
    "analytical_model.check",
    "Check the analytical model",
    "Both the support and the consistency checks (STR-AM-007) in one report, as Run Analysis runs \
     them first.",
    Option<crate::analytical_checks::CheckParams>,
    crate::analytical_checks::Report,
    |s, p| crate::analytical_checks::run(s, true, true, &p.unwrap_or_default().scope)
);

read_command!(
    ResultsPresets,
    "results.presets",
    "Results presets",
    "The Results Explorer's presets (STR-RES-001): the built-in ones (column axial forces, column bending \
     moments, column shear, support reactions, beam moments with deformation, beam shear, beam torsion, \
     transfer beam forces, brace axial forces, deformed shape, and the panel presets, which need panel \
     results and say so), then the project's own. Each has its display: result type, filter, scale, \
     deformation and labels.",
    Option<serde_json::Value>,
    Vec<crate::results::Preset>,
    |s, _| Ok(crate::results::presets(s))
);

read_command!(
    ResultsExtremes,
    "results.extremes",
    "Results extremes",
    "The smallest and largest value of a result type (axial, shear_y, shear_z, torsion, moment_y, moment_z, \
     deformed, reactions) in a load case or combination (default the first), optionally for columns, \
     beams, braces or transfer beams only, with the analytical member (or support node), its physical \
     elements, where along it and the point; and the 20 members with the largest absolute values. In kN, \
     kN·m or mm.",
    Option<crate::results::ExtremesParams>,
    crate::results::Extremes,
    |s, p| crate::results::extremes(s, &p.unwrap_or_default())
);

read_command!(
    ResultsOverlay,
    "results.overlay",
    "A results overlay",
    "What a view showing results draws (or what a given display would): a force diagram's outline and \
     hatching off each member in its local plane, positive and negative apart; the deformed members; or \
     reaction arrows at the supports; with value labels, the title, the extremes and whether the results \
     are stale. Metres, model coordinates.",
    Option<crate::results::OverlayParams>,
    crate::results::Overlay,
    |s, p| crate::results::overlay_of(s, &p.unwrap_or_default())
);

read_command!(
    ResultsPackages,
    "results.packages",
    "Result sets",
    "The Results Manager's list (STR-RES-001): the result set kept with the model, its job, time, \
     provider, whether the model changed since (stale), its load cases and combinations, members and \
     warnings.",
    Option<serde_json::Value>,
    Vec<crate::results::Package>,
    |s, _| Ok(crate::results::packages(s))
);

read_command!(
    ConnectionTypeList,
    "connection_type.list",
    "The steel connection library",
    "The built-in steel connection types (STR-STL-002), authored from first principles: shear plate \
     (fin plate), extended end plate, pinned base plate and brace gusset plate. Each has its group, \
     input signature (members and roles), parameters (key, label, length or count, tab, metric and \
     US defaults) with this project's values, and how many connections use it.",
    Option<serde_json::Value>,
    Vec<crate::steel_fab::TypeInfo>,
    |s, _| Ok(crate::steel_fab::list(s))
);

read_command!(
    ConnectionRuleSets,
    "structural_connection.rule_sets",
    "Connection automation rule sets",
    "The rule sets connection automation can use (STR-STL-004): the built-in Default (base plates \
     under columns, shear plates where beams meet columns or girders, gussets at braces' ends) and \
     the project's own, each an ordered list of rules (a type, the join kind: column_base, \
     beam_column, beam_beam or brace, optional main and connected sections and a depth range); the \
     library types' ranges of applicability (connected member depth, metres); and the runs so far.",
    Option<serde_json::Value>,
    crate::steel_auto::RulesInfo,
    |s, _| Ok(crate::steel_auto::rules_info(s))
);

read_command!(
    ConnectionReportCsv,
    "structural_connection.automation_csv",
    "An automation report as CSV",
    "An automation report (from `structural_connection.automate` or `apply_automation`) as CSV: one \
     row per join with its kind, point, members, type, status (placed, skipped, failed), reason and \
     connection.",
    crate::steel_auto::CsvParams,
    serde_json::Value,
    |_, p| Ok(crate::steel_auto::report_csv(p))
);

edit_command!(
    PlateCreate,
    "plate.create",
    "Place a steel plate",
    "Places a plate (STR-STL-005): a single closed `contour` (corners, or `segments` of lines and \
     arcs through a middle point, which must join within 0.1 mm and not cross) in the coordinates \
     of a work plane (`level` with `offset`, `grid`, `ref_plane`, an explicit `plane`) or of a \
     `face` of a column, beam, brace or plate (`{element, face, offset}`; faces from `face.list`). \
     `thickness` 1–200 mm (default 10 mm); `justification` bottom (the default: the plate on the \
     normal's side, outside a face), centre or top; `material`, `coating` and `role` (stiffener, \
     cap plate, …). A plate on a face moves with its element and stays where it was, unhosted, if \
     the element is deleted. IfcPlate (SHEET), drawn at Fine detail. Returns the plate.",
    "Plate",
    crate::steel_detail::PlateCreateParams,
    ElementId,
    crate::steel_detail::plate_create
);

edit_command!(
    PlateSet,
    "plate.set",
    "Edit plates",
    "Changes plates' `contour` (Edit Contour), `thickness`, `justification`, `material`, `coating` \
     or `role` (STR-STL-005).",
    "Edit Plate",
    crate::steel_detail::PlateSetParams,
    (),
    crate::steel_detail::plate_set
);

read_command!(
    PlateInfo,
    "plate.info",
    "Plates' contour and placement",
    "For each plate: its plane (and whether it is still hosted by a face), contour in the plane and \
     in the world, normal, thickness, justification, area, material, coating and role.",
    crate::steel_detail::IdsParams,
    Vec<crate::steel_detail::PlateInfo>,
    |s, p| crate::steel_detail::plate_info(s, p)
);

edit_command!(
    BoltPatternCreate,
    "bolt_pattern.create",
    "Place a bolt pattern",
    "Places bolts through the elements they join (STR-STL-006): `element_ids` (two or more), the \
     plane they are square to (a work plane or a `face`), and a `layout`: `{shape: rect, rows, \
     columns, spacing_x, spacing_y, centre}` or `{shape: circle, count, radius, centre, \
     start_angle}` in the plane. `diameter` (default M20, 3/4\" under US codes), `standard` \
     (generic_metric or generic_inch proportions), `grade`, `grip_increase`, `hole_tolerance` \
     (2 mm or 1/16\"), `slot`, `inverted`, `location` (shop, site), `coating`. The grip (first entry \
     to last exit through the elements), the bolt length under the head (grip, two washers, a nut \
     and thread, rounded up to 5 mm or 1/4\") and the holes in each element are found from the \
     elements' bodies. An IfcElementAssembly (FASTENER_GROUP) aggregating IfcMechanicalFastener \
     (BOLT) elements; holes are written to IFC as openings. Returns the pattern.",
    "Bolt Pattern",
    crate::steel_detail::PatternCreateParams,
    ElementId,
    crate::steel_detail::bolt_pattern_create
);

edit_command!(
    AnchorPatternCreate,
    "anchor_pattern.create",
    "Place an anchor pattern",
    "Places anchor rods between steel and concrete (STR-STL-006), as `bolt_pattern.create` with \
     `length` (default 400 mm): each rod from a nut above the steel down into the concrete \
     (IfcMechanicalFastener ANCHORBOLT).",
    "Anchor Pattern",
    crate::steel_detail::PatternCreateParams,
    ElementId,
    crate::steel_detail::anchor_pattern_create
);

edit_command!(
    HolePatternCreate,
    "hole_pattern.create",
    "Place a hole pattern",
    "Places holes through elements without fasteners (STR-STL-006), as `bolt_pattern.create`: the \
     holes in each element are found and written to IFC as openings.",
    "Hole Pattern",
    crate::steel_detail::PatternCreateParams,
    ElementId,
    crate::steel_detail::hole_pattern_create
);

edit_command!(
    StudPatternCreate,
    "shear_stud_pattern.create",
    "Place shear studs",
    "Places headed shear studs on one steel element's face (STR-STL-006): a `layout` in the plane, \
     `diameter` (default 19 mm) and `length` (default 100 mm), each stud standing out along the \
     face's normal (IfcMechanicalFastener STUDSHEARCONNECTOR).",
    "Shear Studs",
    crate::steel_detail::PatternCreateParams,
    ElementId,
    crate::steel_detail::stud_pattern_create
);

edit_command!(
    PatternSet,
    "fastener_pattern.set",
    "Edit fastener patterns",
    "Changes fastener patterns' `layout`, `diameter`, `grip_increase`, `hole_tolerance`, \
     `inverted`, `location`, `length` or `grade` (STR-STL-006), finding their grips and holes again \
     (with no changes, after the joined elements changed).",
    "Edit Fastener Pattern",
    crate::steel_detail::PatternSetParams,
    (),
    crate::steel_detail::pattern_set
);

read_command!(
    PatternInfo,
    "fastener_pattern.info",
    "Fastener patterns",
    "For each bolt, anchor, hole or shear stud pattern: its kind, elements, plane, positions, count, \
     diameter, standard and grade, grip, length, hole diameter (and slot), holes in each element, \
     location, whether inverted, and its fastener elements.",
    crate::steel_detail::IdsParams,
    Vec<crate::steel_detail::PatternInfo>,
    |s, p| crate::steel_detail::pattern_info(s, p)
);

read_command!(
    WeldEdges,
    "weld.edges",
    "Edges that can be welded",
    "The edges of the given elements where they touch each other within 1 mm (STR-STL-007): the \
     edges `weld.create` accepts, as pairs of points.",
    crate::steel_detail::IdsParams,
    Vec<[[f64; 3]; 2]>,
    |s, p| crate::steel_detail::weld_edges(s, p)
);

edit_command!(
    WeldCreate,
    "weld.create",
    "Place a weld",
    "Places a weld (STR-STL-007) along an `edge` where `element_ids` (two or more) touch within \
     1 mm (from `weld.edges`): `weld_type` and `weld_type_2` (fillet by default), `a` (the throat, \
     default 6 mm), `length` (from the edge's start; the whole edge by default), `surface`, \
     `preparation`, `location` (shop, site) and `intermittent` `{length, pitch}`; values from the \
     weld table (`weld.tables`). The weld keeps its place when the elements move and is flagged for \
     review; it goes when one of them is deleted. IfcFastener (WELD) with Pset_FastenerWeld. \
     Returns the weld.",
    "Weld",
    crate::steel_detail::WeldCreateParams,
    ElementId,
    crate::steel_detail::weld_create
);

edit_command!(
    WeldSet,
    "weld.set",
    "Edit welds",
    "Changes welds' type, throat, length, surface, preparation, location or intermittence, or with \
     `accept` clears *needs review* after their elements changed (STR-STL-007).",
    "Edit Weld",
    crate::steel_detail::WeldSetParams,
    (),
    crate::steel_detail::weld_set
);

read_command!(
    WeldInfo,
    "weld.info",
    "Welds",
    "For each weld: its elements, edge, types, throat, length, surface, preparation, location, \
     intermittence and whether it needs review (an element changed since it was placed or \
     accepted).",
    crate::steel_detail::IdsParams,
    Vec<crate::steel_detail::WeldInfo>,
    |s, p| crate::steel_detail::weld_info(s, p)
);

read_command!(
    WeldReview,
    "weld.review",
    "Welds needing review",
    "The welds whose elements changed since they were placed or accepted (STR-STL-007).",
    Option<serde_json::Value>,
    Vec<ElementId>,
    |s, _| Ok(crate::steel_detail::weld_review(s))
);

read_command!(
    WeldTables,
    "weld.tables",
    "The weld table",
    "The weld types, surface shapes and preparations welds take their values from (an open data \
     file, STR-STL-007).",
    Option<serde_json::Value>,
    crate::steel_detail::WeldTables,
    |_, _| Ok(crate::steel_detail::weld_tables())
);

read_command!(
    FaceList,
    "face.list",
    "Elements' faces",
    "The named faces of columns, beams, braces and steel plates (STR-STL-005): flange_1, \
     flange_2, web_1, web_2, start and end for framing, top and bottom for plates, each with its \
     plane's origin, x direction and outward normal. Plates and fastener patterns are placed on them \
     with `face: {element, face}`.",
    crate::steel_detail::IdsParams,
    Vec<crate::steel_detail::FaceInfo>,
    |s, p| crate::steel_detail::face_list(s, p)
);

edit_command!(
    CutCope,
    "steel_cut.cope",
    "Cope a member",
    "Copes one of two members that meet (STR-STL-008): the member whose end is at the joint (or \
     `cut_member_id`) loses what lies in the other's flange zones (their thickness plus \
     `clearance`, default 13 mm or 1/2\"), across its flange width plus the clearance. The cope \
     follows the main member (its depth). An IfcOpeningElement (USERDEFINED COPE) voiding the \
     member, in a new sub-connection. Returns the cuts and the sub-connection.",
    "Cope",
    crate::steel_cuts::CopeParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::cope
);

edit_command!(
    CutMiter,
    "steel_cut.miter",
    "Miter two members",
    "Cuts two members meeting at their ends on the plane halving the angle between them \
     (STR-STL-008), one cut on each, in a sub-connection.",
    "Miter",
    crate::steel_cuts::PairParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::miter
);

edit_command!(
    CutSaw,
    "steel_cut.saw",
    "Saw cut a member's end",
    "Cuts member `a`'s end flush with member `b`'s nearer flange (`part: flange`, the default) or \
     web face (`part: web`) (STR-STL-008).",
    "Saw Cut",
    crate::steel_cuts::PairParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::saw
);

edit_command!(
    CutThrough,
    "steel_cut.cut_through",
    "Cut through",
    "Cuts `cut_id` by the body of `cutter_id` grown by `gap` (STR-STL-008); the cut follows both.",
    "Cut Through",
    crate::steel_cuts::CutParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::cut_through
);

edit_command!(
    CutBy,
    "steel_cut.cut_by",
    "Cut by",
    "Cuts `cut_id` by the body of `cutter_id` where they intersect (STR-STL-008).",
    "Cut By",
    crate::steel_cuts::CutParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::cut_by
);

edit_command!(
    CutUncope,
    "steel_cut.uncope",
    "Uncope members",
    "Removes every cope (and skewed cope) on `member_ids` (STR-STL-008); a sub-connection left \
     with no cuts is deleted. Copes belonging to a connection are skipped and listed. With nothing \
     to remove it fails (`uncope.nothing`) and changes nothing.",
    "Uncope",
    crate::steel_cuts::UncopeParams,
    crate::steel_cuts::UncopeReport,
    crate::steel_cuts::uncope
);

read_command!(
    CutList,
    "steel_cut.list",
    "Steel cuts",
    "The cuts and modifiers on elements (every one when `element_ids` is omitted): kind and \
     parameters, target and sub-connection (STR-STL-008).",
    crate::steel_cuts::ListParams,
    Vec<crate::steel_cuts::CutInfo>,
    |s, p| crate::steel_cuts::list(s, p)
);

edit_command!(
    ModShorten,
    "steel_modifier.shorten",
    "Shorten a member end",
    "Moves a beam's or brace's `end` (start or end) back by `length` (forward when negative), \
     through its end extension; `angle_h` and `angle_w` (degrees) tilt the cut across its height and \
     width for a diagonal end (STR-STL-008).",
    "Shorten",
    crate::steel_cuts::ShortenParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::shorten
);

edit_command!(
    ModCopeSkewed,
    "steel_modifier.cope_skewed",
    "Cope a member end",
    "Notches a member's `end`: `length` back from it and `depth` down from the top (`top`, the \
     default) and/or up from the bottom (`bottom`), the notch's inner face turned by `angle` \
     degrees (STR-STL-008).",
    "Skewed Cope",
    crate::steel_cuts::CopeSkewedParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::cope_skewed
);

edit_command!(
    ModContourCut,
    "steel_modifier.contour_cut",
    "Cut a contour",
    "Cuts a closed `contour` (corners or segments) on a work plane or `face` out of an element, \
     `depth` into it (through by default) (STR-STL-008).",
    "Contour Cut",
    crate::steel_cuts::ContourCutParams,
    crate::steel_cuts::Made,
    crate::steel_cuts::contour_cut
);

edit_command!(
    ModCornerCut,
    "steel_modifier.corner_cut",
    "Cut a plate's corner",
    "Cuts a plate's `corner` (from 0, in contour order): `straight` (a chamfer), `convex` (rounded) \
     or `concave` (a notch arc), `size` along each edge from the corner; size 0 removes the cut \
     (STR-STL-008).",
    "Corner Cut",
    crate::steel_cuts::CornerCutParams,
    (),
    crate::steel_cuts::corner_cut
);

edit_command!(
    NumberingRun,
    "steel_numbering.run",
    "Number the steel",
    "Gives the model's steel parts their part marks and its shop assemblies their assembly marks \
     (STR-STL-010). Parts are steel columns, beams and braces, plates placed by hand and connection \
     plates; identical parts (a member's section, length, holes and cuts in its own frame, either way \
     round; a plate's thickness, outline and holes, however placed) share a mark, `c1`, `b1`, `br1`, \
     `p1` by kind (`options`: prefixes and the first number). An assembly is a main member and \
     everything shop-welded to it (shop welds; a connection's plates to their member); identical \
     assemblies share a mark, the prefix in capitals. Part marks go to each element's Tag and \
     `rxd:steel_marks`; each assembly becomes an IfcElementAssembly (STEEL_ASSEMBLY) tagged with its \
     mark, made again on each run. Returns the marks with their counts and elements.",
    "Steel Numbering",
    crate::steel_numbering::RunParams,
    crate::steel_numbering::Report,
    crate::steel_numbering::run
);

read_command!(
    NumberingPreview,
    "steel_numbering.preview",
    "Preview steel numbering",
    "The part and assembly marks `steel_numbering.run` would give, without changing the model \
     (STR-STL-010).",
    crate::steel_numbering::RunParams,
    crate::steel_numbering::Report,
    |s, p| crate::steel_numbering::preview(s, p)
);

edit_command!(
    CheckSetForces,
    "structural_connection.set_design_forces",
    "Set connections' design forces",
    "Sets the design forces connection code checks use (STR-STL-009): per load case or combination \
     (`group`), the axial force `n` (tension positive), the shear `v` in the connected member's depth \
     direction (a beam's end reaction), the shear `vh` across it and the moment `m` (N, N·m). An \
     empty list clears them.",
    "Design Forces",
    crate::connection_check::ForcesParams,
    (),
    crate::connection_check::set_design_forces
);

edit_command!(
    CheckRecord,
    "structural_connection.record_check",
    "Record a connection check",
    "Records a connection design provider's answer on a connection (STR-STL-009): its check items \
     (name, group, action, resistance, ratio, ok, the parameter that would strengthen it), after \
     setting `params` (Presize's sizes) if given. The status is ok when every item passes; the \
     utilisation is the largest ratio; the input answered is hashed so later changes show the status \
     as not calculated. `structural_connection.check` and `presize` call providers and record their \
     answers.",
    "Connection Check",
    crate::connection_check::RecordParams,
    crate::connection_check::CheckRecord,
    crate::connection_check::record
);

read_command!(
    CheckStatus,
    "structural_connection.check_status",
    "Connections' check status",
    "For each connection: its code check status (not_calculated, failed or ok; not calculated when \
     never checked or when it, its members or its design forces changed since), the last check's \
     provider, code, date, utilisation and items, and its design forces (STR-STL-009).",
    crate::connection_check::StatusParams,
    Vec<crate::connection_check::StatusInfo>,
    |s, p| crate::connection_check::status(s, p)
);

edit_command!(
    CustomCustomize,
    "structural_connection.customize",
    "Make a custom connection type",
    "Makes a named custom connection type (STR-STL-003) of standard or generic connections at one \
     join (`connection_ids`) and loose plates, fastener patterns and welds there (`element_ids`): \
     at most three members; the type records the members' roles and section families, each nested \
     connection with its parameters, and each plate, pattern and weld relative to the join (its point, \
     the main member's axis, the way the connected member leaves). What was chosen becomes the first \
     instance. Place the type with `structural_connection.create` and its `type_id` on joins with the \
     same members' roles and section families. Returns the type and the instance.",
    "Customize Connection",
    crate::custom_connections::CustomizeParams,
    crate::custom_connections::Customized,
    crate::custom_connections::customize
);

edit_command!(
    CustomAdd,
    "custom_connection.add_elements",
    "Add to a custom connection type",
    "Adds plates, fastener patterns or welds placed at one of a custom type's instances to the type, \
     and remakes every instance with them (STR-STL-003). With more than one instance it needs \
     `confirm` (the refusal states the count). Returns the instances remade.",
    "Edit Custom Connection",
    crate::custom_connections::EditParams,
    usize,
    crate::custom_connections::add_elements
);

edit_command!(
    CustomRemove,
    "custom_connection.remove_elements",
    "Remove from a custom connection type",
    "Removes sub-elements, picked in any instance, from a custom type and every instance \
     (STR-STL-003); patterns and welds on a removed plate go with it. With more than one instance it \
     needs `confirm` (the refusal states the count). Returns the instances remade.",
    "Edit Custom Connection",
    crate::custom_connections::EditParams,
    usize,
    crate::custom_connections::remove_elements
);

edit_command!(
    CustomExpose,
    "custom_connection.expose_parameter",
    "Expose a custom connection parameter",
    "Makes a sub-element's parameter a type parameter of a custom connection (STR-STL-003): `path` \
     is `<sub-element name>.<parameter>` (a plate's thickness, a pattern's diameter, a weld's a, a \
     nested connection's parameters). Returns its current value.",
    "Expose Parameter",
    crate::custom_connections::ExposeParams,
    f64,
    crate::custom_connections::expose
);

edit_command!(
    CustomSet,
    "custom_connection.set_parameter",
    "Set a custom connection parameter",
    "Sets an exposed type parameter of a custom connection and remakes every instance \
     (STR-STL-003). Returns the instances remade.",
    "Custom Connection Parameter",
    crate::custom_connections::SetParams,
    usize,
    crate::custom_connections::set_parameter
);

edit_command!(
    RebarPlace,
    "rebar.place",
    "Place a rebar set",
    "Places a rebar set in a concrete host by Expand to Host (STR-RBR-004): `placement` is \
     `{kind: section}` for closed links (shape 51) in a column or beam's section spread along it, \
     `{kind: face, face: top|bottom|left|right}` for straight, L or U bars (00, 11, 21) along a \
     member at a face, or `{kind: layer, face, angle, vertical, offset}` for straight bars in a \
     floor's top or bottom layer or a wall's exterior or interior face, each clipped to the \
     outline less the edge cover, or `{kind: through, along}` for straight bars through a wall's \
     thickness `along` metres from its start, spread up it. `varying` (STR-RBR-005) fits each bar \
     to the host at its own place (bars through a slanted or tapered wall); a standard set's bars \
     all take the first one's length. `layout` (STR-RBR-008): `rule` single, fixed_number (n), \
     maximum_spacing (spacing), number_with_spacing (n, spacing), minimum_clear_spacing (clear) or \
     custom (formula like `3x100; ?x200; 5x100` or `1400@100; 900@300`, mm). Bars keep inside the \
     host faces' covers; a shape that cannot fit is refused naming the dimension. Returns the set.",
    "Place Rebar",
    crate::rebar::PlaceParams,
    ElementId,
    crate::rebar::place
);

edit_command!(
    RebarSetLayout,
    "rebar.set_layout",
    "Change rebar sets' layout",
    "Changes the layout rule of rebar sets (STR-RBR-008): `rule` with `n`, `spacing` (m), `clear` \
     (m) or `formula` as `rebar.place` takes them. A layout that does not fit is refused.",
    "Rebar Layout",
    crate::rebar::LayoutParams,
    (),
    crate::rebar::set_layout
);

edit_command!(
    RebarSet,
    "rebar.set",
    "Change rebar sets",
    "Changes rebar sets' bar type, shape code or placement (STR-RBR-004); the bars are worked out \
     again from the host, and a change that does not fit is refused.",
    "Modify Rebar",
    crate::rebar::SetParams,
    (),
    crate::rebar::set
);

edit_command!(
    RebarRehost,
    "rebar.rehost",
    "Move rebar sets to another host",
    "Moves rebar sets to another host that can hold rebar (STR-RBR-001), working their bars out \
     again in it; an orphaned set is put back in order this way.",
    "Rehost Rebar",
    crate::rebar::RehostParams,
    (),
    crate::rebar::rehost
);

read_command!(
    RebarInfoCmd,
    "rebar.info",
    "Rebar sets",
    "Each rebar set (STR-RBR-004): host, whether it is orphaned and why, shape, bar type and \
     diameter, bar count, layout, the length the bars spread over, each bar's exact and rounded \
     length (STR-RBR-002), the first as Properties show it (`exact [rounded]`, mm), total length \
     and mass, the shape's dimensions, role (main or ligature) and centrelines.",
    crate::rebar::InfoParams,
    Vec<crate::rebar::RebarInfo>,
    crate::rebar::info
);

read_command!(
    RebarConstraints,
    "rebar.constraints",
    "A sketched set's handles and constraints",
    "A sketched set's handles (STR-RBR-007): `start`, `segment:N` and `end`, each with where it is, \
     its direction, its constraint and where its reference plane is; and the host's faces a handle \
     can reference.",
    crate::rebar_sketch::ResetParams,
    crate::rebar_sketch::ConstraintsInfo,
    crate::rebar_sketch::constraints
);

read_command!(
    RebarCanHost,
    "rebar.can_host",
    "Whether an element can host rebar",
    "Whether an element can hold rebar (STR-RBR-001): structural concrete columns and beams, \
     structural floors and walls with a concrete layer. With its host kind, the reason when it \
     cannot, and each face's cover type.",
    crate::rebar::CanHostParams,
    crate::rebar::CanHost,
    crate::rebar::can_host
);

read_command!(
    RebarBarTypeList,
    "rebar_bar_type.list",
    "Rebar bar types",
    "The region's bar catalogue (ASTM A615 #3–#18, or 6–50 mm bars) and the project's own bar types \
     (STR-RBR-003), with diameters, material and inside bend diameters for standard bends, hooks \
     and stirrups.",
    Option<serde_json::Value>,
    Vec<crate::rebar::BarType>,
    |s, _| Ok(crate::rebar::bar_types(s))
);

edit_command!(
    RebarBarTypeCreate,
    "rebar_bar_type.create",
    "Make a rebar bar type",
    "Makes a project bar type (STR-RBR-003): name, diameter (m), material and inside bend diameter \
     (default by the region's rule). Returns its id.",
    "New Bar Type",
    crate::rebar::BarTypeParams,
    String,
    crate::rebar::bar_type_create
);

read_command!(
    RebarHookTypeList,
    "rebar_hook_type.list",
    "Rebar hook types",
    "The region's standard hooks (90°, 180°, 135° stirrup or tie) and the project's own (STR-RBR-003): \
     angle and extension as a multiple of the bar diameter with a minimum length.",
    Option<serde_json::Value>,
    Vec<crate::rebar::HookType>,
    |s, _| Ok(crate::rebar::all_hook_types(s))
);

edit_command!(
    RebarHookTypeCreate,
    "rebar_hook_type.create",
    "Make a rebar hook type",
    "Makes a project hook type (STR-RBR-003): name, angle (degrees), extension multiplier of the bar \
     diameter and minimum extension (m). Returns its id.",
    "New Hook Type",
    crate::rebar::HookTypeParams,
    String,
    crate::rebar::hook_type_create
);

read_command!(
    RebarShapeList,
    "rebar_shape.list",
    "Rebar shapes",
    "The rebar shape catalogue (STR-RBR-003): each shape code with its name, dimensions, style \
     (standard or stirrup/tie) and bend count.",
    Option<serde_json::Value>,
    crate::rebar::ShapeCatalogue,
    |_, _| Ok(crate::rebar::shapes())
);

read_command!(
    ReinforcementSettingsGet,
    "reinforcement_settings.get",
    "Reinforcement settings",
    "The project's reinforcement settings (STR-RBR-002): rounding (on, method, bar and segment \
     increments), hooks in shape definitions, area and path rebar, cover types with their uses, \
     the project's bar and hook types, and which settings are locked and why.",
    Option<serde_json::Value>,
    crate::rebar::SettingsInfo,
    |s, _| Ok(crate::rebar::settings_info(s))
);

edit_command!(
    ReinforcementSettingsSet,
    "reinforcement_settings.set",
    "Change a reinforcement setting",
    "Changes a reinforcement setting (STR-RBR-002): `rounding.on`, `rounding.method` (nearest, up, \
     down), `rounding.bar_length` or `rounding.segment_length` (m, at most 0.1), `hooks_in_shape` \
     (locked once rebar is placed) or `area_path_rebar`.",
    "Reinforcement Settings",
    crate::rebar::SetSettingParams,
    (),
    crate::rebar::set_setting
);

edit_command!(
    RebarCoverTypeCreate,
    "rebar_cover_type.create",
    "Make a rebar cover type",
    "Makes a named rebar cover type with its offset (m, 0 to 0.5) (STR-RBR-009). Returns its id.",
    "New Cover Type",
    crate::rebar::CoverTypeParams,
    String,
    crate::rebar::cover_type_create
);

edit_command!(
    RebarCoverTypeSet,
    "rebar_cover_type.set",
    "Change a rebar cover type",
    "Renames a cover type or changes its offset (STR-RBR-009); bars on faces using it follow.",
    "Modify Cover Type",
    crate::rebar::CoverTypeSetParams,
    (),
    crate::rebar::cover_type_set
);

edit_command!(
    RebarCoverTypeDelete,
    "rebar_cover_type.delete",
    "Delete a rebar cover type",
    "Deletes a cover type (STR-RBR-009). When host faces use it a `replacement` is required, else \
     it is refused with the count; returns the faces moved.",
    "Delete Cover Type",
    crate::rebar::CoverTypeDeleteParams,
    usize,
    crate::rebar::cover_type_delete
);

edit_command!(
    RebarCoverAssign,
    "rebar_cover.assign",
    "Assign rebar cover to host faces",
    "Gives hosts' faces a cover type (STR-RBR-009): `face` top, bottom, exterior, interior or other \
     (every face when omitted). Their rebar follows.",
    "Rebar Cover",
    crate::rebar::AssignParams,
    (),
    crate::rebar::assign
);

edit_command!(
    AreaReinforcementCreate,
    "area_reinforcement.create",
    "Lay area reinforcement",
    "Lays area reinforcement in a structural floor or wall (STR-RBR-010): `boundary` a closed loop \
     of [x, y] points in the host's plane (plan for floors; along the wall and up from its base for \
     walls) or `whole_host`; `major_edge` the loop's edge the major bars follow; `layers` each with \
     `face` (top, bottom; exterior, interior), `direction` (major, minor), `on`, `bar_type`, \
     `hook`, `hook_flip`, `spacing` or `n`; `layout_rule` maximum_spacing or fixed_number; \
     `additional_top` and `additional_bottom` offsets (m). Openings and the boundary cut the bars. \
     Returns the system (IfcElementAssembly REINFORCEMENT_UNIT).",
    "Area Reinforcement",
    crate::rebar_systems::AreaCreateParams,
    ElementId,
    crate::rebar_systems::area_create
);

edit_command!(
    AreaReinforcementSet,
    "area_reinforcement.set",
    "Change area reinforcement",
    "Changes area systems' layers, layout rule, boundary, major edge or additional offsets \
     (STR-RBR-010); refused when the bars would not fit.",
    "Modify Area Reinforcement",
    crate::rebar_systems::AreaSetParams,
    (),
    crate::rebar_systems::area_set
);

edit_command!(
    AreaReinforcementRemoveSystem,
    "area_reinforcement.remove_system",
    "Remove an area system, keeping its bars",
    "Removes an area system and keeps its layers as ordinary rebar sets, returned (STR-RBR-010). \
     Needs area and path reinforcement as real rebar (`reinforcement_settings.set` \
     `area_path_rebar`).",
    "Remove Area System",
    crate::rebar_systems::RemoveSystemParams,
    Vec<ElementId>,
    crate::rebar_systems::area_remove_system
);

edit_command!(
    PathReinforcementCreate,
    "path_reinforcement.create",
    "Lay path reinforcement",
    "Lays bars square to an open `path` ([x, y] points in the host's plane) in a structural floor \
     or wall (STR-RBR-011), on its left (or right with `flip`), at `face`, spaced along it by \
     `spacing` or `count` (both: number with spacing). `primary` and optional `alternating` bars \
     each have `bar_type`, `length`, `legs` (segments into the host at both ends), `start_hook`, \
     `end_hook` and `offset` (where they start from the path). Returns the system.",
    "Path Reinforcement",
    crate::rebar_systems::PathCreateParams,
    ElementId,
    crate::rebar_systems::path_create
);

edit_command!(
    PathReinforcementSet,
    "path_reinforcement.set",
    "Change path reinforcement",
    "Changes path systems' `path`, side (`flip`), `layout`, `primary` or `alternating` bars \
     (`null` removes them) (STR-RBR-011).",
    "Modify Path Reinforcement",
    crate::rebar_systems::PathSetParams,
    (),
    crate::rebar_systems::path_set
);

edit_command!(
    FabricAreaCreate,
    "fabric_area.create",
    "Lay a fabric area",
    "Lays fabric sheets over a `boundary` (or `whole_host`) of a structural floor or wall \
     (STR-RBR-012) from its `major_edge`, the sheets' length along it: `sheet_type_id`, \
     `lap_position` (aligned, staggered: every second row's end laps half a sheet along), \
     `major_lap` and `minor_lap` (m), `face`, `offset`. Sheets are cut by the boundary, the host's \
     openings and its cover. Returns the system.",
    "Fabric Area",
    crate::rebar_systems::FabricAreaParams,
    ElementId,
    crate::rebar_systems::fabric_area_create
);

edit_command!(
    FabricAreaSet,
    "fabric_area.set",
    "Change fabric areas",
    "Changes fabric areas' sheet type, lap position, laps, major edge or boundary (STR-RBR-012).",
    "Modify Fabric Area",
    crate::rebar_systems::FabricAreaSetParams,
    (),
    crate::rebar_systems::fabric_area_set
);

edit_command!(
    FabricSheetPlace,
    "fabric_sheet.place",
    "Place a fabric sheet",
    "Places one fabric sheet (IfcReinforcingMesh) in a structural floor or wall (STR-RBR-012): its \
     first corner `point` in the host's plane, `rotation` (degrees), `cut_by_cover` to clip it to \
     the host less the cover, `face`, `offset`.",
    "Fabric Sheet",
    crate::rebar_systems::FabricSheetParams,
    ElementId,
    crate::rebar_systems::fabric_sheet_place
);

read_command!(
    FabricWireTypeList,
    "fabric_wire_type.list",
    "Fabric wire types",
    "Fabric wire types (STR-RBR-012): the region's catalogue (BS 4483 or WRI) and the project's own, \
     with diameter and material.",
    Option<serde_json::Value>,
    Vec<crate::rebar_systems::WireType>,
    |s, _| Ok(crate::rebar_systems::wire_types(s))
);

edit_command!(
    FabricWireTypeCreate,
    "fabric_wire_type.create",
    "Make a fabric wire type",
    "Makes a project fabric wire type: `name`, `diameter` (m), `material` (STR-RBR-012).",
    "New Wire Type",
    crate::rebar_systems::WireTypeParams,
    String,
    crate::rebar_systems::wire_type_create
);

read_command!(
    FabricSheetTypeList,
    "fabric_sheet_type.list",
    "Fabric sheet types",
    "Fabric sheet types (STR-RBR-012): major and minor wire types and spacings, length, width, \
     overhangs and mass per square metre.",
    Option<serde_json::Value>,
    Vec<crate::rebar_systems::SheetTypeInfo>,
    |s, _| Ok(crate::rebar_systems::sheet_type_list(s))
);

edit_command!(
    FabricSheetTypeCreate,
    "fabric_sheet_type.create",
    "Make a fabric sheet type",
    "Makes a project fabric sheet type: `name`, `major_wire`, `minor_wire`, `major_spacing`, \
     `minor_spacing`, `length`, `width` (m), optional overhangs (STR-RBR-012).",
    "New Sheet Type",
    crate::rebar_systems::SheetTypeParams,
    String,
    crate::rebar_systems::sheet_type_create
);

read_command!(
    ReinforcementSystemInfo,
    "reinforcement_system.info",
    "Area, path and fabric reinforcement",
    "Each area, path or fabric system (STR-RBR-010 … 012): kind, host, whether orphaned and why, \
     whether its bars are real rebar, bar groups (area layers; primary and alternating path bars) \
     with counts, lengths and mass, fabric sheets with corners, wires, cuts and mass, totals and \
     the steel's volume.",
    crate::rebar_systems::InfoParams,
    Vec<crate::rebar_systems::SystemInfo>,
    crate::rebar_systems::info
);

edit_command!(
    RebarSetViewVisibility,
    "rebar.set_view_visibility",
    "Set rebar's view visibility states",
    "Sets whether rebar sets or area, path or fabric systems show unobscured (through their host) \
     and as solid (3D views only; tubes at Fine detail, centrelines otherwise) in a view \
     (STR-RBR-017). View data: nothing is written to the rebar.",
    "View Visibility States",
    crate::rebar_views::VisibilityParams,
    (),
    crate::rebar_views::set_view_visibility
);

edit_command!(
    RebarSetPresentation,
    "rebar.set_presentation",
    "Set rebar sets' presentation in a view",
    "Which bars of rebar sets a view draws (STR-RBR-017): `mode` all, first_last, middle or select \
     (with `selected_indices`, from 0). First/Last and Middle add the set's extent line. Counts, \
     schedules and the export keep every bar.",
    "Rebar Presentation",
    crate::rebar_views::PresentationParams,
    (),
    crate::rebar_views::set_presentation
);

read_command!(
    RebarViewStates,
    "rebar.view_states",
    "A rebar element's view states",
    "A rebar set's or system's states in every view (STR-RBR-017): unobscured, solid, presentation \
     and selected bars, with whether the view is 3D (only 3D views take solid).",
    crate::rebar_views::ViewStatesParams,
    Vec<crate::rebar_views::ViewState>,
    crate::rebar_views::view_states
);

edit_command!(
    RebarSetPartition,
    "rebar.set_partition",
    "Put rebar in a partition",
    "Puts rebar sets, fabric sheets and fabric areas in a partition (free text; empty for none) \
     (STR-RBR-016). Identical elements share a number within their partition.",
    "Rebar Partition",
    crate::rebar_numbers::PartitionParams,
    (),
    crate::rebar_numbers::set_partition
);

read_command!(
    ReinforcementNumberingList,
    "reinforcement_numbering.list",
    "Reinforcement numbers",
    "The rebar and fabric number sequences per partition (STR-RBR-016): start number and each \
     number in use with its identity, summary, count, elements and lock.",
    crate::rebar_numbers::ListParams,
    Vec<crate::rebar_numbers::SequenceInfo>,
    |s, p| Ok(crate::rebar_numbers::list(s, p))
);

edit_command!(
    ReinforcementNumberingRenumber,
    "reinforcement_numbering.renumber",
    "Renumber reinforcement",
    "Numbers a partition's rebar or fabric sequence again (STR-RBR-016): in creation order, or \
     with `remove_gaps` closing the gaps in number order; `start` sets the first number. Locked \
     numbers stay and are skipped.",
    "Renumber Reinforcement",
    crate::rebar_numbers::RenumberParams,
    (),
    crate::rebar_numbers::renumber
);

edit_command!(
    ReinforcementNumberingSet,
    "reinforcement_numbering.set",
    "Set a reinforcement number",
    "Gives an identity in a partition's sequence a number, or locks or unlocks it (STR-RBR-016). \
     A number another identity in use has is refused (`reinforcement_numbering.taken`, naming it).",
    "Reinforcement Number",
    crate::rebar_numbers::SetNumberParams,
    (),
    crate::rebar_numbers::set_number
);

edit_command!(
    ReinforcementNumberingApply,
    "reinforcement_numbering.apply",
    "Apply reinforcement number edits",
    "The Reinforcement Numbers dialog's edits as one step (STR-RBR-016): `rows` with an identity's \
     new `number` or `locked`, then `start`, then `remove_gaps` or `renumber`.",
    "Reinforcement Numbers",
    crate::rebar_numbers::ApplyParams,
    (),
    crate::rebar_numbers::apply
);

edit_command!(
    RebarTagPlace,
    "rebar_tag.place",
    "Tag a rebar set",
    "Tags a rebar set in a plan (STR-RBR-018) with the Rebar Tag (its number and \"15 Ø12 @ 150\") \
     or another tag type: `point` the tag's centre (with a leader), else on the middle bar.",
    "Rebar Tag",
    crate::rebar_docs::TagParams,
    ElementId,
    crate::rebar_docs::place_tag
);

edit_command!(
    MultiRebarAnnotationPlace,
    "multi_rebar_annotation.place",
    "Place a multi-rebar annotation",
    "Places a dimension across one or more parallel rebar sets in a plan (STR-RBR-018), measuring \
     from their first bar to their last, reading quantity, size and spacing: `kind` aligned \
     (square to the bars) or linear (along x or y), `line` a point on the dimension line. Sets \
     that are not parallel are refused. It follows the sets and goes with the last of them.",
    "Multi-Rebar Annotation",
    crate::rebar_docs::MultiParams,
    ElementId,
    crate::rebar_docs::place_multi
);

edit_command!(
    BendingDetailPlace,
    "bending_detail.place",
    "Place a bending detail",
    "Places a set's bending detail in a plan (STR-RBR-018): its bar flattened in its own plane at \
     `point`, `scale` (drawn over real size, default 0.25) and `rotation` (degrees), with the shape \
     code and the segment values after rounding. It updates with the set.",
    "Bending Detail",
    crate::rebar_docs::BendingParams,
    ElementId,
    crate::rebar_docs::place_bending
);

read_command!(
    RebarAnnotationList,
    "rebar_annotation.list",
    "Rebar annotations of a plan",
    "The multi-rebar annotations and bending details of a plan as drawn now (STR-RBR-018): their \
     points, extent and text, or their segments and lines.",
    crate::rebar_docs::InfoParams,
    Vec<crate::rebar_docs::Drawn>,
    crate::rebar_docs::list
);

edit_command!(
    RebarScheduleCreate,
    "rebar_schedule.create",
    "Make a bar bending schedule",
    "Makes a rebar schedule (STR-RBR-018) grouped by partition and rebar number: shape code, bar \
     type, segments A–E, quantity, bar length, total length and mass (kg, from the bar's nominal \
     mass per metre). Returns the schedule.",
    "Bar Bending Schedule",
    crate::rebar_docs::ScheduleParams,
    ElementId,
    crate::rebar_docs::create_schedule
);

edit_command!(
    RebarSetSegmentLength,
    "rebar.set_segment_length",
    "Set a rebar segment",
    "Sets one segment (A, B, …; outside dimension in metres, rounded to the segment increment) of a \
     set's shape (STR-RBR-006); `null` resets it. Refused past what the host's cover leaves room for.",
    "Segment Length",
    crate::rebar_edit::SegmentParams,
    (),
    crate::rebar_edit::set_segment_length
);

edit_command!(
    RebarMoveBar,
    "rebar.move_bar_in_set",
    "Move a bar in its set",
    "Moves one bar of a set (its layout position, from 0) along the set's array by `delta` metres \
     (STR-RBR-006).",
    "Move Rebar in Set",
    crate::rebar_edit::MoveBarParams,
    (),
    crate::rebar_edit::move_bar
);

edit_command!(
    RebarRemoveBar,
    "rebar.remove_bar",
    "Remove a bar from its set",
    "Removes one bar (its layout position) from a set (STR-RBR-006); the quantity drops.",
    "Remove Bar",
    crate::rebar_edit::BarParams,
    (),
    crate::rebar_edit::remove_bar
);

edit_command!(
    RebarResetBars,
    "rebar.reset_bars",
    "Reset a set's bars",
    "Brings back a set's removed and moved bars (STR-RBR-006).",
    "Reset Bars",
    crate::rebar_edit::ResetParams,
    (),
    crate::rebar_edit::reset_bars
);

read_command!(
    PrecastConfigGet,
    "precast_config.get",
    "A precast configuration value",
    "A value of the precast configuration (STR-PRE-001) by its dotted path, for example \
     `wall.solid.segmentation.max_length`; the whole configuration for an empty path. Lengths in \
     metres, weights in tonnes, the density in t/m³.",
    crate::precast::GetParams,
    serde_json::Value,
    crate::precast::get
);

edit_command!(
    PrecastConfigSet,
    "precast_config.set",
    "Set a precast configuration value",
    "Sets one value of the precast configuration (STR-PRE-001) by its dotted path. Refused, naming \
     the rule, when it breaks one (minimum above maximum, gaps outside 0–100 mm, weights not above \
     0, an unknown filename placeholder, a data folder that is not there). Raises the version and \
     returns it.",
    "Precast Configuration",
    crate::precast::SetParams,
    u32,
    crate::precast::set
);

edit_command!(
    PrecastConfigApply,
    "precast_config.apply",
    "Replace the precast configuration",
    "Replaces the whole precast configuration (STR-PRE-001) as one step, as the Configuration \
     dialog's OK does; refused naming the first broken rule. Returns the new version.",
    "Precast Configuration",
    crate::precast::ApplyParams,
    u32,
    crate::precast::apply
);

read_command!(
    PrecastConfigExport,
    "precast_config.export",
    "Export the precast configuration",
    "Writes the precast configuration (STR-PRE-001) to a JSON file with its schema version, to \
     share between projects.",
    crate::precast::FileParams,
    (),
    crate::precast::export
);

edit_command!(
    PrecastConfigImport,
    "precast_config.import",
    "Import a precast configuration",
    "Replaces the precast configuration (STR-PRE-001) with a file's, after checking its rules; \
     returns the new version and the components it names that the project does not have.",
    "Import Precast Configuration",
    crate::precast::FileParams,
    crate::precast::Imported,
    crate::precast::import
);

read_command!(
    PrecastConfigAffected,
    "precast_config.affected",
    "Assemblies made with older precast rules",
    "The precast configuration's version and the precast assemblies made with an older one \
     (STR-PRE-001), to re-split.",
    crate::precast::AffectedParams,
    crate::precast::Affected,
    crate::precast::affected_cmd
);

edit_command!(
    PrecastSplit,
    "precast.split",
    "Split walls and floors into precast pieces",
    "Splits straight walls and floors into precast pieces (STR-PRE-002) by the configuration's \
     rules: the fewest pieces within the maximum length (width for floors), height and weight \
     (volume × density), joints the configured gap wide and none nearer an opening than the \
     minimum distance, pieces as even as that allows (hollow-core: standard widths and the rest). \
     `slab_kind` solid, hollow_core or lattice_girder for floors; solid or double for walls. The \
     element's parts are divided (made first if it has none) and each piece's parts go in an \
     IfcElementAssembly (PRECAST_ASSEMBLY) with a piece mark (W1-1, W1-2A, S1-3 …). An element \
     already split is refused, naming its assemblies. Returns the assemblies.",
    "Split",
    crate::precast::SplitParams,
    Vec<ElementId>,
    crate::precast::split
);

edit_command!(
    PrecastResplit,
    "precast.resplit",
    "Split precast assemblies' sources again",
    "Removes the pieces and assemblies a split made of the assemblies' sources and splits them \
     again with the current configuration (STR-PRE-002). Returns the new assemblies.",
    "Re-split",
    crate::precast::ResplitParams,
    Vec<ElementId>,
    crate::precast::resplit
);

edit_command!(
    PrecastSetAutoUpdate,
    "precast.set_auto_update",
    "Turn precast automatic updates on or off",
    "Whether pieces' reinforcement and mounting parts are made again when pieces change \
     (STR-PRE-002). Returns the configuration's new version.",
    "Automatic Updates",
    crate::precast::AutoUpdateParams,
    u32,
    crate::precast::set_auto_update
);

edit_command!(
    PrecastReinforce,
    "precast.reinforce",
    "Reinforce precast pieces",
    "Reinforces precast assemblies of one kind (STR-PRE-003) with the configuration's types: \
     `area_type_id` area bars (walls: vertical and horizontal at both faces; slabs: both ways at \
     top and bottom), clipped to the piece less the edge cover and cut round openings with the \
     cover; walls also `edge_type_id` U-bars round each opening side and free edge. A type whose \
     thickness range does not hold a piece is refused, as are pieces of different kinds. Sets are \
     hosted by the element split, carry the piece mark as their partition and belong to the \
     assembly. Running again removes the generated sets that are as made and keeps those edited \
     since and any added by hand. Returns a report per assembly.",
    "Auto Reinforce",
    crate::precast_rebar::ReinforceParams,
    crate::precast_rebar::Report,
    crate::precast_rebar::reinforce
);

read_command!(
    PrecastCamExport,
    "precast.cam_export",
    "Export precast pieces for production",
    "Writes one file per precast piece (STR-PRE-007; every assembly when `assembly_ids` is empty) \
     with a CAM writer (`cam_writer.list`): the built-in neutral JSON or CSV piece model (outline, \
     openings, lifting anchors, bars by set with shape and bar, fabric wires, marks; millimetres in \
     the piece's production system: origin at the lower-left of its front face, X along, Y up or \
     across, Z in), or an external writer. Files go in `folder`, named by `filename_rule` (else the \
     configuration's): `{Var:Length:MinLength:Pad:Align}` fields of Name, ProdNo, Date, Time and \
     Mark, checked before anything is written (the error gives the position); colliding names get \
     a number. Pieces without reinforcement or mounting parts are warned about. The model is not \
     changed. Returns the files, renames and warnings.",
    crate::precast_cam::ExportParams,
    crate::precast_cam::ExportReport,
    crate::precast_cam::export
);

read_command!(
    CamWriterList,
    "cam_writer.list",
    "CAM writers",
    "The CAM writers there are (STR-PRE-007): the built-in neutral JSON and CSV writers, and the \
     external ones whose manifests (id, name, version, SPDX licence, extension, command) are in the \
     precast data folder's `cam_writers`; each with its format and version.",
    crate::precast_cam::ListParams,
    Vec<crate::precast_cam::WriterInfo>,
    crate::precast_cam::list
);

edit_command!(
    PrecastShopDrawings,
    "precast.shop_drawings",
    "Shop drawings for precast pieces",
    "Makes an A3 shop-drawing sheet per precast piece, or per group of identical pieces with \
     `group_identical` (default; same kind, outline, openings, reinforcement, sheets and anchors to \
     the millimetre) (STR-PRE-006). Each sheet carries a shop drawing drawn from the model whenever \
     shown or printed: the front view at the largest standard scale that fits with chain dimensions \
     of the ends, openings and anchors and the height, opening sizes, the centre-of-gravity symbol \
     with its distances, a section through the thickness, a bill of materials (concrete, rebar by \
     number, fabric, mounting parts) and the piece marks. Sheets made before for the same pieces \
     are replaced. `template_id`: only the built-in `default`. Returns the sheets.",
    "Shop Drawings",
    crate::precast_shop::ShopParams,
    Vec<ElementId>,
    crate::precast_shop::shop_drawings
);

edit_command!(
    PrecastMountingParts,
    "precast.place_mounting_parts",
    "Place mounting parts on precast pieces",
    "Places lifting anchors on precast pieces by the kind's mounting rules (STR-PRE-005): the weight \
     band holding the piece's weight (its parts' volume × density) gives the count; a piece heavier \
     than the last band gets none and is reported. Anchors go on a wall's top edge or a slab's top \
     face, symmetric about the piece's centre of gravity, the edge distance from its ends and the \
     opening distance from openings; when no symmetric places keep those, the nearest valid places \
     with a warning. IfcMechanicalFastener (LIFTING_ANCHOR) held by the assembly, which gets the \
     piece's NetWeight. Running again replaces parts as placed and keeps moved and hand-placed ones. \
     Returns a report per assembly.",
    "Mounting Parts",
    crate::precast_parts::PlaceParams,
    Vec<crate::precast_parts::PartsReport>,
    crate::precast_parts::place
);

edit_command!(
    MountingPartPlace,
    "mounting_part.place",
    "Place a mounting part by hand",
    "Places a lifting anchor on a precast piece by hand at `point` (its head; it runs down into the \
     piece), held by the assembly; later rule runs keep it (STR-PRE-005). Returns the part.",
    "Place Mounting Part",
    crate::precast_parts::HandParams,
    ElementId,
    crate::precast_parts::place_by_hand
);

edit_command!(
    PrecastCustomFabric,
    "precast.generate_custom_fabric",
    "Custom fabric sheets for precast pieces",
    "Makes welded fabric sheets shaped to each precast piece (STR-PRE-004) by a custom fabric rule \
     of the precast configuration (`rule_id`, the first by default): wires of the rule's types and \
     spacings, ending at the edge distance from the piece's outer edges and joints and at the \
     clearance round openings; wires shorter than the minimum left out and counted; a piece larger \
     than the largest sheet split square to the main wires (and across when too wide) into lapped \
     sheets; at the outer, inner or both faces. Sheets are IfcReinforcingMesh, numbered like fabric \
     (identical sheets share a number), partitioned by piece mark and held by the assembly. Running \
     again replaces sheets as made and keeps edited ones. Returns a report per assembly.",
    "Custom Fabric Sheets",
    crate::precast_fabric::GenerateParams,
    Vec<crate::precast_fabric::SheetReport>,
    crate::precast_fabric::generate
);

read_command!(
    PrecastReinforcementTypes,
    "precast.reinforcement_types",
    "Reinforcement types for precast pieces",
    "The area and edge reinforcement types (STR-PRE-003) whose thickness range holds every \
     selected precast assembly's piece, and the pieces' thickness range.",
    crate::precast_rebar::TypesParams,
    crate::precast_rebar::OfferedTypes,
    crate::precast_rebar::offered
);

read_command!(
    PrecastInfo,
    "precast.info",
    "Precast assemblies",
    "Precast assemblies (STR-PRE-002): piece mark, kind, source, parts, volume (m³) and weight \
     (t), the configuration version they were made with, and their place along and up.",
    crate::precast::InfoParams,
    Vec<crate::precast::AssemblyInfo>,
    crate::precast::info
);

edit_command!(
    RebarSketch,
    "rebar.sketch",
    "Sketch a rebar set",
    "Sketches a bar's centreline in a plane inside a concrete host (STR-RBR-007): `curves` are lines \
     `{line: {start, end}}` and arcs `{arc: {start, end, through}}` end to end, in the plane's \
     coordinates (metres); `plane` `{origin, x, normal}` defaults to the host's own (a floor's or \
     wall's middle, a beam's or column's long section through its axis). The shape is matched to \
     00, 11, 21 or 51, else 99 (special); corners are bent at the bar type's bend diameter. \
     `style` standard or stirrup_tie; `hook_start`, `hook_end` hook types. The set spreads along the \
     plane's normal to the far face's cover by `layout` (a single bar by default). Returns the set.",
    "Sketch Rebar",
    crate::rebar_sketch::SketchParams,
    ElementId,
    crate::rebar_sketch::sketch
);

edit_command!(
    RebarPlaceFreeForm,
    "rebar.place_free_form",
    "Place free-form rebar",
    "Places a free-form set (STR-RBR-013). `kind` surface: bars on a concrete roof's shell (dome, \
     vault or pitched), at the `face` bottom or top cover, where planes interpolated from \
     `start_plane` to `end_plane` ({origin, normal}) cut it (or `direction` x or y: planes square \
     to it across the roof), trimmed by the edge cover. `kind` aligned: closed links with 135° \
     hooks where planes square to a `path` (points, or `path_ref` axis for beams and columns, \
     along or up for walls) cut the host's body, each side inset by its own face's cover, so links \
     follow a varying section. `layout` spaces the bars between the planes or along the path. \
     Free-form sets are varying: each rounded length takes its own number. Returns the set.",
    "Free Form Rebar",
    crate::rebar_free::PlaceParams,
    ElementId,
    crate::rebar_free::place
);

edit_command!(
    RebarDistributeAlongPath,
    "rebar.distribute_along_path",
    "Distribute a bar along a path",
    "Carries a main set's first bar along a 3D path (STR-RBR-014): `path` points, or `path_ref` a \
     named path of its host (axis, along, up); `range` [from, to] metres along it; `layout` spaces \
     the bars. The main bar must lie square to the path where it starts; every bar's plane is \
     square to the path's tangent where it lies, its frame carried without extra twist. The set \
     follows the main set and goes with it. Returns the set.",
    "Path Distribution",
    crate::rebar_paths::PathParams,
    ElementId,
    crate::rebar_paths::along_path
);

edit_command!(
    RebarDistributeMorphed,
    "rebar.distribute_morphed",
    "Morph longitudinal bars through stirrups",
    "Longitudinal bars through a set of closed stirrups (STR-RBR-014): `count` bars spread round \
     each stirrup (or `spacing` along the first), each moved in to touch it, joined from the first \
     stirrup to the last. They follow the stirrups' shape, size and bar type, and go with them. \
     Returns the set.",
    "Morphed Distribution",
    crate::rebar_paths::MorphedParams,
    ElementId,
    crate::rebar_paths::morphed
);

edit_command!(
    RebarConstrain,
    "rebar.constrain",
    "Constrain a rebar handle",
    "Constrains a sketched set's handle (STR-RBR-007): `handle_id` `segment:N`, `start` or `end`; \
     `target_kind` cover (a host face's cover), surface (a host face) or bar (another set, \
     `bar_index`), or none to remove it; `target_ref` the face (`top`, `bottom`, `left`, `right`, \
     `start`, `end`, `exterior`, `interior`, `edge:N`) or the set; `offset` further in (metres; \
     defaults to 0 from a cover, else where the handle is). The reference must be within 6° of \
     parallel to a segment, or of square to an end. Constrained handles follow the host.",
    "Constrain Rebar",
    crate::rebar_sketch::ConstrainParams,
    (),
    crate::rebar_sketch::constrain
);

edit_command!(
    RebarResetConstraints,
    "rebar.reset_constraints",
    "Remove a set's constraints",
    "Removes a sketched set's constraints (STR-RBR-007); its handles stay where they are.",
    "Reset Constraints",
    crate::rebar_sketch::ResetParams,
    (),
    crate::rebar_sketch::reset_constraints
);

edit_command!(
    RebarConvertToVarying,
    "rebar.convert_to_varying",
    "Make rebar sets varying",
    "Makes sets varying (STR-RBR-005): each bar fitted to the host at its own place, so bars through \
     a tapered wall follow the taper. Each distinct rounded length takes its own rebar number. Only \
     bars through a wall's thickness vary yet.",
    "Convert to Varying",
    crate::rebar::ConvertParams,
    (),
    crate::rebar::convert_to_varying
);

edit_command!(
    RebarConvertToStandard,
    "rebar.convert_to_standard",
    "Make varying rebar sets standard",
    "Makes varying sets standard (STR-RBR-005), every bar the first one's length; refused while the \
     bars have more than one rounded length, naming how many.",
    "Convert to Standard",
    crate::rebar::ConvertParams,
    (),
    crate::rebar::convert_to_standard
);

edit_command!(
    RebarSplitSet,
    "rebar.split_set",
    "Split a rebar set",
    "Splits a set at one of its bars (from 0) (STR-RBR-006): it keeps the bars before, a new set of \
     the same type and shape takes the rest. Returns the new set.",
    "Split Set",
    crate::rebar_edit::SplitParams,
    ElementId,
    crate::rebar_edit::split_set
);

edit_command!(
    RebarPropagate,
    "rebar.propagate",
    "Propagate rebar to other hosts",
    "Copies sets to other hosts of the same kind (STR-RBR-006), each worked out in its own host: \
     `mode` host (each host's frame) or face (bars at a face moved to `face_ref`). Returns the new \
     sets and the hosts where a set does not fit, with why.",
    "Propagate Rebar",
    crate::rebar_edit::PropagateParams,
    crate::rebar_edit::PropagateOut,
    crate::rebar_edit::propagate
);

edit_command!(
    RebarPickNewHost,
    "rebar.pick_new_host",
    "Move rebar sets to a new host",
    "Moves rebar sets to another host that can hold rebar (STR-RBR-006; as `rebar.rehost`).",
    "Pick New Host",
    crate::rebar::RehostParams,
    (),
    crate::rebar::rehost
);

edit_command!(
    RebarSplice,
    "rebar.splice",
    "Splice a rebar set",
    "Splices a set of straight bars (STR-RBR-015) by `length` from their start or where a plan \
     `line` crosses them: the set keeps the first part, a new set takes the second, overlapping by \
     `lap` (default 50 diameters); `stagger` moves every second bar's splice. The two change \
     together. Returns both.",
    "Splice",
    crate::rebar_edit::SpliceParams,
    Vec<ElementId>,
    crate::rebar_edit::splice
);

edit_command!(
    RebarCouplerPlace,
    "rebar_coupler.place",
    "Place a rebar coupler",
    "Places a coupler (IfcMechanicalFastener, COUPLER) on one bar end or between two (STR-RBR-015): \
     `bar_end_refs` each a set, bar (from 0) and end (start, end). Two ends must be the same size, \
     at most 10 diameters apart and 3 off line. Sized to the bar; deleted with its bars.",
    "Coupler",
    crate::rebar_edit::CouplerParams,
    ElementId,
    crate::rebar_edit::place_coupler
);

read_command!(
    RebarCouplerInfo,
    "rebar_coupler.info",
    "Rebar couplers",
    "Each coupler's sets, diameter, length and axis (STR-RBR-015).",
    crate::rebar_edit::CouplerInfoParams,
    Vec<crate::rebar_edit::CouplerInfo>,
    crate::rebar_edit::coupler_info
);

read_command!(
    CustomList,
    "custom_connection.list",
    "Custom connection types",
    "The project's custom connection types (STR-STL-003): id, name, members' roles and section \
     families, nested connections, plates, patterns and welds, exposed parameters, sub-element names \
     and instances.",
    Option<serde_json::Value>,
    Vec<crate::custom_connections::TypeInfo>,
    |s, _| Ok(crate::custom_connections::list(s))
);

read_command!(
    LoadCaseList,
    "load_case.list",
    "Load natures, cases, combinations and supports",
    "The project's code template and load natures, its load cases with their load counts, its \
     combinations with each case's factor (nested combinations expanded), and its supported nodes.",
    Option<serde_json::Value>,
    crate::loads::Report,
    |s, _| Ok(crate::loads::report(s))
);

read_command!(
    StructuralAnalysisResults,
    "structural_analysis.results",
    "The structural analysis results",
    "The last collected structural analysis (ANA-FEA-001): its job, time, provider, whether the model \
     changed since (stale), warnings, a summary per case and combination (largest displacement and \
     moment, total vertical reaction), and each member's internal forces at stations in `case` (default \
     the first): N, Vy, Vz, T, My, Mz in its local axes (N, N·m).",
    Option<crate::structural_model::ReportParams>,
    crate::structural_model::Report,
    |s, p| crate::structural_model::report(s, &p.unwrap_or_default())
);

read_command!(
    BeamInfo,
    "beam.info",
    "Beams' usage, length and supports",
    "For each beam: its structural usage (resolved from its supports when automatic) and whether it is \
     locked, its cut length, what each end rests on (a column, wall or beam), and its section's \
     designation and material.",
    crate::beams::InfoParams,
    Vec<crate::beams::Info>,
    |s, p| crate::beams::info(s, &p)
);

read_command!(
    MeasureBetween,
    "measure.between",
    "Measure between two points",
    "The distance between two points ([x, y] or [x, y, z], metres) (ARC-MOD-023): its x, y and z \
     components, the plan distance and its angle from the x axis (degrees). Changes nothing.",
    crate::measure::BetweenParams,
    crate::measure::Between,
    |_, p| crate::measure::between(p)
);

read_command!(
    MeasureChain,
    "measure.chain",
    "Measure a chain of points",
    "The lengths of the segments of a chain of points, and their total (ARC-MOD-023).",
    crate::measure::ChainParams,
    crate::measure::Chain,
    |_, p| crate::measure::chain(p)
);

read_command!(
    MeasureAlong,
    "measure.along",
    "Measure along an element",
    "The length of a wall (along its location curve), curtain wall, ramp (its path) or grid line \
     (ARC-MOD-023).",
    crate::measure::AlongParams,
    f64,
    crate::measure::along
);

edit_command!(
    SkylightCreate,
    "skylight.create",
    "Place a skylight in a roof",
    "Places a skylight in a Reviera footprint roof at plan `point`, cutting its own opening: a `roof_window` \
     in a pitched roof (in the slope; `width` across it, `length` up it), or on a flat roof on a `curb` \
     a `flat` glass sheet, a `pyramid`, or a `ridge_lantern` (two glass slopes at `pitch` to a ridge along \
     its `length`, `rotation` from the x axis), whose panes are re-mapped from its length so none is wider \
     than `max_pane`. Returns the skylight (an IfcWindow, SKYLIGHT).",
    "Skylight",
    crate::skylights::CreateParams,
    ElementId,
    crate::skylights::create
);

edit_command!(
    SkylightSet,
    "skylight.set",
    "Change skylights",
    "Changes skylights' centre, style, size, rotation, curb, pitch or largest pane; omitted values are kept.",
    "Edit Skylight",
    crate::skylights::SetParams,
    (),
    crate::skylights::set
);

read_command!(
    MeshObjectInspect,
    "mesh_object.inspect",
    "Inspect a mesh file",
    "Reads a mesh file (OBJ with its MTL, STL, PLY, glTF, GLB) with import `options` (units, up axis, \
     insertion point, clean-up) and reports what it holds without importing it (DM-IMP-015): format, \
     triangles, vertices, materials, its size in metres, the units and up axis used and whether the file \
     stated them, open and non-manifold edges and whether it is a closed solid.",
    [Effect::Read, Effect::FileRead],
    crate::mesh_objects::InspectParams,
    crate::mesh_objects::Report,
    crate::mesh_objects::inspect
);

edit_command!(
    MeshObjectImport,
    "mesh_object.import",
    "Import a mesh as a custom object type",
    "Imports a mesh file (OBJ, STL, PLY, glTF, GLB) as a custom object type (DM-IMP-015): the mesh is \
     stored once in the type, in metres and Z up, with its insertion point (bottom centre by default) at \
     the origin; each material becomes a slot with its colour. `category` (generic_model, furniture, \
     column, plumbing_fixture, lighting_fixture, planting) decides the IFC classes. Meshes above \
     1,000,000 triangles need `allow_large`; above 5,000,000 they are refused. Returns the type; place \
     copies with mesh_object.place.",
    "Import Object",
    [Effect::Edit, Effect::FileRead],
    crate::mesh_objects::ImportParams,
    ElementId,
    crate::mesh_objects::import
);

edit_command!(
    MeshObjectReload,
    "mesh_object.reload",
    "Reload a custom object type from its file",
    "Re-reads a custom object type's mesh from its source file, or from `path`, with its own or new \
     import `options`. Every copy changes; slot colours are kept where slot names still match.",
    "Reload from File",
    [Effect::Edit, Effect::FileRead],
    crate::mesh_objects::ReloadParams,
    (),
    crate::mesh_objects::reload
);

edit_command!(
    MeshObjectPlace,
    "mesh_object.place",
    "Place a custom object",
    "Places a copy of a custom object type at plan `point` on `level`, `offset` above it, turned by \
     `rotation` and optionally mirrored. Copies share the type's mesh. Returns the copy.",
    "Place Object",
    crate::mesh_objects::PlaceParams,
    ElementId,
    crate::mesh_objects::place
);

edit_command!(
    MeshObjectSet,
    "mesh_object.set",
    "Change custom objects",
    "Changes custom objects' point, level, offset, rotation or mirror; omitted values are kept.",
    "Edit Object",
    crate::mesh_objects::SetParams,
    (),
    crate::mesh_objects::set
);

edit_command!(
    MeshObjectSetType,
    "mesh_object.set_type",
    "Change a custom object type",
    "Changes a custom object type's name, uniform `scale` (every copy) and slot colours, and its \
     `category` while it has no copies.",
    "Edit Object Type",
    crate::mesh_objects::SetTypeParams,
    (),
    crate::mesh_objects::set_type
);

read_command!(
    MeshObjectTypes,
    "mesh_object.types",
    "List custom object types",
    "The project's custom object types: name, category, triangles, size, scale, material slots, source \
     file and how many copies each has.",
    crate::mesh_objects::TypesParams,
    Vec<crate::mesh_objects::TypeInfo>,
    crate::mesh_objects::list_types
);

read_command!(
    LibraryCreate,
    "library.create",
    "Make a Component Library",
    "Makes a Component Library (DM-LIB-004) at `path`: the folder (made when missing) with a \
     `reviera-library.json` manifest in its root. A folder that already holds 3D files becomes a library \
     as it is; a manifest already there is kept. Returns the library's summary.",
    [Effect::FileRead, Effect::FileWrite],
    crate::library::CreateParams,
    crate::library::LibraryInfo,
    crate::library::create
);

read_command!(
    LibraryOpen,
    "library.open",
    "Open a Component Library",
    "Opens the Component Library at `path` (a folder with a reviera-library.json): its id, name and item \
     count. A folder without a manifest is refused (make one with library.create).",
    [Effect::FileRead],
    crate::library::OpenParams,
    crate::library::LibraryInfo,
    crate::library::open_library
);

read_command!(
    LibraryList,
    "library.list",
    "List a library folder",
    "A Component Library folder's sub-folders (with how many items are in and below each) and items: \
     mesh files (objects) and IFC files (fragments), each with its name, category, tags and format from \
     the manifest or its folder's settings. `recursive` lists every item below the folder.",
    [Effect::FileRead],
    crate::library::ListParams,
    crate::library::Listing,
    crate::library::list
);

read_command!(
    LibraryImportFiles,
    "library.import_files",
    "Import files into a library",
    "Import into Library: copies `files` (OBJ with its MTL and textures, STL, PLY, glTF with its buffers \
     and images, GLB, IFC) into `folder` of the library and adds their entries to its manifest, with \
     `details` (name, category, tags, import options, product data). A file whose content is already in \
     the library is skipped unless `import_duplicates`; a name in use gets a suffix. Returns what was \
     imported and skipped.",
    [Effect::FileRead, Effect::FileWrite],
    crate::library::ImportFilesParams,
    crate::library::ImportReport,
    crate::library::import_files
);

read_command!(
    LibraryGetMeta,
    "library.get_meta",
    "Get a library item's details",
    "A library item's details from the manifest (what its file's format may not hold) and the settings \
     it inherits from its folder.",
    [Effect::FileRead],
    crate::library::GetMetaParams,
    crate::library::ItemMeta,
    crate::library::get_meta
);

read_command!(
    LibrarySetMeta,
    "library.set_meta",
    "Set library items' details",
    "Sets items' details in the library's manifest (name, description, category, tags, import options, \
     product, provenance, properties; null clears one), or a folder's settings (paths ending in /). The \
     files themselves are not changed.",
    [Effect::FileRead, Effect::FileWrite],
    crate::library::SetMetaParams,
    (),
    crate::library::set_meta
);

read_command!(
    LibraryDeleteItems,
    "library.delete_items",
    "Delete library items",
    "Moves items (and companion files no other item uses) to the library's .deleted folder with their \
     manifest entries; library.restore_items brings them back.",
    [Effect::FileRead, Effect::FileWrite],
    crate::library::DeleteItemsParams,
    Vec<String>,
    crate::library::delete_items
);

read_command!(
    LibraryRestoreItems,
    "library.restore_items",
    "Restore deleted library items",
    "Brings items deleted with library.delete_items back to where they were, with their details.",
    [Effect::FileRead, Effect::FileWrite],
    crate::library::DeleteItemsParams,
    Vec<String>,
    crate::library::restore_items
);

edit_command!(
    ComponentPlace,
    "component.place",
    "Place components",
    "Place Component: one copy at each of `points` on `level` (with `offset`, `rotation`, `mirror`) of a \
     custom object type in the project (`type_id`), or of a Component Library item (`library` and \
     `item`), which is loaded into the project as a type the first time and reused after. Returns the \
     copies.",
    "Place Component",
    [Effect::Edit, Effect::FileRead],
    crate::library::PlaceParams,
    Vec<ElementId>,
    crate::library::place
);

edit_command!(
    ArrayAlongPath,
    "array.along_path",
    "Array along a path",
    "Array along Path (ARC-MOD-033): repeats components (custom object types, or library items loaded as \
     types; several with weights for a random choice) along a path: drawn `points` (polyline, closed or \
     not; `shape` arc through three points; spline through points), a wall or grid `element`, or a floor's, \
     ceiling's or deck's `host_edges` (copies on its top). `distribution`: number, fixed distance (with \
     `justify`), max or min spacing; `counts` per side with corners. `corners`: through, every_corner, \
     skip. `orientation`: follow, fixed, outward, inward, plus `rotation_offset`. `variation`: seeded \
     random rotation and scale ranges and jitter across the path. The array stays parametric: its \
     copies regenerate when the path element, count or component changes. Returns the array.",
    "Array along Path",
    [Effect::Edit, Effect::FileRead],
    crate::path_arrays::AlongPathParams,
    ElementId,
    crate::path_arrays::along_path
);

edit_command!(
    ArraySet,
    "array.set",
    "Change a path array",
    "Changes a path array's components, path, level, offset, distribution, ends, insets, corners, \
     orientation, variation or height; `reseed` picks a new random arrangement. Omitted values are kept.",
    "Edit Array",
    [Effect::Edit, Effect::FileRead],
    crate::path_arrays::SetParams,
    (),
    crate::path_arrays::set
);

edit_command!(
    ArrayExcludeMember,
    "array.exclude_member",
    "Exclude an array member",
    "Leaves a gap at a path array member's place (by index), for an entrance between two columns, say.",
    "Exclude Member",
    crate::path_arrays::MemberParams,
    (),
    crate::path_arrays::exclude_member
);

edit_command!(
    ArrayRestoreMember,
    "array.restore_member",
    "Restore array members",
    "Restores an excluded path array member (by index), or all of them (`\"all\"`).",
    "Restore Member",
    crate::path_arrays::MemberParams,
    (),
    crate::path_arrays::restore_member
);

read_command!(
    ArrayMembers,
    "array.members",
    "List a path array's members",
    "A path array's members as they are now: index, point, height, rotation, scale and component type.",
    crate::path_arrays::ArrayParams,
    Vec<crate::path_arrays::Member>,
    crate::path_arrays::list_members
);

edit_command!(
    ArrayUnarray,
    "array.unarray",
    "Unarray",
    "Turns a path array's members into independent custom objects where they stand, and deletes the \
     array. Returns the new objects.",
    "Unarray",
    crate::path_arrays::ArrayParams,
    Vec<ElementId>,
    crate::path_arrays::unarray
);

edit_command!(
    RefPlaneCreate,
    "refplane.create",
    "Create a reference plane",
    "Creates a reference plane (ARC-REFP-001): an unbounded construction plane through the line from \
     `start` to `end` and containing the direction of the view it is drawn in (`view_normal`; up for a \
     plan, so the plane is vertical). Points are `[x, y]` (at `z`, or the `level`'s elevation, else 0) \
     or `[x, y, z]`. A named plane (names are unique) is offered as a work plane. Written as an \
     IfcAnnotation (ObjectType ReferencePlane) with its line as the Axis representation. Returns the \
     plane.",
    "Reference Plane",
    crate::ref_planes::CreateParams,
    ElementId,
    crate::ref_planes::create
);

edit_command!(
    RefPlaneRename,
    "refplane.rename",
    "Rename a reference plane",
    "Renames a reference plane; an empty name unnames it (it then no longer appears among the named \
     work planes).",
    "Rename Reference Plane",
    crate::ref_planes::RenameParams,
    (),
    crate::ref_planes::rename
);

edit_command!(
    RefPlaneSet,
    "refplane.set",
    "Change a reference plane",
    "Moves a reference plane's line ends (`[x, y]` keeps their heights) or changes its subcategory \
     (empty clears it). Model lines on the plane follow it.",
    "Edit Reference Plane",
    crate::ref_planes::SetParams,
    (),
    crate::ref_planes::set
);

read_command!(
    RefPlaneList,
    "refplane.list",
    "List reference planes",
    "Every reference plane: its name, line, view direction, normal and subcategory.",
    crate::builtin::NoParams,
    Vec<crate::ref_planes::RefPlaneInfo>,
    crate::ref_planes::list_command
);

edit_command!(
    WorkPlaneSet,
    "workplane.set",
    "Set a view's work plane",
    "Sets the active work plane of a view (ARC-WP-001), which the Model Line tool and other \
     work-plane tools draw on: one of `level` (with an `offset`), `grid`, `ref_plane` or an explicit \
     `plane` `{origin, x_dir, normal}`. A plan's work plane is its level until one is set.",
    "Set Work Plane",
    crate::work_planes::SetParams,
    crate::work_planes::WorkPlaneInfo,
    crate::work_planes::set
);

read_command!(
    WorkPlaneGet,
    "workplane.get",
    "Get a view's work plane",
    "A view's active work plane: what defines it, a label, the resolved plane (origin, x, y, normal), \
     and whether its grid is shown.",
    crate::work_planes::ViewParams,
    crate::work_planes::WorkPlaneInfo,
    crate::work_planes::get
);

edit_command!(
    WorkPlaneShow,
    "workplane.show",
    "Show or hide the work plane",
    "Shows or hides a view's work plane as a grid (with `grid_spacing`, metres).",
    "Show Work Plane",
    crate::work_planes::ShowParams,
    (),
    crate::work_planes::show
);

read_command!(
    WorkPlaneList,
    "workplane.list",
    "List the named work planes",
    "The planes a work plane can be chosen from by name: levels, grids and named reference planes.",
    crate::builtin::NoParams,
    Vec<crate::work_planes::NamedPlane>,
    crate::work_planes::list_named
);

edit_command!(
    ElementSetWorkPlane,
    "element.set_work_plane",
    "Move elements to another work plane",
    "Puts model lines on another work plane (one of `level` with `offset`, `grid`, `ref_plane` or \
     `plane`), projected onto it where they stand; they then follow that plane.",
    "Pick New Work Plane",
    crate::work_planes::SetElementParams,
    (),
    crate::work_planes::set_element
);

edit_command!(
    ElementDissociateWorkPlane,
    "element.dissociate_work_plane",
    "Dissociate elements from their work plane",
    "Fixes model lines where they are, on a plane that no longer follows its level, grid or \
     reference plane.",
    "Dissociate",
    crate::work_planes::ElementsParams,
    (),
    crate::work_planes::dissociate
);

edit_command!(
    ModelLineCreate,
    "modelline.create",
    "Draw model lines",
    "Draws model lines (ARC-MLIN-001): 3D lines on a work plane, shown in every view. Each curve \
     (`line` with optional `through` points: one makes an arc, more a spline) is in the work plane's \
     coordinates: on a level, plan x and y; on a vertical plane, along its line from its start and up. \
     The plane is one of `level` (with `offset`), `grid`, `ref_plane` or `plane` (the lowest level when \
     omitted). `line_style` is one of `modelline.styles`. The lines follow their plane. Written as \
     IfcAnnotation (ObjectType ModelLine) with a styled 3D polyline. Returns one line per curve.",
    "Model Lines",
    crate::model_lines::CreateParams,
    Vec<ElementId>,
    crate::model_lines::create
);

edit_command!(
    ModelLineSet,
    "modelline.set",
    "Change model lines",
    "Changes model lines' line style, or one line's curve (work plane coordinates).",
    "Edit Model Lines",
    crate::model_lines::SetParams,
    (),
    crate::model_lines::set
);

read_command!(
    ModelLineList,
    "modelline.list",
    "List model lines",
    "Every model line as it is now: its world polyline, line style and work plane.",
    crate::builtin::NoParams,
    Vec<crate::model_lines::ModelLineInfo>,
    crate::model_lines::list_command
);

read_command!(
    ModelLineStyles,
    "modelline.styles",
    "List line styles",
    "The line styles model lines can use: name, pen, pattern and colour.",
    crate::builtin::NoParams,
    Vec<crate::model_lines::LineStyle>,
    crate::model_lines::styles_command
);

edit_command!(
    DatumSetExtent,
    "datum.set_extent",
    "Move a datum's end",
    "Moves one end (`start` or `end`) of a level, grid or reference plane (ARC-DATUM-001). Without a \
     view, `point` is `[x, y]` in plan and sets the 3D (model) extent shared by every view. In a plan \
     view it sets that view's 2D extent; in a section or elevation `point` is `[across, height]` and \
     sets the view's range (a grid's heights, a level's ends). `model: true` sets the 3D extent from \
     the view instead.",
    "Datum Extent",
    crate::datum_extents::SetExtentParams,
    (),
    crate::datum_extents::set_extent
);

edit_command!(
    DatumResetTo3d,
    "datum.reset_to_3d",
    "Reset to 3D extents",
    "Removes the datums' 2D extents in a view, so they show their 3D extents there again.",
    "Reset to 3D Extents",
    crate::datum_extents::ViewDatumsParams,
    (),
    crate::datum_extents::reset_to_3d
);

edit_command!(
    DatumMaximize3d,
    "datum.maximize_3d",
    "Maximize 3D extents",
    "Stretches grids and reference planes over the model's bounding box (in plan and in height, with \
     a 1 m margin); a level crosses every view again.",
    "Maximize 3D Extents",
    crate::datum_extents::DatumsParams,
    (),
    crate::datum_extents::maximize_3d
);

edit_command!(
    DatumPropagateExtents,
    "datum.propagate_extents",
    "Propagate extents",
    "Copies the datums' 2D extents, bubbles and elbows in one view to other views: plans to plans, \
     sections and elevations to parallel ones.",
    "Propagate Extents",
    crate::datum_extents::PropagateParams,
    (),
    crate::datum_extents::propagate
);

edit_command!(
    DatumSetBubble,
    "datum.set_bubble",
    "Show or hide a datum's bubble",
    "Shows or hides a datum's bubble at one end (`start` or `end`) in one view.",
    "Datum Bubble",
    crate::datum_extents::BubbleParams,
    (),
    crate::datum_extents::set_bubble
);

edit_command!(
    DatumAddElbow,
    "datum.add_elbow",
    "Add an elbow to a grid",
    "Adds a jog near a grid's bubble at one end in one plan: the bubble moves `offset` paper \
     millimetres to the left of the line's direction (negative: right; 5 by default; 0 removes it).",
    "Add Elbow",
    crate::datum_extents::ElbowParams,
    (),
    crate::datum_extents::add_elbow
);

edit_command!(
    DatumAssignScopeBox,
    "datum.assign_scope_box",
    "Assign datums to a scope box",
    "Assigns levels, grids and reference planes to a scope box (ARC-SCOPE-001), or clears it (`null`). \
     They then show only in views whose plane crosses the box, and grid and reference plane lines are \
     clipped to its footprint and follow it. Cleared, they keep their extents.",
    "Assign Scope Box",
    crate::datum_extents::AssignParams,
    (),
    crate::datum_extents::assign_scope_box
);

read_command!(
    DatumGet,
    "datum.get",
    "Get a datum's extents",
    "A datum's 3D extent (line, heights, plan extent, scope box) and, with a view, its 2D extent there \
     (`mode` 3D or 2D), bubbles and elbows.",
    crate::datum_extents::GetParams,
    crate::datum_extents::DatumState,
    crate::datum_extents::get
);

edit_command!(
    ScopeBoxCreate,
    "scopebox.create",
    "Create a scope box",
    "Creates a named scope box (ARC-SCOPE-001) from a rectangle in plan (two opposite corners, square to \
     an optional `rotation`), from a level (or below the lowest) up `height` (or above the highest level). \
     Written as an IfcAnnotation (ObjectType ScopeBox) with a Box representation. Returns the box.",
    "Scope Box",
    crate::scope_boxes::CreateParams,
    ElementId,
    crate::scope_boxes::create
);

edit_command!(
    ScopeBoxSetExtents,
    "scopebox.set_extents",
    "Change a scope box",
    "Resizes a scope box to two opposite corners `min` and `max` (`[x, y, z]`, square to its rotation), \
     turns it (`rotation`) or renames it. Its datums follow.",
    "Edit Scope Box",
    crate::scope_boxes::SetExtentsParams,
    (),
    crate::scope_boxes::set_extents
);

read_command!(
    ScopeBoxList,
    "scopebox.list",
    "List scope boxes",
    "Every scope box: name, centre, size, rotation, heights and the datums assigned to it.",
    crate::builtin::NoParams,
    Vec<crate::scope_boxes::ScopeBoxInfo>,
    crate::scope_boxes::list_command
);

edit_command!(
    ViewAssignScopeBox,
    "view.assign_scope_box",
    "Assign views to a scope box",
    "Records the scope box of views (or clears it with `null`); their crop regions will follow it.",
    "Assign Scope Box",
    crate::scope_boxes::AssignViewsParams,
    (),
    crate::scope_boxes::assign_views
);

edit_command!(
    ConstraintLockDimension,
    "constraint.lock_dimension",
    "Lock a dimension",
    "Locks a dimension's segment (or every segment) at its current value (ARC-CNST-001): from then on \
     moving one of its references moves the other with it. Returns the new constraints.",
    "Lock Dimension",
    crate::constraints::LockParams,
    Vec<ElementId>,
    crate::constraints::lock_dimension
);

edit_command!(
    ConstraintUnlockDimension,
    "constraint.unlock_dimension",
    "Unlock a dimension",
    "Removes the locks of a dimension's segment, or of all its segments.",
    "Unlock Dimension",
    crate::constraints::LockParams,
    (),
    crate::constraints::unlock_dimension
);

edit_command!(
    ConstraintSetEquality,
    "constraint.set_equality",
    "Make a dimension's segments equal (EQ)",
    "Turns EQ on (the dimension's inner references move so its segments are equal, and stay so) or \
     off. Needs a dimension with at least two segments. Returns the constraint when on.",
    "EQ",
    crate::constraints::EqualityParams,
    Option<ElementId>,
    crate::constraints::set_equality
);

edit_command!(
    ConstraintLockAlignment,
    "constraint.lock_alignment",
    "Lock an alignment",
    "Locks two aligned, parallel references together (references as for `dim.create_aligned`: \
     `face:ext`, `face:int` or `axis` of walls, `axis` of grids and reference planes). They then move \
     together. Returns the constraint.",
    "Lock Alignment",
    crate::constraints::AlignmentParams,
    ElementId,
    crate::constraints::lock_alignment
);

edit_command!(
    ConstraintDelete,
    "constraint.delete",
    "Delete constraints",
    "Deletes constraints (Remove Constraints); the elements stay where they are.",
    "Remove Constraints",
    crate::constraints::DeleteParams,
    (),
    crate::constraints::delete
);

read_command!(
    ConstraintList,
    "constraint.list",
    "List constraints",
    "The model's constraints (or those on one element): kind, references, direction, locked value, \
     the dimension that made it, and whether it holds now.",
    crate::constraints::ListParams,
    Vec<crate::constraints::ConstraintInfo>,
    crate::constraints::list
);

read_command!(
    DimTemporaryQuery,
    "dim.temporary_query",
    "Temporary dimensions of an element",
    "The temporary dimensions of an element (ARC-DIM-001): from a straight wall's location line, a grid or \
     a reference plane to the nearest parallel wall, grid or reference plane on each side (within 30 m); \
     from a door's or window's centre along its wall to the nearest crossing walls. Each has its ends, \
     value, the other reference and the direction that lengthens it.",
    crate::temp_dims::QueryParams,
    Vec<crate::temp_dims::TempDim>,
    crate::temp_dims::query
);

edit_command!(
    DimTemporarySet,
    "dim.temporary_set",
    "Drive a temporary dimension",
    "Moves an element so its temporary dimension `index` has `value` (metres); the other reference \
     stays. Doors and windows slide along their wall.",
    "Move",
    crate::temp_dims::SetParams,
    (),
    crate::temp_dims::set
);

edit_command!(
    DimMakePermanent,
    "dim.make_permanent",
    "Make a temporary dimension permanent",
    "Creates an aligned dimension in a view in place of an element's temporary dimension `index`. \
     Returns the dimension.",
    "Dimension",
    crate::temp_dims::PermanentParams,
    ElementId,
    crate::temp_dims::make_permanent
);

edit_command!(
    ViewRevealConstraints,
    "view.reveal_constraints",
    "Reveal constraints",
    "Shows (or hides) every constraint in a view, drawn in red (ARC-CNST-001).",
    "Reveal Constraints",
    crate::constraints::RevealParams,
    (),
    crate::constraints::reveal
);

edit_command!(
    RoofCreateExtrusion,
    "roof.create_extrusion",
    "Create a roof by extrusion",
    "Creates a roof by extrusion (ARC-ROOF-002): an open `profile` of lines and arcs (`through` points) \
     on a vertical work plane (`grid`, `ref_plane` or `plane`), in plane coordinates (along the plane; up \
     from the Reference Level `ref_level`, the highest by default, plus `level_offset`). The profile is \
     the roof's top; the roof's thickness hangs below it, extruded across the plane from `start` to \
     `end` (by default over the walls under the profile, else 0 to 6 m). One straight segment is a \
     shed roof, two a gable, arcs a barrel roof. Returns the roof.",
    "Roof by Extrusion",
    crate::roofs::extrusion::CreateParams,
    ElementId,
    crate::roofs::extrusion::create
);

edit_command!(
    RoofSetExtrusion,
    "roof.set_extrusion",
    "Change a roof by extrusion",
    "Changes a roof by extrusion's profile, Extrusion Start and End, or level offset; omitted values \
     are kept.",
    "Edit Roof",
    crate::roofs::extrusion::SetParams,
    (),
    crate::roofs::extrusion::set
);

edit_command!(
    RoofSetSlopeArrow,
    "roof.set_slope_arrow",
    "Put a slope arrow on a roof edge",
    "Slopes a footprint roof's gable edge with a slope arrow (ARC-ROOF-005): its `tail` on footprint \
     curve `key` (a gable edge: one that does not define slope) and its `head` inside the footprint. `specify` \
     is `slope` (the `slope` along the arrow; the roof's by default) or `height_at_tail` (the \
     `head_offset` above the plate). The edge's plane rises from the plate across the edge, at the \
     arrow's rise over its run square to the edge: an arrow from a gable to the ridge makes a hip.",
    "Slope Arrow",
    crate::roofs::SlopeArrowParams,
    (),
    crate::roofs::set_slope_arrow
);

edit_command!(
    RoofRemoveSlopeArrow,
    "roof.remove_slope_arrow",
    "Remove a slope arrow",
    "Removes the slope arrow of footprint curve `key`; the edge is a gable again.",
    "Remove Slope Arrow",
    crate::roofs::RemoveArrowParams,
    (),
    crate::roofs::remove_slope_arrow
);

edit_command!(
    RoofJoin,
    "roof.join",
    "Join a roof to a wall or roof",
    "Extends a footprint roof's edge (footprint curve `key`) to a wall parallel to it, ending at the \
     wall's nearer face, or to another roof, ending on that roof's top surface (ARC-ROOF-006). The \
     join follows the target. Not for dormers.",
    "Join Roof",
    crate::roofs::JoinParams,
    (),
    crate::roofs::join
);

edit_command!(
    RoofUnjoin,
    "roof.unjoin",
    "Unjoin a roof",
    "Returns a joined roof edge (or all of them) to its own end.",
    "Unjoin Roof",
    crate::roofs::UnjoinParams,
    (),
    crate::roofs::unjoin
);

edit_command!(
    SlabEdgeCreate,
    "slabedge.create",
    "Create a slab edge",
    "Sweeps a rectangular profile (`width` away from the slab × `depth` down; 0.3 × 0.45 m by default, \
     moved by `horizontal_offset` and `vertical_offset`) along a Reviera floor's outline curves (`keys`, \
     consecutive; the whole outline by default, a closed mitred run) (ARC-SLED-001). Written as an \
     IfcSlab (USERDEFINED, ObjectType SLABEDGE). It follows its floor. Returns the slab edge.",
    "Slab Edge",
    crate::slab_edges::CreateParams,
    ElementId,
    crate::slab_edges::create
);

edit_command!(
    SlabEdgeSet,
    "slabedge.set",
    "Change a slab edge",
    "Changes a slab edge's curves, profile size or offsets; omitted values are kept.",
    "Edit Slab Edge",
    crate::slab_edges::SetParams,
    (),
    crate::slab_edges::set
);

edit_command!(
    HostSweepAddSegments,
    "hostsweep.add_segments",
    "Add segments to a slab edge",
    "Adds its floor's outline curves (keys) to a slab edge's run; the run must stay one chain.",
    "Add Segments",
    crate::slab_edges::SegmentsParams,
    (),
    crate::slab_edges::add_segments
);

edit_command!(
    HostSweepRemoveSegments,
    "hostsweep.remove_segments",
    "Remove segments from a slab edge",
    "Removes outline curves (keys) from a slab edge's run; at least one stays.",
    "Remove Segments",
    crate::slab_edges::SegmentsParams,
    (),
    crate::slab_edges::remove_segments
);

edit_command!(
    HostSweepFlip,
    "hostsweep.flip",
    "Flip a slab edge's profile",
    "Mirrors a slab edge's profile `Horizontal`ly (into the slab) or `Vertical`ly (up from the top).",
    "Flip",
    crate::slab_edges::FlipParams,
    (),
    crate::slab_edges::flip
);

edit_command!(
    ShapeAddPoint,
    "shape.add_point",
    "Add a shape point",
    "Adds a point to a floor's top (ARC-SHAPE-001) at `xy`, `along_surface` (the surface's height \
     there plus `offset`, e.g. -0.05 for a drain) or `absolute` (`z`). The top is triangulated through \
     its corners, points and split lines, and the slab follows it. Returns the point's key (`p…`).",
    "Add Point",
    crate::shape_edit::AddPointParams,
    String,
    crate::shape_edit::add_point
);

edit_command!(
    ShapeAddSplitLine,
    "shape.add_split_line",
    "Add a split line",
    "Adds a split line through `points` on a floor's top, `along_surface` or `absolute`; it stays an \
     edge of the triangulation. Returns its key (`l…`).",
    "Add Split Line",
    crate::shape_edit::AddLineParams,
    String,
    crate::shape_edit::add_split_line
);

edit_command!(
    ShapeMove,
    "shape.move",
    "Raise or lower shape modifiers",
    "Raises (or lowers, negative `dz`) a floor's shape points (`p…`), split lines (`l…`) or outline \
     corners (`v1`… in outline order).",
    "Modify Sub Elements",
    crate::shape_edit::MoveParams,
    (),
    crate::shape_edit::move_keys
);

edit_command!(
    ShapeSetElevation,
    "shape.set_elevation",
    "Set shape modifiers' elevation",
    "Sets shape points, split lines or outline corners to an absolute elevation `z`.",
    "Modify Sub Elements",
    crate::shape_edit::ElevationParams,
    (),
    crate::shape_edit::set_elevation
);

edit_command!(
    ShapeReset,
    "shape.reset",
    "Reset Shape",
    "Removes every shape modifier of a floor: it is flat again.",
    "Reset Shape",
    crate::shape_edit::ElementParams,
    (),
    crate::shape_edit::reset
);

read_command!(
    ShapeGet,
    "shape.get",
    "Get a floor's shape modifiers",
    "A floor's shape modifiers: outline corner offsets, points and split lines (heights relative to its top).",
    crate::shape_edit::ElementParams,
    crate::shape_edit::Shape,
    crate::shape_edit::get
);

edit_command!(
    WallCreateBySegment,
    "wall.create_by_segment",
    "Line a wall",
    "A lining wall against `target_id`'s face on `side` (exterior or interior), as long as the face \
     after joins, with the target's base and top (ARC-WALL-014). `hosted` makes it follow the \
     target (ARC-WALL-016); `auto_join` (off, join, join_and_lock) joins its geometry to the target \
     so doors and windows cut both, and locks the faces together.",
    "Wall by Segment",
    crate::walls::linings::BySegmentParams,
    Vec<crate::id::ElementId>,
    crate::walls::linings::by_segment
);

edit_command!(
    WallCreateByRoom,
    "wall.create_by_room",
    "Line a room",
    "Lining walls round `room_id`'s boundary, one per straight segment, inside the room and joined \
     at the corners, as high as the room unless `height` is given (ARC-WALL-014). Their exterior \
     faces the walls unless `flip`. `hosted` hosts each on the wall it lines; `auto_join` as for \
     `wall.create_by_segment`.",
    "Walls by Room",
    crate::walls::linings::ByRoomParams,
    Vec<crate::id::ElementId>,
    crate::walls::linings::by_room
);

edit_command!(
    WallSetHost,
    "wall.set_host",
    "Host a wall on another",
    "The wall follows `host_wall_id` from now on (ARC-WALL-016): kept against the host's face on \
     `side` at `offset` (≥ 0; omitted, as now), its ends as far past the host's ends as now. It \
     must be straight and parallel to the host; cycles are refused.",
    "Pick New Host",
    crate::walls::linings::SetHostParams,
    (),
    crate::walls::linings::set_host
);

edit_command!(
    WallRemoveHost,
    "wall.remove_host",
    "Remove hosted relationships",
    "The walls stay where they are and no longer follow their hosts (ARC-WALL-016).",
    "Remove Hosted Relationship",
    crate::walls::linings::WallIdsParams,
    (),
    crate::walls::linings::remove_host
);

read_command!(
    WallSelectMainHost,
    "wall.select_main_host",
    "A hosted wall's host",
    "The wall a hosted wall follows (ARC-WALL-016), or null.",
    [Effect::Read],
    crate::walls::linings::MainHostParams,
    Option<crate::id::ElementId>,
    |store, p| crate::walls::linings::main_host(store, p)
);

edit_command!(
    GeometryJoin,
    "geometry.join",
    "Join geometry",
    "Joins two walls that touch face to face (ARC-WALL-015): doors, windows and openings in either \
     cut both. `lock` also locks the touching faces together.",
    "Join Geometry",
    crate::walls::linings::JoinParams,
    (),
    crate::walls::linings::join
);

edit_command!(
    GeometryUnjoin,
    "geometry.unjoin",
    "Unjoin geometry",
    "Removes Join Geometry between the given walls, or all of one wall's joins.",
    "Unjoin Geometry",
    crate::walls::linings::UnjoinParams,
    (),
    crate::walls::linings::unjoin
);

edit_command!(
    CompoundSetExtendable,
    "compound.set_extendable",
    "Unlock a layer's edges",
    "Unlocks (true) or locks a wall type's layer `index` (0 the exterior layer) at its `top` or \
     `base` (ARC-CMPD-004): unlocked edges move by walls' Top and Base Extension Distances. The \
     unlocked layers at each end must be adjacent.",
    "Extendable Layer",
    crate::compound::SetExtendableParams,
    (),
    crate::compound::set_extendable
);

edit_command!(
    CompoundSplitRegion,
    "compound.split_region",
    "Split a layer into a region",
    "Splits wall type layer `layer`'s column (ARC-CMPD-003): the part within `distance` of the \
     wall's base or top (`from`) becomes a region in its own `material`; on a shorter wall it is \
     clipped. Returns the region's index.",
    "Split Region",
    crate::compound::SplitRegionParams,
    usize,
    crate::compound::split_region
);

edit_command!(
    CompoundSetRegions,
    "compound.set_regions",
    "Set a wall type's regions",
    "Replaces a wall type's regions (ARC-CMPD-003): each `{layer, from: base|top, distance, \
     material, row?}`.",
    "Regions",
    crate::compound::SetRegionsParams,
    (),
    crate::compound::set_regions
);

edit_command!(
    CompoundMergeRegions,
    "compound.merge_regions",
    "Merge a region back",
    "Merges a wall type's region back into its layer (ARC-CMPD-003).",
    "Merge Regions",
    crate::compound::RegionParams,
    (),
    crate::compound::merge_regions
);

edit_command!(
    CompoundSetRegionAnchor,
    "compound.set_region_anchor",
    "Anchor a region",
    "Sets a region's height: `distance` from the wall's base or top (ARC-CMPD-003).",
    "Region Anchor",
    crate::compound::RegionAnchorParams,
    (),
    crate::compound::set_region_anchor
);

edit_command!(
    CompoundAssignLayer,
    "compound.assign_layer",
    "Assign a layer row to regions",
    "The regions take layer row `layer_index`'s material (ARC-CMPD-003).",
    "Assign Layers",
    crate::compound::AssignLayerParams,
    (),
    crate::compound::assign_layer
);

edit_command!(
    CompoundSetWrapping,
    "compound.set_wrapping",
    "Set a wall type's wrapping",
    "Wrapping at Inserts (none, exterior, interior, both) and at Ends (none, exterior, interior) \
     for a wall type (ARC-WRAP-001): its layers with Wraps line openings' jambs back to the core, \
     and turn round free ends.",
    "Wrapping",
    crate::compound::SetWrappingParams,
    (),
    crate::compound::set_wrapping
);

edit_command!(
    WallSetExtension,
    "wall.set_extension",
    "Extend walls' layers",
    "Sets walls' Top and Base Extension Distances (ARC-CMPD-004): the layers whose edges are \
     unlocked in the type run that far past the top (positive up) or base (negative down). Not \
     while the top is attached.",
    "Layer Extension",
    crate::walls::layered::SetExtensionParams,
    (),
    crate::walls::layered::set_extension
);

edit_command!(
    CurtainAddGrid,
    "curtain.add_grid",
    "Add a curtain grid line",
    "A manual grid line on a curtain wall (ARC-CW-004): `direction` u (vertical, at `position` along \
     the wall from its start) or v (horizontal, `position` above its base); `mode` all segments, one \
     (`segments[0]`) or all_except (`segments`). Returns its key.",
    "Curtain Grid",
    crate::curtain::AddGridParams,
    u32,
    crate::curtain::add_grid
);

edit_command!(
    CurtainSetGridSegments,
    "curtain.set_grid_segments",
    "Add or remove grid line segments",
    "Sets which segments of a curtain wall's grid line (by `direction` and `position`, a type's or a \
     manual line) are on (ARC-CW-004): removing one merges the panels on either side into one; \
     restoring it splits them again.",
    "Add/Remove Segments",
    crate::curtain::SegmentsParams,
    (),
    crate::curtain::set_grid_segments
);

edit_command!(
    CurtainMoveGrid,
    "curtain.move_grid",
    "Move a curtain grid line",
    "Moves a manual grid line to `position` (ARC-CW-004).",
    "Move Grid Line",
    crate::curtain::MoveGridParams,
    (),
    crate::curtain::move_grid
);

edit_command!(
    CurtainDeleteGrid,
    "curtain.delete_grid",
    "Delete a curtain grid line",
    "Deletes a manual grid line (ARC-CW-004).",
    "Delete Grid Line",
    crate::curtain::DeleteGridParams,
    (),
    crate::curtain::delete_grid
);

edit_command!(
    ColorSchemeCreate,
    "colorscheme.create",
    "Create a colour scheme",
    "A colour scheme (ARC-CLR-001) for `category` cat.rooms (default) or cat.areas, colouring by \
     `parameter` (Name, Number, Area, Perimeter or a parameter's name) `mode` value or range \
     (`ranges`: the rows' lower bounds, SI). Returns its key. Show it with view.set_color_scheme.",
    "Color Scheme",
    crate::color_schemes::CreateParams,
    String,
    crate::color_schemes::create
);

edit_command!(
    ColorSchemeSetRows,
    "colorscheme.set_rows",
    "Set a colour scheme's rows",
    "Replaces a colour scheme's rows (`{value | at_least, color: \"#rrggbb\", visible, caption?}`), \
     and optionally its title, parameter and mode. By value, values with no row are still coloured \
     from the palette.",
    "Edit Color Scheme",
    crate::color_schemes::SetRowsParams,
    (),
    crate::color_schemes::set_rows
);

edit_command!(
    ColorSchemeDuplicate,
    "colorscheme.duplicate",
    "Duplicate a colour scheme",
    "A copy of a colour scheme under a new name; returns its key.",
    "Duplicate Color Scheme",
    crate::color_schemes::DuplicateParams,
    String,
    crate::color_schemes::duplicate
);

edit_command!(
    ColorSchemeDelete,
    "colorscheme.delete",
    "Delete a colour scheme",
    "Deletes a colour scheme; plans that showed it show none.",
    "Delete Color Scheme",
    crate::color_schemes::DeleteParams,
    (),
    crate::color_schemes::delete
);

read_command!(
    ColorSchemeLegend,
    "colorscheme.legend",
    "A colour scheme's rows now",
    "The rows of a colour scheme as they apply now: stored rows and values found with no row yet \
     (palette colours), with whether each is in use.",
    [Effect::Read],
    crate::color_schemes::LegendParams,
    Vec<crate::color_schemes::RowNow>,
    |store, p| crate::color_schemes::legend(store, p)
);

edit_command!(
    ViewSetColorScheme,
    "view.set_color_scheme",
    "Show a colour scheme in a plan",
    "Fills a plan's rooms (or an area plan's areas) by a colour scheme (ARC-CLR-001); no \
     `scheme_id` clears it.",
    "Color Scheme",
    crate::color_schemes::SetViewParams,
    (),
    crate::color_schemes::set_view
);

edit_command!(
    AreaSchemeCreate,
    "areascheme.create",
    "Create an area scheme",
    "An area scheme (ARC-AREA-001) with its area types; returns its key. Gross Building and \
     Rentable exist from the start.",
    "Area Scheme",
    crate::areas::CreateSchemeParams,
    String,
    crate::areas::create_scheme
);

edit_command!(
    AreaSchemeSetRules,
    "areascheme.set_rules",
    "Set an area scheme's rules",
    "An area scheme's rule table (ARC-AREA-001): for area types `type_a` and `type_b` either side of \
     a picked wall boundary (`*` any, \"\" outside every area), the boundary's `position`: \
     centerline, face_a (the face toward type_a) or face_b. Optionally its area types.",
    "Area Rules",
    crate::areas::SetRulesParams,
    (),
    crate::areas::set_rules
);

read_command!(
    AreaSchemeList,
    "areascheme.list",
    "List area schemes",
    "The model's area schemes with their area types and rules.",
    [Effect::Read],
    crate::areas::SchemesParams,
    Vec<crate::areas::AreaScheme>,
    |store, p| crate::areas::list_schemes(store, p)
);

edit_command!(
    AreaPlanCreate,
    "areaplan.create",
    "Create area plans",
    "Area plans of `scheme_id` on levels (ARC-AREA-001); `auto_boundaries` adds boundaries on the \
     walls round the building's outside, placed by the scheme's rules.",
    "Area Plan",
    crate::areas::CreatePlanParams,
    Vec<crate::id::ElementId>,
    crate::areas::create_plan
);

edit_command!(
    AreaBoundaryCreate,
    "areaboundary.create",
    "Add area boundaries",
    "Area boundary lines in an area plan (ARC-AREA-001): drawn `lines` (fixed) or picked `wall_ids` \
     (they follow the walls), placed by the scheme's rules with `apply_rules`, else at `offset` from \
     the wall's centreline toward its exterior.",
    "Area Boundary",
    crate::areas::CreateBoundaryParams,
    Vec<crate::id::ElementId>,
    crate::areas::create_boundary
);

edit_command!(
    AreaPlace,
    "area.place",
    "Place an area",
    "An area at `point` in an area plan (ARC-AREA-001): its region is the loop of the scheme's \
     boundaries round it. `area_type` one of the scheme's types (default its first).",
    "Area",
    crate::areas::PlaceParams,
    crate::id::ElementId,
    crate::areas::place
);

edit_command!(
    AreaSetType,
    "area.set_type",
    "Change areas' type",
    "Changes areas' area type; boundaries placed by rules move to suit (ARC-AREA-001).",
    "Area Type",
    crate::areas::SetTypeParams,
    (),
    crate::areas::set_type
);

read_command!(
    AreaList,
    "area.list",
    "List areas",
    "Areas (of one scheme, or all) with their type, name, number and area (m²).",
    [Effect::Read],
    crate::areas::ListParams,
    Vec<crate::areas::AreaListItem>,
    |store, p| crate::areas::list(store, p)
);

edit_command!(
    GroupCreate,
    "group.create",
    "Create a group",
    "Makes the elements (and the doors and windows of walls among them) the first instance of a new group type (ARC-GRP-001); its origin is the middle of their extent. Returns the type's key and the instance.",
    "Create Group",
    crate::groups::CreateParams,
    crate::groups::Created,
    crate::groups::create
);

edit_command!(
    GroupPlace,
    "group.place",
    "Place a group",
    "A new instance of a group type with its origin at `point`, turned by `rotation`: its members are made from the type's template.",
    "Place Group",
    crate::groups::PlaceParams,
    crate::id::ElementId,
    crate::groups::place
);

edit_command!(
    GroupUngroup,
    "group.ungroup",
    "Ungroup",
    "Group instances go; their members stay as independent elements.",
    "Ungroup",
    crate::groups::InstancesParams,
    Vec<crate::id::ElementId>,
    crate::groups::ungroup
);

edit_command!(
    GroupSwapType,
    "group.swap_type",
    "Swap group type",
    "Instances become another group type at their origins.",
    "Swap Group",
    crate::groups::SwapParams,
    (),
    crate::groups::swap_type
);

edit_command!(
    GroupDuplicateType,
    "group.duplicate_type",
    "Duplicate a group type",
    "A copy of a group type under a new name; returns its key.",
    "Duplicate Group",
    crate::groups::TypeNameParams,
    String,
    crate::groups::duplicate_type
);

edit_command!(
    GroupRenameType,
    "group.rename_type",
    "Rename a group type",
    "Renames a group type and its instances.",
    "Rename Group",
    crate::groups::TypeNameParams,
    (),
    crate::groups::rename_type
);

edit_command!(
    GroupEdit,
    "group.edit",
    "Edit a group",
    "Opens the group editor on an instance (ARC-GRP-002): its members can then be changed, added (group.add_members) and removed (group.remove_members) until group.finish or group.cancel.",
    "Edit Group",
    crate::groups::InstanceParams,
    (),
    crate::groups::edit
);

edit_command!(
    GroupAddMembers,
    "group.add_members",
    "Add to the edited group",
    "Elements join the group being edited.",
    "Add to Group",
    crate::groups::MembersParams,
    (),
    crate::groups::add_members
);

edit_command!(
    GroupRemoveMembers,
    "group.remove_members",
    "Remove from the edited group",
    "Members leave the group being edited (they stay as elements).",
    "Remove from Group",
    crate::groups::MembersParams,
    (),
    crate::groups::remove_members
);

edit_command!(
    GroupFinish,
    "group.finish",
    "Finish editing a group",
    "The edited instance becomes its type's template and every other instance of the type is remade from it.",
    "Finish Group",
    crate::groups::NoParams,
    (),
    crate::groups::finish
);

edit_command!(
    GroupCancel,
    "group.cancel",
    "Cancel editing a group",
    "The edited instance is remade from its type as it was; elements added go back to being independent.",
    "Cancel Group",
    crate::groups::NoParams,
    (),
    crate::groups::cancel
);

edit_command!(
    GroupExcludeMember,
    "group.exclude_member",
    "Exclude a group member",
    "A member (by element or key) is gone from this instance only (ARC-GRP-003).",
    "Exclude",
    crate::groups::MemberParams,
    (),
    crate::groups::exclude_member
);

edit_command!(
    GroupRestoreMember,
    "group.restore_member",
    "Restore excluded members",
    "An excluded member (or all, without `member_key`) comes back.",
    "Restore Excluded",
    crate::groups::RestoreParams,
    (),
    crate::groups::restore_member
);

edit_command!(
    GroupMoveMemberToProject,
    "group.move_member_to_project",
    "Move a member to the project",
    "A member leaves this instance as an independent element; returns it.",
    "Move to Project",
    crate::groups::MemberParams,
    crate::id::ElementId,
    crate::groups::move_member_to_project
);

edit_command!(
    SelectionSetSave,
    "selectionset.save",
    "Save a selection",
    "Names a set of elements (ARC-SEL-002); members of groups join as their group. A taken name is refused unless `replace`. Returns its key.",
    "Save Selection",
    crate::selection_sets::SaveParams,
    String,
    crate::selection_sets::save
);

edit_command!(
    SelectionSetEdit,
    "selectionset.edit",
    "Edit a saved selection",
    "Adds and removes members of a saved selection.",
    "Edit Selection",
    crate::selection_sets::EditParams,
    (),
    crate::selection_sets::edit
);

edit_command!(
    SelectionSetDelete,
    "selectionset.delete",
    "Delete a saved selection",
    "Deletes a saved selection.",
    "Delete Selection",
    crate::selection_sets::SetParams,
    (),
    crate::selection_sets::delete
);

edit_command!(
    SelectionSetRename,
    "selectionset.rename",
    "Rename a saved selection",
    "Renames a saved selection.",
    "Rename Selection",
    crate::selection_sets::RenameParams,
    (),
    crate::selection_sets::rename
);

read_command!(
    GroupList,
    "group.list",
    "List groups",
    "Group types with their member counts and instances.",
    [Effect::Read],
    crate::groups::NoParams,
    Vec<crate::groups::TypeInfo>,
    |store, p| crate::groups::list(store, p)
);

read_command!(
    SelectionSetLoad,
    "selectionset.load",
    "Load a saved selection",
    "The members of a saved selection that still exist (select them with selection.set).",
    [Effect::Read],
    crate::selection_sets::SetParams,
    Vec<crate::id::ElementId>,
    |store, p| crate::selection_sets::load(store, p)
);

read_command!(
    SelectionSetList,
    "selectionset.list",
    "List saved selections",
    "Saved selections by name with their member counts.",
    [Effect::Read],
    crate::selection_sets::ListParams,
    Vec<crate::selection_sets::SetInfo>,
    |store, p| crate::selection_sets::list(store, p)
);

edit_command!(
    SelectionSetSetExport,
    "selectionset.set_export",
    "Export selection sets as IfcGroups",
    "The project option Selection sets as IfcGroup (ARC-SEL-002): with `as_ifc_group`, each saved selection is written on save as an IfcGroup (ObjectType SelectionSet) whose IfcRelAssignsToGroup lists exactly its members that still exist.",
    "Selection Sets Export",
    crate::selection_sets::ExportParams,
    (),
    crate::selection_sets::set_export
);

edit_command!(
    InPlaceCreate,
    "inplace.create",
    "Model in place",
    "A unique free-form element (ARC-INPL-001) of a `category` (Generic Models, Walls, Floors, Roofs, \
     Ceilings, Columns, Casework, Furniture, Stairs, Railings), named, with forms: each `kind` \
     extrusion (`start`, `end` out of its plane), blend (`top` profile, `start`, `end`), revolve \
     (`axis` two points in the plane, `start`, `end` angles) or sweep (`path` in plan, `z`, \
     `closed`; the profile x across, y up), with a `profile` in its work plane, solid or `void`.",
    "Model In-Place",
    crate::in_place::CreateParams,
    crate::id::ElementId,
    crate::in_place::create
);

edit_command!(
    InPlaceAddForm,
    "inplace.add_form",
    "Add a form to an in-place element",
    "Adds a solid or void form (as in inplace.create) to an in-place element; returns its index.",
    "Add Form",
    crate::in_place::AddFormParams,
    usize,
    crate::in_place::add_form
);

edit_command!(
    InPlaceRemoveForm,
    "inplace.remove_form",
    "Remove a form from an in-place element",
    "Removes an in-place element's form by index.",
    "Remove Form",
    crate::in_place::RemoveFormParams,
    (),
    crate::in_place::remove_form
);

edit_command!(
    RoomSeparatorCreate,
    "roomseparator.create",
    "Add room separation lines",
    "Lines on a level that split space for rooms like walls (ARC-ROOM-001); `through` gives a point \
     for an arc, per line. Rooms on the level update at once.",
    "Room Separator",
    crate::room_separators::CreateParams,
    Vec<crate::id::ElementId>,
    crate::room_separators::create
);

edit_command!(
    StairNumberTreads,
    "stair.number_treads",
    "Number a stair's treads",
    "Tread numbers for a stair in a view (ARC-STAIR-006), from `start_number` (default 1) by each \
     tread's place along the stair, so they stay when the stair moves.",
    "Number Treads",
    crate::stair_marks::NumberParams,
    crate::id::ElementId,
    crate::stair_marks::number_treads
);

edit_command!(
    ModelTextCreate,
    "modeltext.create",
    "Place model text",
    "Three-dimensional lettering on a work plane (ARC-MTXT-001): `text` (line breaks for lines), at \
     `point` in the plane, `size` letter height (default 300 mm), `depth` out of the plane (0 flat), \
     `rotation`, `align` left, center or right.",
    "Model Text",
    crate::model_text::CreateParams,
    crate::id::ElementId,
    crate::model_text::create
);

edit_command!(
    ModelTextSet,
    "modeltext.set_text",
    "Edit model text",
    "Changes a model text's text, size, depth, alignment or material.",
    "Edit Text",
    crate::model_text::SetParams,
    (),
    crate::model_text::set
);

edit_command!(
    MassCreate,
    "mass.create",
    "Create a mass",
    "An in-place mass (ARC-MASS-001) from forms as for inplace.create, plus `loft` forms (`profiles: [{offset, points}]`, stacked closed profiles with as many points).",
    "In-Place Mass",
    crate::masses::CreateParams,
    crate::id::ElementId,
    crate::masses::create
);

edit_command!(
    MassAddForm,
    "mass.add_form",
    "Add a form to a mass",
    "Adds a form to a mass; returns its index.",
    "Create Form",
    crate::masses::AddFormParams,
    usize,
    crate::masses::add_form
);

edit_command!(
    MassSetForm,
    "mass.set_form",
    "Stretch a mass form",
    "Changes an extrusion form's start and end offsets.",
    "Edit Form",
    crate::masses::SetFormParams,
    (),
    crate::masses::set_form
);

edit_command!(
    MassSetFloors,
    "mass.set_floors",
    "Set mass floors",
    "The levels a mass has floors on (ARC-MASS-002), or `all`. Floors appear where a ticked level crosses the mass (not at its top) and follow it.",
    "Mass Floors",
    crate::masses::SetFloorsParams,
    (),
    crate::masses::set_floors
);

edit_command!(
    WallCreateByFace,
    "wall.create_by_face",
    "Walls by face",
    "Walls on a mass's upright faces (`faces` by name, mass.faces) (ARC-MASS-003): along the face's bottom edge, as high as the face, on `location_line` (default the exterior finish face); they keep their face for element.update_to_face.",
    "Wall by Face",
    crate::masses::ByFaceParams,
    Vec<crate::id::ElementId>,
    crate::masses::walls_by_face
);

edit_command!(
    FloorCreateByMassFloor,
    "floor.create_by_mass_floor",
    "Floors by mass floor",
    "Floors on a mass's mass floors (at `level_ids`, or all) (ARC-MASS-003).",
    "Floor by Face",
    crate::masses::FloorsByMassParams,
    Vec<crate::id::ElementId>,
    crate::masses::floors_by_mass
);

edit_command!(
    RoofCreateByFace,
    "roof.create_by_face",
    "Roofs by face",
    "A roof slab `thickness` thick on each upward mass face (ARC-ROOF-003); they keep their face for element.update_to_face.",
    "Roof by Face",
    crate::masses::ByFaceParams,
    Vec<crate::id::ElementId>,
    crate::masses::roofs_by_face
);

edit_command!(
    CurtainSystemCreateByFace,
    "curtainsystem.create_by_face",
    "Curtain system by face",
    "A curtain system on mass faces (ARC-CW-007): a grid on each face (Grid 1 and Grid 2 rules, default Fixed Distance 1.5 m), panels and mullions, border mullions shared where faces meet.",
    "Curtain System",
    crate::masses::SystemParams,
    crate::id::ElementId,
    crate::masses::system_by_face
);

edit_command!(
    ElementUpdateToFace,
    "element.update_to_face",
    "Update to face",
    "Walls, roofs and curtain systems made by face take their mass faces as they are now (ARC-MASS-003); walls with a level top keep it.",
    "Update to Face",
    crate::masses::UpdateParams,
    (),
    crate::masses::update_to_face
);

read_command!(
    MassFloors,
    "mass.floors",
    "A mass's floors",
    "Its mass floors with area, perimeter, volume and exterior surface area (m², m, m³).",
    [Effect::Read],
    crate::masses::MassParams,
    Vec<crate::masses::MassFloor>,
    |store, p| crate::masses::floors_of(store, p)
);

read_command!(
    MassFaces,
    "mass.faces",
    "A mass's faces",
    "Its named planar faces with their points and outward normals, for the by-face commands.",
    [Effect::Read],
    crate::masses::MassParams,
    Vec<crate::masses::MassFace>,
    |store, p| crate::masses::faces_of(store, p)
);

read_command!(
    MassSelectDependents,
    "mass.select_dependents",
    "Elements made from a mass",
    "The walls, roofs and curtain systems made by face from the mass.",
    [Effect::Read],
    crate::masses::MassParams,
    Vec<crate::id::ElementId>,
    |store, p| crate::masses::dependents(store, p)
);

edit_command!(
    ToposolidCreate,
    "toposolid.create",
    "Create a toposolid",
    "Solid terrain (ARC-TOPO-001) on a level: a `boundary` (omitted: the points' outline) and `points` (x, y and height above the level), triangulated into its top; `thickness` below its lowest point (default 1 m).",
    "Toposolid",
    crate::toposolids::CreateParams,
    crate::id::ElementId,
    crate::toposolids::create
);

edit_command!(
    ToposolidSetPoints,
    "toposolid.set_points",
    "Edit toposolid points",
    "Adds or moves points (`points`: a point at the same x and y takes the new height), removes points (`remove`) and toggles locks (`lock`).",
    "Edit Points",
    crate::toposolids::SetPointsParams,
    (),
    crate::toposolids::set_points
);

edit_command!(
    ToposolidSubdivide,
    "toposolid.subdivide",
    "Sub-divide a toposolid",
    "A region of a toposolid's surface raised by `offset` (ARC-TOPO-002); negative excavates the toposolid there.",
    "Sub-Divide",
    crate::toposolids::SubdivideParams,
    crate::id::ElementId,
    crate::toposolids::subdivide
);

edit_command!(
    ToposolidGraded,
    "toposolid.create_graded_region",
    "Create a graded region",
    "A copy of a toposolid to grade (ARC-TOPO-003), with all its points (`copy`) or the perimeter's (`perimeter_only`); toposolid.cut_fill measures it against the original.",
    "Graded Region",
    crate::toposolids::GradedParams,
    crate::id::ElementId,
    crate::toposolids::create_graded
);

edit_command!(
    ToposolidSimplify,
    "toposolid.simplify",
    "Simplify a toposolid",
    "Removes interior points that change the surface least, to `target_points` or within `max_vertical_error` (ARC-TOPO-005); reports the points left and the deviation from the original points. Locked points stay.",
    "Simplify",
    crate::toposolids::SimplifyParams,
    crate::toposolids::Simplified,
    crate::toposolids::simplify
);

edit_command!(
    SiteSetSettings,
    "site.set_settings",
    "Set site settings",
    "Contours every `contour_interval` metres, passing through `passing_elevation` (ARC-TOPO-004).",
    "Site Settings",
    crate::toposolids::SettingsParams,
    (),
    crate::toposolids::set_settings
);

edit_command!(
    PropertyLineCreate,
    "propertyline.create",
    "Create a property line",
    "A property line (ARC-SITE-001) sketched as `points`, or a `table` of rows (distance, bearing like N 45°30'00\" E or an azimuth, line or arc with radius and side L or R) from `origin`; reports its area when it closes.",
    "Property Line",
    crate::property_lines::CreateParams,
    crate::property_lines::Created,
    crate::property_lines::create
);

edit_command!(
    SiteComponentPlace,
    "sitecomponent.place",
    "Place a site component",
    "A component (tree, bench, car) at `point`, standing on a toposolid (`host_id`, following its surface) or the level.",
    "Site Component",
    crate::toposolids::SiteComponentParams,
    crate::id::ElementId,
    crate::toposolids::place_component
);

read_command!(
    ToposolidCutFill,
    "toposolid.cut_fill",
    "Cut and fill",
    "A graded region's cut, fill and net against its original (m³), exact for the triangulated surfaces.",
    [Effect::Read],
    crate::toposolids::IdParams,
    crate::toposolids::CutFill,
    |store, p| crate::toposolids::cut_fill_of(store, p)
);

read_command!(
    ToposolidInfo,
    "toposolid.info",
    "Toposolid figures",
    "Its surface area in plan, volume, point count and (sub-divisions) excavated volume.",
    [Effect::Read],
    crate::toposolids::IdParams,
    crate::toposolids::TopoInfo,
    |store, p| crate::toposolids::info(store, p)
);

read_command!(
    PropertyLineArea,
    "propertyline.area",
    "A property line's area",
    "Its area if it closes (m²).",
    [Effect::Read],
    crate::property_lines::AreaParams,
    Option<f64>,
    |store, p| crate::property_lines::area_of(store, p)
);

edit_command!(
    PartsCreate,
    "parts.create",
    "Create parts",
    "Divides each element into parts (ARC-PART-001): one per layer of a straight layered wall (its finished body sliced at the layer boundaries, openings and joins included), or one for the whole body of a single-layer wall, floor, ceiling, roof, column or beam. Parts follow their element. Ineligible elements are skipped with a warning.",
    "Create Parts",
    crate::parts::CreateParams,
    Vec<crate::id::ElementId>,
    crate::parts::create
);

edit_command!(
    PartsDivide,
    "parts.divide",
    "Divide parts",
    "Cuts parts into pieces (ARC-PART-002) along sketched `lines` (each swept along the work plane's `normal`, by default the first part's largest face's) and intersecting `refs` (levels, grids and reference planes with an offset, which the cuts follow), with a `gap` between pieces.",
    "Divide Parts",
    crate::parts::DivideParams,
    crate::parts::Divided,
    crate::parts::divide
);

edit_command!(
    PartsMerge,
    "parts.merge",
    "Merge parts",
    "One part from touching parts of the same material (ARC-PART-003); the merged parts are kept, not drawn, until it is deleted.",
    "Merge Parts",
    crate::parts::IdsParams,
    crate::id::ElementId,
    crate::parts::merge
);

edit_command!(
    PartsExclude,
    "parts.exclude",
    "Exclude parts",
    "Excluded parts stay in the model but are not drawn, scheduled or exported (ARC-PART-003).",
    "Exclude Parts",
    crate::parts::IdsParams,
    (),
    crate::parts::exclude
);

edit_command!(
    PartsRestore,
    "parts.restore",
    "Restore parts",
    "Brings excluded parts back.",
    "Restore Parts",
    crate::parts::IdsParams,
    (),
    crate::parts::restore
);

edit_command!(
    PartsSetOverrides,
    "parts.set_overrides",
    "Set part overrides",
    "A part's own `material` (empty for Material By Original) and Base Level (`base_level_id`, or `base_level`: by_original or non_associated) (ARC-PART-004).",
    "Part Overrides",
    crate::parts::OverridesParams,
    (),
    crate::parts::set_overrides
);

edit_command!(
    ViewSetPartsVisibility,
    "view.set_parts_visibility",
    "Set a view's parts visibility",
    "`show_parts` (sources with parts hidden), `show_original` (parts hidden) or `show_both` (ARC-PART-004).",
    "Parts Visibility",
    crate::parts::VisibilityParams,
    (),
    crate::parts::set_visibility
);

read_command!(
    PartsInfo,
    "parts.info",
    "A part's properties",
    "Volume, largest face area, material (and whether by original), Core or Finish, original category and type, base level, excluded and whether it is divided or merged (ARC-PART-004).",
    [Effect::Read],
    crate::parts::InfoParams,
    crate::parts::PartInfo,
    |store, p| crate::parts::info(store, p)
);

edit_command!(
    RoofCreateGlazingType,
    "roof.create_glazing_type",
    "Create a sloped glazing type",
    "A Sloped Glazing roof type (ARC-CW-008): a roof given it with `element.set_type` becomes glazed \
     planes, keeping its sketch. `grid1` lays out lines parallel to the eaves, measured up the slope \
     (default Fixed Distance 1.5 m); `grid2` lines up the slope (default Maximum Spacing 1.2 m); \
     mullions and panel as for curtain walls.",
    "Sloped Glazing Type",
    crate::roofs::glazing::CreateTypeParams,
    crate::id::ElementId,
    crate::roofs::glazing::create_type
);

edit_command!(
    RoofSetGlazingType,
    "roof.set_glazing_type",
    "Change a sloped glazing type",
    "Changes a Sloped Glazing type's Grid 1 and Grid 2 rules (`{layout, spacing, number}`), mullion \
     width and depth, or panel (ARC-CW-008).",
    "Sloped Glazing Type",
    crate::roofs::glazing::SetTypeParams,
    (),
    crate::roofs::glazing::set_type
);

edit_command!(
    CurtainSetLayout,
    "curtain.set_layout",
    "Set a curtain wall's grid edits",
    "Replaces a curtain wall's manual grid lines (`{key, dir, at}`), removed segments \
     (`{dir, at, segment}`) and swapped panels (`{cell, panel}`) in one step (ARC-CW-004/006).",
    "Edit Grid",
    crate::curtain::SetLayoutParams,
    (),
    crate::curtain::set_layout
);

edit_command!(
    CurtainSetPanelType,
    "curtain.set_panel_type",
    "Swap curtain panels",
    "Swaps the panels of curtain wall cells (ARC-CW-006): `cells` as [column, row] from the start and \
     base; `panel` glazed, solid, empty or door (a curtain-wall door, an IfcDoor in the curtain \
     wall); null returns to the type's panel. Cells merged by removed segments share one panel.",
    "Swap Panel",
    crate::curtain::SetPanelParams,
    (),
    crate::curtain::set_panel_type
);

edit_command!(
    WallSetLocationLine,
    "wall.set_location_line",
    "Change walls' location line",
    "Sets walls' Location Line (wall_centerline, core_centerline, finish_face_exterior, \
     finish_face_interior, core_face_exterior, core_face_interior; ARC-WALL-002): the bodies stay \
     where they are and the location curves move to the new line, which then stays put through \
     type changes and flips about.",
    "Location Line",
    crate::walls::SetLocationLineParams,
    (),
    crate::walls::set_location_line
);

edit_command!(
    WallSetEndWrap,
    "wall.set_end_wrap",
    "Set a wall end's wrapping",
    "Overrides the type's Wrapping at Ends at one end of a wall (ARC-WRAP-001): by_type, \
     exterior, interior or none.",
    "End Wrapping",
    crate::walls::layered::SetEndWrapParams,
    (),
    crate::walls::layered::set_end_wrap
);

edit_command!(
    WallSweepCreate,
    "wallsweep.create",
    "Add a wall sweep",
    "A profile run along walls' faces (ARC-WALL-010): a cornice or skirting. `orientation` horizontal \
     (at `offset_from_level` above the first wall's base level, continuing around the walls joined \
     end to end with it unless `chain` is false, mitred at corners) or vertical (at \
     `distance_along` the first wall, base to top); `side` exterior or interior; a rectangular \
     profile `width` out from the face by `height`, or `profile` points (x out from the face, y up \
     or along); `cuts_wall` also cuts the walls where it overlaps. An IfcCovering (MOLDING).",
    "Wall Sweep",
    crate::wall_sweeps::CreateParams,
    crate::id::ElementId,
    crate::wall_sweeps::create_sweep
);

edit_command!(
    WallRevealCreate,
    "wallreveal.create",
    "Add a wall reveal",
    "A groove cut along walls' faces (ARC-WALL-010), as `wallsweep.create`: the rectangular profile \
     is `width` into the wall by `height`. It cuts every layer it reaches; an IfcOpeningElement \
     (RECESS). Not on slanted walls.",
    "Wall Reveal",
    crate::wall_sweeps::CreateParams,
    crate::id::ElementId,
    crate::wall_sweeps::create_reveal
);

edit_command!(
    WallSweepAddWalls,
    "wallsweep.add_walls",
    "Add walls to a sweep",
    "Runs a wall sweep or reveal along more walls (ARC-WALL-010).",
    "Add Walls",
    crate::wall_sweeps::WallsParams,
    (),
    crate::wall_sweeps::add_walls
);

edit_command!(
    WallSweepRemoveWalls,
    "wallsweep.remove_walls",
    "Remove walls from a sweep",
    "Stops a wall sweep or reveal running along walls (ARC-WALL-010); one left with no walls is \
     deleted.",
    "Remove Walls",
    crate::wall_sweeps::WallsParams,
    (),
    crate::wall_sweeps::remove_walls
);

edit_command!(
    WallSweepSet,
    "wallsweep.set",
    "Change a wall sweep",
    "Changes a wall sweep's or reveal's height above its level, distance along its wall \
     (vertical), offset from the face, side, profile points, Cuts Wall or material (ARC-WALL-010).",
    "Wall Sweep",
    crate::wall_sweeps::SetParams,
    (),
    crate::wall_sweeps::set
);

edit_command!(
    WallEditProfile,
    "wall.edit_profile",
    "Edit a wall's profile",
    "Changes a straight wall's elevation outline (ARC-WALL-009): `loops` is the outer loop then any \
     holes, as points `[u, z]` with `u` along the wall from its start and `z` above its base \
     (metres). Each point keeps to the nearer end and the nearer of base and top, so the top line \
     follows the wall's top and a notch keeps its size. Doors and windows still cut the body.",
    "Edit Profile",
    crate::walls::profile::EditParams,
    (),
    crate::walls::profile::edit
);

edit_command!(
    WallResetProfile,
    "wall.reset_profile",
    "Reset walls' profiles",
    "Restores the rectangular elevation outline of walls whose profile was edited (ARC-WALL-009).",
    "Reset Profile",
    crate::walls::profile::ResetParams,
    (),
    crate::walls::profile::reset
);

read_command!(
    WallGetProfile,
    "wall.get_profile",
    "Get a wall's profile",
    "A straight wall's elevation outline now, as loops of `[u, z]` points (along from the start, \
     above the base; metres): the outer loop then holes; the rectangle when not edited.",
    [Effect::Read],
    crate::walls::profile::GetParams,
    Vec<Vec<[f64; 2]>>,
    |store, p| crate::walls::profile::get(store, p)
);

edit_command!(
    StackedWallSetRows,
    "stackedwall.set_rows",
    "Set a stacked wall type's rows",
    "Makes a wall type a stacked wall type (ARC-STKW-001): `rows` from top to bottom, each a basic wall \
     type with a `height` (exactly one row omits it: the Variable row, which takes the rest of the \
     wall's height), an `offset` from the `offset_reference` (wall_centerline, core_centerline, \
     finish_face_exterior, finish_face_interior, core_face_exterior, core_face_interior; positive \
     toward the exterior) and a `flip`. Every wall of the type must stay taller than its fixed rows.",
    "Edit Stack",
    crate::walls::stacked::SetRowsParams,
    (),
    crate::walls::stacked::set_rows
);

edit_command!(
    StackedWallCreateType,
    "stackedwall.create_type",
    "Create a stacked wall type",
    "A new stacked wall type named `name` with `rows` as for `stackedwall.set_rows`; draw it with \
     `wall.create` and its `type_id`.",
    "New Stacked Wall Type",
    crate::walls::stacked::CreateTypeParams,
    crate::id::ElementId,
    crate::walls::stacked::create_type
);

edit_command!(
    WallBreakUp,
    "wall.break_up",
    "Break up stacked walls",
    "Turns stacked walls into their sub-walls as basic walls, which keep the sub-walls' GlobalIds, \
     positions and heights; doors and windows go to the sub-wall their centre is in.",
    "Break Up",
    crate::walls::stacked::BreakUpParams,
    Vec<crate::id::ElementId>,
    crate::walls::stacked::break_up
);

edit_command!(
    WallSetCrossSection,
    "wall.set_cross_section",
    "Slant or taper walls",
    "Sets straight walls' cross-section (ARC-WALL-005): `vertical`; `slanted` by `angle` from vertical \
     (a positive angle leans the top to the right of the drawing direction, the interior when not \
     flipped); or `tapered`, its exterior and interior faces leaning inward by `exterior_angle` and \
     `interior_angle`, which needs a variable layer in the type that stays thicker than zero at the top. \
     Written to IFC as a B-rep.",
    "Cross-Section",
    crate::walls::cross_section::SetParams,
    (),
    crate::walls::cross_section::set
);
