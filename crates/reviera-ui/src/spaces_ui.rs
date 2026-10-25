// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP spaces in plans (MEP-SPACE-001): each placed space on a floor plan's level shows a boxed
//! label (SPACE number, name) below where a room's label would be, in blue; a selected space
//! shows its boundary. A space is picked by its label. Space separation lines are drawn by the
//! room separator drawing, in blue.

use reviera_core::ElementId;
use reviera_core::mep_spaces;
use reviera_render::annotation::{Label, ROOM_TEXT_MM, label_strokes, labels_extent, room_tag_labels};
use reviera_render::plan::Lines;

use super::Gui;

/// A space's label lines and its outline rings.
type SpaceLabel = (ElementId, Vec<Label>, Vec<Vec<[f64; 2]>>);

pub(crate) const SPACE_COLOR: [f32; 4] = [0.1, 0.4, 0.85, 1.0];

impl Gui {
    /// The labels of the placed spaces on a plan's level, with each space's outline rings.
    fn space_labels(&mut self, index: usize) -> Vec<SpaceLabel> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone() else { return Vec::new() };
        if plan.ceiling {
            return Vec::new();
        }
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let Some(level) = store.get(plan.level).map(|l| l.guid().to_string()) else { return Vec::new() };
        let z = plan.range.cut as f32 + 0.003;
        let mut out = Vec::new();
        for (id, r, g) in mep_spaces::all(&store) {
            if r.level != level {
                continue;
            }
            let Some(g) = g else { continue };
            let Some(e) = store.get(id) else { continue };
            let (number, name) = reviera_core::rooms::number_and_name(e);
            // Below a room label at the same point (three lines of 4 mm, and the box's margin).
            let drop = 12.0 * plan.scale as f64 / 1000.0;
            let at = [g.tag_point[0], g.tag_point[1] - drop];
            let lines = vec![format!("SPACE {}", number.to_uppercase()), name.to_uppercase()];
            let labels = room_tag_labels(at, &lines, plan.scale, z);
            let rings = g.region.as_ref().map(|reg| std::iter::once(reg.outer_ring()).chain(reg.hole_rings()).collect()).unwrap_or_default();
            out.push((id, labels, rings));
        }
        out
    }

    /// Space labels in their boxes, and the selected spaces' boundaries, for a plan.
    pub(crate) fn space_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let Some(plan) = self.views[index].plan.clone() else { return Vec::new() };
        let z = plan.range.cut as f32 + 0.003;
        let pad = ROOM_TEXT_MM * plan.scale / 1000.0 * 0.5;
        let mut normal = Lines { color: SPACE_COLOR, width: 0.18, segments: Vec::new() };
        let mut lit = Lines { color: reviera_render::HIGHLIGHT, width: 0.35, segments: Vec::new() };
        for (id, labels, rings) in self.space_labels(index) {
            let on = selected.contains(&id);
            let target = if on { &mut lit } else { &mut normal };
            target.segments.extend(label_strokes(&labels));
            if let Some((lo, hi)) = labels_extent(&labels) {
                let (a, b) = ([lo[0] - pad, lo[1] - pad], [hi[0] + pad, hi[1] + pad]);
                let c = [a, [b[0], a[1]], b, [a[0], b[1]]];
                for i in 0..4 {
                    let (p, q) = (c[i], c[(i + 1) % 4]);
                    target.segments.push([[p[0], p[1], z], [q[0], q[1], z]]);
                }
            }
            if on {
                for ring in &rings {
                    for i in 0..ring.len() {
                        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                        lit.segments.push([[p[0] as f32, p[1] as f32, z], [q[0] as f32, q[1] as f32, z]]);
                    }
                }
            }
        }
        vec![normal, lit]
    }

    /// The space whose label is under pixel (`x`, `y`) in the active plan.
    pub(crate) fn space_label_at(&mut self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let p = self.plan_xy(x, y)?;
        let scale = self.views[index].plan.as_ref()?.scale;
        let pad = ROOM_TEXT_MM * scale / 1000.0;
        self.space_labels(index).into_iter().find_map(|(id, labels, _)| {
            let (lo, hi) = labels_extent(&labels)?;
            let (px, py) = (p[0] as f32, p[1] as f32);
            (px >= lo[0] - pad && px <= hi[0] + pad && py >= lo[1] - pad && py <= hi[1] + pad).then_some(id)
        })
    }
}
