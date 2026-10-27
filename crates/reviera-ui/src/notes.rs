// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Tags and text notes in plans (DOC-ANN-009…011):
//! - **Tools.** Tag by Category (TG) tags the element clicked, Room Tag (RT) the room clicked,
//!   and Text (TX) places a note where clicked. Tag All Not Tagged tags whole categories.
//! - **Drawing.** Tags and notes draw with the plan; orphaned tags in orange.
//! - **Editing.** They are selected by clicking and moved by dragging. Properties change a tag's
//!   type, leader and orientation, edit its value (the host's parameter), and edit a note's
//!   text, type, width and rotation.

use reviera_core::ElementId;
use reviera_core::notes::{self, LeaderMode};
use reviera_render::annotation::label_strokes;
use reviera_render::notes::outline_contains;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const NOTE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const ORPHAN_COLOR: [f32; 4] = [0.95, 0.45, 0.05, 1.0];

/// The tag and text tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTool {
    Tag,
    RoomTag,
    Text,
    /// Analyze › Color Fill › Duct Legend / Pipe Legend (MEP-DUCT-010).
    DuctLegend,
    PipeLegend,
    /// Analyze › Route Analysis › Path of Travel (ANA-ROUTE-001): a start, then an end.
    PathOfTravel,
    /// Annotate › Spot Elevation and Spot Coordinate (DOC-ANN-006/007): a point on the model,
    /// then where the text goes.
    SpotElevation,
    SpotCoordinate,
    /// Annotate › Spot Slope (DOC-ANN-008): one click on a sloped face.
    SpotSlope,
    /// Annotate › Keynote (DOC-ANN-014): the element, then where the keynote goes; a user
    /// keynote's key is chosen first.
    KeynoteElement,
    KeynoteUser,
}

/// Tag All Not Tagged's categories: key and label.
const TAG_ALL: &[(&str, &str)] = &[
    ("cat.doors", "Doors"),
    ("cat.windows", "Windows"),
    ("cat.rooms", "Rooms"),
    ("cat.walls", "Walls"),
];

/// Asks for a note's text in a multi-line box; `None` when cancelled.
pub(crate) fn ask_text(frame: &Frame, title: &str, current: &str) -> Option<String> {
    let dialog = Dialog::builder(frame, title)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(460, 280)
        .build();
    let text = TextCtrl::builder(&dialog)
        .with_style(TextCtrlStyle::MultiLine)
        .with_value(current)
        .build();
    let ok = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label("OK")
        .build();
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("Cancel")
        .build();
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    buttons.add(&cancel, 0, SizerFlag::All, 6);
    buttons.add(&ok, 0, SizerFlag::All, 6);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add(&text, 1, SizerFlag::Expand | SizerFlag::All, 8);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    dialog.set_sizer(sizer, true);
    dialog.set_affirmative_id(ID_OK);
    text.set_focus();
    let answer = (dialog.show_modal() == ID_OK).then(|| text.get_value());
    dialog.destroy();
    answer.filter(|t| !t.trim().is_empty())
}

impl NoteTool {
    /// Whether the tool works in sections and elevations too.
    fn in_sections(self) -> bool {
        matches!(
            self,
            NoteTool::Tag | NoteTool::Text | NoteTool::SpotElevation | NoteTool::SpotCoordinate | NoteTool::SpotSlope | NoteTool::KeynoteElement | NoteTool::KeynoteUser
        )
    }
}

impl Gui {
    /// The point of the model under pixel (`x`, `y`) of the active plan, section or elevation,
    /// with its element and the direction the view looks there: what the view shows (its range,
    /// clipping and hidden elements as for picking); in a plan, the element's top there.
    pub(crate) fn model_point_at(&self, x: i32, y: i32, panel: &Panel) -> Option<(Option<ElementId>, [f64; 3], [f64; 3])> {
        let v = self.active_view()?;
        let size = panel.get_client_size();
        let (origin, dir) = v.camera.ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let visibility = self.view_visibility(v);
        if let Some(s) = &v.section {
            let far = -(s.def.far as f32);
            let (i, t) = s.scene.pick_visible(origin, dir, &|p| p.z <= 1e-4 && p.z >= far, &visibility)?;
            let local = origin + dir * t;
            let frame = s.def.frame();
            let p = frame.to_world([f64::from(local.x), f64::from(local.y), f64::from(local.z)]);
            return Some((s.scene.meshes[i].element, p, [frame.look[0], frame.look[1], 0.0]));
        }
        let plan = v.plan.as_ref()?;
        let (bottom, top) = plan.drawn();
        let (b, slices) = self.view_clip(v);
        let clipping = reviera_render::Clipping::of(b.as_ref(), &slices);
        let keep = |p: glam::Vec3| (bottom..=top).contains(&p.z) && clipping.keeps(p);
        let scene = self.scenes.get(&v.doc)?;
        let (i, t) = scene.pick_visible(origin, dir, &keep, &visibility)?;
        let p = origin + dir * t;
        let element = scene.meshes[i].element;
        // A plan reads the top of what is there: a slab's top, the top of a wall it cuts.
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        let top = element.and_then(|e| reviera_app::spots::top_at(scene, e, x, y));
        Some((element, top.unwrap_or([x, y, f64::from(p.z)]), [0.0, 0.0, -1.0]))
    }

    pub(crate) fn start_note_tool(&mut self, kind: NoteTool) {
        let frame = self.frame;
        // Tags and text go in sections and elevations too.
        // Text goes in legends too.
        let in_legend = self.active_view().is_some_and(|v| v.legend.is_some());
        let in_section = self.active_view().is_some_and(|v| v.section.is_some());
        if !(in_section && kind.in_sections()) && !(in_legend && kind == NoteTool::Text) && !self.ensure_plan(false) {
            return;
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_line_tool();
        self.end_insert_tool();
        self.end_modify();
        self.cancel_sketch();
        self.end_ceiling_tool();
        self.end_room_tool();
        self.end_section_tool();
        self.leave_walk();
        // A user keynote's key, from the keynote table.
        if kind == NoteTool::KeynoteUser {
            let Some(key) = self.choose_keynote() else { return };
            self.keynote_key = Some(key);
        }
        self.note_tool = Some(kind);
        frame.set_status_text(
            match kind {
                NoteTool::Tag => {
                    "Tag by Category: click a door, window, room, wall or other element; Esc ends"
                }
                NoteTool::RoomTag => "Room Tag: click inside a room; Esc ends",
                NoteTool::Text => "Text: click where the text's top-left corner goes; Esc ends",
                NoteTool::DuctLegend => "Duct Legend: click where the legend's top-left corner goes",
                NoteTool::PipeLegend => "Pipe Legend: click where the legend's top-left corner goes",
                NoteTool::PathOfTravel => "Path of Travel: click the start point; Esc ends",
                NoteTool::SpotElevation => "Spot Elevation: click a point on an element; Esc ends",
                NoteTool::SpotCoordinate => "Spot Coordinate: click a point on an element; Esc ends",
                NoteTool::SpotSlope => "Spot Slope: click a sloped face; Esc ends",
                NoteTool::KeynoteElement => "Element Keynote: click an element; Esc ends",
                NoteTool::KeynoteUser => "User Keynote: click the element it is for; Esc ends",
            },
            0,
        );
    }

    pub(crate) fn end_note_tool(&mut self) -> bool {
        self.route_start = None;
        self.spot_pick = None;
        self.keynote_pick = None;
        self.note_tool.take().is_some()
    }

    /// The active plan's element id.
    fn active_plan(&self) -> Option<ElementId> {
        self.active_view()
            .filter(|v| v.plan.is_some())
            .and_then(|v| v.element)
    }

    /// The active plan's, section's, elevation's or legend's element id: the views tags and text
    /// go in.
    fn active_annotation_view(&self) -> Option<ElementId> {
        self.active_view()
            .filter(|v| v.plan.is_some() || v.section.is_some() || v.legend.is_some())
            .and_then(|v| v.element)
    }

    /// A click with a note tool.
    pub(crate) fn note_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let view = match self.note_tool {
            Some(k) if k.in_sections() => self.active_annotation_view(),
            _ => self.active_plan(),
        };
        let (Some(kind), Some(plan), Some(p)) = (self.note_tool, view, self.plan_xy(x, y)) else {
            return;
        };
        let result = match kind {
            NoteTool::Tag => {
                let hit = self
                    .room_label_at(x, y)
                    .or_else(|| self.element_at(x, y, panel))
                    .or_else(|| self.room_inside(x, y))
                    .filter(|h| !self.is_note(*h));
                let Some(host) = hit else {
                    return frame.set_status_text("Click an element to tag it", 0);
                };
                self.run("tag.create", json!({ "view_id": plan, "host_ref": host }))
            }
            NoteTool::RoomTag => {
                let Some(room) = self.room_label_at(x, y).or_else(|| self.room_inside(x, y)) else {
                    return frame.set_status_text("Click inside a room to tag it", 0);
                };
                self.run(
                    "tag.create_room",
                    json!({ "view_id": plan, "room_id": room, "position": p }),
                )
            }
            NoteTool::PathOfTravel => {
                self.path_click(p);
                return;
            }
            NoteTool::DuctLegend | NoteTool::PipeLegend => {
                let kind = if kind == NoteTool::DuctLegend { reviera_core::mep_color_fill::Kind::Duct } else { reviera_core::mep_color_fill::Kind::Pipe };
                self.end_note_tool();
                let Some(scheme) = self.choose_mep_scheme(kind) else {
                    return;
                };
                self.run(
                    "mep_color_legend.place",
                    json!({ "view_id": plan, "kind": kind, "scheme_id": scheme, "point": p }),
                )
            }
            NoteTool::KeynoteElement | NoteTool::KeynoteUser => {
                // First the element, then where the keynote goes.
                let Some(host) = self.keynote_pick.take() else {
                    let hit = self.element_at(x, y, panel).or_else(|| self.room_inside(x, y)).filter(|h| !self.is_note(*h));
                    let Some(host) = hit else {
                        return frame.set_status_text("Click an element", 0);
                    };
                    self.keynote_pick = Some(host);
                    return frame.set_status_text("Click where the keynote goes", 0);
                };
                let mut args = json!({ "view_id": plan, "host_ref": host, "position": p });
                if kind == NoteTool::KeynoteUser {
                    args["mode"] = json!("user");
                    args["key"] = json!(self.keynote_key.clone());
                }
                self.run("keynote.create", args)
            }
            NoteTool::SpotSlope => {
                let Some((element, mut point, direction)) = self.model_point_at(x, y, panel) else {
                    return frame.set_status_text("Click a sloped face of an element", 0);
                };
                // A section shows a slope as its cut profile: the point clicked on the cut plane.
                if let Some(s) = self.active_view().and_then(|v| v.section.as_ref()).filter(|s| !s.elevation) {
                    point = s.def.frame().to_world([p[0], p[1], 0.0]);
                }
                self.run("spot.create_slope", json!({ "view_id": plan, "ref": element, "point": point, "direction": direction }))
            }
            NoteTool::SpotElevation | NoteTool::SpotCoordinate => {
                // First the point on the model, then where the text goes.
                let Some((element, point, direction)) = self.spot_pick.take() else {
                    let Some(hit) = self.model_point_at(x, y, panel) else {
                        return frame.set_status_text("Click a point on an element", 0);
                    };
                    self.spot_pick = Some(hit);
                    return frame.set_status_text("Click where the text goes", 0);
                };
                let command = if kind == NoteTool::SpotElevation { "spot.create_elevation" } else { "spot.create_coordinate" };
                self.run(command, json!({ "view_id": plan, "ref": element, "point": point, "direction": direction, "leader_point": p }))
            }
            NoteTool::Text => {
                let Some(content) = ask_text(&frame, "Text", "") else {
                    return;
                };
                self.run(
                    "text.create",
                    json!({ "view_id": plan, "position": p, "content": content }),
                )
            }
        };
        match result {
            Ok(_) => {
                self.notes_changed();
                // The Spot tools go on: the next point.
                if matches!(kind, NoteTool::SpotElevation | NoteTool::SpotCoordinate | NoteTool::SpotSlope) {
                    frame.set_status_text("Spot placed; click a point on an element for the next, or Esc", 0);
                }
                if matches!(kind, NoteTool::KeynoteElement | NoteTool::KeynoteUser) {
                    frame.set_status_text("Keynote placed; click the next element, or Esc", 0);
                }
            }
            Err(e) => error(&frame, &format!("Nothing was placed:\n{e}")),
        }
    }

    /// Redraws the active model's plans and Properties after an annotation change.
    fn notes_changed(&mut self) {
        let frame = self.frame;
        self.refresh_scene(&frame);
    }

    /// Annotate › Tag All Not Tagged.
    pub(crate) fn tag_all(&mut self) {
        let frame = self.frame;
        let in_section = self.active_view().is_some_and(|v| v.section.is_some());
        if !in_section && !self.ensure_plan(false) {
            return;
        }
        let Some(plan) = self.active_annotation_view() else {
            return;
        };
        let labels: Vec<&str> = TAG_ALL.iter().map(|c| c.1).collect();
        let dialog = MultiChoiceDialog::builder(
            &frame,
            if in_section {
                "Tag every element of these categories the view shows that has no tag yet:"
            } else {
                "Tag every element of these categories on the plan's level that has no tag yet:"
            },
            "Tag All Not Tagged",
            &labels,
        )
        .build();
        dialog.set_selections(&(0..TAG_ALL.len() as i32).collect::<Vec<_>>());
        if dialog.show_modal() != ID_OK {
            return;
        }
        let categories: Vec<&str> = dialog
            .get_selections()
            .into_iter()
            .filter_map(|i| TAG_ALL.get(usize::try_from(i).ok()?).map(|c| c.0))
            .collect();
        match self.run(
            "tag.tag_all",
            json!({ "view_id": plan, "categories": categories }),
        ) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("Tagged {n} element(s)"), 0);
                self.notes_changed();
            }
            Err(e) => error(&frame, &format!("Nothing was tagged:\n{e}")),
        }
    }

    /// A key from the keynote table, chosen in a list (`None` when cancelled or there is none).
    fn choose_keynote(&mut self) -> Option<String> {
        let frame = self.frame;
        let table = self.run("keynotes.list", json!({})).ok()?;
        let entries = table["entries"].as_array().cloned().unwrap_or_default();
        if entries.is_empty() {
            frame.set_status_text("User Keynote: the project has no keynote table (Manage › Load Keynotes)", 0);
            return None;
        }
        let labels: Vec<String> = entries.iter().map(|k| format!("{} — {}", k["key"].as_str().unwrap_or(""), k["text"].as_str().unwrap_or(""))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Keynote:", "User Keynote", &refs).build();
        if d.show_modal() != ID_OK {
            return None;
        }
        let k = usize::try_from(d.get_selection()).ok().and_then(|i| entries.get(i))?;
        k["key"].as_str().map(str::to_owned)
    }

    /// Whether an element is a tag, text note, keynote or spot.
    pub(crate) fn is_note(&self, id: ElementId) -> bool {
        self.doc
            .and_then(|d| self.app.document(d))
            .and_then(|d| d.doc.store().get(id))
            .is_some_and(|e| {
                notes::tag_data(e).is_some()
                    || notes::text_data(e).is_some()
                    || reviera_core::spots::spot_data(e).is_some()
                    || reviera_core::keynote_tags::keynote_data(e).is_some()
            })
    }

    /// The tags and notes of view `index` (a plan, section or elevation), as drawn.
    pub(crate) fn view_notes(&self, index: usize) -> Vec<reviera_app::notes::NoteGraphic> {
        let v = &self.views[index];
        let (Some(element), Some((scale, z))) = (v.element, self.annotation_space(index)) else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store()) else {
            return Vec::new();
        };
        let Some(view) = store.get(element) else {
            return Vec::new();
        };
        reviera_app::notes::view_notes(store, view, scale, z + 0.002, self.scenes.get(&v.doc))
    }

    /// The lines of the tags and notes of view `index` (drawn with the plan).
    pub(crate) fn note_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let width = self.app.document(self.views[index].doc).map_or(0.18, |d| {
            reviera_core::standards::pen_set(d.doc.store()).annotation_width(1) as f32
        });
        let mut normal = Lines {
            color: NOTE_COLOR,
            width,
            segments: Vec::new(),
        };
        let mut orphaned = Lines {
            color: ORPHAN_COLOR,
            ..normal.clone()
        };
        let mut lit = Lines {
            color: reviera_render::HIGHLIGHT,
            ..normal.clone()
        };
        for n in self.view_notes(index) {
            let target = if selected.contains(&n.id) {
                &mut lit
            } else if n.orphaned {
                &mut orphaned
            } else {
                &mut normal
            };
            target.segments.extend(n.parts.segments);
            target.segments.extend(label_strokes(&n.parts.labels));
        }
        vec![normal, orphaned, lit]
    }

    /// The tag or text note under pixel (`x`, `y`) of the active plan.
    pub(crate) fn note_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let p = self.plan_xy(x, y)?;
        let p = [p[0] as f32, p[1] as f32];
        self.view_notes(index)
            .into_iter()
            .rev()
            .find(|n| outline_contains(&n.parts.outline, p))
            .map(|n| n.id)
    }

    /// Properties rows of a selected tag or text note; `false` when it is neither.
    pub(crate) fn note_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        if let Some(d) = notes::tag_data(e) {
            let host = reviera_core::GlobalId::parse(&d.host)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|h| store.get(h));
            let category = host
                .and_then(reviera_core::categories::category_of)
                .map_or("", |c| c.key);
            grid.append(Property::category("Tag", "c_tag"));
            let types = notes::tag_types_for(category);
            let names: Vec<(String, i32)> = types
                .iter()
                .enumerate()
                .map(|(i, t)| (t.name.to_owned(), i as i32))
                .collect();
            let index = types
                .iter()
                .position(|t| t.name == d.tag_type)
                .map_or(-1, |i| i as i32);
            grid.append(Property::enumeration("Tag Type", "tag_type", names, index));
            let leaders = [
                ("None", LeaderMode::None),
                ("Attached", LeaderMode::Attached),
                ("Free End", LeaderMode::Free),
            ];
            let leader = leaders.iter().position(|l| l.1 == d.leader).unwrap_or(0) as i32;
            grid.append(Property::enumeration(
                "Leader",
                "tag_leader",
                leaders
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (l.0.to_owned(), i as i32))
                    .collect::<Vec<(String, i32)>>(),
                leader,
            ));
            grid.append(Property::enumeration(
                "Orientation",
                "tag_orient",
                vec![("Horizontal".to_owned(), 0), ("Vertical".to_owned(), 1)],
                i32::from(d.vertical),
            ));
            // The value shown: editing it writes the host's parameter (DOC-ANN-010).
            let t = notes::tag_type(&d.tag_type);
            if let (Some(h), Some(t)) = (host, t) {
                let parents = std::collections::HashMap::new();
                let reader = reviera_core::schedules::FieldReader::new(&store, &parents);
                for (i, f) in t.fields.iter().enumerate() {
                    let cell = reader.cell(h, f);
                    let key = format!("tag_v{i}");
                    let label = match *f {
                        "param:elem.mark" => "Mark",
                        "room.name" => "Room Name",
                        "room.number" => "Room Number",
                        "room.area" => "Area",
                        "type" => "Type",
                        other => other,
                    };
                    grid.append(Property::string(label, &key, cell.text));
                    grid.set_property_read_only(&key, !cell.editable);
                }
            } else {
                grid.append(Property::string("Host", "tag_host", "Deleted (orphaned)"));
                grid.set_property_read_only("tag_host", true);
            }
            return true;
        }
        if let Some(d) = notes::text_data(e) {
            grid.append(Property::category("Text", "c_text"));
            grid.append(Property::string(
                "Text",
                "txt_content",
                d.content.replace('\n', "\\n"),
            ));
            grid.set_help_string("txt_content", "\\n starts a new line");
            let index = notes::TEXT_TYPES
                .iter()
                .position(|t| t.name == d.text_type)
                .map_or(-1, |i| i as i32);
            grid.append(Property::enumeration(
                "Type",
                "txt_type",
                notes::TEXT_TYPES
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.name.to_owned(), i as i32))
                    .collect::<Vec<(String, i32)>>(),
                index,
            ));
            grid.append(Property::string(
                "Width (paper mm)",
                "txt_width",
                d.width.map_or_else(|| "0".to_owned(), |w| format!("{w}")),
            ));
            grid.set_help_string(
                "txt_width",
                "The wrapping width on paper; 0 for unwrapped text",
            );
            grid.append(Property::string(
                "Rotation",
                "txt_rotation",
                format!("{:.0}°", d.rotation.to_degrees()),
            ));
            grid.append(Property::string(
                "Leaders",
                "txt_leaders",
                d.leaders.len().to_string(),
            ));
            grid.set_property_read_only("txt_leaders", true);
            return true;
        }
        if let Some(d) = reviera_core::keynote_tags::keynote_data(e) {
            use reviera_core::keynote_tags::{Mode, Style};
            let user = d.mode == Mode::User;
            grid.append(Property::category(if user { "User Keynote" } else { "Element Keynote" }, "c_keynote"));
            let k = reviera_core::keynote_tags::resolve(&store, id, d.clone());
            let value = match (&k.key, &k.text) {
                (Some(key), Some(text)) => format!("{key} — {text}"),
                (Some(key), None) => format!("{key} (not in the keynote table)"),
                _ => "? (the element's type has no keynote)".to_owned(),
            };
            grid.append(Property::string("Keynote", "kn_value", value));
            grid.set_property_read_only("kn_value", true);
            if user {
                grid.append(Property::string("Key", "kn_key", d.key.clone().unwrap_or_default()));
            }
            let styles = [Style::BoxedNumber, Style::Number, Style::Text];
            let names = ["Boxed Number", "Number", "Text"];
            let index = styles.iter().position(|s| *s == d.style).unwrap_or(0) as i32;
            grid.append(Property::enumeration("Style", "kn_style", names.iter().enumerate().map(|(i, n)| ((*n).to_owned(), i as i32)).collect::<Vec<(String, i32)>>(), index));
            grid.append(Property::enumeration("Leader", "kn_leader", vec![("None".to_owned(), 0), ("Attached".to_owned(), 1)], i32::from(d.leader)));
            return true;
        }
        if let Some(d) = reviera_core::spots::spot_data(e) {
            use reviera_core::spots::{Base, Display, Representation, SlopeFormat, SpotKind};
            let elevation = d.spot_kind == SpotKind::Elevation;
            let title = match d.spot_kind {
                SpotKind::Elevation => "Spot Elevation",
                SpotKind::Coordinate => "Spot Coordinate",
                SpotKind::Slope => "Spot Slope",
            };
            grid.append(Property::category(title, "c_spot"));
            // The value as it reads now, on the model.
            let scene = self.doc.and_then(|doc| self.scenes.get(&doc));
            let m = reviera_app::spots::measure(&store, scene, id, d.clone());
            let value = if m.orphaned { format!("{} (orphaned)", m.lines.join(", ")) } else { m.lines.join(", ") };
            grid.append(Property::string("Value", "spot_value", value));
            grid.set_property_read_only("spot_value", true);
            if d.spot_kind == SpotKind::Slope {
                let formats = [SlopeFormat::Percent, SlopeFormat::Ratio, SlopeFormat::Degrees];
                let names = ["Percent", "Ratio (1 : n)", "Degrees"];
                let index = formats.iter().position(|f| *f == d.slope_format).unwrap_or(0) as i32;
                grid.append(Property::enumeration("Slope Units", "spot_format", names.iter().enumerate().map(|(i, n)| ((*n).to_owned(), i as i32)).collect::<Vec<(String, i32)>>(), index));
                // The triangle is for sections and elevations.
                if reviera_core::sections::frame_of_view(&store, &d.view).is_some() {
                    let index = i32::from(d.representation == Representation::Triangle);
                    grid.append(Property::enumeration("Representation", "spot_repr", vec![("Arrow".to_owned(), 0), ("Triangle".to_owned(), 1)], index));
                }
                grid.append(Property::string("Prefix", "spot_prefix", d.prefix.clone()));
                grid.append(Property::string("Suffix", "spot_suffix", d.suffix.clone()));
                return true;
            }
            let mut bases: Vec<(String, Base)> = vec![("Project Base Point".into(), Base::Project), ("Survey Point".into(), Base::Survey)];
            if elevation {
                for l in reviera_core::levels::list(&store) {
                    bases.push((format!("Level: {}", l.name), Base::Level(l.guid.to_string())));
                }
            }
            let index = bases.iter().position(|(_, b)| *b == d.base).map_or(-1, |i| i as i32);
            grid.append(Property::enumeration("Relative Base", "spot_base", bases.iter().enumerate().map(|(i, (n, _))| (n.clone(), i as i32)).collect::<Vec<(String, i32)>>(), index));
            if elevation {
                let displays = [Display::Actual, Display::Top, Display::Bottom, Display::TopAndBottom];
                let names = ["Actual (Selected) Elevation", "Top Elevation", "Bottom Elevation", "Top & Bottom Elevations"];
                let index = displays.iter().position(|x| *x == d.display).map_or(0, |i| i as i32);
                grid.append(Property::enumeration("Display Elevations", "spot_display", names.iter().enumerate().map(|(i, n)| ((*n).to_owned(), i as i32)).collect::<Vec<(String, i32)>>(), index));
            } else {
                grid.append(Property::enumeration("Include Elevation", "spot_include", vec![("No".to_owned(), 0), ("Yes".to_owned(), 1)], i32::from(d.include_elevation)));
            }
            grid.append(Property::string("Prefix", "spot_prefix", d.prefix.clone()));
            grid.append(Property::string("Suffix", "spot_suffix", d.suffix.clone()));
            return true;
        }
        false
    }

    /// An edit of a tag, text note or spot row; `false` if `property` is not one.
    pub(crate) fn note_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let Some(&id) = self.selection().first() else {
            return false;
        };
        let text = text.unwrap_or_default();
        let result = match property {
            "tag_type" => self.run(
                "tag.set_type",
                json!({ "annotation_ids": [id], "tag_type_id": text }),
            ),
            "tag_orient" => {
                let current = self
                    .doc
                    .and_then(|d| self.app.document(d))
                    .and_then(|d| notes::tag_data(d.doc.store().get(id)?));
                let Some(d) = current else { return true };
                self.run(
                    "tag.set_type",
                    json!({
                        "annotation_ids": [id], "tag_type_id": d.tag_type,
                        "orientation": text.to_lowercase(),
                    }),
                )
            }
            "tag_leader" => {
                let mode = match text.as_str() {
                    "Attached" => "attached",
                    "Free End" => "free",
                    _ => "none",
                };
                self.run(
                    "tag.set_leader",
                    json!({ "annotation_id": id, "end_mode": mode }),
                )
            }
            p if p.starts_with("tag_v") => {
                let Some(store) = self
                    .doc
                    .and_then(|d| self.app.document(d))
                    .map(|d| d.doc.store().clone())
                else {
                    return true;
                };
                let Some(d) = store.get(id).and_then(notes::tag_data) else {
                    return true;
                };
                let field = p[5..]
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| notes::tag_type(&d.tag_type)?.fields.get(i).copied());
                let host =
                    reviera_core::GlobalId::parse(&d.host).and_then(|g| store.find_by_guid(&g));
                let (Some(field), Some(host)) = (field, host) else {
                    return true;
                };
                self.run(
                    "schedule.set_cell",
                    json!({ "element_ids": [host], "field": field, "value": text }),
                )
            }
            "txt_content" => self.run(
                "text.set_content",
                json!({ "annotation_id": id, "content": text.replace("\\n", "\n") }),
            ),
            "txt_type" => self.run(
                "text.set",
                json!({ "annotation_ids": [id], "type_id": text }),
            ),
            "txt_width" => match text.trim().trim_end_matches("mm").trim().parse::<f64>() {
                Ok(w) => self.run("text.set", json!({ "annotation_ids": [id], "width": w })),
                Err(_) => Err("the width is a number of paper millimetres".to_owned()),
            },
            "txt_rotation" => self.run(
                "text.set",
                json!({ "annotation_ids": [id], "rotation": text }),
            ),
            "spot_base" => {
                let base = match text.as_str() {
                    "Project Base Point" => json!("project"),
                    "Survey Point" => json!("survey"),
                    t => {
                        let Some(name) = t.strip_prefix("Level: ") else { return true };
                        let level = self
                            .doc
                            .and_then(|d| self.app.document(d))
                            .and_then(|d| reviera_core::levels::list(d.doc.store()).into_iter().find(|l| l.name == name));
                        let Some(l) = level else { return true };
                        json!({ "level": l.id })
                    }
                };
                self.run("spot.set", json!({ "annotation_ids": [id], "base": base }))
            }
            "spot_display" => {
                let display = match text.as_str() {
                    "Top Elevation" => "top",
                    "Bottom Elevation" => "bottom",
                    "Top & Bottom Elevations" => "top_and_bottom",
                    _ => "actual",
                };
                self.run("spot.set", json!({ "annotation_ids": [id], "display": display }))
            }
            "kn_key" => self.run("keynote.set", json!({ "annotation_ids": [id], "key": text })),
            "kn_style" => {
                let style = match text.as_str() {
                    "Number" => "number",
                    "Text" => "text",
                    _ => "boxed_number",
                };
                self.run("keynote.set", json!({ "annotation_ids": [id], "style": style }))
            }
            "kn_leader" => self.run("keynote.set", json!({ "annotation_ids": [id], "leader": text == "Attached" })),
            "spot_format" => {
                let format = match text.as_str() {
                    "Ratio (1 : n)" => "ratio",
                    "Degrees" => "degrees",
                    _ => "percent",
                };
                self.run("spot.set", json!({ "annotation_ids": [id], "slope_format": format }))
            }
            "spot_repr" => self.run("spot.set", json!({ "annotation_ids": [id], "representation": text.to_lowercase() })),
            "spot_include" => self.run("spot.set", json!({ "annotation_ids": [id], "include_elevation": text == "Yes" })),
            "spot_prefix" => self.run("spot.set", json!({ "annotation_ids": [id], "prefix": text })),
            "spot_suffix" => self.run("spot.set", json!({ "annotation_ids": [id], "suffix": text })),
            _ => return false,
        };
        if let Err(e) = result {
            error(&frame, &format!("The annotation was not changed:\n{e}"));
        }
        self.notes_changed();
        true
    }
}
