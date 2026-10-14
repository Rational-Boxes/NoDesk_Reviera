// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curtain walls in the window (ARC-CW-001 … 006): the Curtain Wall tool, and a selected
//! curtain wall's Properties (constraints, grid justification and offset, and its type's layout,
//! mullions and panels).
//!
//! The tool works in plans: click the start and the end of the base line (snapping like the Wall
//! tool); the curtain wall runs from the plan's level up to the next level (3 m when there is
//! none). Drawn along a wall, it cuts its own opening in it. The tool repeats until Esc.

use reviera_core::ElementId;
use reviera_core::curtain::{self, Justification, Layout, Panel};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
const DUPLICATE: &str = "Duplicate Type…";
const TYPE_KEY: &str = "IfcCurtainWallType";
const LAYOUTS: [(&str, Layout); 5] = [
    ("None", Layout::None),
    ("Fixed Distance", Layout::FixedDistance),
    ("Fixed Number", Layout::FixedNumber),
    ("Maximum Spacing", Layout::MaximumSpacing),
    ("Minimum Spacing", Layout::MinimumSpacing),
];
const JUSTIFICATIONS: [(&str, Justification); 3] = [
    ("Beginning", Justification::Beginning),
    ("Center", Justification::Center),
    ("End", Justification::End),
];
const PANELS: [(&str, Panel); 3] = [
    ("Glazed", Panel::Glazed),
    ("Solid", Panel::Solid),
    ("Empty", Panel::Empty),
];

/// The Curtain Wall tool: its level, the start once clicked, and the point under the cursor.
#[derive(Debug, Clone)]
pub struct CurtainTool {
    level: ElementId,
    start: Option<[f64; 2]>,
    cursor: Option<[f64; 2]>,
}

fn choices<T>(items: &[(&str, T)]) -> Vec<(String, i32)> {
    items
        .iter()
        .enumerate()
        .map(|(i, (l, _))| ((*l).to_owned(), i as i32))
        .collect()
}

impl Gui {
    /// Architecture › Curtain Wall.
    pub(crate) fn start_curtain_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Curtain walls are drawn in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.curtain_tool = Some(CurtainTool {
            level,
            start: None,
            cursor: None,
        });
        self.properties_stale = true;
        frame.set_status_text("Curtain Wall: click the start of its base line", 0);
    }

    pub(crate) fn end_curtain_tool(&mut self) -> bool {
        if self.curtain_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// Esc: drops the start, then ends the tool.
    pub(crate) fn curtain_escape(&mut self) -> bool {
        let Some(t) = &mut self.curtain_tool else {
            return false;
        };
        if t.start.take().is_some() {
            if let Some(v) = self.viewport() {
                v.set_overlay_lines(&[]);
            }
            self.frame
                .set_status_text("Curtain Wall: click the start of its base line", 0);
            return true;
        }
        self.end_curtain_tool()
    }

    fn tool_curtain_type(&self) -> Option<ElementId> {
        let store = self.store_now()?;
        let types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| curtain::curtain_type(e).is_some())
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        self.insert_types
            .get(TYPE_KEY)
            .copied()
            .filter(|t| types.iter().any(|(i, _)| i == t))
            .or_else(|| {
                types
                    .iter()
                    .find(|(_, n)| n == curtain::DEFAULT_TYPE_NAME)
                    .or(types.first())
                    .map(|(i, _)| *i)
            })
    }

    pub(crate) fn curtain_motion(&mut self, x: i32, y: i32) {
        let from = self.curtain_tool.as_ref().and_then(|t| t.start);
        let lines = self.snap_lines();
        let p = self.snapped_at(x, y, from, &lines).map(|s| s.point);
        let Some(t) = &mut self.curtain_tool else {
            return;
        };
        t.cursor = p;
        let (Some(a), Some(b)) = (t.start, p) else {
            return;
        };
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |pl| pl.range.cut) as f32
            + 0.004;
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: vec![[[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]],
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
        let length = self.format_length((b[0] - a[0]).hypot(b[1] - a[1]));
        self.frame.set_status_text(
            &format!(
                "Curtain Wall: click the end of its base line ({length}). Esc drops the start."
            ),
            0,
        );
    }

    pub(crate) fn curtain_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        self.curtain_motion(x, y);
        let Some(t) = self.curtain_tool.clone() else {
            return;
        };
        let Some(p) = t.cursor else { return };
        let Some(a) = t.start else {
            if let Some(t) = &mut self.curtain_tool {
                t.start = Some(p);
            }
            return frame.set_status_text("Curtain Wall: click the end of its base line", 0);
        };
        if (p[0] - a[0]).hypot(p[1] - a[1]) < 0.01 {
            return;
        }
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
            "curve": { "start": a, "end": p }, "base_level": t.level, "top_constraint": top,
        });
        if let Some(ty) = self.tool_curtain_type() {
            args["type_id"] = json!(ty);
        }
        match self.run("curtain.create", args) {
            Ok(_) => {
                if let Some(t) = &mut self.curtain_tool {
                    t.start = None;
                }
                if let Some(v) = self.viewport() {
                    v.set_overlay_lines(&[]);
                }
                self.refresh_scene(&frame);
                self.properties_stale = true;
                frame.set_status_text("Curtain Wall: click the start of the next one, or Esc", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The type selector and the type's rows (`cwt_`), for `current`.
    fn curtain_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>) {
        let Some(store) = self.store_now() else {
            return;
        };
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| curtain::curtain_type(e).is_some())
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut names: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        names.push((DUPLICATE.to_owned(), names.len() as i32));
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_cw_type"));
        grid.append(Property::enumeration("Type", "cwt_type", names, index));
        self.type_rows = current;
        let Some(t) = current
            .and_then(|c| store.get(c))
            .and_then(curtain::curtain_type)
        else {
            return;
        };
        let at = |l: Layout| LAYOUTS.iter().position(|x| x.1 == l).unwrap_or(0) as i32;
        for (dir, rule, label) in [("u", t.u, "Vertical Grid"), ("v", t.v, "Horizontal Grid")] {
            grid.append(Property::enumeration(
                format!("{label} Layout (type)"),
                format!("cwt_{dir}_layout"),
                choices(&LAYOUTS),
                at(rule.layout),
            ));
            match rule.layout {
                Layout::FixedNumber => {
                    grid.append(Property::string(
                        format!("{label} Number (type)"),
                        format!("cwt_{dir}_number"),
                        rule.number.to_string(),
                    ));
                }
                Layout::None => {}
                _ => {
                    let s = self.format_length(rule.spacing);
                    grid.append(Property::string(
                        format!("{label} Spacing (type)"),
                        format!("cwt_{dir}_spacing"),
                        s,
                    ));
                }
            }
        }
        let m = t.interior.or(t.border);
        let (w, d) = m.map_or((String::new(), String::new()), |m| {
            (self.format_length(m.width), self.format_length(m.depth))
        });
        grid.append(Property::string(
            "Mullion Width (type)",
            "cwt_mullion_width",
            w,
        ));
        grid.append(Property::string(
            "Mullion Depth (type)",
            "cwt_mullion_depth",
            d,
        ));
        grid.set_help_string(
            "cwt_mullion_width",
            "0 leaves the curtain wall without mullions",
        );
        grid.append(Property::boolean(
            "Border Mullions (type)",
            "cwt_border",
            t.border.is_some(),
        ));
        let panel = PANELS.iter().position(|p| p.1 == t.panel).unwrap_or(0) as i32;
        grid.append(Property::enumeration(
            "Panels (type)",
            "cwt_panel",
            choices(&PANELS),
            panel,
        ));
        let th = self.format_length(t.panel_thickness);
        grid.append(Property::string(
            "Panel Thickness (type)",
            "cwt_panel_thickness",
            th,
        ));
    }

    /// The palette while the Curtain Wall tool runs.
    pub(crate) fn curtain_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.curtain_tool.is_none() {
            return false;
        }
        grid.append(Property::category("Curtain Wall tool", "c_cwtool"));
        let current = self.tool_curtain_type();
        self.curtain_type_rows(grid, current);
        true
    }

    /// The rows of a selected curtain wall; `false` if `id` is not one.
    pub(crate) fn curtain_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = curtain::curtain_recipe(e) else {
            return false;
        };
        let z = r.extent.z(&store).unwrap_or((0.0, 0.0));
        grid.append(Property::category("Constraints", "c_cw"));
        self.extent_rows(grid, "cw_", &r.extent, z);
        let length = self.format_length(r.curve.length());
        grid.append(Property::string("Length", "cw_length", length));
        grid.set_property_read_only("cw_length", true);
        grid.append(Property::category("Grids", "c_cw_grids"));
        let j = |x: Justification| JUSTIFICATIONS.iter().position(|y| y.1 == x).unwrap_or(0) as i32;
        for (dir, p, label) in [("u", r.u, "Vertical Grid"), ("v", r.v, "Horizontal Grid")] {
            grid.append(Property::enumeration(
                format!("{label} Justification"),
                format!("cw_{dir}_just"),
                choices(&JUSTIFICATIONS),
                j(p.justification),
            ));
            let o = self.format_length(p.offset);
            grid.append(Property::string(
                format!("{label} Offset"),
                format!("cw_{dir}_offset"),
                o,
            ));
        }
        let current = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        self.curtain_type_rows(grid, current);
        true
    }

    fn selected_curtains(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).and_then(curtain::curtain_recipe).is_some())
            .collect()
    }

    /// An edit of a curtain wall row; `false` if `property` is not one.
    pub(crate) fn curtain_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("cw_") && !property.starts_with("cwt_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected = self.selected_curtains();
        let Some(store) = self.store_now() else {
            return true;
        };
        let level = |name: &str| {
            reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == name)
                .map(|l| l.id)
        };
        let set = |g: &mut Gui, mut args: Value| -> Result<(), String> {
            args["curtain_ids"] = json!(selected);
            g.run("curtain.set", args).map(|_| ())
        };
        let type_param = |g: &mut Gui, param: &str, value: Value| -> Option<Result<(), String>> {
            let t = g.type_rows?;
            Some(
                g.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": param, "value": value }),
                )
                .map(|_| ()),
            )
        };
        let key = |label: &str, items: &[(&str, &str)]| {
            items
                .iter()
                .find(|(l, _)| *l == label)
                .map(|(_, k)| (*k).to_owned())
        };
        let result: Option<Result<(), String>> = match property {
            "cw_base" => level(&text).map(|l| set(self, json!({ "base_level": l }))),
            "cw_base_off" => Some(set(self, json!({ "base_offset": text }))),
            "cw_top" if text == "Unconnected" => {
                let h = selected
                    .first()
                    .and_then(|c| {
                        curtain::curtain_recipe(store.get(*c)?)?
                            .extent
                            .z(&store)
                            .ok()
                    })
                    .map_or(3.0, |(z0, z1)| z1 - z0);
                Some(set(self, json!({ "top_constraint": { "unconnected": h } })))
            }
            "cw_top" => {
                level(&text).map(|l| set(self, json!({ "top_constraint": { "level": l } })))
            }
            "cw_height" => Some(set(
                self,
                json!({ "top_constraint": { "unconnected": text } }),
            )),
            "cw_top_off" => Some(set(self, json!({ "top_offset": text }))),
            "cw_u_just" | "cw_v_just" => {
                let k = key(
                    &text,
                    &[
                        ("Beginning", "beginning"),
                        ("Center", "center"),
                        ("End", "end"),
                    ],
                );
                let field = if property == "cw_u_just" {
                    "u_justification"
                } else {
                    "v_justification"
                };
                k.map(|k| set(self, json!({ field: k })))
            }
            "cw_u_offset" => Some(set(self, json!({ "u_offset": text }))),
            "cw_v_offset" => Some(set(self, json!({ "v_offset": text }))),
            "cwt_type" => self.choose_curtain_type(&text, &selected),
            "cwt_u_layout" | "cwt_v_layout" => {
                let k = key(
                    &text,
                    &[
                        ("None", "none"),
                        ("Fixed Distance", "fixed_distance"),
                        ("Fixed Number", "fixed_number"),
                        ("Maximum Spacing", "maximum_spacing"),
                        ("Minimum Spacing", "minimum_spacing"),
                    ],
                );
                let param = &property[4..];
                k.and_then(|k| type_param(self, param, json!(k)))
            }
            "cwt_u_spacing"
            | "cwt_v_spacing"
            | "cwt_u_number"
            | "cwt_v_number"
            | "cwt_mullion_width"
            | "cwt_mullion_depth"
            | "cwt_panel_thickness" => {
                let param = property[4..].to_owned();
                type_param(self, &param, json!(text))
            }
            "cwt_border" => type_param(
                self,
                "border_mullions",
                json!(text == "true" || text == "1"),
            ),
            "cwt_panel" => {
                let k = key(
                    &text,
                    &[("Glazed", "glazed"), ("Solid", "solid"), ("Empty", "empty")],
                );
                k.and_then(|k| type_param(self, "panel", json!(k)))
            }
            _ => None,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    fn choose_curtain_type(
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
                .find(|e| curtain::curtain_type(e).is_some() && e.name.as_deref() == Some(choice))?
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
