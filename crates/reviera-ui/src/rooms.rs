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
    /// The Space tool (MEP-SPACE-001): clicks place MEP spaces, not rooms.
    space: bool,
}

impl Gui {
    /// Architecture › Room.
    pub(crate) fn start_room_tool(&mut self) {
        self.start_room_tool_as(false);
    }

    /// Analyze › Spaces & Zones › Space (MEP-SPACE-001).
    pub(crate) fn start_space_tool(&mut self) {
        self.start_room_tool_as(true);
    }

    fn start_room_tool_as(&mut self, space: bool) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return frame.set_status_text("Rooms are placed in plan views (View › Floor Plan)", 0);
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_line_tool();
        self.end_insert_tool();
        self.end_modify();
        self.cancel_sketch();
        self.end_ceiling_tool();
        self.leave_walk();
        self.room_tool = Some(RoomTool { hover: Vec::new(), space });
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        frame.set_status_text(
            if space {
                "Space: click inside a region enclosed by walls (or Place Spaces Automatically); Esc ends"
            } else {
                "Room: click inside a region enclosed by walls (or Place Rooms Automatically); Esc ends"
            },
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
        let space = self.room_tool.as_ref().is_some_and(|t| t.space);
        let (command, what) = if space { ("space.place", "Space") } else { ("room.place", "Room") };
        match self.app.execute(
            command,
            json!({ "level_id": plan.level, "point": p }),
            &super::user(),
        ) {
            Ok(out) => {
                let note = out.warnings.first().map_or(
                    format!("{what} placed; click another region, or Esc"),
                    |w| format!("{what} placed — {}", w.message),
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

    /// Analyze › Spaces & Zones › Place Spaces Automatically on the plan's level.
    pub(crate) fn place_spaces_automatically(&mut self) {
        let frame = self.frame;
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return frame.set_status_text("Spaces are placed in plan views", 0);
        };
        match self.run("space.place_auto", json!({ "level_id": plan.level })) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} space{} placed", if n == 1 { "" } else { "s" }), 0);
                self.after_room_change();
            }
            Err(e) => error(&frame, &format!("Spaces were not placed:\n{e}")),
        }
    }

    /// Analyze › Spaces & Zones › Space Naming: every space takes the number and name of the room
    /// at its location.
    pub(crate) fn name_spaces_from_rooms(&mut self) {
        let frame = self.frame;
        match self.run("space.name_from_rooms", json!({})) {
            Ok(v) => frame.set_status_text(&format!("{} space(s) named from rooms", v.as_array().map_or(0, Vec::len)), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Analyze › Spaces & Zones › Spaces: the spaces with their area, type and airflow; Edit
    /// changes one's properties and overrides, Type gives it a space type, Unplace keeps it
    /// without a location.
    pub(crate) fn spaces_dialog(&mut self) {
        const EDIT: i32 = 7401;
        const TYPE: i32 = 7402;
        const UNPLACE: i32 = 7403;
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("space.info", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|s| {
                    let placed = s["placed"].as_bool().unwrap_or(true);
                    vec![
                        s["number"].as_str().unwrap_or("").to_owned(),
                        s["name"].as_str().unwrap_or("").to_owned(),
                        if placed { format!("{:.2}", s["area"].as_f64().unwrap_or(0.0)) } else { "Not Placed".into() },
                        if s["occupiable"].as_bool() == Some(true) { "Yes".into() } else { "No".into() },
                        s["space_type"].as_str().unwrap_or("").to_owned(),
                        format!("{:.1}", s["actual_supply"].as_f64().unwrap_or(0.0) * 1000.0),
                    ]
                })
                .collect();
            let (code, at) = super::mep_loads_ui::list_dialog(
                &frame,
                "Spaces",
                "MEP spaces, separate from rooms: their loads, people and airflow. A space under a room is occupiable.",
                &[("Number", 70), ("Name", 140), ("Area m²", 80), ("Occupiable", 80), ("Space type", 110), ("Supply L/s", 80)],
                &rows,
                &[(EDIT, "Edit…", "Change its properties and override its type's values"), (TYPE, "Type…", "Give it a space type"), (UNPLACE, "Unplace", "Keep it, without a location")],
                selected,
            );
            selected = at;
            let Some(s) = at.and_then(|k| list.get(k)).cloned() else {
                if matches!(code, EDIT | TYPE | UNPLACE) {
                    continue;
                }
                break;
            };
            let id = s["id"].clone();
            let result = match code {
                EDIT => match self.space_form(&s) {
                    Some(args) => {
                        let mut args = args;
                        args["ids"] = json!([id]);
                        self.run("space.set", args).map(|_| ())
                    }
                    None => continue,
                },
                TYPE => {
                    let types = self.run("space_type.list", json!({ "which": "space" })).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
                    let mut labels = vec!["(none)".to_owned()];
                    labels.extend(types.iter().map(|t| t["name"].as_str().unwrap_or("").to_owned()));
                    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let d = SingleChoiceDialog::builder(&frame, "Space type:", "Space Type", &refs).build();
                    if d.show_modal() != ID_OK {
                        continue;
                    }
                    let k = usize::try_from(d.get_selection()).unwrap_or(0);
                    let ty = k.checked_sub(1).and_then(|i| types.get(i)).map(|t| t["id"].clone()).unwrap_or(serde_json::Value::Null);
                    self.run("space.set_type", json!({ "element_ids": [id], "type_id": ty })).map(|_| ())
                }
                UNPLACE => self.run("space.unplace", json!({ "ids": [id] })).map(|_| ()),
                _ => break,
            };
            if let Err(e) = result {
                error(&frame, &e);
            }
            self.after_room_change();
        }
    }

    /// A space's properties and overrides in a form: the `space.set` arguments, or none.
    fn space_form(&mut self, s: &serde_json::Value) -> Option<serde_json::Value> {
        const CONDITIONS: [(&str, &str); 6] = [
            ("Heated and cooled", "heated_and_cooled"),
            ("Heated", "heated"),
            ("Cooled", "cooled"),
            ("Unconditioned", "unconditioned"),
            ("Vented", "vented"),
            ("Naturally vented only", "naturally_vented_only"),
        ];
        let f = super::reinforcement_ui::Form::new(&self.frame, &format!("Space {}", s["number"].as_str().unwrap_or("")));
        let p = &s["props"];
        let number = f.text("Number", s["number"].as_str().unwrap_or(""), "");
        let name = f.text("Name", s["name"].as_str().unwrap_or(""), "");
        let labels: Vec<String> = CONDITIONS.iter().map(|c| c.0.to_owned()).collect();
        let at = CONDITIONS.iter().position(|c| Some(c.1) == p["condition"].as_str()).unwrap_or(0);
        let condition = f.choice("Condition", &labels, at);
        let plenum = f.check("Plenum", "Return-air plenum", p["return_air_plenum"].as_bool().unwrap_or(false));
        let num = |v: &serde_json::Value| v.as_f64().map_or(String::new(), |x| format!("{x}"));
        let refl = f.text("Reflectances", &format!("{}, {}, {}", num(&p["ceiling_reflectance"]), num(&p["wall_reflectance"]), num(&p["floor_reflectance"])), "Ceiling, wall and floor, 0 to 1");
        let workplane = f.text("Workplane height (m)", &num(&p["workplane_height"]), "Lighting calculation workplane above the floor");
        let supply = f.text("Specified supply (L/s)", &p["specified_supply"].as_f64().map_or(String::new(), |v| format!("{}", v * 1000.0)), "Blank: none");
        let over = &p["overrides"];
        let v = &s["values"];
        let lighting = f.text("Lighting load (W/m²)", &over["lighting_load_density"].as_f64().map_or(String::new(), |x| x.to_string()), &format!("Blank: the type's ({})", num(&v["lighting_load_density"])));
        let power = f.text("Power load (W/m²)", &over["power_load_density"].as_f64().map_or(String::new(), |x| x.to_string()), &format!("Blank: the type's ({})", num(&v["power_load_density"])));
        let people = f.text("Area per person (m²)", &over["area_per_person"].as_f64().map_or(String::new(), |x| x.to_string()), &format!("Blank: the type's ({})", num(&v["area_per_person"])));
        let ok = f.show("Blank load and people values follow the space type.", "OK");
        let parse = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let opt = |t: &TextCtrl| parse(t).map_or(serde_json::Value::Null, |x| json!(x));
        let r: Vec<f64> = refl.get_value().split(',').filter_map(|x| x.trim().parse().ok()).collect();
        let mut args = json!({
            "number": number.get_value().trim(),
            "name": name.get_value().trim(),
            "condition": CONDITIONS[condition.get_selection().map_or(0, |i| i as usize)].1,
            "return_air_plenum": plenum.get_value(),
            "specified_supply": parse(&supply).map_or(-1.0, |x| x / 1000.0),
            "overrides": { "lighting_load_density": opt(&lighting), "power_load_density": opt(&power), "area_per_person": opt(&people) },
        });
        if let [c, w, fl] = r[..] {
            args["ceiling_reflectance"] = json!(c);
            args["wall_reflectance"] = json!(w);
            args["floor_reflectance"] = json!(fl);
        }
        if let Some(h) = parse(&workplane) {
            args["workplane_height"] = json!(h);
        }
        f.dialog.destroy();
        ok.then_some(args)
    }
}
