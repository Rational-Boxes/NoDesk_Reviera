// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sketch mode (Ch 3 §3.1.3, ARC-SKCH-001) for floor boundaries: a modal session in a plan with
//! its own contextual tab (Draw: Pick Walls, Line, Rectangle; Mode: Finish, Cancel).
//!
//! - **Pick Walls** (the default) takes the face of the wall on the cursor's side, locked so the
//!   floor follows the wall; Tab takes the whole chain of joined walls on that side.
//! - **Line** draws chained lines, snapping like the drawing tools; clicking the chain's first
//!   point closes it.
//! - **Rectangle** takes two corners.
//!
//! Delete removes the last curve. Finish validates (ARC-SKCH-002): a failed Finish keeps the
//! session open and marks the problem points; Cancel discards the session.

use reviera_core::ElementId;
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
}

/// A sketch curve: a line, and the wall face it was picked from.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub line: Line,
    pub lock: Option<(ElementId, String)>,
}

/// A sketch session.
#[derive(Debug)]
pub struct SketchSession {
    /// The floor being edited (Edit Boundary); `None` for a new floor.
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
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame
                .set_status_text("Floors are sketched in plan views (View › Floor Plan)", 0);
        };
        self.begin_sketch(None, level, Vec::new());
    }

    /// Edit Boundary of the selected floor.
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
        let Some((floor, recipe, sketch)) = ids.iter().find_map(|id| {
            let e = store.get(*id)?;
            Some((
                *id,
                reviera_core::floors::floor_recipe(e)?,
                reviera_core::sketch::sketch_of(e)?,
            ))
        }) else {
            return frame.set_status_text("Select a floor to edit its boundary", 0);
        };
        let level = reviera_core::GlobalId::parse(&recipe.level)
            .and_then(|g| store.find_by_guid(&g))
            .unwrap_or(ElementId(0));
        let curves = sketch
            .curves
            .iter()
            .map(|c| Curve {
                line: c.line,
                lock: c.lock.as_ref().and_then(|l| {
                    let id = reviera_core::GlobalId::parse(&l.element)
                        .and_then(|g| store.find_by_guid(&g))?;
                    Some((id, l.sub.clone()))
                }),
            })
            .collect();
        self.begin_sketch(Some(floor), level, curves);
    }

    fn begin_sketch(&mut self, host: Option<ElementId>, level: ElementId, curves: Vec<Curve>) {
        let frame = self.frame;
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        self.sketch = Some(SketchSession {
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
        };
        let verb = if s.host.is_some() {
            "Edit Boundary"
        } else {
            "Floor"
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
        Some(Curve {
            line: Line {
                start: shift(g.start),
                end: shift(g.end),
            },
            lock: Some((wall, sub.to_owned())),
        })
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
                        s.curves.push(Curve {
                            line: Line { start: a, end: p },
                            lock: None,
                        });
                        let closed = s.chain_start == Some(p);
                        s.last = (!closed).then_some(p);
                        if closed {
                            s.chain_start = None;
                        }
                    }
                    Some(_) => {}
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
                            s.curves.push(Curve {
                                line: Line {
                                    start: c[k],
                                    end: c[(k + 1) % 4],
                                },
                                lock: None,
                            });
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
        for c in picks {
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
                v
            })
            .collect();
        let (command, args) = match s.host {
            Some(f) => (
                "floor.edit_boundary",
                json!({ "floor_id": f, "loops": [curves] }),
            ),
            None => {
                let mut a = json!({ "loops": [curves], "level_id": s.level });
                if let Some(t) = self.floor_type {
                    a["type_id"] = json!(t);
                }
                ("floor.create", a)
            }
        };
        match self.app.execute(command, args, &super::user()) {
            Ok(out) => {
                let made = s.host.or_else(|| out.result.as_u64().map(ElementId));
                self.sketch = None;
                if let Some(v) = self.viewport() {
                    v.set_overlay_lines(&[]);
                }
                self.refresh_scene(&frame);
                if let Some(f) = made {
                    let _ = self.run("selection.set", json!({ "ids": [f] }));
                }
                self.selection_changed();
                frame.set_status_text("Floor finished", 0);
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

const DUPLICATE: &str = "Duplicate Type…";

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
            reviera_core::floors::floor_recipe(e),
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
                        .and_then(reviera_core::floors::floor_recipe)
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
