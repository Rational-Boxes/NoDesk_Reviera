// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Deck properties (ARC-DECK-001): a selected deck shows its board and framing settings, which
//! lay it out again (`deck.set_boards`, `deck.set_framing`), and its cut list.

use reviera_core::ElementId;
use reviera_core::decks::{self, Stagger};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const STAGGERS: &[(Stagger, &str, &str)] = &[
    (Stagger::Longest, "Longest pieces", "longest"),
    (Stagger::StairStep, "Stair-step", "stair_step"),
    (Stagger::Random, "Random", "random"),
];

impl Gui {
    /// A deck's rows; `false` when `id` is not a deck.
    pub(crate) fn deck_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(s) = decks::deck_settings(e) else {
            return false;
        };
        let mm = |v: f64| format!("{:.0}", v * 1000.0);
        grid.append(Property::category("Boards", "c_deck_boards"));
        let rows = [
            ("Board Width (mm)", "dk_boardWidth", mm(s.board_width)),
            (
                "Board Thickness (mm)",
                "dk_boardThickness",
                mm(s.board_thickness),
            ),
            ("Gap (mm)", "dk_gap", mm(s.gap)),
            (
                "Direction (°)",
                "dk_direction",
                s.direction.map_or_else(
                    || "Along longest edge".to_owned(),
                    |a| format!("{:.1}", a.to_degrees()),
                ),
            ),
            (
                "Stock Lengths (m)",
                "dk_stockLengths",
                s.stock_lengths
                    .iter()
                    .map(|l| format!("{l}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            (
                "Picture Frame Rows",
                "dk_pictureFrame",
                s.picture_frame.to_string(),
            ),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        grid.append(Property::boolean(
            "Balance Courses",
            "dk_balanceCourses",
            s.balance_courses,
        ));
        let index = STAGGERS
            .iter()
            .position(|x| x.0 == s.stagger)
            .map_or(0, |i| i as i32);
        grid.append(Property::enumeration(
            "Joint Stagger",
            "dk_stagger",
            STAGGERS
                .iter()
                .enumerate()
                .map(|(i, x)| (x.1.to_owned(), i as i32))
                .collect::<Vec<(String, i32)>>(),
            index,
        ));
        grid.append(Property::category("Framing", "c_deck_framing"));
        let rows = [
            ("Joist Spacing (mm)", "dk_joistSpacing", mm(s.joist_spacing)),
            ("Beam Spacing (mm)", "dk_beamSpacing", mm(s.beam_spacing)),
            ("Post Spacing (mm)", "dk_postSpacing", mm(s.post_spacing)),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        if let Ok(g) = decks::regenerate(&store, e) {
            let c = &g.cut_list;
            grid.append(Property::category("Cut List", "c_deck_cut"));
            let stock = c
                .stock
                .iter()
                .map(|(l, n)| format!("{n} × {l} m"))
                .collect::<Vec<_>>()
                .join(", ");
            let rows = [
                ("Boards", c.boards.to_string()),
                ("Board Length (m)", format!("{:.2}", c.board_length)),
                ("Stock to Buy", stock),
                ("Area (m²)", format!("{:.2}", c.area)),
                ("Joists", format!("{} ({:.1} m)", c.joists, c.joist_length)),
                ("Beams", format!("{} ({:.1} m)", c.beams, c.beam_length)),
                ("Posts and Footings", c.posts.to_string()),
                ("Screws", c.screws.to_string()),
            ];
            for (i, (label, value)) in rows.into_iter().enumerate() {
                let key = format!("dk_ro{i}");
                grid.append(Property::string(label, &key, value));
                grid.set_property_read_only(&key, true);
            }
            for (i, w) in c.warnings.iter().enumerate() {
                let key = format!("dk_warn{i}");
                grid.append(Property::string("Warning", &key, w.clone()));
                grid.set_property_read_only(&key, true);
            }
        }
        true
    }

    /// An edit of a deck row; `false` if `property` is not one.
    pub(crate) fn deck_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(field) = property.strip_prefix("dk_") else {
            return false;
        };
        let frame = self.frame;
        let Some(&deck) = self.selection().first() else {
            return true;
        };
        let text = text.unwrap_or_default();
        let mm = |t: &str| {
            t.trim()
                .trim_end_matches("mm")
                .trim()
                .parse::<f64>()
                .ok()
                .map(|v| v / 1000.0)
        };
        let value = match field {
            "boardWidth" | "boardThickness" | "gap" | "joistSpacing" | "beamSpacing"
            | "postSpacing" => mm(&text).map(|v| json!(v)),
            "direction" => Some(
                text.trim()
                    .trim_end_matches('°')
                    .parse::<f64>()
                    .map_or(json!(null), |d| json!(d.to_radians())),
            ),
            "stockLengths" => {
                let v: Option<Vec<f64>> = text
                    .split(',')
                    .map(|p| p.trim().trim_end_matches('m').trim().parse::<f64>().ok())
                    .collect();
                v.map(|v| json!(v))
            }
            "pictureFrame" => text.trim().parse::<u32>().ok().map(|v| json!(v)),
            "balanceCourses" => Some(json!(text.eq_ignore_ascii_case("true") || text == "1")),
            "stagger" => STAGGERS.iter().find(|x| x.1 == text).map(|x| json!(x.2)),
            _ => return false,
        };
        let Some(value) = value else {
            error(&frame, "That is not a valid value");
            return true;
        };
        let command = if field.ends_with("Spacing") {
            "deck.set_framing"
        } else {
            "deck.set_boards"
        };
        if let Err(e) = self.run(
            command,
            json!({ "deck_id": deck, "settings": { field: value } }),
        ) {
            error(&frame, &format!("The deck was not changed:\n{e}"));
        }
        self.refresh_scene(&frame);
        true
    }
}
