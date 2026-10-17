// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Beams in the window (STR-BEAM-001, STR-BEAM-002): the Beam tool (Structure › Beam), On Grids,
//! Load Section for beam types, and a selected beam's Properties (level, end offsets, usage,
//! justification, orientation, rotation, extensions, its type's section).
//!
//! The tool works in plans: click a start and an end (snapping to grid crossings, wall lines and
//! the like); the beam's top is at the plan's level. It chains: each end starts the next beam,
//! until Esc.

use reviera_core::ElementId;
use reviera_core::beams::{self, Orientation, Usage, YJustification, ZJustification};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
pub(crate) const TYPE_KEY: &str = "IfcBeamType";
const DUPLICATE: &str = "Duplicate Type…";

/// The Beam tool: its level and the chain's last point.
#[derive(Debug, Clone)]
pub struct BeamTool {
    level: ElementId,
    start: Option<[f64; 2]>,
}

const USAGES: [(&str, Usage); 6] = [
    ("Automatic", Usage::Automatic),
    ("Girder", Usage::Girder),
    ("Joist", Usage::Joist),
    ("Purlin", Usage::Purlin),
    ("Horizontal Bracing", Usage::HorizontalBracing),
    ("Other", Usage::Other),
];
const Y_JUST: [(&str, YJustification); 4] = [
    ("Origin", YJustification::Origin),
    ("Left", YJustification::Left),
    ("Center", YJustification::Centre),
    ("Right", YJustification::Right),
];
const Z_JUST: [(&str, ZJustification); 4] = [
    ("Origin", ZJustification::Origin),
    ("Top", ZJustification::Top),
    ("Center", ZJustification::Centre),
    ("Bottom", ZJustification::Bottom),
];
const ORIENTATIONS: [(&str, Orientation); 2] = [
    ("Normal", Orientation::Normal),
    ("Horizontal", Orientation::Horizontal),
];

fn choices<T>(list: &[(&str, T)]) -> Vec<(String, i32)> {
    list.iter()
        .enumerate()
        .map(|(i, (n, _))| ((*n).to_owned(), i as i32))
        .collect()
}

fn index<T: PartialEq>(list: &[(&str, T)], v: &T) -> i32 {
    list.iter()
        .position(|(_, x)| x == v)
        .map_or(-1, |i| i as i32)
}

impl Gui {
    /// Structure › Beam.
    pub(crate) fn start_beam_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Beams are drawn in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.beam_tool = Some(BeamTool { level, start: None });
        self.properties_stale = true;
        frame.set_status_text(
            "Beam: click its start, then its end (it snaps to grid crossings); each end starts the next. Esc ends the tool.",
            0,
        );
    }

    pub(crate) fn end_beam_tool(&mut self) -> bool {
        if self.beam_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// The type the tool draws: the one last chosen, else the first beam type (none: the
    /// region's default, made by `beam.create`).
    fn tool_beam_type(&self) -> Option<ElementId> {
        let store = self.store_now()?;
        let types: Vec<ElementId> = store
            .iter()
            .filter(|e| beams::beam_type(e).is_some())
            .map(|e| e.id())
            .collect();
        self.insert_types
            .get(TYPE_KEY)
            .copied()
            .filter(|t| types.contains(t))
            .or_else(|| types.first().copied())
    }

    fn beam_point(&self, x: i32, y: i32, from: Option<[f64; 2]>) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        self.snapped_at(x, y, from, &lines).map(|s| s.point)
    }

    pub(crate) fn beam_motion(&mut self, x: i32, y: i32) {
        let Some(t) = self.beam_tool.clone() else {
            return;
        };
        let (Some(a), Some(b)) = (t.start, self.beam_point(x, y, t.start)) else {
            return;
        };
        let width = self
            .tool_beam_type()
            .and_then(|ty| self.store_now()?.get(ty).and_then(beams::beam_type))
            .map_or(0.15, |ty| ty.width);
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |pl| pl.range.cut) as f32
            + 0.004;
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
        let n = [
            -(b[1] - a[1]) / l * width / 2.0,
            (b[0] - a[0]) / l * width / 2.0,
        ];
        let p = |q: [f64; 2], k: f64| [(q[0] + n[0] * k) as f32, (q[1] + n[1] * k) as f32, z];
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: vec![
                [p(a, 1.0), p(b, 1.0)],
                [p(a, -1.0), p(b, -1.0)],
                [p(a, 1.0), p(a, -1.0)],
                [p(b, 1.0), p(b, -1.0)],
            ],
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    pub(crate) fn beam_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.beam_tool.clone() else {
            return;
        };
        let Some(p) = self.beam_point(x, y, t.start) else {
            return;
        };
        let Some(a) = t.start else {
            if let Some(t) = &mut self.beam_tool {
                t.start = Some(p);
            }
            return frame.set_status_text("Beam: click its end", 0);
        };
        if (p[0] - a[0]).hypot(p[1] - a[1]) < 1e-3 {
            return;
        }
        let mut args = json!({
            "curve": { "start": a, "end": p }, "reference_level": t.level,
        });
        if let Some(ty) = self.tool_beam_type() {
            args["type_id"] = json!(ty);
        }
        match self.run("beam.create", args) {
            Ok(v) => {
                if let Some(t) = &mut self.beam_tool {
                    t.start = Some(p);
                }
                let usage = self
                    .run("beam.info", json!({ "beam_ids": [v] }))
                    .ok()
                    .and_then(|i| i[0]["usage"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                frame.set_status_text(
                    &format!("Beam placed ({usage}); click the next end, or Esc"),
                    0,
                );
                self.refresh_scene(&frame);
                self.properties_stale = true;
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The beam type selector (`bt_type`) and its section rows (`ct_`, shared with columns).
    fn beam_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>) {
        let Some(store) = self.store_now() else {
            return;
        };
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| beams::beam_type(e).is_some())
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut list: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        list.push((DUPLICATE.to_owned(), list.len() as i32));
        let at = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_beam_type"));
        grid.append(Property::enumeration("Type", "bt_type", list, at));
        self.type_rows = current;
        if let Some(t) = current
            .and_then(|c| store.get(c))
            .and_then(beams::beam_type)
        {
            self.section_rows(grid, &t);
        }
    }

    /// The palette while the Beam tool runs.
    pub(crate) fn beam_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.beam_tool.is_none() {
            return false;
        }
        grid.append(Property::category("Beam tool", "c_beamtool"));
        let current = self.tool_beam_type();
        self.beam_type_rows(grid, current);
        true
    }

    /// The rows of a selected beam; `false` if `id` is not one.
    pub(crate) fn beam_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = beams::beam_recipe(e) else {
            return false;
        };
        let info = self
            .run("beam.info", json!({ "beam_ids": [id] }))
            .ok()
            .map(|v| v[0].clone())
            .unwrap_or_default();
        grid.append(Property::category("Constraints", "c_beam"));
        let levels = reviera_core::levels::list(&store);
        let names: Vec<(String, i32)> = levels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i as i32))
            .collect();
        let at = levels
            .iter()
            .position(|l| {
                store
                    .get(l.id)
                    .is_some_and(|e| e.guid().to_string() == r.level)
            })
            .map_or(-1, |i| i as i32);
        grid.append(Property::enumeration(
            "Reference Level",
            "bm_level",
            names,
            at,
        ));
        let v = self.format_length(r.start_offset);
        grid.append(Property::string("Start Level Offset", "bm_start_off", v));
        let v = self.format_length(r.end_offset);
        grid.append(Property::string("End Level Offset", "bm_end_off", v));
        grid.append(Property::enumeration(
            "Orientation",
            "bm_orientation",
            choices(&ORIENTATIONS),
            index(&ORIENTATIONS, &r.orientation),
        ));
        let degrees = format!("{:.1}°", r.rotation.to_degrees());
        grid.append(Property::string(
            "Cross-Section Rotation",
            "bm_rotation",
            degrees,
        ));
        grid.append(Property::category("Geometric Position", "c_beam_pos"));
        grid.append(Property::enumeration(
            "y Justification",
            "bm_yj",
            choices(&Y_JUST),
            index(&Y_JUST, &r.y_justification),
        ));
        let v = self.format_length(r.y_offset);
        grid.append(Property::string("y Offset Value", "bm_yo", v));
        grid.append(Property::enumeration(
            "z Justification",
            "bm_zj",
            choices(&Z_JUST),
            index(&Z_JUST, &r.z_justification),
        ));
        let v = self.format_length(r.z_offset);
        grid.append(Property::string("z Offset Value", "bm_zo", v));
        let v = self.format_length(r.start_extension);
        grid.append(Property::string("Start Extension", "bm_start_ext", v));
        let v = self.format_length(r.end_extension);
        grid.append(Property::string("End Extension", "bm_end_ext", v));
        grid.append(Property::category("Structural", "c_beam_str"));
        // Automatic shows what it resolves to.
        let mut usages = choices(&USAGES);
        if r.usage == Usage::Automatic {
            let resolved = USAGES
                .iter()
                .find(|(_, u)| serde_json::to_value(u).ok().as_ref() == Some(&info["usage"]))
                .map_or("", |(n, _)| *n);
            usages[0].0 = format!("Automatic ({resolved})");
        }
        grid.append(Property::enumeration(
            "Structural Usage",
            "bm_usage",
            usages,
            index(&USAGES, &r.usage),
        ));
        let supports = |k: usize| {
            info["supports"][k][0].as_str().map_or("—".to_owned(), |s| {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            })
        };
        let s = format!("{} / {}", supports(0), supports(1));
        grid.append(Property::string(
            "Supported At Start / End",
            "bm_supports",
            s,
        ));
        grid.set_property_read_only("bm_supports", true);
        let v = self.format_length(info["length"].as_f64().unwrap_or(0.0));
        grid.append(Property::string("Cut Length", "bm_length", v));
        grid.set_property_read_only("bm_length", true);
        let current = e
            .param("@type")
            .and_then(serde_json::Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        self.beam_type_rows(grid, current);
        true
    }

    fn selected_beams(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).and_then(beams::beam_recipe).is_some())
            .collect()
    }

    /// An edit of a beam row; `false` if `property` is not one.
    pub(crate) fn beam_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("bm_") && property != "bt_type" {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected = self.selected_beams();
        let Some(store) = self.store_now() else {
            return true;
        };
        let set = |g: &mut Gui, mut args: serde_json::Value| -> Result<(), String> {
            args["beam_ids"] = json!(selected);
            g.run("beam.set", args).map(|_| ())
        };
        let pick = |list: Vec<(&str, serde_json::Value)>| {
            list.into_iter()
                .find(|(n, _)| text.starts_with(n))
                .map(|(_, v)| v)
        };
        let result: Option<Result<(), String>> = match property {
            "bm_level" => reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == text)
                .map(|l| set(self, json!({ "reference_level": l.id }))),
            "bm_start_off" => Some(set(self, json!({ "start_offset": text }))),
            "bm_end_off" => Some(set(self, json!({ "end_offset": text }))),
            "bm_yo" => Some(set(self, json!({ "y_offset": text }))),
            "bm_zo" => Some(set(self, json!({ "z_offset": text }))),
            "bm_start_ext" => Some(set(self, json!({ "start_extension": text }))),
            "bm_end_ext" => Some(set(self, json!({ "end_extension": text }))),
            "bm_rotation" => {
                let t = if text.trim().ends_with('°') || text.contains("deg") {
                    text.clone()
                } else {
                    format!("{}°", text.trim())
                };
                Some(set(self, json!({ "rotation": t })))
            }
            "bm_orientation" => pick(ORIENTATIONS.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "orientation": v }))),
            "bm_yj" => pick(Y_JUST.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "y_justification": v }))),
            "bm_zj" => pick(Z_JUST.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "z_justification": v }))),
            "bm_usage" => pick(USAGES.iter().map(|(n, v)| (*n, json!(v))).collect()).map(|v| {
                self.run(
                    "beam.set_usage",
                    json!({ "beam_ids": selected, "usage": v }),
                )
                .map(|_| ())
            }),
            "bt_type" => self.choose_beam_type(&text, &selected),
            _ => None,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    fn choose_beam_type(
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
                .find(|e| beams::beam_type(e).is_some() && e.name.as_deref() == Some(choice))?
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

    /// Structure › Beams on Grids: a beam in each bay between columns along the selected grid
    /// lines, at the plan's level.
    pub(crate) fn beams_on_grids(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let grids: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| store.get(*id).is_some_and(reviera_core::grids::is_grid))
            .collect();
        if grids.is_empty() {
            return frame.set_status_text(
                "Select the grid lines first (drag a window or Ctrl+click), then Beams on Grids",
                0,
            );
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Beams on Grids works in plan views", 0);
        };
        let mut args = json!({ "grid_ids": grids, "level": level });
        if let Some(t) = self.tool_beam_type() {
            args["type_id"] = json!(t);
        }
        match self.run("beam.create_on_grids", args) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} beams placed between columns"), 0);
                self.clear_selection();
                self.refresh_scene(&frame);
            }
            Err(e) => error(&frame, &format!("The beams were not placed:\n{e}")),
        }
    }
}
