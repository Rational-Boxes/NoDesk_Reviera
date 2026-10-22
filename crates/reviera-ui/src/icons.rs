// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reviera's own tool icons (UI-ARCH-005: SVG through `wxBitmapBundle::FromSVG`): simple line
//! drawings on a 32 × 32 grid, in one ink colour with a blue accent and a pale fill, in a light
//! and a dark variant. They are drawn for Reviera; no third-party icon set is used (NFR-CR-001).

/// Ink, accent, fill and paper colours for light or dark chrome.
fn palette(dark: bool) -> [&'static str; 4] {
    if dark {
        ["#d5dee8", "#5aa7f0", "#33475c", "#1f2a36"]
    } else {
        ["#2f3d4c", "#2e7fd6", "#d7e5f2", "#ffffff"]
    }
}

/// The body of an icon (SVG elements), by name.
fn body(name: &str, dark: bool) -> Option<String> {
    let [i, a, f, paper] = palette(dark);
    let s = format!(
        r#"fill="none" stroke="{i}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round""#
    );
    let sa = format!(
        r#"fill="none" stroke="{a}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round""#
    );
    Some(match name {
        "new" => format!(
            r#"<path d="M8 3h11l6 6v20H8z" fill="{f}" {s}/><path d="M19 3v6h6" {s}/><path d="M16 15v10M11 20h10" {sa}/>"#
        ),
        "open" => format!(
            r#"<path d="M3 8h9l3 3h13v4" {s}/><path d="M3 8v19h22l5-12H8L3 27" fill="{f}" {s}/>"#
        ),
        "save" => format!(
            r#"<path d="M5 4h19l4 4v20H5z" fill="{f}" {s}/><path d="M10 4v7h11V4" {s}/><rect x="9" y="17" width="15" height="11" fill="{paper}" {s}/><path d="M12 21h9M12 24h9" {sa}/>"#
        ),
        "save_as" => format!(
            r#"<path d="M5 4h17l4 4v9" {s}/><path d="M5 4v24h10" {s}/><path d="M10 4v7h11V4" {s}/><path d="M18 28l2-6 8-8 4 4-8 8z" fill="{f}" {sa}/>"#
        ),
        "pdf" => format!(
            r#"<path d="M7 3h12l6 6v20H7z" fill="{f}" {s}/><path d="M19 3v6h6" {s}/><path d="M11 22v-6h3a2 2 0 010 4h-3" {sa}/><path d="M19 16v6" {sa}/>"#
        ),
        "exit" => {
            format!(r#"<path d="M14 5H6v22h8" {s}/><path d="M13 16h15M23 11l5 5-5 5" {sa}/>"#)
        }
        "undo" => {
            format!(r#"<path d="M9 13h12a6 6 0 010 12H12" {s}/><path d="M13 8l-5 5 5 5" {sa}/>"#)
        }
        "redo" => {
            format!(r#"<path d="M23 13H11a6 6 0 000 12h9" {s}/><path d="M19 8l5 5-5 5" {sa}/>"#)
        }
        "modify" => format!(r#"<path d="M8 4l16 13-7 1 4 9-3 1-4-9-6 5z" fill="{f}" {s}/>"#),
        "delete" => format!(
            r#"<path d="M6 9h20M12 9V5h8v4" {s}/><path d="M8 9l2 19h12l2-19" fill="{f}" {s}/><path d="M13 14v10M19 14v10" {sa}/>"#
        ),
        "wall" => format!(
            r#"<path d="M3 24l8-6h18l-8 6z" fill="{f}" {s}/><path d="M3 24V10l8-6h18v14" {s}/><path d="M3 10h18v14M21 10l8-6" {s}/><path d="M6 15h12M6 20h12" {sa}/>"#
        ),
        "door" => format!(
            r#"<path d="M4 28h24" {s}/><rect x="8" y="4" width="16" height="24" fill="{f}" {s}/><path d="M12 8h8v20h-8z" fill="{paper}" {s}/><circle cx="18" cy="18" r="1.4" fill="{a}"/>"#
        ),
        "window" => format!(
            r#"<rect x="5" y="6" width="22" height="20" fill="{f}" {s}/><path d="M16 6v20M5 16h22" {s}/><path d="M9 10l4-1M20 10l4-1" {sa}/>"#
        ),
        "grid" => format!(
            r#"<path d="M10 9v20M22 9v20M4 15h24M4 25h24" {s} stroke-dasharray="4 2"/><circle cx="10" cy="5" r="3.5" fill="{paper}" {sa}/><circle cx="22" cy="5" r="3.5" fill="{paper}" {sa}/>"#
        ),
        "ref_plane" => format!(
            r#"<path d="M6 28L26 4" {sa} stroke-dasharray="5 3"/><circle cx="6" cy="28" r="2" fill="{f}" {s}/><circle cx="26" cy="4" r="2" fill="{f}" {s}/>"#
        ),
        "model_line" => format!(
            r#"<path d="M4 26L14 12l6 8 8-14" {s}/><path d="M4 26l-1 2M28 6l1-2" {sa}/>"#
        ),
        "reveal_constraints" => format!(
            r#"<path d="M4 10v14M28 10v14M4 17h24" {s} stroke-dasharray="3 2"/><rect x="12" y="4" width="8" height="7" rx="1" fill="{f}" {sa}/><path d="M14 4V2.5a2 2 0 014 0V4" {sa}/>"#
        ),
        "lock" => format!(
            r#"<rect x="8" y="14" width="16" height="13" rx="2" fill="{f}" {s}/><path d="M11 14V9a5 5 0 0110 0v5" {s}/><circle cx="16" cy="20" r="1.8" fill="{paper}" {sa}/>"#
        ),
        "equality" => format!(
            r#"<path d="M4 8v16M16 8v16M28 8v16" {s}/><path d="M4 16h24" {sa}/><path d="M8 12h4M8 14h4M20 12h4M20 14h4" {sa}/>"#
        ),
        "toposolid" => format!(
            r#"<path d="M3 24l6-8 6 4 6-9 8 13z" fill="{f}" {s}/><path d="M3 24v3h26v-3" {sa}/>"#
        ),
        "topo_subdivide" => format!(
            r#"<path d="M3 24l6-8 6 4 6-9 8 13z" fill="{f}" {s}/><path d="M10 22h12" {sa} stroke-dasharray="3 2"/>"#
        ),
        "topo_graded" => format!(
            r#"<path d="M3 24l6-8 6 4 6-9 8 13" fill="none" {s} stroke-dasharray="3 2"/><path d="M3 24h8l4-6h4l4 6h6" {sa}/>"#
        ),
        "topo_cut_fill" => format!(
            r#"<path d="M3 20h26" {s}/><path d="M6 20q5-8 10 0z" fill="{f}" {sa}/><path d="M16 20q5 8 10 0z" {sa}/>"#
        ),
        "topo_simplify" => format!(
            r#"<path d="M3 24l4-3 3 2 3-5 3 1 3-4 3 2 4-5 4 12z" fill="none" {s}/><path d="M3 24l10-10 16 10" {sa}/>"#
        ),
        "site_settings" => format!(
            r#"<path d="M4 8q12-6 24 0M4 16q12-6 24 0M4 24q12-6 24 0" fill="none" {s}/>"#
        ),
        "property_line" => format!(
            r#"<path d="M5 6h22v20H5z" fill="none" {s} stroke-dasharray="6 2 1 2"/><circle cx="5" cy="6" r="2" fill="{f}" {sa}/>"#
        ),
        "site_component" => format!(
            r#"<circle cx="16" cy="12" r="8" fill="{f}" {s}/><path d="M16 20v8M10 28h12" {sa}/>"#
        ),
        "parts_create" => format!(
            r#"<path d="M5 6h6v20H5zM13 6h5v20h-5zM20 6h7v20h-7z" fill="{f}" {s}/>"#
        ),
        "parts_divide" => format!(
            r#"<path d="M4 8h24v16H4z" fill="{f}" {s}/><path d="M12 4v24M20 4v24" {sa} stroke-dasharray="3 2"/>"#
        ),
        "parts_merge" => format!(
            r#"<path d="M4 10h10v12H4zM18 10h10v12H18z" fill="{f}" {s}/><path d="M12 16h8M17 13l3 3-3 3" {sa}/>"#
        ),
        "parts_exclude" => format!(
            r#"<path d="M6 6h20v20H6z" fill="none" {s} stroke-dasharray="3 2"/><path d="M10 10l12 12M22 10L10 22" {sa}/>"#
        ),
        "parts_restore" => format!(
            r#"<path d="M6 6h20v20H6z" fill="{f}" {s}/><path d="M11 17l4 4 7-9" {sa}/>"#
        ),
        "parts_visibility" => format!(
            r#"<path d="M3 16q13-12 26 0q-13 12-26 0z" fill="none" {s}/><path d="M12 12h3v8h-3zM17 12h3v8h-3z" fill="{f}" {sa}/>"#
        ),
        "winder_stair" => format!(
            r#"<path d="M4 28V20h8l4 4 4-8V4h8v24z" fill="{f}" {s}/><path d="M12 20l4-4M16 24l4-4M20 16h8M20 10h8M4 24h8" {sa}/>"#
        ),
        "mass" => format!(
            r#"<path d="M6 26V10l10-6 10 6v16z" fill="{f}" {s}/><path d="M6 10l10 6 10-6M16 16v10" {sa}/>"#
        ),
        "mass_floors" => format!(
            r#"<path d="M6 26V6h20v20z" fill="{f}" {s}/><path d="M6 12h20M6 19h20" {sa}/>"#
        ),
        "wall_face" => format!(
            r#"<path d="M4 26V8l12-4v18z" fill="{f}" {s}/><path d="M18 26V8h8v18z" fill="{f}" {sa}/>"#
        ),
        "floor_face" => format!(
            r#"<path d="M4 8h24v18H4z" fill="none" {s}/><path d="M2 18l10-4h18l-10 4z" fill="{f}" {sa}/>"#
        ),
        "roof_face" => format!(
            r#"<path d="M6 26V14h20v12z" fill="none" {s}/><path d="M3 15L16 5l13 10z" fill="{f}" {sa}/>"#
        ),
        "curtain_system" => format!(
            r#"<path d="M4 26V6h24v20z" fill="{f}" {s}/><path d="M12 6v20M20 6v20M4 16h24" {sa}/>"#
        ),
        "update_face" => format!(
            r#"<path d="M6 26V10h14v16z" fill="{f}" {s}/><path d="M22 6a8 8 0 0 1 4 10" {sa}/><path d="M27 12l-1 4-4-1" {sa}/>"#
        ),
        "room_separator" => format!(
            r#"<path d="M4 6h24v20H4z" fill="{f}" {s}/><path d="M16 6v20" {sa} stroke-dasharray="3 2"/>"#
        ),
        "tread_numbers" => format!(
            r#"<path d="M4 26h6v-6h6v-6h6V8h6" fill="none" {s}/><path d="M6 17h2M12 11h2" {sa}/>"#
        ),
        "in_place" => format!(
            r#"<path d="M6 26V12l10-8 10 8v14z" fill="{f}" {s}/><path d="M12 26v-8h8v8" fill="{paper}" {sa}/>"#
        ),
        "model_text" => format!(
            r#"<path d="M5 26L12 6h3l7 20M8 19h11" fill="none" {s}/><path d="M22 26h6" {sa}/>"#
        ),
        "group_place" => format!(
            r#"<path d="M4 4h10v10H4zM18 18h10v10H18z" fill="{f}" {s}/><path d="M14 14l4 4" {sa}/>"#
        ),
        "group_create" => format!(
            r#"<path d="M3 3h26v26H3z" fill="none" {sa} stroke-dasharray="3 2"/><path d="M7 7h8v8H7zM17 17h8v8h-8z" fill="{f}" {s}/>"#
        ),
        "group_edit" => format!(
            r#"<path d="M3 3h26v26H3z" fill="none" {s} stroke-dasharray="3 2"/><path d="M9 23l2-6 10-10 4 4-10 10z" fill="{f}" {sa}/>"#
        ),
        "group_ungroup" => format!(
            r#"<path d="M4 4h9v9H4zM19 19h9v9h-9z" fill="{f}" {s}/><path d="M17 4h11v11" fill="none" {sa} stroke-dasharray="3 2"/>"#
        ),
        "group_add" => format!(
            r#"<path d="M3 3h20v20H3z" fill="none" {s} stroke-dasharray="3 2"/><path d="M24 18v10M19 23h10" {sa}/>"#
        ),
        "group_remove" => format!(
            r#"<path d="M3 3h20v20H3z" fill="none" {s} stroke-dasharray="3 2"/><path d="M19 23h10" {sa}/>"#
        ),
        "group_exclude" => format!(
            r#"<path d="M3 3h26v26H3z" fill="none" {s} stroke-dasharray="3 2"/><path d="M10 10l12 12M22 10L10 22" {sa}/>"#
        ),
        "group_restore" => format!(
            r#"<path d="M3 3h26v26H3z" fill="none" {s} stroke-dasharray="3 2"/><path d="M22 12a7 7 0 1 0 1 8" {sa}/><path d="M22 6v6h-6" {sa}/>"#
        ),
        "selection_save" => format!(
            r#"<path d="M4 4h18v18H4z" fill="none" {s} stroke-dasharray="3 2"/><path d="M16 16h12v12H16z" fill="{f}" {sa}/>"#
        ),
        "selection_load" => format!(
            r#"<path d="M4 4h18v18H4z" fill="none" {sa} stroke-dasharray="3 2"/><path d="M10 10h8v8h-8z" fill="{f}" {s}/>"#
        ),
        "area_plan" => format!(
            r#"<path d="M4 6h24v20H4z" fill="{f}" {s}/><path d="M4 16h24M16 6v20" {sa} stroke-dasharray="3 2"/>"#
        ),
        "area_boundary" => format!(
            r#"<path d="M4 6h24v20H4z" fill="{f}" {s}/><path d="M7 9h18v14H7z" fill="none" {sa}/>"#
        ),
        "area" => format!(
            r#"<path d="M4 6h24v20H4z" fill="{f}" {s}/><path d="M9 20l7-10 7 10z" fill="{paper}" {sa}/>"#
        ),
        "color_scheme" => format!(
            r#"<path d="M4 6h12v10H4z" fill="{f}" {s}/><path d="M16 6h12v10H16zM4 16h12v10H4z" fill="{paper}" {sa}/><path d="M16 16h12v10H16z" fill="{f}" {s}/>"#
        ),
        "sloped_glazing" => format!(
            r#"<path d="M3 24L16 8l13 16z" fill="{f}" {s}/><path d="M9.5 16h13M16 8v16" {sa}/>"#
        ),
        "curtain_grid" => format!(
            r#"<path d="M4 4h24v24H4z" fill="{f}" {s}/><path d="M16 4v24M4 16h12" {s}/><path d="M16 16h12" {sa} stroke-dasharray="2 2"/>"#
        ),
        "lining" => format!(
            r#"<path d="M4 4h10v24H4z" fill="{f}" {s}/><path d="M16 4h4v24h-4z" fill="{f}" {sa}/>"#
        ),
        "host_pick" => format!(
            r#"<path d="M4 4h8v24H4z" fill="{f}" {s}/><path d="M16 4h4v24h-4z" fill="{f}" {sa}/><path d="M22 16h8M26 12l4 4-4 4" {sa}/>"#
        ),
        "host_remove" => format!(
            r#"<path d="M4 4h8v24H4z" fill="{f}" {s}/><path d="M20 4h4v24h-4z" fill="{f}" {s}/><path d="M13 13l6 6M19 13l-6 6" {sa}/>"#
        ),
        "host_select" => format!(
            r#"<path d="M4 4h8v24H4z" fill="{f}" {sa}/><path d="M16 4h4v24h-4z" fill="{f}" {s}/>"#
        ),
        "wall_sweep" => format!(
            r#"<path d="M6 4h12v24H6z" fill="{f}" {s}/><path d="M18 8h6v3l-3 2h-3z" fill="{f}" {sa}/>"#
        ),
        "wall_reveal" => format!(
            r#"<path d="M8 4h12v24H8z" fill="{f}" {s}/><path d="M20 14h-4v4h4" fill="{paper}" {sa}/>"#
        ),
        "wall_profile" => format!(
            r#"<path d="M3 28V12h10l3-8 3 8h10v16z" fill="{f}" {s}/><path d="M8 18h5v5H8z" fill="{paper}" {sa}/>"#
        ),
        "stack" => format!(
            r#"<path d="M8 4h14v14H8z" fill="{f}" {s}/><path d="M6 18h20v10H6z" fill="{f}" {sa}/>"#
        ),
        "break_up" => format!(
            r#"<path d="M8 4h14v11H8z" fill="{f}" {s}/><path d="M6 19h20v9H6z" fill="{f}" {s}/><path d="M3 17h26" {sa} stroke-dasharray="2 2"/>"#
        ),
        "split_gap" => format!(
            r#"<path d="M3 13h11v6H3zM18 13h11v6H18z" fill="{f}" {s}/><path d="M16 8v16" {sa} stroke-dasharray="2 2"/>"#
        ),
        "split_horizontal" => format!(
            r#"<path d="M9 4h14v11H9zM9 17h14v11H9z" fill="{f}" {s}/><path d="M5 16h22" {sa} stroke-dasharray="2 2"/>"#
        ),
        "shape_point" => format!(
            r#"<path d="M3 12l13-6 13 6-13 6z" fill="{f}" {s}/><path d="M16 18v8" {sa}/><circle cx="16" cy="18" r="2" fill="{paper}" {sa}/>"#
        ),
        "shape_reset" => format!(
            r#"<path d="M3 16l13-6 13 6-13 6z" fill="{f}" {s}/><path d="M24 6a6 6 0 10-2 8" {sa}/><path d="M24 2v4h-4" {sa}/>"#
        ),
        "slab_edge" => format!(
            r#"<path d="M3 12h26v4H3z" fill="{f}" {s}/><path d="M3 16h6v8H3z" fill="{f}" {sa}/><path d="M23 16h6v8h-6z" fill="{f}" {sa}/>"#
        ),
        "join_roof" => format!(
            r#"<path d="M2 22l10-8 10 8z" fill="{f}" {s}/><path d="M14 18l8-12 8 12" {s}/><path d="M20 16h6" {sa}/>"#
        ),
        "slope_arrow" => format!(
            r#"<path d="M4 26h24L16 8z" fill="{f}" {s}/><path d="M16 25V13" {sa}/><path d="M12 17l4-5 4 5" {sa}/>"#
        ),
        "roof_extrusion" => format!(
            r#"<path d="M3 20l8-8 8 8M3 20v2M19 20v2" {s}/><path d="M11 12l10-6 8 8-8 8" fill="{f}" {sa}/><path d="M19 20l10-6" {s}/>"#
        ),
        "opening_face" => format!(
            r#"<path d="M3 24L17 6l12 10L15 28z" fill="{f}" {s}/><path d="M12 18l5-5 4 3-5 5z" fill="{paper}" {sa}/>"#
        ),
        "scope_box" => format!(
            r#"<path d="M5 11l8-6h14l-8 6zM5 11v14h14V11M19 25l8-6V5" fill="{f}" {s} stroke-dasharray="4 2"/>"#
        ),
        "datum_reset" => format!(
            r#"<path d="M6 6v20M26 6v20" {s} stroke-dasharray="4 2"/><path d="M10 16h12" {sa}/><path d="M13 12l-4 4 4 4" {sa}/>"#
        ),
        "datum_maximize" => format!(
            r#"<path d="M16 9v14" {s} stroke-dasharray="4 2"/><path d="M16 3l-4 5h8zM16 29l-4-5h8z" fill="{f}" {sa}/><rect x="5" y="11" width="22" height="10" fill="none" {s}/>"#
        ),
        "datum_propagate" => format!(
            r#"<rect x="3" y="6" width="11" height="14" fill="{f}" {s}/><rect x="18" y="12" width="11" height="14" fill="{f}" {s}/><path d="M8 9v8M24 15v8" {sa}/><path d="M14 13h4" {s}/>"#
        ),
        "work_plane" => format!(
            r#"<path d="M3 22l10-10h16L19 22z" fill="{f}" {s}/><path d="M8 17h16M13 12l-5 10M21 12l-5 10" {sa}/>"#
        ),
        "work_plane_show" => format!(
            r#"<path d="M3 24l10-10h16L19 24z" fill="{f}" {s}/><path d="M8 19h16M13 14l-5 10M21 14l-5 10" {sa}/><circle cx="16" cy="7" r="3" fill="{paper}" {s}/><path d="M10 7q6-6 12 0q-6 6-12 0" {s}/>"#
        ),
        "level" => format!(
            r#"<path d="M3 12h18M3 24h18" {s} stroke-dasharray="5 2"/><path d="M21 12l4-4 4 4-4 4z" fill="{f}" {sa}/><path d="M21 24l4-4 4 4-4 4z" fill="{f}" {sa}/>"#
        ),
        "dimension" => format!(
            r#"<path d="M5 8v18M27 8v18" {s}/><path d="M5 16h22" {sa}/><path d="M3 18l4-4M25 18l4-4" {sa}/><path d="M12 11h8" {s}/>"#
        ),
        "plan" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M4 16h10v11M18 5v8h10" {s}/><path d="M14 16a5 5 0 015 5" {sa}/>"#
        ),
        "view3d" => format!(
            r#"<path d="M16 4l11 6v12l-11 6-11-6V10z" fill="{f}" {s}/><path d="M5 10l11 6 11-6M16 16v12" {s}/><path d="M16 16l11-6" {sa}/>"#
        ),
        "view_new" => format!(
            r#"<path d="M14 6l9 5v10l-9 5-9-5V11z" fill="{f}" {s}/><path d="M5 11l9 5 9-5M14 16v10" {s}/><path d="M26 3v8M22 7h8" {sa}/>"#
        ),
        "zoom_fit" => format!(
            r#"<circle cx="14" cy="14" r="8" fill="{f}" {s}/><path d="M20 20l7 7" {s}/><path d="M10 12V10h3M18 12V10h-3M10 16v2h3M18 16v2h-3" {sa}/>"#
        ),
        "zoom_prev" => format!(
            r#"<circle cx="16" cy="14" r="8" fill="{f}" {s}/><path d="M22 20l6 6" {s}/><path d="M18 10l-4 4 4 4" {sa}/>"#
        ),
        "zoom_next" => format!(
            r#"<circle cx="16" cy="14" r="8" fill="{f}" {s}/><path d="M22 20l6 6" {s}/><path d="M14 10l4 4-4 4" {sa}/>"#
        ),
        "home" => format!(
            r#"<path d="M4 16L16 5l12 11" {s}/><path d="M8 13v14h16V13" fill="{f}" {s}/><path d="M14 27v-7h4v7" {sa}/>"#
        ),
        "set_home" => format!(
            r#"<path d="M4 16L16 5l12 11" {s}/><path d="M8 13v14h16V13" fill="{f}" {s}/><path d="M12 20l3 3 6-6" {sa}/>"#
        ),
        "perspective" => format!(
            r#"<path d="M4 26l8-18h8l8 18z" fill="{f}" {s}/><path d="M12 8l-2 18M20 8l2 18M7 19h18" {sa}/>"#
        ),
        "walk" => format!(
            r#"<circle cx="17" cy="5" r="2.6" fill="{a}"/><path d="M16 10l-4 8 5 3-2 8M16 10l4 6 5 1M13 14l-5 2M12 18l-5 9" {s}/>"#
        ),
        "close_view" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M4 11h24" {s}/><path d="M12 15l8 8M20 15l-8 8" {sa}/>"#
        ),
        "next_view" => format!(
            r#"<rect x="3" y="7" width="18" height="18" fill="{f}" {s}/><path d="M17 16h12M24 11l5 5-5 5" {sa}/>"#
        ),
        "prev_view" => format!(
            r#"<rect x="11" y="7" width="18" height="18" fill="{f}" {s}/><path d="M15 16H3M8 11l-5 5 5 5" {sa}/>"#
        ),
        "tile" => format!(
            r#"<rect x="4" y="4" width="11" height="11" fill="{f}" {s}/><rect x="17" y="4" width="11" height="11" fill="{f}" {s}/><rect x="4" y="17" width="11" height="11" fill="{f}" {s}/><rect x="17" y="17" width="11" height="11" fill="{f}" {s}/>"#
        ),
        "tab_all" => format!(
            r#"<path d="M4 10h8l2-4h6l2 4h6v18H4z" fill="{f}" {s}/><path d="M4 10h24" {sa}/>"#
        ),
        "check_circuits" => format!(
            r#"<path d="M3 20l5-8 4 6 4-10" {s}/><path d="M26 6l4 8h-8z" {sa}/><path d="M26 9v2" {sa}/>"#
        ),
        "circuit_create" => format!(
            r#"<rect x="3" y="4" width="8" height="12" fill="{f}" {s}/><path d="M11 10c6 0 6 8 12 8" stroke-dasharray="2 2" {s}/><circle cx="25" cy="18" r="3" {sa}/><path d="M25 3v6M22 6h6" {sa}/>"#
        ),
        "circuit_panel" => format!(
            r#"<rect x="4" y="4" width="12" height="24" fill="{f}" {s}/><path d="M7 9h6M7 14h6M7 19h6" {s}/><path d="M16 14h12" stroke-dasharray="2 2" {sa}/>"#
        ),
        "panel_settings" => format!(
            r#"<rect x="4" y="4" width="12" height="24" fill="{f}" {s}/><path d="M7 9h6M7 14h6M7 19h6" {s}/><circle cx="24" cy="20" r="4" {sa}/><path d="M24 13v3M24 24v3M17 20h3M28 20h3" {sa}/>"#
        ),
        "gutter" => format!(
            r#"<path d="M3 8l13-5 13 5" fill="none" {s}/><path d="M4 11h24a6 6 0 0 1-12 6h0a6 6 0 0 1-12-6z" fill="{f}" {s}/>"#
        ),
        "downspout" => format!(
            r#"<path d="M3 6h22a5 5 0 0 1-10 4" fill="{f}" {s}/><path d="M18 10v6l-5 4v9" fill="none" stroke-width="3" {sa}/>"#
        ),
        "penetrations" => format!(
            r#"<rect x="12" y="3" width="8" height="26" fill="{f}" {s}/><path d="M3 16h26" stroke-width="4" {sa}/><circle cx="16" cy="16" r="4.5" {s}/>"#
        ),
        "mep_size" => format!(
            r#"<path d="M3 12h12v8H3zM15 13h8v6h-8z" fill="{f}" {s}/><path d="M26 6v20M23 9l3-3 3 3M23 23l3 3 3-3" {sa}/>"#
        ),
        "schedule_filter" => format!(
            r#"<path d="M4 5h24l-9 11v9l-6 3V16z" fill="{f}" {s}/><path d="M22 22h7M22 26h5" {sa}/>"#
        ),
        "system_inspector" => format!(
            r#"<path d="M3 10h14M17 10l-4-3M17 10l-4 3M3 20h10" fill="none" {s}/><circle cx="21" cy="19" r="5" fill="{f}" {sa}/><path d="M25 23l5 5" stroke-width="2.5" {sa}/>"#
        ),
        "thermostat" => format!(
            r#"<rect x="8" y="4" width="16" height="24" rx="3" fill="{f}" {s}/><circle cx="16" cy="13" r="5" fill="none" {sa}/><path d="M16 13l3-3" {sa}/><path d="M12 22h8" {s}/>"#
        ),
        "assign_control" => format!(
            r#"<rect x="3" y="8" width="8" height="12" rx="2" fill="{f}" {s}/><rect x="19" y="10" width="10" height="10" fill="{f}" {s}/><path d="M11 14h8M16 11l3 3-3 3" fill="none" {sa}/>"#
        ),
        "plumbing_fixture" => format!(
            r#"<path d="M4 12h24v3a12 9 0 0 1-24 0z" fill="{f}" {s}/><path d="M16 6v6M12 6h8M16 24v5" {sa}/><path d="M2 12h28" {s}/>"#
        ),
        "analytical_connection" => format!(
            r#"<rect x="3" y="18" width="10" height="10" fill="{f}" {s}/><path d="M3 6h26" stroke-width="5" {s}/><path d="M8 18V9" stroke-dasharray="3 2" {sa}/>"#
        ),
        "hydronic" => format!(
            r#"<rect x="4" y="6" width="24" height="20" rx="3" fill="none" stroke-width="3" {s}/><circle cx="16" cy="6" r="4" fill="{f}" {s}/><path d="M14 4l4 2-4 2" {sa}/><rect x="11" y="22" width="10" height="7" fill="{f}" {s}/>"#
        ),
        "hydraulic_separation" => format!(
            r#"<path d="M3 10h26M3 22h26" stroke-width="4" {s}/><path d="M13 10v12M19 10v12" stroke-width="4" {sa}/><path d="M16 4v24" stroke-dasharray="3 2" {s}/>"#
        ),
        "sprinkler" => format!(
            r#"<path d="M16 2v8" stroke-width="4" {s}/><path d="M12 10h8l-2 8h-4z" fill="{f}" {s}/><path d="M8 22h16" {s}/><path d="M9 26l-3 4M16 26v4M23 26l3 4" {sa}/>"#
        ),
        "pipe_fitting" => format!(
            r#"<path d="M3 20h10a6 6 0 0 0 6-6V3" fill="none" stroke-width="6" {s}/><path d="M3 20h10a6 6 0 0 0 6-6V3" fill="none" stroke-width="4" stroke="{f}"/><path d="M11 16v8M15 10h8" {sa}/>"#
        ),
        "damper" => format!(
            r#"<rect x="2" y="9" width="28" height="14" fill="none" stroke-width="3" {s}/><path d="M10 11l4 10M18 11l4 10" stroke-width="3" {sa}/><path d="M16 9V4" {s}/>"#
        ),
        "insulation" => format!(
            r#"<rect x="3" y="7" width="26" height="18" rx="4" fill="{f}" {s}/><rect x="3" y="12" width="26" height="8" fill="none" stroke-width="3" {s}/><path d="M7 9l3 2M13 9l3 2M19 9l3 2M7 22l3 2M13 22l3 2M19 22l3 2" {sa}/>"#
        ),
        "duct_lining" => format!(
            r#"<rect x="3" y="7" width="26" height="18" fill="none" stroke-width="3" {s}/><rect x="7" y="11" width="18" height="10" fill="none" stroke-dasharray="3 2" {sa}/>"#
        ),
        "heal" => format!(
            r#"<path d="M3 16h10M19 16h10" stroke-width="5" {s}/><path d="M13 16h6" stroke-width="5" {sa}/><path d="M16 8v4M16 20v4" {sa}/>"#
        ),
        "flex_duct" => format!(
            r#"<path d="M3 24c6 0 4-16 10-16s4 16 10 16 4-12 6-12" fill="none" stroke-width="6" {s}/><path d="M7 21v4M11 13v4M15 9v4M19 17v4M23 21v4" {sa}/>"#
        ),
        "convert_flex" => format!(
            r#"<path d="M3 8h12" stroke-width="5" {s}/><path d="M18 8l-3-3v6z" fill="{f}" {sa}/><path d="M3 24c4 0 4-6 8-6s4 6 8 6 4-6 10-6" fill="none" stroke-width="5" {s}/><rect x="22" y="3" width="8" height="8" fill="{f}" {s}/>"#
        ),
        "valve" => format!(
            r#"<path d="M2 16h5M25 16h5" stroke-width="4" {s}/><path d="M7 10l18 12V10L7 22z" fill="{f}" {s}/><path d="M16 16V6M11 6h10" {sa}/>"#
        ),
        "tee_to_cross" => format!(
            r#"<path d="M3 16h26M16 16V3" stroke-width="5" {s}/><path d="M16 16v13" stroke-width="5" stroke-dasharray="3 2" {sa}/>"#
        ),
        "pipe_slope" => format!(
            r#"<path d="M3 12l26 8" stroke-width="5" {s}/><path d="M3 12l26 8" stroke-width="3" stroke="{f}"/><path d="M6 26h20M26 26v-6" fill="none" {sa}/>"#
        ),
        "parallel_pipes" => format!(
            r#"<path d="M3 8h18a6 6 0 0 1 6 6v15M3 16h14a4 4 0 0 1 4 4v9M3 24h10v5" fill="none" stroke-width="3" {sa}/>"#
        ),
        "riser" => format!(
            r#"<path d="M3 24h12a4 4 0 0 0 4-4V6" fill="none" stroke-width="5" {s}/><path d="M3 24h12a4 4 0 0 0 4-4V6" fill="none" stroke-width="3" stroke="{f}"/><path d="M15 9l4-5 4 5" fill="none" {sa}/><path d="M24 4h6" {sa}/>"#
        ),
        "bind_riser" => format!(
            r#"<path d="M12 28V8" stroke-width="5" {s}/><path d="M12 28V8" stroke-width="3" stroke="{f}"/><path d="M4 6h24" stroke-dasharray="3 2" {sa}/><path d="M18 14l4-4 4 4M22 10v10" fill="none" {sa}/>"#
        ),
        "sizing_calculator" => format!(
            r#"<rect x="5" y="3" width="18" height="26" rx="2" fill="{f}" {s}/><rect x="8" y="6" width="12" height="5" {s}/><path d="M9 16h2M13 16h2M17 16h2M9 20h2M13 20h2M17 20h2M9 24h2M13 24h2" {s}/><path d="M25 10v16M23 12l2-2 2 2M23 24l2 2 2-2" {sa}/>"#
        ),
        "mep_network" => format!(
            r#"<path d="M3 8h10v6H3zM13 10h8l4 4v12" fill="none" {s}/><circle cx="25" cy="27" r="2" fill="{f}" {s}/><path d="M6 20c3-3 5 3 8 0s5 3 8 0" fill="none" {sa}/>"#
        ),
        "analytical_segments" => format!(
            r#"<path d="M3 16h8M21 16h8" stroke-width="3" {s}/><path d="M11 16h10" stroke-width="3" {sa}/><circle cx="11" cy="16" r="2" fill="{f}" {s}/><circle cx="21" cy="16" r="2" fill="{f}" {s}/><path d="M13 9h6M16 6v6" {sa}/>"#
        ),
        "pressure_report" => format!(
            r#"<path d="M6 3h14l6 6v20H6z" fill="{f}" {s}/><path d="M20 3v6h6" fill="none" {s}/><path d="M10 24l4-6 4 3 5-8" fill="none" {sa}/>"#
        ),
        "fixture_units" => format!(
            r#"<path d="M6 6h14v6a7 7 0 0 1-14 0z" fill="{f}" {s}/><path d="M13 19v8M8 27h10" {s}/><path d="M23 10h6M23 16h6M23 22h6" {sa}/>"#
        ),
        "circuit_properties" => format!(
            r#"<rect x="4" y="4" width="18" height="24" fill="{f}" {s}/><path d="M8 10h10M8 15h10M8 20h6" {s}/><path d="M24 12l-4 7h5l-4 8" {sa}/>"#
        ),
        "wiring" => format!(
            r#"<path d="M4 8c8 0 8 16 16 16h8M4 16h8M4 24c4 0 6-4 8-8" fill="none" {sa}/><circle cx="28" cy="24" r="2" {s}/>"#
        ),
        "cable_tray" => format!(
            r#"<path d="M3 12l7-5h19l-7 5z" fill="{f}" {s}/><path d="M3 12v8l7-5V7M22 12v8l7-5" {s}/><path d="M3 20h19v-8" {s}/><path d="M9 12l3 8M15 12l3 8" {sa}/>"#
        ),
        "conduit" => format!(
            r#"<path d="M3 22h10a8 8 0 0 0 8-8V4" fill="none" stroke-width="5" {s}/><path d="M3 22h10a8 8 0 0 0 8-8V4" {sa}/>"#
        ),
        "tray_fitting" => format!(
            r#"<path d="M3 13h26v7H3z" fill="{f}" {s}/><path d="M13 13V4h7v9" fill="{f}" {s}/><path d="M13 13h7" {sa}/>"#
        ),
        "conduit_fitting" => format!(
            r#"<rect x="11" y="10" width="11" height="11" fill="{f}" {s}/><path d="M3 15.5h8M22 15.5h8M16.5 10V3" stroke-width="3" {sa}/>"#
        ),
        "parallel_conduits" => format!(
            r#"<path d="M3 9h12a6 6 0 0 1 6 6v14M3 15h12M3 21h6" fill="none" {sa}/><path d="M15 15v14M9 21v8" {s}/>"#
        ),
        "fitting_plus" => format!(
            r#"<path d="M4 20h10v-10" fill="none" stroke-width="4" {s}/><path d="M22 6v10M17 11h10" {sa}/>"#
        ),
        "fitting_minus" => format!(
            r#"<path d="M4 20h18M13 20v-10" fill="none" stroke-width="4" {s}/><path d="M18 8h10" {sa}/>"#
        ),
        "duct" => format!(
            r#"<path d="M3 11h16v10H3z" fill="{f}" {s}/><path d="M19 11h4l6 6v-6M19 21h10" {s}/><path d="M23 11v10" {sa}/>"#
        ),
        "pipe" => format!(
            r#"<path d="M3 13h14a4 4 0 0 1 4 4v12" fill="none" stroke-width="5" {s}/><path d="M3 13h14a4 4 0 0 1 4 4v12" {sa}/>"#
        ),
        "cap_ends" => format!(
            r#"<path d="M3 11h18v10H3z" fill="{f}" {s}/><path d="M23 9v14" stroke-width="3" {sa}/>"#
        ),
        "panel_supply" => format!(
            r#"<rect x="3" y="3" width="10" height="12" fill="{f}" {s}/><rect x="19" y="17" width="10" height="12" fill="{f}" {s}/><path d="M8 15v8h11" {sa}/><path d="M16 20l3 3-3 3" {sa}/>"#
        ),
        "panel_hierarchy" => format!(
            r#"<rect x="11" y="3" width="10" height="7" fill="{f}" {s}/><rect x="3" y="22" width="10" height="7" fill="{f}" {s}/><rect x="19" y="22" width="10" height="7" fill="{f}" {s}/><path d="M16 10v6M8 16h16M8 16v6M24 16v6" {sa}/>"#
        ),
        "load_classes" => format!(
            r#"<rect x="4" y="5" width="8" height="6" fill="{f}" {s}/><rect x="4" y="14" width="8" height="6" fill="{f}" {s}/><rect x="4" y="23" width="8" height="6" fill="{f}" {s}/><path d="M15 8h13M15 17h10M15 26h7" {sa}/>"#
        ),
        "demand_factor" => format!(
            r#"<path d="M4 28V4M4 28h24" {s}/><path d="M4 10h10l6 8h8" fill="none" {sa}/><circle cx="19" cy="5" r="1.6" {s}/><circle cx="26" cy="11" r="1.6" {s}/><path d="M27 4l-9 8" {s}/>"#
        ),
        "remote_analysis" => format!(
            r#"<path d="M8 22a5 5 0 0 1 1-10 7 7 0 0 1 13-2 5 5 0 0 1 3 9.5" fill="{f}" {s}/><rect x="11" y="18" width="10" height="9" rx="1" {sa}/><path d="M13 18v-3a3 3 0 0 1 6 0v3" {sa}/>"#
        ),
        "terminal" => format!(
            r#"<rect x="3" y="5" width="26" height="22" rx="2" fill="{f}" {s}/><path d="M8 12l5 4-5 4" {sa}/><path d="M15 21h8" {s}/>"#
        ),
        "system_browser" => format!(
            r#"<path d="M6 5v22M6 10h5M6 18h5M6 26h5" {s}/><circle cx="16" cy="10" r="3" fill="{f}" {s}/><circle cx="16" cy="18" r="3" fill="{f}" {s}/><circle cx="16" cy="26" r="3" fill="{f}" {s}/><path d="M19 10h8M19 18h8M19 26h8" {sa}/>"#
        ),
        "mep_types" => format!(
            r#"<rect x="4" y="12" width="14" height="14" fill="{f}" {s}/><path d="M18 19h6" {sa}/><circle cx="25" cy="19" r="2.5" {sa}/><path d="M11 12V6h12" {s}/>"#
        ),
        "mep_component" => format!(
            r#"<rect x="6" y="10" width="20" height="14" fill="{f}" {s}/><path d="M26 17h4M2 17h4" {sa}/><circle cx="16" cy="17" r="3" {s}/>"#
        ),
        "mep_connect" => format!(
            r#"<rect x="3" y="11" width="10" height="10" fill="{f}" {s}/><rect x="19" y="11" width="10" height="10" fill="{f}" {s}/><path d="M13 16h6" {sa}/>"#
        ),
        "system_create" => format!(
            r#"<rect x="4" y="4" width="10" height="8" fill="{f}" {s}/><path d="M9 12v6h12M9 18v6h12" {s}/><circle cx="23" cy="18" r="2.5" {sa}/><circle cx="23" cy="24" r="2.5" {sa}/><path d="M25 5v6M22 8h6" {sa}/>"#
        ),
        "system_add" => format!(
            r#"<path d="M5 8h14M5 16h14M5 24h8" {s}/><path d="M23 18v10M18 23h10" {sa}/>"#
        ),
        "system_remove" => format!(
            r#"<path d="M5 8h14M5 16h14M5 24h8" {s}/><path d="M18 23h10" {sa}/>"#
        ),
        "system_equipment" => format!(
            r#"<rect x="4" y="9" width="14" height="14" fill="{f}" {s}/><path d="M18 16h10" {s}/><path d="M24 12l4 4-4 4" {sa}/>"#
        ),
        "system_disconnect" => format!(
            r#"<rect x="4" y="9" width="14" height="14" fill="{f}" {s}/><path d="M18 16h3M25 16h3" {s}/><path d="M21 12l4 8" {sa}/>"#
        ),
        "system_divide" => format!(
            r#"<path d="M4 10h10M4 22h10" {s}/><path d="M18 10h10M18 22h10" {sa}/><path d="M16 4v24" stroke-dasharray="2 2" {s}/>"#
        ),
        "system_types" => format!(
            r#"<rect x="4" y="5" width="10" height="6" fill="{f}" {s}/><rect x="4" y="13" width="10" height="6" fill="{f}" {s}/><rect x="4" y="21" width="10" height="6" fill="{f}" {s}/><path d="M17 8h11M17 16h11M17 24h11" {sa}/>"#
        ),
        "check_duct" => format!(
            r#"<rect x="3" y="12" width="20" height="8" fill="{f}" {s}/><path d="M26 6l4 8h-8z" {sa}/><path d="M26 9v2" {sa}/>"#
        ),
        "check_pipe" => format!(
            r#"<path d="M3 16h20" stroke-width="5" {s}/><path d="M26 6l4 8h-8z" {sa}/><path d="M26 9v2" {sa}/>"#
        ),
        "show_disconnects" => format!(
            r#"<path d="M3 16h10M19 16h10" {s}/><circle cx="16" cy="16" r="5" {sa}/>"#
        ),
        "full_screen" => format!(
            r#"<rect x="7" y="9" width="18" height="14" fill="{f}" {s}/><path d="M4 10V4h6M22 4h6v6M28 22v6h-6M10 28H4v-6" {sa}/>"#
        ),
        "focus_mode" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M4 10h24" {s}/><rect x="9" y="14" width="14" height="9" {sa}/>"#
        ),
        "radial_menu" => format!(
            r#"<circle cx="16" cy="16" r="12" fill="{f}" {s}/><circle cx="16" cy="16" r="4" {s}/><path d="M16 4v8M16 20v8M4 16h8M20 16h8" {s}/><path d="M24.5 7.5l-5.7 5.7" {sa}/>"#
        ),
        "properties" => format!(
            r#"<rect x="5" y="4" width="22" height="24" fill="{f}" {s}/><path d="M9 10h6M9 16h6M9 22h6" {s}/><path d="M18 10h5M18 16h5M18 22h5" {sa}/>"#
        ),
        "browser" => format!(
            r#"<path d="M6 5v22M6 10h6M6 18h6M6 26h6" {s}/><rect x="12" y="7" width="15" height="6" fill="{f}" {s}/><rect x="12" y="15" width="15" height="6" fill="{f}" {s}/><rect x="12" y="23" width="15" height="6" fill="{f}" {sa}/>"#
        ),
        "move" => format!(
            r#"<path d="M16 3v26M3 16h26" {s}/><path d="M12 7l4-4 4 4M12 25l4 4 4-4M7 12l-4 4 4 4M25 12l4 4-4 4" {sa}/>"#
        ),
        "copy" => format!(
            r#"<rect x="4" y="4" width="15" height="15" fill="{paper}" {s}/><rect x="13" y="13" width="15" height="15" fill="{f}" {sa}/>"#
        ),
        "rotate" => format!(
            r#"<path d="M27 16a11 11 0 11-4-8.5" {s}/><path d="M24 3v6h-6" {sa}/><circle cx="16" cy="16" r="2" fill="{a}"/>"#
        ),
        "mirror_pick" => format!(
            r#"<path d="M16 3v26" {sa} stroke-dasharray="4 2"/><path d="M13 8L4 24h9z" fill="{f}" {s}/><path d="M19 8l9 16h-9z" fill="{paper}" {s}/>"#
        ),
        "mirror_draw" => format!(
            r#"<path d="M16 7v22" {sa} stroke-dasharray="4 2"/><path d="M13 12L4 26h9z" fill="{f}" {s}/><path d="M19 12l9 14h-9z" fill="{paper}" {s}/><path d="M12 4h8" {sa}/>"#
        ),
        "align" => format!(
            r#"<path d="M5 4v24" {sa}/><rect x="9" y="7" width="14" height="6" fill="{f}" {s}/><rect x="9" y="19" width="19" height="6" fill="{f}" {s}/>"#
        ),
        "offset" => format!(r#"<path d="M4 22L14 8h14" {s}/><path d="M4 28l12-16h12" {sa}/>"#),
        "trim" => format!(
            r#"<path d="M6 26V10M6 26h16" {s}/><path d="M6 10V5M22 26h5" {sa} stroke-dasharray="2 2"/><circle cx="6" cy="26" r="2.4" fill="{a}"/>"#
        ),
        "trim_single" => format!(
            r#"<path d="M24 4v24" {sa}/><path d="M4 16h20" {s}/><path d="M24 16h4" {s} stroke-dasharray="2 2"/>"#
        ),
        "split" => format!(r#"<path d="M3 16h10M19 16h10" {s}/><path d="M14 8l4 16" {sa}/>"#),
        "pin" => format!(
            r#"<path d="M12 4h8l-1 8 4 4H9l4-4z" fill="{f}" {s}/><path d="M16 16v12" {sa}/>"#
        ),
        "unpin" => format!(
            r#"<path d="M12 4h8l-1 8 4 4H9l4-4z" fill="{f}" {s}/><path d="M16 16v12" {s}/><path d="M6 6l20 20" {sa}/>"#
        ),
        "flip" => format!(
            r#"<path d="M4 16h24" {s}/><path d="M16 4v8M12 8l4-4 4 4M16 28v-8M12 24l4 4 4-4" {sa}/>"#
        ),
        "flip_hand" => format!(
            r#"<path d="M16 6v20" {s}/><path d="M4 16h8M8 12l-4 4 4 4M28 16h-8M24 12l4 4-4 4" {sa}/>"#
        ),
        "ceiling" => format!(
            r#"<path d="M3 10l9-6h17l-9 6z" fill="{f}" {s}/><path d="M3 10v3h17v-3M20 13l9-6V4" {s}/><path d="M8 18v8M16 18v8M24 18v8" {sa} stroke-dasharray="2 2"/>"#
        ),
        "auto_ceiling" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M7 8h18v16H7z" {sa}/><path d="M16 12l3 6h-6z" fill="{a}"/>"#
        ),
        "section_box" => format!(
            r#"<path d="M6 11l8-5h12v15l-8 5H6z" fill="{f}" {s}/><path d="M6 11h12v15M18 11l8-5" {s}/><path d="M10 20l3-3 3 3" {sa}/><circle cx="12" cy="18.5" r="1.6" fill="{a}"/>"#
        ),
        "selection_box" => format!(
            r#"<path d="M5 10l7-5h15v14l-7 5H5z" {s} stroke-dasharray="3 2"/><rect x="10" y="11" width="8" height="8" fill="{f}" {sa}/>"#
        ),
        "slice_h" => format!(
            r#"<path d="M8 6h16v20H8z" fill="{f}" {s}/><path d="M3 16h26" {sa}/><path d="M8 6h16v10H8z" fill="{paper}" {s} stroke-dasharray="2 2"/>"#
        ),
        "slice_v" => format!(
            r#"<path d="M6 8h20v16H6z" fill="{f}" {s}/><path d="M16 3v26" {sa}/><path d="M16 8h10v16H16z" fill="{paper}" {s} stroke-dasharray="2 2"/>"#
        ),
        "clear_slices" => format!(
            r#"<path d="M8 8h16v16H8z" fill="{f}" {s}/><path d="M3 16h12" {sa} stroke-dasharray="2 2"/><path d="M19 19l8 8M27 19l-8 8" {sa}/>"#
        ),
        "hide" => format!(
            r#"<path d="M3 16c4-6 9-9 13-9s9 3 13 9c-4 6-9 9-13 9S7 22 3 16z" fill="{f}" {s}/><circle cx="16" cy="16" r="4" {s}/><path d="M5 27L27 5" {sa}/>"#
        ),
        "hide_tree" => format!(
            r#"<rect x="4" y="4" width="8" height="6" fill="{f}" {s}/><path d="M8 10v14h6M8 17h6" {s}/><rect x="14" y="14" width="8" height="6" {s}/><rect x="14" y="21" width="8" height="6" {s}/><path d="M18 30L30 12" {sa}/>"#
        ),
        "hide_category" => format!(
            r#"<rect x="4" y="5" width="10" height="10" fill="{f}" {s}/><rect x="18" y="5" width="10" height="10" fill="{f}" {s}/><rect x="4" y="18" width="10" height="10" fill="{f}" {s}/><path d="M17 29L29 17" {sa}/>"#
        ),
        "isolate" => format!(
            r#"<rect x="4" y="4" width="24" height="24" {s} stroke-dasharray="3 3"/><rect x="11" y="11" width="10" height="10" fill="{f}" {sa}/>"#
        ),
        "isolate_tree" => format!(
            r#"<rect x="3" y="3" width="26" height="26" {s} stroke-dasharray="3 3"/><rect x="8" y="7" width="8" height="5" fill="{f}" {sa}/><path d="M12 12v12h5M12 18h5" {sa}/><rect x="17" y="16" width="7" height="4" {sa}/><rect x="17" y="22" width="7" height="4" {sa}/>"#
        ),
        "isolate_category" => format!(
            r#"<rect x="3" y="3" width="26" height="26" {s} stroke-dasharray="3 3"/><rect x="7" y="7" width="8" height="8" fill="{f}" {sa}/><rect x="17" y="7" width="8" height="8" fill="{f}" {sa}/><rect x="7" y="17" width="8" height="8" {s}/>"#
        ),
        "ghost" => format!(
            r#"<rect x="4" y="4" width="24" height="24" {s} stroke-dasharray="3 3" opacity="0.5"/><rect x="11" y="11" width="10" height="10" fill="{f}" {sa}/><path d="M4 4l7 7M28 4l-7 7M4 28l7-7M28 28l-7-7" {s} opacity="0.5"/>"#
        ),
        "translucent" => format!(
            r#"<rect x="4" y="8" width="16" height="16" fill="{f}" {s}/><rect x="12" y="4" width="16" height="16" fill="{f}" fill-opacity="0.45" {sa}/>"#
        ),
        "reset_temp" => format!(
            r#"<path d="M3 16c4-6 9-9 13-9s9 3 13 9c-4 6-9 9-13 9S7 22 3 16z" fill="{f}" {s}/><circle cx="16" cy="16" r="4" {sa}/>"#
        ),
        "tree" => format!(
            r#"<rect x="4" y="3" width="10" height="6" fill="{f}" {s}/><path d="M9 9v17h7M9 17h7" {s}/><rect x="16" y="14" width="12" height="6" {sa}/><rect x="16" y="23" width="12" height="6" {sa}/>"#
        ),
        "schedule" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M4 11h24M4 17h24M4 22h24M13 5v22" {s} stroke-width="1.4"/><path d="M4 5h24v6H4z" fill="{a}" opacity="0.5"/>"#
        ),
        "sheet" => format!(
            r#"<rect x="3" y="6" width="26" height="20" fill="{f}" {s}/><rect x="6" y="9" width="12" height="9" {s} stroke-width="1"/><path d="M19 20h7v4h-7z" fill="{a}" {sa}/>"#
        ),
        "place_view" => format!(
            r#"<rect x="3" y="6" width="26" height="20" fill="{f}" {s}/><path d="M8 11h10v8H8zM8 15h10M13 11v8" {s} stroke-width="1.2"/><path d="M22 10v8M18 14h8" {sa}/>"#
        ),
        "place_schedule" => format!(
            r#"<rect x="3" y="6" width="26" height="20" fill="{f}" {s}/><path d="M7 10h11v9H7zM7 13h11M7 16h11M11 10v9" {s} stroke-width="1.1"/><path d="M23 10v8M19 14h8" {sa}/>"#
        ),
        "open_view" => format!(
            r#"<rect x="3" y="6" width="20" height="16" fill="{f}" {s}/><path d="M7 10h9v8H7z" {s} stroke-width="1.2"/><path d="M18 20l9 7M27 21v6h-6" {sa}/>"#
        ),
        "text" => format!(
            r#"<path d="M6 7h20M16 7v19M11 26h10" {s} stroke-width="2.6"/><path d="M6 7v3M26 7v3" {sa}/>"#
        ),
        "tag" => format!(
            r#"<rect x="3" y="9" width="17" height="11" fill="{f}" {s}/><path d="M7 15h9" {s}/><path d="M20 14l9 9" {sa}/><circle cx="29" cy="23" r="1.5" fill="{a}"/>"#
        ),
        "room_tag" => format!(
            r#"<path d="M3 3h26v26H3z" {s}/><path d="M9 12h14M11 17h10M12 22h8" {sa}/>"#
        ),
        "tag_all" => format!(
            r#"<rect x="3" y="4" width="13" height="8" fill="{f}" {s}/><rect x="16" y="13" width="13" height="8" fill="{f}" {s}/><rect x="3" y="22" width="13" height="8" fill="{f}" {s}/><path d="M7 8h5M20 17h5M7 26h5" {sa}/>"#
        ),
        "stair" => format!(
            r#"<path d="M3 27h6v-5h6v-5h6v-5h6v-5h3" fill="none" {s} stroke-width="2"/><path d="M3 27h27V7" fill="{f}" opacity="0.5"/><path d="M8 12l6-6M14 6h-4M14 6v4" {sa}/>"#
        ),
        "spiral_stair" => format!(
            r#"<circle cx="16" cy="16" r="12" fill="{f}" {s}/><circle cx="16" cy="16" r="3" {s}/><path d="M16 13V4M18.5 14.5l7-6M19 16h9M18.5 17.5l7 6" {s} stroke-width="1.2"/><path d="M8 22a10 10 0 0 1 0-12" {sa}/>"#
        ),
        "deck" => format!(
            r#"<path d="M3 12h26M3 17h26M3 22h26" {s} stroke-width="3.2"/><path d="M11 12v10M21 17v5M16 12v5" {sa} stroke-width="1.2"/><path d="M6 23v6M26 23v6" {s}/>"#
        ),
        "railing" => format!(
            r#"<path d="M3 9h26M3 25h26" {s} stroke-width="2.4"/><path d="M5 9v16M27 9v16" {s} stroke-width="2.6"/><path d="M10 9v16M14 9v16M18 9v16M22 9v16" {sa} stroke-width="1.2"/>"#
        ),
        "guard_edges" => format!(
            r#"<path d="M3 26h26l-6-8H9z" fill="{f}" {s}/><path d="M9 18V9h14v9M9 12h14" {sa}/><path d="M13 12v6M17 12v6" {sa} stroke-width="1.2"/>"#
        ),
        "rail_opening" => format!(
            r#"<path d="M3 12h9M20 12h9M3 26h9M20 26h9" {sa}/><path d="M4 9v18M11 9v18M21 9v18M28 9v18" {s}/><path d="M14 19h4" {sa} stroke-dasharray="1 1.5"/>"#
        ),
        "gate" => format!(
            r#"<path d="M4 8v20M28 8v20" {s}/><rect x="7" y="11" width="18" height="15" fill="{f}" {s}/><path d="M11 11v15M15 11v15M19 11v15M23 11v15" {sa} stroke-width="1.2"/>"#
        ),
        "double_gate" => format!(
            r#"<path d="M3 8v20M29 8v20" {s}/><rect x="5" y="11" width="10" height="15" fill="{f}" {s}/><rect x="17" y="11" width="10" height="15" fill="{f}" {s}/><path d="M10 11v15M22 11v15" {sa} stroke-width="1.2"/>"#
        ),
        "arc_wall" => format!(
            r#"<path d="M4 26A20 20 0 0 1 26 6" fill="none" {sa} stroke-width="5"/><path d="M4 26A20 20 0 0 1 26 6" fill="none" stroke="{f}" stroke-width="2"/>"#
        ),
        "spline_wall" => format!(
            r#"<path d="M3 24C9 4 15 30 21 12S29 6 29 6" fill="none" {sa} stroke-width="5"/><path d="M3 24C9 4 15 30 21 12S29 6 29 6" fill="none" stroke="{f}" stroke-width="2"/>"#
        ),
        "arched_roof" => format!(
            r#"<path d="M3 24C3 10 29 10 29 24" fill="none" {sa}/><path d="M7 24v4h18v-4" {s}/><path d="M3 24h26" {s}/>"#
        ),
        "dome" => format!(
            r#"<path d="M5 22a11 11 0 0 1 22 0z" fill="{f}" {s}/><path d="M5 22h22v5H5z" {s}/><path d="M16 11V6" {sa}/>"#
        ),
        "virtual_volumes" => format!(
            r#"<path d="M5 27V5h4v22" fill="{f}" {s}/><path d="M9 27A18 18 0 0 0 27 9" fill="none" {sa} stroke-dasharray="2 2"/><path d="M9 9h18" {sa} stroke-dasharray="2 2"/>"#
        ),
        "remove_boolean" => format!(
            r#"<rect x="4" y="9" width="15" height="15" fill="{f}" {s}/><rect x="13" y="4" width="15" height="15" fill="none" {s}/><path d="M20 22l8 8M28 22l-8 8" {sa}/>"#
        ),
        "sketch_arc" => format!(
            r#"<path d="M5 25A13 13 0 0 1 27 25" fill="none" {sa}/><circle cx="5" cy="25" r="2" fill="{a}"/><circle cx="27" cy="25" r="2" fill="{a}"/>"#
        ),
        "sketch_circle" => format!(
            r#"<circle cx="16" cy="16" r="11" fill="none" {sa}/><path d="M16 16h11" {s}/><circle cx="16" cy="16" r="1.5" fill="{a}"/>"#
        ),
        "accessibility" => format!(
            r#"<circle cx="14" cy="5" r="2.5" fill="{a}"/><path d="M14 9v9h7l3 7" fill="none" {sa}/><path d="M9 13h10" {s}/><path d="M11 15a7 7 0 1 0 8 9" fill="none" {s}/>"#
        ),
        "close_model" => format!(
            r#"<path d="M6 4h14l6 6v18H6z" fill="{f}" {s}/><path d="M12 15l8 8M20 15l-8 8" {sa}/>"#
        ),
        "help" => format!(
            r#"<circle cx="16" cy="16" r="12" fill="{f}" {s}/><path d="M12 12.5a4 4 0 1 1 5.5 3.7c-1 .4-1.5 1.2-1.5 2.3v1" fill="none" {sa}/><circle cx="16" cy="23.5" r="1.4" fill="{a}"/>"#
        ),
        "about" => format!(
            r#"<circle cx="16" cy="16" r="12" fill="{f}" {s}/><path d="M16 14v9" {sa}/><circle cx="16" cy="10" r="1.4" fill="{a}"/>"#
        ),
        "model_properties" => format!(
            r#"<path d="M6 4h14l6 6v18H6z" fill="{f}" {s}/><path d="M10 12h12M10 17h12M10 22h7" {s}/><path d="M21 22l2 2 4-5" {sa}/>"#
        ),
        // A classical column: capital, fluted shaft, base.
        "component" => format!(
            r#"<path d="M7 5h18v4H7zM9 27h14v-3H9z" fill="{f}" {s}/><path d="M11 9h10l-1 15h-8z" fill="{paper}" {s}/><path d="M14 11v11M18 11v11" {sa}/>"#
        ),
        "component_create" => format!(
            r#"<rect x="4" y="9" width="12" height="12" fill="{f}" {s}/><path d="M16 15h5M18 12l3 3-3 3" {sa}/><path d="M22 6h7v20h-7" {s}/><path d="M24 11h3M24 16h3M24 21h3" {s}/>"#
        ),
        "import_object" => format!(
            r#"<path d="M14 15l9-5 6 3v10l-9 5-6-3z" fill="{f}" {s}/><path d="M14 15l6 3 9-5M20 18v10" {s}/><path d="M3 9h9M8 5l4 4-4 4" {sa}/>"#
        ),
        "library" => format!(
            r#"<path d="M3 8h9l3 3h14v16H3z" fill="{f}" {s}/><rect x="8" y="15" width="6" height="8" fill="{paper}" {sa}/><rect x="18" y="15" width="6" height="8" fill="{paper}" {sa}/>"#
        ),
        "path_array" => format!(
            r#"<path d="M3 26C10 26 10 8 29 8" {s}/><circle cx="5" cy="26" r="2.6" fill="{f}" {sa}/><circle cx="12" cy="20" r="2.6" fill="{f}" {sa}/><circle cx="16" cy="12" r="2.6" fill="{f}" {sa}/><circle cx="25" cy="8" r="2.6" fill="{f}" {sa}/>"#
        ),
        "unarray" => format!(
            r#"<circle cx="7" cy="16" r="3" fill="{f}" {s}/><circle cx="16" cy="16" r="3" fill="{f}" {s}/><circle cx="25" cy="16" r="3" fill="{f}" {s}/><path d="M4 8l24 16M4 24L28 8" {sa}/>"#
        ),
        "skylight" => format!(
            r#"<path d="M2 26L16 6l14 20z" fill="{f}" {s}/><path d="M12 22l4-8 5 8z" fill="{paper}" {sa}/><path d="M16 14v8M13 18h6" {sa}/>"#
        ),
        "ramp" => format!(
            r#"<path d="M3 27h26V11z" fill="{f}" {s}/><path d="M7 25l18-11" {sa}/><path d="M21 13l4 1-1 4" {sa}/>"#
        ),
        "measure" => format!(
            r#"<rect x="3" y="11" width="26" height="10" fill="{f}" {s}/><path d="M8 11v4M13 11v6M18 11v4M23 11v6" {sa}/>"#
        ),
        "curtain_wall" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M12 4v24M20 4v24M4 16h24" {sa}/>"#
        ),
        "assembly" => format!(
            r#"<path d="M6 4v24M11 4v24M21 4v24M26 4v24" {s}/><rect x="11" y="4" width="10" height="24" fill="{f}" {sa}/>"#
        ),
        "array" => format!(
            r#"<rect x="3" y="12" width="7" height="8" fill="{f}" {s}/><rect x="13" y="12" width="7" height="8" fill="{f}" {sa}/><rect x="23" y="12" width="7" height="8" fill="{f}" {sa}/><path d="M6 25h20" {s}/>"#
        ),
        "array_radial" => format!(
            r#"<circle cx="16" cy="16" r="2" fill="{a}"/><rect x="13" y="3" width="6" height="6" fill="{f}" {s}/><rect x="23" y="13" width="6" height="6" fill="{f}" {sa}/><rect x="13" y="23" width="6" height="6" fill="{f}" {sa}/><rect x="3" y="13" width="6" height="6" fill="{f}" {sa}/>"#
        ),
        "column" => format!(
            r#"<path d="M9 4h14v4H9zM9 25h14v4H9z" fill="{f}" {s}/><path d="M11 8v17M21 8v17" {s}/><path d="M16 10v13" {sa}/>"#
        ),
        "structural_column" => format!(
            r#"<path d="M8 4h16v4H8zM8 25h16v4H8z" fill="{a}" {s}/><path d="M15 8h2v17h-2z" fill="{f}" {s}/>"#
        ),
        "structural_beam" => format!(
            r#"<path d="M3 9h26v3H3zM3 20h26v3H3z" fill="{a}" {s}/><path d="M3 12h26v8H3z" fill="{f}" {s}/>"#
        ),
        "beams_on_grids" => format!(
            r#"<path d="M4 16h24M9 4v24M23 4v24" {s} stroke-dasharray="3 2"/><path d="M7 14h4v4H7zM21 14h4v4h-4z" fill="{a}" {s}/><path d="M11 14.5h10v3H11z" fill="{f}" {s}/>"#
        ),
        "structural_brace" => format!(
            r#"<path d="M4 4v24M28 4v24" {s}/><path d="M6 26L26 6l2 2L8 28z" fill="{f}" {s}/>"#
        ),
        "slanted_column" => format!(
            r#"<path d="M4 26h12v3H4zM16 4h12v3H16z" fill="{a}" {s}/><path d="M9 26l10-19h3L12 26z" fill="{f}" {s}/>"#
        ),
        "footing" => format!(
            r#"<path d="M13 4h6v14h-6z" fill="{a}" {s}/><path d="M4 18h24v8H4z" fill="{f}" {s}/>"#
        ),
        "footings_at_columns" => format!(
            r#"<path d="M7 4h4v10H7zM21 4h4v10h-4z" fill="{a}" {s}/><path d="M3 14h12v5H3zM17 14h12v5H17z" fill="{f}" {s}/><path d="M9 22v6M23 22v6" {sa}/>"#
        ),
        "wall_foundation" => format!(
            r#"<path d="M12 3h8v17h-8z" fill="{a}" {s}/><path d="M5 20h22v7H5z" fill="{f}" {s}/>"#
        ),
        "foundation_slab" => format!(
            r#"<path d="M3 13l8-6h18l-8 6z" fill="{a}" {s}/><path d="M3 13h18v8H3zM21 13l8-6v8l-8 6z" fill="{f}" {s}/>"#
        ),
        "structural_wall" => format!(
            r#"<path d="M6 4h20v24H6z" fill="{f}" {s}/><path d="M6 10h20M6 16h20M6 22h20M16 4v6M11 10v6M21 10v6M16 16v6M11 22v6M21 22v6" {sa}/>"#
        ),
        "structural_floor" => format!(
            r#"<path d="M3 14l8-6h18l-8 6z" fill="{a}" {s}/><path d="M3 14h18v6H3zM21 14l8-6v6l-8 6z" fill="{f}" {s}/><path d="M8 24h14M19 21l3 3-3 3" {sa}/>"#
        ),
        "beam_system" => format!(
            r#"<path d="M4 4h24v24H4z" fill="none" {sa}/><path d="M9 4v24M14 4v24M19 4v24M24 4v24" {s}/>"#
        ),
        "beam_system_remove" => format!(
            r#"<path d="M4 4h24v24H4z" fill="none" {sa} stroke-dasharray="3 2"/><path d="M10 4v24M16 4v24M22 4v24" {s}/>"#
        ),
        "truss" => format!(
            r#"<path d="M3 22h26M3 10h26M3 10v12M29 10v12M10 10v12M16 10v12M22 10v12" {s}/><path d="M3 10l7 12M10 10l6 12M22 22l7-12M16 22l6-12" {sa}/>"#
        ),
        "truss_remove" => format!(
            r#"<path d="M3 22h26M3 10h26" {s} stroke-dasharray="3 2"/><path d="M3 10l7 12M10 22l6-12M16 10l6 12M22 22l7-12" {sa}/>"#
        ),
        "rebar" => format!(
            r#"<rect x="5" y="5" width="22" height="22" fill="{f}" {s}/><rect x="9" y="9" width="14" height="14" rx="2" fill="none" {sa}/><circle cx="11" cy="11" r="1.6" fill="{a}"/><circle cx="21" cy="11" r="1.6" fill="{a}"/><circle cx="11" cy="21" r="1.6" fill="{a}"/><circle cx="21" cy="21" r="1.6" fill="{a}"/>"#
        ),
        "rebar_cover" => format!(
            r#"<rect x="5" y="5" width="22" height="22" fill="{f}" {s}/><path d="M5 22h22" {sa} stroke-dasharray="2 2"/><path d="M16 22v5M14 24l2-2 2 2M14 25l2 2 2-2" {sa}/>"#
        ),
        "rebar_remove_bar" => format!(
            r#"<path d="M8 6v20M14 6v20M26 6v20" {sa}/><path d="M17 12l6 8M23 12l-6 8" {s}/>"#
        ),
        "rebar_split" => format!(
            r#"<path d="M6 6v20M10 6v20M14 6v20M20 6v20M24 6v20" {sa}/><path d="M17 4v24" {s} stroke-dasharray="2 2"/>"#
        ),
        "rebar_move_bar" => format!(
            r#"<path d="M8 6v20M24 6v20" {sa}/><path d="M14 6v20" {sa} stroke-dasharray="2 2"/><path d="M14 16h6M18 13l3 3-3 3" {s}/>"#
        ),
        "rebar_reset" => format!(
            r#"<path d="M8 8v16M14 8v16M20 8v16" {sa}/><path d="M24 10a7 7 0 1 1-2-4M24 4v4h-4" fill="none" {s}/>"#
        ),
        "rebar_path_distribution" => format!(
            r#"<path d="M4 26C10 26 14 6 28 6" fill="none" {s} stroke-dasharray="2 2"/><path d="M6 20v10M14 14v10M22 6v10" {sa}/>"#
        ),
        "rebar_morphed" => format!(
            r#"<circle cx="10" cy="16" r="6" fill="none" {s}/><circle cx="22" cy="16" r="6" fill="none" {s}/><path d="M4 12h24M4 20h24" {sa}/>"#
        ),
        "precast_split" => format!(
            r#"<rect x="3" y="9" width="8" height="14" fill="{f}" {s}/><rect x="12" y="9" width="8" height="14" fill="{f}" {s}/><rect x="21" y="9" width="8" height="14" fill="{f}" {s}/><path d="M11.5 5v22M20.5 5v22" {sa} stroke-dasharray="2 2"/>"#
        ),
        "precast_resplit" => format!(
            r#"<rect x="4" y="12" width="11" height="14" fill="{f}" {s}/><rect x="17" y="12" width="11" height="14" fill="{f}" {s}/><path d="M24 8a8 6 0 1 0-16 0M24 3v5h-5" fill="none" {sa}/>"#
        ),
        "precast_reinforce" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><rect x="12" y="10" width="8" height="12" fill="none" {s}/><path d="M8 9v18M24 9v18M4 13h8M20 13h8" {sa}/>"#
        ),
        "precast_fabric" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M8 9v14M12 9v14M16 9v4M16 19v4M20 9v14M24 9v14M6 11h20M6 15h7M19 15h7M6 19h20" {sa}/><rect x="13" y="13" width="6" height="6" fill="none" {s}/>"#
        ),
        "precast_anchor" => format!(
            r#"<rect x="4" y="14" width="24" height="14" fill="{f}" {s}/><path d="M11 8v12M21 8v12" {sa}/><path d="M8 8a3 3 0 0 1 6 0M18 8a3 3 0 0 1 6 0" fill="none" {s}/>"#
        ),
        "precast_shop" => format!(
            r#"<rect x="3" y="5" width="26" height="22" fill="{f}" {s}/><rect x="6" y="8" width="13" height="9" fill="none" {sa}/><path d="M6 20h13M22 8h4M22 12h4M22 16h4M22 20h4M22 24h4" {s}/>"#
        ),
        "precast_cam" => format!(
            r#"<rect x="3" y="14" width="16" height="12" fill="{f}" {s}/><path d="M21 8h8v18h-8" fill="none" {s}/><path d="M11 6v6M8 9l3 3 3-3M23 13h4M23 17h4M23 21h4" {sa}/>"#
        ),
        "precast_config" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M8 10h4M8 16h4M8 22h4M14 10h10M16 16h8M16 22h8" {sa}/><path d="M12 10v12" {s}/>"#
        ),
        "rebar_free_form" => format!(
            r#"<path d="M3 26C6 10 26 10 29 26" fill="none" {s}/><path d="M7 25C10 14 22 14 25 25M11 25c2-6 8-6 10 0" fill="none" {sa}/>"#
        ),
        "rebar_sketch" => format!(
            r#"<rect x="3" y="7" width="26" height="18" fill="{f}" {s}/><path d="M7 21V12l3-2h12l3 2v9" fill="none" {sa}/><path d="M20 28l7-7 2 2-7 7z" fill="{a}"/>"#
        ),
        "rebar_constraints" => format!(
            r#"<rect x="3" y="5" width="26" height="22" fill="{f}" {s}/><path d="M8 13h16" {sa}/><path d="M16 5v6M14 9l2 2 2-2" {s}/><path d="M3 5h26" stroke="{a}" stroke-width="2"/>"#
        ),
        "rebar_varying" => format!(
            r#"<path d="M6 28L11 4h10l5 24z" fill="{f}" {s}/><path d="M10 23h12M11 17h10M12 11h8" {sa}/>"#
        ),
        "rebar_standard" => format!(
            r#"<rect x="7" y="4" width="18" height="24" fill="{f}" {s}/><path d="M11 23h10M11 16h10M11 9h10" {sa}/>"#
        ),
        "rebar_propagate" => format!(
            r#"<rect x="3" y="12" width="8" height="8" fill="{f}" {s}/><rect x="21" y="6" width="8" height="8" fill="{f}" {s}/><rect x="21" y="18" width="8" height="8" fill="{f}" {s}/><path d="M11 16l10-6M11 16l10 6" {sa}/>"#
        ),
        "rebar_new_host" => format!(
            r#"<rect x="4" y="8" width="10" height="16" fill="{f}" {s} stroke-dasharray="2 2"/><rect x="18" y="8" width="10" height="16" fill="{f}" {s}/><path d="M9 12v8M23 12v8" {sa}/><path d="M12 6c4-3 8-3 11 0" fill="none" {s}/>"#
        ),
        "rebar_splice" => format!(
            r#"<path d="M3 13h18M11 19h18" {sa}/><path d="M11 10v12M21 10v12" {s} stroke-dasharray="2 1"/>"#
        ),
        "rebar_coupler" => format!(
            r#"<path d="M3 16h9M20 16h9" {sa}/><rect x="11" y="12" width="10" height="8" rx="1" fill="{f}" {s}/>"#
        ),
        "rebar_tag" => format!(
            r#"<path d="M6 24L26 8" {sa}/><rect x="12" y="5" width="16" height="9" fill="{f}" {s}/><path d="M15 9.5h10" {s}/>"#
        ),
        "multi_rebar" => format!(
            r#"<path d="M8 6v20M14 6v20M20 6v20" {sa}/><path d="M4 22h24M8 20v4M20 20v4" {s}/><path d="M9 14h11" {s} stroke-dasharray="2 1"/>"#
        ),
        "bending_detail" => format!(
            r#"<path d="M6 8v16h20V8" fill="none" {sa}/><path d="M6 6h3M23 6h3M12 27h8" {s}/>"#
        ),
        "rebar_schedule" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M4 11h24M4 17h24M4 22h24M12 5v22M20 5v22" {s}/><path d="M22 14v6h4v-6" fill="none" {sa}/>"#
        ),
        "rebar_show_all" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M8 10v12M12 10v12M16 10v12M20 10v12M24 10v12" {sa}/>"#
        ),
        "rebar_first_last" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M8 10v12M24 10v12" {sa}/><path d="M8 16h16M8 14v4M24 14v4" {s} stroke-dasharray="2 2"/>"#
        ),
        "rebar_middle" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M16 10v12" {sa}/><path d="M8 16h16M8 14v4M24 14v4" {s} stroke-dasharray="2 2"/>"#
        ),
        "rebar_select" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M10 10v12M20 10v12" {sa}/><path d="M22 18l6 6-3 1 2 3-2 1-2-3-2 2z" fill="{a}"/>"#
        ),
        "rebar_visibility" => format!(
            r#"<rect x="4" y="8" width="18" height="18" fill="{f}" {s} stroke-dasharray="3 2"/><path d="M8 12v10M13 12v10M18 12v10" {sa}/><path d="M18 8c4-4 10-1 12 3-2 4-8 7-12 3" fill="none" {s}/><circle cx="24" cy="10" r="1.6" fill="{a}"/>"#
        ),
        "rebar_numbers" => format!(
            r#"<rect x="4" y="6" width="24" height="20" fill="{f}" {s}/><path d="M9 12l2-2v12M15 12c0-3 5-3 5 0 0 2-5 5-5 10h5M23 10h4l-2 4c3 0 3 8-2 8" fill="none" {sa}/>"#
        ),
        "coordinates" => format!(
            r#"<path d="M6 26V6M6 26h20" {s}/><path d="M6 6l-2 3M6 6l2 3M26 26l-3-2M26 26l-3 2" {s}/><circle cx="19" cy="13" r="2.2" fill="{a}"/><path d="M19 13H6M19 13v13" {sa} stroke-dasharray="2 2"/>"#
        ),
        "area_reinforcement" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M8 11h16M8 16h16M8 21h16M11 8v16M16 8v16M21 8v16" {sa}/>"#
        ),
        "path_reinforcement" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M4 26h24" {s} stroke-width="2"/><path d="M8 26V12M13 26V17M18 26V12M23 26V17" {sa}/>"#
        ),
        "fabric_area" => format!(
            r#"<path d="M4 4h24v24H4z" fill="{f}" {s}/><path d="M6 6h12v10H6zM14 14h12v10H14z" fill="none" {sa}/><path d="M9 6v10M12 6v10M15 6v10M6 9h12M6 12h12" {sa} stroke-width="0.8"/>"#
        ),
        "fabric_sheet" => format!(
            r#"<path d="M6 8h20v16H6z" fill="{f}" {sa}/><path d="M10 8v16M14 8v16M18 8v16M22 8v16M6 12h20M6 16h20M6 20h20" {sa} stroke-width="0.8"/>"#
        ),
        "area_remove" => format!(
            r#"<rect x="4" y="4" width="20" height="20" fill="{f}" {s} stroke-dasharray="3 2"/><path d="M8 10h12M8 15h12M8 20h12" {sa}/><path d="M22 22l7 7M29 22l-7 7" {s}/>"#
        ),
        "rebar_settings" => format!(
            r#"<rect x="4" y="6" width="16" height="16" fill="{f}" {s}/><path d="M7 9h10v10H7z" fill="none" {sa}/><circle cx="24" cy="20" r="4" fill="none" {sa}/><path d="M24 13v3M24 24v3M17 20h3M28 20h3" {sa}/>"#
        ),
        "structural_settings" => format!(
            r#"<path d="M5 6h14v3H5zM9 9h6v14H9zM5 23h14v3H5z" fill="{f}" {s}/><circle cx="24" cy="20" r="4" fill="none" {sa}/><path d="M24 13v3M24 24v3M17 20h3M28 20h3" {sa}/>"#
        ),
        "structural_connection" => format!(
            r#"<path d="M4 16h24M16 4v24" {s}/><circle cx="16" cy="16" r="5" fill="{a}" {s}/>"#
        ),
        "connection_customize" => format!(
            r#"<path d="M4 16h24M16 4v24" {s}/><circle cx="16" cy="16" r="5" fill="{a}" {s}/><path d="M22 6l4 4-8 8h-4v-4z" fill="{f}" {s}/>"#
        ),
        "connection_add" => format!(
            r#"<circle cx="13" cy="16" r="6" fill="{a}" {s}/><path d="M24 10v12M18 16h12" {sa}/>"#
        ),
        "connection_remove" => format!(
            r#"<circle cx="13" cy="16" r="6" fill="{a}" {s}/><path d="M18 16h12" {sa}/>"#
        ),
        "connection_parameter" => format!(
            r#"<circle cx="12" cy="16" r="6" fill="{a}" {s}/><path d="M20 10h8M20 16h8M20 22h8" {sa}/>"#
        ),
        "connection_check" => format!(
            r#"<path d="M4 16h24M16 4v24" {s}/><circle cx="16" cy="16" r="5" fill="{a}" {s}/><path d="M19 26l3 3 6-7" {sa}/>"#
        ),
        "connection_presize" => format!(
            r#"<circle cx="12" cy="16" r="4" fill="{f}" {s}/><circle cx="22" cy="16" r="7" fill="{a}" {s}/>"#
        ),
        "connection_report" => format!(
            r#"<path d="M6 4h14l6 6v18H6z" fill="{f}" {s}/><path d="M10 22l4-5 4 3 4-6" {sa}/>"#
        ),
        "steel_numbering" => format!(
            r#"<path d="M4 10h24v12H4z" fill="{f}" {s}/><path d="M9 14v4M13 14h3l-3 4h3M19 14h3v2h-3v2h3" {sa}/>"#
        ),
        "steel_nc" => format!(
            r#"<path d="M6 4h14l6 6v18H6z" fill="{f}" {s}/><path d="M10 14h12M10 18h12M10 22h8" {sa}/>"#
        ),
        "steel_cope" => format!(
            r#"<path d="M4 12h12v4h12v6H4z" fill="{f}" {s}/><path d="M16 8v8" {sa}/>"#
        ),
        "steel_miter" => format!(
            r#"<path d="M4 20h14l6-6V4h-6v8l-2 2H4z" fill="{f}" {s}/><path d="M16 14l8-8" {sa}/>"#
        ),
        "steel_saw" => format!(
            r#"<path d="M4 12h16v8H4z" fill="{f}" {s}/><path d="M22 6v20" {sa}/><path d="M24 6v20" {s}/>"#
        ),
        "steel_cut_through" => format!(
            r#"<path d="M4 10h24v12H4z" fill="{f}" {s}/><circle cx="16" cy="16" r="5" fill="none" {sa}/>"#
        ),
        "steel_cut_by" => format!(
            r#"<path d="M4 10h24v12H4z" fill="{f}" {s}/><path d="M12 4h8v24h-8z" fill="none" {sa}/>"#
        ),
        "steel_uncope" => format!(
            r#"<path d="M4 12h24v10H4z" fill="{f}" {s}/><path d="M16 4l6 6M22 4l-6 6" {sa}/>"#
        ),
        "steel_shorten" => format!(
            r#"<path d="M4 12h16v8H4z" fill="{f}" {s}/><path d="M28 16h-6M24 13l-3 3 3 3" {sa}/>"#
        ),
        "steel_corner_cut" => format!(
            r#"<path d="M4 6h18l6 6v14H4z" fill="{f}" {s}/><path d="M22 6l6 6" {sa}/>"#
        ),
        "steel_contour_cut" => format!(
            r#"<path d="M4 6h24v20H4z" fill="{f}" {s}/><path d="M11 12h10v8H11z" fill="none" {sa}/>"#
        ),
        "steel_plate" => format!(
            r#"<path d="M6 10l14-4 6 6-14 4z" fill="{a}" {s}/><path d="M6 10v4l6 6v-4M12 16v4l14-4v-4" fill="{f}" {s}/>"#
        ),
        "bolt_pattern" => format!(
            r#"<path d="M4 12h24v8H4z" fill="{f}" {s}/><path d="M10 6v20M16 6v20M22 6v20" {sa}/><path d="M8 6h4M14 6h4M20 6h4" {s}/>"#
        ),
        "weld" => format!(
            r#"<path d="M4 22h24M14 4v18" {s}/><path d="M14 22v-6l-6 6z" fill="{a}" {s}/><path d="M18 10l4-4M20 12l4-4" {sa}/>"#
        ),
        "connection_automate" => format!(
            r#"<path d="M4 12h16M12 4v16" {s}/><circle cx="12" cy="12" r="4" fill="{a}" {s}/><path d="M18 22l3 3 6-7" {sa}/>"#
        ),
        "pile" => format!(
            r#"<path d="M12 4h8v20l-4 5-4-5z" fill="{f}" {s}/>"#
        ),
        "pile_cap" => format!(
            r#"<path d="M4 6h24v8H4z" fill="{a}" {s}/><path d="M7 14h5v12H7zM20 14h5v12h-5z" fill="{f}" {s}/>"#
        ),
        "physical_to_analytical" => format!(
            r#"<path d="M4 6h6v20H4z" fill="{f}" {s}/><path d="M12 16h6M15 13l3 3-3 3" {sa}/><path d="M24 6v20" stroke="rgb(208,26,26)" stroke-width="2"/><circle cx="24" cy="6" r="2" fill="rgb(26,77,208)"/><circle cx="24" cy="26" r="2" fill="rgb(26,77,208)"/>"#
        ),
        "analytical_to_physical" => format!(
            r#"<path d="M6 6v20" stroke="rgb(208,26,26)" stroke-width="2"/><path d="M11 16h6M14 13l3 3-3 3" {sa}/><path d="M21 6h6v20h-6z" fill="{f}" {s}/>"#
        ),
        "analytical_disconnect" => format!(
            r#"<path d="M4 6h6v20H4z" fill="{f}" {s}/><path d="M24 6v20" stroke="rgb(208,26,26)" stroke-width="2"/><path d="M13 12l6 8M19 12l-6 8" {sa}/>"#
        ),
        "analytical_merge" => {
            r#"<path d="M4 8l10 8M4 24l10-8M28 16H14" stroke="rgb(208,26,26)" stroke-width="2"/><circle cx="14" cy="16" r="3" fill="rgb(26,77,208)"/>"#
        .to_string()
        }
        "analytical_show" => {
            r#"<path d="M6 26V8h20v18M6 8l20 18" stroke="rgb(208,26,26)" stroke-width="2" fill="none"/><circle cx="6" cy="8" r="2" fill="rgb(26,77,208)"/><circle cx="26" cy="8" r="2" fill="rgb(26,77,208)"/>"#
        .to_string()
        }
        "load_cases" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M4 11h24M12 4v24" {s}/><path d="M16 15v8M14 21l2 2 2-2M22 15v8M20 21l2 2 2-2" {sa}/>"#
        ),
        "point_load" => format!(
            r#"<path d="M4 24h24" {s}/><path d="M16 4v18M12 17l4 5 4-5" {sa}/>"#
        ),
        "line_load" => format!(
            r#"<path d="M4 24h24" {s}/><path d="M6 8h20" {sa}/><path d="M8 8v14M6 19l2 3 2-3M16 8v14M14 19l2 3 2-3M24 8v14M22 19l2 3 2-3" {sa}/>"#
        ),
        "area_load" => format!(
            r#"<path d="M3 26l6-6h20l-6 6z" fill="{f}" {s}/><path d="M10 4v12M8 13l2 3 2-3M18 4v12M16 13l2 3 2-3M26 4v12M24 13l2 3 2-3" {sa}/>"#
        ),
        "point_support" => format!(
            r#"<circle cx="16" cy="10" r="3" fill="rgb(26,77,208)"/><path d="M16 13l-7 10h14z" fill="{f}" {sa}/><path d="M6 26h20" {s}/>"#
        ),
        "analytical_adjust" => format!(
            r#"<path d="M4 24L16 12l12 12" stroke="rgb(208,26,26)" stroke-width="2" fill="none"/><circle cx="16" cy="12" r="3" fill="rgb(26,77,208)"/><path d="M16 9V3M13 6l3-3 3 3M22 12h5M24 9l3 3-3 3" {sa}/>"#
        ),
        "analytical_reset" => format!(
            r#"<path d="M6 22h20" stroke="rgb(208,26,26)" stroke-width="2"/><path d="M6 12h20" stroke="rgb(208,26,26)" stroke-width="1.5" stroke-dasharray="3 2"/><path d="M16 12v8M13 17l3 3 3-3" {sa}/>"#
        ),
        "results_explorer" => format!(
            r#"<path d="M3 12h26" {s}/><path d="M3 12q13 16 26 0" fill="{f}" stroke="rgb(26,90,230)" stroke-width="2"/><path d="M9 12v5M16 12v8M23 12v5" stroke="rgb(26,90,230)" stroke-width="1.5"/><path d="M3 12l-1 4h2zM29 12l-1 4h2z" fill="{a}" {s}/>"#
        ),
        "analytical_panel" => format!(
            r#"<path d="M4 22l8-14h16l-8 14z" fill="{f}" stroke="rgb(190,50,115)" stroke-width="2"/><circle cx="4" cy="22" r="2" fill="rgb(26,77,208)"/><circle cx="28" cy="8" r="2" fill="rgb(26,77,208)"/>"#
        ),
        "analytical_panel_extrude" => format!(
            r#"<path d="M6 26h20V8H6z" fill="{f}" stroke="rgb(190,50,115)" stroke-width="2"/><path d="M16 22V12M12 16l4-4 4 4" {sa}/>"#
        ),
        "analytical_panel_curved" => format!(
            r#"<path d="M4 26q12-8 24 0V10q-12-8-24 0z" fill="{f}" stroke="rgb(190,50,115)" stroke-width="2"/>"#
        ),
        "analytical_opening" => format!(
            r#"<path d="M4 26h24V6H4z" fill="{f}" stroke="rgb(190,50,115)" stroke-width="2"/><path d="M11 20h10v-8H11z" fill="none" {sa}/>"#
        ),
        "analytical_split" => format!(
            r#"<path d="M4 26h24V6H4z" fill="{f}" stroke="rgb(190,50,115)" stroke-width="2"/><path d="M18 3v26" {sa}/>"#
        ),
        "analytical_cladding" => format!(
            r#"<path d="M4 26h24V6H4z" fill="none" stroke="rgb(140,140,150)" stroke-width="2" stroke-dasharray="3 2"/><path d="M8 10h16M8 16h16M8 22h16" {s}/>"#
        ),
        "analytical_check_supports" => format!(
            r#"<path d="M4 12h18" stroke="rgb(208,26,26)" stroke-width="2"/><path d="M4 12l-2 6h4z" fill="{a}" {s}/><circle cx="24" cy="22" r="6" fill="none" {sa}/><path d="M21 22l2 2 4-4" {sa}/>"#
        ),
        "analytical_check_consistency" => format!(
            r#"<path d="M3 8h10M16 8h10" stroke="rgb(208,26,26)" stroke-width="2"/><circle cx="13" cy="8" r="2" fill="rgb(26,77,208)"/><circle cx="16" cy="8" r="2" fill="rgb(26,77,208)"/><circle cx="22" cy="22" r="6" fill="none" {sa}/><path d="M22 18v5M22 25v1" {sa}/>"#
        ),
        "analytical_checks" => format!(
            r#"<rect x="4" y="4" width="24" height="24" fill="{f}" {s}/><path d="M8 10h3M8 16h3M8 22h3M14 10h10M14 16h10M14 22h10" {s}/><path d="M8 10h3" {sa}/>"#
        ),
        "column_base_supports" => format!(
            r#"<path d="M13 3h6v17h-6z" fill="{f}" {s}/><path d="M8 28l8-8 8 8z" fill="{a}" {s}/>"#
        ),
        "load_combinations" => format!(
            r#"<path d="M8 4v14M4 14l4 4 4-4M20 4v14M16 14l4 4 4-4" {sa}/><path d="M4 24h24M4 28h16" {s}/>"#
        ),
        "run_analysis" => format!(
            r#"<path d="M4 22h24" {s}/><path d="M6 22v-4M26 22v-4" {s}/><path d="M6 18q10 8 20 0" fill="none" {sa}/><path d="M12 4l10 6-10 6z" fill="{a}" {s}/>"#
        ),
        "analysis_results" => format!(
            r#"<path d="M4 26h24M6 26V14M12 26V8M18 26V16M24 26V11" {s}/><path d="M6 14l6-6 6 8 6-5" fill="none" {sa}/>"#
        ),
        "section_catalog" => format!(
            r#"<path d="M6 6h12v3h-4v14h4v3H6v-3h4V9H6z" fill="{f}" {s}/><path d="M21 8h7M21 14h7M21 20h7M21 26h5" {sa}/>"#
        ),
        "columns_at_grids" => format!(
            r#"<path d="M4 9h24M4 23h24M9 4v24M23 4v24" {s} stroke-dasharray="3 2"/><path d="M7 7h4v4H7zM21 7h4v4h-4zM7 21h4v4H7zM21 21h4v4h-4z" fill="{a}" {s}/>"#
        ),
        "column_attach" => format!(
            r#"<path d="M3 10L16 3l13 7" fill="none" {sa}/><path d="M11 9v19h10V6" fill="{f}" {s}/><path d="M16 22v-9M13 16l3-3 3 3" {sa}/>"#
        ),
        "column_detach" => format!(
            r#"<path d="M3 8L16 1l13 7" fill="none" {s}/><path d="M11 13v15h10V13z" fill="{f}" {s}/><path d="M16 4v6M13 7l3 3 3-3" {sa}/>"#
        ),
        "wall_opening" => format!(
            r#"<path d="M3 24l8-6h18l-8 6z" fill="{f}" {s}/><path d="M3 24V10l8-6h18v14" {s}/><path d="M3 10h18v14M21 10l8-6" {s}/><path d="M8 24v-10h8v10" fill="{paper}" {sa}/>"#
        ),
        "vertical_opening" => format!(
            r#"<path d="M2 20l10-7h18l-10 7z" fill="{f}" {s}/><path d="M2 20v4h18l10-7v-4M20 20v4" {s}/><path d="M12 17l3-2h6l-3 2z" fill="{paper}" {sa}/><path d="M16 4v9M13 7l3-3 3 3" {sa}/>"#
        ),
        "shaft" => format!(
            r#"<path d="M3 10l8-5h18l-8 5zM3 19l8-5h18l-8 5zM3 28l8-5h18l-8 5z" fill="{f}" {s}/><path d="M13 9l3-2h6l-3 2zM13 18l3-2h6l-3 2zM13 27l3-2h6l-3 2z" fill="{paper}" {sa}/>"#
        ),
        "stair_opening" => format!(
            r#"<path d="M3 28h6v-5h6v-5h6v-5h6" fill="none" {s}/><path d="M12 5h17v8H12z" fill="none" stroke-dasharray="2 2" {sa}/><path d="M12 5l17 8M29 5L12 13" {sa}/>"#
        ),
        "script" => format!(
            r#"<path d="M7 4h14l5 5v19H7z" fill="{f}" {s}/><path d="M12 14l-3 3 3 3M19 14l3 3-3 3" {sa}/>"#
        ),
        "script_import" => format!(
            r#"<path d="M7 4h14l5 5v19H7z" fill="{f}" {s}/><path d="M16 10v11M12 17l4 4 4-4" {sa}/>"#
        ),
        "script_library" => format!(
            r#"<path d="M4 6h7v21H4zM12 6h7v21h-7z" fill="{f}" {s}/><path d="M21 7l6-1 3 20-6 1z" {s}/><path d="M6 10h3M14 10h3" {sa}/>"#
        ),
        "clash" => format!(
            r#"<rect x="3" y="11" width="16" height="10" fill="{f}" {s}/><rect x="13" y="4" width="8" height="24" fill="none" {s}/><path d="M13 11h6v10h-6z" fill="{a}" {s}/><path d="M24 6l5 5M29 6l-5 5" {sa}/>"#
        ),
        "overlaps" => format!(
            r#"<rect x="4" y="9" width="15" height="15" fill="{f}" {s}/><rect x="13" y="4" width="15" height="15" fill="none" {sa}/><path d="M13 9h6v10h-6z" fill="{a}" {s}/>"#
        ),
        "eaves" => format!(
            r#"<path d="M2 16L16 5l14 11" fill="none" {sa}/><path d="M7 12v16h18V12" fill="{f}" {s}/><path d="M2 16v3h5M30 16v3h-5" {sa}/>"#
        ),
        "dormer" => format!(
            r#"<path d="M2 26L16 8l14 18z" fill="{f}" {s}/><path d="M11 22v-6h6v6" {sa}/><path d="M10 16l4-4 4 4" {sa}/>"#
        ),
        "fields" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M12 5v22M20 5v22" {s}/><path d="M23 13l3 3-3 3" {sa}/>"#
        ),
        "sort" => format!(
            r#"<path d="M9 5v22M5 23l4 4 4-4" {sa}/><path d="M16 8h12M16 14h9M16 20h6M16 26h3" {s}/>"#
        ),
        "itemize" => format!(
            r#"<path d="M5 7h22M5 13h22M5 19h22M5 25h22" {s}/><path d="M24 4v24" {sa} stroke-dasharray="2 2"/>"#
        ),
        "export_csv" => format!(
            r#"<path d="M7 3h12l6 6v20H7z" fill="{f}" {s}/><path d="M11 15h10M11 19h10M11 23h6" {s} stroke-width="1.4"/><path d="M22 14v10M18 20l4 4 4-4" {sa}/>"#
        ),
        "section" => format!(
            r#"<path d="M4 16h24" {s} stroke-dasharray="5 2 1 2"/><path d="M4 9v7M28 9v7" {s}/><path d="M4 22l3-5 3 5zM22 22l3-5 3 5z" fill="{a}" {sa}/>"#
        ),
        "elevation" => format!(
            r#"<rect x="9" y="9" width="14" height="14" fill="{f}" {s}/><path d="M16 3l5 6H11z" fill="{a}" {sa}/><path d="M4 28h24" {s}/>"#
        ),
        "room" => format!(
            r#"<path d="M4 4h24v24H4z" {s}/><path d="M7 7h18v18H7z" fill="{f}" {s} stroke-width="1"/><path d="M11 14h10M13 19h6" {sa}/>"#
        ),
        "auto_room" => format!(
            r#"<path d="M4 4h24v24H4zM16 4v24M4 16h12" {s}/><path d="M7 8h6M7 11h4M19 9h6M19 12h4M7 20h6M7 23h4" {sa} stroke-width="1.6"/>"#
        ),
        "attach" => format!(
            r#"<path d="M3 14L16 5l13 9" {s}/><path d="M8 13v14h16V13" fill="{f}" {s}/><path d="M16 22v-9M12 17l4-4 4 4" {sa}/>"#
        ),
        "detach" => format!(
            r#"<path d="M3 10L16 2l13 8" {s}/><path d="M8 16v11h16V16" fill="{f}" {s}/><path d="M16 9v6M12 12l4 4 4-4" {sa}/>"#
        ),
        "roof" => format!(
            r#"<path d="M2 18L16 6l14 12" {s}/><path d="M5 16l11-9 11 9v2H5z" fill="{f}" {s}/><path d="M7 18v9h18v-9" {s}/><path d="M16 6v4" {sa}/>"#
        ),
        "slope" => format!(
            r#"<path d="M4 26h24" {s}/><path d="M6 26L26 10v16z" fill="{f}" {s}/><path d="M13 21l3-4 3 4z" fill="{a}" {sa}/>"#
        ),
        "overhang" => format!(
            r#"<path d="M3 15L16 6l13 9" {s}/><path d="M9 14v13M23 14v13" {s}/><path d="M23 19h6M26 17l3 2-3 2" {sa}/>"#
        ),
        "ceiling_plan" => format!(
            r#"<rect x="4" y="5" width="24" height="22" fill="{f}" {s}/><path d="M4 12h24M4 19h24M12 5v22M20 5v22" {sa} stroke-width="1.2"/>"#
        ),
        "floor" => format!(
            r#"<path d="M3 20l9-7h17l-9 7z" fill="{f}" {s}/><path d="M3 20v4h17v-4M20 24l9-7v-4" {s}/><path d="M8 18h12" {sa}/>"#
        ),
        "edit_boundary" => format!(
            r#"<path d="M4 8h16v16H4z" {s} stroke-dasharray="3 2"/><path d="M17 27l2-6 9-9 4 4-9 9z" fill="{f}" {sa}/>"#
        ),
        "pick_walls" => format!(
            r#"<path d="M3 26V6h22" {s}/><path d="M6 23V9h19" {sa}/><path d="M18 14l6 13 2-5 5-2z" fill="{f}" {s}/>"#
        ),
        "line" => format!(r#"<path d="M5 27L27 5" {sa}/><circle cx="5" cy="27" r="2.5" fill="{paper}" {s}/><circle cx="27" cy="5" r="2.5" fill="{paper}" {s}/>"#),
        "rectangle" => format!(r#"<rect x="5" y="8" width="22" height="16" fill="{f}" {sa}/>"#),
        "finish" => r##"<path d="M5 17l7 7L27 8" fill="none" stroke="#2fa84f" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round"/>"##.to_owned(),
        "cancel" => r##"<path d="M7 7l18 18M25 7L7 25" fill="none" stroke="#d63b2f" stroke-width="3.5" stroke-linecap="round"/>"##.to_owned(),
        _ => return None,
    })
}

/// The SVG of an icon for light or dark chrome (a placeholder square for unknown names).
pub fn svg(name: &str, dark: bool) -> String {
    let [ink, _, fill, _] = palette(dark);
    let body = body(name, dark).unwrap_or_else(|| {
        format!(r#"<rect x="6" y="6" width="20" height="20" fill="{fill}" stroke="{ink}" stroke-width="2"/>"#)
    });
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">{body}</svg>"#
    )
}

/// Whether an icon exists.
#[cfg(test)]
pub fn exists(name: &str) -> bool {
    body(name, false).is_some()
}
