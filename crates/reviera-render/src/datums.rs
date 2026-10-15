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
use reviera_core::{ElementId, Store, datum_extents, grids, ref_planes, scope_boxes, standards};
use reviera_core::{property_lines, toposolids};

use crate::annotation::{GridMark, GridStyle};
use crate::plan::Lines;

/// Scope boxes' colour.
pub const SCOPE_BOX_COLOR: [f32; 4] = [0.55, 0.25, 0.6, 1.0];

/// The grids a plan (view GlobalId, cut at `cut`) shows, with its 2D extents, bubbles and elbows
/// (ARC-DATUM-001), leaving out those whose scope box it misses.
pub fn plan_grids(store: &Store, view: Option<&str>, cut: f64) -> Vec<(ElementId, GridMark)> {
    let pens = standards::pen_set(store);
    grids::list(store)
        .into_iter()
        .filter_map(|g| {
            let e = store.get(g.id)?;
            if !datum_extents::shows_in_plan(store, e, cut) {
                return None;
            }
            let v = view
                .map(|v| datum_extents::in_view(e, v))
                .unwrap_or_default();
            let line = v.line.unwrap_or(g.curve);
            let bubbles = match g.style.symbol {
                grids::Symbol::None => [false, false],
                grids::Symbol::Bubble => v.bubbles.unwrap_or(g.bubbles),
            };
            Some((
                g.id,
                GridMark {
                    start: line.start,
                    end: line.end,
                    name: g.name,
                    bubbles,
                    style: GridStyle::of(&g.style, &|p| pens.annotation_width(p) as f32),
                    elbows: v.elbows.map(|x| x.map(|o| o as f32)),
                },
            ))
        })
        .collect()
}

/// Scope boxes a plan cut at `cut` passes through: dashed outlines with their names, drawn at
/// height `draw_z` at 1:`scale`.
pub fn scope_box_lines(
    store: &Store,
    cut: f64,
    draw_z: f64,
    scale: f64,
    selected: &HashSet<ElementId>,
) -> Vec<Lines> {
    let pens = standards::pen_set(store);
    let unit = scale / 1000.0;
    scope_boxes::in_plan(store, cut)
        .into_iter()
        .map(|(id, name, b)| {
            let c = b.corners();
            let mut pts: Vec<[f64; 3]> = c.iter().map(|p| [p[0], p[1], draw_z]).collect();
            pts.push(pts[0]);
            let mut segments: Vec<[[f32; 3]; 2]> = model_lines::dashes(&pts, Pattern::Centre, unit)
                .into_iter()
                .map(|s| seg(s[0], s[1]))
                .collect();
            // The name inside the box's last corner, 2.5 mm text.
            let (u, v) = b.axes();
            let h = 2.5 * unit;
            let origin = glam::Vec3::new(
                (c[3][0] + u[0] * h - v[0] * 2.0 * h) as f32,
                (c[3][1] + u[1] * h - v[1] * 2.0 * h) as f32,
                draw_z as f32,
            );
            let right = glam::Vec3::new(u[0] as f32, u[1] as f32, 0.0);
            let up = glam::Vec3::new(v[0] as f32, v[1] as f32, 0.0);
            segments.extend(
                crate::font::strokes(&name, origin, right, up, h as f32)
                    .into_iter()
                    .map(|[p, q]| [p.to_array(), q.to_array()]),
            );
            lines_of(
                selected,
                id,
                SCOPE_BOX_COLOR,
                pens.annotation_width(1) as f32,
                segments,
            )
        })
        .collect()
}

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
#[allow(clippy::too_many_arguments)]
pub fn plan_lines(
    store: &Store,
    view: Option<&str>,
    cut: f64,
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
        if ref_planes::plan_trace(&rec).is_none() {
            continue;
        }
        let e = store.get(r.id).expect("listed");
        if !datum_extents::shows_in_plan(store, e, cut) {
            continue;
        }
        let Some(l) = datum_extents::plan_line(e, view) else {
            continue;
        };
        let (a, b) = (l.start, l.end);
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
    out.extend(site_lines(store, z, scale, selected));
    out
}

/// Contours' and property lines' colours.
pub const CONTOUR_COLOR: [f32; 4] = [0.55, 0.40, 0.22, 1.0];
pub const PROPERTY_LINE_COLOR: [f32; 4] = [0.75, 0.10, 0.10, 1.0];

/// A site in plan (ARC-TOPO-002, ARC-SITE-001): toposolids' contours at the site settings'
/// interval (every fifth from the passing elevation heavier), and property lines dash-dotted.
fn site_lines(store: &Store, z: f64, scale: f64, selected: &HashSet<ElementId>) -> Vec<Lines> {
    let pens = standards::pen_set(store);
    let unit = scale / 1000.0;
    let s = toposolids::settings(store);
    let mut out = Vec::new();
    for e in store.iter().filter(|e| toposolids::recipe(e).is_some()) {
        let (mut minor, mut major) = (Vec::new(), Vec::new());
        for (h, line) in toposolids::contours(store, e) {
            let k = ((h - s.passing_elevation) / s.contour_interval).round() as i64;
            let into = if k.rem_euclid(5) == 0 {
                &mut major
            } else {
                &mut minor
            };
            into.extend(
                line.windows(2)
                    .map(|w| seg([w[0][0], w[0][1], z], [w[1][0], w[1][1], z])),
            );
        }
        for (segments, pen) in [(minor, 1), (major, 3)] {
            if !segments.is_empty() {
                out.push(lines_of(
                    selected,
                    e.id(),
                    CONTOUR_COLOR,
                    pens.model_width(pen, scale) as f32,
                    segments,
                ));
            }
        }
    }
    for e in store.iter() {
        let Some(r) = property_lines::recipe(e) else {
            continue;
        };
        let pts: Vec<[f64; 3]> = r.points.iter().map(|p| [p[0], p[1], z]).collect();
        let segments = model_lines::dashes(&pts, Pattern::Centre, unit)
            .into_iter()
            .map(|s| seg(s[0], s[1]))
            .collect();
        out.push(lines_of(
            selected,
            e.id(),
            PROPERTY_LINE_COLOR,
            pens.model_width(3, scale) as f32,
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
    view: Option<&str>,
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
            let e = store.get(r.id).expect("listed");
            if !datum_extents::shows_in_section(store, e, frame) {
                continue;
            }
            let Some(x) =
                frame.grid_crossing([rec.start[0], rec.start[1]], [rec.end[0], rec.end[1]])
            else {
                continue;
            };
            let [lo, hi] = datum_extents::heights(store, e, view).unwrap_or([levels.0, levels.1]);
            [[x, lo, z], [x, hi, z]]
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
