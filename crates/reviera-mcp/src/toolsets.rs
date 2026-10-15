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
    let set = match domain {
        "element" | "edit" | "model" | "registry" | "param" | "global" | "pset" | "qto"
        | "warnings" | "failure" | "regen" | "snaps" | "selection" | "graph" => "core",
        "wall" | "floor" | "roof" | "door" | "window" | "stair" | "modify" | "str" | "mep"
        | "level" | "grid" | "room" | "ceiling" | "insert" | "sketch" | "dormer" | "railing"
        | "deck" | "site" | "cme" | "host" | "numbering" | "decal" | "spatial" | "building"
        | "assembly" | "part" | "geometry" | "opening" | "column" | "compound" | "curtain"
        | "ramp" | "measure" | "skylight" | "mesh_object" => "modeling",
        "view" | "sheet" | "schedule" | "annotate" | "render" | "walkthrough" | "xr" | "ui"
        | "tag" | "text" | "anno" | "dim" | "viewport" | "section" | "inspect" => "documentation",
        "family" | "family_doc" | "type" | "instance" | "library" | "component" | "formula"
        | "subcategory" => "families",
        "material" | "object_style" | "line_pattern" | "line_style" | "line_weight"
        | "fill_pattern" | "image" | "objectstyle" | "lineweight" => "materials",
        "project" | "location" | "coordinates" | "position" | "phase" | "standards" | "units"
        | "code" | "purge" | "classification" | "keynotes" | "paramlib" | "external_props" => {
            "project_setup"
        }
        "file" | "import" | "export" | "live" | "recovery" | "validate" | "rxd" | "journal"
        | "history" | "snapshot" | "cobie" => "io",
        "bimserver" | "link" | "workset" | "bcf" | "cde" | "api" | "http" | "overlap" | "clash"
        | "clearance" => "collaboration",
        "analysis" => "analysis",
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
