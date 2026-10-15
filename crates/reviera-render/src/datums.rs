// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Drawing reference planes (ARC-REFP-001) and model lines (ARC-MLIN-001) in views.
//!
//! - A plan shows each vertical reference plane as its line, dashed green, and each model line
//!   whose points are within its view range, in its line style.
//! - A section or elevation shows model lines within its depth, projected, and each reference
//!   plane it looks along: a vertical plane crossing it as a vertical line over the levels, a
//!   horizontal one as a line across.
//! - A 3D view shows model lines (not reference planes).
//!
//! Widths are paper millimetres (model pens for model lines, annotation pen 1 for reference
//! planes); patterns are in paper millimetres too, scaled by the view scale.

use std::collections::HashSet;

use reviera_core::model_lines::{self, Pattern};
use reviera_core::sections::SectionFrame;
use reviera_core::{ElementId, Store, ref_planes, standards};

use crate::plan::Lines;

/// Reference planes' colour.
pub const REF_PLANE_COLOR: [f32; 4] = [0.05, 0.55, 0.15, 1.0];

fn seg(a: [f64; 3], b: [f64; 3]) -> [[f32; 3]; 2] {
    [a.map(|v| v as f32), b.map(|v| v as f32)]
}

fn color(c: [u8; 3]) -> [f32; 4] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        1.0,
    ]
}

/// Lines of one element: its own colour, or the highlight when selected.
fn lines_of(
    selected: &HashSet<ElementId>,
    id: ElementId,
    color: [f32; 4],
    width: f32,
    segments: Vec<[[f32; 3]; 2]>,
) -> Lines {
    Lines {
        color: if selected.contains(&id) {
            crate::HIGHLIGHT
        } else {
            color
        },
        width,
        segments,
    }
}

/// Model lines and reference planes in a plan whose drawn range is `bottom`…`top`, drawn at
/// height `z` (over the model) at 1:`scale`.
pub fn plan_lines(
    store: &Store,
    bottom: f64,
    top: f64,
    z: f64,
    scale: f64,
    selected: &HashSet<ElementId>,
) -> Vec<Lines> {
    let pens = standards::pen_set(store);
    let unit = scale / 1000.0;
    let mut out = Vec::new();
    let ref_width = pens.annotation_width(1) as f32;
    for r in ref_planes::list(store) {
        let Some(rec) = store.get(r.id).and_then(ref_planes::recipe) else {
            continue;
        };
        let Some((a, b)) = ref_planes::plan_trace(&rec) else {
            continue;
        };
        let pts = [[a[0], a[1], z], [b[0], b[1], z]];
        let segments = model_lines::dashes(&pts, Pattern::Dashed, unit)
            .into_iter()
            .map(|s| seg(s[0], s[1]))
            .collect();
        out.push(lines_of(
            selected,
            r.id,
            REF_PLANE_COLOR,
            ref_width,
            segments,
        ));
    }
    for l in model_lines::all(store) {
        let inside = l
            .points
            .iter()
            .all(|p| p[2] >= bottom - 1e-6 && p[2] <= top + 1e-6);
        if !inside {
            continue;
        }
        let style = model_lines::style(&l.line_style).unwrap_or(&model_lines::STYLES[0]);
        let flat: Vec<[f64; 3]> = l.points.iter().map(|p| [p[0], p[1], z]).collect();
        let segments = model_lines::dashes(&flat, style.pattern, unit)
            .into_iter()
            .map(|s| seg(s[0], s[1]))
            .collect();
        out.push(lines_of(
            selected,
            l.id,
            color(style.color),
            pens.model_width(style.pen, scale) as f32,
            segments,
        ));
    }
    out
}

/// Model lines and reference planes in a section or elevation, in its frame's coordinates (x
/// right, y up, z toward the viewer), drawn at depth `z`; `levels` is the vertical span reference
/// planes are drawn over.
pub fn section_lines(
    store: &Store,
    frame: &SectionFrame,
    levels: (f64, f64),
    z: f64,
    scale: f64,
    selected: &HashSet<ElementId>,
) -> Vec<Lines> {
    let pens = standards::pen_set(store);
    let unit = scale / 1000.0;
    let mut out = Vec::new();
    let ref_width = pens.annotation_width(1) as f32;
    for r in ref_planes::list(store) {
        let Some(rec) = store.get(r.id).and_then(ref_planes::recipe) else {
            continue;
        };
        let pts = if r.normal[2].abs() < 1e-9 {
            // A vertical plane: where it crosses the view, over the levels.
            let Some(x) =
                frame.grid_crossing([rec.start[0], rec.start[1]], [rec.end[0], rec.end[1]])
            else {
                continue;
            };
            [[x, levels.0, z], [x, levels.1, z]]
        } else if r.normal[2].abs() > 1.0 - 1e-9 {
            // A horizontal plane: a line across at its height, over the line's extent.
            let (a, b) = (frame.to_local(rec.start), frame.to_local(rec.end));
            if (a[0] - b[0]).abs() < 1e-9 {
                [
                    [-unit * 5.0, rec.start[2], z],
                    [frame.width + unit * 5.0, rec.start[2], z],
                ]
            } else {
                [[a[0], rec.start[2], z], [b[0], rec.start[2], z]]
            }
        } else {
            continue;
        };
        let segments = model_lines::dashes(&pts, Pattern::Dashed, unit)
            .into_iter()
            .map(|s| seg(s[0], s[1]))
            .collect();
        out.push(lines_of(
            selected,
            r.id,
            REF_PLANE_COLOR,
            ref_width,
            segments,
        ));
    }
    for l in model_lines::all(store) {
        let local: Vec<[f64; 3]> = l.points.iter().map(|p| frame.to_local(*p)).collect();
        let inside = local
            .iter()
            .all(|q| q[2] <= 1e-6 && q[2] >= -frame.far - 1e-6);
        if !inside {
            continue;
        }
        let style = model_lines::style(&l.line_style).unwrap_or(&model_lines::STYLES[0]);
        let flat: Vec<[f64; 3]> = local.iter().map(|q| [q[0], q[1], z]).collect();
        let segments = model_lines::dashes(&flat, style.pattern, unit)
            .into_iter()
            .map(|s| seg(s[0], s[1]))
            .collect();
        out.push(lines_of(
            selected,
            l.id,
            color(style.color),
            pens.model_width(style.pen, scale) as f32,
            segments,
        ));
    }
    out
}

/// Model lines in a 3D view (world coordinates), solid, at their pens' widths at 1:100.
pub fn model_lines_3d(store: &Store, selected: &HashSet<ElementId>) -> Vec<Lines> {
    let pens = standards::pen_set(store);
    model_lines::all(store)
        .into_iter()
        .map(|l| {
            let style = model_lines::style(&l.line_style).unwrap_or(&model_lines::STYLES[0]);
            let segments = l.points.windows(2).map(|w| seg(w[0], w[1])).collect();
            lines_of(
                selected,
                l.id,
                color(style.color),
                pens.model_width(style.pen, 100.0) as f32,
                segments,
            )
        })
        .collect()
}
