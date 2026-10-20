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
    "Make a duct or pipe type",
    "Makes a duct or pipe type (`domain` duct or pipe; `shape` round, rectangular, oval; pipes round) named `name` with `routing` preferences (MEP-DUCT-002: rows {part, min, max} per role elbow, junction (tee, tap), cross, transition, cap, and `preferred_junction` tee or tap; one row of each by default), `material` and `roughness`. Returns the type.",
    "Duct or Pipe Type",
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
