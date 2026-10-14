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
        "model_properties" => format!(
            r#"<path d="M6 4h14l6 6v18H6z" fill="{f}" {s}/><path d="M10 12h12M10 17h12M10 22h7" {s}/><path d="M21 22l2 2 4-5" {sa}/>"#
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
