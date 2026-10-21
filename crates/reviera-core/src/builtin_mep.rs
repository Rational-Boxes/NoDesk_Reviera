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
    "Loads Reviera's starter MEP component types (`names`, all when empty): Air Handling Unit, Supply Diffuser 600, Return Grille 600, Exhaust Fan, Water Heater, Circulation Pump, Lavatory, Water Closet, Distribution Board, Receptacle and Ceiling Luminaire: simple boxes with connectors, sizes and flows Reviera's own provisional values, for laying out systems before manufacturers' content. A type of the same name already in the model is reused. Returns the types by name.",
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
    "Gives duct segments a new size (`width`, `height`, `diameter`); the legs of the fittings joined to them follow.",
    "Duct Size",
    crate::mep_runs::SetSizeParams,
    (),
    crate::mep_runs::set_size
);

edit_command!(
    PipeSetSize,
    "pipe.set_size",
    "Resize pipes",
    "Gives pipe segments a new `diameter`; the legs of the fittings joined to them follow.",
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
    "Makes a segment type (`domain` duct, pipe, cable_tray or conduit; `shape` round, rectangular, oval; pipes and conduit round, tray rectangular) named `name` with `routing` preferences (MEP-DUCT-002: rows {part, min, max} per role elbow, junction (tee, tap), cross, transition, cap, union (coupling), and `preferred_junction` tee or tap; one row of each by default, a coupling union for conduit), `material` and `roughness`. Tray and conduit: `with_fittings` (default true; without, bends are curved segments and fittings cannot be placed), `bend_radius_multiplier` (bend radius over width or diameter; 1 for tray, 6 for conduit by default), tray `tray_kind` (ladder, channel, solid_bottom, trough) and conduit `standard` (a conduit standard's id: iec_61386, emt, or one made with conduit_standard.create). Returns the type.",
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
    "Puts a circuit on a panel in the first free slots that fit its poles (MEP-ELEC-003): refused when the panel's distribution system does not give the circuit's voltage (line to ground for one pole, line to line for two or three; three poles need three phases) or it is full. The circuit is numbered by the panel's naming scheme. Returns its number.",
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
