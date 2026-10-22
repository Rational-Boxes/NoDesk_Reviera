// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Toolsets: command groups that keep the tool list manageable (DM-MCP-003).

use reviera_core::command::RESERVED_DOMAINS;

/// The built-in toolsets with a one-line description.
pub const TOOLSETS: &[(&str, &str)] = &[
    (
        "core",
        "Query and inspect elements, parameters, selection, undo and history",
    ),
    (
        "modeling",
        "Walls, floors, roofs, components, structure, MEP and modify tools",
    ),
    (
        "documentation",
        "Views, sheets, schedules, annotation and rendering",
    ),
    ("families", "Families, types and the family editor"),
    ("materials", "Materials and graphic standards"),
    (
        "project_setup",
        "Project information, units, phases, standards and location",
    ),
    ("io", "Open, save, import, export, recovery and validation"),
    (
        "collaboration",
        "BIMserver, links, worksets, issues and REST services",
    ),
    ("analysis", "Analysis and simulation"),
    ("scripting", "Lua scripts, add-ins and graphs"),
    ("admin", "Application, settings and MCP administration"),
    ("other", "Commands outside the standard domains"),
];

/// The toolset a command belongs to, from its domain. Add-in commands (`<addin-id>.<verb>`) get a
/// toolset per add-in: `addin:<addin-id>`.
pub fn toolset_of(command: &str) -> String {
    let parts: Vec<&str> = command.split('.').collect();
    let domain = parts[0];
    if !RESERVED_DOMAINS.contains(&domain) && parts.len() >= 3 {
        return format!("addin:{}", parts[..parts.len() - 1].join("."));
    }
    let set =
        match domain {
            "element" | "edit" | "model" | "registry" | "param" | "global" | "pset" | "qto"
            | "warnings" | "failure" | "regen" | "snaps" | "selection" | "graph" => "core",
            "wall"
            | "floor"
            | "roof"
            | "door"
            | "window"
            | "stair"
            | "modify"
            | "str"
            | "structural"
            | "structural_settings"
            | "beam"
            | "structural_connection"
            | "connection_type"
            | "plate"
            | "bolt_pattern"
            | "anchor_pattern"
            | "hole_pattern"
            | "shear_stud_pattern"
            | "fastener_pattern"
            | "weld"
            | "face"
            | "steel_cut"
            | "steel_modifier"
            | "steel_numbering"
            | "steel_nc"
            | "connection_check"
            | "custom_connection"
            | "rebar"
            | "rebar_bar_type"
            | "rebar_hook_type"
            | "rebar_shape"
            | "rebar_cover_type"
            | "rebar_cover"
            | "reinforcement_settings"
            | "area_reinforcement"
            | "path_reinforcement"
            | "fabric_area"
            | "fabric_sheet"
            | "fabric_wire_type"
            | "fabric_sheet_type"
            | "reinforcement_system"
            | "reinforcement_numbering"
            | "rebar_tag"
            | "multi_rebar_annotation"
            | "bending_detail"
            | "rebar_annotation"
            | "rebar_schedule"
            | "rebar_coupler"
            | "precast"
            | "precast_config"
            | "connector"
            | "circuit"
            | "panel"
            | "panel_schedule"
            | "load_classification"
            | "demand_factor"
            | "electrical_settings"
            | "distribution_system"
            | "voltage"
            | "duct"
            | "pipe"
            | "cable_tray"
            | "cable_tray_fitting"
            | "conduit"
            | "conduit_fitting"
            | "conduit_standard"
            | "conductor_size"
            | "cable_type"
            | "conductor_ampacity"
            | "wiring"
            | "fixture_unit_method"
            | "gutter"
            | "mep_fitting"
            | "mep_segment_type"
            | "routing_preferences"
            | "mep_run"
            | "mep_type"
            | "system"
            | "system_type"
            | "mep_check"
            | "mep_network"
            | "mep_settings"
            | "analytical_segment"
            | "pressure_loss_report"
            | "system_inspector"
            | "sizing_calculator"
            | "mech_control_device"
            | "pipe_fitting"
            | "pipe_accessory"
            | "duct_accessory"
            | "mep_segment"
            | "mep_covering_type"
            | "mep_covering"
            | "mep_insulation"
            | "duct_lining"
            | "flex_duct"
            | "plumbing_fixture"
            | "sprinkler"
            | "sprinkler_type"
            | "hydronic"
            | "hydraulic_separation"
            | "analytical_pipe_connection"
            | "mounting_part"
            | "cam_writer"
            | "beam_system"
            | "truss"
            | "brace"
            | "framing"
            | "footing"
            | "wall_foundation"
            | "foundation_slab"
            | "foundation"
            | "pile"
            | "pile_cap"
            | "mep"
            | "level"
            | "grid"
            | "room"
            | "ceiling"
            | "insert"
            | "sketch"
            | "dormer"
            | "railing"
            | "deck"
            | "site"
            | "cme"
            | "host"
            | "numbering"
            | "decal"
            | "spatial"
            | "building"
            | "assembly"
            | "part"
            | "geometry"
            | "opening"
            | "column"
            | "compound"
            | "curtain"
            | "ramp"
            | "measure"
            | "skylight"
            | "mesh_object"
            | "array"
            | "refplane"
            | "workplane"
            | "modelline"
            | "datum"
            | "scopebox"
            | "constraint"
            | "slabedge"
            | "hostsweep"
            | "shape"
            | "stackedwall"
            | "wallsweep"
            | "wallreveal"
            | "area"
            | "areascheme"
            | "areaplan"
            | "areaboundary"
            | "group"
            | "selectionset"
            | "modeltext"
            | "inplace"
            | "roomseparator"
            | "mass"
            | "curtainsystem"
            | "toposolid"
            | "propertyline"
            | "sitecomponent"
            | "parts" => "modeling",
            "view" | "sheet" | "schedule" | "annotate" | "render" | "walkthrough" | "xr" | "ui"
            | "tag" | "text" | "anno" | "dim" | "viewport" | "section" | "inspect"
            | "colorscheme" => "documentation",
            "family" | "family_doc" | "type" | "instance" | "library" | "component" | "formula"
            | "subcategory" => "families",
            "material" | "object_style" | "line_pattern" | "line_style" | "line_weight"
            | "fill_pattern" | "image" | "objectstyle" | "lineweight" => "materials",
            "project" | "location" | "coordinates" | "position" | "phase" | "standards"
            | "units" | "code" | "purge" | "classification" | "keynotes" | "paramlib"
            | "external_props" => "project_setup",
            "file" | "import" | "export" | "live" | "recovery" | "validate" | "rxd" | "journal"
            | "history" | "snapshot" | "cobie" => "io",
            "bimserver" | "link" | "workset" | "bcf" | "cde" | "api" | "http" | "overlap"
            | "clash" | "clearance" => "collaboration",
            "analysis"
            | "analysis_provider"
            | "analysis_job"
            | "analysis_result"
            | "analysis_style"
            | "compute"
            | "compute_host"
            | "subscription"
            | "structural_analysis"
            | "results"
            | "analytical_member"
            | "analytical_node"
            | "analytical_panel"
            | "analytical_opening"
            | "analytical_association"
            | "analytical_automation"
            | "analytical_model"
            | "structural_code"
            | "load_nature"
            | "load_case"
            | "load_combination"
            | "load"
            | "boundary_condition" => "analysis",
            "script" | "plugin" | "addin" | "lua" => "scripting",
            "app" | "mcp" | "settings" | "txn" => "admin",
            _ => "other",
        };
    set.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_map_to_toolsets() {
        assert_eq!(toolset_of("edit.undo"), "core");
        assert_eq!(toolset_of("file.open"), "io");
        assert_eq!(toolset_of("compute.submit"), "analysis");
        assert_eq!(toolset_of("compute_host.list"), "analysis");
        assert_eq!(toolset_of("family_doc.form.extrusion"), "families");
        assert_eq!(
            toolset_of("org.example.roomtools.renumber"),
            "addin:org.example.roomtools"
        );
        assert_eq!(toolset_of("test.thing"), "other");
    }

    /// Every built-in command (the core's, the store's and the application's) is in a named
    /// toolset, so agents find the high-level tools (tags, stairs, sheets, …) where they look.
    #[test]
    fn every_built_in_command_has_a_toolset() {
        let app = reviera_app::App::new(reviera_app::AppConfig::default());
        let loose: Vec<String> = app
            .registry()
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| toolset_of(n) == "other")
            .collect();
        assert!(loose.is_empty(), "commands without a toolset: {loose:?}");
    }
}
