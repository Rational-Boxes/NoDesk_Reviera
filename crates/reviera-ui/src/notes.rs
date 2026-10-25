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

impl Gui {
    pub(crate) fn start_note_tool(&mut self, kind: NoteTool) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
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
            },
            0,
        );
    }

    pub(crate) fn end_note_tool(&mut self) -> bool {
        self.note_tool.take().is_some()
    }

    /// The active plan's element id.
    fn active_plan(&self) -> Option<ElementId> {
        self.active_view()
            .filter(|v| v.plan.is_some())
            .and_then(|v| v.element)
    }

    /// A click with a note tool.
    pub(crate) fn note_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let (Some(kind), Some(plan), Some(p)) =
            (self.note_tool, self.active_plan(), self.plan_xy(x, y))
        else {
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
            Ok(_) => self.notes_changed(),
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
        if !self.ensure_plan(false) {
            return;
        }
        let Some(plan) = self.active_plan() else {
            return;
        };
        let labels: Vec<&str> = TAG_ALL.iter().map(|c| c.1).collect();
        let dialog = MultiChoiceDialog::builder(
            &frame,
            "Tag every element of these categories on the plan's level that has no tag yet:",
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

    /// Whether an element is a tag or text note.
    pub(crate) fn is_note(&self, id: ElementId) -> bool {
        self.doc
            .and_then(|d| self.app.document(d))
            .and_then(|d| d.doc.store().get(id))
            .is_some_and(|e| notes::tag_data(e).is_some() || notes::text_data(e).is_some())
    }

    /// The tags and notes of view `index`, as drawn.
    fn view_notes(&self, index: usize) -> Vec<reviera_app::notes::NoteGraphic> {
        let v = &self.views[index];
        let (Some(element), Some(plan)) = (v.element, v.plan.as_ref()) else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store()) else {
            return Vec::new();
        };
        let Some(view) = store.get(element) else {
            return Vec::new();
        };
        reviera_app::notes::view_notes(store, view, plan.scale, plan.range.cut as f32 + 0.002)
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
        false
    }

    /// An edit of a tag or text note row; `false` if `property` is not one.
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
            _ => return false,
        };
        if let Err(e) = result {
            error(&frame, &format!("The annotation was not changed:\n{e}"));
        }
        self.notes_changed();
        true
    }
}
