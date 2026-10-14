// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketch mode (Ch 3 §3.1.3, ARC-SKCH-001) for floor and ceiling boundaries and roof footprints
//! (ARC-ROOF-001), and the Ceiling tool (ARC-CEIL-001: an automatic ceiling in the room under
//! the cursor, or a sketch).
//!
//! Sketch mode: a modal session in a plan with
//! its own contextual tab (Draw: Pick Walls, Line, Rectangle, Arc, Circle; Mode: Finish,
//! Cancel).
//!
//! - **Pick Walls** (the default) takes the face of the wall on the cursor's side, locked so the
//!   floor follows the wall; Tab takes the whole chain of joined walls on that side.
//! - **Line** draws chained lines, snapping like the drawing tools; clicking the chain's first
//!   point closes it.
//! - **Rectangle** takes two corners.
//! - **Arc** takes its start, its end and a point on it; **Circle** its centre and a point on
//!   it (two half arcs). A curved wall picked with Pick Walls gives its face's curve.
//! - **Defines Slope** (roofs) switches the clicked curve between sloping and gable. Slope-defining
//!   curves show a triangle. Walls picked for a roof take the session's overhang.
//!
//! - **Modify** selects a curve: drag it to move it across (its neighbours stretch to meet
//!   it), drag a corner to move it, Delete removes it; Properties show its length (editable)
//!   and, for roofs, Defines Slope, Slope and Overhang. Editing a finished footprint starts here.
//!
//! Delete removes the selected curve, else the last curve. Finish validates (ARC-SKCH-002): a failed Finish keeps the
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
    /// Select, move and delete curves.
    Select,
    PickWalls,
    Line,
    Rectangle,
    /// Start, end, then a point on the arc.
    Arc,
    /// Centre, then a point on the circle.
    Circle,
    /// Click a roof footprint curve to switch it between sloping and gable.
    DefinesSlope,
}

/// What a sketch makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchKind {
    Slab(SlabKind),
    Roof,
    /// A deck's outline (a deck is a floor with boards and framing, ARC-DECK-001).
    Deck,
    /// A vertical opening's loop (ARC-OPEN-002): edited by its grips only.
    Opening,
}

/// A sketch curve: a line, the wall face it was picked from, and for roofs whether it is a
/// gable, its overhang and its own slope.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub line: Line,
    /// Points it passes through between its ends: an arc (one) or a spline (more).
    pub through: Vec<[f64; 2]>,
    pub lock: Option<(ElementId, String)>,
    pub gable: bool,
    pub overhang: f64,
    pub slope: Option<f64>,
}

impl Curve {
    pub fn new(line: Line, lock: Option<(ElementId, String)>) -> Self {
        Self {
            line,
            through: Vec::new(),
            lock,
            gable: false,
            overhang: 0.0,
            slope: None,
        }
    }
}

impl Curve {
    /// A curve from `line`'s start to its end through `through`.
    pub fn curved(line: Line, through: Vec<[f64; 2]>, lock: Option<(ElementId, String)>) -> Self {
        Self {
            through,
            ..Self::new(line, lock)
        }
    }

    /// Its polyline from start to end (sampled when curved).
    pub fn points(&self) -> Vec<[f64; 2]> {
        let mut pts = vec![self.line.start];
        pts.extend(reviera_core::sketch::interior(self.line, &self.through));
        pts.push(self.line.end);
        pts
    }

    pub fn is_curved(&self) -> bool {
        !self.through.is_empty()
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
    /// A roof finished as an arched roof (ARC-ROOF-008).
    pub arched: bool,
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
    /// The selected curve (Modify).
    pub selected: Option<usize>,
    /// A curve or corner being dragged: what, where the drag started, and the curves before it.
    drag: Option<(SketchDrag, [f64; 2], Vec<Curve>)>,
}

/// What a drag in the sketch moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SketchDrag {
    Curve(usize),
    Corner([f64; 2]),
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

    /// Architecture › Deck: a new deck's outline on the plan's level.
    pub(crate) fn start_deck_sketch(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Decks are sketched in plan views", 0);
        };
        self.begin_sketch(SketchKind::Deck, None, level, Vec::new());
        // A deck usually starts against the house: draw it with lines or a rectangle.
        self.set_draw_mode(DrawMode::Rectangle);
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
        let Some((floor, kind, level, curves)) = ids.iter().find_map(|id| {
            let (kind, _) = reviera_core::floors::slab_recipe(store.get(*id)?)?;
            let (_, level, curves) = self.element_curves(*id)?;
            Some((*id, kind, level, curves))
        }) else {
            return frame.set_status_text("Select a floor or ceiling to edit its boundary", 0);
        };
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
            arched: false,
            host,
            level,
            curves,
            // Editing a finished boundary starts by selecting; a new one by picking walls.
            mode: if host.is_some() {
                DrawMode::Select
            } else {
                DrawMode::PickWalls
            },
            last: None,
            chain_start: None,
            corner: None,
            cursor: None,
            hover: Vec::new(),
            problems: Vec::new(),
            selected: None,
            drag: None,
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
            DrawMode::Select => {
                "Modify: click a curve to select it, drag it or a corner to move it, Delete removes it"
            }
            DrawMode::PickWalls => {
                "Pick Walls: click a wall on the side to follow (Tab takes the joined chain)"
            }
            DrawMode::Line if s.last.is_none() => "Line: click the start point",
            DrawMode::Line => {
                "Line: click the next point; click the first point to close (Esc ends the chain)"
            }
            DrawMode::Rectangle if s.corner.is_none() => "Rectangle: click the first corner",
            DrawMode::Rectangle => "Rectangle: click the opposite corner",
            DrawMode::Arc if s.chain_start.is_none() => "Arc: click its start",
            DrawMode::Arc if s.last.is_none() => "Arc: click its end",
            DrawMode::Arc => "Arc: click a point on the arc",
            DrawMode::Circle if s.corner.is_none() => "Circle: click its centre",
            DrawMode::Circle => "Circle: click a point on it",
            DrawMode::DefinesSlope => "Defines Slope: click a curve to make it slope or a gable",
        };
        let verb = match (s.host, s.kind) {
            (Some(_), SketchKind::Roof) => "Edit Footprint",
            (Some(_), _) => "Edit Boundary",
            (None, SketchKind::Slab(SlabKind::Floor)) => "Floor",
            (None, SketchKind::Slab(SlabKind::Ceiling)) => "Ceiling",
            (None, SketchKind::Roof) => "Roof by Footprint",
            (None, SketchKind::Deck) => "Deck",
            (None, SketchKind::Opening) => "Opening",
        };
        let text = format!(
            "{verb} · {what} · {} curves · Finish ✓ or Cancel ✗ on the ribbon",
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
        // A curved wall's face: its curve, through points along it.
        if let Some(path) = reviera_core::walls::curved::path_of(&recipe) {
            let solved = reviera_core::walls::solve(store);
            let ([minus, plus], _) = reviera_core::walls::curved::outline(
                &path,
                g.width,
                reviera_core::walls::curved::ends_of(&solved, wall),
            );
            let pts = if plus_side { plus } else { minus };
            let n = pts.len();
            // An arc's face is an arc through its middle; a spline's, through points along it.
            let through: Vec<[f64; 2]> = if recipe.through.len() == 1 {
                vec![pts[n / 2]]
            } else {
                let step = (n / 24).max(1);
                (step..n - 1).step_by(step).map(|k| pts[k]).collect()
            };
            return Some(Curve::curved(
                Line {
                    start: pts[0],
                    end: pts[n - 1],
                },
                through,
                Some((wall, sub.to_owned())),
            ));
        }
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
        let store = store.doc.store();
        let solved = reviera_core::walls::solve(store);
        // Location ends, for walls that meet in line (a round room's arcs) without a join.
        let ends: Vec<(ElementId, [[f64; 2]; 2])> = store
            .iter()
            .filter_map(|e| {
                let r = reviera_core::walls::wall_recipe(e)?;
                Some((e.id(), [r.curve.start, r.curve.end]))
            })
            .collect();
        let meets = |a: [[f64; 2]; 2], b: [[f64; 2]; 2]| {
            a.iter()
                .any(|p| b.iter().any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-3))
        };
        let mut chain = vec![wall];
        let mut i = 0;
        while i < chain.len() {
            let mut next: Vec<ElementId> = solved
                .joins_of(chain[i])
                .into_iter()
                // End-to-end joins only (L corners), not T junctions.
                .filter(|j| j.other_end.is_some())
                .flat_map(|j| [j.wall, j.other])
                .collect();
            if let Some((_, mine)) = ends.iter().find(|(id, _)| *id == chain[i]) {
                next.extend(
                    ends.iter()
                        .filter(|(_, theirs)| meets(*mine, *theirs))
                        .map(|(id, _)| *id),
                );
            }
            for w in next {
                if !chain.contains(&w) {
                    chain.push(w);
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
        let paths: std::collections::HashMap<ElementId, reviera_core::walls::curved::Path> = walls
            .iter()
            .filter_map(|w| {
                let r = reviera_core::walls::wall_recipe(store.get(*w)?)?;
                Some((*w, reviera_core::walls::curved::path_of(&r)?))
            })
            .collect();
        let toward_wall = |w: ElementId, g: &reviera_core::walls::WallGeometry, p: [f64; 2]| {
            if let Some(path) = paths.get(&w) {
                let (q, u) = path.at(path.project(p));
                return u[0] * (p[1] - q[1]) - u[1] * (p[0] - q[0]) >= 0.0;
            }
            let nn = g.normal();
            (p[0] - g.start[0]) * nn[0] + (p[1] - g.start[1]) * nn[1] >= 0.0
        };
        // Whether the cursor is on the side facing the chain's centre.
        let inside = geoms
            .iter()
            .find(|(w, _)| *w == wall)
            .is_some_and(|(w, g)| toward_wall(*w, g, centre) == toward_wall(*w, g, point));
        geoms
            .iter()
            .filter_map(|(w, g)| {
                let plus = toward_wall(*w, g, centre) == inside;
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
            DrawMode::Select => {
                let Some(p) = self.plan_xy(x, y) else { return };
                let reach = self.sketch_reach();
                let s = self.sketch.as_mut().expect("checked");
                s.selected = crate::sketch_edit::nearest(&s.curves, p, reach).map(|n| n.0);
                self.properties_stale = true;
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
            DrawMode::Arc => {
                let Some(p) = p else { return };
                let s = self.sketch.as_mut().expect("checked");
                match (s.chain_start, s.last) {
                    (None, _) => s.chain_start = Some(p),
                    (Some(a), None) if a != p => s.last = Some(p),
                    (Some(a), Some(b)) => {
                        let line = Line { start: a, end: b };
                        if reviera_core::sketch::interior(line, &[p]).is_empty() {
                            return self.frame.set_status_text(
                                "Arc: the point must be off the line between the ends",
                                0,
                            );
                        }
                        s.curves.push(Curve::curved(line, vec![p], None));
                        s.chain_start = None;
                        s.last = None;
                    }
                    _ => {}
                }
            }
            DrawMode::Circle => {
                let Some(p) = p else { return };
                let s = self.sketch.as_mut().expect("checked");
                match s.corner.take() {
                    None => s.corner = Some(p),
                    Some(c) => {
                        let r = (p[0] - c[0]).hypot(p[1] - c[1]);
                        if r < 0.01 {
                            s.corner = Some(c);
                        } else {
                            // Two half arcs, counter-clockwise from the east.
                            let (e, w) = ([c[0] + r, c[1]], [c[0] - r, c[1]]);
                            let (n, so) = ([c[0], c[1] + r], [c[0], c[1] - r]);
                            s.curves
                                .push(Curve::curved(Line { start: e, end: w }, vec![n], None));
                            s.curves
                                .push(Curve::curved(Line { start: w, end: e }, vec![so], None));
                        }
                    }
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
        let frame = self.frame;
        let Some(s) = &mut self.sketch else { return };
        for mut c in picks {
            // Roof footprints are straight (an arched roof or a dome makes the curve).
            if s.kind == SketchKind::Roof && c.is_curved() {
                frame.set_status_text(
                    "Roof footprints are straight: use Arched Roof or Dome over curved walls",
                    0,
                );
                continue;
            }
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
                match s.selected.take() {
                    Some(i) if i < s.curves.len() => {
                        s.curves.remove(i);
                    }
                    _ => {
                        s.curves.pop();
                    }
                }
                s.problems.clear();
                self.properties_stale = true;
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
        let curves = curves_json(s.kind, &s.curves);
        let (command, args) = match (s.host, s.kind) {
            (Some(host), kind) => edit_command(kind, host, curves),
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
                if s.arched {
                    ("roof.create_arched", a)
                } else {
                    ("roof.create_footprint", a)
                }
            }
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
            (None, SketchKind::Deck) => (
                "deck.create",
                json!({ "outline": [curves], "level_id": s.level }),
            ),
            // Openings are made by their tools and edited by their grips, not sketched.
            (None, SketchKind::Opening) => return,
        };
        match self.app.execute(command, args, &super::user()) {
            Ok(out) => {
                let made = s.host.or_else(|| out.result.as_u64().map(ElementId));
                // A new roof offers to attach the walls under it, so gables fill.
                let attach: Vec<ElementId> = match made {
                    Some(roof) if s.kind == SketchKind::Roof && s.host.is_none() => self
                        .app
                        .document(self.doc.expect("a document"))
                        .map(|d| reviera_core::walls::attach::walls_below(d.doc.store(), roof))
                        .unwrap_or_default(),
                    _ => Vec::new(),
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
            for w in c.points().windows(2) {
                sketch.segments.push(seg(w[0], w[1]));
            }
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
                let pts = c.points();
                let m = if c.is_curved() {
                    pts[pts.len() / 2]
                } else {
                    [
                        (c.line.start[0] + c.line.end[0]) / 2.0,
                        (c.line.start[1] + c.line.end[1]) / 2.0,
                    ]
                };
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
        // The selected curve, with grips at its corners.
        let mut selected = Lines {
            color: reviera_render::HIGHLIGHT,
            width: 0.7,
            segments: Vec::new(),
        };
        if let Some(c) = s.selected.and_then(|i| s.curves.get(i)) {
            for w in c.points().windows(2) {
                selected.segments.push(seg(w[0], w[1]));
            }
            let r = 1.2 * mm;
            for p in [c.line.start, c.line.end] {
                let q = [
                    [p[0] - r, p[1] - r],
                    [p[0] + r, p[1] - r],
                    [p[0] + r, p[1] + r],
                    [p[0] - r, p[1] + r],
                ];
                for k in 0..4 {
                    selected.segments.push(seg(q[k], q[(k + 1) % 4]));
                }
            }
        }
        let mut hover = Lines {
            color: HOVER,
            width: 0.35,
            segments: Vec::new(),
        };
        for c in &s.hover {
            for w in c.points().windows(2) {
                hover.segments.push(seg(w[0], w[1]));
            }
        }
        if let Some(c) = s.cursor.map(|c| c.point) {
            match s.mode {
                DrawMode::Arc => {
                    let pts = match (s.chain_start, s.last) {
                        (Some(a), None) => vec![a, c],
                        (Some(a), Some(b)) => {
                            Curve::curved(Line { start: a, end: b }, vec![c], None).points()
                        }
                        _ => Vec::new(),
                    };
                    for w in pts.windows(2) {
                        hover.segments.push(seg(w[0], w[1]));
                    }
                }
                DrawMode::Circle => {
                    if let Some(o) = s.corner {
                        let r = (c[0] - o[0]).hypot(c[1] - o[1]);
                        let ring: Vec<[f64; 2]> = (0..=72)
                            .map(|k| {
                                let a = k as f64 / 72.0 * std::f64::consts::TAU;
                                [o[0] + r * a.cos(), o[1] + r * a.sin()]
                            })
                            .collect();
                        for w in ring.windows(2) {
                            hover.segments.push(seg(w[0], w[1]));
                        }
                        hover.segments.push(seg(o, c));
                    }
                }
                _ => {
                    if let Some(a) = s.last {
                        hover.segments.push(seg(a, c));
                    }
                    if let Some(a) = s.corner {
                        let q = [a, [c[0], a[1]], c, [a[0], c[1]]];
                        for k in 0..4 {
                            hover.segments.push(seg(q[k], q[(k + 1) % 4]));
                        }
                    }
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
            v.set_overlay_lines(&[sketch, selected, hover, problems]);
        }
    }
}

impl Gui {
    /// How near (metres) the cursor must be to pick a sketch curve: 3 mm on paper.
    fn sketch_reach(&self) -> f64 {
        self.active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.3, |plan| 3.0 * f64::from(plan.scale) / 1000.0)
    }

    /// A press in Modify: a corner or curve under the cursor starts to drag (and is selected).
    pub(crate) fn sketch_drag_start(&mut self, x: i32, y: i32) -> Option<[f64; 2]> {
        let p = self.plan_xy(x, y)?;
        let reach = self.sketch_reach();
        let s = self
            .sketch
            .as_mut()
            .filter(|s| s.mode == DrawMode::Select)?;
        let (i, corner) = crate::sketch_edit::nearest(&s.curves, p, reach)?;
        let what = match corner {
            Some(c) => SketchDrag::Corner(c),
            None => SketchDrag::Curve(i),
        };
        s.selected = Some(i);
        s.drag = Some((what, p, s.curves.clone()));
        self.properties_stale = true;
        self.sketch_preview();
        Some(p)
    }

    /// Dragging: the curves are moved from where they were when the drag started.
    pub(crate) fn sketch_drag_motion(&mut self, x: i32, y: i32) {
        let from = self
            .sketch
            .as_ref()
            .and_then(|s| s.drag.as_ref().map(|d| d.1));
        let lines = self.snap_lines();
        let Some(to) = self.snapped_at(x, y, from, &lines).map(|c| c.point) else {
            return;
        };
        let Some(s) = &mut self.sketch else { return };
        let Some((what, from, original)) = s.drag.clone() else {
            return;
        };
        let mut curves = original;
        match what {
            SketchDrag::Curve(i) => {
                crate::sketch_edit::move_curve(&mut curves, i, [to[0] - from[0], to[1] - from[1]]);
            }
            SketchDrag::Corner(c) => crate::sketch_edit::move_corner(&mut curves, c, to),
        }
        s.curves = curves;
        s.problems.clear();
        self.sketch_preview();
    }

    pub(crate) fn sketch_drag_end(&mut self, x: i32, y: i32, click: bool) {
        if !click {
            self.sketch_drag_motion(x, y);
        }
        if let Some(s) = &mut self.sketch {
            if click && let Some((_, _, original)) = s.drag.take() {
                // A click only selects.
                s.curves = original;
            }
            s.drag = None;
        }
        self.properties_stale = true;
        self.sketch_prompt();
        self.sketch_preview();
    }

    /// Properties of the selected sketch curve; `false` when none is selected.
    pub(crate) fn sketch_rows(&mut self, grid: &wxdragon::prelude::PropertyGrid) -> bool {
        use wxdragon::prelude::Property;
        let Some(s) = &self.sketch else { return false };
        let roof = s.kind == SketchKind::Roof;
        let Some(c) = s.selected.and_then(|i| s.curves.get(i)).cloned() else {
            return false;
        };
        let length: f64 = c
            .points()
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum();
        grid.append(Property::category("Sketch Curve", "c_skc"));
        let text = self.format_length(length);
        grid.append(Property::string("Length", "skc_length", text));
        if c.is_curved() {
            grid.set_property_read_only("skc_length", true);
            grid.set_help_string("skc_length", "Along the curve; drag its ends to change it");
        }
        let lock = if c.lock.is_some() {
            "Locked to its wall"
        } else {
            "Free"
        };
        grid.append(Property::string("Follows Wall", "skc_lock", lock));
        grid.set_property_read_only("skc_lock", true);
        if roof {
            grid.append(Property::boolean(
                "Defines Roof Slope",
                "skc_slopes",
                !c.gable,
            ));
            let slope = c.slope.map_or_else(String::new, |a| {
                self.run("units.format", json!({ "value": a, "data_type": "angle" }))
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{:.2}°", a.to_degrees()))
            });
            grid.append(Property::string("Slope", "skc_slope", slope));
            grid.set_help_string("skc_slope", "This curve's own slope; empty uses the roof's");
            let overhang = self.format_length(c.overhang);
            grid.append(Property::string("Overhang", "skc_overhang", overhang));
        }
        true
    }

    /// An edit of the selected sketch curve's row; `false` if `property` is not one.
    pub(crate) fn sketch_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(i) = self.sketch.as_ref().and_then(|s| s.selected) else {
            return false;
        };
        if !property.starts_with("skc_") {
            return false;
        }
        let text = text.unwrap_or_default();
        let parse = |g: &mut Gui, t: &str, data_type: &str| {
            g.run("units.parse", json!({ "text": t, "data_type": data_type }))
                .ok()
                .and_then(|v| v.as_f64())
        };
        let value = match property {
            "skc_length" | "skc_overhang" => parse(self, &text, "length"),
            "skc_slope" if !text.trim().is_empty() => parse(self, &text, "angle"),
            _ => None,
        };
        let frame = self.frame;
        let Some(s) = &mut self.sketch else {
            return true;
        };
        match property {
            "skc_length" => match value {
                Some(l) if l > 0.0 => crate::sketch_edit::set_length(&mut s.curves, i, l),
                _ => super::error(&frame, "The length must be a positive length"),
            },
            "skc_overhang" => match value {
                Some(o) if o >= 0.0 => s.curves[i].overhang = o,
                _ => super::error(&frame, "The overhang must be a length of 0 or more"),
            },
            "skc_slope" if text.trim().is_empty() => s.curves[i].slope = None,
            "skc_slope" => match value {
                Some(a) if a > 0.0 && a < 89.9f64.to_radians() => s.curves[i].slope = Some(a),
                _ => super::error(&frame, "The slope must be between 0° and 89.9°"),
            },
            "skc_slopes" => s.curves[i].gable = !(text.eq_ignore_ascii_case("true") || text == "1"),
            _ => {}
        }
        self.properties_stale = true;
        self.sketch_preview();
        true
    }

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
    pub(crate) fn top_level_wall_top(&self, level: ElementId) -> Option<f64> {
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

/// Sketch curves as the create and edit commands take them.
pub(crate) fn curves_json(kind: SketchKind, curves: &[Curve]) -> Vec<Value> {
    curves
        .iter()
        .map(|c| {
            let mut v = json!({ "line": { "start": c.line.start, "end": c.line.end } });
            if c.is_curved() {
                v["through"] = json!(c.through);
            }
            if let Some((e, sub)) = &c.lock {
                v["lock"] = json!({ "element": e, "sub": sub });
            }
            if kind == SketchKind::Roof {
                v["defines_slope"] = json!(!c.gable);
                v["overhang"] = json!(c.overhang);
                if let Some(a) = c.slope {
                    v["slope"] = json!(a);
                }
            }
            v
        })
        .collect()
}

/// The command and arguments that give `host` (a roof, floor or ceiling) new curves.
pub(crate) fn edit_command(
    kind: SketchKind,
    host: ElementId,
    curves: Vec<Value>,
) -> (&'static str, Value) {
    match kind {
        SketchKind::Roof => (
            "roof.edit_footprint",
            json!({ "roof_id": host, "curves": curves }),
        ),
        SketchKind::Slab(_) | SketchKind::Deck => (
            "floor.edit_boundary",
            json!({ "floor_id": host, "loops": [curves] }),
        ),
        SketchKind::Opening => {
            let corners: Vec<Value> = curves.iter().map(|c| c["line"]["start"].clone()).collect();
            (
                "opening.set_loops",
                json!({ "opening_id": host, "loops": [corners] }),
            )
        }
    }
}

impl Gui {
    /// The curves of a roof, floor or ceiling as they are now (each loop's corners where its
    /// curves' current lines meet), with their locks and a roof's edge data; and its kind and
    /// level.
    pub(crate) fn element_curves(
        &self,
        id: ElementId,
    ) -> Option<(SketchKind, ElementId, Vec<Curve>)> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(id)?;
        // A vertical opening: its (first) loop as straight curves, on its host's level.
        if let Some(r) = reviera_core::slab_openings::opening_recipe(e) {
            let level = reviera_core::GlobalId::parse(&r.host)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|h| reviera_core::floors::slab_recipe(store.get(h)?))
                .and_then(|(_, f)| reviera_core::GlobalId::parse(&f.level))
                .and_then(|g| store.find_by_guid(&g))
                .unwrap_or(ElementId(0));
            let ring = r.loops.first()?;
            let curves = (0..ring.len())
                .map(|i| {
                    Curve::curved(
                        Line {
                            start: ring[i],
                            end: ring[(i + 1) % ring.len()],
                        },
                        Vec::new(),
                        None,
                    )
                })
                .collect();
            return Some((SketchKind::Opening, level, curves));
        }
        let sketch = reviera_core::sketch::sketch_of(e)?;
        let roof = reviera_core::roofs::roof_recipe(e);
        let (kind, level) = match (&roof, reviera_core::floors::slab_recipe(e)) {
            (Some(r), _) => (SketchKind::Roof, r.level.clone()),
            (None, Some((k, r))) => (SketchKind::Slab(k), r.level),
            _ => return None,
        };
        let level = reviera_core::GlobalId::parse(&level)
            .and_then(|g| store.find_by_guid(&g))
            .unwrap_or(ElementId(0));
        let mut curves = Vec::new();
        for lp in reviera_core::sketch::loop_curves(store, &sketch) {
            for lc in lp {
                let Some(c) = sketch.curves.iter().find(|c| c.key == lc.key) else {
                    continue;
                };
                let lock = c.lock.as_ref().and_then(|l| {
                    let id = reviera_core::GlobalId::parse(&l.element)
                        .and_then(|g| store.find_by_guid(&g))?;
                    Some((id, l.sub.clone()))
                });
                // A curve following a curved face passes through its points now.
                let through = if c.through.is_empty() {
                    Vec::new()
                } else if lock.is_some() {
                    lc.interior.clone()
                } else {
                    c.through.clone()
                };
                let mut curve = Curve::curved(lc.line, through, lock);
                if let Some(r) = &roof {
                    let edge = r.edge(lc.key);
                    curve.gable = !edge.defines_slope;
                    curve.overhang = edge.overhang;
                    curve.slope = edge.slope;
                }
                curves.push(curve);
            }
        }
        Some((kind, level, curves))
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
