// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Modify tools in plan views (Ch 3 §3.12, ARC-MOD-001 … 013), their previews, and graphical
//! editing of the selection (Ch 1 §1.22.2): grips at the ends of a selected wall or grid, and
//! dragging selected walls and grids.
//!
//! The tools follow the usual design-application conventions: Move, Copy, Rotate and Mirror act
//! on the selection made first; points snap like the drawing tools; lengths and angles can be
//! typed and committed with Enter; Esc drops the last pick, then ends the tool. Align, Offset,
//! Trim/Extend and Split pick elements in the view and stay active for more picks.

use reviera_core::ElementId;
use reviera_core::modify::Xform;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::WxWidget;

use super::dims::Candidate;
use super::{Gui, snap};

const GHOST: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
const GRIP: [f32; 4] = [0.95, 0.55, 0.05, 1.0];
/// Grips are hit within this many pixels.
const GRIP_PX: f32 = 8.0;

/// The modify tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Copy,
    Rotate,
    MirrorPick,
    MirrorDraw,
    Align,
    Offset,
    TrimCorner,
    TrimSingle,
    Split,
    /// Arrays (ARC-MOD-026): a row of copies, or copies round a centre.
    ArrayLinear,
    ArrayRadial,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Move => "Move",
            Kind::Copy => "Copy",
            Kind::Rotate => "Rotate",
            Kind::MirrorPick => "Mirror – Pick Axis",
            Kind::MirrorDraw => "Mirror – Draw Axis",
            Kind::Align => "Align",
            Kind::Offset => "Offset",
            Kind::TrimCorner => "Trim/Extend to Corner",
            Kind::TrimSingle => "Trim/Extend Single Element",
            Kind::Split => "Split Element",
            Kind::ArrayLinear => "Array",
            Kind::ArrayRadial => "Radial Array",
        }
    }

    /// Whether the tool acts on the selection made before it starts.
    fn needs_selection(self) -> bool {
        matches!(
            self,
            Kind::Move
                | Kind::Copy
                | Kind::Rotate
                | Kind::MirrorPick
                | Kind::MirrorDraw
                | Kind::ArrayLinear
                | Kind::ArrayRadial
        )
    }

    /// The UI action of the tool (ribbon catalog).
    pub fn of_action(action: &str) -> Option<Self> {
        Some(match action {
            "element.move" => Kind::Move,
            "element.copy" => Kind::Copy,
            "element.rotate" => Kind::Rotate,
            "element.mirror_pick" => Kind::MirrorPick,
            "element.mirror_draw" => Kind::MirrorDraw,
            "element.align" => Kind::Align,
            "element.offset" => Kind::Offset,
            "element.trim_corner" => Kind::TrimCorner,
            "element.trim_extend" => Kind::TrimSingle,
            "element.split" => Kind::Split,
            "element.array_linear" => Kind::ArrayLinear,
            "element.array_radial" => Kind::ArrayRadial,
            _ => return None,
        })
    }
}

/// A modify tool in progress.
#[derive(Debug)]
pub struct ModifyTool {
    pub kind: Kind,
    /// The elements Move, Copy, Rotate and Mirror act on.
    targets: Vec<ElementId>,
    /// Picked points: Move/Copy base; Rotate first ray; Mirror axis start.
    points: Vec<[f64; 2]>,
    /// Align's reference.
    reference: Option<Candidate>,
    /// Trim's first wall or boundary, with the picked point.
    first: Option<(ElementId, [f64; 2])>,
    /// Rotate's centre.
    center: [f64; 2],
    /// Offset distance, metres.
    distance: f64,
    /// The snapped point under the cursor, and what it snapped to.
    cursor: Option<snap::Snap>,
    /// The reference under the cursor (Align, Offset, Mirror – Pick Axis).
    hover: Option<Candidate>,
    /// A length or angle being typed.
    typed: String,
    lines: Vec<([f64; 2], [f64; 2])>,
    /// An array's members (the original included) and whether the second pick is the last
    /// member.
    count: u32,
    last: bool,
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

impl Gui {
    /// Starts a modify tool (plans only).
    pub(crate) fn start_modify(&mut self, kind: Kind) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame.set_status_text(
                &format!("{} works in plan views (View › Floor Plan)", kind.label()),
                0,
            );
        }
        let targets = self.selection();
        if kind.needs_selection() && targets.is_empty() {
            return frame.set_status_text(
                &format!("Select the elements first, then start {}", kind.label()),
                0,
            );
        }
        // An array asks how many members, and where the second pick goes.
        let (count, last) = match kind {
            Kind::ArrayLinear | Kind::ArrayRadial => {
                let Some(o) = self.array_options(kind) else {
                    return;
                };
                o
            }
            _ => (0, false),
        };
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_line_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        let center = self.selection_center(&targets).unwrap_or([0.0, 0.0]);
        let lines = self.snap_lines();
        self.modify_tool = Some(ModifyTool {
            kind,
            targets,
            points: Vec::new(),
            reference: None,
            first: None,
            center,
            distance: self.offset_distance,
            cursor: None,
            hover: None,
            typed: String::new(),
            lines,
            count,
            last,
        });
        if !kind.needs_selection() {
            self.clear_selection();
        }
        self.modify_prompt();
        self.modify_preview();
    }

    pub(crate) fn end_modify(&mut self) -> bool {
        if self.modify_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Ready", 0);
        self.update_grips();
        true
    }

    /// Esc: drops the typed value or the last pick, then ends the tool.
    pub(crate) fn modify_escape(&mut self) {
        let Some(t) = &mut self.modify_tool else {
            return;
        };
        if !t.typed.is_empty() {
            t.typed.clear();
        } else if t.points.pop().is_some()
            || t.reference.take().is_some()
            || t.first.take().is_some()
        {
        } else {
            self.end_modify();
            return;
        }
        self.modify_prompt();
        self.modify_preview();
    }

    /// Lines to snap to: walls' location lines, grids and the plan's cut edges.
    pub(crate) fn snap_lines(&self) -> Vec<([f64; 2], [f64; 2])> {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        let mut lines: Vec<([f64; 2], [f64; 2])> = store
            .iter()
            .filter_map(reviera_core::walls::wall_recipe)
            .map(|r| (r.curve.start, r.curve.end))
            .collect();
        lines.extend(
            reviera_core::grids::list(&store)
                .into_iter()
                .map(|g| (g.curve.start, g.curve.end)),
        );
        if let Some(plan) = self.active_view().and_then(|v| v.plan.as_ref()) {
            let p = |q: [f32; 2]| [f64::from(q[0]), f64::from(q[1])];
            lines.extend(plan.cut_edges.iter().map(|(_, s)| (p(s[0]), p(s[1]))));
        }
        lines
    }

    /// The snapped plan point under pixel (`x`, `y`), angles measured from `from`.
    pub(crate) fn snapped_at(
        &self,
        x: i32,
        y: i32,
        from: Option<[f64; 2]>,
        lines: &[([f64; 2], [f64; 2])],
    ) -> Option<snap::Snap> {
        let v = self.active_view()?;
        let z = v.plan.as_ref()?.range.cut as f32;
        let raw = self.plan_xy(x, y)?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let to_screen = |p: [f64; 2]| {
            v.camera
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let mut s = snap::snap(raw, (x as f32, y as f32), from, lines, to_screen);
        if s.kind == snap::SnapKind::None {
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    /// The plan centre of elements' extents (Rotate's default centre).
    fn selection_center(&self, ids: &[ElementId]) -> Option<[f64; 2]> {
        let segs = self.outline(ids, None);
        let pts: Vec<[f64; 2]> = segs.iter().flat_map(|s| *s).collect();
        let first = pts.first()?;
        let (lo, hi) = pts.iter().fold((*first, *first), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        });
        Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
    }

    /// Plan outlines of elements (walls' footprints, grids' lines, doors' and windows'
    /// openings), transformed by `x`.
    fn outline(&self, ids: &[ElementId], x: Option<Xform>) -> Vec<[[f64; 2]; 2]> {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        let t = |p: [f64; 2]| x.map_or(p, |x| x.point(p));
        let mut out = Vec::new();
        let ring = |out: &mut Vec<[[f64; 2]; 2]>, pts: &[[f64; 2]]| {
            for k in 0..pts.len() {
                out.push([t(pts[k]), t(pts[(k + 1) % pts.len()])]);
            }
        };
        for id in ids {
            let Some(e) = store.get(*id) else { continue };
            if reviera_core::walls::wall_recipe(e).is_some() {
                if let Ok(g) = reviera_core::walls::regenerate(&store, e) {
                    ring(&mut out, &g.footprint());
                }
            } else if let Some(g) = reviera_core::grids::grid_data(e) {
                out.push([t(g.curve.start), t(g.curve.end)]);
            } else if reviera_core::columns::column_recipe(e).is_some() {
                if let Ok(g) = reviera_core::columns::regenerate(&store, e) {
                    ring(&mut out, &g.ring);
                }
            } else if reviera_core::curtain::curtain_recipe(e).is_some() {
                if let Ok(g) = reviera_core::curtain::regenerate(&store, e) {
                    ring(&mut out, &g.footprint());
                }
            } else if let Ok(p) = reviera_core::inserts::resolve(&store, e) {
                let c = p.cut();
                let half = p.wall.width / 2.0;
                let corner = |a: f64, b: f64| {
                    let w = p.world([a - p.recipe.along, b, 0.0]);
                    [w[0], w[1]]
                };
                ring(
                    &mut out,
                    &[
                        corner(c.x[0], -half),
                        corner(c.x[1], -half),
                        corner(c.x[1], half),
                        corner(c.x[0], half),
                    ],
                );
            }
        }
        out
    }

    /// The transformation the tool would apply with the cursor where it is.
    fn tool_xform(&self) -> Option<Xform> {
        let t = self.modify_tool.as_ref()?;
        let cursor = t.cursor.map(|c| c.point);
        match t.kind {
            Kind::Move | Kind::Copy | Kind::ArrayLinear => {
                let base = *t.points.first()?;
                let to = self.typed_point(base, cursor?).unwrap_or(cursor?);
                let v = sub(to, base);
                Some(Xform::Translate([v[0], v[1], 0.0]))
            }
            Kind::Rotate | Kind::ArrayRadial => {
                let ray = *t.points.first()?;
                let angle = match self.typed_number() {
                    Some(deg) => deg.to_radians(),
                    None => {
                        let (a, b) = (sub(ray, t.center), sub(cursor?, t.center));
                        b[1].atan2(b[0]) - a[1].atan2(a[0])
                    }
                };
                Some(Xform::Rotate {
                    center: t.center,
                    angle,
                })
            }
            Kind::MirrorDraw => {
                let a = *t.points.first()?;
                let b = cursor?;
                (sub(b, a) != [0.0, 0.0]).then_some(Xform::Mirror { a, b })
            }
            Kind::MirrorPick => {
                let h = t.hover.as_ref()?;
                Some(Xform::Mirror {
                    a: h.segment[0],
                    b: h.segment[1],
                })
            }
            _ => None,
        }
    }

    fn typed_number(&self) -> Option<f64> {
        let t = self.modify_tool.as_ref()?;
        t.typed.trim().trim_end_matches('°').trim().parse().ok()
    }

    /// The point a typed length reaches from `base` toward `toward`.
    fn typed_point(&self, base: [f64; 2], toward: [f64; 2]) -> Option<[f64; 2]> {
        let t = self.modify_tool.as_ref()?;
        if t.typed.is_empty() {
            return None;
        }
        let store = self.app.document(self.doc?)?.doc.store();
        let settings = reviera_core::units::settings_of(store);
        let length = reviera_core::units::parse(&t.typed, "length", &settings).ok()?;
        let d = sub(toward, base);
        let l = d[0].hypot(d[1]);
        (l > 1e-9).then(|| [base[0] + d[0] / l * length, base[1] + d[1] / l * length])
    }

    fn modify_prompt(&mut self) {
        let Some(distance) = self.modify_tool.as_ref().map(|t| t.distance) else {
            return;
        };
        let distance = self.format_length(distance);
        let Some(t) = &self.modify_tool else { return };
        let what = match t.kind {
            Kind::Move | Kind::Copy if t.points.is_empty() => "click the start point".to_owned(),
            Kind::Move | Kind::Copy => {
                "click the end point, or type a distance and press Enter".to_owned()
            }
            Kind::Rotate if t.points.is_empty() => {
                "click a point on the first ray (from the centre ✕)".to_owned()
            }
            Kind::Rotate => {
                "click the second ray, or type an angle in degrees and press Enter".to_owned()
            }
            Kind::ArrayLinear if t.points.is_empty() => {
                format!("{} members: click the start point", t.count)
            }
            Kind::ArrayLinear => format!(
                "click where the {} member goes, or type a distance and press Enter",
                if t.last { "last" } else { "second" }
            ),
            Kind::ArrayRadial if t.points.is_empty() => format!(
                "{} members round the centre ✕: click a point on the first ray",
                t.count
            ),
            Kind::ArrayRadial => format!(
                "click the ray of the {} member, or type an angle in degrees (360 for a full turn) and press Enter",
                if t.last { "last" } else { "second" }
            ),
            Kind::MirrorPick => "pick a wall, wall face or grid as the axis".to_owned(),
            Kind::MirrorDraw if t.points.is_empty() => {
                "click the first point of the axis".to_owned()
            }
            Kind::MirrorDraw => "click the second point of the axis".to_owned(),
            Kind::Align if t.reference.is_none() => {
                "pick the reference line (a wall face, centreline or grid)".to_owned()
            }
            Kind::Align => "pick the line to align with it; Esc picks a new reference".to_owned(),
            Kind::Offset => {
                let d = distance.clone();
                format!(
                    "offset {} toward the cursor: click a wall or grid (type a distance to change it)",
                    if t.typed.is_empty() {
                        d
                    } else {
                        format!("{}_", t.typed)
                    }
                )
            }
            Kind::TrimCorner if t.first.is_none() => {
                "click the first wall, on the part to keep".to_owned()
            }
            Kind::TrimCorner => "click the second wall, on the part to keep".to_owned(),
            Kind::TrimSingle if t.first.is_none() => {
                "pick the boundary (a wall or grid)".to_owned()
            }
            Kind::TrimSingle => {
                "click walls to trim or extend, on the part to keep; Esc picks a new boundary"
                    .to_owned()
            }
            Kind::Split => "click a wall where it should be split".to_owned(),
        };
        let mut text = format!("{}: {what}", t.kind.label());
        if let Some(c) = t.cursor
            && c.kind != snap::SnapKind::None
        {
            text += &format!(" · {}", c.kind.label());
        }
        if !t.typed.is_empty() && t.kind != Kind::Offset {
            text += &format!(" · {}_", t.typed);
        }
        self.frame.set_status_text(&text, 0);
    }

    pub(crate) fn modify_motion(&mut self, x: i32, y: i32) {
        let Some(t) = &self.modify_tool else { return };
        let from = match t.kind {
            Kind::Rotate | Kind::ArrayRadial => Some(t.center),
            _ => t.points.first().copied(),
        };
        let lines = t.lines.clone();
        let kind = t.kind;
        let reference = t.reference.as_ref().map(|r| r.direction);
        let cursor = self.snapped_at(x, y, from, &lines);
        let hover = match kind {
            Kind::Align => self.reference_at(x, y, reference),
            Kind::MirrorPick | Kind::Offset => self.reference_at(x, y, None),
            _ => None,
        };
        if let Some(t) = &mut self.modify_tool {
            t.cursor = cursor;
            t.hover = hover;
        }
        self.modify_prompt();
        self.modify_preview();
    }

    /// The wall or grid element of a reference argument.
    fn candidate_element(c: &Candidate) -> Option<ElementId> {
        c.arg.get("element").and_then(Value::as_u64).map(ElementId)
    }

    pub(crate) fn modify_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        self.modify_motion(x, y);
        let Some(t) = &self.modify_tool else { return };
        let kind = t.kind;
        let cursor = t.cursor.map(|c| c.point);
        let result: Option<Result<Value, String>> = match kind {
            Kind::Move
            | Kind::Copy
            | Kind::Rotate
            | Kind::MirrorDraw
            | Kind::ArrayLinear
            | Kind::ArrayRadial
                if t.points.is_empty() =>
            {
                if let (Some(p), Some(t)) = (cursor, &mut self.modify_tool) {
                    t.points.push(p);
                }
                None
            }
            Kind::Move
            | Kind::Copy
            | Kind::Rotate
            | Kind::MirrorDraw
            | Kind::MirrorPick
            | Kind::ArrayLinear
            | Kind::ArrayRadial => self.commit_xform(),
            Kind::Align => match (t.reference.clone(), t.hover.clone()) {
                (None, Some(h)) => {
                    if let Some(t) = &mut self.modify_tool {
                        t.reference = Some(h);
                    }
                    None
                }
                (Some(r), Some(h)) => Some(self.run(
                    "element.align",
                    json!({ "reference": r.arg, "targets": [h.arg] }),
                )),
                _ => None,
            },
            Kind::Offset => {
                let element = t.hover.as_ref().and_then(Self::candidate_element);
                match (element, cursor) {
                    (Some(e), Some(side)) => Some(self.run(
                        "element.offset",
                        json!({ "element_ids": [e], "distance": t.distance, "side_point": side, "copy": true }),
                    )),
                    _ => None,
                }
            }
            Kind::TrimCorner | Kind::TrimSingle | Kind::Split => {
                let (Some(point), Some(panel)) = (cursor, self.active_view().map(|v| v.panel))
                else {
                    return;
                };
                let hit = self.element_at(x, y, &panel);
                let first = t.first;
                match (kind, first, hit) {
                    (_, _, None) => None,
                    (Kind::Split, _, Some(e)) => {
                        Some(self.run("element.split", json!({ "element_id": e, "point": point })))
                    }
                    (_, None, Some(e)) => {
                        if let Some(t) = &mut self.modify_tool {
                            t.first = Some((e, point));
                        }
                        None
                    }
                    (Kind::TrimCorner, Some((a, pa)), Some(b)) => {
                        let r = self.run(
                            "element.trim_corner",
                            json!({
                                "a": { "element_id": a, "keep_point": pa },
                                "b": { "element_id": b, "keep_point": point },
                            }),
                        );
                        if let Some(t) = &mut self.modify_tool {
                            t.first = None;
                        }
                        Some(r)
                    }
                    (_, Some((boundary, _)), Some(target)) => Some(self.run(
                        "element.trim_extend",
                        json!({ "boundary": boundary, "target": { "element_id": target, "keep_point": point } }),
                    )),
                }
            }
        };
        match &result {
            Some(Ok(_)) => {
                self.refresh_scene(&frame);
                let lines = self.snap_lines();
                if let Some(t) = &mut self.modify_tool {
                    t.lines = lines;
                    t.typed.clear();
                }
            }
            Some(Err(e)) => frame.set_status_text(e, 0),
            None => {}
        }
        if result.as_ref().is_none_or(|r| r.is_ok()) {
            self.modify_prompt();
        }
        self.modify_preview();
    }

    /// Runs Move, Copy, Rotate or Mirror with the current transformation; the tool ends and the
    /// result is selected.
    fn commit_xform(&mut self) -> Option<Result<Value, String>> {
        let x = self.tool_xform()?;
        let t = self.modify_tool.as_ref()?;
        let ids = t.targets.clone();
        let (command, args) = match (t.kind, x) {
            (Kind::Move, Xform::Translate(v)) => {
                ("element.move", json!({ "element_ids": ids, "vector": v }))
            }
            (Kind::Copy, Xform::Translate(v)) => (
                "element.copy",
                json!({ "element_ids": ids, "vectors": [v] }),
            ),
            (Kind::Rotate, Xform::Rotate { center, angle }) => (
                "element.rotate",
                json!({ "element_ids": ids, "center": center, "angle": angle }),
            ),
            (_, Xform::Mirror { a, b }) => (
                "element.mirror",
                json!({ "element_ids": ids, "axis": { "start": a, "end": b }, "copy": true }),
            ),
            (Kind::ArrayLinear, Xform::Translate(v)) => (
                "element.array_linear",
                json!({
                    "element_ids": ids, "vector": v, "count": t.count,
                    "move_to": if t.last { "last" } else { "second" },
                }),
            ),
            (Kind::ArrayRadial, Xform::Rotate { center, angle }) => (
                "element.array_radial",
                json!({
                    "element_ids": ids, "center": center, "angle": angle, "count": t.count,
                    "move_to": if t.last { "last" } else { "second" },
                }),
            ),
            _ => return None,
        };
        let result = self.run(command, args);
        if let Ok(v) = &result {
            let frame = self.frame;
            self.refresh_scene(&frame);
            // Select what the tool made or moved.
            let made: Vec<u64> = match command {
                "element.copy" => v[0]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .collect(),
                // Every member of an array.
                "element.array_linear" | "element.array_radial" => v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_array)
                    .flatten()
                    .filter_map(Value::as_u64)
                    .collect(),
                _ => v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .collect(),
            };
            self.end_modify();
            let _ = self.run("selection.set", json!({ "ids": made }));
            self.selection_changed();
        }
        Some(result)
    }

    /// The transformations the tool's preview shows for `x`: `x` itself, or for an array every
    /// member after the original.
    fn array_members(&self, x: Xform) -> Vec<Xform> {
        let Some(t) = &self.modify_tool else {
            return vec![x];
        };
        if !matches!(t.kind, Kind::ArrayLinear | Kind::ArrayRadial) || t.count < 2 {
            return vec![x];
        }
        let n = f64::from(t.count - 1);
        match x {
            Xform::Translate(v) => {
                let step = if t.last { v.map(|c| c / n) } else { v };
                (1..t.count)
                    .map(|k| Xform::Translate(step.map(|c| c * f64::from(k))))
                    .collect()
            }
            Xform::Rotate { center, angle } => {
                let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-6;
                let step = match (t.last, full) {
                    (false, _) => angle,
                    (true, true) => angle / f64::from(t.count),
                    (true, false) => angle / n,
                };
                (1..t.count)
                    .map(|k| Xform::Rotate {
                        center,
                        angle: step * f64::from(k),
                    })
                    .collect()
            }
            Xform::Mirror { .. } => vec![x],
        }
    }

    /// Asks an array's number of members and where its second pick goes; `None` if cancelled.
    fn array_options(&mut self, kind: Kind) -> Option<(u32, bool)> {
        use wxdragon::prelude::*;
        let frame = self.frame;
        let title = kind.label();
        let d =
            TextEntryDialog::builder(&frame, "Number of members (the original included):", title)
                .with_default_value(&self.array_count.to_string())
                .build();
        if d.show_modal() != ID_OK {
            return None;
        }
        let count: u32 = match d.get_value()?.trim().parse() {
            Ok(n) if (2..=1000).contains(&n) => n,
            _ => {
                frame.set_status_text("An array has between 2 and 1000 members", 0);
                return None;
            }
        };
        let choices = [
            "2nd: the pick places the second member",
            "Last: the pick places the last member",
        ];
        let c = SingleChoiceDialog::builder(&frame, "Move To:", title, &choices).build();
        c.set_selection(i32::from(self.array_last));
        if c.show_modal() != ID_OK {
            return None;
        }
        let last = c.get_selection() == 1;
        self.array_count = count;
        self.array_last = last;
        Some((count, last))
    }

    /// Keys in a modify tool: digits and units type a length or angle, Enter commits it,
    /// Backspace edits it. Returns whether the key was used.
    pub(crate) fn modify_key(&mut self, key: i32) -> bool {
        let Some(t) = &mut self.modify_tool else {
            return false;
        };
        use wxdragon::prelude::{WXK_BACK, WXK_NUMPAD_ENTER, WXK_RETURN};
        let typing = matches!(
            t.kind,
            Kind::Move
                | Kind::Copy
                | Kind::Rotate
                | Kind::Offset
                | Kind::ArrayLinear
                | Kind::ArrayRadial
        );
        match key {
            WXK_BACK if !t.typed.is_empty() => {
                t.typed.pop();
            }
            WXK_RETURN | WXK_NUMPAD_ENTER if !t.typed.is_empty() => {
                if t.kind == Kind::Offset {
                    let text = std::mem::take(&mut t.typed);
                    match self.run(
                        "units.parse",
                        json!({ "text": text, "data_type": "length" }),
                    ) {
                        Ok(v) => {
                            let d = v.as_f64().unwrap_or_default();
                            if let Some(t) = &mut self.modify_tool {
                                t.distance = d;
                            }
                            self.offset_distance = d;
                        }
                        Err(e) => self.frame.set_status_text(&e, 0),
                    }
                } else if let Some(Err(e)) = self.commit_xform() {
                    self.frame.set_status_text(&e, 0);
                    return true;
                }
            }
            _ => {
                let Some(c) = u32::try_from(key).ok().and_then(char::from_u32) else {
                    return false;
                };
                let starts = c.is_ascii_digit() || c == '.' || c == '-';
                let continues = !t.typed.is_empty() && (c.is_ascii_graphic() || c == ' ');
                let ready = t.kind == Kind::Offset || !t.points.is_empty();
                if !typing || !ready || !(starts || continues) {
                    return false;
                }
                t.typed.push(c);
            }
        }
        self.modify_prompt();
        self.modify_preview();
        true
    }

    /// The tool's transient lines: the elements as they would end up, rubber bands, the axis and
    /// the picked or hovered references.
    pub(crate) fn modify_preview(&mut self) {
        let Some(t) = &self.modify_tool else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let mm = plan.scale / 1000.0;
        let mut segs: Vec<[[f64; 2]; 2]> = Vec::new();
        let cursor = t.cursor.map(|c| c.point);
        if let Some(c) = cursor {
            let r = f64::from(1.5 * mm);
            segs.extend([
                [[c[0] - r, c[1] - r], [c[0] + r, c[1] - r]],
                [[c[0] + r, c[1] - r], [c[0] + r, c[1] + r]],
                [[c[0] + r, c[1] + r], [c[0] - r, c[1] + r]],
                [[c[0] - r, c[1] + r], [c[0] - r, c[1] - r]],
            ]);
        }
        match t.kind {
            Kind::Move | Kind::Copy | Kind::ArrayLinear => {
                if let (Some(b), Some(c)) = (t.points.first(), cursor) {
                    segs.push([*b, self.typed_point(*b, c).unwrap_or(c)]);
                }
            }
            Kind::Rotate | Kind::ArrayRadial => {
                let c = t.center;
                let r = f64::from(2.5 * mm);
                segs.push([[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]]);
                segs.push([[c[0] - r, c[1] + r], [c[0] + r, c[1] - r]]);
                for p in t.points.iter().chain(cursor.iter()) {
                    segs.push([c, *p]);
                }
            }
            Kind::MirrorDraw => {
                if let (Some(a), Some(b)) = (t.points.first(), cursor) {
                    segs.push([*a, b]);
                }
            }
            _ => {}
        }
        for c in t.hover.iter().chain(t.reference.iter()) {
            segs.push(c.segment);
        }
        if let Some(x) = self.tool_xform() {
            let targets = self
                .modify_tool
                .as_ref()
                .map(|t| t.targets.clone())
                .unwrap_or_default();
            for m in self.array_members(x) {
                segs.extend(self.outline(&targets, Some(m)));
            }
        }
        // Offset: the hovered wall or grid where it would go.
        if let Some(t) = &self.modify_tool
            && t.kind == Kind::Offset
            && let (Some(h), Some(c)) = (&t.hover, cursor)
            && let Some(id) = Self::candidate_element(h)
        {
            let n = [-h.direction[1], h.direction[0]];
            let side = if (c[0] - h.point[0]) * n[0] + (c[1] - h.point[1]) * n[1] >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let v = [n[0] * side * t.distance, n[1] * side * t.distance, 0.0];
            segs.extend(self.outline(&[id], Some(Xform::Translate(v))));
        }
        let lines = Lines {
            color: GHOST,
            width: 0.25,
            segments: segs
                .into_iter()
                .map(|s| s.map(|[x, y]| [x as f32, y as f32, z]))
                .collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Whether a length or angle is being typed in a modify tool (letters then belong to it).
    pub(crate) fn modify_typing(&self) -> bool {
        self.modify_tool
            .as_ref()
            .is_some_and(|t| !t.typed.is_empty())
    }

    /// Pins or unpins the selection (PN / UP).
    pub(crate) fn pin_selection(&mut self, pin: bool) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select the elements to pin or unpin first", 0);
        }
        let command = if pin { "element.pin" } else { "element.unpin" };
        match self.run(command, json!({ "element_ids": ids })) {
            Ok(_) => frame.set_status_text(if pin { "Pinned" } else { "Unpinned" }, 0),
            Err(e) => frame.set_status_text(&e, 0),
        }
        self.properties_stale = true;
    }

    // ---- Grips and dragging (no tool running) ----

    /// The ends of the selected wall or grid in a plan, when exactly one is selected.
    fn grips(&mut self) -> Vec<(ElementId, usize, [f64; 2])> {
        if !self.in_plan() || self.any_tool() {
            return Vec::new();
        }
        let ids = self.selection();
        let [id] = ids[..] else {
            return Vec::new();
        };
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        let Some(e) = store.get(id) else {
            return Vec::new();
        };
        // A grid's or reference plane's line in this view (its 2D extent, else its 3D one).
        let view = self
            .active_view()
            .and_then(|v| v.element)
            .and_then(|v| store.get(v))
            .map(|v| v.guid().to_string());
        let line = reviera_core::walls::wall_recipe(e)
            .map(|r| r.curve)
            .or_else(|| reviera_core::datum_extents::plan_line(e, view.as_deref()));
        line.map_or_else(Vec::new, |l| vec![(id, 0, l.start), (id, 1, l.end)])
    }

    /// Whether clicks and boxes select: no tool or pick mode is running, outside sheets and
    /// walk mode.
    pub(crate) fn picking_free(&self) -> bool {
        !self.any_tool()
            && self.room_tool.is_none()
            && self.section_tool.is_none()
            && self.dome_tool.is_none()
            && self.feature_tool.is_none()
            && self.slice_pick.is_none()
            && self.attach_pick.is_none()
            && self.host_pick.is_none()
            && self.area_pick.is_none()
            && self.group_pick.is_none()
            && self.text_pick.is_none()
            && !self.in_sheet()
            && !self.active_view().is_some_and(|v| v.nav.walk.is_some())
    }

    /// Whether any tool is running.
    pub(crate) fn any_tool(&self) -> bool {
        self.tool.is_some()
            || self.sketch.is_some()
            || self.ceiling_tool.is_some()
            || self.dim_tool.is_some()
            || self.grid_tool.is_some()
            || self.line_tool.is_some()
            || self.insert_tool.is_some()
            || self.modify_tool.is_some()
            || self.note_tool.is_some()
            || self.stair_tool.is_some()
            || self.dormer_tool.is_some()
            || self.railing_tool.is_some()
            || self.column_tool.is_some()
            || self.curtain_tool.is_some()
            || self.ramp_tool.is_some()
            || self.measure_tool.is_some()
            || self.skylight_tool.is_some()
            || self.place_tool.is_some()
            || self.array_tool.is_some()
    }

    /// Shows the grips of the selection (or nothing).
    pub(crate) fn update_grips(&mut self) {
        if self.any_tool() {
            return;
        }
        let grips = self.grips();
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let r = 1.2 * plan.scale / 1000.0;
        let mut lines = Lines {
            color: GRIP,
            width: 0.35,
            segments: Vec::new(),
        };
        // A roof's, floor's or ceiling's outline, and grips at its corners and edge middles.
        let curves = self.footprint().map(|f| f.2).unwrap_or_default();
        for c in &curves {
            for w in c.points().windows(2) {
                let (a, b) = (w[0], w[1]);
                lines
                    .segments
                    .push([[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]);
            }
        }
        let footprint: Vec<[f64; 2]> = Self::footprint_grips(&curves)
            .into_iter()
            .map(|g| g.1)
            .collect();
        for [x, y] in grips.into_iter().map(|g| g.2).chain(footprint) {
            let (x, y) = (x as f32, y as f32);
            let c = [
                [x - r, y - r],
                [x + r, y - r],
                [x + r, y + r],
                [x - r, y + r],
            ];
            for k in 0..4 {
                let (a, b) = (c[k], c[(k + 1) % 4]);
                lines.segments.push([[a[0], a[1], z], [b[0], b[1], z]]);
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// A grip under pixel (`x`, `y`): the element, which end, and the other end.
    pub(crate) fn grip_at(&mut self, x: i32, y: i32) -> Option<(ElementId, usize, [f64; 2])> {
        let grips = self.grips();
        let z = self.active_view()?.plan.as_ref()?.range.cut as f32;
        let hit = grips
            .iter()
            .find(|(_, _, p)| self.pixel_distance(x, y, [*p, *p], z) <= GRIP_PX)?;
        let other = grips.iter().find(|g| g.1 != hit.1)?.2;
        Some((hit.0, hit.1, other))
    }

    /// The snapped point for a grip drag from the fixed end `other`.
    fn grip_point(&self, x: i32, y: i32, other: [f64; 2]) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        self.snapped_at(x, y, Some(other), &lines).map(|s| s.point)
    }

    pub(crate) fn grip_drag_motion(
        &mut self,
        id: ElementId,
        end: usize,
        other: [f64; 2],
        x: i32,
        y: i32,
    ) {
        let Some(p) = self.grip_point(x, y, other) else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let (a, b) = if end == 0 { (p, other) } else { (other, p) };
        let _ = id;
        let lines = Lines {
            color: GHOST,
            width: 0.35,
            segments: vec![[[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]],
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Releases a grip: the wall or grid takes its new line (one undoable step).
    pub(crate) fn grip_drag_end(
        &mut self,
        id: ElementId,
        end: usize,
        other: [f64; 2],
        x: i32,
        y: i32,
        click: bool,
    ) {
        let frame = self.frame;
        if !click && let Some(p) = self.grip_point(x, y, other) {
            let (a, b) = if end == 0 { (p, other) } else { (other, p) };
            let is_wall = self
                .doc
                .and_then(|d| self.app.document(d))
                .and_then(|d| {
                    d.doc
                        .store()
                        .get(id)
                        .and_then(reviera_core::walls::wall_recipe)
                })
                .is_some();
            let line = json!({ "line": { "start": a, "end": b } });
            let result = if is_wall {
                self.run("wall.set_location", json!({ "wall_id": id, "curve": line }))
            } else {
                // A datum's end: in this view only while it has a 2D extent here
                // (ARC-DATUM-001), else its 3D extent.
                let view = self.active_view().and_then(|v| v.element);
                let two_d = self.store_now().is_some_and(|store| {
                    let guid = view
                        .and_then(|v| store.get(v))
                        .map(|v| v.guid().to_string());
                    store
                        .get(id)
                        .zip(guid)
                        .is_some_and(|(e, g)| reviera_core::datum_extents::in_view(e, &g).is_2d())
                });
                self.run(
                    "datum.set_extent",
                    json!({
                        "datum_id": id,
                        "view_id": if two_d { json!(view) } else { json!(null) },
                        "end": if end == 0 { "start" } else { "end" },
                        "point": p,
                    }),
                )
            };
            if let Err(e) = result {
                frame.set_status_text(&e, 0);
            }
            self.refresh_scene(&frame);
        }
        self.update_grips();
    }

    /// Whether a left press at (`x`, `y`) in a plan starts dragging the selected walls and grids.
    pub(crate) fn selection_drag_start(
        &mut self,
        x: i32,
        y: i32,
        panel: &wxdragon::prelude::Panel,
    ) -> Option<[f64; 2]> {
        if !self.in_plan() || self.any_tool() {
            return None;
        }
        let hit = self.element_at(x, y, panel)?;
        let selected = self.selection();
        if !selected.contains(&hit) {
            return None;
        }
        let store = self
            .doc
            .and_then(|d| self.app.document(d))?
            .doc
            .store()
            .clone();
        let movable = |id: &ElementId| {
            store.get(*id).is_some_and(|e| {
                reviera_core::walls::wall_recipe(e).is_some()
                    || reviera_core::grids::is_grid(e)
                    || reviera_core::notes::tag_data(e).is_some()
                    || reviera_core::notes::text_data(e).is_some()
            })
        };
        if !selected.iter().all(movable) {
            return None;
        }
        self.plan_xy(x, y)
    }

    fn drag_vector(&self, from: [f64; 2], x: i32, y: i32) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        let to = self.snapped_at(x, y, Some(from), &lines)?.point;
        Some(sub(to, from))
    }

    pub(crate) fn selection_drag_motion(&mut self, from: [f64; 2], x: i32, y: i32) {
        let Some(v) = self.drag_vector(from, x, y) else {
            return;
        };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let ids = self.selection();
        let segs = self.outline(&ids, Some(Xform::Translate([v[0], v[1], 0.0])));
        let lines = Lines {
            color: GHOST,
            width: 0.25,
            segments: segs
                .into_iter()
                .map(|s| s.map(|[x, y]| [x as f32, y as f32, z]))
                .collect(),
        };
        if let Some(vp) = self.viewport() {
            vp.set_overlay_lines(&[lines]);
        }
    }

    pub(crate) fn selection_drag_end(&mut self, from: [f64; 2], x: i32, y: i32, click: bool) {
        let frame = self.frame;
        if !click && let Some(v) = self.drag_vector(from, x, y) {
            let ids = self.selection();
            if let Err(e) = self.run(
                "element.move",
                json!({ "element_ids": ids, "vector": [v[0], v[1], 0.0] }),
            ) {
                frame.set_status_text(&e, 0);
            }
            self.refresh_scene(&frame);
        }
        self.update_grips();
    }
}
