// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architectural columns in the window (ARC-COL-001): the Column tool, a selected column's
//! Properties (constraints, rotation, type and section), and Attach Top / Detach Top.
//!
//! The tool works in plans: the column's section follows the cursor (snapping to grid
//! intersections, wall lines and the like); Spacebar turns it by 90°; a click places it from the
//! plan's level up to the next level (3 m when there is none). It repeats until Esc.

use reviera_core::ElementId;
use reviera_core::columns::{self, Section};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
const DUPLICATE: &str = "Duplicate Type…";
const TYPE_KEY: &str = "IfcColumnType";

/// The Column tool: its level, rotation and the point under the cursor.
#[derive(Debug, Clone)]
pub struct ColumnTool {
    level: ElementId,
    rotation: f64,
    cursor: Option<[f64; 2]>,
}

impl Gui {
    /// Architecture › Column.
    pub(crate) fn start_column_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Columns are placed in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.column_tool = Some(ColumnTool {
            level,
            rotation: 0.0,
            cursor: None,
        });
        self.properties_stale = true;
        frame.set_status_text(
            "Column: click to place it (Spacebar turns it 90°). Esc ends the tool.",
            0,
        );
    }

    pub(crate) fn end_column_tool(&mut self) -> bool {
        if self.column_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// The type the tool places: the one last chosen, else the default (if it exists yet).
    fn tool_column_type(&self) -> Option<ElementId> {
        let store = self.store_now()?;
        let types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| columns::column_type(e).is_some())
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        self.insert_types
            .get(TYPE_KEY)
            .copied()
            .filter(|t| types.iter().any(|(i, _)| i == t))
            .or_else(|| {
                types
                    .iter()
                    .find(|(_, n)| n == columns::DEFAULT_TYPE_NAME)
                    .or(types.first())
                    .map(|(i, _)| *i)
            })
    }

    /// The point the tool would place at, snapped.
    fn column_point(&self, x: i32, y: i32) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        self.snapped_at(x, y, None, &lines).map(|s| s.point)
    }

    pub(crate) fn column_motion(&mut self, x: i32, y: i32) {
        let p = self.column_point(x, y);
        let ty = self
            .tool_column_type()
            .and_then(|t| self.store_now()?.get(t).and_then(columns::column_type))
            .unwrap_or_default();
        let Some(t) = &mut self.column_tool else {
            return;
        };
        t.cursor = p;
        let rotation = t.rotation;
        let Some(p) = p else { return };
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |pl| pl.range.cut) as f32
            + 0.004;
        let (s, c) = rotation.sin_cos();
        let ring: Vec<[f32; 3]> = ty
            .outline()
            .into_iter()
            .map(|[u, v]| {
                [
                    (p[0] + u * c - v * s) as f32,
                    (p[1] + u * s + v * c) as f32,
                    z,
                ]
            })
            .collect();
        let n = ring.len();
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: (0..n).map(|i| [ring[i], ring[(i + 1) % n]]).collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    pub(crate) fn column_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.column_point(x, y) else {
            return;
        };
        let Some(t) = self.column_tool.clone() else {
            return;
        };
        // Up to the next level, else 3 m.
        let top = self.store_now().and_then(|st| {
            let z = reviera_core::levels::elevation(st.get(t.level)?);
            reviera_core::levels::list(&st)
                .into_iter()
                .find(|l| l.elevation > z + 1e-6)
                .map(|l| l.id)
        });
        let top = match top {
            Some(l) => json!({ "level": l }),
            None => json!({ "unconnected": 3.0 }),
        };
        let mut args = json!({
            "point": p, "base_level": t.level, "top_constraint": top, "rotation": t.rotation,
        });
        if let Some(ty) = self.tool_column_type() {
            args["type_id"] = json!(ty);
        }
        match self.run("column.place_architectural", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                self.column_motion(x, y);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Spacebar in the Column tool turns it by 90°.
    pub(crate) fn column_key(&mut self, key: i32, x: i32, y: i32) -> bool {
        if key != WXK_SPACE {
            return false;
        }
        let Some(t) = &mut self.column_tool else {
            return false;
        };
        t.rotation = (t.rotation + std::f64::consts::FRAC_PI_2) % std::f64::consts::TAU;
        self.column_motion(x, y);
        true
    }

    /// The type selector and the type's section rows (`ct_`), for `current`.
    fn column_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>) {
        let Some(store) = self.store_now() else {
            return;
        };
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| columns::column_type(e).is_some())
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut choices: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        choices.push((DUPLICATE.to_owned(), choices.len() as i32));
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_col_type"));
        grid.append(Property::enumeration("Type", "ct_type", choices, index));
        self.type_rows = current;
        let Some(t) = current
            .and_then(|c| store.get(c))
            .and_then(columns::column_type)
        else {
            return;
        };
        let sections = vec![("Rectangular".to_owned(), 0), ("Round".to_owned(), 1)];
        let round = t.section == Section::Round;
        grid.append(Property::enumeration(
            "Section (type)",
            "ct_section",
            sections,
            i32::from(round),
        ));
        let w = self.format_length(t.width);
        if round {
            grid.append(Property::string("Diameter (type)", "ct_width", w));
        } else {
            grid.append(Property::string("Width (type)", "ct_width", w));
            let d = self.format_length(t.depth);
            grid.append(Property::string("Depth (type)", "ct_depth", d));
        }
    }

    /// The palette while the Column tool runs.
    pub(crate) fn column_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.column_tool.is_none() {
            return false;
        }
        grid.append(Property::category("Column tool", "c_coltool"));
        let current = self.tool_column_type();
        self.column_type_rows(grid, current);
        true
    }

    /// The rows of a selected column; `false` if `id` is not one.
    pub(crate) fn column_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = columns::column_recipe(e) else {
            return false;
        };
        let z = r.extent.z(&store).unwrap_or((0.0, 0.0));
        grid.append(Property::category("Constraints", "c_col"));
        self.extent_rows(grid, "co_", &r.extent, z);
        let attached = match &r.attach_top {
            Some(a) => reviera_core::GlobalId::parse(&a.target)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|t| store.get(t))
                .map_or("—".to_owned(), |t| {
                    format!(
                        "{} ({:?})",
                        t.name.clone().unwrap_or_else(|| t.class.clone()),
                        a.justification
                    )
                }),
            None => "No".to_owned(),
        };
        grid.append(Property::string("Top Is Attached", "co_attached", attached));
        grid.set_property_read_only("co_attached", true);
        let degrees = format!("{:.1}°", r.rotation.to_degrees());
        grid.append(Property::string("Rotation", "co_rotation", degrees));
        let current = e
            .param("@type")
            .and_then(serde_json::Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        self.column_type_rows(grid, current);
        true
    }

    /// The selected columns.
    fn selected_columns(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).and_then(columns::column_recipe).is_some())
            .collect()
    }

    /// Modify | Columns › Attach Top: each selected column's top to what is directly above it.
    pub(crate) fn attach_column_tops(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let mut missing = 0;
        for c in self.selected_columns() {
            match columns::target_above(&store, c) {
                Some(t) => {
                    if let Err(e) = self.run(
                        "column.attach",
                        json!({ "column_ids": [c], "target_id": t }),
                    ) {
                        error(&frame, &format!("The column was not attached:\n{e}"));
                    }
                }
                None => missing += 1,
            }
        }
        if missing > 0 {
            frame.set_status_text("No roof, floor or ceiling over some of the columns", 0);
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
    }

    /// Modify | Columns › Detach Top.
    pub(crate) fn detach_column_tops(&mut self) {
        let frame = self.frame;
        let ids = self.selected_columns();
        if let Err(e) = self.run("column.detach", json!({ "column_ids": ids })) {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
    }

    /// An edit of a column row; `false` if `property` is not one.
    pub(crate) fn column_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("co_") && !property.starts_with("ct_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected = self.selected_columns();
        let Some(store) = self.store_now() else {
            return true;
        };
        let level = |name: &str| {
            reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == name)
                .map(|l| l.id)
        };
        let set = |g: &mut Gui, mut args: serde_json::Value| -> Result<(), String> {
            args["column_ids"] = json!(selected);
            g.run("column.set", args).map(|_| ())
        };
        let result: Option<Result<(), String>> = match property {
            "co_base" => level(&text).map(|l| set(self, json!({ "base_level": l }))),
            "co_base_off" => Some(set(self, json!({ "base_offset": text }))),
            "co_top" if text == "Unconnected" => {
                let h = selected
                    .first()
                    .and_then(|c| {
                        columns::column_recipe(store.get(*c)?)?
                            .extent
                            .z(&store)
                            .ok()
                    })
                    .map_or(3.0, |(z0, z1)| z1 - z0);
                Some(set(self, json!({ "top_constraint": { "unconnected": h } })))
            }
            "co_top" => {
                level(&text).map(|l| set(self, json!({ "top_constraint": { "level": l } })))
            }
            "co_height" => Some(set(
                self,
                json!({ "top_constraint": { "unconnected": text } }),
            )),
            "co_top_off" => Some(set(self, json!({ "top_offset": text }))),
            "co_rotation" => {
                let t = if text.trim().ends_with('°') || text.contains("deg") {
                    text.clone()
                } else {
                    format!("{}°", text.trim())
                };
                Some(set(self, json!({ "rotation": t })))
            }
            "ct_type" => self.choose_column_type(&text, &selected),
            "ct_section" => self.type_rows.map(|t| {
                let v = if text == "Round" {
                    "round"
                } else {
                    "rectangular"
                };
                self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": "section", "value": v }),
                )
                .map(|_| ())
            }),
            "ct_width" | "ct_depth" => self.type_rows.map(|t| {
                let param = if property == "ct_width" {
                    "width"
                } else {
                    "depth"
                };
                self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": param, "value": text }),
                )
                .map(|_| ())
            }),
            _ => None,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    fn choose_column_type(
        &mut self,
        choice: &str,
        selected: &[ElementId],
    ) -> Option<Result<(), String>> {
        let frame = self.frame;
        let store = self.store_now()?;
        let chosen = if choice == DUPLICATE {
            let current = self.type_rows?;
            let proposed = reviera_core::types::duplicate_name(&store, current);
            let dialog =
                TextEntryDialog::builder(&frame, "Name of the new type:", "Duplicate Type")
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
            store
                .iter()
                .find(|e| columns::column_type(e).is_some() && e.name.as_deref() == Some(choice))?
                .id()
        };
        if selected.is_empty() {
            self.insert_types.insert(TYPE_KEY, chosen);
            return Some(Ok(()));
        }
        Some(
            self.run(
                "element.set_type",
                json!({ "element_ids": selected, "type_id": chosen }),
            )
            .map(|_| ()),
        )
    }
}
