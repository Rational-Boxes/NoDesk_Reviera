// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rooms in the window (Ch 3 §3.9.1, ARC-ROOM-001):
//! - the Room tool: hovering shows the enclosed region under the cursor; a click places a room,
//!   and Place Rooms Automatically fills every enclosed region of the level;
//! - room labels in plans (Name, Number and Area at the room's tag point, interim until room tag
//!   elements exist);
//! - picking a room by its label or its interior;
//! - the room's Properties rows.

use reviera_core::ElementId;
use reviera_core::rooms;
use reviera_core::walls::Line;
use reviera_render::RenderBackend;
use reviera_render::annotation::{Label, label_strokes, labels_extent, room_tag_labels};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

/// Room labels and the selected room's boundary.
const ROOM_COLOR: [f32; 4] = [0.15, 0.15, 0.15, 1.0];
const HOVER: [f32; 4] = [0.95, 0.45, 0.9, 1.0];

/// The Room tool: the region under the cursor.
#[derive(Debug, Default)]
pub struct RoomTool {
    hover: Vec<Line>,
}

impl Gui {
    /// Architecture › Room.
    pub(crate) fn start_room_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return frame.set_status_text("Rooms are placed in plan views (View › Floor Plan)", 0);
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.cancel_sketch();
        self.end_ceiling_tool();
        self.leave_walk();
        self.room_tool = Some(RoomTool::default());
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        frame.set_status_text(
            "Room: click inside a region enclosed by walls (or Place Rooms Automatically); Esc ends",
            0,
        );
    }

    pub(crate) fn end_room_tool(&mut self) -> bool {
        if self.room_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.update_ribbon_context();
        true
    }

    /// The region of wall faces under pixel (`x`, `y`) on the plan's level.
    fn room_region(&self, x: i32, y: i32) -> Option<Vec<Line>> {
        let plan = self.active_view()?.plan.clone()?;
        let p = self.plan_xy(x, y)?;
        let store = self.app.document(self.doc?)?.doc.store();
        let edges = reviera_core::arrangement::wall_edges(store, plan.elevation + 0.001);
        let r = reviera_core::arrangement::region_around(&edges, p)?;
        Some(
            std::iter::once(&r.outer)
                .chain(&r.holes)
                .flatten()
                .map(|e| e.line)
                .collect(),
        )
    }

    pub(crate) fn room_motion(&mut self, x: i32, y: i32) {
        let hover = self.room_region(x, y).unwrap_or_default();
        if let Some(t) = &mut self.room_tool {
            t.hover = hover;
        }
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let segments = self
            .room_tool
            .as_ref()
            .map(|t| {
                t.hover
                    .iter()
                    .map(|l| {
                        [
                            [l.start[0] as f32, l.start[1] as f32, z],
                            [l.end[0] as f32, l.end[1] as f32, z],
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[Lines {
                color: HOVER,
                width: 0.5,
                segments,
            }]);
        }
    }

    pub(crate) fn room_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let (Some(plan), Some(p)) = (
            self.active_view().and_then(|v| v.plan.clone()),
            self.plan_xy(x, y),
        ) else {
            return;
        };
        match self.app.execute(
            "room.place",
            json!({ "level_id": plan.level, "point": p }),
            &super::user(),
        ) {
            Ok(out) => {
                let note = out.warnings.first().map_or(
                    "Room placed; click another region, or Esc".to_owned(),
                    |w| format!("Room placed — {}", w.message),
                );
                frame.set_status_text(&note, 0);
                self.after_room_change();
            }
            Err(e) => frame.set_status_text(&e.message, 0),
        }
    }

    /// Place Rooms Automatically on the plan's level.
    pub(crate) fn place_rooms_automatically(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return frame.set_status_text("Rooms are placed in plan views", 0);
        };
        match self.run(
            "room.place_automatically",
            json!({ "level_id": plan.level }),
        ) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(
                    &format!("{n} room{} placed", if n == 1 { "" } else { "s" }),
                    0,
                );
                self.after_room_change();
            }
            Err(e) => error(&frame, &format!("Rooms were not placed:\n{e}")),
        }
    }

    fn after_room_change(&mut self) {
        let frame = self.frame;
        self.refresh_scene(&frame);
        if let Some(doc) = self.doc {
            self.plans_redraw(doc);
        }
    }

    /// The labels of the rooms on a plan's level, with the room each belongs to.
    fn room_labels(&mut self, index: usize) -> Vec<(ElementId, Vec<Label>, rooms::RoomInfo)> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone() else {
            return Vec::new();
        };
        if plan.ceiling {
            return Vec::new();
        }
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let Some(level) = store.get(plan.level).map(|l| l.guid().to_string()) else {
            return Vec::new();
        };
        let z = plan.range.cut as f32 + 0.003;
        // A room tagged in the view shows its tag instead of the interim label.
        let tagged = v
            .element
            .and_then(|e| store.get(e))
            .map(|view| reviera_app::notes::tagged_rooms(&store, view))
            .unwrap_or_default();
        let mut out = Vec::new();
        for r in rooms::all(&store)
            .into_iter()
            .filter(|r| r.level == level)
            .filter(|r| {
                store
                    .get(r.id)
                    .is_none_or(|e| !tagged.contains(e.guid().as_str()))
            })
        {
            let area = if r.geometry.enclosed() {
                self.run(
                    "units.format",
                    json!({ "value": r.geometry.area(), "data_type": "area" }),
                )
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:.2} m²", r.geometry.area()))
            } else {
                "Not Enclosed".to_owned()
            };
            // The stroke font has capitals only (and the m of units).
            let lines = vec![r.name.to_uppercase(), r.number.to_uppercase(), area];
            let labels = room_tag_labels(r.geometry.tag_point, &lines, plan.scale, z);
            out.push((r.id, labels, r));
        }
        out
    }

    /// Room labels and the selected rooms' boundaries, for a plan.
    pub(crate) fn room_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let z = self.views[index]
            .plan
            .as_ref()
            .map_or(0.0, |p| p.range.cut as f32 + 0.003);
        let mut normal = Lines {
            color: ROOM_COLOR,
            width: 0.18,
            segments: Vec::new(),
        };
        let mut lit = Lines {
            color: reviera_render::HIGHLIGHT,
            width: 0.35,
            segments: Vec::new(),
        };
        for (id, labels, r) in self.room_labels(index) {
            let target = if selected.contains(&id) {
                &mut lit
            } else {
                &mut normal
            };
            target.segments.extend(label_strokes(&labels));
            if selected.contains(&id)
                && let Some(region) = &r.geometry.region
            {
                for e in std::iter::once(&region.outer)
                    .chain(&region.holes)
                    .flatten()
                {
                    lit.segments.push([
                        [e.line.start[0] as f32, e.line.start[1] as f32, z],
                        [e.line.end[0] as f32, e.line.end[1] as f32, z],
                    ]);
                }
            }
        }
        vec![normal, lit]
    }

    /// The room whose label is under pixel (`x`, `y`) in the active plan.
    pub(crate) fn room_label_at(&mut self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let p = self.plan_xy(x, y)?;
        let scale = self.views[index].plan.as_ref()?.scale;
        let pad = 1.0 * scale / 1000.0;
        self.room_labels(index)
            .into_iter()
            .find_map(|(id, labels, _)| {
                let (lo, hi) = labels_extent(&labels)?;
                let (px, py) = (p[0] as f32, p[1] as f32);
                (px >= lo[0] - pad && px <= hi[0] + pad && py >= lo[1] - pad && py <= hi[1] + pad)
                    .then_some(id)
            })
    }

    /// The room whose region contains pixel (`x`, `y`) in the active plan.
    pub(crate) fn room_inside(&self, x: i32, y: i32) -> Option<ElementId> {
        let plan = self.active_view()?.plan.as_ref()?;
        if plan.ceiling {
            return None;
        }
        let p = self.plan_xy(x, y)?;
        let store = self.app.document(self.doc?)?.doc.store();
        rooms::room_at(store, plan.level, p)
    }

    /// Properties of a selected room.
    pub(crate) fn room_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let Some(recipe) = rooms::room_recipe(e) else {
            return;
        };
        let (_, name) = rooms::number_and_name(e);
        let level_name = |guid: &str| {
            reviera_core::GlobalId::parse(guid)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| store.get(l)?.name.clone())
                .unwrap_or_default()
        };
        // Number is the element's name, shown with the identity rows above.
        grid.append(Property::category("Room", "c_room_id"));
        grid.append(Property::string("Name", "rm_name", name));
        grid.append(Property::category("Constraints", "c_room"));
        grid.append(Property::string(
            "Level",
            "rm_level",
            level_name(&recipe.level),
        ));
        grid.set_property_read_only("rm_level", true);
        let upper = level_name(recipe.upper_level.as_deref().unwrap_or(&recipe.level));
        grid.append(Property::string("Upper Limit", "rm_upper", upper));
        grid.set_property_read_only("rm_upper", true);
        let limit = self.format_length(recipe.limit_offset);
        grid.append(Property::string("Limit Offset", "rm_limit", limit));
        let base = self.format_length(recipe.base_offset);
        grid.append(Property::string("Base Offset", "rm_base", base));
        grid.append(Property::category("Dimensions", "c_room_dims"));
        let Ok(g) = rooms::regenerate(&store, e) else {
            return;
        };
        let fmt = |gui: &mut Gui, v: f64, t: &str| {
            gui.run("units.format", json!({ "value": v, "data_type": t }))
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{v:.3}"))
        };
        let redundant = rooms::all(&store).iter().any(|r| r.id == id && r.redundant);
        let rows = if g.enclosed() {
            vec![
                ("Area", fmt(self, g.area(), "area")),
                ("Perimeter", self.format_length(g.perimeter())),
                ("Unbounded Height", self.format_length(g.unbounded_height())),
                ("Volume", fmt(self, g.volume(), "volume")),
                (
                    "Status",
                    if redundant { "Redundant" } else { "Placed" }.to_owned(),
                ),
            ]
        } else {
            vec![
                ("Area", "Not Enclosed".to_owned()),
                ("Unbounded Height", self.format_length(g.unbounded_height())),
                ("Status", "Not Enclosed".to_owned()),
            ]
        };
        for (k, (label, value)) in rows.into_iter().enumerate() {
            let key = format!("rm_q{k}");
            grid.append(Property::string(label, &key, value));
            grid.set_property_read_only(&key, true);
        }
    }

    /// An edit of a room row; `false` if `property` is not one.
    pub(crate) fn room_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected: Vec<ElementId> = {
            let store = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store().clone());
            self.selection()
                .into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(rooms::room_recipe)
                        .is_some()
                })
                .collect()
        };
        let key = match property {
            "rm_number" => "number",
            "rm_name" => "name",
            "rm_limit" => "limit_offset",
            "rm_base" => "base_offset",
            _ => return false,
        };
        let mut args = json!({ "room_ids": selected });
        args[key] = json!(text);
        if let Err(e) = self.run("room.set", args) {
            error(&frame, &format!("The room was not changed:\n{e}"));
        }
        self.properties_stale = true;
        self.after_room_change();
        true
    }
}
