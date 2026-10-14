// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Openings in the window: Architecture › Opening › Wall, Vertical and Shaft (ARC-OPEN-002,
//! ARC-OPEN-003, ARC-OPEN-004), and the Properties of a selected wall opening or shaft.
//!
//! - **Wall** runs as a mode of the Door and Window tool: hovering a Reviera wall previews a
//!   rectangular opening of the tool's width and height centred on the cursor, kept inside the
//!   wall; a click cuts it, and the tool repeats until Esc.
//! - **Vertical** opens a sketch on the plan's level; Finish cuts the floor (in a ceiling plan,
//!   the ceiling) on that level that the loops overlap most, else a roof.
//! - **Shaft** opens a sketch on the plan's level; Finish makes a shaft from that level to the
//!   next one up.

use reviera_core::ElementId;
use reviera_core::wall_openings::{self, Resolved};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};
use crate::sketch::{DrawMode, SketchKind};

const PREVIEW_LINES: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
const UNCONNECTED: &str = "Unconnected";

/// The Wall Opening mode of the insert tool: the size it cuts and what it would cut now.
#[derive(Debug, Clone, PartialEq)]
pub struct WallOpeningMode {
    pub width: f64,
    pub height: f64,
    /// The wall under the cursor and the span along it.
    pub hover: Option<(ElementId, [f64; 2], Vec<[f64; 2]>)>,
}

impl Default for WallOpeningMode {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: wall_openings::DEFAULT_HEIGHT,
            hover: None,
        }
    }
}

impl Gui {
    /// Architecture › Opening › Wall.
    pub(crate) fn start_wall_opening_tool(&mut self) {
        let mode = self
            .insert_tool
            .as_ref()
            .and_then(|t| t.wall_opening.clone())
            .unwrap_or_default();
        self.start_insert_tool(reviera_core::inserts::Kind::Door);
        if let Some(t) = &mut self.insert_tool {
            t.wall_opening = Some(WallOpeningMode {
                hover: None,
                ..mode
            });
        }
        self.wall_opening_prompt();
    }

    /// Architecture › Opening › Vertical: a sketch on the plan's level.
    pub(crate) fn start_vertical_opening(&mut self) {
        self.start_opening_sketch(SketchKind::Opening);
    }

    /// Architecture › Opening › Shaft: a sketch on the plan's level.
    pub(crate) fn start_shaft(&mut self) {
        self.start_opening_sketch(SketchKind::Shaft);
    }

    fn start_opening_sketch(&mut self, kind: SketchKind) {
        let frame = self.frame;
        let ceiling = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .is_some_and(|p| p.ceiling);
        if !self.ensure_plan(ceiling) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Openings are sketched in plan views", 0);
        };
        self.begin_sketch(kind, None, level, Vec::new());
        self.set_draw_mode(DrawMode::Rectangle);
    }

    pub(crate) fn wall_opening_prompt(&mut self) {
        let Some(m) = self
            .insert_tool
            .as_ref()
            .and_then(|t| t.wall_opening.clone())
        else {
            return;
        };
        let size = format!(
            "{} × {}",
            self.format_length(m.width),
            self.format_length(m.height)
        );
        let text = match &m.hover {
            None => format!("Wall Opening ({size}): hover a wall"),
            Some((_, u, _)) => format!(
                "Click to cut a {size} opening, {} from the wall's start. Esc ends the tool.",
                self.format_length(u[0])
            ),
        };
        self.frame.set_status_text(&text, 0);
    }

    /// The span the Wall Opening tool would cut at pixel (`x`, `y`): centred on the cursor along
    /// the wall, slid to stay inside it; with its plan outline.
    fn wall_opening_candidate(
        &self,
        x: i32,
        y: i32,
    ) -> Option<(ElementId, [f64; 2], Vec<[f64; 2]>)> {
        let m = self.insert_tool.as_ref()?.wall_opening.as_ref()?;
        let (host, point, _) = self.wall_under(x, y)?;
        let store = self.store_now()?;
        let w = store.get(host)?;
        let r = reviera_core::walls::wall_recipe(w)?;
        let wall = reviera_core::walls::regenerate(&store, w).ok()?;
        let base = reviera_core::levels::elevation_of(&store, &r.base_level).ok()?;
        let probe = Resolved {
            id: ElementId(0),
            host,
            wall,
            path: reviera_core::walls::curved::path_of(&r),
            recipe: wall_openings::WallOpeningRecipe {
                kind: wall_openings::WALL_OPENING_RECIPE.into(),
                host: w.guid().to_string(),
                u: [0.0, m.width],
                extent: reviera_core::levels::Extent {
                    base_level: r.base_level.clone(),
                    base_offset: 0.0,
                    top: json!({ "unconnected": m.height }),
                    top_offset: 0.0,
                },
            },
            z: [base, base + m.height],
        };
        let span = probe.span();
        if m.width > span {
            return None;
        }
        let half = m.width / 2.0;
        let mid = probe.along_of(point).clamp(half, span - half);
        let u = [
            ((mid - half) * 1000.0).round() / 1000.0,
            ((mid - half) * 1000.0).round() / 1000.0 + m.width,
        ];
        let mut shown = probe;
        shown.recipe.u = u;
        Some((host, u, shown.outline()))
    }

    pub(crate) fn wall_opening_motion(&mut self, x: i32, y: i32) {
        let hover = self.wall_opening_candidate(x, y);
        if let Some(m) = self
            .insert_tool
            .as_mut()
            .and_then(|t| t.wall_opening.as_mut())
        {
            m.hover = hover.clone();
        }
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |p| p.range.cut) as f32
            + 0.004;
        let mut lines = Lines {
            color: PREVIEW_LINES,
            width: 0.5,
            segments: Vec::new(),
        };
        if let Some((_, _, outline)) = &hover {
            let n = outline.len();
            for i in 0..n {
                let (a, b) = (outline[i], outline[(i + 1) % n]);
                lines
                    .segments
                    .push([[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]);
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay(&[]);
            v.set_overlay_lines(&[lines]);
        }
        self.wall_opening_prompt();
    }

    pub(crate) fn wall_opening_click(&mut self, x: i32, y: i32) {
        let Some((host, u, _)) = self.wall_opening_candidate(x, y) else {
            return self
                .frame
                .set_status_text("Hover a Reviera wall to cut an opening in it", 0);
        };
        let height = self
            .insert_tool
            .as_ref()
            .and_then(|t| t.wall_opening.as_ref())
            .map_or(wall_openings::DEFAULT_HEIGHT, |m| m.height);
        let args = json!({
            "wall_id": host, "u0": u[0], "u1": u[1],
            "top_constraint": { "unconnected": height },
        });
        match self.run("opening.create_wall", args) {
            Ok(_) => {
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.wall_opening_motion(x, y);
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
    }

    /// The palette while the Wall Opening tool runs: the width and height it cuts.
    pub(crate) fn wall_opening_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(m) = self
            .insert_tool
            .as_ref()
            .and_then(|t| t.wall_opening.clone())
        else {
            return false;
        };
        grid.append(Property::category("Wall Opening tool", "c_wotool"));
        let (w, h) = (self.format_length(m.width), self.format_length(m.height));
        grid.append(Property::string("Width", "wot_width", w));
        grid.append(Property::string("Height", "wot_height", h));
        true
    }

    fn parse_length(&mut self, text: &str) -> Option<f64> {
        self.run(
            "units.parse",
            json!({ "text": text, "data_type": "length" }),
        )
        .ok()
        .and_then(|v| v.as_f64())
    }

    /// The selected wall openings.
    fn selected_wall_openings(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(wall_openings::wall_opening_recipe)
                    .is_some()
            })
            .collect()
    }

    /// The selected shafts.
    fn selected_shafts(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::slab_openings::shaft_recipe)
                    .is_some()
            })
            .collect()
    }

    /// Base and top rows of an extent (`prefix` names them): base level and offset, top
    /// constraint, height and top offset.
    fn extent_rows(
        &mut self,
        grid: &PropertyGrid,
        prefix: &str,
        e: &reviera_core::levels::Extent,
        z: (f64, f64),
    ) {
        let Some(store) = self.store_now() else {
            return;
        };
        let levels = reviera_core::levels::list(&store);
        let names: Vec<(String, i32)> = levels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i as i32 + 1))
            .collect();
        let base = levels
            .iter()
            .position(|l| l.guid == e.base_level)
            .map_or(-1, |i| i as i32 + 1);
        grid.append(Property::enumeration(
            "Base Constraint",
            format!("{prefix}base"),
            names.clone(),
            base,
        ));
        let off = self.format_length(e.base_offset);
        grid.append(Property::string(
            "Base Offset",
            format!("{prefix}base_off"),
            off,
        ));
        let mut tops = vec![(UNCONNECTED.to_owned(), 0)];
        tops.extend(names);
        let top = match e.top_level() {
            Some(g) => levels
                .iter()
                .position(|l| l.guid == g)
                .map_or(-1, |i| i as i32 + 1),
            None => 0,
        };
        grid.append(Property::enumeration(
            "Top Constraint",
            format!("{prefix}top"),
            tops,
            top,
        ));
        let h = self.format_length(z.1 - z.0);
        let height = format!("{prefix}height");
        grid.append(Property::string("Unconnected Height", &height, h));
        let t = self.format_length(e.top_offset);
        let top_off = format!("{prefix}top_off");
        grid.append(Property::string("Top Offset", &top_off, t));
        if e.top_level().is_some() {
            grid.set_property_read_only(&height, true);
            grid.set_help_string(
                &height,
                "Follows the top level; choose Unconnected to set it",
            );
        } else {
            grid.set_property_read_only(&top_off, true);
            grid.set_help_string(&top_off, "Applies when the top is a level");
        }
    }

    /// The rows of a selected wall opening or shaft; `false` if `id` is neither.
    pub(crate) fn opening_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        if let Ok(o) = wall_openings::resolve(&store, e) {
            grid.append(Property::category("Constraints", "c_wo"));
            self.extent_rows(grid, "wo_", &o.recipe.extent, (o.z[0], o.z[1]));
            grid.append(Property::category("Dimensions", "c_wo_dims"));
            let w = self.format_length(o.width());
            grid.append(Property::string("Width", "wo_width", w));
            let along = self.format_length(o.recipe.u[0]);
            grid.append(Property::string("From Wall Start", "wo_along", along));
            let from_end = self.format_length(o.span() - o.recipe.u[1]);
            grid.append(Property::string("From Wall End", "wo_from_end", from_end));
            return true;
        }
        if let Some(r) = reviera_core::slab_openings::shaft_recipe(e) {
            let z = r.extent.z(&store).unwrap_or((0.0, 0.0));
            grid.append(Property::category("Constraints", "c_sh"));
            self.extent_rows(grid, "sh_", &r.extent, z);
            grid.append(Property::category("Cuts", "c_sh_cuts"));
            let hosts = reviera_core::slab_openings::shaft_hosts(&store, &r);
            let names: Vec<String> = hosts
                .iter()
                .filter_map(|(h, _, _)| {
                    let h = store.get(*h)?;
                    Some(h.name.clone().unwrap_or_else(|| {
                        reviera_core::categories::category_of(h)
                            .map_or_else(|| h.class.clone(), |c| c.name.to_owned())
                    }))
                })
                .collect();
            let text = if names.is_empty() {
                "nothing yet".to_owned()
            } else {
                format!("{} ({})", names.len(), names.join(", "))
            };
            grid.append(Property::string(
                "Floors, ceilings and roofs",
                "sh_cuts",
                text,
            ));
            grid.set_property_read_only("sh_cuts", true);
            return true;
        }
        false
    }

    /// An edit of a wall opening, shaft or Wall Opening tool row; `false` if it is not one.
    pub(crate) fn opening_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        if property == "wot_width" || property == "wot_height" {
            if let Some(v) = self.parse_length(&text).filter(|v| *v > 0.0)
                && let Some(m) = self
                    .insert_tool
                    .as_mut()
                    .and_then(|t| t.wall_opening.as_mut())
            {
                if property == "wot_width" {
                    m.width = v;
                } else {
                    m.height = v;
                }
            }
            self.properties_stale = true;
            self.wall_opening_prompt();
            return true;
        }
        let (command, key, ids) = if let Some(rest) = property.strip_prefix("wo_") {
            (
                "opening.edit_wall",
                "opening_id",
                (rest, self.selected_wall_openings()),
            )
        } else if let Some(rest) = property.strip_prefix("sh_") {
            (
                "opening.edit_shaft",
                "shaft_id",
                (rest, self.selected_shafts()),
            )
        } else {
            return false;
        };
        let (row, ids) = ids;
        let Some(store) = self.store_now() else {
            return true;
        };
        let level = |name: &str| {
            reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == name)
                .map(|l| l.id)
        };
        let mut result: Result<(), String> = Ok(());
        for id in ids {
            let args: Option<Value> = match row {
                "base" => level(&text).map(|l| json!({ "base_level": l })),
                "base_off" => Some(json!({ "base_offset": text })),
                "top" if text == UNCONNECTED => {
                    // Keep the current height.
                    let e = store.get(id).expect("selected");
                    let z = wall_openings::resolve(&store, e)
                        .map(|o| (o.z[0], o.z[1]))
                        .ok()
                        .or_else(|| {
                            reviera_core::slab_openings::shaft_recipe(e)?
                                .extent
                                .z(&store)
                                .ok()
                        })
                        .unwrap_or((0.0, 3.0));
                    Some(json!({ "top_constraint": { "unconnected": z.1 - z.0 } }))
                }
                "top" => level(&text).map(|l| json!({ "top_constraint": { "level": l } })),
                "height" => Some(json!({ "top_constraint": { "unconnected": text } })),
                "top_off" => Some(json!({ "top_offset": text })),
                "width" | "along" | "from_end" => {
                    let Some(v) = self.parse_length(&text) else {
                        result = Err(format!("`{text}` is not a length"));
                        break;
                    };
                    let Ok(o) = wall_openings::resolve(&store, store.get(id).expect("selected"))
                    else {
                        continue;
                    };
                    let [u0, u1] = o.recipe.u;
                    let (a, b) = match row {
                        "width" => (u0, u0 + v),
                        "along" => (v, v + (u1 - u0)),
                        _ => (o.span() - v - (u1 - u0), o.span() - v),
                    };
                    Some(json!({ "u0": a, "u1": b }))
                }
                _ => None,
            };
            let Some(mut args) = args else { continue };
            args[key] = json!(id);
            if let Err(e) = self.run(command, args) {
                result = Err(e);
                break;
            }
        }
        if let Err(e) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }
}
