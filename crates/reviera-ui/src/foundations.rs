// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Foundations in the window (STR-FND-001 … 003): Structure › Foundation › Isolated (click in a
//! plan; the footing's top at the plan's level), At Columns and Wall (for the selected columns
//! or walls), and Slab (a boundary sketch, finished as a foundation slab).

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

/// The Isolated Footing tool (or the Pile or Pile Cap tool): its level and what it places.
#[derive(Debug, Clone)]
pub struct FootingTool {
    level: ElementId,
    command: &'static str,
}

impl Gui {
    /// Structure › Foundation › Isolated (`footing.place`), Pile (`pile.place`) or Pile Cap
    /// (`pile_cap.place`).
    pub(crate) fn start_footing_tool(&mut self, command: &'static str) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Footings are placed in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.footing_tool = Some(FootingTool { level, command });
        frame.set_status_text(
            match command {
                "pile.place" => "Pile: click its centre (it snaps to grid crossings); its top is at this level. Esc ends the tool.",
                "pile_cap.place" => "Pile Cap: click its centre; its top is at this level, its piles under it. Esc ends the tool.",
                _ => "Isolated Footing: click its centre (it snaps to grid crossings); its top is at this level. Esc ends the tool.",
            },
            0,
        );
    }

    pub(crate) fn end_footing_tool(&mut self) -> bool {
        self.footing_tool.take().is_some()
    }

    pub(crate) fn footing_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.footing_tool.clone() else {
            return;
        };
        let lines = self.snap_lines();
        let Some(p) = self.snapped_at(x, y, None, &lines).map(|s| s.point) else {
            return;
        };
        match self.run(t.command, json!({ "point": p, "level": t.level })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Placed; click the next, or Esc", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    fn selected_of(&mut self, keep: fn(&reviera_core::Element) -> bool) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).is_some_and(keep))
            .collect()
    }

    /// Steel › Connection: a generic connection at the join of the selected members.
    pub(crate) fn connect_members(&mut self) {
        let frame = self.frame;
        let members = self.selected_of(|e| {
            reviera_core::beams::beam_recipe(e).is_some()
                || reviera_core::columns::column_recipe(e).is_some()
        });
        if members.is_empty() {
            return frame.set_status_text(
                "Select the members that meet (Ctrl+click or a window), main member first, then Connection; or columns for base plates",
                0,
            );
        }
        // The types that fit: generic for two or more members; a library type by its inputs.
        let lib = self.run("connection_type.list", json!({})).unwrap_or_default();
        let all_columns = self.store_now().is_some_and(|s| {
            members
                .iter()
                .all(|m| s.get(*m).and_then(reviera_core::columns::column_recipe).is_some())
        });
        let mut choices: Vec<(String, String)> = Vec::new();
        if members.len() >= 2 {
            choices.push(("generic".into(), "Generic (a symbol and its information)".into()));
        }
        for t in lib.as_array().into_iter().flatten() {
            let inputs = t["inputs"].as_array().map_or(0, Vec::len);
            let fits = inputs == members.len() || (inputs == 1 && all_columns);
            if fits {
                choices.push((
                    t["id"].as_str().unwrap_or_default().to_owned(),
                    format!("{} — {}", t["name"].as_str().unwrap_or_default(), t["group"].as_str().unwrap_or_default()),
                ));
            }
        }
        // The project's custom types with as many members (they check the join when placed).
        let custom = self.run("custom_connection.list", json!({})).unwrap_or_default();
        for t in custom.as_array().into_iter().flatten() {
            if t["members"].as_array().map_or(0, Vec::len) == members.len() {
                choices.push((
                    t["id"].as_str().unwrap_or_default().to_owned(),
                    format!("{} — Custom", t["name"].as_str().unwrap_or_default()),
                ));
            }
        }
        if choices.is_empty() {
            return frame.set_status_text("No connection type takes that selection", 0);
        }
        let labels: Vec<&str> = choices.iter().map(|c| c.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Connection type:", "Connection", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((ty, _)) = usize::try_from(d.get_selection()).ok().and_then(|i| choices.get(i)) else {
            return;
        };
        // A one-member type (a base plate) goes on each selected column.
        let groups: Vec<Vec<ElementId>> = if all_columns
            && lib.as_array().into_iter().flatten().any(|t| t["id"] == ty.as_str() && t["inputs"].as_array().map_or(0, Vec::len) == 1)
        {
            members.iter().map(|m| vec![*m]).collect()
        } else {
            vec![members.clone()]
        };
        let (mut placed, mut had) = (0, 0);
        for g in groups {
            match self.run(
                "structural_connection.create",
                json!({ "member_ids": g, "type_id": ty }),
            ) {
                Ok(_) => placed += 1,
                Err(e) if e.contains("connection.exists") => had += 1,
                Err(e) => {
                    error(&frame, &format!("The connection was not placed:\n{e}"));
                    break;
                }
            }
        }
        let already = if had > 0 {
            format!(" ({had} already had one)")
        } else {
            String::new()
        };
        if placed == 0 && had > 0 {
            frame.set_status_text(&format!("No connection placed: the members already have one of that type{already}"), 0);
        }
        if placed > 0 {
            frame.set_status_text(
                &format!(
                    "{placed} connection(s) placed{already}; their plates, bolts and welds show in views at Fine detail"
                ),
                0,
            );
            self.clear_selection();
            self.refresh_scene(&frame);
        }
    }

    /// Structure › Foundation › At Columns: a footing under each selected column.
    pub(crate) fn footings_at_columns(&mut self) {
        let frame = self.frame;
        let cols = self.selected_of(|e| reviera_core::columns::column_recipe(e).is_some());
        if cols.is_empty() {
            return frame.set_status_text("Select the columns first, then At Columns", 0);
        }
        match self.run("footing.place_at_columns", json!({ "column_ids": cols })) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} footings placed under columns"), 0);
                self.refresh_scene(&frame);
            }
            Err(e) => error(&frame, &format!("The footings were not placed:\n{e}")),
        }
    }

    /// Structure › Foundation › Wall: a wall foundation under each selected wall.
    pub(crate) fn wall_foundations(&mut self) {
        let frame = self.frame;
        let walls = self.selected_of(|e| reviera_core::walls::wall_recipe(e).is_some());
        if walls.is_empty() {
            return frame.set_status_text("Select the walls first, then Wall Foundation", 0);
        }
        match self.run("wall_foundation.create", json!({ "wall_ids": walls })) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} wall foundations placed"), 0);
                self.refresh_scene(&frame);
            }
            Err(e) => error(
                &frame,
                &format!("The wall foundations were not placed:\n{e}"),
            ),
        }
    }

    /// Structure › Floor: sketch the boundary; Finish makes a structural floor spanning along its
    /// first edge.
    pub(crate) fn start_structural_floor_sketch(&mut self) {
        self.start_floor_sketch();
        if let Some(s) = &mut self.sketch {
            s.structural = true;
            self.frame.set_status_text(
                "Structural Floor: sketch the boundary; it spans along the first edge you draw. Finish to make it.",
                0,
            );
        }
    }

    /// Structure › Foundation › Slab: sketch the boundary; Finish makes a foundation slab.
    pub(crate) fn start_foundation_slab_sketch(&mut self) {
        self.start_floor_sketch();
        if let Some(s) = &mut self.sketch {
            s.foundation = true;
            self.frame.set_status_text(
                "Foundation Slab: sketch the boundary; its top is at this level, its thickness below. Finish to make it.",
                0,
            );
        }
    }

    /// Analyze › Analytical › Panel by Boundary: sketch the boundary in a plan; Finish makes a
    /// planar analytical panel at the plan's level.
    pub(crate) fn start_analytical_panel_sketch(&mut self) {
        self.start_floor_sketch();
        if let Some(s) = &mut self.sketch {
            s.analytical_panel = true;
            self.frame.set_status_text(
                "Analytical Panel: sketch the boundary at this level; its corners become nodes, shared with members there. Finish to make it.",
                0,
            );
        }
    }
}
