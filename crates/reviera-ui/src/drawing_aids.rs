// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Drawing aids in plans (spec Ch 3 §3.1.6–3.1.9, §3.14):
//!
//! - **Typed lengths** (listening dimensions, ARC-DIM-002) in the Grid, Ref Plane, Model Line and
//!   Scope Box tools, as the Wall tool has them: after the first point, typing a value (`3600`,
//!   `1200+600`, `4.5 m`, `8 6`) opens it beside the cursor with the direction frozen; Enter
//!   places the point that far along it, Backspace edits, Esc drops it.
//! - **Temporary dimensions** (ARC-DIM-001): with one wall, grid, reference plane, door or window
//!   selected, its distances to the nearest references on each side; click a value to type a
//!   new one, which moves the selected element.
//! - **Constraints** (ARC-CNST-001): Lock and EQ on a selected dimension's Properties; a locked
//!   segment reads with a padlock and EQ segments read "EQ"; the constraints of the selection are
//!   drawn dashed blue, and every constraint red with View › Reveal Constraints.
//! - **Tab** (ARC-KEY-001) cycles what is under the cursor: the element, then the chain of walls
//!   joined to it, then other elements there; the next click selects the candidate shown.

use std::collections::BTreeSet;

use reviera_core::ElementId;
use reviera_core::constraints::Kind;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const TEMP_COLOR: [f32; 4] = [0.1, 0.45, 0.85, 1.0];
const CONSTRAINT_COLOR: [f32; 4] = [0.15, 0.4, 0.95, 1.0];
const REVEALED_COLOR: [f32; 4] = [0.85, 0.1, 0.1, 1.0];
/// Text height of temporary dimensions and typed values (paper millimetres).
const TEXT_MM: f64 = 2.5;

/// A typed length's state: the start, the cursor, the text and the frozen direction.
type TypedState = ([f64; 2], [f64; 2], String, Option<[f64; 2]>);

/// Tab's candidates under the cursor.
#[derive(Debug, Clone, Default)]
pub struct TabCycle {
    pub at: (i32, i32),
    pub candidates: Vec<Vec<ElementId>>,
    pub index: usize,
}

fn text_strokes(text: &str, origin: [f64; 2], h: f64, z: f64) -> Vec<[[f32; 3]; 2]> {
    reviera_render::font::strokes(
        text,
        glam::Vec3::new(origin[0] as f32, origin[1] as f32, z as f32),
        glam::Vec3::X,
        glam::Vec3::Y,
        h as f32,
    )
    .into_iter()
    .map(|[p, q]| [p.to_array(), q.to_array()])
    .collect()
}

impl Gui {
    // ---- Typed lengths in the Grid and line tools ----

    /// The start, cursor and typed text of the running grid or line tool.
    fn typed_state(&self) -> Option<TypedState> {
        if let Some(t) = &self.grid_tool {
            let (a, b) = self.grid_ends()?;
            return Some((a, b, t.typed.clone(), t.typed_dir));
        }
        if let Some(t) = &self.line_tool {
            let (a, b) = self.line_ends()?;
            return Some((a, b, t.typed.clone(), t.typed_dir));
        }
        None
    }

    fn set_typed(&mut self, text: String, dir: Option<[f64; 2]>) {
        if let Some(t) = &mut self.grid_tool {
            t.typed = text;
            t.typed_dir = dir;
        } else if let Some(t) = &mut self.line_tool {
            t.typed = text;
            t.typed_dir = dir;
        }
    }

    /// Whether a typed length is open (keys go to it, not to shortcuts).
    pub(crate) fn typed_active(&self) -> bool {
        self.typed_state().is_some_and(|s| !s.2.is_empty())
    }

    /// A key for a typed length in the Grid or line tools; `true` when used.
    pub(crate) fn typed_length_key(&mut self, key: i32) -> bool {
        let Some((start, cursor, mut text, dir)) = self.typed_state() else {
            return false;
        };
        match key {
            WXK_BACK if !text.is_empty() => {
                text.pop();
                let dir = (!text.is_empty()).then_some(dir).flatten();
                self.set_typed(text, dir);
            }
            WXK_ESCAPE if !text.is_empty() => self.set_typed(String::new(), None),
            WXK_RETURN | WXK_NUMPAD_ENTER if !text.is_empty() => {
                let length = match self.run(
                    "units.parse",
                    json!({ "text": text, "data_type": "length" }),
                ) {
                    Ok(v) => v.as_f64().unwrap_or_default(),
                    Err(_) => {
                        self.frame
                            .set_status_text(&format!("Invalid value: {text}"), 0);
                        return true;
                    }
                };
                if length <= 0.0 {
                    self.frame
                        .set_status_text("Value must be greater than 0", 0);
                    return true;
                }
                let d = dir.unwrap_or([1.0, 0.0]);
                let end = [start[0] + d[0] * length, start[1] + d[1] * length]
                    .map(|v| (v * 1e6).round() / 1e6);
                if self.grid_tool.is_some() {
                    self.grid_click_at(end);
                } else {
                    self.line_click_at(end);
                }
                return true;
            }
            _ => {
                let Some(c) = u32::try_from(key).ok().and_then(char::from_u32) else {
                    return false;
                };
                let starts = c.is_ascii_digit() || c == '.' || c == '(' || c == '-';
                let continues = !text.is_empty() && (c.is_ascii_graphic() || c == ' ' || c == '×');
                if !(starts || continues) {
                    return false;
                }
                // The direction freezes when typing begins.
                let dir = dir.or_else(|| {
                    let (dx, dy) = (cursor[0] - start[0], cursor[1] - start[1]);
                    let l = dx.hypot(dy);
                    Some(if l > 1e-9 {
                        [dx / l, dy / l]
                    } else {
                        [1.0, 0.0]
                    })
                });
                text.push(c);
                self.set_typed(text, dir);
            }
        }
        let typed = self.typed_state().map(|s| s.2).unwrap_or_default();
        self.frame.set_status_text(
            &if typed.is_empty() {
                "Ready".to_owned()
            } else {
                format!("Length: {typed}_  (Enter places it, Esc drops it)")
            },
            0,
        );
        if self.grid_tool.is_some() {
            self.grid_preview();
        } else {
            self.line_preview();
        }
        true
    }

    /// The typed length beside the cursor, and the frozen direction, for a tool's preview.
    pub(crate) fn typed_overlay(&self, z: f32, scale: f32) -> Option<Lines> {
        let (start, cursor, text, dir) = self.typed_state()?;
        if text.is_empty() {
            return None;
        }
        let d = dir.unwrap_or([1.0, 0.0]);
        let reach = ((cursor[0] - start[0]) * d[0] + (cursor[1] - start[1]) * d[1]).max(0.5);
        let end = [start[0] + d[0] * reach, start[1] + d[1] * reach];
        let mm = f64::from(scale) / 1000.0;
        let mut segments = vec![[
            [start[0] as f32, start[1] as f32, z],
            [end[0] as f32, end[1] as f32, z],
        ]];
        segments.extend(text_strokes(
            &format!("{text}_"),
            [cursor[0] + 2.0 * mm, cursor[1] + 2.0 * mm],
            TEXT_MM * mm,
            f64::from(z),
        ));
        Some(Lines {
            color: TEMP_COLOR,
            width: 0.25,
            segments,
        })
    }

    // ---- Constraints on dimensions ----

    /// Lock and EQ rows of a selected dimension.
    pub(crate) fn dimension_constraint_rows(
        &mut self,
        grid: &PropertyGrid,
        id: ElementId,
        segments: usize,
    ) {
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let cons = reviera_core::constraints::of_dimension(&store, e.guid().as_str());
        grid.append(Property::category("Constraints", "c_dc"));
        for i in 0..segments {
            let locked = cons
                .iter()
                .any(|(_, c)| c.kind == Kind::Distance && c.segment == Some(i));
            grid.append(Property::boolean(
                format!("Lock Segment {}", i + 1),
                format!("dc_lock{i}"),
                locked,
            ));
        }
        if segments >= 2 {
            let eq = cons.iter().any(|(_, c)| c.kind == Kind::Equality);
            grid.append(Property::boolean("EQ", "dc_eq", eq));
        }
    }

    pub(crate) fn dimension_constraint_edit(
        &mut self,
        property: &str,
        text: Option<String>,
    ) -> bool {
        let Some(row) = property.strip_prefix("dc_") else {
            return false;
        };
        let on = matches!(text.as_deref(), Some("true" | "1"));
        let Some(&id) = self.selection().first() else {
            return true;
        };
        let result = if row == "eq" {
            self.run(
                "constraint.set_equality",
                json!({ "dimension_id": id, "on": on }),
            )
        } else if let Some(i) = row
            .strip_prefix("lock")
            .and_then(|i| i.parse::<usize>().ok())
        {
            let command = if on {
                "constraint.lock_dimension"
            } else {
                "constraint.unlock_dimension"
            };
            self.run(command, json!({ "dimension_id": id, "segment": i }))
        } else {
            return true;
        };
        let frame = self.frame;
        match result {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
        true
    }

    /// Dimension › Lock (every segment of the selected dimensions).
    pub(crate) fn lock_selected_dimensions(&mut self) {
        let frame = self.frame;
        for id in self.selection() {
            if let Err(e) = self.run("constraint.lock_dimension", json!({ "dimension_id": id })) {
                frame.set_status_text(&e, 0);
            }
        }
        self.refresh_scene(&frame);
        self.properties_stale = true;
    }

    /// Dimension › EQ (toggles equality on the selected dimension).
    pub(crate) fn toggle_dimension_eq(&mut self) {
        let frame = self.frame;
        let Some(&id) = self.selection().first() else {
            return;
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let on = store.get(id).is_some_and(|e| {
            reviera_core::constraints::of_dimension(&store, e.guid().as_str())
                .iter()
                .any(|(_, c)| c.kind == Kind::Equality)
        });
        match self.run(
            "constraint.set_equality",
            json!({ "dimension_id": id, "on": !on }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
    }

    /// View › Reveal Constraints (toggles it in the active view).
    pub(crate) fn toggle_reveal_constraints(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return;
        };
        let on = self
            .store_now()
            .and_then(|s| s.get(view).map(reviera_core::constraints::revealed))
            .unwrap_or(false);
        match self.run(
            "view.reveal_constraints",
            json!({ "view_id": view, "on": !on }),
        ) {
            Ok(_) => {
                frame.set_status_text(
                    if on {
                        "Constraints hidden"
                    } else {
                        "Constraints revealed"
                    },
                    0,
                );
                if let Some(doc) = self.active_view().map(|v| v.doc) {
                    self.plans_redraw(doc);
                }
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Constraints drawn in plan `index`: every one in red when revealed, else those of the
    /// selection in blue, as dashed lines between their references.
    pub(crate) fn constraint_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected: BTreeSet<String> = {
            let store = self.store_now();
            self.selection()
                .into_iter()
                .filter_map(|id| Some(store.as_ref()?.get(id)?.guid().to_string()))
                .collect()
        };
        let v = &self.views[index];
        let Some(plan) = &v.plan else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let revealed = v
            .element
            .and_then(|e| store.get(e))
            .is_some_and(reviera_core::constraints::revealed);
        let z = plan.range.cut + 0.003;
        let unit = f64::from(plan.scale) / 1000.0;
        let mut out = Vec::new();
        for (id, kind, pts) in reviera_core::constraints::drawn(&store) {
            let c = store.get(id).and_then(reviera_core::constraints::data);
            let mine = c.is_some_and(|c| {
                c.refs
                    .iter()
                    .any(|r| r.element.as_ref().is_some_and(|g| selected.contains(g)))
            });
            if !revealed && !mine {
                continue;
            }
            let poly: Vec<[f64; 3]> = pts.iter().map(|p| [p[0], p[1], z]).collect();
            let mut segments: Vec<[[f32; 3]; 2]> = reviera_core::model_lines::dashes(
                &poly,
                reviera_core::model_lines::Pattern::Dashed,
                unit,
            )
            .into_iter()
            .map(|s| [s[0].map(|v| v as f32), s[1].map(|v| v as f32)])
            .collect();
            if kind == Kind::Equality
                && let Some(p) = pts.first()
            {
                segments.extend(text_strokes(
                    "EQ",
                    [p[0] + unit, p[1] + unit],
                    2.0 * unit,
                    z,
                ));
            }
            out.push(Lines {
                color: if revealed {
                    REVEALED_COLOR
                } else {
                    CONSTRAINT_COLOR
                },
                width: 0.25,
                segments,
            });
        }
        out
    }

    // ---- Temporary dimensions ----

    /// The temporary dimensions shown now: one element selected in a plan, no tool running.
    fn temp_dims(&mut self) -> Vec<(ElementId, reviera_core::temp_dims::TempDim)> {
        if self.any_tool() || !self.in_plan() {
            return Vec::new();
        }
        let ids = self.selection();
        let [id] = ids[..] else { return Vec::new() };
        let level = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .and_then(|p| self.store_now()?.get(p.level).map(|l| l.guid().to_string()));
        self.store_now()
            .map(|s| reviera_core::temp_dims::of_element(&s, id, level.as_deref()))
            .unwrap_or_default()
            .into_iter()
            .map(|d| (id, d))
            .collect()
    }

    /// Where a temporary dimension's value is written, and the text (plan, metres).
    fn temp_dim_label(
        &mut self,
        d: &reviera_core::temp_dims::TempDim,
        scale: f64,
    ) -> ([f64; 2], String, f64) {
        let mm = scale / 1000.0;
        let mid = [(d.from[0] + d.to[0]) / 2.0, (d.from[1] + d.to[1]) / 2.0];
        let text = self.format_length(d.value);
        let h = TEXT_MM * mm;
        let w = reviera_render::font::width(&text, h as f32) as f64;
        ([mid[0] - w / 2.0, mid[1] + h * 0.6], text, w)
    }

    /// Lines of the temporary dimensions of plan `index` (the active one only).
    pub(crate) fn temp_dim_lines(&mut self, index: usize) -> Vec<Lines> {
        if self.active != Some(self.views[index].id) {
            return Vec::new();
        }
        let Some(plan) = self.views[index].plan.clone() else {
            return Vec::new();
        };
        let scale = f64::from(plan.scale);
        let z = plan.range.cut + 0.004;
        let mm = scale / 1000.0;
        let mut segments = Vec::new();
        for (_, d) in self.temp_dims() {
            let at = |p: [f64; 2]| [p[0] as f32, p[1] as f32, z as f32];
            segments.push([at(d.from), at(d.to)]);
            // Ticks at both ends.
            let dir = [d.to[0] - d.from[0], d.to[1] - d.from[1]];
            let l = dir[0].hypot(dir[1]).max(1e-9);
            let (u, n) = ([dir[0] / l, dir[1] / l], [-dir[1] / l, dir[0] / l]);
            for p in [d.from, d.to] {
                let k = 1.5 * mm;
                segments.push([
                    at([p[0] - (u[0] + n[0]) * k, p[1] - (u[1] + n[1]) * k]),
                    at([p[0] + (u[0] + n[0]) * k, p[1] + (u[1] + n[1]) * k]),
                ]);
            }
            let (origin, text, _) = self.temp_dim_label(&d, scale);
            segments.extend(text_strokes(&text, origin, TEXT_MM * mm, z));
        }
        if segments.is_empty() {
            return Vec::new();
        }
        vec![Lines {
            color: TEMP_COLOR,
            width: 0.18,
            segments,
        }]
    }

    /// A click on a temporary dimension's value: asks for a new one and moves the element.
    /// `true` when the click was on one.
    pub(crate) fn temp_dim_click(&mut self, x: i32, y: i32) -> bool {
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return false;
        };
        let Some(p) = self.plan_xy(x, y) else {
            return false;
        };
        let scale = f64::from(plan.scale);
        let h = TEXT_MM * scale / 1000.0;
        let dims = self.temp_dims();
        for (index, (id, d)) in dims.iter().enumerate() {
            let (origin, text, w) = self.temp_dim_label(d, scale);
            let inside = p[0] >= origin[0] - h * 0.3
                && p[0] <= origin[0] + w + h * 0.3
                && p[1] >= origin[1] - h * 0.3
                && p[1] <= origin[1] + h * 1.3;
            if !inside {
                continue;
            }
            let frame = self.frame;
            let dialog = TextEntryDialog::builder(
                &frame,
                &format!("New distance to the {}:", d.other_kind),
                "Temporary Dimension",
            )
            .with_default_value(&text)
            .build();
            if dialog.show_modal() != ID_OK {
                return true;
            }
            let typed = dialog.get_value().unwrap_or_default();
            let value = match self.run(
                "units.parse",
                json!({ "text": typed, "data_type": "length" }),
            ) {
                Ok(v) => v.as_f64().unwrap_or_default(),
                Err(_) => {
                    frame.set_status_text(&format!("Invalid value: {typed}"), 0);
                    return true;
                }
            };
            match self.run(
                "dim.temporary_set",
                json!({ "element_id": id, "index": index, "value": value, "view_id": self.active_view().and_then(|v| v.element) }),
            ) {
                Ok(_) => {
                    self.refresh_scene(&frame);
                    self.update_grips();
                }
                Err(e) => frame.set_status_text(&e, 0),
            }
            return true;
        }
        false
    }

    // ---- Tab cycling ----

    /// Tab (Shift+Tab backwards): the next candidate under the cursor.
    pub(crate) fn tab_cycle(&mut self, back: bool, panel: &Panel) {
        if self.any_tool() || self.doc.is_none() {
            return;
        }
        let (x, y) = self.mouse;
        let fresh = self.tab.as_ref().is_none_or(|t| t.at != (x, y));
        if fresh {
            let mut candidates: Vec<Vec<ElementId>> = Vec::new();
            let push = |c: Vec<ElementId>, list: &mut Vec<Vec<ElementId>>| {
                if !c.is_empty() && !list.contains(&c) {
                    list.push(c);
                }
            };
            let first = self.element_at(x, y, panel);
            if let Some(hit) = first {
                push(vec![hit], &mut candidates);
                let chain = self.wall_chain(hit);
                if chain.len() > 1 {
                    push(chain, &mut candidates);
                }
            }
            // Other things at the same place.
            for other in [
                self.note_at(x, y),
                self.dim_at(x, y),
                self.grid_at(x, y),
                self.datum_at(x, y),
                self.insert_at(x, y),
                self.opening_at(x, y),
                self.room_label_at(x, y),
            ]
            .into_iter()
            .flatten()
            {
                push(vec![other], &mut candidates);
            }
            if candidates.is_empty() {
                return;
            }
            self.tab = Some(TabCycle {
                at: (x, y),
                candidates,
                index: 0,
            });
        } else if let Some(t) = &mut self.tab {
            let n = t.candidates.len();
            t.index = if back {
                (t.index + n - 1) % n
            } else {
                (t.index + 1) % n
            };
        }
        let Some(t) = self.tab.clone() else { return };
        let current = t.candidates[t.index].clone();
        let what = if current.len() > 1 {
            format!("{} walls in a chain", current.len())
        } else {
            self.store_now()
                .and_then(|s| {
                    let e = s.get(current[0])?;
                    Some(
                        reviera_core::categories::category_of(e)
                            .map(|c| c.name.to_owned())
                            .unwrap_or_else(|| e.class.clone()),
                    )
                })
                .unwrap_or_default()
        };
        self.frame.set_status_text(
            &format!(
                "Tab {}/{}: {what} — click to select it",
                t.index + 1,
                t.candidates.len()
            ),
            0,
        );
        if let Some(v) = self.viewport() {
            v.set_highlight(&current);
        }
        // Tab keeps the focus in the view.
        panel.set_focus();
    }

    /// The Tab candidate a click at (`x`, `y`) selects, if Tab cycling is active there.
    pub(crate) fn tab_pick(&mut self, x: i32, y: i32) -> Option<Vec<ElementId>> {
        let t = self.tab.take()?;
        ((t.at.0 - x).abs() <= 3 && (t.at.1 - y).abs() <= 3).then(|| t.candidates[t.index].clone())
    }
}
