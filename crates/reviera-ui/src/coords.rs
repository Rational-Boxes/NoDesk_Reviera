// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Coordinates (XY): exact points for every tool that takes them, beside sketching with the
//! mouse.
//!
//! The dialog edits what is in progress, or what is selected:
//!
//! - **Sketch mode** (floors, roofs, openings, …): each curve on its own line,
//!   `x, y; x, y` (more points make a chain of lines, `via x, y` an arc), so a sketched line can
//!   be put exactly where it belongs; new lines add curves.
//! - **Reinforcement and plate sketches**: their points, one per line.
//! - **Any other tool** taking points (walls, beams, columns, grids, lines, railings, …): the
//!   points are handed to the tool one by one, exactly as clicks, and may finish it.
//! - **A selected element**: a wall's, beam's or grid's ends, a plate's contour, an area or
//!   fabric area's boundary or a path system's path, changed in one step.
//!
//! A point is `x, y` (absolute), `@dx, dy` (from the point before) or `@d<a` (a distance at an
//! angle in degrees from the x axis), lengths in project units.

use std::cell::Cell;

use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::sketch::Curve;
use super::{Gui, error};

thread_local! {
    static TYPED: Cell<Option<[f64; 2]>> = const { Cell::new(None) };
}

/// The point the Coordinates dialog is handing to a tool, which the tools' point functions
/// return in place of the cursor's.
pub(crate) fn typed() -> Option<[f64; 2]> {
    TYPED.with(Cell::get)
}

/// Sets the typed point for the life of the guard.
struct Feeding;

impl Feeding {
    fn new(p: [f64; 2]) -> Self {
        TYPED.with(|t| t.set(Some(p)));
        Feeding
    }
}

impl Drop for Feeding {
    fn drop(&mut self) {
        TYPED.with(|t| t.set(None));
    }
}

/// What the dialog edits.
enum Target {
    Sketch,
    Rebar,
    Plate,
    Tool,
    Element(ElementId, Kind),
}

#[derive(Clone, Copy)]
enum Kind {
    Wall,
    Grid,
    Beam,
    Plate,
    Area,
    FabricArea,
    Path,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Wall => "wall",
            Kind::Grid => "grid",
            Kind::Beam => "beam",
            Kind::Plate => "plate",
            Kind::Area => "area reinforcement",
            Kind::FabricArea => "fabric area",
            Kind::Path => "path reinforcement",
        }
    }
}

/// Reads points, `x, y`, `@dx, dy` or `@d<a`, each from the one before (`last` before the first).
fn parse_points(gui: &mut Gui, text: &str, mut last: Option<[f64; 2]>) -> Result<Vec<[f64; 2]>, String> {
    let mut out = Vec::new();
    for raw in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let p = parse_point(gui, raw, last)?;
        out.push(p);
        last = Some(p);
    }
    Ok(out)
}

fn parse_point(gui: &mut Gui, raw: &str, last: Option<[f64; 2]>) -> Result<[f64; 2], String> {
    let bad = || format!("`{raw}` is not a point (x, y  or  @dx, dy  or  @distance<angle)");
    let mut len = |t: &str| gui.parse_length_text(t.trim()).ok_or_else(bad);
    // `<distance<angle`: polar from the point before, as the spec writes it.
    if let Some(rest) = raw.strip_prefix('<') {
        let base = last.ok_or_else(|| format!("`{raw}` is relative, but there is no point before it"))?;
        let (d, a) = rest.split_once('<').ok_or_else(bad)?;
        let d = len(d)?;
        let a: f64 = a.trim().trim_end_matches('°').parse().map_err(|_| bad())?;
        let a = a.to_radians();
        return Ok([base[0] + d * a.cos(), base[1] + d * a.sin()]);
    }
    if let Some(rel) = raw.strip_prefix('@') {
        let base = last.ok_or_else(|| format!("`{raw}` is relative, but there is no point before it"))?;
        if let Some((d, a)) = rel.split_once('<') {
            let d = len(d)?;
            let a: f64 = a.trim().trim_end_matches('°').parse().map_err(|_| bad())?;
            let a = a.to_radians();
            return Ok([base[0] + d * a.cos(), base[1] + d * a.sin()]);
        }
        let (x, y) = rel.split_once(',').ok_or_else(bad)?;
        return Ok([base[0] + len(x)?, base[1] + len(y)?]);
    }
    let (x, y) = raw.split_once(',').ok_or_else(bad)?;
    Ok([len(x)?, len(y)?])
}

impl Gui {
    pub(crate) fn parse_length_text(&mut self, text: &str) -> Option<f64> {
        self.run("units.parse", json!({ "text": text, "data_type": "length" })).ok().and_then(|v| v.as_f64())
    }

    fn show_point(&mut self, p: [f64; 2]) -> String {
        format!("{}, {}", self.format_length(p[0]), self.format_length(p[1]))
    }

    /// The selected element whose points the dialog can edit.
    fn coordinate_element(&mut self) -> Option<(ElementId, Kind)> {
        let store = self.store_now()?;
        let sel = self.selection();
        let [id] = sel[..] else { return None };
        let e = store.get(id)?;
        let kind = if reviera_core::walls::wall_recipe(e).is_some_and(|r| r.through.is_empty()) {
            Kind::Wall
        } else if reviera_core::grids::grid_data(e).is_some() {
            Kind::Grid
        } else if reviera_core::beams::beam_recipe(e).is_some() {
            Kind::Beam
        } else if reviera_core::steel_detail::plate_recipe(e).is_some() {
            Kind::Plate
        } else if reviera_core::rebar_systems::area_recipe(e).is_some() {
            Kind::Area
        } else if reviera_core::rebar_systems::fabric_area_recipe(e).is_some() {
            Kind::FabricArea
        } else if reviera_core::rebar_systems::path_recipe(e).is_some() {
            Kind::Path
        } else {
            return None;
        };
        Some((id, kind))
    }

    /// An element's points as the dialog shows them.
    fn element_points(&mut self, id: ElementId, kind: Kind) -> Vec<[f64; 2]> {
        let Some(store) = self.store_now() else { return Vec::new() };
        let Some(e) = store.get(id) else { return Vec::new() };
        use reviera_core::rebar_systems as rs;
        match kind {
            Kind::Wall => reviera_core::walls::wall_recipe(e).map(|r| vec![r.curve.start, r.curve.end]),
            Kind::Grid => reviera_core::grids::grid_data(e).map(|d| vec![d.curve.start, d.curve.end]),
            Kind::Beam => reviera_core::beams::beam_recipe(e).map(|r| vec![r.start, r.end]),
            Kind::Plate => reviera_core::steel_detail::plate_recipe(e).map(|r| r.contour),
            Kind::Area => rs::area_recipe(e).and_then(|r| r.boundary.or_else(|| outline_of(&store, &r.host))),
            Kind::FabricArea => rs::fabric_area_recipe(e).and_then(|r| r.boundary.or_else(|| outline_of(&store, &r.host))),
            Kind::Path => rs::path_recipe(e).map(|r| r.path),
        }
        .unwrap_or_default()
    }

    /// Modify › Coordinates (XY).
    pub(crate) fn coordinates(&mut self) {
        let frame = self.frame;
        let target = if self.sketch.is_some() {
            Target::Sketch
        } else if self.rebar_sketch.is_some() {
            Target::Rebar
        } else if matches!(self.steel_tool, Some(super::steel_detail_ui::SteelTool::Plate { .. })) {
            Target::Plate
        } else if self.tool_active() {
            Target::Tool
        } else if let Some((id, kind)) = self.coordinate_element() {
            Target::Element(id, kind)
        } else {
            return frame.set_status_text(
                "Coordinates: start a tool or a sketch, or select a wall, beam, grid, plate or reinforcement system",
                0,
            );
        };
        // What is there now, and how it is read.
        let (title, about, text, finish) = match &target {
            Target::Sketch => {
                let curves = self.sketch.as_ref().map(|s| s.curves.clone()).unwrap_or_default();
                let lines: Vec<String> = curves
                    .iter()
                    .map(|c| {
                        let mut l = format!("{}; {}", self.show_point(c.line.start), self.show_point(c.line.end));
                        if let Some(m) = c.through.first() {
                            l += &format!(" via {}", self.show_point(*m));
                        }
                        l
                    })
                    .collect();
                ("Sketch", "Plan x, y. One curve per line: start; end (more points chain lines; via x, y makes an arc). Add lines for new curves.", lines.join("\n"), Some("Finish the sketch"))
            }
            Target::Rebar => {
                let pts = self.rebar_sketch.as_ref().map(|t| t.points.clone()).unwrap_or_default();
                let lines: Vec<String> = pts.into_iter().map(|p| self.show_point(p)).collect();
                ("Reinforcement Sketch", "In the host's plane: plan x, y for floors; along the wall and up from its base for walls. One point per line.", lines.join("\n"), Some("Finish"))
            }
            Target::Plate => {
                let pts = match &self.steel_tool {
                    Some(super::steel_detail_ui::SteelTool::Plate { points, .. }) => points.clone(),
                    _ => Vec::new(),
                };
                let lines: Vec<String> = pts.into_iter().map(|p| self.show_point(p)).collect();
                ("Plate Contour", "In the face's or work plane's coordinates. One corner per line.", lines.join("\n"), Some("Finish the plate"))
            }
            Target::Tool => (
                "Coordinates",
                "Points handed to the tool in progress, one per line, as if clicked (plan x, y; a plate or reinforcement sketch's own plane).",
                String::new(),
                Some("Then press Enter (finish the chain)"),
            ),
            Target::Element(id, kind) => {
                let pts = self.element_points(*id, *kind);
                let lines: Vec<String> = pts.into_iter().map(|p| self.show_point(p)).collect();
                let about = match kind {
                    Kind::Plate => "The plate's corners in its plane, one per line.",
                    Kind::Area | Kind::FabricArea | Kind::Path => "In the host's plane (plan x, y for floors; along and up for walls), one point per line.",
                    _ => "Plan x, y: the start, then the end.",
                };
                (kind.name(), about, lines.join("\n"), None)
            }
        };
        let dialog = Dialog::builder(&frame, &format!("Coordinates — {title}")).with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let note = StaticText::builder(&dialog)
            .with_label(&format!("{about}\nx, y  ·  @dx, dy from the point before  ·  @distance<angle or <distance<angle (degrees)"))
            .build();
        sizer.add(&note, 0, SizerFlag::All, 10);
        let edit = TextCtrl::builder(&dialog).with_value(&text).with_style(TextCtrlStyle::MultiLine).build();
        edit.set_min_size(Size::new(420, 200));
        sizer.add(&edit, 1, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, 10);
        let done = finish.map(|label| {
            let c = CheckBox::builder(&dialog).with_label(label).build();
            sizer.add(&c, 0, SizerFlag::All, 10);
            c
        });
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Apply").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let text = edit.get_value();
        let finish = done.is_some_and(|c| c.get_value());
        dialog.destroy();
        let result = match target {
            Target::Sketch => self.apply_sketch_coordinates(&text, finish),
            Target::Rebar => self.apply_rebar_coordinates(&text, finish),
            Target::Plate => self.apply_plate_coordinates(&text, finish),
            Target::Tool => self.feed_coordinates(&text, finish),
            Target::Element(id, kind) => self.apply_element_coordinates(id, kind, &text),
        };
        if let Err(e) = result {
            error(&frame, &format!("The coordinates were not applied:\n{e}"));
        }
    }

    fn apply_sketch_coordinates(&mut self, text: &str, finish: bool) -> Result<(), String> {
        let old = self.sketch.as_ref().map(|s| s.curves.clone()).unwrap_or_default();
        let mut curves: Vec<Curve> = Vec::new();
        let mut last: Option<[f64; 2]> = None;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (pts, via) = match line.split_once(" via ") {
                Some((a, b)) => (a, Some(b)),
                None => (line, None),
            };
            let pts = parse_points(self, pts, last)?;
            if pts.len() < 2 {
                return Err(format!("`{line}` needs a start and an end"));
            }
            let via = via.map(|v| parse_point(self, v.trim(), pts.first().copied())).transpose()?;
            for w in pts.windows(2) {
                let l = reviera_core::walls::Line { start: w[0], end: w[1] };
                let k = curves.len();
                // A curve left where it was keeps what it was picked from and its roof data.
                let mut c = match old.get(k) {
                    Some(o) if o.line == l => o.clone(),
                    Some(o) => Curve { line: l, through: Vec::new(), lock: None, ..o.clone() },
                    None => Curve::new(l, None),
                };
                if pts.len() == 2 {
                    if let Some(m) = via {
                        c.through = vec![m];
                    } else if old.get(k).is_none_or(|o| o.line != l) {
                        c.through.clear();
                    }
                }
                curves.push(c);
            }
            last = pts.last().copied();
        }
        let mode = self.sketch.as_ref().map(|s| s.mode);
        if let Some(s) = &mut self.sketch {
            s.curves = curves;
            s.selected = None;
        }
        if let Some(m) = mode {
            self.set_draw_mode(m);
        }
        if finish {
            self.finish_sketch();
        }
        Ok(())
    }

    fn apply_rebar_coordinates(&mut self, text: &str, finish: bool) -> Result<(), String> {
        let pts = parse_points(self, &text.replace('\n', ";"), None)?;
        if let Some(t) = &mut self.rebar_sketch {
            // Typed corners make the boundary itself, not a rectangle's two corners.
            if t.rectangle && pts.len() != 2 {
                t.rectangle = false;
            }
            t.points = pts;
        }
        if finish {
            self.finish_rebar_sketch();
        } else {
            let (x, y) = self.mouse;
            self.rebar_sketch_motion(x, y);
        }
        Ok(())
    }

    fn apply_plate_coordinates(&mut self, text: &str, finish: bool) -> Result<(), String> {
        let pts = parse_points(self, &text.replace('\n', ";"), None)?;
        if let Some(super::steel_detail_ui::SteelTool::Plate { points, .. }) = &mut self.steel_tool {
            *points = pts;
        }
        let frame = self.frame;
        if finish {
            self.tools_key(WXK_RETURN, false, &frame);
        } else {
            let (x, y) = self.mouse;
            self.steel_motion(x, y);
        }
        Ok(())
    }

    /// Hands points to the tool in progress as clicks at the mouse, each its own typed point.
    fn feed_coordinates(&mut self, text: &str, finish: bool) -> Result<(), String> {
        let frame = self.frame;
        // Relative points start from the tool's last point.
        let last = self.tool.as_ref().and_then(|t| t.last).or_else(|| self.sketch_last());
        let pts = parse_points(self, &text.replace('\n', ";"), last)?;
        let Some(panel) = self.active_view().map(|v| v.panel) else {
            return Err("there is no view to work in".into());
        };
        let (x, y) = self.mouse;
        for p in pts {
            let _typed = Feeding::new(p);
            self.tools_motion(x, y, &panel, &frame);
            if !self.tools_click(x, y, &panel, &frame) {
                return Err("no tool is taking points".into());
            }
        }
        if finish {
            self.tools_key(WXK_RETURN, false, &frame);
        }
        panel.refresh(false, None);
        Ok(())
    }

    fn apply_element_coordinates(&mut self, id: ElementId, kind: Kind, text: &str) -> Result<(), String> {
        let frame = self.frame;
        let pts = parse_points(self, &text.replace('\n', ";"), None)?;
        let ends = || -> Result<([f64; 2], [f64; 2]), String> {
            match pts[..] {
                [a, b] => Ok((a, b)),
                _ => Err(format!("a {} takes two points, its start and its end", kind.name())),
            }
        };
        let (command, args): (&str, Value) = match kind {
            Kind::Wall => {
                let (a, b) = ends()?;
                ("wall.set_location", json!({ "wall_id": id, "curve": { "line": { "start": a, "end": b } } }))
            }
            Kind::Grid => {
                let (a, b) = ends()?;
                ("grid.set_curve", json!({ "grid_id": id, "curve": { "line": { "start": a, "end": b } } }))
            }
            Kind::Beam => {
                let (a, b) = ends()?;
                ("beam.set", json!({ "beam_ids": [id], "curve": { "start": a, "end": b } }))
            }
            Kind::Plate => ("plate.set", json!({ "plate_ids": [id], "contour": pts })),
            Kind::Area => ("area_reinforcement.set", json!({ "system_ids": [id], "boundary": pts })),
            Kind::FabricArea => ("fabric_area.set", json!({ "system_ids": [id], "boundary": pts })),
            Kind::Path => ("path_reinforcement.set", json!({ "system_ids": [id], "path": pts })),
        };
        self.run(command, args)?;
        self.refresh_scene(&frame);
        self.properties_stale = true;
        frame.set_status_text(&format!("The {}'s coordinates were changed", kind.name()), 0);
        Ok(())
    }
}

/// A host's outline in its plane, by its GlobalId.
fn outline_of(store: &reviera_core::Store, guid: &str) -> Option<Vec<[f64; 2]>> {
    let id = reviera_core::GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g))?;
    reviera_core::rebar_systems::outline(store, store.get(id)?)
}
