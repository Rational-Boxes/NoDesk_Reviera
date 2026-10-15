// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Grid tool (ARC-GRID-001) in plan views: click the start and the end of a grid line. Points
//! snap like the Wall tool's (ends and midpoints of grids and walls, cut edges, 15° directions
//! from the start); the preview shows the line with its bubble and the name it will get, in the
//! style of the grid type the tool places (chosen in Properties while the tool runs).

use reviera_core::ElementId;
use reviera_render::RenderBackend;
use reviera_render::annotation::{GridMark, GridStyle, grid_lines};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, snap};

const PREVIEW_COLOR: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

/// The Grid tool.
#[derive(Debug, Default)]
pub struct GridTool {
    start: Option<[f64; 2]>,
    cursor: Option<snap::Snap>,
    /// A typed length (ARC-DIM-002) and the direction it is measured in, frozen when typing
    /// began.
    pub(crate) typed: String,
    pub(crate) typed_dir: Option<[f64; 2]>,
    /// Lines to snap to: grids, Reviera walls and the plan's cut edges.
    lines: Vec<([f64; 2], [f64; 2])>,
}

impl Gui {
    /// The grid type the Grid tool places: the one last chosen, else the default (`None` until
    /// the project has grid types; the first grid creates the default).
    pub(crate) fn tool_grid_type(&self) -> Option<ElementId> {
        let store = self.app.document(self.doc?)?.doc.store();
        let types = reviera_core::grids::types(store);
        self.grid_type
            .filter(|t| types.iter().any(|i| i.id == *t))
            .or_else(|| {
                types
                    .iter()
                    .find(|t| t.name == reviera_core::grids::DEFAULT_TYPE_NAME)
                    .or(types.first())
                    .map(|t| t.id)
            })
    }
}

impl Gui {
    /// Starts the Grid tool (plans only).
    pub(crate) fn start_grid_tool(&mut self) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame
                .set_status_text("Grids are drawn in plan views (View › New Floor Plan)", 0);
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        self.grid_tool = Some(GridTool::default());
        self.update_grid_snaps();
        self.clear_selection();
        self.properties_stale = true;
        self.grid_prompt();
    }

    pub(crate) fn end_grid_tool(&mut self) -> bool {
        if self.grid_tool.take().is_none() {
            return false;
        }
        self.properties_stale = true;
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Ready", 0);
        true
    }

    /// Esc: drops the start point, then ends the tool.
    pub(crate) fn grid_escape(&mut self) {
        match &mut self.grid_tool {
            Some(t) if t.start.is_some() => {
                t.start = None;
                self.grid_preview();
                self.grid_prompt();
            }
            Some(_) => {
                self.end_grid_tool();
            }
            None => {}
        }
    }

    fn update_grid_snaps(&mut self) {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let mut lines: Vec<([f64; 2], [f64; 2])> = reviera_core::grids::list(&store)
            .into_iter()
            .map(|g| (g.curve.start, g.curve.end))
            .collect();
        lines.extend(
            store
                .iter()
                .filter_map(reviera_core::walls::wall_recipe)
                .map(|r| (r.curve.start, r.curve.end)),
        );
        if let Some(plan) = self.active_view().and_then(|v| v.plan.as_ref()) {
            let p = |q: [f32; 2]| [f64::from(q[0]), f64::from(q[1])];
            lines.extend(plan.cut_edges.iter().map(|(_, s)| (p(s[0]), p(s[1]))));
        }
        if let Some(t) = &mut self.grid_tool {
            t.lines = lines;
        }
    }

    fn grid_prompt(&mut self) {
        let Some(t) = &self.grid_tool else { return };
        let mut text = if t.start.is_none() {
            "Grid: click the start point".to_owned()
        } else {
            "Click the end point (the bubble goes at the end). Esc restarts.".to_owned()
        };
        if let Some(c) = t.cursor
            && c.kind != snap::SnapKind::None
        {
            text += &format!(" · {}", c.kind.label());
        }
        self.frame.set_status_text(&text, 0);
    }

    /// The snapped plan point under pixel (`x`, `y`).
    fn grid_snapped(&self, x: i32, y: i32) -> Option<snap::Snap> {
        let t = self.grid_tool.as_ref()?;
        let v = self.active_view()?;
        let z = v.plan.as_ref()?.range.cut as f32;
        let raw = self.plan_xy(x, y)?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let to_screen = |p: [f64; 2]| {
            v.camera
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let mut s = snap::snap(raw, (x as f32, y as f32), t.start, &t.lines, to_screen);
        if s.kind == snap::SnapKind::None {
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    pub(crate) fn grid_motion(&mut self, x: i32, y: i32) {
        let cursor = self.grid_snapped(x, y);
        if let Some(t) = &mut self.grid_tool {
            t.cursor = cursor;
        }
        self.grid_preview();
        self.grid_prompt();
    }

    pub(crate) fn grid_click(&mut self, x: i32, y: i32) {
        let Some(p) = self.grid_snapped(x, y) else {
            return;
        };
        self.grid_click_at(p.point);
    }

    /// The start and the current cursor point of the grid being drawn.
    pub(crate) fn grid_ends(&self) -> Option<([f64; 2], [f64; 2])> {
        let t = self.grid_tool.as_ref()?;
        Some((t.start?, t.cursor?.point))
    }

    pub(crate) fn grid_click_at(&mut self, point: [f64; 2]) {
        let Some(t) = &mut self.grid_tool else { return };
        t.typed.clear();
        t.typed_dir = None;
        let Some(start) = t.start else {
            t.start = Some(point);
            self.grid_preview();
            return self.grid_prompt();
        };
        let args = json!({
            "curve": { "line": { "start": start, "end": point } },
            "type_id": self.tool_grid_type(),
        });
        match self.run("grid.create", args) {
            Ok(_) => {
                if let Some(t) = &mut self.grid_tool {
                    t.start = None;
                }
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.update_grid_snaps();
                self.grid_preview();
                self.grid_prompt();
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
    }

    /// The snap marker and the grid being drawn, with the name it will get.
    pub(crate) fn grid_preview(&mut self) {
        let Some(t) = &self.grid_tool else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.003;
        let mut lines = Lines {
            color: PREVIEW_COLOR,
            width: 0.25,
            segments: Vec::new(),
        };
        if let Some(c) = t.cursor {
            // A small square, 1.5 mm on paper.
            let r = 0.75 * plan.scale / 1000.0;
            let [x, y] = c.point.map(|v| v as f32);
            let corners = [
                [x - r, y - r],
                [x + r, y - r],
                [x + r, y + r],
                [x - r, y + r],
            ];
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                lines.segments.push([[a[0], a[1], z], [b[0], b[1], z]]);
            }
        }
        if let (Some(start), Some(c)) = (t.start, t.cursor) {
            let Some(store) = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store().clone())
            else {
                return;
            };
            let t = self
                .tool_grid_type()
                .and_then(|t| store.get(t))
                .and_then(reviera_core::grids::grid_type)
                .unwrap_or_default();
            let pens = reviera_core::standards::pen_set(&store);
            let mark = GridMark {
                start,
                end: c.point,
                name: reviera_core::grids::auto_name(&store),
                bubbles: t.bubbles,
                style: GridStyle::of(&t, &|p| pens.annotation_width(p) as f32),
                elbows: [None, None],
            };
            // The type's patterns and bubbles, in the preview colour.
            for l in grid_lines(&mark, plan.scale, z) {
                lines.segments.extend(l.segments);
            }
        }
        let mut all = vec![lines];
        all.extend(self.typed_overlay(z, plan.scale));
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&all);
        }
    }
}

/// The last entry of the grid Type drop-down.
const DUPLICATE: &str = "Duplicate Type…";
const SYMBOLS: [&str; 2] = ["Bubble", "None"];
const CENTERS: [&str; 3] = ["Continuous", "None", "Custom"];

/// Drop-down choices, valued by position.
fn choices<S: AsRef<str>>(labels: &[S]) -> Vec<(String, i32)> {
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_ref().to_owned(), i as i32))
        .collect()
}

/// A choice label to the value `type.set_param` takes (`Dash dot` → `dash_dot`).
fn key(label: &str) -> String {
    label.trim().to_ascii_lowercase().replace(' ', "_")
}

/// A value to its choice label (`dash_dot` → `Dash dot`).
fn label(key: &str) -> String {
    let mut s = key.replace('_', " ");
    if let Some(f) = s.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    s
}

impl Gui {
    /// The grids among the selection.
    fn selected_grids(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.doc.and_then(|d| self.app.document(d)) else {
            return Vec::new();
        };
        let store = store.doc.store();
        ids.into_iter()
            .filter(|id| store.get(*id).is_some_and(reviera_core::grids::is_grid))
            .collect()
    }

    /// The palette rows of a grid type: the Type drop-down (`current` selected) and the type's
    /// values, applying to the selected grids or, with none, to the Grid tool.
    pub(crate) fn grid_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>) {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let types = reviera_core::grids::types(&store);
        let mut names: Vec<String> = types.iter().map(|t| t.name.clone()).collect();
        let index = current
            .and_then(|c| types.iter().position(|t| t.id == c))
            .map_or(-1, |i| i as i32);
        if current.is_some() {
            names.push(DUPLICATE.to_owned());
        }
        grid.append(Property::enumeration(
            "Type",
            "gt_type",
            choices(&names),
            index,
        ));
        grid.set_help_string(
            "gt_type",
            "The grid type; Duplicate Type… copies it under a new name",
        );
        let Some(info) = current.and_then(|c| types.into_iter().find(|t| t.id == c)) else {
            return;
        };
        let t = info.values;
        grid.append(Property::category(
            format!("Type Parameters ({} grids)", info.instances),
            "c_gridtype",
        ));
        grid.append(Property::string("Type Name", "gt_name", info.name));
        let pos = |list: &[&str], v: &str| {
            list.iter()
                .position(|l| key(l) == v)
                .map_or(0, |i| i as i32)
        };
        let symbol = serde_json::to_value(t.symbol)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let center = serde_json::to_value(t.center_segment)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        grid.append(Property::enumeration(
            "Symbol",
            "gt:symbol",
            choices(&SYMBOLS),
            pos(&SYMBOLS, &symbol),
        ));
        grid.append(Property::enumeration(
            "Center Segment",
            "gt:center_segment",
            choices(&CENTERS),
            pos(&CENTERS, &center),
        ));
        let patterns: Vec<String> = reviera_core::grids::PATTERNS
            .iter()
            .map(|p| label(p))
            .collect();
        let pattern_index = |p: &str| {
            reviera_core::grids::PATTERNS
                .iter()
                .position(|q| *q == p)
                .map_or(0, |i| i as i32)
        };
        for (prefix, title, s) in [("center", "Center", &t.center), ("end", "End", &t.end)] {
            grid.append(Property::int(
                format!("{title} Weight"),
                format!("gt:{prefix}_pen"),
                i64::from(s.pen),
            ));
            grid.append(Property::string(
                format!("{title} Colour"),
                format!("gt:{prefix}_colour"),
                s.colour.clone(),
            ));
            grid.append(Property::enumeration(
                format!("{title} Pattern"),
                format!("gt:{prefix}_pattern"),
                choices(&patterns),
                pattern_index(&s.pattern),
            ));
        }
        grid.set_help_string(
            "gt:center_pen",
            "Annotation pen 1-16, for a Custom centre segment",
        );
        grid.set_help_string(
            "gt:end_pen",
            "Annotation pen 1-16; the whole line when Continuous",
        );
        grid.append(Property::string(
            "End Segments Length",
            "gt:end_length",
            format!("{} mm", t.end_length_mm),
        ));
        grid.set_help_string("gt:end_length", "Length of each end segment on paper");
        grid.append(Property::boolean(
            "Bubble at Start",
            "gt:bubble_start",
            t.bubbles[0],
        ));
        grid.append(Property::boolean(
            "Bubble at End",
            "gt:bubble_end",
            t.bubbles[1],
        ));
        self.type_rows = Some(info.id);
    }

    /// The palette while the Grid tool runs with nothing selected.
    pub(crate) fn grid_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.grid_tool.is_none() {
            return false;
        }
        grid.append(Property::category("Grid tool", "c_gridtool"));
        let current = self.tool_grid_type();
        self.grid_type_rows(grid, current);
        true
    }

    /// An edit of a grid type row; `false` if `property` is not one.
    pub(crate) fn grid_type_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let current = self.type_rows;
        let result = if property == "gt_type" {
            self.choose_grid_type(&text, current)
        } else if property == "gt_name" {
            current.map(|t| {
                self.run("type.rename", json!({ "type_id": t, "name": text }))
                    .map(|_| ())
            })
        } else if let Some(param) = property.strip_prefix("gt:") {
            let value =
                if param.ends_with("_pattern") || param == "symbol" || param == "center_segment" {
                    json!(key(&text))
                } else {
                    json!(text)
                };
            current.map(|t| {
                self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": param, "value": value }),
                )
                .map(|_| ())
            })
        } else {
            return false;
        };
        if let Some(Err(e)) = result {
            super::error(&frame, &format!("The grid type was not changed:\n{e}"));
        }
        self.refresh_scene(&frame);
        self.grid_preview();
        true
    }

    /// The Type drop-down: another type for the selected grids (or the tool), or a duplicate of
    /// the current one.
    fn choose_grid_type(
        &mut self,
        choice: &str,
        current: Option<ElementId>,
    ) -> Option<Result<(), String>> {
        let frame = self.frame;
        let grids = self.selected_grids();
        let chosen = if choice == DUPLICATE {
            let current = current?;
            let proposed = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| reviera_core::types::duplicate_name(d.doc.store(), current))
                .unwrap_or_default();
            let dialog =
                TextEntryDialog::builder(&frame, "Name of the new grid type:", "Duplicate Type")
                    .with_default_value(&proposed)
                    .build();
            if dialog.show_modal() != ID_OK {
                return None;
            }
            let name = dialog.get_value()?;
            match self.run(
                "type.duplicate",
                json!({ "type_id": current, "new_name": name }),
            ) {
                Ok(v) => ElementId(v.as_u64().unwrap_or_default()),
                Err(e) => return Some(Err(e)),
            }
        } else {
            let store = self.app.document(self.doc?)?.doc.store();
            reviera_core::grids::types(store)
                .into_iter()
                .find(|t| t.name == choice)?
                .id
        };
        if grids.is_empty() {
            self.grid_type = Some(chosen);
            self.properties_stale = true;
            return Some(Ok(()));
        }
        Some(
            self.run(
                "element.set_type",
                json!({ "element_ids": grids, "type_id": chosen }),
            )
            .map(|_| ()),
        )
    }
}
