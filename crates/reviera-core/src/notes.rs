// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Tags and text notes (spec Ch 5 §5.13, DOC-ANN-009…011).
//!
//! Both are view annotation (see `annotations`): RXD-only elements of class
//! `rxd:views/annotation/<viewId>/<objectId>` with their data in `rxd:anno`.
//!
//! - **Tags** (`anno.tag`) label a host element with values read live from it: the fields of
//!   the tag type are schedule fields (`param:elem.mark`, `type`, `room.name`, `room.area`, …),
//!   so a tag and a schedule always agree. A tag keeps its head at an offset from its host's
//!   anchor (an insert's centre, a wall's midpoint, a room's tag point), so it follows the host.
//!   A tag whose host is gone is *orphaned*: it stays where it was (DOC-ANN-000).
//! - **Text notes** (`anno.text`) are free text at a plan position, with a text type (its size
//!   on paper), an optional wrapping width, a rotation and leaders.
//! - **Views:** both go in floor and ceiling plans, sections and elevations. Positions are in
//!   the view's 2D coordinates: plan x and y, or in a section or elevation the frame's x (to the
//!   viewer's right) and height; a tag's host is anchored through the frame there.
//!
//! Tag and text types are built in for now (tag families and the text type editor come later);
//! each category's default tag type can be changed (`tag.set_default_type`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::schedules::FieldReader;
use crate::store::Store;

pub const KIND_TAG: &str = "anno.tag";
pub const KIND_TEXT: &str = "anno.text";
/// The element holding the default tag type of each category.
pub const TAG_SETTINGS_CLASS: &str = "rxd:project/tags";
pub const TAG_SETTINGS_KEY: &str = "rxd:tags";

/// The outline drawn around a tag's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    None,
    Box,
    Hexagon,
}

/// A tag type: the categories it tags (any when empty), its fields (one text line each) and
/// its outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TagType {
    pub name: &'static str,
    pub categories: &'static [&'static str],
    pub fields: &'static [&'static str],
    pub frame: Frame,
}

pub const TAG_TYPES: &[TagType] = &[
    TagType {
        name: "Door Tag",
        categories: &["cat.doors"],
        fields: &["param:elem.mark"],
        frame: Frame::Box,
    },
    TagType {
        name: "Window Tag",
        categories: &["cat.windows"],
        fields: &["param:elem.mark"],
        frame: Frame::Hexagon,
    },
    TagType {
        name: "Room Tag",
        categories: &["cat.rooms"],
        fields: &["room.name", "room.number"],
        frame: Frame::None,
    },
    TagType {
        name: "Room Tag With Area",
        categories: &["cat.rooms"],
        fields: &["room.name", "room.number", "room.area"],
        frame: Frame::None,
    },
    TagType {
        name: "Wall Tag",
        categories: &["cat.walls"],
        fields: &["type"],
        frame: Frame::Box,
    },
    // Rebar sets (STR-RBR-018): number and "15 Ø12 @ 150".
    TagType {
        name: "Rebar Tag",
        categories: &["cat.structural_rebar"],
        fields: &["rebar.number", "rebar.description"],
        frame: Frame::Box,
    },
    TagType {
        name: "Multi-Category Tag",
        categories: &[],
        fields: &["param:elem.mark"],
        frame: Frame::Box,
    },
];

pub fn tag_type(name: &str) -> Option<&'static TagType> {
    TAG_TYPES.iter().find(|t| t.name == name)
}

/// Whether tag type `t` can tag elements of `category`.
pub fn tags_category(t: &TagType, category: &str) -> bool {
    t.categories.is_empty() || t.categories.contains(&category)
}

/// The tag types that can tag `category`.
pub fn tag_types_for(category: &str) -> Vec<&'static TagType> {
    TAG_TYPES
        .iter()
        .filter(|t| tags_category(t, category))
        .collect()
}

/// The default tag type of `category`: the project's choice, else the built-in one.
pub fn default_tag_type(store: &Store, category: &str) -> &'static TagType {
    let chosen = store
        .iter()
        .find(|e| e.class == TAG_SETTINGS_CLASS)
        .and_then(|e| e.param(TAG_SETTINGS_KEY)?.get(category)?.as_str())
        .and_then(tag_type);
    chosen.unwrap_or_else(|| {
        TAG_TYPES
            .iter()
            .find(|t| t.categories.contains(&category) && t.name != "Room Tag")
            .unwrap_or(&TAG_TYPES[TAG_TYPES.len() - 1])
    })
}

/// A text type: its name and its text height on paper (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct TextType {
    pub name: &'static str,
    pub size: f64,
}

pub const TEXT_TYPES: &[TextType] = &[
    TextType {
        name: "Text 2.5 mm",
        size: 2.5,
    },
    TextType {
        name: "Text 3.5 mm",
        size: 3.5,
    },
    TextType {
        name: "Text 5 mm",
        size: 5.0,
    },
];

pub fn text_type(name: &str) -> Option<&'static TextType> {
    TEXT_TYPES.iter().find(|t| t.name == name)
}

/// The text height of tags on paper (mm).
pub const TAG_TEXT_MM: f64 = 2.5;

/// How a tag's leader ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaderMode {
    /// No leader.
    #[default]
    None,
    /// The leader ends at the host and follows it.
    Attached,
    /// The leader ends at a fixed point.
    Free,
}

/// Stored data of a tag (`anno.tag`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagData {
    pub kind: String,
    /// The view (GlobalId) that owns the tag.
    pub view: String,
    /// The tagged element (GlobalId).
    pub host: String,
    pub tag_type: String,
    /// The tag head's centre relative to the host's anchor (plan, metres).
    pub offset: [f64; 2],
    /// The host's anchor when the tag was last placed: where an orphaned tag stays.
    pub anchor: [f64; 2],
    #[serde(default)]
    pub leader: LeaderMode,
    /// The end of a free leader (plan, metres).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_end: Option<[f64; 2]>,
    /// Text reads upward instead of left to right.
    #[serde(default)]
    pub vertical: bool,
}

/// The side of a text note a leader leaves from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// Where on that side a leader is attached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Attach {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// A text note's leader: from its side of the text through `points`, ending in an arrow at the
/// last one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextLeader {
    pub side: Side,
    pub attach: Attach,
    pub points: Vec<[f64; 2]>,
}

/// Stored data of a text note (`anno.text`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    pub kind: String,
    pub view: String,
    /// The top-left corner of the text (plan, metres).
    pub position: [f64; 2],
    /// Counter-clockwise, radians.
    #[serde(default)]
    pub rotation: f64,
    /// The wrapping width on paper (mm); `None` for unwrapped text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Plain text; lines are separated by `\n`.
    pub content: String,
    pub text_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<TextLeader>,
}

fn anno<T: serde::de::DeserializeOwned>(e: &Element, kind: &str) -> Option<T> {
    if !is_annotation(e) {
        return None;
    }
    let v = e.param(ANNO_KEY)?;
    if v.get("kind")?.as_str()? != kind {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

pub fn tag_data(e: &Element) -> Option<TagData> {
    anno(e, KIND_TAG)
}

pub fn text_data(e: &Element) -> Option<TextData> {
    anno(e, KIND_TEXT)
}

/// Where a host's tag hangs from: a door's or window's centre, a wall's midpoint, a room's tag
/// point. `None` for elements Reviera does not model (their tags keep their place).
pub fn host_anchor(store: &Store, host: &Element) -> Option<[f64; 2]> {
    if let Ok(p) = crate::inserts::resolve(store, host) {
        let c = p.world([0.0, 0.0, 0.0]);
        return Some([c[0], c[1]]);
    }
    if let Some(r) = crate::walls::wall_recipe(host) {
        let c = r.curve;
        return Some([(c.start[0] + c.end[0]) / 2.0, (c.start[1] + c.end[1]) / 2.0]);
    }
    if crate::rooms::room_recipe(host).is_some() {
        return crate::rooms::regenerate(store, host)
            .ok()
            .map(|g| g.tag_point);
    }
    if let Some(a) = crate::rebar_docs::anchor(store, host) {
        return Some(a);
    }
    None
}

/// The host's point in the model a tag in a section or elevation hangs from: a door's or
/// window's centre, a wall's middle at half its height, a room's tag point 1.2 m above its
/// level (EXPERT_REVIEW E-43).
pub fn host_point(store: &Store, host: &Element) -> Option<[f64; 3]> {
    if let Ok(p) = crate::inserts::resolve(store, host) {
        return Some(p.world([0.0, 0.0, 0.0]));
    }
    if crate::walls::wall_recipe(host).is_some() {
        let g = crate::walls::regenerate(store, host).ok()?;
        return Some([(g.start[0] + g.end[0]) / 2.0, (g.start[1] + g.end[1]) / 2.0, g.base_z + g.height / 2.0]);
    }
    if let Some(r) = crate::rooms::room_recipe(host) {
        let z = crate::levels::elevation_of(store, &r.level).ok()?;
        let p = crate::rooms::regenerate(store, host).ok()?.tag_point;
        return Some([p[0], p[1], z + 1.2]);
    }
    None
}

/// Where a host's tag hangs from in view `view` (GlobalId): in plan ([`host_anchor`]), or its
/// [`host_point`] in a section's or elevation's frame.
pub fn host_anchor_in(store: &Store, host: &Element, view: &str) -> Option<[f64; 2]> {
    match crate::sections::frame_of_view(store, view) {
        Some((frame, _)) => host_point(store, host).map(|p| {
            let q = frame.to_local(p);
            [q[0], q[1]]
        }),
        None => host_anchor(store, host),
    }
}

/// A tag as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedTag {
    pub id: ElementId,
    pub data: TagData,
    pub host: Option<ElementId>,
    /// The host's anchor now (or the stored one when orphaned).
    pub anchor: [f64; 2],
    /// The centre of the tag's head.
    pub head: [f64; 2],
    /// The text, one line per field of the tag type.
    pub lines: Vec<String>,
    pub frame: Frame,
    pub orphaned: bool,
}

/// The text a tag type shows for `host`.
pub fn tag_lines(reader: &FieldReader<'_>, host: &Element, t: &TagType) -> Vec<String> {
    let lines: Vec<String> = t.fields.iter().map(|f| reader.cell(host, f).text).collect();
    if lines.iter().all(String::is_empty) {
        vec!["?".into()]
    } else {
        lines
    }
}

/// Resolves one tag against the model.
pub fn resolve_tag(
    store: &Store,
    reader: &FieldReader<'_>,
    id: ElementId,
    data: TagData,
) -> ResolvedTag {
    let host = GlobalId::parse(&data.host).and_then(|g| store.find_by_guid(&g));
    let host_e = host.and_then(|h| store.get(h));
    let t = tag_type(&data.tag_type).unwrap_or(&TAG_TYPES[TAG_TYPES.len() - 1]);
    let anchor = host_e
        .and_then(|h| host_anchor_in(store, h, &data.view))
        .unwrap_or(data.anchor);
    let head = [anchor[0] + data.offset[0], anchor[1] + data.offset[1]];
    let lines = match host_e {
        Some(h) => tag_lines(reader, h, t),
        None => vec!["?".into()],
    };
    ResolvedTag {
        id,
        host,
        anchor,
        head,
        lines,
        frame: t.frame,
        orphaned: host_e.is_none(),
        data,
    }
}

/// The tags of a view (by its GlobalId).
pub fn tags_of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, TagData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = tag_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

/// The text notes of a view (by its GlobalId).
pub fn texts_of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, TextData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = text_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

/// A view that can hold tags and text: a floor or ceiling plan, a section or an elevation. Its
/// GlobalId, level (GlobalId; plans) and scale.
fn plan_of(store: &Store, r: &ElementRef) -> Result<(String, Option<String>, f64)> {
    let (id, data) = crate::views::resolve_view(store, r)?;
    if !crate::plans::is_plan(&data.kind) && !crate::sections::is_section(&data.kind) {
        return Err(Error::new(
            "anno.view_not_supported",
            "tags and text notes are placed in plans, sections and elevations (sheets come later)",
        ));
    }
    let e = store.get(id).expect("resolved");
    let scale = e
        .param(crate::views::VIEW_KEY)
        .and_then(|v| v.get("scale")?.as_f64())
        .unwrap_or(100.0);
    Ok((e.guid().to_string(), data.level, scale))
}

/// Whether an element shows in a section's or elevation's depth: its tag point between the view
/// plane and the far clip, within the view's width.
fn in_section(store: &Store, e: &Element, frame: &crate::sections::SectionFrame) -> bool {
    host_point(store, e).is_some_and(|p| {
        let q = frame.to_local(p);
        (-1e-6..=frame.width + 1e-6).contains(&q[0]) && q[2] <= 1e-6 && q[2] >= -frame.far
    })
}

/// Whether a Reviera element stands on level `level` (GlobalId): walls by their base level,
/// doors and windows by their wall's, rooms by theirs.
fn on_level(store: &Store, e: &Element, level: &str) -> bool {
    if let Some(r) = crate::walls::wall_recipe(e) {
        return r.base_level == level;
    }
    if let Some(r) = crate::rooms::room_recipe(e) {
        return r.level == level;
    }
    if let Some(r) = crate::inserts::insert_recipe(e) {
        return GlobalId::parse(&r.host)
            .and_then(|g| store.get(store.find_by_guid(&g)?))
            .and_then(crate::walls::wall_recipe)
            .is_some_and(|w| w.base_level == level);
    }
    false
}

fn create_annotation(
    txn: &mut crate::txn::Transaction<'_>,
    view: &str,
    value: serde_json::Value,
) -> Result<ElementId> {
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{view}/{}", guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, value),
    )
}

fn write_anno<T: Serialize>(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    data: &T,
) -> Result<()> {
    let value = serde_json::to_value(data).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(ANNO_KEY.into(), value);
    })
}

fn resolve_tag_ref(store: &Store, r: &ElementRef) -> Result<(ElementId, TagData)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("tag.not_found", "no such tag"))?;
    let d = tag_data(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("tag.not_found", format!("element {id} is not a tag")))?;
    Ok((id, d))
}

fn resolve_text_ref(store: &Store, r: &ElementRef) -> Result<(ElementId, TextData)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("text.not_found", "no such text note"))?;
    let d = text_data(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("text.not_found", format!("element {id} is not a text note")))?;
    Ok((id, d))
}

// ---- Tag commands ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTagParams {
    /// A floor or ceiling plan, a section or an elevation.
    pub view_id: ElementRef,
    /// The element to tag.
    pub host_ref: ElementRef,
    /// The centre of the tag (plan, metres); default on the host (beside it for walls).
    pub position: Option<[f64; 2]>,
    /// A tag type name (`Door Tag`, `Room Tag`, …); default the category's default tag type.
    pub tag_type_id: Option<String>,
    pub orientation: Option<Orientation>,
    /// A leader attached to the host (default off).
    pub leader: Option<bool>,
}

/// One tag: the host's category picks or checks the tag type.
#[allow(clippy::too_many_arguments)]
fn new_tag(
    store: &Store,
    view: &str,
    scale: f64,
    host: &Element,
    position: Option<[f64; 2]>,
    tag_type_id: Option<&str>,
    orientation: Option<Orientation>,
    leader: Option<bool>,
) -> Result<TagData> {
    let category = crate::categories::category_of(host)
        .filter(|_| !crate::categories::is_type(host))
        .ok_or_else(|| {
            Error::new(
                "tag.not_taggable",
                format!("element {} cannot be tagged", host.id()),
            )
        })?;
    let t = match tag_type_id {
        Some(name) => {
            let t = tag_type(name)
                .ok_or_else(|| Error::new("tag.unknown_type", format!("no tag type `{name}`")))?;
            if !tags_category(t, category.key) {
                return Err(Error::new(
                    "tag.wrong_category",
                    format!("a {} cannot tag {}", t.name, category.name.to_lowercase()),
                ));
            }
            t
        }
        None => default_tag_type(store, category.key),
    };
    let anchor = match (host_anchor_in(store, host, view), position) {
        (Some(a), _) => a,
        (None, Some(p)) => p,
        (None, None) => {
            return Err(Error::new(
                "tag.no_position",
                "this element needs a position for its tag",
            ));
        }
    };
    let offset = match position {
        Some(p) => [p[0] - anchor[0], p[1] - anchor[1]],
        // A wall's tag sits beside the wall, clear of it on paper.
        None => match crate::walls::wall_recipe(host).filter(|_| crate::sections::frame_of_view(store, view).is_none()) {
            Some(r) => {
                let (s, e) = (r.curve.start, r.curve.end);
                let len = (e[0] - s[0]).hypot(e[1] - s[1]).max(f64::EPSILON);
                let n = [-(e[1] - s[1]) / len, (e[0] - s[0]) / len];
                let d = 6.0 * scale / 1000.0;
                [n[0] * d, n[1] * d]
            }
            None => [0.0, 0.0],
        },
    };
    Ok(TagData {
        kind: KIND_TAG.into(),
        view: view.to_owned(),
        host: host.guid().to_string(),
        tag_type: t.name.to_owned(),
        offset,
        anchor,
        leader: if leader.unwrap_or(false) {
            LeaderMode::Attached
        } else {
            LeaderMode::None
        },
        leader_end: None,
        vertical: orientation == Some(Orientation::Vertical),
    })
}

pub(crate) fn create_tag(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateTagParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view, _, scale) = plan_of(&store, &p.view_id)?;
    let host = store
        .resolve(&p.host_ref)
        .and_then(|h| store.get(h))
        .ok_or_else(|| Error::new("element.not_found", "no such element to tag"))?;
    let data = new_tag(
        &store,
        &view,
        scale,
        host,
        p.position,
        p.tag_type_id.as_deref(),
        p.orientation,
        p.leader,
    )?;
    create_annotation(txn, &view, serde_json::to_value(&data).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRoomTagParams {
    pub view_id: ElementRef,
    pub room_id: ElementRef,
    pub position: Option<[f64; 2]>,
    /// `Room Tag` or `Room Tag With Area`; default the rooms' default tag type.
    pub tag_type_id: Option<String>,
}

pub(crate) fn create_room_tag(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateRoomTagParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let room = store
        .resolve(&p.room_id)
        .and_then(|r| store.get(r))
        .ok_or_else(|| Error::new("room.not_found", "no such room"))?;
    if crate::categories::category_of(room).is_none_or(|c| c.key != "cat.rooms") {
        return Err(Error::new(
            "room.not_found",
            format!("element {} is not a room", room.id()),
        ));
    }
    create_tag(
        txn,
        CreateTagParams {
            view_id: p.view_id,
            host_ref: p.room_id,
            position: p.position,
            tag_type_id: p.tag_type_id,
            orientation: None,
            leader: None,
        },
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagScope {
    /// Every element of the categories on the plan's level.
    #[default]
    View,
    /// The elements in `element_ids`.
    Selection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagAllParams {
    pub view_id: ElementRef,
    /// Category keys (`cat.doors`, `cat.windows`, `cat.rooms`, `cat.walls`, …).
    pub categories: Vec<String>,
    #[serde(default)]
    pub scope: TagScope,
    /// The selection, for scope `selection`.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    pub leader: Option<bool>,
    pub orientation: Option<Orientation>,
}

/// Tag All Not Tagged: tags each element of the categories that has no tag of its default tag
/// type in the view yet.
pub(crate) fn tag_all(
    txn: &mut crate::txn::Transaction<'_>,
    p: TagAllParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (view, level, scale) = plan_of(&store, &p.view_id)?;
    for c in &p.categories {
        if crate::categories::category(c).is_none() {
            return Err(Error::new(
                "tag.unknown_category",
                format!("no category `{c}`"),
            ));
        }
    }
    let frame = crate::sections::frame_of_view(&store, &view).map(|(f, _)| f);
    let candidates: Vec<&Element> = match p.scope {
        // In a section or elevation, what shows in its depth.
        TagScope::View => store
            .iter()
            .filter(|e| match &frame {
                Some(f) => in_section(&store, e, f),
                None => level.as_deref().is_some_and(|l| on_level(&store, e, l)),
            })
            .collect(),
        TagScope::Selection => p
            .element_ids
            .iter()
            .filter_map(|r| store.get(store.resolve(r)?))
            .collect(),
    };
    let existing: Vec<(String, String)> = tags_of_view(&store, &view)
        .into_iter()
        .map(|(_, d)| (d.host, d.tag_type))
        .collect();
    let mut out = Vec::new();
    for e in candidates {
        let Some(category) = crate::categories::category_of(e) else {
            continue;
        };
        if crate::categories::is_type(e) || !p.categories.iter().any(|c| c == category.key) {
            continue;
        }
        let t = default_tag_type(&store, category.key);
        let host = e.guid().to_string();
        if existing.iter().any(|(h, ty)| *h == host && ty == t.name) {
            continue;
        }
        let Ok(data) = new_tag(&store, &view, scale, e, None, None, p.orientation, p.leader) else {
            continue;
        };
        out.push(create_annotation(
            txn,
            &view,
            serde_json::to_value(&data).expect("serialises"),
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLeaderParams {
    pub annotation_id: ElementRef,
    /// `none`, `attached` (ends at the host and follows it) or `free` (ends at `points`' last).
    pub end_mode: LeaderMode,
    /// The end of a free leader (plan, metres); default where the host is now.
    pub points: Option<Vec<[f64; 2]>>,
}

pub(crate) fn set_leader(txn: &mut crate::txn::Transaction<'_>, p: SetLeaderParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut d) = resolve_tag_ref(&store, &p.annotation_id)?;
    d.leader = p.end_mode;
    d.leader_end =
        match p.end_mode {
            LeaderMode::Free => Some(p.points.and_then(|ps| ps.last().copied()).unwrap_or_else(
                || {
                    GlobalId::parse(&d.host)
                        .and_then(|g| store.get(store.find_by_guid(&g)?))
                        .and_then(|h| host_anchor_in(&store, h, &d.view))
                        .unwrap_or(d.anchor)
                },
            )),
            _ => None,
        };
    write_anno(txn, id, &d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTagTypeParams {
    pub annotation_ids: Vec<ElementRef>,
    pub tag_type_id: String,
    pub orientation: Option<Orientation>,
}

/// Changes tags' type (it must tag their hosts' category) and orientation.
pub(crate) fn set_tag_type(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetTagTypeParams,
) -> Result<()> {
    let t = tag_type(&p.tag_type_id).ok_or_else(|| {
        Error::new(
            "tag.unknown_type",
            format!("no tag type `{}`", p.tag_type_id),
        )
    })?;
    for r in &p.annotation_ids {
        let store = txn.store().clone();
        let (id, mut d) = resolve_tag_ref(&store, r)?;
        let category = GlobalId::parse(&d.host)
            .and_then(|g| store.get(store.find_by_guid(&g)?))
            .and_then(crate::categories::category_of);
        if let Some(c) = category
            && !tags_category(t, c.key)
        {
            return Err(Error::new(
                "tag.wrong_category",
                format!("a {} cannot tag {}", t.name, c.name.to_lowercase()),
            ));
        }
        d.tag_type = t.name.to_owned();
        if let Some(o) = p.orientation {
            d.vertical = o == Orientation::Vertical;
        }
        write_anno(txn, id, &d)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDefaultTypeParams {
    pub category: String,
    pub tag_type_id: String,
}

/// Loaded Tags: the tag type Tag by Category and Tag All use for a category.
pub(crate) fn set_default_type(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetDefaultTypeParams,
) -> Result<()> {
    let t = tag_type(&p.tag_type_id).ok_or_else(|| {
        Error::new(
            "tag.unknown_type",
            format!("no tag type `{}`", p.tag_type_id),
        )
    })?;
    if crate::categories::category(&p.category).is_none() || !tags_category(t, &p.category) {
        return Err(Error::new(
            "tag.wrong_category",
            format!("a {} cannot tag `{}`", t.name, p.category),
        ));
    }
    let existing = txn
        .store()
        .iter()
        .find(|e| e.class == TAG_SETTINGS_CLASS)
        .map(|e| (e.id(), e.param(TAG_SETTINGS_KEY).cloned()));
    let mut map: BTreeMap<String, String> = existing
        .as_ref()
        .and_then(|(_, v)| serde_json::from_value(v.clone()?).ok())
        .unwrap_or_default();
    map.insert(p.category, t.name.to_owned());
    let value = serde_json::to_value(&map).expect("serialises");
    match existing {
        Some((id, _)) => txn.modify(id, |e| {
            e.params.insert(TAG_SETTINGS_KEY.into(), value);
        }),
        None => txn
            .create(
                NewElement::new(TAG_SETTINGS_CLASS)
                    .name("Loaded Tags")
                    .param(TAG_SETTINGS_KEY, value),
            )
            .map(|_| ()),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub view_id: ElementRef,
}

/// The tags of a view with their text now (`tag.list`).
pub(crate) fn list_tags(store: &Store, p: ListParams) -> Result<Vec<ResolvedTag>> {
    let (id, _) = crate::views::resolve_view(store, &p.view_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    let parents = std::collections::HashMap::new();
    let reader = FieldReader::new(store, &parents);
    Ok(tags_of_view(store, &guid)
        .into_iter()
        .map(|(id, d)| resolve_tag(store, &reader, id, d))
        .collect())
}

// ---- Text commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTextParams {
    pub view_id: ElementRef,
    /// The text's top-left corner (plan, metres).
    pub position: [f64; 2],
    /// Plain text; `\n` separates lines.
    pub content: String,
    /// A text type (`Text 2.5 mm`, `Text 3.5 mm`, `Text 5 mm`); default `Text 2.5 mm`.
    pub type_id: Option<String>,
    /// Wrapping width on paper (mm); default unwrapped.
    pub width: Option<f64>,
    /// Counter-clockwise; accepts units ("90°").
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
}

fn check_text_type(name: &str) -> Result<()> {
    text_type(name).map(|_| ()).ok_or_else(|| {
        Error::new(
            "text.unknown_type",
            format!(
                "no text type `{name}` (types: {})",
                TEXT_TYPES
                    .iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })
}

fn check_width(width: Option<f64>) -> Result<()> {
    match width {
        Some(w) if !(1.0..=2000.0).contains(&w) => Err(Error::new(
            "text.bad_width",
            "the wrapping width must be 1–2000 mm on paper",
        )),
        _ => Ok(()),
    }
}

pub(crate) fn create_text(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateTextParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view, _, _) = plan_of(&store, &p.view_id)?;
    if p.content.trim().is_empty() {
        return Err(Error::new("text.empty", "a text note needs some text"));
    }
    let text_type = p.type_id.unwrap_or_else(|| TEXT_TYPES[0].name.to_owned());
    check_text_type(&text_type)?;
    check_width(p.width)?;
    let data = TextData {
        kind: KIND_TEXT.into(),
        view: view.clone(),
        position: p.position,
        rotation: p.rotation.unwrap_or(0.0),
        width: p.width,
        content: p.content,
        text_type,
        leaders: Vec::new(),
    };
    create_annotation(txn, &view, serde_json::to_value(&data).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetContentParams {
    pub annotation_id: ElementRef,
    pub content: String,
}

/// Replaces a note's text; a note left empty is deleted.
pub(crate) fn set_content(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetContentParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut d) = resolve_text_ref(&store, &p.annotation_id)?;
    if p.content.trim().is_empty() {
        return txn.delete(id);
    }
    d.content = p.content;
    write_anno(txn, id, &d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTextParams {
    pub annotation_ids: Vec<ElementRef>,
    pub type_id: Option<String>,
    /// Wrapping width on paper (mm); 0 makes the text unwrapped.
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
}

/// Changes text notes' type, wrapping width and rotation.
pub(crate) fn set_text(txn: &mut crate::txn::Transaction<'_>, p: SetTextParams) -> Result<()> {
    if let Some(t) = &p.type_id {
        check_text_type(t)?;
    }
    let width = p.width.map(|w| (w > 0.0).then_some(w));
    if let Some(w) = width {
        check_width(w)?;
    }
    for r in &p.annotation_ids {
        let store = txn.store().clone();
        let (id, mut d) = resolve_text_ref(&store, r)?;
        if let Some(t) = &p.type_id {
            d.text_type.clone_from(t);
        }
        if let Some(w) = width {
            d.width = w;
        }
        if let Some(a) = p.rotation {
            d.rotation = a;
        }
        write_anno(txn, id, &d)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaderShape {
    #[default]
    Straight,
    Arc,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLeaderParams {
    pub annotation_id: ElementRef,
    /// Default: the side facing the leader's end.
    pub side: Option<Side>,
    #[serde(default)]
    pub attach: Attach,
    /// An optional elbow, then the arrow's end (plan, metres).
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub shape: LeaderShape,
}

pub(crate) fn add_leader(txn: &mut crate::txn::Transaction<'_>, p: AddLeaderParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut d) = resolve_text_ref(&store, &p.annotation_id)?;
    if p.shape == LeaderShape::Arc {
        return Err(Error::new(
            "text.arc_leader_unsupported",
            "arc leaders come later; use a straight leader",
        ));
    }
    let Some(end) = p.points.last().copied() else {
        return Err(Error::new(
            "text.leader_needs_points",
            "a leader needs its end point",
        ));
    };
    if p.points.len() > 2 {
        return Err(Error::new(
            "text.leader_too_many_points",
            "a leader has at most an elbow and an end",
        ));
    }
    // The side facing the end, in the note's own frame.
    let side = p.side.unwrap_or_else(|| {
        let (s, c) = d.rotation.sin_cos();
        let local_x = (end[0] - d.position[0]) * c + (end[1] - d.position[1]) * s;
        if local_x < 0.0 {
            Side::Left
        } else {
            Side::Right
        }
    });
    d.leaders.push(TextLeader {
        side,
        attach: p.attach,
        points: p.points,
    });
    write_anno(txn, id, &d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveLeadersParams {
    pub annotation_id: ElementRef,
}

pub(crate) fn remove_leaders(
    txn: &mut crate::txn::Transaction<'_>,
    p: RemoveLeadersParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut d) = resolve_text_ref(&store, &p.annotation_id)?;
    d.leaders.clear();
    write_anno(txn, id, &d)
}

/// One text note of `text.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TextInfo {
    pub id: ElementId,
    pub data: TextData,
}

pub(crate) fn list_texts(store: &Store, p: ListParams) -> Result<Vec<TextInfo>> {
    let (id, _) = crate::views::resolve_view(store, &p.view_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    Ok(texts_of_view(store, &guid)
        .into_iter()
        .map(|(id, data)| TextInfo { id, data })
        .collect())
}

// ---- Move, rotate, mirror and copy (`element.move` and friends) ----

/// Transforms a tag or text note; `None` when `e` is neither. A tag whose host is transformed
/// too follows it and is left alone (`host_moves`).
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    e: &Element,
    x: &crate::modify::Xform,
    copy: bool,
    host_moves: &dyn Fn(&str) -> bool,
) -> Result<Option<Option<ElementId>>> {
    use crate::modify::Xform;
    let store = txn.store().clone();
    if let Some(mut d) = tag_data(e) {
        if host_moves(&d.host) {
            return Ok(Some(None));
        }
        let anchor = GlobalId::parse(&d.host)
            .and_then(|g| store.get(store.find_by_guid(&g)?))
            .and_then(|h| host_anchor_in(&store, h, &d.view))
            .unwrap_or(d.anchor);
        let head = x.point([anchor[0] + d.offset[0], anchor[1] + d.offset[1]]);
        d.offset = [head[0] - anchor[0], head[1] - anchor[1]];
        d.leader_end = d.leader_end.map(|p| x.point(p));
        return if copy {
            create_annotation(
                txn,
                &d.view.clone(),
                serde_json::to_value(&d).expect("serialises"),
            )
            .map(|id| Some(Some(id)))
        } else {
            write_anno(txn, e.id(), &d).map(|()| Some(Some(e.id())))
        };
    }
    if let Some(mut d) = text_data(e) {
        d.position = x.point(d.position);
        for l in &mut d.leaders {
            for p in &mut l.points {
                *p = x.point(*p);
            }
        }
        match *x {
            Xform::Rotate { angle, .. } => d.rotation += angle,
            // A mirrored note still reads left to right: its corner moves, its text does not.
            Xform::Mirror { .. } | Xform::Translate(_) => {}
        }
        return if copy {
            create_annotation(
                txn,
                &d.view.clone(),
                serde_json::to_value(&d).expect("serialises"),
            )
            .map(|id| Some(Some(id)))
        } else {
            write_anno(txn, e.id(), &d).map(|()| Some(Some(e.id())))
        };
    }
    Ok(None)
}
