// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Tags and text notes of a view, resolved against the model and laid out (DOC-ANN-009…011).
//! The window and PDF output draw the same parts.

use std::collections::HashMap;

use reviera_core::notes::{self, LeaderMode};
use reviera_core::schedules::FieldReader;
use reviera_core::{Element, ElementId, Store};
use reviera_render::notes::{NoteParts, TagMark, TextMark, tag_parts, text_parts};

/// A tag or text note as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteGraphic {
    pub id: ElementId,
    /// The tagged element, for tags whose host exists.
    pub host: Option<ElementId>,
    pub parts: NoteParts,
    /// A tag whose host was deleted.
    pub orphaned: bool,
}

/// The tags, text notes and spots of `view` at 1:`scale`, at elevation `z`; spots measured
/// against the model's `scene` (as stored without one).
pub fn view_notes(store: &Store, view: &Element, scale: f32, z: f32, scene: Option<&reviera_render::RenderScene>) -> Vec<NoteGraphic> {
    let guid = view.guid().to_string();
    let parents = HashMap::new();
    let reader = FieldReader::new(store, &parents);
    let mut out = Vec::new();
    for (id, data) in notes::tags_of_view(store, &guid) {
        let t = notes::resolve_tag(store, &reader, id, data);
        let leader = match t.data.leader {
            LeaderMode::None => None,
            LeaderMode::Attached => Some(t.anchor),
            LeaderMode::Free => t.data.leader_end,
        };
        let mark = TagMark {
            head: t.head,
            lines: t.lines,
            frame: t.frame,
            vertical: t.data.vertical,
            leader,
            text_mm: notes::TAG_TEXT_MM as f32,
        };
        out.push(NoteGraphic {
            id,
            host: t.host,
            parts: tag_parts(&mark, scale, z),
            orphaned: t.orphaned,
        });
    }
    for (id, d) in notes::texts_of_view(store, &guid) {
        let size = notes::text_type(&d.text_type).map_or(2.5, |t| t.size);
        let mark = TextMark {
            position: d.position,
            rotation: d.rotation,
            size_mm: size as f32,
            width_mm: d.width.map(|w| w as f32),
            content: d.content,
            leaders: d.leaders,
        };
        out.push(NoteGraphic {
            id,
            host: None,
            parts: text_parts(&mark, scale, z),
            orphaned: false,
        });
    }
    // Spot elevations and coordinates (DOC-ANN-006/007), their element as their host.
    for m in crate::spots::of_view(store, scene, &guid) {
        let mark = reviera_render::notes::SpotMark {
            point: reviera_core::spots::in_view(store, &guid, m.picked),
            text: m.data.text,
            lines: m.lines,
            symbol: match m.data.spot_kind {
                reviera_core::spots::SpotKind::Elevation => reviera_render::notes::SpotSymbol::Triangle,
                reviera_core::spots::SpotKind::Coordinate => reviera_render::notes::SpotSymbol::Cross,
            },
            text_mm: notes::TAG_TEXT_MM as f32,
        };
        let host = m.data.element.as_deref().and_then(reviera_core::GlobalId::parse).and_then(|g| store.find_by_guid(&g));
        out.push(NoteGraphic { id: m.id, host, parts: reviera_render::notes::spot_parts(&mark, scale, z), orphaned: m.orphaned });
    }
    out
}

/// The rooms tagged in `view` (GlobalIds): their interim labels give way to the tags.
pub fn tagged_rooms(store: &Store, view: &Element) -> std::collections::HashSet<String> {
    notes::tags_of_view(store, view.guid().as_str())
        .into_iter()
        .filter(|(_, d)| {
            notes::tag_type(&d.tag_type).is_some_and(|t| t.categories == ["cat.rooms"])
        })
        .map(|(_, d)| d.host)
        .collect()
}
