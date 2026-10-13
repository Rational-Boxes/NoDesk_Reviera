// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketch mode (Ch 3 §3.1.3, ARC-SKCH-001) for floor and ceiling boundaries and roof footprints
//! (ARC-ROOF-001), and the Ceiling tool (ARC-CEIL-001: an automatic ceiling in the room under
//! the cursor, or a sketch).
//!
//! Sketch mode: a modal session in a plan with
//! its own contextual tab (Draw: Pick Walls, Line, Rectangle; Mode: Finish, Cancel).
//!
//! - **Pick Walls** (the default) takes the face of the wall on the cursor's side, locked so the
//!   floor follows the wall; Tab takes the whole chain of joined walls on that side.
//! - **Line** draws chained lines, snapping like the drawing tools; clicking the chain's first
//!   point closes it.
//! - **Rectangle** takes two corners.
//! - **Defines Slope** (roofs) switches the clicked curve between sloping and gable. Slope-defining
//!   curves show a triangle. Walls picked for a roof take the session's overhang.
//!
//! Delete removes the last curve. Finish validates (ARC-SKCH-002): a failed Finish keeps the
//! session open and marks the problem points; Cancel discards the session.

use reviera_core::ElementId;
use reviera_core::floors::SlabKind;
use reviera_core::walls::Line;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};

use super::{Gui, snap};

/// Sketch curves are drawn in magenta (the sketch colour), problems in red.
const SKETCH: [f32; 4] = [0.85, 0.0, 0.75, 1.0];
const HOVER: [f32; 4] = [0.95, 0.45, 0.9, 1.0];
const PROBLEM: [f32; 4] = [0.95, 0.1, 0.05, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawMode {
    PickWalls,
    Line,
    Rectangle,
    /// Click a roof footprint curve to switch it between sloping and gable.
    DefinesSlope,
}

/// What a sketch makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchKind {
    Slab(SlabKind),
    Roof,
}

/// A sketch curve: a line, the wall face it was picked from, and for roofs whether it is a
/// gable, its overhang and its own slope.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub line: Line,
    pub lock: Option<(ElementId, String)>,
    pub gable: bool,
    pub overhang: f64,
    pub slope: Option<f64>,
}

impl Curve {
    pub fn new(line: Line, lock: Option<(ElementId, String)>) -> Self {
        Self {
            line,
            lock,
            gable: false,
            overhang: 0.0,
            slope: None,
        }
    }
}

/// The overhang walls picked for a roof take, metres.
pub const ROOF_OVERHANG: f64 = 0.6;

/// A sketch session.
#[derive(Debug)]
pub struct SketchSession {
    /// Floor, ceiling or roof.
    pub kind: SketchKind,
    /// The overhang of walls picked for a roof.
    pub overhang: f64,
    /// The floor or ceiling being edited (Edit Boundary); `None` for a new one.
    pub host: Option<ElementId>,
    pub level: ElementId,
    pub curves: Vec<Curve>,
    pub mode: DrawMode,
    /// The end of the line chain being drawn, and where the chain started.
    last: Option<[f64; 2]>,
    chain_start: Option<[f64; 2]>,
    /// The first corner of a rectangle.
    corner: Option<[f64; 2]>,
    cursor: Option<snap::Snap>,
    /// The wall faces Pick Walls would take now.
    hover: Vec<Curve>,
    /// Points of the last failed Finish.
    problems: Vec<[f64; 2]>,
}

impl Gui {
    /// Architecture › Floor: a new floor boundary on the plan's level.
    pub(crate) fn start_floor_sketch(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame
                .set_status_text("Floors are sketched in plan views (View › Floor Plan)", 0);
        };
        self.begin_sketch(SketchKind::Slab(SlabKind::Floor), None, level, Vec::new());
    }

    /// Sketch Ceiling: a new ceiling boundary on the plan's level.
    pub(crate) fn start_ceiling_sketch(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(true) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Ceilings are placed in plan views", 0);
        };
        self.end_ceiling_tool();
        self.begin_sketch(SketchKind::Slab(SlabKind::Ceiling), None, level, Vec::new());
    }

    /// Edit Boundary of the selected floor or ceiling.
    pub(crate) fn edit_floor_boundary(&mut self) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame.set_status_text("Edit Boundary works in plan views", 0);
        }
        let ids = self.selection();
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some((floor, (kind, recipe), sketch)) = ids.iter().find_map(|id| {
            let e = store.get(*id)?;
            Some((
                *id,
                reviera_core::floors::slab_recipe(e)?,
                reviera_core::sketch::sketch_of(e)?,
            ))
        }) else {
            return frame.set_status_text("Select a floor or ceiling to edit its boundary", 0);
        };
        let level = reviera_core::GlobalId::parse(&recipe.level)
            .and_then(|g| store.find_by_guid(&g))
            .unwrap_or(ElementId(0));
        let curves = sketch
            .curves
            .iter()
            .map(|c| {
                Curve::new(
                    c.line,
                    c.lock.as_ref().and_then(|l| {
                        let id = reviera_core::GlobalId::parse(&l.element)
                            .and_then(|g| store.find_by_guid(&g))?;
                        Some((id, l.sub.clone()))
                    }),
                )
            })
            .collect();
        self.begin_sketch(SketchKind::Slab(kind), Some(floor), level, curves);
    }

    pub(crate) fn begin_sketch(
        &mut self,
        kind: SketchKind,
        host: Option<ElementId>,
        level: ElementId,
        curves: Vec<Curve>,
    ) {
        let frame = self.frame;
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        self.end_ceiling_tool();
        self.sketch = Some(SketchSession {
            kind,
            overhang: ROOF_OVERHANG,
            host,
            level,
            curves,
            mode: DrawMode::PickWalls,
            last: None,
            chain_start: None,
            corner: None,
            cursor: None,
            hover: Vec::new(),
            problems: Vec::new(),
        });
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        self.sketch_prompt();
        self.sketch_preview();
    }

    /// Ends the session without changing the model.
    pub(crate) fn cancel_sketch(&mut self) {
        if self.sketch.take().is_none() {
            return;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.frame.set_status_text("Sketch cancelled", 0);
        self.update_ribbon_context();
    }

    pub(crate) fn set_draw_mode(&mut self, mode: DrawMode) {
        if let Some(s) = &mut self.sketch {
            s.mode = mode;
            s.last = None;
            s.chain_start = None;
            s.corner = None;
            s.hover.clear();
        }
        self.sketch_prompt();
        self.sketch_preview();
    }

    fn sketch_prompt(&mut self) {
        let Some(s) = &self.sketch else { return };
        let what = match s.mode {
            DrawMode::PickWalls => {
                "Pick Walls: click a wall on the side to follow (Tab takes the joined chain)"
            }
            DrawMode::Line if s.last.is_none() => "Line: click the start point",
            DrawMode::Line => {
                "Line: click the next point; click the first point to close (Esc ends the chain)"
            }
            DrawMode::Rectangle if s.corner.is_none() => "Rectangle: click the first corner",
            DrawMode::Rectangle => "Rectangle: click the opposite corner",
            DrawMode::DefinesSlope => "Defines Slope: click a curve to make it slope or a gable",
        };
        let verb = match (s.host, s.kind) {
            (Some(_), SketchKind::Roof) => "Edit Footprint",
            (Some(_), _) => "Edit Boundary",
            (None, SketchKind::Slab(SlabKind::Floor)) => "Floor",
            (None, SketchKind::Slab(SlabKind::Ceiling)) => "Ceiling",
            (None, SketchKind::Roof) => "Roof by Footprint",
        };
        let text = format!(
            "{verb} · {what} · {} curves · Finish ✓ or Cancel ✗ on the ribbon (Delete removes the last curve)",
            s.curves.len()
        );
        self.frame.set_status_text(&text, 0);
    }

    /// The wall face on the cursor's side as a sketch curve, locked to the wall.
    fn face_curve(&self, wall: ElementId, plus_side: bool) -> Option<Curve> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(wall)?;
        let recipe = reviera_core::walls::wall_recipe(e)?;
        let g = reviera_core::walls::regenerate(store, e).ok()?;
        // The exterior face is on the +normal side unless the wall is flipped.
        let sub = if plus_side != recipe.flip {
            "face:ext"
        } else {
            "face:int"
        };
        let n = g.normal();
        let s = if plus_side { 1.0 } else { -1.0 } * g.width / 2.0;
        let shift = |p: [f64; 2]| [p[0] + n[0] * s, p[1] + n[1] * s];
        Some(Curve::new(
            Line {
                start: shift(g.start),
                end: shift(g.end),
            },
            Some((wall, sub.to_owned())),
        ))
    }

    /// The walls joined end to end with `wall`, in a chain (Tab).
    fn wall_chain(&self, wall: ElementId) -> Vec<ElementId> {
        let Some(store) = self.doc.and_then(|d| self.app.document(d)) else {
            return vec![wall];
        };
        let solved = reviera_core::walls::solve(store.doc.store());
        let mut chain = vec![wall];
        let mut i = 0;
        while i < chain.len() {
            for j in solved.joins_of(chain[i]) {
                // End-to-end joins only (L corners), not T junctions.
                if j.other_end.is_none() {
                    continue;
                }
                for w in [j.wall, j.other] {
                    if !chain.contains(&w) {
                        chain.push(w);
                    }
                }
            }
            i += 1;
        }
        chain
    }

    /// Pick Walls candidates under the cursor: one face, or with `chain` the faces of every
    /// joined wall on the same side (toward or away from the chain's centre).
    fn pick_candidates(&self, x: i32, y: i32, chain: bool) -> Vec<Curve> {
        let Some((wall, point, plus)) = self.wall_under(x, y) else {
            return Vec::new();
        };
        if !chain {
            return self.face_curve(wall, plus).into_iter().collect();
        }
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        let walls = self.wall_chain(wall);
        let geoms: Vec<_> = walls
            .iter()
            .filter_map(|w| {
                Some((
                    *w,
                    reviera_core::walls::regenerate(&store, store.get(*w)?).ok()?,
                ))
            })
            .collect();
        let n = geoms.len().max(1) as f64;
        let centre = geoms.iter().fold([0.0, 0.0], |c, (_, g)| {
            [
                c[0] + (g.start[0] + g.end[0]) / 2.0 / n,
                c[1] + (g.start[1] + g.end[1]) / 2.0 / n,
            ]
        });
        let toward = |g: &reviera_core::walls::WallGeometry, p: [f64; 2]| {
            let nn = g.normal();
            (p[0] - g.start[0]) * nn[0] + (p[1] - g.start[1]) * nn[1] >= 0.0
        };
        // Whether the cursor is on the side facing the chain's centre.
        let inside = geoms
            .iter()
            .find(|(w, _)| *w == wall)
            .is_some_and(|(_, g)| toward(g, centre) == toward(g, point));
        geoms
            .iter()
            .filter_map(|(w, g)| {
                let plus = toward(g, centre) == inside;
                self.face_curve(*w, plus)
            })
            .collect()
    }

    pub(crate) fn sketch_motion(&mut self, x: i32, y: i32) {
        let Some(mode) = self.sketch.as_ref().map(|s| s.mode) else {
            return;
        };
        let from = self.sketch.as_ref().and_then(|s| s.last.or(s.corner));
        let lines = self.sketch_snap_lines();
        let cursor = self.snapped_at(x, y, from, &lines);
        let hover = if mode == DrawMode::PickWalls {
            self.pick_candidates(x, y, false)
        } else {
            Vec::new()
        };
        if let Some(s) = &mut self.sketch {
            s.cursor = cursor;
            s.hover = hover;
        }
        self.sketch_preview();
    }

    /// Lines to snap to: the sketch's curves, walls and the plan's cut edges.
    fn sketch_snap_lines(&self) -> Vec<([f64; 2], [f64; 2])> {
        let mut lines = self.snap_lines();
        if let Some(s) = &self.sketch {
            lines.extend(s.curves.iter().map(|c| (c.line.start, c.line.end)));
        }
        lines
    }

    pub(crate) fn sketch_click(&mut self, x: i32, y: i32) {
        let Some(s) = &self.sketch else { return };
        let p = s.cursor.map(|c| c.point);
        match s.mode {
            DrawMode::PickWalls => {
                let picks = self.pick_candidates(x, y, false);
                self.add_curves(picks);
            }
            DrawMode::Line => {
                let Some(p) = p else { return };
                let s = self.sketch.as_mut().expect("checked");
                match s.last {
                    None => {
                        s.last = Some(p);
                        s.chain_start = Some(p);
                    }
                    Some(a) if a != p => {
                        s.curves.push(Curve::new(Line { start: a, end: p }, None));
                        let closed = s.chain_start == Some(p);
                        s.last = (!closed).then_some(p);
                        if closed {
                            s.chain_start = None;
                        }
                    }
                    Some(_) => {}
                }
            }
            DrawMode::DefinesSlope => {
                let Some(p) = self.plan_xy(x, y) else { return };
                let reach = self
                    .active_view()
                    .and_then(|v| v.plan.as_ref())
                    .map_or(0.3, |plan| 3.0 * f64::from(plan.scale) / 1000.0);
                let s = self.sketch.as_mut().expect("checked");
                let nearest = s
                    .curves
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, distance(p, c.line)))
                    .filter(|(_, d)| *d <= reach)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((i, _)) = nearest {
                    s.curves[i].gable = !s.curves[i].gable;
                }
            }
            DrawMode::Rectangle => {
                let Some(p) = p else { return };
                let s = self.sketch.as_mut().expect("checked");
                match s.corner.take() {
                    None => s.corner = Some(p),
                    Some(a) if a[0] != p[0] && a[1] != p[1] => {
                        let c = [a, [p[0], a[1]], p, [a[0], p[1]]];
                        for k in 0..4 {
                            s.curves.push(Curve::new(
                                Line {
                                    start: c[k],
                                    end: c[(k + 1) % 4],
                                },
                                None,
                            ));
                        }
                    }
                    Some(a) => s.corner = Some(a),
                }
            }
        }
        if let Some(s) = &mut self.sketch {
            s.problems.clear();
        }
        self.sketch_prompt();
        self.sketch_preview();
    }

    fn add_curves(&mut self, picks: Vec<Curve>) {
        let Some(s) = &mut self.sketch else { return };
        for mut c in picks {
            // Walls picked for a roof take the overhang.
            if s.kind == SketchKind::Roof {
                c.overhang = s.overhang;
            }
            // A wall face already in the sketch is not taken twice.
            if c.lock.is_some() && s.curves.iter().any(|k| k.lock == c.lock) {
                continue;
            }
            s.curves.push(c);
        }
    }

    /// Keys in sketch mode: Tab picks a chain of walls, Delete removes the last curve, Esc ends
    /// the line chain. Returns whether the key was used.
    pub(crate) fn sketch_key(&mut self, key: i32, x: i32, y: i32) -> bool {
        use wxdragon::prelude::{WXK_BACK, WXK_DELETE, WXK_ESCAPE, WXK_TAB};
        let Some(s) = &mut self.sketch else {
            return false;
        };
        match key {
            WXK_TAB if s.mode == DrawMode::PickWalls => {
                let picks = self.pick_candidates(x, y, true);
                self.add_curves(picks);
            }
            WXK_DELETE | WXK_BACK => {
                s.curves.pop();
                s.problems.clear();
            }
            WXK_ESCAPE => {
                if s.last.is_none() && s.corner.is_none() {
                    // Esc with nothing in progress leaves the session as it is (Finish or
                    // Cancel ends it).
                    self.frame
                        .set_status_text("Finish ✓ or Cancel ✗ the sketch on the ribbon", 0);
                    return true;
                }
                s.last = None;
                s.chain_start = None;
                s.corner = None;
            }
            _ => return false,
        }
        self.sketch_prompt();
        self.sketch_preview();
        true
    }

    /// Finish: creates the floor (or replaces the edited boundary); a failed check keeps the
    /// session open and marks the problem points.
    pub(crate) fn finish_sketch(&mut self) {
        let frame = self.frame;
        let Some(s) = &self.sketch else { return };
        let curves: Vec<Value> = s
            .curves
            .iter()
            .map(|c| {
                let mut v = json!({ "line": { "start": c.line.start, "end": c.line.end } });
                if let Some((e, sub)) = &c.lock {
                    v["lock"] = json!({ "element": e, "sub": sub });
                }
                if s.kind == SketchKind::Roof {
                    v["defines_slope"] = json!(!c.gable);
                    v["overhang"] = json!(c.overhang);
                    if let Some(a) = c.slope {
                        v["slope"] = json!(a);
                    }
                }
                v
            })
            .collect();
        let (command, args) = match (s.host, s.kind) {
            (Some(r), SketchKind::Roof) => (
                "roof.edit_footprint",
                json!({ "roof_id": r, "curves": curves }),
            ),
            (None, SketchKind::Roof) => {
                let mut a = json!({ "curves": curves, "level_id": s.level });
                // A roof on picked walls sits on their tops; so does one drawn on the top
                // level, which has no level above to sit on.
                if let Some(top) = self
                    .picked_wall_top(s)
                    .or_else(|| self.top_level_wall_top(s.level))
                {
                    a["base_offset"] = json!(top);
                }
                ("roof.create_footprint", a)
            }
            (Some(f), _) => (
                "floor.edit_boundary",
                json!({ "floor_id": f, "loops": [curves] }),
            ),
            (None, SketchKind::Slab(SlabKind::Floor)) => {
                let mut a = json!({ "loops": [curves], "level_id": s.level });
                if let Some(t) = self.floor_type {
                    a["type_id"] = json!(t);
                }
                ("floor.create", a)
            }
            (None, SketchKind::Slab(SlabKind::Ceiling)) => (
                "ceiling.create",
                json!({ "loops": [curves], "level_id": s.level }),
            ),
        };
        match self.app.execute(command, args, &super::user()) {
            Ok(out) => {
                let made = s.host.or_else(|| out.result.as_u64().map(ElementId));
                // A new roof on picked walls offers to attach them.
                let attach: Vec<ElementId> = if s.kind == SketchKind::Roof && s.host.is_none() {
                    s.curves
                        .iter()
                        .filter_map(|c| c.lock.as_ref().map(|l| l.0))
                        .collect()
                } else {
                    Vec::new()
                };
                self.sketch = None;
                if let Some(v) = self.viewport() {
                    v.set_overlay_lines(&[]);
                }
                self.refresh_scene(&frame);
                if let Some(f) = made {
                    let _ = self.run("selection.set", json!({ "ids": [f] }));
                }
                self.selection_changed();
                frame.set_status_text("Finished", 0);
                if let Some(roof) = made.filter(|_| !attach.is_empty()) {
                    self.offer_attach(roof, attach);
                }
            }
            Err(e) => {
                let points: Vec<[f64; 2]> =
                    serde_json::from_value(e.details["points"].clone()).unwrap_or_default();
                if let Some(s) = &mut self.sketch {
                    s.problems = points;
                }
                frame.set_status_text(
                    &format!("{} — the sketch stays open to fix it", e.message),
                    0,
                );
                self.sketch_preview();
            }
        }
    }

    /// Draws the sketch: its curves, the Pick Walls candidates, the line or rectangle being
    /// drawn, and the problem points.
    pub(crate) fn sketch_preview(&mut self) {
        let Some(s) = &self.sketch else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let mm = f64::from(plan.scale) / 1000.0;
        let seg = |a: [f64; 2], b: [f64; 2]| {
            [[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]
        };
        let mut sketch = Lines {
            color: SKETCH,
            width: 0.5,
            segments: Vec::new(),
        };
        for c in &s.curves {
            sketch.segments.push(seg(c.line.start, c.line.end));
            // A roof's slope-defining curves show a triangle on their left (inside a
            // counter-clockwise footprint).
            if s.kind == SketchKind::Roof && !c.gable {
                let (a, b) = (c.line.start, c.line.end);
                let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
                let (u, n) = (
                    [(b[0] - a[0]) / l, (b[1] - a[1]) / l],
                    [-(b[1] - a[1]) / l, (b[0] - a[0]) / l],
                );
                let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let (w, h) = (2.5 * mm, 3.0 * mm);
                let at = |along: f64, out: f64| {
                    [
                        m[0] + u[0] * along + n[0] * out,
                        m[1] + u[1] * along + n[1] * out,
                    ]
                };
                let tri = [at(-w, 1.0 * mm), at(w, 1.0 * mm), at(0.0, 1.0 * mm + h)];
                for k in 0..3 {
                    sketch.segments.push(seg(tri[k], tri[(k + 1) % 3]));
                }
            }
            // Locked curves show a small padlock square at their middle.
            if c.lock.is_some() {
                let m = [
                    (c.line.start[0] + c.line.end[0]) / 2.0,
                    (c.line.start[1] + c.line.end[1]) / 2.0,
                ];
                let r = 1.0 * mm;
                let q = [
                    [m[0] - r, m[1] - r],
                    [m[0] + r, m[1] - r],
                    [m[0] + r, m[1] + r],
                    [m[0] - r, m[1] + r],
                ];
                for k in 0..4 {
                    sketch.segments.push(seg(q[k], q[(k + 1) % 4]));
                }
            }
        }
        let mut hover = Lines {
            color: HOVER,
            width: 0.35,
            segments: Vec::new(),
        };
        for c in &s.hover {
            hover.segments.push(seg(c.line.start, c.line.end));
        }
        if let Some(c) = s.cursor.map(|c| c.point) {
            if let Some(a) = s.last {
                hover.segments.push(seg(a, c));
            }
            if let Some(a) = s.corner {
                let q = [a, [c[0], a[1]], c, [a[0], c[1]]];
                for k in 0..4 {
                    hover.segments.push(seg(q[k], q[(k + 1) % 4]));
                }
            }
            let r = 1.5 * mm;
            hover.segments.push(seg([c[0] - r, c[1]], [c[0] + r, c[1]]));
            hover.segments.push(seg([c[0], c[1] - r], [c[0], c[1] + r]));
        }
        let mut problems = Lines {
            color: PROBLEM,
            width: 0.5,
            segments: Vec::new(),
        };
        for p in &s.problems {
            let r = 4.0 * mm;
            let ring: Vec<[f64; 2]> = (0..=24)
                .map(|k| {
                    let a = k as f64 / 24.0 * std::f64::consts::TAU;
                    [p[0] + r * a.cos(), p[1] + r * a.sin()]
                })
                .collect();
            for w in ring.windows(2) {
                problems.segments.push(seg(w[0], w[1]));
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[sketch, hover, problems]);
        }
    }
}

impl Gui {
    /// The height above the session's level of the highest top among the walls its curves are
    /// locked to.
    fn picked_wall_top(&self, s: &SketchSession) -> Option<f64> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let level = crate::levels_elevation(store, s.level)?;
        s.curves
            .iter()
            .filter_map(|c| {
                let (wall, _) = c.lock.as_ref()?;
                let g = reviera_core::walls::regenerate(store, store.get(*wall)?).ok()?;
                Some(g.base_z + g.height - level)
            })
            .reduce(f64::max)
    }
}

impl Gui {
    /// The highest wall top above `level` among the walls based on it, when `level` is the
    /// highest level (relative to the level, metres).
    fn top_level_wall_top(&self, level: ElementId) -> Option<f64> {
        if self.levels().last().map(|l| l.id) != Some(level) {
            return None;
        }
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let z = crate::levels_elevation(store, level)?;
        let guid = store.get(level)?.guid().to_string();
        store
            .iter()
            .filter(|e| reviera_core::walls::wall_recipe(e).is_some_and(|r| r.base_level == guid))
            .filter_map(|e| reviera_core::walls::regenerate(store, e).ok())
            .map(|g| g.base_z + g.height - z)
            .reduce(f64::max)
    }
}

/// The distance from point `p` to the segment `l`.
fn distance(p: [f64; 2], l: Line) -> f64 {
    let (a, b) = (l.start, l.end);
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-18);
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

pub(crate) const DUPLICATE: &str = "Duplicate Type…";

impl Gui {
    /// Properties of a selected floor: level, height offset, area, perimeter, volume, and its
    /// type with the thickness.
    pub(crate) fn floor_rows(&mut self, grid: &wxdragon::prelude::PropertyGrid, id: ElementId) {
        use wxdragon::prelude::Property;
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let (Some(recipe), Ok(g)) = (
            reviera_core::floors::slab_recipe(e).map(|(_, r)| r),
            reviera_core::floors::regenerate(&store, e),
        ) else {
            return;
        };
        let level = reviera_core::GlobalId::parse(&recipe.level)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|l| store.get(l)?.name.clone())
            .unwrap_or_default();
        grid.append(Property::category("Constraints", "c_floor"));
        grid.append(Property::string("Level", "fl_level", level));
        grid.set_property_read_only("fl_level", true);
        let offset = self.format_length(recipe.offset);
        grid.append(Property::string(
            "Height Offset From Level",
            "fl_offset",
            offset,
        ));
        grid.append(Property::category("Dimensions", "c_floor_dims"));
        let area = self
            .run(
                "units.format",
                json!({ "value": g.area(), "data_type": "area" }),
            )
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{:.2} m²", g.area()));
        let volume = self
            .run(
                "units.format",
                json!({ "value": g.volume(), "data_type": "volume" }),
            )
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{:.3} m³", g.volume()));
        let perimeter = self.format_length(g.perimeter());
        let thickness = self.format_length(g.thickness);
        for (k, (label, value)) in [
            ("Area", area),
            ("Perimeter", perimeter),
            ("Volume", volume),
            ("Thickness", thickness.clone()),
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("fl_q{k}");
            grid.append(Property::string(label, &name, value));
            grid.set_property_read_only(&name, true);
        }
        // The type selector and the type's thickness.
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|t| reviera_core::floors::type_thickness(t).is_some())
            .map(|t| (t.id(), t.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let current = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        let mut choices: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        choices.push((DUPLICATE.to_owned(), choices.len() as i32));
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_floor_type"));
        grid.append(Property::enumeration("Type", "fl_type", choices, index));
        grid.append(Property::string(
            "Thickness (type)",
            "fl_thickness",
            thickness,
        ));
        self.type_rows = current;
    }

    /// An edit of a floor row; `false` if `property` is not one.
    pub(crate) fn floor_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let floors: Vec<ElementId> = {
            let ids = self.selection();
            let store = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store().clone());
            ids.into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(reviera_core::floors::slab_recipe)
                        .is_some()
                })
                .collect()
        };
        let result = match property {
            "fl_offset" => self.run(
                "floor.set_level",
                json!({ "floor_ids": floors, "offset": text }),
            ),
            "fl_thickness" => match self.type_rows {
                Some(t) => self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": "thickness", "value": text }),
                ),
                None => return true,
            },
            "fl_type" if text == DUPLICATE => {
                let Some(current) = self.type_rows else {
                    return true;
                };
                let proposed = self
                    .doc
                    .and_then(|d| self.app.document(d))
                    .map(|d| reviera_core::types::duplicate_name(d.doc.store(), current))
                    .unwrap_or_default();
                let dialog = wxdragon::prelude::TextEntryDialog::builder(
                    &frame,
                    "Name of the new floor type:",
                    "Duplicate Type",
                )
                .with_default_value(&proposed)
                .build();
                if dialog.show_modal() != wxdragon::prelude::ID_OK {
                    return true;
                }
                let name = dialog.get_value().unwrap_or(proposed);
                match self.run(
                    "type.duplicate",
                    json!({ "type_id": current, "new_name": name }),
                ) {
                    Ok(t) => self.run(
                        "element.set_type",
                        json!({ "element_ids": floors, "type_id": t }),
                    ),
                    Err(e) => Err(e),
                }
            }
            "fl_type" => {
                let chosen = self.doc.and_then(|d| self.app.document(d)).and_then(|d| {
                    d.doc
                        .store()
                        .iter()
                        .find(|t| {
                            reviera_core::floors::type_thickness(t).is_some()
                                && t.name.as_deref() == Some(text.as_str())
                        })
                        .map(|t| t.id())
                });
                match chosen {
                    Some(t) => {
                        self.floor_type = Some(t);
                        self.run(
                            "element.set_type",
                            json!({ "element_ids": floors, "type_id": t }),
                        )
                    }
                    None => return true,
                }
            }
            _ => return false,
        };
        if let Err(e) = result {
            super::error(&frame, &format!("The floor was not changed:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }
}

/// The Ceiling tool: an automatic ceiling in the room under the cursor (the default), or Sketch
/// Ceiling from its contextual tab.
#[derive(Debug, Default)]
pub struct CeilingTool {
    /// The loop of wall faces around the cursor at the ceiling's height.
    hover: Vec<Line>,
}

impl Gui {
    /// Architecture › Ceiling.
    pub(crate) fn start_ceiling_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(true) {
            return frame.set_status_text(
                "Ceilings are placed in plan views (ceiling plans are best)",
                0,
            );
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.cancel_sketch();
        self.leave_walk();
        self.ceiling_tool = Some(CeilingTool::default());
        let _ = self.run("selection.clear", json!({}));
        self.selection_changed();
        frame.set_status_text(
            "Automatic Ceiling: click inside a room enclosed by walls (or Sketch Ceiling on the ribbon); Esc ends",
            0,
        );
    }

    pub(crate) fn end_ceiling_tool(&mut self) -> bool {
        if self.ceiling_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.update_ribbon_context();
        true
    }

    /// The level and the room loop around pixel (`x`, `y`) at the default ceiling height.
    fn ceiling_loop(&self, x: i32, y: i32) -> Option<(ElementId, [f64; 2], Vec<Line>)> {
        let plan = self.active_view()?.plan.clone()?;
        let p = self.plan_xy(x, y)?;
        let store = self.app.document(self.doc?)?.doc.store();
        let z = plan.elevation + reviera_core::floors::DEFAULT_CEILING_OFFSET;
        let edges = reviera_core::arrangement::wall_edges(store, z);
        let lp = reviera_core::arrangement::loop_around(&edges, p)?;
        Some((plan.level, p, lp.into_iter().map(|e| e.line).collect()))
    }

    pub(crate) fn ceiling_motion(&mut self, x: i32, y: i32) {
        let hover = self
            .ceiling_loop(x, y)
            .map(|(_, _, l)| l)
            .unwrap_or_default();
        if let Some(t) = &mut self.ceiling_tool {
            t.hover = hover;
        }
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let segments = self
            .ceiling_tool
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

    pub(crate) fn ceiling_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some((level, point, _)) = self.ceiling_loop(x, y) else {
            return frame.set_status_text("No room enclosed by walls here", 0);
        };
        match self.run(
            "ceiling.create_auto",
            json!({ "level_id": level, "point": point }),
        ) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Ceiling placed; click another room, or Esc", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
