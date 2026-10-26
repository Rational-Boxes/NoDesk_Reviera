// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP commands: connectors, connections, starter component types, system types, systems and
//! system checks (Ch 4 §4.6). Registered with the core commands by [`crate::builtin::register`].

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

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add(ConnectorAdd),
        registry.add(ConnectorSet),
        registry.add(ConnectorRemove),
        registry.add(ConnectorList),
        registry.add(MepConnect),
        registry.add(MepDisconnect),
        registry.add(MepConnectTouching),
        registry.add(MepNetworkOf),
        registry.add(MepTypeCreate),
        registry.add(MepTypeLoadStarters),
        registry.add(SystemTypeList),
        registry.add(SystemTypeCreate),
        registry.add(SystemTypeSet),
        registry.add(SystemCreate),
        registry.add(SystemAdd),
        registry.add(SystemRemove),
        registry.add(SystemSelectEquipment),
        registry.add(SystemDisconnectEquipment),
        registry.add(SystemDivide),
        registry.add(SystemSet),
        registry.add(SystemList),
        registry.add(SystemUnassigned),
        registry.add(MepCheckSystems),
        registry.add(MepCheckDisconnects),
        registry.add(DuctCreate),
        registry.add(PipeCreate),
        registry.add(DuctSetSize),
        registry.add(PipeSetSize),
        registry.add(DuctCapOpenEnds),
        registry.add(PipeCapOpenEnds),
        registry.add(MepSegmentTypeCreate),
        registry.add(MepSegmentTypeList),
        registry.add(RoutingPreferencesGet),
        registry.add(RoutingPreferencesSet),
        registry.add(MepRunReapplyType),
        registry.add(ElectricalSettingsGet),
        registry.add(ElectricalSettingsSet),
        registry.add(DistributionSystemCreate),
        registry.add(VoltageCreate),
        registry.add(PanelSet),
        registry.add(PanelList),
        registry.add(CircuitCreate),
        registry.add(CircuitAdd),
        registry.add(CircuitRemove),
        registry.add(CircuitSelectPanel),
        registry.add(CircuitDisconnectPanel),
        registry.add(CircuitCompatiblePanels),
        registry.add(CircuitList),
        registry.add(PanelCreateFeeder),
        registry.add(PanelSetSupply),
        registry.add(PanelHierarchy),
        registry.add(CircuitSet),
        registry.add(LoadClassificationCreate),
        registry.add(LoadClassificationSet),
        registry.add(LoadClassificationDelete),
        registry.add(DemandFactorCreate),
        registry.add(DemandFactorSet),
        registry.add(DemandFactorDelete),
        registry.add(DemandFactorEvaluate),
        registry.add(PanelScheduleCreate),
        registry.add(PanelScheduleChangePanel),
        registry.add(PanelLoadsSummary),
        registry.add(LoadClassificationList),
        registry.add(CircuitProperties),
        registry.add(CircuitSizeCable),
        registry.add(CircuitSetPath),
        registry.add(CircuitResetPath),
        registry.add(CircuitPath),
        registry.add(ConductorSizeCreate),
        registry.add(CableTypeCreate),
        registry.add(ConductorAmpacitySet),
        registry.add(WiringSet),
        registry.add(MepSize),
        registry.add(MepRunSetSlope),
        registry.add(PipeFittingPlace),
        registry.add(PipeAccessoryPlace),
        registry.add(MepFittingTeeToCross),
        registry.add(PipeCreateParallel),
        registry.add(MepSettingsSetSlopes),
        registry.add(MepSettingsSlopes),
        registry.add(MechControlDevicePlace),
        registry.add(PlumbingFixturePlace),
        registry.add(SprinklerPlace),
        registry.add(SprinklerTypeSet),
        registry.add(HydronicSetEnabled),
        registry.add(HydronicAnalyze),
        registry.add(HydraulicSeparationAdd),
        registry.add(HydraulicSeparationRemove),
        registry.add(AnalyticalPipeConnectionCreate),
        registry.add(AnalyticalPipeConnectionCreateAuto),
        registry.add(AnalyticalPipeConnectionSet),
        registry.add(AnalyticalPipeConnectionList),
        registry.add(DuctAccessoryPlace),
        registry.add(MepSegmentHeal),
        registry.add(MepCoveringTypeCreate),
        registry.add(MepInsulationAdd),
        registry.add(MepInsulationSet),
        registry.add(MepInsulationRemove),
        registry.add(DuctLiningAdd),
        registry.add(DuctLiningSet),
        registry.add(DuctLiningRemove),
        registry.add(MepCoveringList),
        registry.add(FlexDuctCreate),
        registry.add(DuctConvertToFlex),
        registry.add(MepRunJustify),
        registry.add(DuctPlaceholderCreate),
        registry.add(PipePlaceholderCreate),
        registry.add(PlaceholderConvert),
        registry.add(AirTerminalPlace),
        registry.add(SpaceAirflow),
        registry.add(MechEquipmentPlace),
        registry.add(EquipmentSetCreate),
        registry.add(EquipmentSetAdd),
        registry.add(EquipmentSetRemove),
        registry.add(EquipmentSetSetDuty),
        registry.add(EquipmentSetList),
        registry.add(SystemFlowTotals),
        registry.add(MepConnectInto),
        registry.add(MechanicalSettingsGet),
        registry.add(MechanicalSettingsSet),
        registry.add(DuctSizeTableGet),
        registry.add(DuctSizeTableSet),
        registry.add(PipeSegmentCreate),
        registry.add(FluidCreate),
        registry.add(SpaceTypeCreate),
        registry.add(BuildingTypeCreate),
        registry.add(SpaceTypeSet),
        registry.add(SpaceTypeDelete),
        registry.add(SpaceTypeList),
        registry.add(SpaceSetType),
        registry.add(BuildingSetType),
        registry.add(ElectricalEquipmentPlace),
        registry.add(DevicePlace),
        registry.add(LightingFixturePlace),
        registry.add(LightingFixtureTypeSetPhotometry),
        registry.add(ComponentTypeSetCalcPoint),
        registry.add(SpaceElements),
        registry.add(SpacePlace),
        registry.add(SpacePlaceAuto),
        registry.add(SpaceUnplace),
        registry.add(SpaceSet),
        registry.add(SpaceNameFromRooms),
        registry.add(SpaceInfo),
        registry.add(SpaceSeparatorCreate),
        registry.add(LightingCalcAverage),
        registry.add(MepColorLegendPlace),
        registry.add(CircuitRenumber),
        registry.add(CarbonAddLibrary),
        registry.add(PathOfTravelCreate),
        registry.add(PathOfTravelUpdate),
        registry.add(PathOfTravelWaypoint),
        registry.add(PathOfTravelList),
        registry.add(RouteSettingsSet),
        registry.add(RouteSettingsGet),
        registry.add(CarbonMatch),
        registry.add(CarbonAutoMatch),
        registry.add(CarbonMaterials),
        registry.add(CarbonRecord),
        registry.add(SolarRecord),
        registry.add(EnergyModelGet),
        registry.add(EnergyModelSummary),
        registry.add(EnergyModelSetThermal),
        registry.add(SolarResults),
        registry.add(CarbonResults),
        registry.add(CircuitRenumberPreview),
        registry.add(CircuitSetLocked),
        registry.add(LightingFixtureTypeSetLighting),
        registry.add(SystemZoneCreate),
        registry.add(SystemZoneAssign),
        registry.add(SystemZoneSet),
        registry.add(SystemZoneInfo),
        registry.add(SystemZoneApplyConvert),
        registry.add(GlobalCreate),
        registry.add(GlobalSet),
        registry.add(GlobalDelete),
        registry.add(GlobalAssociate),
        registry.add(GlobalDissociate),
        registry.add(GlobalLabelDimension),
        registry.add(GlobalList),
        registry.add(GlobalSelectDriven),
        registry.add(FormulaValidate),
        registry.add(ClassificationCreate),
        registry.add(ClassificationAssign),
        registry.add(ClassificationUnassign),
        registry.add(ClassificationDelete),
        registry.add(ClassificationList),
        registry.add(ClassificationSearch),
        registry.add(ClassificationOf),
        registry.add(KeynotesSet),
        registry.add(KeynotesList),
        registry.add(StandardsApply),
        registry.add(MechControlDeviceAssign),
        registry.add(MechControlDeviceList),
        registry.add(MepRunCreateRiser),
        registry.add(MepRunBindRiser),
        registry.add(SizingCalculatorCandidates),
        registry.add(MepNetworkCalculate),
        registry.add(AnalyticalSegmentList),
        registry.add(SystemInspectorStart),
        registry.add(AnalyticalSegmentOverride),
        registry.add(MepSettingsSetFrictionEquation),
        registry.add(PressureLossReportExport),
        registry.add(PressureLossReportSavePreset),
        registry.add(MepPenetrations),
        registry.add(GutterCreate),
        registry.add(GutterSet),
        registry.add(GutterAddOutlet),
        registry.add(GutterSize),
        registry.add(MepSetPenetrationRule),
        registry.add(MepRunLockSize),
        registry.add(FixtureUnitMethodSet),
        registry.add(FixtureUnitMethodEvaluate),
        registry.add(CableTrayCreate),
        registry.add(ConduitCreate),
        registry.add(CableTraySplit),
        registry.add(ConduitSplit),
        registry.add(CableTrayFittingPlace),
        registry.add(ConduitFittingPlace),
        registry.add(MepFittingUpgrade),
        registry.add(MepFittingDowngrade),
        registry.add(ConduitCreateParallel),
        registry.add(ConduitStandardCreate),
        registry.add(CableTraySetSize),
        registry.add(ConduitSetSize),
    ];
    for r in results {
        r.expect("MEP commands are valid");
    }
}

edit_command!(
    ConnectorAdd,
    "connector.add",
    "Add a connector to a component type",
    "Adds an MEP connector to a component type (MEP-CONN-001): a `domain` (duct, pipe, electrical, cable_tray, conduit), its `origin` and outward `direction` in the type's frame (metres, the insertion point at the origin), a `shape` (round, rectangular, oval) and `diameter` or `width` and `height`, and the domain's values: duct and pipe `classification` (supply_air, return_air, exhaust_air, other_air, hydronic_supply, hydronic_return, domestic_hot_water, domestic_cold_water, sanitary, vent, fire_protection_wet/dry/pre_action/other, other; fitting and global for fittings), `flow_direction` (in, out, bidirectional), `flow_config` (calculated, preset, system), `flow` (m³/s), `flow_factor` (0–1), `loss_method`, `utility`, `fixture_units`, `connection_type`; electrical `electrical` {system, voltage, poles, load (VA), power_factor, lagging, load_classification, motor}; conduit `surface`. Every copy of the type gets the port. Returns its index.",
    "Add Connector",
    crate::mep::AddParams,
    usize,
    crate::mep::add
);

edit_command!(
    ConnectorSet,
    "connector.set",
    "Change a connector",
    "Changes connector `connector_index` (MEP-CONN-001). On a component type any of its values (as `connector.add`); its copies follow. On a placed copy only its system and flow values (classification, flow_direction, flow_config, flow, flow_factor, loss_method, utility, fixture_units, electrical) for that copy; geometry and size belong to the type and are refused.",
    "Change Connector",
    crate::mep::SetParams,
    (),
    crate::mep::set
);

edit_command!(
    ConnectorRemove,
    "connector.remove",
    "Remove a connector from a type",
    "Removes connector `connector_index` from a component type: connections to its ports are dropped and the copies' later connectors keep their connections and values.",
    "Remove Connector",
    crate::mep::RemoveParams,
    (),
    crate::mep::remove
);

read_command!(
    ConnectorList,
    "connector.list",
    "A type's connectors or a copy's ports",
    "The connectors of a component type, or the ports of a placed copy with where each is (project coordinates, metres), its outward direction, the ports it is connected to and the systems it is in.",
    crate::mep::ListParams,
    Vec<crate::mep::ConnectorInfo>,
    crate::mep::list
);

edit_command!(
    MepConnect,
    "mep.connect",
    "Connect two ports",
    "Joins two ports `port_a` and `port_b` ({element, port}) (MEP-CONN-002). Refused across domains, for electrical ports (they join through circuits), when a port already has a partner (conduit surface connectors take any number), and when shapes or sizes differ (\"Connectors differ in size or shape and no transition is available\"). Returns the connection.",
    "Connect",
    crate::mep::ConnectParams,
    crate::mep::Connection,
    crate::mep::connect
);

edit_command!(
    MepDisconnect,
    "mep.disconnect",
    "Disconnect two ports",
    "Separates two connected ports (MEP-CONN-002); refused when they are not connected.",
    "Disconnect",
    crate::mep::ConnectParams,
    (),
    crate::mep::disconnect
);

edit_command!(
    MepConnectTouching,
    "mep.connect_touching",
    "Connect coinciding ports",
    "Connects the open ports of `element_ids` (every element with connectors when empty) to other open ports that coincide with them: within 0.5 mm, facing each other within 0.5°, of the same domain, shape and size (MEP-CONN-002). Placing and moving components does this by itself. Returns the connections made.",
    "Connect",
    crate::mep::ConnectTouchingParams,
    Vec<crate::mep::Connection>,
    crate::mep::connect_touching
);

read_command!(
    MepNetworkOf,
    "mep.network_of",
    "The elements connected to an element",
    "The elements of an element's network (MEP-CONN-002): every element reached through connections, it first.",
    crate::mep::NetworkParams,
    Vec<ElementId>,
    crate::mep::network_of
);

edit_command!(
    MepTypeCreate,
    "mep_type.create",
    "Make a box-shaped MEP component type",
    "Makes a component type of a box `size` [width, depth, height] (metres; insertion point at the bottom centre) of a `category` (mechanical_equipment, air_terminal, plumbing_fixture, lighting_fixture, electrical_equipment, electrical_fixture, sprinkler, generic_model, …), an optional RGBA `color`, with `connectors` (each as `connector.add` without the type). Place copies with `mesh_object.place`. Returns the type.",
    "MEP Component Type",
    crate::mep::CreateTypeParams,
    ElementId,
    crate::mep::create_type
);

edit_command!(
    MepTypeLoadStarters,
    "mep_type.load_starters",
    "Load the starter MEP component types",
    "Loads Reviera's starter MEP component types (`names`, all when empty): Air Handling Unit, Supply Diffuser 600, Return Grille 600, Exhaust Fan, Water Heater, Circulation Pump, Lavatory, Water Closet, Distribution Board, Receptacle, Ceiling Luminaire, the plumbing fixtures, control devices, VAV Box, Pendent, Upright and Sidewall Sprinkler, Chiller, Boiler and Fan Coil Unit (an unknown name is refused listing them all): simple boxes with connectors, sizes and flows Reviera's own provisional values, for laying out systems before manufacturers' content. A type of the same name already in the model is reused. Returns the types by name.",
    "Load MEP Types",
    crate::mep::LoadStartersParams,
    std::collections::BTreeMap<String, ElementId>,
    crate::mep::load_starters
);

read_command!(
    SystemTypeList,
    "system_type.list",
    "The MEP system types",
    "Every system type (MEP-SYS-001): the built-in one of each classification (as changed) and those made, with colour, line pattern, material, calculations, abbreviation, rise/drop symbols, fluid and temperature.",
    crate::mep_systems::Empty,
    Vec<crate::mep_systems::SystemType>,
    crate::mep_systems::list_types
);

edit_command!(
    SystemTypeCreate,
    "system_type.create",
    "Make an MEP system type",
    "Makes a system type of a `classification` named `name` (unique), starting from the classification's built-in type, with any of `color` (RGBA 0–1), `line_pattern`, `material`, `calculations` (all, flow_only, performance, none), `abbreviation`, `rise_drop`, `fluid`, `fluid_temperature` (°C), `flow_conversion`, `ifc_predefined` (hydronic: HEATING or CHILLEDWATER) (MEP-SYS-001). Returns its id.",
    "System Type",
    crate::mep_systems::CreateTypeParams,
    String,
    crate::mep_systems::create_type
);

edit_command!(
    SystemTypeSet,
    "system_type.set",
    "Change an MEP system type",
    "Changes a system type's values (as `system_type.create`) (MEP-SYS-001); the members of its systems are drawn in its new colour.",
    "Change System Type",
    crate::mep_systems::SetTypeParams,
    (),
    crate::mep_systems::set_type
);

edit_command!(
    SystemCreate,
    "system.create",
    "Make an MEP system",
    "Makes a duct or piping system (`kind`: duct, piping; power, data and switch systems come with circuits) of `element_ids` (terminals, fixtures, equipment) by their connectors of the system type's classification (`system_type_id`, by default the first member's classification's built-in type) (MEP-SYS-002). An element without such a free connector is refused. The name is `name` (unique within the classification) or the type's name and a number. Returns the system (an IfcDistributionSystem).",
    "Create System",
    crate::mep_systems::CreateParams,
    ElementId,
    crate::mep_systems::create
);

edit_command!(
    SystemAdd,
    "system.add",
    "Add elements to an MEP system",
    "Adds `element_ids` to a system by their connectors of its classification (MEP-SYS-002).",
    "Add To System",
    crate::mep_systems::MembersParams,
    (),
    crate::mep_systems::add
);

edit_command!(
    SystemRemove,
    "system.remove",
    "Remove elements from an MEP system",
    "Takes `element_ids` out of a system (MEP-SYS-002); equipment among them stops serving it.",
    "Remove From System",
    crate::mep_systems::MembersParams,
    (),
    crate::mep_systems::remove
);

edit_command!(
    SystemSelectEquipment,
    "system.select_equipment",
    "Set a system's equipment",
    "Sets the equipment that serves a system (MEP-SYS-002); it joins the system by its connector of the system's classification.",
    "Select Equipment",
    crate::mep_systems::EquipmentParams,
    (),
    crate::mep_systems::select_equipment
);

edit_command!(
    SystemDisconnectEquipment,
    "system.disconnect_equipment",
    "Disconnect a system's equipment",
    "Stops the equipment serving a system; it leaves the system (MEP-SYS-002).",
    "Disconnect Equipment",
    crate::mep_systems::SystemParams,
    (),
    crate::mep_systems::disconnect_equipment
);

edit_command!(
    SystemDivide,
    "system.divide",
    "Divide an MEP system",
    "Splits a system whose members form several separate networks into one system per network, named \"Name 1\", \"Name 2\", … (the first keeps the original's identity); the equipment goes with its network (MEP-SYS-002). Returns the systems.",
    "Divide System",
    crate::mep_systems::SystemParams,
    Vec<ElementId>,
    crate::mep_systems::divide
);

edit_command!(
    SystemSet,
    "system.set",
    "Rename an MEP system or change its type",
    "Renames a system (`name`, unique within its classification: \"A system with this name already exists\") or gives it another type of the same classification (`system_type_id`).",
    "Change System",
    crate::mep_systems::SetParams,
    (),
    crate::mep_systems::set
);

read_command!(
    SystemList,
    "system.list",
    "The MEP systems",
    "Every system (or those with `element_id`): name, classification, type, member ports and elements, and equipment.",
    crate::mep_systems::ListParams,
    Vec<crate::mep_systems::SystemInfo>,
    crate::mep_systems::list
);

read_command!(
    SystemUnassigned,
    "system.unassigned",
    "Connectors in no system",
    "Duct and pipe ports in no system, by domain (the System Browser's Unassigned node).",
    crate::mep_systems::Empty,
    std::collections::BTreeMap<String, Vec<(ElementId, usize)>>,
    crate::mep_systems::list_unassigned
);

read_command!(
    MepCheckSystems,
    "mep_check.systems",
    "Check duct or pipe systems",
    "Checks duct or pipe systems or circuits (`kind`: duct, pipe, circuit) (MEP-SYS-005); circuits: no panel, or a panel whose distribution system does not fit. Systems: systems not well connected (no equipment, or members not connected to it), flow and demand mismatches (a preset equipment flow unlike its preset terminals' total; a calculated outlet with nothing downstream) and flow direction mismatches (connected connectors both In or both Out). Each issue: kind, elements, connectors and message. Changes nothing.",
    crate::mep_systems::CheckParams,
    Vec<crate::mep_systems::Issue>,
    crate::mep_systems::check
);

read_command!(
    MepCheckDisconnects,
    "mep_check.show_disconnects",
    "Open connectors",
    "The open connectors of `domains` (duct, pipe, cable_tray, conduit; all when empty) (MEP-SYS-005): ports with no partner, with where each is.",
    crate::mep_systems::DisconnectsParams,
    Vec<crate::mep::Port>,
    crate::mep_systems::disconnects
);

edit_command!(
    DuctCreate,
    "duct.create",
    "Draw a duct run",
    "Draws a duct run (MEP-DUCT-001) along `points` ([x, y] at `middle_elevation` above `level`, 2.7 m by default, or [x, y, z] with z above the level) of a `shape` (rectangular, round, oval; the type's) and size (`width` × `height`, or `diameter`), of `type_id` (the built-in Rectangular, Round or Oval Duct when omitted), carrying `system_type_id`. An elbow goes at each corner by the type's routing preferences (a change of height is a riser between two elbows), and a tee or tap where the run ends or starts on an existing duct's side (square to it); where no routing row fits, nothing is inserted and a warning names the role and size. Its ends connect to the connectors they meet, and it joins the system of what it connects to. Returns the segments, fittings, what it connected to and the system.",
    "Duct",
    crate::mep_runs::CreateParams,
    crate::mep_runs::RunReport,
    crate::mep_runs::create_duct
);

edit_command!(
    PipeCreate,
    "pipe.create",
    "Draw a pipe run",
    "Draws a pipe run (MEP-PIPE-001) as `duct.create` does a duct run, round only (`diameter`), at 2.4 m by default, of `type_id` (the built-in Standard Pipe when omitted); `slope` makes level legs fall (rise over run, 0.01 for 1 %) from the first point.",
    "Pipe",
    crate::mep_runs::CreateParams,
    crate::mep_runs::RunReport,
    crate::mep_runs::create_pipe
);

edit_command!(
    DuctSetSize,
    "duct.set_size",
    "Resize ducts",
    "Gives duct segments a new size (`width`, `height`, `diameter`); the legs of the fittings joined to them follow. Refused on size-locked segments (\"Element is size-locked\").",
    "Duct Size",
    crate::mep_runs::SetSizeParams,
    (),
    crate::mep_runs::set_size
);

edit_command!(
    PipeSetSize,
    "pipe.set_size",
    "Resize pipes",
    "Gives pipe segments a new `diameter`; the legs of the fittings joined to them follow. Refused on size-locked segments (\"Element is size-locked\").",
    "Pipe Size",
    crate::mep_runs::SetSizeParams,
    (),
    crate::mep_runs::set_size
);

edit_command!(
    DuctCapOpenEnds,
    "duct.cap_open_ends",
    "Cap open duct ends",
    "Caps the open ends of the given ducts and duct fittings (all when empty) with the routing preferences' cap; returns the caps.",
    "Cap Open Ends",
    crate::mep_runs::CapParams,
    Vec<ElementId>,
    crate::mep_runs::cap_open_ends
);

edit_command!(
    PipeCapOpenEnds,
    "pipe.cap_open_ends",
    "Cap open pipe ends",
    "Caps the open ends of the given pipes and pipe fittings (all when empty) with the routing preferences' cap; returns the caps.",
    "Cap Open Ends",
    crate::mep_runs::CapParams,
    Vec<ElementId>,
    crate::mep_runs::cap_open_ends
);

edit_command!(
    MepSegmentTypeCreate,
    "mep_segment_type.create",
    "Make a duct, pipe, cable tray or conduit type",
    "Makes a segment type (`domain` duct, pipe, cable_tray or conduit; `shape` round, rectangular, oval; pipes and conduit round, tray rectangular) named `name` with `routing` preferences (MEP-DUCT-002: rows {part, min, max} per role elbow, junction (tee, tap), cross, transition, cap, union (coupling), and `preferred_junction` tee or tap; one row of each by default, a coupling union for conduit), `material` and `roughness`. Tray and conduit: `with_fittings` (default true; without, bends are curved segments and fittings cannot be placed), `bend_radius_multiplier` (bend radius over width or diameter; 1 for tray, 6 for conduit by default), tray `tray_kind` (ladder, channel, solid_bottom, trough) and conduit `standard` (a conduit standard's id: iec_61386, emt, or one made with conduit_standard.create). Duct: `flexible` makes a flex duct type (MEP-DUCT-005, drawn with flex_duct.create; its `bend_radius_multiplier` is the minimum bend radius over the diameter, default 1) with a `loss_multiplier` for compressed flex (1 to 10, default 1). Returns the type.",
    "Segment Type",
    crate::mep_runs::CreateTypeParams,
    ElementId,
    crate::mep_runs::create_type
);

read_command!(
    MepSegmentTypeList,
    "mep_segment_type.list",
    "The duct and pipe types",
    "Every duct and pipe type (or those of `domain`) with its shape, routing preferences, material and roughness.",
    crate::mep_runs::ListTypesParams,
    Vec<crate::mep_runs::TypeInfo>,
    crate::mep_runs::list_types
);

read_command!(
    RoutingPreferencesGet,
    "routing_preferences.get",
    "A type's routing preferences",
    "The routing preferences of a duct or pipe type (MEP-DUCT-002).",
    crate::mep_runs::RoutingGetParams,
    crate::mep_runs::Routing,
    crate::mep_runs::routing_get
);

edit_command!(
    RoutingPreferencesSet,
    "routing_preferences.set",
    "Set a type's routing preferences",
    "Replaces a duct or pipe type's routing preferences (MEP-DUCT-002); runs drawn from now on use them, and `mep_run.reapply_type` refits existing fittings.",
    "Routing Preferences",
    crate::mep_runs::RoutingSetParams,
    (),
    crate::mep_runs::routing_set
);

edit_command!(
    MepRunReapplyType,
    "mep_run.reapply_type",
    "Refit fittings by routing preferences",
    "Re-chooses fittings (or those joined to the given segments; all when empty) by their segments' types' routing preferences (MEP-DUCT-002): a fitting whose part changes is remade; one whose role has no row that fits is removed with a warning, its ends left open. Returns what was replaced and removed.",
    "Reapply Type",
    crate::mep_runs::ReapplyParams,
    crate::mep_runs::ReapplyReport,
    crate::mep_runs::reapply_type
);

read_command!(
    ElectricalSettingsGet,
    "electrical_settings.get",
    "The electrical settings",
    "The project's electrical settings (MEP-SET-002): voltage definitions (value, minimum, maximum), distribution systems (phase single or three, wye or delta, wires, line-to-line and line-to-ground voltage), the circuit naming scheme (numerical, prefixed, panel_name_number) and whether multi-pole circuits are named by their first slot only.",
    crate::mep_electrical::Empty,
    crate::mep_electrical::ElectricalSettings,
    crate::mep_electrical::get_settings
);

edit_command!(
    ElectricalSettingsSet,
    "electrical_settings.set",
    "Change the electrical settings",
    "Sets the project's circuit naming scheme (`naming`: numerical, prefixed, panel_name_number) and `first_slot_only` (MEP-ELEC-012); every circuit on a panel set to By Project is renamed.",
    "Electrical Settings",
    crate::mep_electrical::SetSettingsParams,
    (),
    crate::mep_electrical::set_settings
);

edit_command!(
    DistributionSystemCreate,
    "distribution_system.create",
    "Make a distribution system",
    "Adds a distribution system (MEP-SET-002): `name`, `phase` (single, three), `configuration` (wye, delta), `wires` (2–4), `v_ll` and `v_lg` (volts; three-phase needs v_ll). Returns its id.",
    "Distribution System",
    crate::mep_electrical::DistributionParams,
    String,
    crate::mep_electrical::create_distribution
);

edit_command!(
    VoltageCreate,
    "voltage.create",
    "Make a voltage definition",
    "Adds a voltage definition (MEP-SET-002): `name`, `value`, `min` and `max` (volts). Returns its id.",
    "Voltage",
    crate::mep_electrical::VoltageParams,
    String,
    crate::mep_electrical::create_voltage
);

edit_command!(
    PanelSet,
    "panel.set",
    "Set a panel's name, distribution system and slots",
    "Sets a panel's (electrical equipment with an electrical connector) `name` (shown in circuit numbers), `distribution_system`, `slots` (1–168; poles take every second slot) and `naming` (by_project, numerical, prefixed, panel_name_number). Refused when its circuits would no longer fit. Circuits are renumbered.",
    "Panel",
    crate::mep_electrical::PanelSetParams,
    (),
    crate::mep_electrical::set_panel
);

read_command!(
    PanelList,
    "panel.list",
    "The panels",
    "Every panel with its distribution system, slots, naming, circuits, used slots and connected load (VA).",
    crate::mep_electrical::Empty,
    Vec<crate::mep_electrical::PanelInfo>,
    crate::mep_electrical::list_panels
);

edit_command!(
    CircuitCreate,
    "circuit.create",
    "Make a circuit",
    "Makes a circuit (MEP-ELEC-002) of `kind` (power, data, telephone, security, fire_alarm, nurse_call, controls, communication) of devices and fixtures by their electrical connectors of the kind; power circuits have a `voltage` and `poles` (the first device's when omitted), and devices of another voltage or pole count are skipped and reported, with warnings. A device on another circuit moves to it. Returns the circuit, the devices added, skipped (with the reason) and moved.",
    "Create Circuit",
    crate::mep_electrical::CreateParams,
    crate::mep_electrical::CircuitReport,
    crate::mep_electrical::create
);

edit_command!(
    CircuitAdd,
    "circuit.add",
    "Add devices to a circuit",
    "Adds devices to a circuit (MEP-ELEC-003); an incompatible one is refused with \"Incompatible: <reason>\"; one on another circuit is moved off it.",
    "Add to Circuit",
    crate::mep_electrical::MembersParams,
    crate::mep_electrical::CircuitReport,
    crate::mep_electrical::add
);

edit_command!(
    CircuitRemove,
    "circuit.remove",
    "Remove devices from a circuit",
    "Takes devices off a circuit (MEP-ELEC-003); removing the last one deletes the circuit.",
    "Remove from Circuit",
    crate::mep_electrical::MembersParams,
    (),
    crate::mep_electrical::remove
);

edit_command!(
    CircuitSelectPanel,
    "circuit.select_panel",
    "Put a circuit on a panel",
    "Puts a circuit on a panel from `slot` (refused when taken or beyond the last slot) or in the first free slots that fit its poles (MEP-ELEC-003): refused when the panel's distribution system does not give the circuit's voltage (line to ground for one pole, line to line for two or three; three poles need three phases) or it is full. The circuit is numbered by the panel's naming scheme. Returns its number.",
    "Select Panel",
    crate::mep_electrical::PanelParams,
    String,
    crate::mep_electrical::select_panel
);

edit_command!(
    CircuitDisconnectPanel,
    "circuit.disconnect_panel",
    "Take a circuit off its panel",
    "Frees the circuit's slots (MEP-ELEC-003); it keeps its members and loses its number.",
    "Disconnect Panel",
    crate::mep_electrical::CircuitParams,
    (),
    crate::mep_electrical::disconnect_panel
);

read_command!(
    CircuitCompatiblePanels,
    "circuit.compatible_panels",
    "Panels a circuit can go on",
    "The panels whose distribution system fits a circuit and that have free slots for it, optionally those whose name contains `search`, with their distribution system and free slots.",
    crate::mep_electrical::CompatibleParams,
    Vec<crate::mep_electrical::PanelChoice>,
    crate::mep_electrical::compatible_panels
);

read_command!(
    CircuitList,
    "circuit.list",
    "The circuits",
    "Every circuit (or those on a panel or with a device): number, kind, voltage, poles, members, panel, slots and load (VA).",
    crate::mep_electrical::ListParams,
    Vec<crate::mep_electrical::CircuitInfo>,
    crate::mep_electrical::list
);

edit_command!(
    PanelCreateFeeder,
    "panel.create_feeder",
    "Feed a panel from another",
    "Feeds a panel or transformer (`child_panel_id`) from another (`parent_panel_id`) (MEP-ELEC-006): a feeder circuit from the parent to the child's feed connector, on a `connection` breaker (taking slots) or the parent's feed-through lugs (no slot; one per parent, the parent's lugs on). The child's distribution system must match what the parent gives (a transformer gives its secondary: \"Transformer has no secondary distribution system\" until it is set). Loops are refused naming the chain. The parent then carries the child's whole connected load. Returns the feeder.",
    "Feeder",
    crate::mep_electrical::FeederParams,
    ElementId,
    crate::mep_electrical::create_feeder
);

edit_command!(
    PanelSetSupply,
    "panel.set_supply",
    "Choose a panel's supply",
    "Moves a panel's feeder to another source (`source_id`), with the checks of `panel.create_feeder`, or makes one. Returns the feeder.",
    "Select Supply",
    crate::mep_electrical::SupplyParams,
    ElementId,
    crate::mep_electrical::set_supply
);

read_command!(
    PanelHierarchy,
    "panel.hierarchy",
    "The distribution tree",
    "The panels from `root_id` (or every panel without a supply) down through their feeders: name, connected load (VA, sub-panels included), feeder and children.",
    crate::mep_electrical::HierarchyParams,
    Vec<crate::mep_electrical::PanelNode>,
    crate::mep_electrical::hierarchy
);

edit_command!(
    CircuitSet,
    "circuit.set",
    "Set a circuit's load name and rating",
    "Sets a circuit's `load_name` (empty: made from its loads again, the load classification and first device, or for a feeder the panel it feeds), `rating` (the protective device, amperes; 0 clears it), `cable_type` (a cable type's id; empty: the project's default), `cable_size` (a conductor size's id for the phase conductors, kept with a warning when it fails the checks; empty: sized automatically) and `runs` (parallel sets) (MEP-ELEC-005). Sizing re-runs on every change. Panel schedules show and edit the load name and rating.",
    "Circuit Properties",
    crate::mep_electrical::CircuitSetParams,
    (),
    crate::mep_electrical::set_circuit
);

edit_command!(
    LoadClassificationCreate,
    "load_classification.create",
    "Make a load classification",
    "Adds a load classification (MEP-ELEC-009): `name` (1–64 characters, unique without case), `demand_factor_id` (default constant 100 %) and `space_load`. Connectors name it in `electrical.load_classification`. Returns its id.",
    "Load Classification",
    crate::mep_loads::ClassificationCreateParams,
    String,
    crate::mep_loads::create_classification
);

edit_command!(
    LoadClassificationSet,
    "load_classification.set",
    "Change a load classification",
    "Renames a load classification (every connector naming it is renamed too) or changes its `demand_factor_id` or `space_load` (MEP-ELEC-009). Panel schedule summaries follow.",
    "Load Classification",
    crate::mep_loads::ClassificationSetParams,
    (),
    crate::mep_loads::set_classification
);

edit_command!(
    LoadClassificationDelete,
    "load_classification.delete",
    "Delete a load classification",
    "Deletes a load classification (MEP-ELEC-009); while connectors use it, `replace_with` (another classification's id) is required and they take it. The defaults (Lighting, Power, HVAC, Motor, Other) cannot be deleted. Returns how many connectors moved.",
    "Delete Load Classification",
    crate::mep_loads::ClassificationDeleteParams,
    usize,
    crate::mep_loads::delete_classification
);

edit_command!(
    DemandFactorCreate,
    "demand_factor.create",
    "Make a demand factor",
    "Adds a demand factor (MEP-ELEC-010): `name`, `method` (constant, quantity: by the number of loads, load: by their total VA), `ranges` [{from, to?, percent}] (one for constant), `incremental` (each range's percentage on the part in it; otherwise the whole total at the percentage of its range) and `motor_percents` (the largest-motor rule: motor loads in descending order, the last percentage for the rest). Returns its id.",
    "Demand Factor",
    crate::mep_loads::DemandFactorParams,
    String,
    crate::mep_loads::create_demand_factor
);

edit_command!(
    DemandFactorSet,
    "demand_factor.set",
    "Change a demand factor",
    "Changes a demand factor's name, method, ranges, incremental flag or motor percentages (MEP-ELEC-010).",
    "Demand Factor",
    crate::mep_loads::DemandFactorSetParams,
    (),
    crate::mep_loads::set_demand_factor
);

edit_command!(
    DemandFactorDelete,
    "demand_factor.delete",
    "Delete a demand factor",
    "Deletes a demand factor no load classification uses (MEP-ELEC-010); the 100 % default is kept.",
    "Delete Demand Factor",
    crate::mep_loads::DemandFactorDeleteParams,
    (),
    crate::mep_loads::delete_demand_factor
);

read_command!(
    DemandFactorEvaluate,
    "demand_factor.evaluate",
    "Apply a demand factor",
    "The demand (VA) of `loads` (VA each, or {va, motor}) under a demand factor (MEP-ELEC-010).",
    crate::mep_loads::EvaluateParams,
    f64,
    crate::mep_loads::evaluate_command
);

edit_command!(
    PanelScheduleCreate,
    "panel_schedule.create",
    "Make panel schedules",
    "Makes a panel schedule for each of `panel_ids` (MEP-ELEC-013): a live schedule view of the panel's circuits in slot order (number, load name, trip, poles, load per phase, total, classification), feeders on lugs, the totals and currents per phase, and the loads summary by load classification with demand factors (sub-panels rolling up by their own classifications). Load names and trips are edited in their cells. Returns the schedules.",
    "Panel Schedule",
    crate::mep_panel_schedule::CreateParams,
    Vec<ElementId>,
    crate::mep_panel_schedule::create
);

edit_command!(
    PanelScheduleChangePanel,
    "panel_schedule.change_panel",
    "Show another panel in a panel schedule",
    "Points a panel schedule (`view_id`) at another panel (MEP-ELEC-015).",
    "Change Panel",
    crate::mep_panel_schedule::ChangePanelParams,
    (),
    crate::mep_panel_schedule::change_panel
);

read_command!(
    PanelLoadsSummary,
    "panel.loads_summary",
    "A panel's loads by classification",
    "A panel's connected load by load classification, through its sub-panels, with each classification's demand factor and the estimated demand (VA) (MEP-ELEC-010, 013).",
    crate::mep_panel_schedule::SummaryParams,
    Vec<crate::mep_panel_schedule::SummaryLine>,
    crate::mep_panel_schedule::summary_command
);

read_command!(
    LoadClassificationList,
    "load_classification.list",
    "The load classifications",
    "The load classifications (MEP-ELEC-009) with their demand factor, space-load flag, how many connectors name them and whether they are defaults. Demand factors are in `electrical_settings.get`.",
    crate::mep_electrical::Empty,
    Vec<crate::mep_loads::ClassificationInfo>,
    crate::mep_loads::list_classifications
);

edit_command!(
    CableTrayCreate,
    "cable_tray.create",
    "Draw a cable tray run",
    "Draws a cable tray run (MEP-ELEC-016) along `points` ([x, y] at `middle_elevation` above `level`, 3.0 m by default, or [x, y, z]), `width` × `height`, of `type_id` (the built-in Ladder Cable Tray when omitted). Bends go at corners with a radius of the width × the type's multiplier (or `bend_radius`; below half the width is refused): fittings, or for a type without fittings curved segments; a change of height is a riser between two bends; a tee where the run ends on a tray's side. Returns the segments, fittings and what it connected to.",
    "Cable Tray",
    crate::mep_runs::CreateParams,
    crate::mep_runs::RunReport,
    crate::mep_runs::create_tray
);

edit_command!(
    ConduitCreate,
    "conduit.create",
    "Draw a conduit run",
    "Draws a conduit run (MEP-ELEC-017) along `points` ([x, y] at `middle_elevation`, 2.8 m by default, or [x, y, z]) of `diameter` (one of the type's standard's sizes; others are refused), of `type_id` (the built-in Conduit, IEC 61386, when omitted). Bends take the standard's radius for the diameter, else the diameter × the type's multiplier (or `bend_radius`): fittings, or for a type without fittings curved segments. Returns the segments, fittings and what it connected to.",
    "Conduit",
    crate::mep_runs::CreateParams,
    crate::mep_runs::RunReport,
    crate::mep_runs::create_conduit
);

edit_command!(
    CableTraySplit,
    "cable_tray.split",
    "Split a cable tray",
    "Cuts a straight tray segment (`id`) at `point` ([x, y] in plan, or [x, y, z]; the middle when omitted) into two connected segments, with a coupling where the type's routing has a union row. Returns the segments and coupling.",
    "Split",
    crate::mep_runs::SplitParams,
    crate::mep_runs::SplitReport,
    crate::mep_runs::split
);

edit_command!(
    ConduitSplit,
    "conduit.split",
    "Split a conduit",
    "Cuts a straight conduit (`id`) at `point` into two connected lengths, with a coupling where the type's routing has one (the built-in conduit type does). Returns the segments and coupling.",
    "Split",
    crate::mep_runs::SplitParams,
    crate::mep_runs::SplitReport,
    crate::mep_runs::split
);

edit_command!(
    CableTrayFittingPlace,
    "cable_tray_fitting.place",
    "Place a cable tray fitting",
    "Places a tray fitting (MEP-ELEC-016) of `part` (elbow, tee, cross, coupling, cap) at `point` ([x, y] in plan or [x, y, z]) or on `host_id`: on an open end it takes the end's size and connects (an elbow turns `angle` degrees, 90 by default, towards `direction`, level left by default, with the type's bend radius or `bend_radius`); along a tray a tee, cross or coupling splits it and keeps both parts connected (a tee's branch towards `direction`). Refused on types without fittings, naming the type. Returns the fitting and the new part of a split tray.",
    "Cable Tray Fitting",
    crate::mep_runs::PlaceFittingParams,
    crate::mep_runs::PlaceReport,
    crate::mep_runs::place_tray_fitting
);

edit_command!(
    ConduitFittingPlace,
    "conduit_fitting.place",
    "Place a conduit fitting or box",
    "Places a conduit fitting (MEP-ELEC-017) of `part` (elbow, tee, cross, coupling, cap, box) as `cable_tray_fitting.place` does; a `box` (an IfcJunctionBox) has 2 to 6 `connectors`, and along a conduit splits it. Refused on types without fittings. Returns the fitting and the new part of a split conduit.",
    "Conduit Fitting",
    crate::mep_runs::PlaceFittingParams,
    crate::mep_runs::PlaceReport,
    crate::mep_runs::place_conduit_fitting
);

edit_command!(
    MepFittingUpgrade,
    "mep_fitting.upgrade",
    "Upgrade a fitting (\"+\")",
    "Turns an elbow into a tee, its run continuing straight with an open leg, or a tee into a cross (MEP-DUCT-003, MEP-ELEC-016). Connections are kept. Returns the new part.",
    "Upgrade Fitting",
    crate::mep_runs::FittingParams,
    crate::mep_runs::Part,
    crate::mep_runs::upgrade
);

edit_command!(
    MepFittingDowngrade,
    "mep_fitting.downgrade",
    "Downgrade a fitting (\"−\")",
    "Removes an open leg (`leg`, or the last open one): a cross becomes a tee, a tee whose other legs turn becomes an elbow; refused when the leg is connected, when the remaining legs are in line, and for boxes. Returns the new part.",
    "Downgrade Fitting",
    crate::mep_runs::FittingParams,
    crate::mep_runs::Part,
    crate::mep_runs::downgrade
);

edit_command!(
    ConduitCreateParallel,
    "conduit.create_parallel",
    "Make parallel conduits",
    "Copies a level conduit run (`run_ids`, any of its elements) `h_count` across (the original included) at `h_offset` to the left of its direction (negative: right) and `v_count` up at `v_offset` (MEP-ELEC-017), with `bend` same (every bend the type's radius) or concentric (the bends share their centres, so radii grow by the offset outward; only for conduit types without fittings). Runs with branches or vertical legs are refused. Returns the elements made.",
    "Parallel Conduits",
    crate::mep_runs::ParallelParams,
    Vec<ElementId>,
    crate::mep_runs::create_parallel
);

edit_command!(
    ConduitStandardCreate,
    "conduit_standard.create",
    "Make a conduit standard",
    "Adds a conduit standard to the Electrical Settings (MEP-ELEC-017): `name` and its `sizes` [{diameter (outside, metres), name?, bend_radius? (metres)}]. Conduit types name a standard; only its sizes can be drawn. Built in: iec_61386 (16–63 mm) and emt (1/2\" to 2\" with their bend radii). Returns its id.",
    "Conduit Standard",
    crate::mep_electrical::ConduitStandardParams,
    String,
    crate::mep_electrical::create_conduit_standard
);

edit_command!(
    CableTraySetSize,
    "cable_tray.set_size",
    "Resize cable tray",
    "Gives tray segments a new `width` and `height`; the legs of the fittings joined to them follow.",
    "Cable Tray Size",
    crate::mep_runs::SetSizeParams,
    (),
    crate::mep_runs::set_size
);

edit_command!(
    ConduitSetSize,
    "conduit.set_size",
    "Resize conduit",
    "Gives conduit a new `diameter` (one of its type's standard's sizes); the legs of the fittings joined to it follow.",
    "Conduit Size",
    crate::mep_runs::SetSizeParams,
    (),
    crate::mep_runs::set_size
);

read_command!(
    CircuitProperties,
    "circuit.properties",
    "A circuit's calculated properties",
    "A circuit's calculated properties (MEP-ELEC-005): number, load name, phase labels, elements, apparent load (VA), power factor, design current (A), rating, path mode, offset, points and length (m) and whether a custom path is out of date (MEP-ELEC-004), cable type, conductor size (sized automatically unless given, `cable_size_auto`), runs, voltage drop (V and % of the circuit's voltage) against its limit, the corrected ampacity where the table gives one, and warnings (a size that fails, no ampacity table, no panel).",
    crate::mep_cables::CircuitRefParams,
    crate::mep_cables::CircuitCalc,
    crate::mep_cables::properties
);

edit_command!(
    CircuitSizeCable,
    "circuit.size_cable",
    "Size a circuit's cable",
    "Sizes a circuit's phase conductors again (a size given by hand is dropped), optionally with its own voltage drop limit `max_vd_percent` (MEP-ELEC-005): the smallest size of the cable type's size system whose voltage drop over the path length is within the limit and which, where the ampacity table has rows for the cable's material and insulation, carries the rating (or the design current) after the correction factor. Returns the circuit's properties.",
    "Size Cable",
    crate::mep_cables::SizeParams,
    crate::mep_cables::CircuitCalc,
    crate::mep_cables::size_cable
);

edit_command!(
    CircuitSetPath,
    "circuit.set_path",
    "Set a circuit's path",
    "Sets a circuit's path (MEP-ELEC-004): `mode` farthest_device (the orthogonal route, X then Y then Z, from the panel's connector to the most distant device), all_devices (a daisy chain through every device by nearest neighbour) or custom (`nodes` [x, y, z] after the panel, then the farthest device; flagged when devices move), and `offset` (metres, −10 to 10: horizontal runs above the higher end, or below the lower for a negative one). Refused for a circuit without a panel.",
    "Circuit Path",
    crate::mep_cables::PathParams,
    (),
    crate::mep_cables::set_path
);

edit_command!(
    CircuitResetPath,
    "circuit.reset_path",
    "Reset a circuit's path",
    "Drops a custom path's nodes: the circuit runs to its farthest device again (MEP-ELEC-004).",
    "Reset Path",
    crate::mep_cables::CircuitRefParams,
    (),
    crate::mep_cables::reset_path
);

read_command!(
    CircuitPath,
    "circuit.path",
    "A circuit's path",
    "A circuit's path mode, nodes, offset, points, length (m) and whether it may be out of date (MEP-ELEC-004).",
    crate::mep_cables::CircuitRefParams,
    crate::mep_cables::PathOut,
    crate::mep_cables::path_command
);

edit_command!(
    ConductorSizeCreate,
    "conductor_size.create",
    "Add a conductor size",
    "Adds a conductor size (MEP-SET-003): `name` and `area_mm2`, or an AWG `gauge` (14, 2, 1/0 … 4/0; its area from the AWG definition). Built in: IEC 1–300 mm² and AWG 22 to 4/0 with 250–500 kcmil. Returns its id.",
    "Conductor Size",
    crate::mep_cables::ConductorSizeParams,
    String,
    crate::mep_cables::create_conductor_size
);

edit_command!(
    CableTypeCreate,
    "cable_type.create",
    "Add a cable type",
    "Adds a cable type (MEP-SET-003): `name`, `material_id` (copper, aluminium), `insulation_id` (pvc 70 °C, xlpe 90 °C, thhn 90 °C), and either `size_system` (iec or awg: circuits using it are sized from those sizes) or fixed `phase_conductors` {count, size_id}; `neutral` and `ground`. Built in: Cu/PVC (the default), Cu/XLPE, Al/XLPE and Cu THHN (AWG). Returns its id.",
    "Cable Type",
    crate::mep_cables::CableTypeParams,
    String,
    crate::mep_cables::create_cable_type
);

edit_command!(
    ConductorAmpacitySet,
    "conductor_ampacity.set",
    "Set conductor ampacities",
    "Sets rows of the ampacity table (MEP-SET-003) `rows` [{material, insulation, size, amperes}] (amperes 0 removes a row). Reviera ships no ampacities, as code tables may be copyrighted: enter your code's values. Sizing checks current capacity only for cables whose material and insulation have rows. Returns the table's row count.",
    "Ampacities",
    crate::mep_cables::AmpacityParams,
    usize,
    crate::mep_cables::set_ampacities
);

edit_command!(
    WiringSet,
    "wiring.set",
    "Change the wiring settings",
    "Sets the project's wiring settings (MEP-SET-002): `default_cable_type`, `max_voltage_drop_percent` (3 by default), `correction_factor` applied to ampacities (ambient and grouping together; 1 by default), `path_offset` (metres) and conductor `reactance` (Ω/m, 0.00008 by default).",
    "Wiring Settings",
    crate::mep_cables::WiringParams,
    (),
    crate::mep_cables::set_wiring
);

edit_command!(
    MepSize,
    "mep.size",
    "Size ducts or pipes",
    "Sizes the segments of a system (`system_id`) or those given (`element_ids`) from their design flow, summed from the terminals beyond each one, walking from the system's equipment (MEP-CALC-001, MEP-PIPE-005): `method` equal_friction (default; `targets.friction` 1 Pa/m for ducts, 300 Pa/m for pipes), friction, velocity (`targets.velocity`, 8 m/s for ducts, 1.5 m/s for pipes) or friction_velocity with `combine` and/or; the smallest size of the segment type's sizing table meeting them, within `restrict` {height, width, inner_diameter} (restrictions win, with a warning; rectangles keep an aspect ratio up to 4); `branch_rule` calculated_only, match_connector or larger_of for branches to terminals; `equation` colebrook or haaland. Domestic water converts the fixture units beyond each segment with the fixture-unit table (`flush_valves` for the flush-valve curve). Air at 20 °C; water by the system type's temperature. Size-locked segments are skipped; sanitary and vent systems are refused. Returns the report.",
    "Size",
    crate::mep_sizing::SizeParams,
    crate::mep_sizing::SizeReport,
    crate::mep_sizing::size
);

edit_command!(
    MechControlDevicePlace,
    "mech_control_device.place",
    "Place a control device",
    "Places a thermostat, humidistat, sensor or controller (a component type of the Mechanical Control Device, Sensor or Controller category) at `point` on `level`, `offset` (mounting height, 1.5 m by default) above it (MEP-DUCT-012): on the face of `host_wall` (`side` exterior or interior, else the side the point is on), or of the nearest wall face within 300 mm; with `work_plane`, or no wall near, free (`rotation`). On a wall it follows the face, its back against it. `controlled_ids` as mech_control_device.assign. Returns its id.",
    "Place Control Device",
    crate::mep_controls::PlaceParams,
    crate::ElementId,
    crate::mep_controls::place
);

edit_command!(
    PlumbingFixturePlace,
    "plumbing_fixture.place",
    "Place a plumbing fixture",
    "Places a plumbing fixture or waste terminal (a component type of the Plumbing Fixture or Waste Terminal category: basins, WCs, sinks, showers, urinals, floor drains) at `point` on `level`, `offset` above it (0 by default) (MEP-PIPE-006): on the face of `host_wall` (`side`), or of the nearest wall face within 300 mm, its back (+y, where its connectors are) against the face; it then follows the wall, and stays where it is with a warning if the wall is deleted. With `work_plane`, or no wall near, free (`rotation`). Returns its id.",
    "Place Plumbing Fixture",
    crate::mep_controls::PlaceParams,
    crate::ElementId,
    crate::mep_controls::place_fixture
);

edit_command!(
    SprinklerPlace,
    "sprinkler.place",
    "Place a sprinkler",
    "Places a sprinkler head (a component type of the Sprinkler category) at `point` on `level` (MEP-PIPE-007), by its type's mounting: pendent (and recessed, concealed, ceiling) heads hang from `host_ceiling`, or the ceiling on the level over the point, their top at its underside (`offset` from the underside), and follow it; with no ceiling, at the default ceiling height. Upright heads stand 3 m above the level, or on `host_ceiling`. Sidewall heads go on the face of `host_wall` or the nearest wall within 300 mm, 2.45 m up, their back against it. `work_plane`: hosted by nothing, `offset` above the level. Returns its id.",
    "Place Sprinkler",
    crate::mep_sprinklers::PlaceParams,
    crate::ElementId,
    crate::mep_sprinklers::place
);

edit_command!(
    SprinklerTypeSet,
    "sprinkler_type.set",
    "Set sprinkler head properties",
    "Sets a sprinkler type's head (MEP-PIPE-007): `mounting` (pendant, recessed_pendant, concealed, ceiling, upright, sidewall, cutoff, other), `k_factor` (L/min/bar^½), `activation_temperature` (°C), `response` (quick, standard), `activation` (bulb, fusible_solder), `coverage_area` (m²), `connection_size` (m). Kept as Pset_FireSuppressionTerminalTypeSprinkler on the type: the K-factor as DischargeCoefficient, the temperature in kelvin; a bulb's liquid colour follows its rating. Heads are chosen, not sized: Reviera does not select them. Returns the type.",
    "Sprinkler Type Properties",
    crate::mep_sprinklers::SetTypeParams,
    crate::ElementId,
    crate::mep_sprinklers::set_type
);

edit_command!(
    HydronicSetEnabled,
    "hydronic.set_enabled",
    "Turn hydronic network analysis on or off",
    "Turns hydronic network analysis on or off (Mechanical Settings › Pipe › Calculation, MEP-PIPE-008). Off by default.",
    "Hydronic Network Analysis",
    crate::mep_hydronic::EnabledParams,
    (),
    crate::mep_hydronic::set_enabled
);

read_command!(
    HydronicAnalyze,
    "hydronic.analyze",
    "Analyse hydronic loops",
    "Analyses the closed-loop hydronic networks (MEP-PIPE-008; the analysis must be on): the loops left joined when hydraulic separations are taken out, each with one pump, primary where a boiler or chiller is, secondary and tertiary across separations. Per loop: the pump's flow (its terminals' design flows, plus the flows of the loops beyond the separations it serves) and head (the largest circuit loss from discharge through a terminal or separation back to suction, from pipe friction, fitting and equipment connector losses), the critical terminal and each load's circuit loss. Loops that cannot be analysed are listed in `problems` (two pumps in one loop, no source, no pipes, no terminals, an open circuit).",
    crate::mep_hydronic::AnalyzeParams,
    crate::mep_hydronic::HydronicReport,
    crate::mep_hydronic::analyze
);

edit_command!(
    HydraulicSeparationAdd,
    "hydraulic_separation.add",
    "Add a hydraulic separation",
    "Marks a pipe, pipe fitting or tank as a hydraulic separation (a decoupler, MEP-PIPE-008): with the tees joined directly to it (a common pipe between close tees) it splits the network into primary, secondary and tertiary loops, each pump reporting only its own loop's pressure drop. Returns the element.",
    "Add Hydraulic Separation",
    crate::mep_hydronic::SeparationParams,
    crate::ElementId,
    crate::mep_hydronic::add
);

edit_command!(
    HydraulicSeparationRemove,
    "hydraulic_separation.remove",
    "Remove a hydraulic separation",
    "Removes the hydraulic separation mark from a pipe, fitting or tank (MEP-PIPE-008).",
    "Remove Hydraulic Separation",
    crate::mep_hydronic::SeparationParams,
    (),
    crate::mep_hydronic::remove
);

edit_command!(
    AnalyticalPipeConnectionCreate,
    "analytical_pipe_connection.create",
    "Connect equipment to a pipe analytically",
    "Joins an equipment connector to a pipe logically, without modelling the connection (MEP-PIPE-009): it attaches at the connector's perpendicular projection onto the pipe's centreline, or the nearer pipe end beyond it, and follows both as they move. Only an open pipe connector whose classification matches the pipe's (its own or its system's) is joined (\"Connector 2: Hydronic Return, pipe is Hydronic Supply\" otherwise). The connector then counts as connected, the equipment joins the pipe's system, and flow passes in the hydronic analysis. Deleting the equipment or the pipe deletes it (`element.delete` deletes it alone); a physical pipe on the same connector replaces it. Returns its id.",
    "Analytical Connection",
    crate::mep_analytical::CreateParams,
    crate::ElementId,
    crate::mep_analytical::create
);

edit_command!(
    AnalyticalPipeConnectionCreateAuto,
    "analytical_pipe_connection.create_auto",
    "Connect equipment to the nearest pipes analytically",
    "Joins each open pipe connector of the equipment analytically to the nearest pipe of its classification within `radius` (3 m by default) (MEP-PIPE-009). Connectors with no such pipe are skipped and listed (`skipped`, and the warning `analytical_connection.skipped`). Returns the connections made.",
    "Analytical Connections",
    crate::mep_analytical::AutoParams,
    crate::mep_analytical::AutoReport,
    crate::mep_analytical::create_auto
);

edit_command!(
    AnalyticalPipeConnectionSet,
    "analytical_pipe_connection.set",
    "Set an analytical connection's loss",
    "Sets an analytical pipe connection's fixed loss (Pa; null for none), counted in the hydronic analysis (MEP-PIPE-009).",
    "Analytical Connection Loss",
    crate::mep_analytical::SetParams,
    (),
    crate::mep_analytical::set
);

read_command!(
    AnalyticalPipeConnectionList,
    "analytical_pipe_connection.list",
    "List analytical pipe connections",
    "Every analytical pipe connection (MEP-PIPE-009): its equipment, connector and pipe, where it attaches (and whether at a pipe end), and its fixed loss.",
    crate::mep_analytical::ListParams,
    Vec<crate::mep_analytical::Resolved>,
    crate::mep_analytical::list
);

edit_command!(
    DuctAccessoryPlace,
    "duct_accessory.place",
    "Insert a damper, filter, coil or sound attenuator",
    "Inserts an inline duct accessory (MEP-DUCT-004) `kind` fire_damper, smoke_damper, control_damper, balancing_damper, backdraft_damper, filter, heating_coil, cooling_coil or sound_attenuator, of the duct's size and shape and in its system: along a straight duct (`duct_id`, at `point` or its middle), which is split and connected through it, or on an open duct end at `point`. `length` face to face (default 300 mm for fire and smoke dampers, filters and coils, 200 mm for other dampers, 900 mm for sound attenuators). Refused with \"Not enough straight duct for this accessory\" when the straight duct is shorter than the accessory and two 10 mm stubs. An insulated duct's insulation continues over it. IFC: IfcDamper (FIREDAMPER, SMOKEDAMPER, CONTROLDAMPER, BALANCINGDAMPER, BACKDRAFTDAMPER), IfcFilter AIRPARTICLEFILTER, IfcCoil WATERHEATINGCOIL / WATERCOOLINGCOIL, IfcDuctSilencer ROUND / RECTANGULAR / FLATOVAL.",
    "Duct Accessory",
    crate::mep_runs::DuctAccessoryParams,
    crate::mep_runs::DuctAccessoryReport,
    crate::mep_runs::place_duct_accessory
);

edit_command!(
    FlexDuctCreate,
    "flex_duct.create",
    "Draw a flexible duct",
    "Draws a flex duct (MEP-DUCT-005): an IfcDuctSegment FLEXIBLESEGMENT along a spline through `points` (`[x, y, z]` above `level`, or `[x, y]` at `middle_elevation`, default 2.7 m), of a flexible type (`type_id`; the built-in Round or Rectangular Flex Duct when omitted: 3 mm roughness, minimum bend radius 1 × diameter). It starts on `start_port`, on an open duct connector at the first point, on a rigid duct's centreline (a tap from that duct's routing preferences is inserted; refused naming the size when none fits) or at a free point, and ends on `end_port`, a connector at the last point (joined, its direction followed) or a free point. `snap` finds connectors (and the start a centreline) in plan within 300 mm. Size: `diameter`, or `width` and `height`; else the connector's. System: `system_type_id`, else what it starts or ends on. A bend tighter than the type's minimum radius is drawn with a warning. Its length (for schedules, Qto_DuctSegmentBaseQuantities.Length and the pressure drop, times the type's loss multiplier) is the spline's.",
    "Flex Duct",
    crate::mep_flex::CreateParams,
    crate::mep_runs::RunReport,
    crate::mep_flex::create
);

edit_command!(
    DuctConvertToFlex,
    "duct.convert_to_flex",
    "Convert final duct segments to flex",
    "For each terminal in `terminal_ids` (MEP-DUCT-005 Convert to Flex), replaces the final rigid duct segment (with the elbow at the terminal, if any) by a flex duct of its size, system and level, from the segment's far end to the terminal's connector, when it is no longer than `max_length` (metres). Terminals whose final run is longer, or that do not end in a plain straight duct, are left and listed in `skipped` with the reason. `type_id`: a flexible type (default the built-in of the duct's shape). Returns `ids` (the flex ducts) and `skipped`.",
    "Convert to Flex Duct",
    crate::mep_flex::ConvertParams,
    crate::mep_flex::ConvertReport,
    crate::mep_flex::convert
);

edit_command!(
    MepRunJustify,
    "mep_run.justify",
    "Justify a run on a face or edge",
    "Aligns a duct, pipe, cable tray or conduit run whose segments change size (MEP-DUCT-006): `element_ids` are its segments and fittings, one chain without branches (else refused: \"Justify needs a single run without branches\"); the `control_end` segment (`start`, the end it was drawn from, or `end`) stays put and every other segment moves square to its axis so its `alignment` point (TL, TC, TR, ML, MC, MR, BL, BC, BR: top, middle or bottom × left, centre or right, as seen travelling from the control; round sections use their diameter) continues the control's line through transitions, couplings and accessories; across elbows segments move with what they follow. Fittings follow, transitions becoming eccentric. Size-locked and sloped segments are only moved. Each segment keeps the justification, so a later size change keeps its aligned face in place. A run end joined to something outside the run that would move is refused, naming it. Returns the control segment and the elements moved.",
    "Justify",
    crate::mep_justify::JustifyParams,
    crate::mep_justify::JustifyReport,
    crate::mep_justify::justify
);

edit_command!(
    MepSegmentHeal,
    "mep_segment.heal",
    "Rejoin two segments",
    "Joins two straight duct or pipe segments in line and of one size, whose facing ends are open (as deleting an inline accessory leaves them), into the first (MEP-DUCT-004 Heal); the second is deleted and the far ends keep their connections. Returns the joined segment.",
    "Heal",
    crate::mep_runs::HealParams,
    crate::ElementId,
    crate::mep_runs::heal
);

edit_command!(
    MepCoveringTypeCreate,
    "mep_covering_type.create",
    "Create an insulation or lining type",
    "Creates an insulation or duct lining type (MEP-DUCT-004): `covering` insulation or lining, a `material`, and for lining the air side's `roughness` (metres; 1.5 mm by default) used by the pressure-drop calculation. IFC: IfcCoveringType INSULATION. Returns its id.",
    "Create Covering Type",
    crate::mep_coverings::CreateTypeParams,
    crate::ElementId,
    crate::mep_coverings::create_type
);

edit_command!(
    MepInsulationAdd,
    "mep_insulation.add",
    "Add insulation",
    "Adds insulation of `type_id` (the built-in Duct Insulation when omitted) and `thickness` (25 mm by default; above 0) outside each duct, pipe, fitting or accessory (MEP-DUCT-004); one already there takes the new type and thickness. Other elements are skipped with the warning `mep_insulation.skipped`. Insulation follows its host's size and position and is deleted with it; it is included in clash checks and does not change pressure drops. IFC: IfcCovering INSULATION covering the host (IfcRelCoversBldgElements), thickness as Qto_CoveringBaseQuantities.Width. Returns the coverings.",
    "Add Insulation",
    crate::mep_coverings::AddParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::add_insulation
);

edit_command!(
    MepInsulationSet,
    "mep_insulation.set",
    "Edit insulation",
    "Changes the type or thickness of the insulation on the elements (or of the given insulation) (MEP-DUCT-004 Edit Insulation). Returns the coverings changed.",
    "Edit Insulation",
    crate::mep_coverings::SetParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::set_insulation
);

edit_command!(
    MepInsulationRemove,
    "mep_insulation.remove",
    "Remove insulation",
    "Deletes the insulation on the elements (or the given insulation) (MEP-DUCT-004). Returns the coverings deleted.",
    "Remove Insulation",
    crate::mep_coverings::RemoveParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::remove_insulation
);

edit_command!(
    DuctLiningAdd,
    "duct_lining.add",
    "Add duct lining",
    "Adds lining of `type_id` (the built-in Duct Lining when omitted) and `thickness` (25 mm by default; above 0 and less than half the duct's smallest side) inside each duct and duct fitting (MEP-DUCT-004); others are skipped with the warning `duct_lining.skipped`. Lining reduces the free section the pressure-drop calculation uses (each side less twice the thickness) and gives it the lining type's roughness. IFC: IfcCovering INSULATION named Duct Lining covering the duct. Returns the coverings.",
    "Add Lining",
    crate::mep_coverings::AddParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::add_lining
);

edit_command!(
    DuctLiningSet,
    "duct_lining.set",
    "Edit duct lining",
    "Changes the type or thickness of the lining in the ducts (or of the given lining) (MEP-DUCT-004). Returns the coverings changed.",
    "Edit Lining",
    crate::mep_coverings::SetParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::set_lining
);

edit_command!(
    DuctLiningRemove,
    "duct_lining.remove",
    "Remove duct lining",
    "Deletes the lining in the ducts (or the given lining) (MEP-DUCT-004). Returns the coverings deleted.",
    "Remove Lining",
    crate::mep_coverings::RemoveParams,
    Vec<crate::ElementId>,
    crate::mep_coverings::remove_lining
);

read_command!(
    MepCoveringList,
    "mep_covering.list",
    "List insulation and lining",
    "Every insulation and duct lining (of the given hosts, when `element_ids` is given): its kind, host, type and thickness (MEP-DUCT-004).",
    crate::mep_coverings::ListParams,
    Vec<crate::mep_coverings::CoveringInfo>,
    crate::mep_coverings::list
);

edit_command!(
    MechControlDeviceAssign,
    "mech_control_device.assign",
    "Assign controlled elements",
    "Sets the elements a control device controls (`add` to add to them): equipment, VAV boxes, dampers, terminals, of any discipline; not the device itself, another control device or a type (MEP-DUCT-012). In IFC: an IfcRelFlowControlElements to each distribution flow element.",
    "Assign Controlled Element",
    crate::mep_controls::AssignParams,
    (),
    crate::mep_controls::assign
);

read_command!(
    MechControlDeviceList,
    "mech_control_device.list",
    "Control devices",
    "Every control device: its type name, IFC class and predefined type, host wall and the elements it controls (MEP-DUCT-012).",
    crate::mep_controls::ListParams,
    Vec<crate::mep_controls::DeviceInfo>,
    crate::mep_controls::list
);

edit_command!(
    MepRunSetSlope,
    "mep_run.set_slope",
    "Slope a section of pipe",
    "Sets the slope (rise over run; 0.02 for 2 %) of a connected section of pipe segments and fittings (`element_ids`) from `control_point` ({element_id, port}: one of its two open ends, the lowest by default), which stays; the pipe rises away from it (MEP-PIPE-002). Fittings follow, their run legs tilted; vertical segments keep their geometry; taps ride on their segments. Segments joined from outside (branches, the run beyond) have their joined end moved and their slope recalculated, fittings joined from outside move with them; anything else that cannot follow refuses the change, naming it. Refused \"Selection is not a single connected section\" when the elements are not one run. A slope not in the allowed list is applied with a warning. Writes Pset_PipeSegmentOccurrence.Gradient.",
    "Slope",
    crate::mep_slopes::SlopeParams,
    crate::mep_slopes::SlopeReport,
    crate::mep_slopes::set_slope
);

edit_command!(
    PipeFittingPlace,
    "pipe_fitting.place",
    "Place a pipe fitting",
    "Places a pipe fitting (MEP-PIPE-004) `part` elbow (`angle`, 90° by default, turning toward `direction`), tee, cross, coupling or cap on an open pipe end (`host_id` or found at `point`), taking its size and connecting; a tee, cross or coupling along a pipe splits it. Elbows along a pipe are refused: \"Elbows must be placed at an open end\".",
    "Pipe Fitting",
    crate::mep_runs::PlaceFittingParams,
    crate::mep_runs::PlaceReport,
    crate::mep_runs::place_pipe_fitting
);

edit_command!(
    PipeAccessoryPlace,
    "pipe_accessory.place",
    "Insert a valve, strainer or meter",
    "Inserts an inline pipe accessory (MEP-PIPE-004) `kind` gate_valve, ball_valve, butterfly_valve, globe_valve, check_valve, pressure_reducing_valve, relief_valve, strainer or water_meter in a pipe (`pipe_id`, at `point` or its middle), which is split; it takes the pipe's size and system; `rotation` turns a valve's handle about the pipe; `connection` flanged (from 50 mm by default) or threaded: a flanged one gets the routing preferences' union-role flange on each side. IFC: IfcValve (ISOLATING for gate, ball and butterfly, REGULATING globe, CHECK, PRESSUREREDUCING, PRESSURERELIEF), IfcFilter STRAINER, IfcFlowMeter WATERMETER; flanges IfcPipeFitting CONNECTOR.",
    "Pipe Accessory",
    crate::mep_runs::AccessoryParams,
    crate::mep_runs::AccessoryReport,
    crate::mep_runs::place_accessory
);

edit_command!(
    MepFittingTeeToCross,
    "mep_fitting.tee_to_cross",
    "Change a tee to a cross",
    "Changes a tee into a cross: a fourth leg opposite the branch, of the run's size, left open (MEP-PIPE-004).",
    "Tee to Cross",
    crate::mep_runs::TeeToCrossParams,
    (),
    crate::mep_runs::tee_to_cross
);

edit_command!(
    PipeCreateParallel,
    "pipe.create_parallel",
    "Parallel pipes",
    "Copies a pipe run (`run_ids`: any of its elements finds the whole run, level or sloped, without tees, crosses or taps) into a bank (MEP-PIPE-003): `h_count` across and `v_count` up, the original included (1–20 each), `h_offset` (to the left of the run's direction; negative: right) and `v_offset` (upward) centre to centre, at least the pipe's outside size (\"Offset smaller than pipe outside size\"). Each copy stays parallel through the bends with elbows by the type's routing preferences (concentric), the same type, size, system type and slope; copies join a system only through its equipment (reported), open ends stay open. Refused naming the tee, cross or tap that branches the run. Returns the new elements.",
    "Parallel Pipes",
    crate::mep_runs::ParallelParams,
    Vec<crate::ElementId>,
    crate::mep_runs::create_parallel_pipes
);

edit_command!(
    MepSettingsSetSlopes,
    "mep_settings.set_slopes",
    "Set the allowed pipe slopes",
    "Sets the allowed pipe slopes (rise over run, above 0 and at most 1) that the Slope Editor offers (Mechanical Settings › Slopes, MEP-PIPE-002).",
    "Pipe Slopes",
    crate::mep_slopes::SlopesParams,
    (),
    crate::mep_slopes::set_slopes
);

read_command!(
    MepSettingsSlopes,
    "mep_settings.slopes",
    "The allowed pipe slopes",
    "The allowed pipe slopes (rise over run): the project's, else Reviera's defaults (0.5 %, 1 %, 1/8 in per ft, 2 %, 1/4 in per ft, 4 %).",
    crate::mep_slopes::ListSlopesParams,
    Vec<f64>,
    crate::mep_slopes::list_slopes
);

edit_command!(
    MepRunCreateRiser,
    "mep_run.create_riser",
    "Create a riser",
    "From an open duct, pipe, cable tray or conduit connector (`element_id`, `connector_index`: a segment's 0 start or 1 end), an elbow from the routing preferences and one vertical segment of the same type, size and system up (or down, a drop) to `top_level` + `top_offset`, its far end open (MEP-DUCT-011). With `bind` (default) the end keeps to the level when the level's elevation changes, and the runs joined beyond it move with it. Refused, creating nothing: a connector that is not open, a surface connector, no elbow row for the size (\"No Elbow in routing preferences for 300 mm\"), the target at the start (\"Riser length is zero\") or too close for the elbow. Returns the segment and fitting.",
    "Create Riser",
    crate::mep_runs::RiserParams,
    crate::mep_runs::RunReport,
    crate::mep_runs::create_riser
);

edit_command!(
    MepRunBindRiser,
    "mep_run.bind_riser",
    "Bind a riser to a level",
    "Binds a vertical segment's end to `top_level` + `top_offset`, moving it (and the runs beyond it) there; without `top_level`, unbinds it, keeping its geometry (MEP-DUCT-011).",
    "Bind Riser",
    crate::mep_runs::BindRiserParams,
    (),
    crate::mep_runs::bind_riser
);

read_command!(
    SizingCalculatorCandidates,
    "sizing_calculator.candidates",
    "Candidate sizes for a duct or pipe",
    "The Sizing Calculator (MEP-CALC-005): candidate sizes for one duct or pipe segment (`element_id`: its flow, kept or from its system, domain, shape, type and roughness) or a `flow` in m³/s with `domain` and `shape`, against `friction` (Pa/m) and `velocity` (m/s) bands {target, tolerance 0–0.5, 10 % by default; targets 1 Pa/m and 5 m/s for ducts, 300 Pa/m and 1.5 m/s for pipes} and an `aspect` maximum {max, 3 by default}; with no basis, friction. From the type's sizing table (rectangles wider than high, or `swap`), within `restrict`; each with friction, velocity, aspect ratio, equivalent diameter and deviations, closest to the friction target first. When none meets every basis, the three nearest, with the note \"No size within tolerance\". Nothing changes; apply with duct.set_size or pipe.set_size.",
    crate::mep_calculator::CandidatesParams,
    crate::mep_calculator::CandidatesResult,
    crate::mep_calculator::candidates
);

edit_command!(
    MepNetworkCalculate,
    "mep_network.calculate",
    "Calculate pressure drop",
    "Calculates a duct or pipe system's network (`system_id`; every duct and pipe system when omitted, refusals then being warnings) from its equipment (MEP-CALC-002): each segment's and fitting's design flow, velocity, friction (Darcy–Weisbach; `equation` colebrook or haaland, the project's setting by default) and loss (segments by friction over their length, fittings by Reviera's coefficients of the velocity pressure, taps by the branch coefficient, terminals by their connector's coefficient or specific loss; overrides win), the critical path to the terminal with the highest loss, static pressures and terminals' excess pressure. Systems whose type calculates Flow Only get flows and velocities without losses. The results are kept on the system and its elements until their inputs change (then stale). Returns each system's critical loss and how many analytical segments it has.",
    "Calculate Network",
    crate::mep_network::CalculateParams,
    crate::mep_network::CalculateReport,
    crate::mep_network::calculate
);

read_command!(
    AnalyticalSegmentList,
    "analytical_segment.list",
    "A system's analytical segments",
    "The kept network results of a system (MEP-CALC-002): its analytical segments (one per segment and fitting: network id, size, flow m³/s, velocity m/s, length, friction Pa/m, coefficient, loss Pa, static pressure Pa, critical, overridden, branches), terminals (own and path loss, excess pressure), the critical path from the equipment, the critical loss and total flow, and `stale` when the model has changed since it was calculated. Refused when never calculated.",
    crate::mep_network::ListParams,
    crate::mep_network::NetworkResult,
    crate::mep_network::list
);

read_command!(
    SystemInspectorStart,
    "system_inspector.start",
    "Inspect a system's pressure",
    "Starts the System Inspector on a duct or pipe system (MEP-CALC-003): refused for fire protection, without equipment, unless its type's Calculations is All or Performance, with open connectors (\"System has open connectors: N. Run Show Disconnects\"), or while its results are stale (run mep_network.calculate). Returns its kept results with its sections: stretches of constant flow between junctions, each with its elements, flow, velocity, loss, static pressure in and out, critical flag, centreline and terminal; and terminals' excess pressure. In the window the inspector draws flow arrows, highlights the critical path and shows a section's values on hover; it changes nothing and ends when the model is edited.",
    crate::mep_network::InspectParams,
    crate::mep_network::NetworkResult,
    crate::mep_network::inspect
);

edit_command!(
    AnalyticalSegmentOverride,
    "analytical_segment.override",
    "Override a segment's loss",
    "Gives a duct or pipe segment or fitting a measured or given `loss` (Pa), or a fitting a `coefficient`, in place of the calculated one; `clear` removes them (MEP-CALC-002). The network's results become stale until it is calculated again; reports mark it overridden.",
    "Override Loss",
    crate::mep_network::OverrideParams,
    (),
    crate::mep_network::set_override
);

edit_command!(
    MepSettingsSetFrictionEquation,
    "mep_settings.set_friction_equation",
    "Set the friction equation",
    "Sets the project's friction equation for network calculations: colebrook (default) or haaland (MEP-CALC-002).",
    "Friction Equation",
    crate::mep_network::EquationParams,
    (),
    crate::mep_network::set_equation
);

edit_command!(
    PressureLossReportExport,
    "pressure_loss_report.export",
    "Write a pressure loss report",
    "Writes the pressure loss report of `system_ids` to `path` as `format` csv (SI base units, a header naming them; one row per segment, fitting, total and critical-path step) or html (the project's units) (MEP-CALC-004). `settings`: a `preset_id`, or `sections` (project, system, total, segments, fittings, critical_path; all by default), `without_fittings`, `without_critical_path`. Only systems whose type calculates All, with equipment, are reported; stale or missing results are calculated first (an undoable change). `overwrite` to replace a file.",
    "Pressure Loss Report",
    [Effect::Edit, Effect::FileWrite],
    crate::mep_network::ReportParams,
    crate::mep_network::ReportResult,
    crate::mep_network::export_report
);

edit_command!(
    PressureLossReportSavePreset,
    "pressure_loss_report.save_preset",
    "Save pressure loss report settings",
    "Saves report settings (sections, without_fittings, without_critical_path) as a named preset in the MEP settings, so a team uses one format (MEP-CALC-004). Returns its id.",
    "Report Preset",
    crate::mep_network::PresetParams,
    String,
    crate::mep_network::save_preset
);

edit_command!(
    MepRunLockSize,
    "mep_run.lock_size",
    "Lock segments' sizes",
    "Size-locks (`locked` true) or unlocks segments: sizing leaves locked ones as they are and lists them as skipped (MEP-CALC-001). Returns how many changed.",
    "Lock Size",
    crate::mep_sizing::LockParams,
    usize,
    crate::mep_sizing::lock_size
);

edit_command!(
    FixtureUnitMethodSet,
    "fixture_unit_method.set",
    "Import a fixture-unit table",
    "Imports a fixture-unit table (MEP-PIPE-005): `name`, `source` (code and edition), `csv` with rows `fixture_units, flow_tank, flow_valve` ascending (a header line is skipped; a row out of order is refused naming its line), in `unit` l/s or gpm; for `system_types` (domestic water type ids), else as the default. Domestic water sizing converts the fixture units beyond each segment with it. Reviera ships none. Returns its id.",
    "Fixture-Unit Table",
    crate::mep_sizing::FixtureTableParams,
    String,
    crate::mep_sizing::set_fixture_table
);

read_command!(
    FixtureUnitMethodEvaluate,
    "fixture_unit_method.evaluate",
    "Convert fixture units to flow",
    "The flow (l/s) of a `fixture_units` total by a table (`table_id`, the default when omitted), on the flush-tank or (`flush_valves`) flush-valve curve, interpolated.",
    crate::mep_sizing::EvaluateFixtureParams,
    f64,
    crate::mep_sizing::evaluate_fixture
);

read_command!(
    MepPenetrations,
    "mep.penetrations",
    "Where runs pass through walls, floors and beams",
    "The crossings of ducts, pipes, cable tray and conduit (`run_ids`, or every run) with walls, floors, ceilings and beams (MEP-PEN-001): the run, host and its kind, the mode (create: an opening is made, following the run; flag: reported only, the default for framing), the point, clearance, size and the opening's outline (along and up a wall, or in plan for a slab).",
    crate::mep_penetrations::PenetrationsParams,
    Vec<crate::mep_penetrations::Crossing>,
    crate::mep_penetrations::penetrations
);

edit_command!(
    MepSetPenetrationRule,
    "mep.set_penetration_rule",
    "Set a penetration rule",
    "Sets how runs through a `host_category` (wall, floor, ceiling, framing) are treated, for one `system` classification or every one (MEP-PEN-001): `mode` create (an opening, the default for walls, floors and ceilings), flag (reported only; the only choice for framing) or never, and the `clearance` around the run (25 mm by default).",
    "Penetration Rule",
    crate::mep_penetrations::RuleParams,
    (),
    crate::mep_penetrations::set_rule
);

edit_command!(
    GutterCreate,
    "gutter.create",
    "Hang a gutter along a roof's eaves",
    "Makes a gutter (MEP-PIPE-010) along `roof_id`'s eave curves `edges` (keys from roof.eaves, in order round the roof; every eave when empty): an IfcPipeSegment (GUTTER) of `profile` half_round (default), k_style or box, `width` (125 mm) and `depth` (half the width), hung on the fascia (or at the eave), mitred at corners, falling at `fall` (1:500) toward its outlets. It follows the roof: changing the overhang, fascia or bearing moves it. Returns the gutter.",
    "Gutter",
    crate::mep_gutters::CreateParams,
    ElementId,
    crate::mep_gutters::create
);

edit_command!(
    GutterSet,
    "gutter.set",
    "Change a gutter",
    "Changes a gutter's `profile`, `width`, `depth` or `fall` (MEP-PIPE-010).",
    "Gutter",
    crate::mep_gutters::SetParams,
    (),
    crate::mep_gutters::set
);

edit_command!(
    GutterAddOutlet,
    "gutter.add_outlet",
    "Add an outlet and downspout to a gutter",
    "Adds an outlet to a gutter (MEP-PIPE-010) `at` metres along it, or nearest a plan `point` (its middle when neither): an IfcPipeFitting (CONNECTOR) of the downspout's `diameter` (68 mm), with, given `downspout_to` ({level: id} or {point: [x, y, z]}), a downspout: down from the outlet, back toward the wall by the eave's offset with bends, then down to that level (or point). The gutter, outlet and downspout join one Storm Drainage system (made when needed); the gutter now falls toward the outlet. Returns where, the fitting, the downspout and the system.",
    "Gutter Outlet",
    crate::mep_gutters::OutletParams,
    crate::mep_gutters::OutletReport,
    crate::mep_gutters::add_outlet
);

read_command!(
    GutterSize,
    "gutter.size",
    "Check a gutter's and downspouts' sizes",
    "Checks a gutter (MEP-PIPE-010) at a design `rainfall` intensity (mm/h, 75 by default): the plan area of the roof faces draining to it, shared between its outlets by the length nearest each, gives each outlet's flow, checked against the gutter's capacity (Manning flowing full, n 0.012, at its fall) and the downspout's (an orifice under half the gutter's depth); too small ones are flagged.",
    crate::mep_gutters::SizeParams,
    crate::mep_gutters::SizeReport,
    crate::mep_gutters::size
);

edit_command!(
    DuctPlaceholderCreate,
    "duct_placeholder.create",
    "Draw duct placeholders",
    "Draws duct placeholders (MEP-DUCT-007): single-line IfcDuctSegments without fittings or body (IFC `Axis` only), one per leg of `points` (`[x, y]` at `middle_elevation` above `level`, 2.7 m by default, or `[x, y, z]`), of `type_id` (its routing preferences are used on conversion), `shape` and size (`width` × `height`, or `diameter`), carrying `system_type_id`. Ends that meet join each other at any angle and size, and connectors of equipment there; an end on another placeholder's side splits it there. Placeholders count in system flow and Check Systems, not in sizing, pressure drop or interference. Convert them with `placeholder.convert`.",
    "Duct Placeholder",
    crate::mep_runs::CreateParams,
    crate::mep_placeholders::PlaceholderReport,
    crate::mep_placeholders::create_duct
);

edit_command!(
    PipePlaceholderCreate,
    "pipe_placeholder.create",
    "Draw pipe placeholders",
    "Draws pipe placeholders (MEP-DUCT-007) as `duct_placeholder.create` does duct placeholders, round only (`diameter`), at 2.4 m by default; `slope` makes level legs fall (rise over run) from the first point, and conversion keeps it.",
    "Pipe Placeholder",
    crate::mep_runs::CreateParams,
    crate::mep_placeholders::PlaceholderReport,
    crate::mep_placeholders::create_pipe
);

edit_command!(
    PlaceholderConvert,
    "placeholder.convert",
    "Convert placeholders to ducts or pipes",
    "Converts the placeholders among `ids` (MEP-DUCT-007) into real ducts or pipes with fittings from each type's routing preferences: legs in line join directly (a transition where the size changes), legs at an angle by an elbow, three legs by a tee (two in line, the branch square), four by a cross. Each placeholder keeps its GlobalId, size, slope and system; the segments are shortened by the fittings' legs. Where a point cannot be fitted (more than four legs, a branch off square, a size change at a bend, no routing row, a placeholder not being converted, a connector not faced at its size) its ends stay open and are listed in `open` with the reason, and a warning names each. Returns `element_ids` (segments then fittings). Undo restores the placeholders.",
    "Convert Placeholder",
    crate::mep_placeholders::ConvertParams,
    crate::mep_placeholders::ConvertReport,
    crate::mep_placeholders::convert
);

edit_command!(
    AirTerminalPlace,
    "air_terminal.place",
    "Place an air terminal",
    "Places an air terminal (MEP-DUCT-009): a component of an Air Terminal type (`type_id`; diffusers, grilles, registers, louvres) at `point` on `level`, `offset` above it, turned by `rotation`. `flow` (m³/s) becomes its duct connector's preset flow. With `duct_id` (a straight level duct) it goes on the duct's bottom face at the point on its axis nearest `point`, its upward connector joined to a tap port the duct gains there. Its preset flow counts toward the supply, return or exhaust airflow of the room it stands in (`space.airflow`). Returns the terminal, the duct and the room.",
    "Air Terminal",
    crate::mep_equipment::AirTerminalParams,
    crate::mep_equipment::AirTerminalReport,
    crate::mep_equipment::place_air_terminal
);

read_command!(
    SpaceAirflow,
    "space.airflow",
    "Rooms' actual airflow",
    "Each room's actual supply, return and exhaust airflow (m³/s, MEP-DUCT-009): the preset flows of the air terminals standing in it, by their connectors' classification, with the terminals counted. `room_ids` narrows it; every room with terminals when empty. Room schedules show the same as Actual Supply, Return and Exhaust Airflow.",
    crate::mep_equipment::AirflowParams,
    Vec<crate::mep_equipment::SpaceAirflow>,
    crate::mep_equipment::airflow
);

edit_command!(
    MechEquipmentPlace,
    "mech_equipment.place",
    "Place mechanical equipment",
    "Places mechanical equipment (MEP-DUCT-009): a component of an air handler or other unitary equipment, VAV box, fan, boiler, chiller, pump or space heater (radiator) type at `point` on `level`, `offset` above it, turned by `rotation`; its connectors join those they meet. Returns its id.",
    "Mechanical Equipment",
    crate::mep_equipment::EquipmentParams,
    crate::id::ElementId,
    crate::mep_equipment::place_equipment
);

edit_command!(
    EquipmentSetCreate,
    "equipment_set.create",
    "Make an equipment set",
    "Groups two or more mechanical equipment units (`equipment_ids`, typically pumps) into a duty/standby set (MEP-DUCT-009): an IfcGroup with ObjectType EquipmentSet; `duty_ids` are on duty and the rest stand by. A unit is in one set at most. Only duty units count in `system.flow_totals`. Returns the set.",
    "Equipment Set",
    crate::mep_equipment::CreateSetParams,
    crate::id::ElementId,
    crate::mep_equipment::create_set
);

edit_command!(
    EquipmentSetAdd,
    "equipment_set.add",
    "Add units to an equipment set",
    "Adds `equipment_ids` to the set `set_id`, standing by unless `duty`.",
    "Add to Equipment Set",
    crate::mep_equipment::EditSetParams,
    (),
    crate::mep_equipment::add_to_set
);

edit_command!(
    EquipmentSetRemove,
    "equipment_set.remove",
    "Remove units from an equipment set",
    "Takes `equipment_ids` out of the set `set_id`; refused if no duty unit would be left.",
    "Remove from Equipment Set",
    crate::mep_equipment::EditSetParams,
    (),
    crate::mep_equipment::remove_from_set
);

edit_command!(
    EquipmentSetSetDuty,
    "equipment_set.set_duty",
    "Choose an equipment set's duty units",
    "Makes `duty_ids` (units of the set `set_id`) the duty units; the others stand by.",
    "Set Duty Units",
    crate::mep_equipment::SetDutyParams,
    (),
    crate::mep_equipment::set_duty
);

read_command!(
    EquipmentSetList,
    "equipment_set.list",
    "The equipment sets",
    "Every equipment set with its duty and standby units.",
    crate::builtin::NoParams,
    Vec<crate::mep_equipment::EquipmentSetInfo>,
    crate::mep_equipment::list_sets
);

read_command!(
    SystemFlowTotals,
    "system.flow_totals",
    "A system's flow totals",
    "A system's terminal flow (the preset flows of its terminals and fixtures, m³/s) and equipment flow (each unit's largest connector flow in the system); units standing by in an equipment set are listed but not counted (MEP-DUCT-009).",
    crate::mep_equipment::TotalsParams,
    crate::mep_equipment::FlowTotals,
    crate::mep_equipment::flow_totals
);

edit_command!(
    MepConnectInto,
    "mep.connect_into",
    "Connect a connector into a main",
    "Draws a duct or pipe (MEP-DUCT-009) from the open connector `connector_index` of `element_id` into the straight level main `target_id`, at the connector's size and the main's type: out along the connector (straight up or down to the main's height when it faces up or down), then square to the main, ending on its side, where the routing preferences put a tee or tap. Refused when the connector faces away from the main or the main does not pass it. Returns the run as `duct.create` does.",
    "Connect Into",
    crate::mep_equipment::ConnectIntoParams,
    crate::mep_runs::RunReport,
    crate::mep_equipment::connect_into
);

read_command!(
    MechanicalSettingsGet,
    "mechanical_settings.get",
    "Mechanical settings",
    "The project's Mechanical Settings (MEP-SET-001) at a dotted `path` (the whole tree when empty): `hidden_line` (draw, line_style, inside_gap, outside_gap, single_line_gap in paper mm); `duct` (fitting_annotation_scale, fitting_annotation_size, air_density kg/m³, air_viscosity Pa·s, size_separator, size_suffix, connector_separator, flat_on_top, flat_on_bottom, rise_drop_size, `angles` {mode: any | increment {step} | list {angles}}, `conversion` {main_type, branch_type, main_offset, branch_offset, flex, flex_max_length}, `sizes` {rectangular, oval, round: [{size m, in_lists, in_sizing}]}, `calculation.friction_equation` colebrook | haaland | altshul_tsal); `pipe` (annotation_size, size_prefix, size_suffix, connector_separator, angles, conversion, `segments` [{id, material, schedule, roughness, sizes [{nominal, inner, outer, in_lists, in_sizing}]}], `fluids` [{id, name, table [{temperature °C, density, viscosity Pa·s}]}], `slopes` (rise over run, zero first), `calculation` {hydronic_analysis, pressure_drop}).",
    crate::mep_mech_settings::GetParams,
    serde_json::Value,
    crate::mep_mech_settings::get
);

edit_command!(
    MechanicalSettingsSet,
    "mechanical_settings.set",
    "Change a mechanical setting",
    "Sets the Mechanical Setting at the dotted `path` (as `mechanical_settings.get` shows the tree) to `value`, checked before anything is kept: air properties above zero, angle increments above 0° and at most 90°, listed angles 0–180°, sizes above zero and not repeated, fluids' temperatures rising. `pipe.slopes` must keep zero (\"The zero slope cannot be removed\"); a slope over 45° is kept with a warning. Corners of duct and pipe runs, and placeholder conversion, keep to `duct.angles` and `pipe.angles`.",
    "Mechanical Settings",
    crate::mep_mech_settings::SetParams,
    (),
    crate::mep_mech_settings::set
);

read_command!(
    DuctSizeTableGet,
    "duct_size_table.get",
    "A duct size table",
    "A duct shape's size table (`shape`: rectangular, oval, round): each size (metres) with Used in Size Lists and Used in Sizing.",
    crate::mep_mech_settings::DuctSizesParams,
    Vec<crate::mep_mech_settings::SizeRow>,
    crate::mep_mech_settings::get_duct_sizes
);

edit_command!(
    DuctSizeTableSet,
    "duct_size_table.set",
    "Set a duct size table",
    "Replaces a duct shape's size table (`shape`, `sizes`: [{size m, in_lists, in_sizing}]). Sizes in lists are offered in the size drop-downs; sizes in sizing are what sizing chooses from for types without their own table. Taking a size off leaves ducts of that size as they are.",
    "Duct Sizes",
    crate::mep_mech_settings::DuctSizeTableParams,
    (),
    crate::mep_mech_settings::set_duct_sizes
);

edit_command!(
    PipeSegmentCreate,
    "pipe_segment.create",
    "Make a pipe segment",
    "Makes a pipe segment (MEP-SET-001): a material (`material_id`, its name) with a `schedule` or class, `roughness` (m) and `sizes` [{nominal, inner, outer (m), in_lists, in_sizing}]. Pipe sizing chooses from the segments' sizes in sizing for types without their own table; the first segment made replaces the built-in Generic one. Each size is exported as an IfcPipeSegmentType with Pset_PipeSegmentTypeCommon. Returns its id.",
    "Pipe Segment",
    crate::mep_mech_settings::PipeSegmentParams,
    String,
    crate::mep_mech_settings::create_segment
);

edit_command!(
    FluidCreate,
    "fluid.create",
    "Make a fluid",
    "Makes a fluid (MEP-SET-001): `name` and a `table` of temperature (°C), density (kg/m³) and dynamic viscosity (Pa·s), interpolated for systems whose type names the fluid at its temperature (outside the table: \"Temperature outside fluid table range\"). Water is built in. Returns its id.",
    "Fluid",
    crate::mep_mech_settings::FluidParams,
    String,
    crate::mep_mech_settings::create_fluid
);

edit_command!(
    SpaceTypeCreate,
    "space_type.create",
    "Make a space type",
    "Makes a space type (MEP-SET-004) for rooms' design loads: `name`, `values` (lighting_load_density and power_load_density W/m², area_per_person m², sensible_heat_per_person and latent_heat_per_person W, plenum_lighting 0–1, occupancy_schedule, lighting_schedule, power_schedule, heating_set_point and cooling_set_point °C, outside_air_per_person L/s and outside_air_per_area L/s·m², opening_time and closing_time \"08:00\"), or `from` another type's (Duplicate). Kept in the RXD document rxd:analysis. Returns its id.",
    "Space Type",
    crate::space_types::CreateParams,
    String,
    crate::space_types::create_space
);

edit_command!(
    BuildingTypeCreate,
    "building_type.create",
    "Make a building type",
    "Makes a building type (MEP-SET-004) with the same values as `space_type.create`. Returns its id.",
    "Building Type",
    crate::space_types::CreateParams,
    String,
    crate::space_types::create_building
);

edit_command!(
    SpaceTypeSet,
    "space_type.set",
    "Change a building or space type",
    "Renames (`name`) or changes the `values` of a building or space type (`which`: building | space; `type_id`: its id or name).",
    "Building/Space Type Settings",
    crate::space_types::SetParams,
    (),
    crate::space_types::set
);

edit_command!(
    SpaceTypeDelete,
    "space_type.delete",
    "Delete a building or space type",
    "Deletes a building or space type (`which`, `type_id`); refused while rooms or buildings use it, naming how many (\"The space type \\\"Office\\\" cannot be deleted: 12 spaces use it\").",
    "Delete Space Type",
    crate::space_types::DeleteParams,
    (),
    crate::space_types::delete
);

read_command!(
    SpaceTypeList,
    "space_type.list",
    "Building or space types",
    "The building or space types (`which`) with their values and how many buildings or rooms use each.",
    crate::space_types::ListParams,
    Vec<crate::space_types::TypeInfo>,
    crate::space_types::list
);

edit_command!(
    SpaceSetType,
    "space.set_type",
    "Give rooms a space type",
    "Gives the rooms `element_ids` the space type `type_id` (id or name); none clears it.",
    "Set Space Type",
    crate::space_types::AssignParams,
    (),
    crate::space_types::assign_space
);

edit_command!(
    BuildingSetType,
    "building.set_type",
    "Give a building a building type",
    "Gives the buildings `element_ids` the building type `type_id` (id or name); none clears it.",
    "Set Building Type",
    crate::space_types::AssignParams,
    (),
    crate::space_types::assign_building
);

edit_command!(
    ElectricalEquipmentPlace,
    "electrical_equipment.place",
    "Place electrical equipment",
    "Places electrical equipment (MEP-ELEC-001): a panelboard, switchboard, motor control centre (IfcElectricDistributionBoard) or transformer type at `point` on `level`: on the face of `host_wall`, or of the nearest wall within 300 mm (its back against it, following it), else standing free (`work_plane`), `offset` above the level (0 by default). Returns its id.",
    "Electrical Equipment",
    crate::mep_lighting::PlaceParams,
    crate::id::ElementId,
    crate::mep_lighting::place_equipment
);

edit_command!(
    DevicePlace,
    "device.place",
    "Place an electrical device",
    "Places a device (MEP-ELEC-001): a receptacle or data, telephone or communications outlet (IfcOutlet), a switch (IfcSwitchingDevice), a fire alarm or security device (IfcAlarm) or a detector (IfcSensor) on the face of `host_wall` or the nearest wall within 300 mm, `offset` above the level (0.3 m by default), following the wall; else free (`work_plane`). Returns its id.",
    "Device",
    crate::mep_lighting::PlaceParams,
    crate::id::ElementId,
    crate::mep_lighting::place_device
);

edit_command!(
    LightingFixturePlace,
    "lighting_fixture.place",
    "Place a lighting fixture",
    "Places a lighting fixture (MEP-ELEC-001): under `host_ceiling` or the ceiling over `point` on `level`, its top at the ceiling's underside and following it; on the face of `host_wall` when given (2.1 m up by default); else at `offset` (2.6 m less its height by default). The room it belongs to is decided by its type's room calculation point (`component_type.set_calc_point`). Returns its id.",
    "Lighting Fixture",
    crate::mep_lighting::PlaceParams,
    crate::id::ElementId,
    crate::mep_lighting::place_fixture
);

edit_command!(
    LightingFixtureTypeSetPhotometry,
    "lighting_fixture_type.set_photometry",
    "Give a lighting fixture type a photometric file",
    "Reads an IES LM-63 (.ies) or EULUMDAT (.ldt) photometric web `file` for the lighting fixture type `type_id` and keeps its name, lumens and peak intensity, with a `colour_temperature` (K, 3000 by default); none clears it. On export each fixture of the type gets an IfcLightSourceGoniometric whose distribution is an IfcDocumentReference to the file. Returns what was read.",
    "Photometry",
    crate::mep_lighting::PhotometryParams,
    Option<crate::mep_lighting::Photometry>,
    crate::mep_lighting::set_photometry
);

edit_command!(
    ComponentTypeSetCalcPoint,
    "component_type.set_calc_point",
    "Set a type's room calculation point",
    "Sets a component type's room calculation point (MEP-ELEC-001), in its own frame (metres; none: the insertion point): the room each copy belongs to for schedules (the Room field), `space.elements` and calculations is the one this point lies in, wherever the body reaches.",
    "Room Calculation Point",
    crate::mep_lighting::CalcPointParams,
    (),
    crate::mep_lighting::set_calc_point
);

read_command!(
    SpaceElements,
    "space.elements",
    "The components in a room",
    "The placed components whose room calculation point lies in the room `room_id`, optionally of `categories` (lighting_fixture, electrical_fixture, …).",
    crate::mep_lighting::ContentsParams,
    Vec<crate::id::ElementId>,
    crate::mep_lighting::contents
);

edit_command!(
    SpacePlace,
    "space.place",
    "Place a space",
    "Places an MEP space (MEP-SPACE-001): an IfcSpace (ObjectType MEPSpace, SPACE) filling the region around `point` on `level_id`, bounded as rooms are, from `base_offset` to `limit_offset` above `upper_limit` (its own level and 4000 mm by default), numbered in sequence unless `number` is given. Spaces are separate from rooms; a space under a room is occupiable. Returns its id.",
    "Space",
    crate::mep_spaces::PlaceParams,
    crate::id::ElementId,
    crate::mep_spaces::place
);

edit_command!(
    SpacePlaceAuto,
    "space.place_auto",
    "Place spaces automatically",
    "Places a space in every enclosed region of the level `level_id` larger than 0.025 m² that has none, numbered in sequence top to bottom, then left to right. Returns the spaces.",
    "Place Spaces Automatically",
    crate::mep_spaces::PlaceAutoParams,
    Vec<crate::id::ElementId>,
    crate::mep_spaces::place_auto
);

edit_command!(
    SpaceUnplace,
    "space.unplace",
    "Unplace spaces",
    "Unplaces the spaces `ids`: they keep their number, name and properties and stay in schedules as Not Placed until deleted; `space.set` with a `point` places one again.",
    "Unplace Spaces",
    crate::mep_spaces::IdsParams,
    (),
    crate::mep_spaces::unplace
);

edit_command!(
    SpaceSet,
    "space.set",
    "Change spaces",
    "Sets spaces' (`ids`) number, name, `point` (placing an unplaced one), `limit_offset`, `base_offset`, `condition` (heated_and_cooled, heated, cooled, unconditioned, vented, naturally_vented_only), `return_air_plenum`, ceiling, wall and floor reflectances (0–1), `workplane_height`, specified supply, return and exhaust airflow (m³/s; negative clears), and `overrides` of the space type's values (lighting_load_density, power_load_density, area_per_person, sensible_heat_per_person, latent_heat_per_person, occupancy_schedule, lighting_schedule, power_schedule, heating_set_point, cooling_set_point, outside_air_per_person, outside_air_per_area; null returns a value to the type's). The type itself is set with `space.set_type`.",
    "Space Properties",
    crate::mep_spaces::SetParams,
    (),
    crate::mep_spaces::set
);

edit_command!(
    SpaceNameFromRooms,
    "space.name_from_rooms",
    "Name spaces from rooms",
    "Space Naming (MEP-SPACE-001): each space (`ids`, or every space) takes the number and name of the room placed at its location. Returns the spaces named.",
    "Space Naming",
    crate::mep_spaces::NameParams,
    Vec<crate::id::ElementId>,
    crate::mep_spaces::name_from_rooms
);

read_command!(
    SpaceInfo,
    "space.info",
    "Spaces' properties",
    "Each space's (`ids`, or every space) number, name, whether placed and enclosed, area, volume and height, whether occupiable, its space type, properties, the effective values (the type's with the overrides, which are listed), actual lighting and power loads and densities from the fixtures and devices in it, actual supply, return and exhaust airflow from its terminals, and the number of people.",
    crate::mep_spaces::InfoParams,
    Vec<crate::mep_spaces::SpaceInfo>,
    crate::mep_spaces::infos
);

edit_command!(
    PathOfTravelCreate,
    "path_of_travel.create",
    "Make a path of travel",
    "The shortest walking route (ANA-ROUTE-001) in the plan view `view_id` from `start` to `end` through `waypoints` in order, keeping the clearance from obstacles in the analysis zone: walls (doors passable, windows not) and components reaching into the zone, except ignored categories (route_settings). One level; stairs and ramps are not followed. Kept as a view annotation with its route, length and time (length ÷ walking speed). Refused when no route keeps clear. Returns the path.",
    "Path of Travel",
    crate::path_of_travel::CreateParams,
    crate::id::ElementId,
    crate::path_of_travel::create
);

edit_command!(
    PathOfTravelUpdate,
    "path_of_travel.update",
    "Update paths of travel",
    "Works the routes of paths of travel out again after the model changed: `ids`, or every path of `view_id` (or of the model) when empty. Returns the paths whose route changed.",
    "Update Paths of Travel",
    crate::path_of_travel::UpdateParams,
    Vec<crate::id::ElementId>,
    crate::path_of_travel::update
);

edit_command!(
    PathOfTravelWaypoint,
    "path_of_travel.add_waypoint",
    "Add a waypoint to a path of travel",
    "Adds the waypoint `point` to the path `id` at `index` among its waypoints (the end when omitted), or with `remove` removes the waypoint at `index`; the route is worked out again. Returns its length.",
    "Path Waypoint",
    crate::path_of_travel::WaypointParams,
    f64,
    crate::path_of_travel::waypoint
);

read_command!(
    PathOfTravelList,
    "path_of_travel.list",
    "Paths of travel",
    "Every path of travel (of `view_id` when given) with its view, length (m), time (s), route and waypoints.",
    crate::path_of_travel::ListParams,
    Vec<crate::path_of_travel::PathInfo>,
    crate::path_of_travel::list
);

edit_command!(
    RouteSettingsSet,
    "route_settings.set",
    "Set route analysis settings",
    "Route analysis settings (ANA-ROUTE-002): the analysis zone's `zone_bottom` and `zone_top` above the level (m), `walking_speed` (m/s), `clearance` (m) and `ignored_categories` (category keys whose elements are not obstacles). Run path_of_travel.update afterwards. Returns the settings.",
    "Route Settings",
    crate::path_of_travel::SettingsParams,
    crate::path_of_travel::Settings,
    crate::path_of_travel::set_settings
);

read_command!(
    RouteSettingsGet,
    "route_settings.get",
    "Route analysis settings",
    "The analysis zone offsets, walking speed, clearance and ignored categories.",
    crate::path_of_travel::Empty,
    crate::path_of_travel::Settings,
    crate::path_of_travel::get_settings
);

edit_command!(
    CarbonAddLibrary,
    "embodied_carbon.add_library",
    "Add an emission factor library",
    "Adds emission factors to the project (ANA-CARB-001): `name`, `file` and `factors` [{id, name, class, unit (kg, m3, m2, piece), stages {a1_a3, a4, a5, c1_c4, d, biogenic} in kg CO2e per unit, density, service_life, source, valid_until}]. A library of the same name is replaced, keeping its id and the matches. Use embodied_carbon.import_factors to read a CSV. Returns its id.",
    "Emission Factors",
    crate::embodied_carbon::AddLibraryParams,
    String,
    crate::embodied_carbon::add_library
);

edit_command!(
    CarbonMatch,
    "embodied_carbon.match",
    "Match a material to a factor",
    "Matches the project material `material` (by name) to the factor `factor_ref` (library_id/factor_id), or unmatches it with null (ANA-CARB-001).",
    "Match Material",
    crate::embodied_carbon::MatchParams,
    (),
    crate::embodied_carbon::match_material
);

edit_command!(
    CarbonAutoMatch,
    "embodied_carbon.auto_match",
    "Propose factors for materials",
    "Proposes a factor for each unmatched material, the one whose name or class shares the most words with the material's name (ANA-CARB-001); with `apply` the proposals become matches. Materials no factor shares a word with stay unmatched. Returns material → factor_ref.",
    "Auto-match Materials",
    crate::embodied_carbon::AutoMatchParams,
    std::collections::BTreeMap<String, String>,
    crate::embodied_carbon::auto_match
);

read_command!(
    CarbonMaterials,
    "embodied_carbon.materials",
    "Materials for embodied carbon",
    "The project's materials in walls (layers less openings), floors, ceilings and roofs (ANA-CARB-001): each with its volume (m3), the elements it is in, matched or unmatched, its factor, and a proposal when unmatched.",
    crate::embodied_carbon::Empty,
    Vec<crate::embodied_carbon::MaterialRow>,
    crate::embodied_carbon::materials
);

read_command!(
    EnergyModelGet,
    "energy_model.get",
    "The energy model",
    "The simplified energy model (ANA-EM-003, ANA-ENV-003), derived from the MEP spaces now: each placed, enclosed space as an analytical space (area, volume, height, system-zone) with its surfaces: exterior and interior walls (interior when another space lies beyond, with that space as `adjacent`), windows and doors in exterior walls, slab on grade or interior floor, roof or interior ceiling, air boundaries at separation lines; each with area, U-value (the type's ThermalTransmittance, else a class default), outward normal, element and outline.",
    crate::energy_model::Empty,
    crate::energy_model::EnergyModel,
    crate::energy_model::get
);

read_command!(
    EnergyModelSummary,
    "energy_model.summary",
    "Energy model summary",
    "The energy model's spaces, skipped spaces, area by surface class, surfaces still on default U-values, UA to the outside (W/K) and window-to-wall ratio.",
    crate::energy_model::Empty,
    crate::energy_model::Summary,
    crate::energy_model::summary
);

edit_command!(
    EnergyModelSetThermal,
    "energy_model.set_thermal",
    "Set a type's thermal values",
    "Sets an element type's U-value (`u_value`, W/m²K, written as ThermalTransmittance in its common property set: Pset_WallCommon, Pset_WindowCommon, Pset_DoorCommon, Pset_SlabCommon or Pset_RoofCommon, so it goes to IFC) and, for windows, the solar heat gain coefficient (`shgc`, Pset_DoorWindowGlazingType.SolarHeatGainTransmittance); null clears a value (the class default is used).",
    "Thermal Values",
    crate::energy_model::ThermalParams,
    (),
    crate::energy_model::set_thermal
);

edit_command!(
    SolarRecord,
    "solar.record",
    "Record a solar study",
    "Keeps a finished solar study (ANA-SOLAR-001, ANA-ENV-002): the analysis.solar job's `results` (per face: grid points and kWh/m² values, area, mean, total, PV energy, cost and payback), its type, period, grid, PV settings and the hash of its inputs. solar.collect calls it; the last ten studies are kept. Returns the study.",
    "Solar Study",
    crate::solar::RecordParams,
    crate::solar::Study,
    crate::solar::record
);

read_command!(
    SolarResults,
    "solar.results",
    "Solar study results",
    "A solar study's (`study_id`, else the latest) per-face results: element, area (m²), mean insolation (kWh/m²), total (kWh), and for PV studies kWh a year, cost and simple payback; with the period, sun hours and totals.",
    crate::solar::ResultsParams,
    crate::solar::StudyReport,
    crate::solar::results
);

edit_command!(
    CarbonRecord,
    "embodied_carbon.record",
    "Record an embodied carbon run",
    "Keeps a finished embodied carbon run (ANA-CARB-001, ANA-ENV-001): the `results` of an analysis.carbon job, its distillation's `key_map` (element key → GlobalId) and `inputs`, the `factors` used and the `options`. embodied_carbon.collect calls it; the last ten runs are kept. Returns the run.",
    "Embodied Carbon",
    crate::embodied_carbon::RecordParams,
    crate::embodied_carbon::Run,
    crate::embodied_carbon::record
);

read_command!(
    CarbonResults,
    "embodied_carbon.results",
    "Embodied carbon results",
    "A run's (`run_id`, else the latest) totals by stage, results by category, material and element, excluded materials and the unmatched share (ANA-CARB-001), with the elements whose inputs changed since (stale) and whether the matched factors changed.",
    crate::embodied_carbon::ResultsParams,
    crate::embodied_carbon::Results,
    crate::embodied_carbon::results
);

edit_command!(
    CircuitRenumber,
    "circuit.renumber",
    "Renumber a panel's circuits",
    "Renumber Circuits (MEP-ELEC-021): the circuits of the panel `panel_id` (feeders too, not one on feed-through lugs) move to the lowest slots from `start` (default 1), keeping their order and poles; locked circuits stay in their slots and the others flow round them. Refused, naming the circuit, when one would pass the last slot. Names follow the panel's naming scheme; hand-typed load names are kept. Reports each circuit before and after with its phases, and the phase loads and imbalance before and after, warning when the imbalance grows by more than 10 points. Nothing to move adds no undo step.",
    "Renumber Circuits",
    crate::mep_renumber::RenumberParams,
    crate::mep_renumber::Renumbering,
    crate::mep_renumber::renumber
);

read_command!(
    CircuitRenumberPreview,
    "circuit.renumber_preview",
    "Preview renumbering a panel",
    "What `circuit.renumber` would do to the panel `panel_id` from `start`, without changing it: each circuit's old and new name and slots, load name, phases and whether it moves, and the phase loads and imbalance before and after.",
    crate::mep_renumber::RenumberParams,
    crate::mep_renumber::Renumbering,
    crate::mep_renumber::preview
);

edit_command!(
    CircuitSetLocked,
    "circuit.set_locked",
    "Lock circuits in their slots",
    "Locks (`locked` true) or unlocks circuits (`circuit_ids`) in their slots: renumbering leaves locked circuits where they are (MEP-ELEC-015).",
    "Lock Circuits",
    crate::mep_renumber::LockParams,
    (),
    crate::mep_renumber::set_locked
);

edit_command!(
    MepColorLegendPlace,
    "mep_color_legend.place",
    "Place a duct or pipe legend",
    "Places a duct or pipe colour fill legend (MEP-DUCT-010) in the plan view `view_id` at `point` (its top-left corner): the view then colours that kind's segments, and fittings unless the scheme leaves them out, by the scheme `scheme_id` (a colorscheme of cat.ducts or cat.pipes, made with colorscheme.create: Flow, Velocity, Friction, Pressure Drop, Size, System Type, System Name; for pipes also Fluid Temperature and Slope). One duct and one pipe legend per view; deleting the legend removes the fill from that view only. Elements with no value take the scheme's No value colour. Returns the legend.",
    "Color Fill Legend",
    crate::mep_color_fill::PlaceParams,
    crate::id::ElementId,
    crate::mep_color_fill::place
);

read_command!(
    LightingCalcAverage,
    "lighting.calc_average",
    "Average estimated illumination",
    "The lumen (zonal cavity) method for each space (`space_ids`, or every space; ANA-LIGHT-001): E = N × Φ × CU × LLF ÷ A over the lighting fixtures whose room calculation point is in the space, Φ from the type's photometric file or its own lumens, RCR = 2.5 × h_rc × perimeter ÷ area from the workplane to the fixtures' average height, CU interpolated in the type's CU table at the RCR and the space's reflectances, LLF from the space, else the type, else 0.8. Each result has the illuminance (lx), RCR, fixtures counted, excluded fixtures with their reason (no CU data, no lumen data), the target (Pset_SpaceLightingRequirements.Illuminance) and whether it is met; unplaced or unenclosed spaces are skipped with a reason. Worked out from the model as it is now.",
    crate::mep_illumination::CalcParams,
    Vec<crate::mep_illumination::Illumination>,
    crate::mep_illumination::calc_average
);

edit_command!(
    LightingFixtureTypeSetLighting,
    "lighting_fixture_type.set_lighting",
    "Set a fixture type's lumen method data",
    "Sets a lighting fixture type's (`type_id`) data for average estimated illumination (ANA-LIGHT-001): `lumens` per fixture (used when its photometric file states none), `light_loss_factor` (0–1; 0.8 when none) and `cu_table` {rcr: [rising RCR rows], columns: [[ceiling, wall, floor reflectance 0–1]...], values: [[CU per column] per row]}; null clears a value. Reviera ships no CU tables: take them from the manufacturer's data. Returns the type's data.",
    "Lighting Data",
    crate::mep_illumination::SetLightingParams,
    crate::mep_illumination::LightingData,
    crate::mep_illumination::set_lighting
);

edit_command!(
    SpaceSeparatorCreate,
    "space_separator.create",
    "Add space separation lines",
    "Lines on the level `level_id` that split space for MEP spaces (MEP-SPACE-001) like walls, without bounding rooms (room separation lines bound both); `through` gives a point for an arc, per line. Spaces on the level update at once. Returns the lines.",
    "Space Separator",
    crate::room_separators::CreateParams,
    Vec<crate::id::ElementId>,
    crate::room_separators::create_space
);

edit_command!(
    SystemZoneCreate,
    "system_zone.create",
    "Make a system-zone",
    "Makes a system-zone (MEP-ZONE-001): an IfcSpatialZone (THERMAL) on `level_id` marking the area an air system, water loop or zone equipment serves, either sketched (`boundary`, its corners in plan, metres) or covering the whole level (`whole_level`: the box round everything on it, kept up to date). `height` defaults to the next level (3 m on the top level). Every MEP space on the level inside or touching it is assigned to it. Returns its id.",
    "System-Zone",
    crate::system_zones::CreateParams,
    crate::id::ElementId,
    crate::system_zones::create
);

edit_command!(
    SystemZoneAssign,
    "system_zone.assign",
    "Assign a system-zone",
    "Assigns the system-zone `zone_id` to what serves it, `equipment_or_system_id`: a piece of mechanical equipment or an MEP system (an air system or a water loop); null clears it. In IFC the system serves the zone by IfcRelServicesBuildings, equipment is grouped into its IfcZone.",
    "Assign System-Zone",
    crate::system_zones::AssignParams,
    (),
    crate::system_zones::assign
);

edit_command!(
    SystemZoneSet,
    "system_zone.set",
    "Change system-zones",
    "Changes system-zones (`zone_ids`): `name`, a new `boundary` or `whole_level`, `height` (null follows the levels), the heating and cooling set points (°C), humidity limits (relative, %) and outdoor air per person (m³/s) and per area (m³/s per m²); null clears a value. The heating set point must be below the cooling one. Set points are written to Pset_SpaceThermalRequirements on each assigned space.",
    "Edit System-Zone",
    crate::system_zones::SetParams,
    (),
    crate::system_zones::set
);

read_command!(
    SystemZoneInfo,
    "system_zone.info",
    "System-zones",
    "Each system-zone's (`zone_ids`, or every one) name, level, whether it covers the whole level, outline, area and height, its assigned spaces (MEP spaces on its level inside or touching it) with their numbers, what serves it (equipment or system) and its set points.",
    crate::system_zones::InfoParams,
    Vec<crate::system_zones::ZoneInfo>,
    crate::system_zones::info_cmd
);

edit_command!(
    SystemZoneApplyConvert,
    "system_zone.apply_convert",
    "Replace legacy zones",
    "Replaces each legacy IfcZone (`zones[].legacy_id`) with a system-zone of its name on `level_id` with the given `boundary`, `height` and `props` (set points), as `system_zone.convert_legacy` works them out. One undo step. Returns the system-zones.",
    "Convert Zones",
    crate::system_zones::ApplyConvertParams,
    Vec<crate::id::ElementId>,
    crate::system_zones::apply_convert
);

edit_command!(
    GlobalCreate,
    "global.create",
    "Make a global parameter",
    "Makes a global parameter (DM-GPAR-001): `name` (unique, case-insensitive, no [ ] { } | ; < > @), `data_type` (length, area, volume, angle, number, integer, yes_no, text, material), a `value` (SI: metres, radians) or a `formula` over other globals (`CorridorWidth * 2 + 300 mm`; units must agree; cycles refused), `group`, `reporting` (its value read from the one dimension it labels) and `tooltip`. Returns its id.",
    "Global Parameters",
    crate::globals::CreateParams,
    String,
    crate::globals::create
);

edit_command!(
    GlobalSet,
    "global.set",
    "Change a global parameter",
    "Sets a global's (`global_id`: id or name) `value` or `formula` (empty removes it), `name`, `group` or `tooltip`. Formulas are worked out again and every labelled dimension and associated parameter follows, in one transaction: labelled dimensions keep their first reference and move the second.",
    "Global Parameters",
    crate::globals::SetParams,
    (),
    crate::globals::set
);

edit_command!(
    GlobalDelete,
    "global.delete",
    "Delete a global parameter",
    "Deletes a global: associated parameters keep their values, labelled dimensions are unlabelled (their lock removed). Refused while another global's formula uses it. Returns how many uses it had.",
    "Global Parameters",
    crate::globals::IdParams,
    crate::globals::DeleteReport,
    crate::globals::delete
);

edit_command!(
    GlobalAssociate,
    "global.associate",
    "Drive element parameters by a global",
    "Associates the instance parameter `param_ref` (as `param.set` takes it, or `insert.sill` for doors' and windows' sill height) of `element_ids` with the global `global_id` of the same data type: it takes the global's value now and whenever it changes. Type and read-only parameters are refused.",
    "Associate Global Parameter",
    crate::globals::AssociateParams,
    (),
    crate::globals::associate
);

edit_command!(
    GlobalDissociate,
    "global.dissociate",
    "Stop a global driving element parameters",
    "The parameter `param_ref` of `element_ids` keeps its value and is no longer driven by a global.",
    "Dissociate Global Parameter",
    crate::globals::DissociateParams,
    (),
    crate::globals::dissociate
);

edit_command!(
    GlobalLabelDimension,
    "global.label_dimension",
    "Label a dimension with a global",
    "Labels a one-segment dimension with a length global: a driving global locks it at its value (its first reference stays, the second moves, now and whenever the global changes); a reporting global reads its value from it (one dimension only).",
    "Label Dimension",
    crate::globals::LabelParams,
    (),
    crate::globals::label_dimension
);

read_command!(
    GlobalList,
    "global.list",
    "The global parameters",
    "Every global parameter with its value worked out now (formulas evaluated, reporting ones measured), formula, group, reporting flag, tooltip, associations and number of uses.",
    crate::builtin::NoParams,
    Vec<crate::globals::GlobalInfo>,
    crate::globals::list
);

read_command!(
    GlobalSelectDriven,
    "global.select_driven",
    "What a global drives",
    "The elements and dimensions the global `global_id` drives.",
    crate::globals::IdParams,
    Vec<crate::id::ElementId>,
    crate::globals::select_driven
);

read_command!(
    FormulaValidate,
    "formula.validate",
    "Check a formula",
    "Checks a `formula` over the global parameters (unknown names, units that do not agree, cycles through `global_id` when given) and returns its value now (SI).",
    crate::globals::ValidateParams,
    f64,
    crate::globals::validate
);

edit_command!(
    ClassificationCreate,
    "classification.create",
    "Add a classification system",
    "Adds a classification system to the project (DM-CLS-001): `name` (Uniclass 2015, OmniClass, …), `source`, `edition`, `location` and its table `items` [{code, parent?, title}] (parents must be in the table, codes unique). A system of the same name is replaced, keeping assignments whose codes remain. Files are read by `classification.load`. Returns its id.",
    "Classification System",
    crate::classification::CreateParams,
    String,
    crate::classification::create
);

edit_command!(
    ClassificationAssign,
    "classification.assign",
    "Classify elements",
    "Gives types or occurrences (`element_ids`) the `code` of the system `system_id` (id or name), replacing their code in that system; one code per system per element. Exported as IfcClassificationReference associated by IfcRelAssociatesClassification.",
    "Classify",
    crate::classification::AssignParams,
    (),
    crate::classification::assign
);

edit_command!(
    ClassificationUnassign,
    "classification.unassign",
    "Remove elements' classification",
    "Removes the elements' code in the system `system_id`.",
    "Unclassify",
    crate::classification::UnassignParams,
    (),
    crate::classification::unassign
);

edit_command!(
    ClassificationDelete,
    "classification.delete",
    "Remove a classification system",
    "Removes a classification system and every element's code in it; returns how many elements lost a code.",
    "Remove Classification System",
    crate::classification::DeleteParams,
    usize,
    crate::classification::delete
);

read_command!(
    ClassificationList,
    "classification.list",
    "The classification systems",
    "The project's classification systems with their source, edition, location, number of codes and number of elements classified.",
    crate::builtin::NoParams,
    Vec<crate::classification::SystemInfo>,
    crate::classification::list
);

read_command!(
    ClassificationSearch,
    "classification.search",
    "Search a classification table",
    "A system's codes whose code or title contains `text` (every code when empty), up to `limit` (500).",
    crate::classification::SearchParams,
    Vec<crate::classification::Item>,
    crate::classification::search
);

read_command!(
    ClassificationOf,
    "classification.of",
    "An element's classification",
    "An element's codes, one per system, with the system names and code titles.",
    crate::classification::OfParams,
    Vec<crate::classification::Classified>,
    crate::classification::of
);

edit_command!(
    KeynotesSet,
    "keynotes.set",
    "Set the keynote table",
    "Sets the project's keynote table: `path` (the file, relative to the project when beside it) and `entries` [{key, text, parent?}]. Files are read by `keynotes.load`.",
    "Keynotes",
    crate::classification::KeynoteTable,
    (),
    crate::classification::set_keynotes
);

read_command!(
    KeynotesList,
    "keynotes.list",
    "The keynote table",
    "The project's keynote file and entries, those whose key or text contains `text` (every entry when empty).",
    crate::classification::KeynoteListParams,
    crate::classification::KeynoteTable,
    crate::classification::list_keynotes
);

edit_command!(
    StandardsApply,
    "standards.apply",
    "Apply transferred project standards",
    "The target side of Transfer Project Standards (DM-XFER-001), normally called by `standards.transfer`: makes `creates` (elements with their GlobalIds, class, name and parameters), replaces the parameters of `updates` (elements of this project, GlobalIds kept), and writes `settings` (holder project, project_settings, parameters or a standards element class; key; value), as one transaction. Returns the number of changes.",
    "Transfer Project Standards",
    crate::standards_transfer::ApplyParams,
    usize,
    crate::standards_transfer::apply
);
